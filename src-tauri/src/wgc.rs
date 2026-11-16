//! A7 on Windows: the screen side of the capture loop. Windows Graphics
//! Capture grabs the primary monitor (our overlay window is left out of it
//! with `WDA_EXCLUDEFROMCAPTURE`, so every frame is clean), the ONNX model
//! finds the balloons, and placements go to the overlay window as events.

use crate::api::{CropIn, PlacementOut};
use crate::detect::{self, Onnx};
use crate::export::{self, Rgb};
use crate::pipeline::{Captured, Event, Screen};
use crate::tracker;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use windows::core::{factory, Interface};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// Frames a second sent to P6.
const FRAME_GAP: Duration = Duration::from_millis(100);
/// Full frames kept for detection and strips of a recent `seq`.
const KEEP: usize = 4;
/// WGC hands over a frame only when the screen changed, and P6 asks for
/// detection and a strip only once the same screen came twice: a still
/// screen is sent again this many times, then every `IDLE_EVERY` ticks.
const STILL_REPEATS: u32 = 3;
const IDLE_EVERY: u32 = 10;

/// The taskbar, in the pixels of a `width` x `height` capture of the
/// primary monitor: what of the monitor is not its work area, at any edge.
/// (An auto-hidden taskbar leaves the work area whole and is not reported.)
fn taskbar(width: u32, height: u32) -> Vec<(u32, u32, u32, u32)> {
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: the primary monitor handle is always valid and `mi` carries its size.
    if !unsafe { GetMonitorInfoW(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY), &mut mi) }.as_bool() {
        return vec![];
    }
    let (m, w) = (mi.rcMonitor, mi.rcWork);
    let (mw, mh) = ((m.right - m.left).max(1) as f64, (m.bottom - m.top).max(1) as f64);
    // The capture is in physical pixels, the monitor rectangle may not be.
    let x = |v: i32| ((v - m.left) as f64 * width as f64 / mw).round() as u32;
    let y = |v: i32| ((v - m.top) as f64 * height as f64 / mh).round() as u32;
    let mut out = vec![];
    if w.top > m.top {
        out.push((0, 0, width, y(w.top)));
    }
    if w.bottom < m.bottom {
        out.push((0, y(w.bottom), width, height.saturating_sub(y(w.bottom))));
    }
    if w.left > m.left {
        out.push((0, 0, x(w.left), height));
    }
    if w.right < m.right {
        out.push((x(w.right), 0, width.saturating_sub(x(w.right)), height));
    }
    out
}

/// A Windows Graphics Capture session on the primary monitor.
struct Wgc {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    staging: Option<(ID3D11Texture2D, u32, u32)>,
    d3d: IDirect3DDevice,
    /// The size the frame pool was made for: the monitor's, when last seen.
    size: SizeInt32,
}

impl Wgc {
    fn new() -> windows::core::Result<Wgc> {
        let mut device = None;
        let mut ctx = None;
        // SAFETY: plain out-parameters for a new hardware device.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )?;
        }
        let missing = || windows::core::Error::from(windows::Win32::Foundation::E_FAIL);
        let device: ID3D11Device = device.ok_or_else(missing)?;
        let ctx = ctx.ok_or_else(missing)?;
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: `dxgi` is the live device created above.
        let d3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? }.cast()?;
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        // SAFETY: the primary monitor handle is valid for the session's lifetime.
        let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY))? };
        let size = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Wgc { device, ctx, pool, session, staging: None, d3d, size })
    }

    /// The newest frame since the last call, as RGB.
    fn grab(&mut self) -> windows::core::Result<Option<Rgb>> {
        let mut newest = None;
        while let Ok(f) = self.pool.TryGetNextFrame() {
            newest = Some(f);
        }
        let Some(frame) = newest else { return Ok(None) };
        let content = frame.ContentSize()?;
        if content != self.size {
            // The monitor turned or changed resolution: the pool is made
            // again in the new size, so frames hold the whole screen in its
            // own coordinates (P6 starts a new picture in that size). This
            // frame, laid out for the old size, is dropped.
            let _ = frame.Close();
            self.pool.Recreate(&self.d3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, content)?;
            self.size = content;
            return Ok(None);
        }
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: the surface of a live frame is a D3D11 texture.
        let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `desc` is a plain out-parameter.
        unsafe { tex.GetDesc(&mut desc) };
        let (w, h) = (desc.Width, desc.Height);
        if !self.staging.as_ref().is_some_and(|s| (s.1, s.2) == (w, h)) {
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut t = None;
            // SAFETY: `desc` describes a CPU-readable copy of the frame.
            unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut t))? };
            self.staging = t.map(|t| (t, w, h));
        }
        let Some((staging, _, _)) = &self.staging else { return Ok(None) };
        let mut map = D3D11_MAPPED_SUBRESOURCE::default();
        let mut px = Vec::with_capacity(w as usize * h as usize * 3);
        // SAFETY: the staging texture has the frame's size and format; rows
        // are read within `RowPitch` while the texture is mapped.
        unsafe {
            self.ctx.CopyResource(staging, &tex);
            self.ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut map))?;
            for y in 0..h as usize {
                let row = std::slice::from_raw_parts((map.pData as *const u8).add(y * map.RowPitch as usize), w as usize * 4);
                for p in row.chunks_exact(4) {
                    px.extend_from_slice(&[p[2], p[1], p[0]]);
                }
            }
            self.ctx.Unmap(staging, 0);
        }
        let _ = frame.Close();
        Ok(Some(Rgb { w, h, px }))
    }
}

impl Drop for Wgc {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Needs a desktop session: `cargo test -p realtime-screen-translator --features app wgc`.
    #[test]
    fn primary_monitor_frame_arrives() {
        let mut cap = Wgc::new().expect("Windows Graphics Capture did not start");
        let deadline = Instant::now() + Duration::from_secs(5);
        let img = loop {
            if let Some(img) = cap.grab().unwrap() {
                break img;
            }
            assert!(Instant::now() < deadline, "no frame in 5 s");
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(img.w > 0 && img.h > 0 && img.px.len() == (img.w * img.h * 3) as usize);
        let thumb = tracker::thumbnail(&img.gray(), img.w as usize, img.h as usize);
        assert_eq!(thumb.len() % tracker::THUMB_W, 0);
    }
}

thread_local! {
    // The capture objects stay on the loop thread that made them.
    static CAPTURE: RefCell<Option<Wgc>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct Inner {
    on: bool,
    seq: u64,
    frames: VecDeque<(u64, Arc<Rgb>)>,
    /// The last frame sent and its thumbnail, sent again while nothing changes.
    last: Option<(Arc<Rgb>, Vec<u8>)>,
    idle: u32,
}

pub struct WinScreen {
    app: AppHandle,
    model: PathBuf,
    inner: Mutex<Inner>,
    wake: Condvar,
    detector: Mutex<Option<Arc<Onnx>>>,
}

impl WinScreen {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> WinScreen {
        let model = crate::models::path(&data_dir);
        WinScreen { app, model, inner: Mutex::default(), wake: Condvar::new(), detector: Mutex::new(None) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Capture runs while translation is on (P4 .. P5).
    pub fn set_on(&self, on: bool) {
        let mut i = self.lock();
        i.on = on;
        if !on {
            i.frames.clear();
            i.last = None;
        }
        self.wake.notify_all();
    }

    fn frame(&self, seq: u64) -> Option<Arc<Rgb>> {
        self.lock().frames.iter().find(|f| f.0 == seq).map(|f| f.1.clone())
    }
}

impl Screen for WinScreen {
    fn next(&self) -> Option<Event> {
        let mut last = Instant::now() - FRAME_GAP;
        loop {
            {
                let mut i = self.lock();
                if !i.on {
                    CAPTURE.with(|c| c.borrow_mut().take());
                    while !i.on {
                        i = self.wake.wait(i).unwrap_or_else(|e| e.into_inner());
                    }
                }
            }
            std::thread::sleep(FRAME_GAP.saturating_sub(last.elapsed()));
            last = Instant::now();
            let grabbed = CAPTURE.with(|c| {
                let mut c = c.borrow_mut();
                if c.is_none() {
                    *c = Some(Wgc::new()?);
                }
                c.as_mut().unwrap().grab()
            });
            let (img, thumb) = match grabbed {
                Ok(Some(img)) => {
                    let thumb = tracker::thumbnail(&img.gray(), img.w as usize, img.h as usize);
                    (Arc::new(img), thumb)
                }
                Ok(None) => {
                    let mut i = self.lock();
                    i.idle += 1;
                    let again = i.idle <= STILL_REPEATS || i.idle % IDLE_EVERY == 0;
                    match i.last.clone() {
                        Some(last) if again => last,
                        _ => continue,
                    }
                }
                Err(e) => {
                    eprintln!("screen capture: {e}");
                    CAPTURE.with(|c| c.borrow_mut().take());
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                }
            };
            let (width, height) = (img.w, img.h);
            let mut i = self.lock();
            if !i.on {
                continue;
            }
            if i.last.as_ref().is_none_or(|l| !Arc::ptr_eq(&l.0, &img)) {
                i.idle = 0;
                i.last = Some((img.clone(), thumb.clone()));
            }
            i.seq += 1;
            let seq = i.seq;
            i.frames.push_back((seq, img));
            while i.frames.len() > KEEP {
                i.frames.pop_front();
            }
            return Some(Event::Frame(Captured { seq, width, height, thumb, captured_at: crate::now_ms(), button: None, bars: taskbar(width, height) }));
        }
    }

    fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        let model = {
            // Loaded on first use, and again later if the file was missing then.
            let mut d = self.detector.lock().unwrap_or_else(|e| e.into_inner());
            if d.is_none() {
                match Onnx::load(&self.model) {
                    Ok(m) => *d = Some(Arc::new(m)),
                    Err(e) => eprintln!("balloon model: {e}"),
                }
            }
            d.clone().ok_or(Some("detector_missing"))?
        };
        let img = self.frame(seq).ok_or(None)?;
        match model.detect(&img) {
            Ok(d) => Ok(detect::crops(&img, &d, top, bottom)),
            Err(e) => {
                eprintln!("balloon model: {e}");
                Err(None)
            }
        }
    }

    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        let img = self.frame(seq)?;
        export::encode_png(&export::rows(&img, top, bottom)).ok()
    }

    fn draw(&self, placements: &[PlacementOut], clear: bool) {
        let _ = self.app.emit_to("overlay", "placements", serde_json::json!({"placements": placements, "clear": clear}));
    }
}
