//! Edge cases of the sixth review: a start whose book cannot be made, a stop
//! racing a start, exports racing each other, corner rounding over the
//! source text, and a blocked capture with bright system pixels in it.

mod support;

use realtime_screen_translator::api::{ExportIn, FrameIn, FrameOut, KeyIn, StripIn};
use realtime_screen_translator::export;
use realtime_screen_translator::layout;
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::store::PlacementRow;
use realtime_screen_translator::tracker;
use realtime_screen_translator::Backend;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use support::*;

#[derive(Default)]
struct Device {
    export: PathBuf,
    stopped: AtomicBool,
    stop_ms: AtomicU64,
}

impl Platform for Device {
    fn request_permissions(&self) -> Permissions {
        self.permissions()
    }
    fn permissions(&self) -> Permissions {
        Permissions { capture: true, overlay: true }
    }
    fn start(&self) -> Result<(), String> {
        self.stopped.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) {
        // A slow teardown (closing the overlay window, the capture session).
        std::thread::sleep(Duration::from_millis(self.stop_ms.load(Ordering::SeqCst)));
        self.stopped.store(true, Ordering::SeqCst);
    }
    fn emit(&self, _: &str, _: serde_json::Value) {}
    fn export_dir(&self) -> PathBuf {
        self.export.clone()
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}

fn open(dirs: &Dirs, gemini_url: &str) -> (Backend, Arc<Device>) {
    let device = Arc::new(Device { export: dirs.export.clone(), ..Default::default() });
    let backend = Backend::open(dirs.data.clone(), Some(gemini_url.to_string()), device.clone()).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    (backend, device)
}

fn send(backend: &Backend, seq: u64, thumb: Vec<u8>) -> FrameOut {
    let f = FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: seq as i64 * 100 };
    backend.frame(f).unwrap()
}

#[test]
fn a_start_whose_book_cannot_be_made_takes_capture_and_overlay_down() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let (backend, device) = open(&dirs, &gemini.url);
    let other = rusqlite::Connection::open(dirs.data.join("translator.sqlite3")).unwrap();
    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(backend.start().is_err(), "a book was made through a locked database");
    assert!(device.stopped.load(Ordering::SeqCst), "capture and the overlay stayed up with no session");
    assert!(!backend.status().on);
    other.execute_batch("ROLLBACK").unwrap();
    backend.start().expect("start after the lock went");
    assert!(!device.stopped.load(Ordering::SeqCst));
}

#[test]
fn a_stop_still_tearing_down_does_not_take_down_the_next_start() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let (backend, device) = open(&dirs, &gemini.url);
    backend.start().unwrap();
    device.stop_ms.store(400, Ordering::SeqCst);
    std::thread::scope(|s| {
        s.spawn(|| backend.stop().unwrap());
        std::thread::sleep(Duration::from_millis(100));
        backend.start().expect("start after stop");
    });
    assert!(backend.status().on);
    assert!(!device.stopped.load(Ordering::SeqCst), "the earlier stop took down the new session's capture and overlay");
}

#[test]
fn exports_at_the_same_time_write_separate_files() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let (backend, _device) = open(&dirs, &gemini.url);
    backend.start().unwrap();
    let scene = Scene::new(601);
    send(&backend, 1, scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = send(&backend, 2, scene.thumb(0, SCREEN_W, SCREEN_H));
    let r = f.save.expect("P6 asked for no strip");
    let image = scene.screen_png(0, SCREEN_W, SCREEN_H, r.top, r.bottom);
    backend.strip(StripIn { picture_id: f.picture_id, frame_seq: 2, top: r.top, bottom: r.bottom, image }).unwrap();
    let book_id = backend.stop().unwrap().book_id.unwrap();

    let files: Vec<String> = std::thread::scope(|s| {
        let runs: Vec<_> = (0..8)
            .map(|_| s.spawn(|| backend.export(ExportIn { book_id, format: "png".into() }).unwrap().files))
            .collect();
        runs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(files.iter().collect::<HashSet<_>>().len(), 8, "two exports shared a file: {files:?}");
    for f in &files {
        let img = decode_png(&std::fs::read(f).unwrap());
        assert_eq!((img.w, img.h), (SCREEN_W, 2400), "{f} was written over");
    }
}

#[test]
fn the_rounded_shape_covers_the_source_text_to_its_corners() {
    let dir = Dirs::new().export;
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("black.png");
    std::fs::write(&path, png_rgb(600, 1200, |_, _| [0, 0, 0])).unwrap();
    let strip = realtime_screen_translator::store::StripRow { y_start: 0, height: 1200, file_path: path.to_string_lossy().into() };
    for (w, h) in [(120, 200), (300, 300), (60, 40)] {
        let s = layout::fit(400 - 200, 900 - 200, w, h, "안녕");
        let p = PlacementRow { id: 1, fingerprint: String::new(), x: s.x, y: s.y, w: s.w, h: s.h, radius: s.radius, font_px: s.font_px, ko: String::new() };
        let img = export::render(600, 0, 1200, &[strip.clone()], &[p]).unwrap();
        for y in [700, 700 + h as usize - 1] {
            for x in [200, 200 + w as usize - 1] {
                let v = img.px[(y * 600 + x) * 3];
                assert_eq!(v, 255, "source pixel ({x}, {y}) of a {w}x{h} text area shows past the corner of {s:?}");
            }
        }
    }
}

#[test]
fn a_black_capture_with_the_status_bar_and_the_navigation_bar_in_it_is_blocked() {
    // (The floating button is blacked out by the capture loop before P6:
    // `blocked_capture_edges` covers it.)
    let rows = (SCREEN_H * THUMB_W / SCREEN_W) as usize;
    let w = THUMB_W as usize;
    let mut thumb = vec![0u8; w * rows];
    // Status bar text and icons, the navigation bar, a cursor.
    for r in 0..3 {
        for c in 0..w {
            thumb[r * w + c] = if c % 3 == 0 { 230 } else { 0 };
        }
    }
    for c in 0..w {
        thumb[(rows - 2) * w + c] = 40;
    }
    thumb[(rows / 2) * w + 40] = 255;
    assert!(tracker::is_blocked(&thumb), "bright system pixels made a blocked capture a picture");
    assert!(!tracker::is_blocked(&Scene::new(602).thumb(0, SCREEN_W, SCREEN_H)));
}
