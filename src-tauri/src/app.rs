//! The Tauri app shell: every backend command registered under its A1 name
//! (`POST /frames` is `post_frames`), argument names as in `api.yaml`.
//!
//! Platform layer. Windows (A7): nothing to ask for, the overlay is a
//! transparent, always-on-top, click-through window left out of screen
//! capture, `wgc::WinScreen` captures and detects, DPAPI seals the key and
//! exports go to the Downloads folder. Android: the Kotlin `ScreenPlugin`
//! asks for capture and draw-over-apps, captures, detects, draws the overlay
//! and the floating on/off button, and seals the key with the Keystore.
//! Either way `pipeline::run` drives P6, P7 and P9 from the captured frames.

use crate::api::*;
use crate::pipeline;
use crate::platform::{Permissions, Platform};
use crate::Backend;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppPlatform {
    app: AppHandle,
    data_dir: PathBuf,
    #[cfg(windows)]
    screen: Arc<crate::wgc::WinScreen>,
}

const OVERLAY: &str = "overlay";

/// Windows (P4, R29): the on/off button over other apps, a small
/// always-on-top window near the bottom right edge of the work area, left
/// out of capture like the overlay. Its page is inline, and a click is a
/// navigation to `rst-toggle:` that switches translation instead of loading.
#[cfg(windows)]
mod button {
    use super::*;

    pub const LABEL: &str = "onoff";
    const HTML: &str = "<!doctype html><meta charset=utf-8><body style='margin:0;overflow:hidden;background:transparent'>        <a id=b href='rst-toggle:tap' style='display:block;box-sizing:border-box;height:100vh;line-height:100vh;border-radius:24px;        background:rgba(34,34,34,.85);color:white;font:600 14px sans-serif;text-align:center;text-decoration:none;user-select:none'>번역 켜기</a>";

    pub fn show(app: &AppHandle) -> Result<(), String> {
        if app.get_webview_window(LABEL).is_some() {
            return Ok(());
        }
        let url: tauri::Url = format!("data:text/html;charset=utf-8,{}", HTML.replace(' ', "%20")).parse().map_err(|e| format!("{e}"))?;
        let handle = app.clone();
        let w = tauri::WebviewWindowBuilder::new(app, LABEL, tauri::WebviewUrl::External(url))
            .inner_size(112.0, 44.0)
            .transparent(true)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .visible(false)
            .on_navigation(move |url| {
                if url.scheme() != "rst-toggle" {
                    return true;
                }
                // Off the UI thread: start and stop build and close windows.
                let h = handle.clone();
                std::thread::spawn(move || pipeline::toggle(&h.state::<Backend>()));
                false
            })
            .build()
            .map_err(|e| e.to_string())?;
        if let Ok(hwnd) = w.hwnd() {
            use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
            // SAFETY: the handle belongs to the window built above.
            unsafe { SetWindowDisplayAffinity(hwnd.0 as _, WDA_EXCLUDEFROMCAPTURE) };
        }
        if let (Ok(Some(m)), Ok(size)) = (w.primary_monitor(), w.outer_size()) {
            let area = m.work_area();
            let gap = (24.0 * m.scale_factor()) as i32;
            let x = area.position.x + area.size.width as i32 - size.width as i32 - gap;
            let y = area.position.y + area.size.height as i32 - size.height as i32 - gap;
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
        }
        w.show().map_err(|e| e.to_string())
    }

    pub fn label(app: &AppHandle, on: bool) {
        if let Some(w) = app.get_webview_window(LABEL) {
            let text = if on { "번역 끄기" } else { "번역 켜기" };
            let _ = w.eval(format!("document.getElementById('b').textContent='{text}'"));
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use super::*;
    use base64::Engine;
    use serde::Deserialize;
    use serde_json::{json, Value};
    use tauri_plugin_rst_screen::Screen as Plugin;

    pub fn call<T: serde::de::DeserializeOwned>(app: &AppHandle, command: &str, args: Value) -> Result<T, String> {
        app.state::<Plugin<tauri::Wry>>().call(command, args)
    }

    pub fn b64(s: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD.decode(s).unwrap_or_default()
    }

    pub fn enc(b: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(b)
    }

    #[derive(Deserialize)]
    pub struct Perms {
        pub capture: bool,
        pub overlay: bool,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Next {
        kind: String,
        #[serde(default)]
        seq: u64,
        #[serde(default)]
        width: u32,
        #[serde(default)]
        height: u32,
        #[serde(default)]
        thumb: String,
        #[serde(default)]
        captured_at: i64,
        /// The floating button's [x, y, w, h] in the frame (the capture holds it).
        #[serde(default)]
        button: Option<[u32; 4]>,
    }

    #[derive(Deserialize)]
    struct Crop {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        kind: String,
        score: f32,
        image: String,
    }

    #[derive(Deserialize)]
    struct Detected {
        #[serde(default)]
        missing: bool,
        #[serde(default)]
        failed: bool,
        crops: Vec<Crop>,
    }

    #[derive(Deserialize)]
    pub struct Published {
        pub location: String,
    }

    #[derive(Deserialize)]
    pub struct Bytes {
        pub data: Option<String>,
    }

    /// The Kotlin plugin as the capture loop's screen side; `.1` is where
    /// [`crate::models`] keeps the balloon model.
    pub struct AndroidScreen(pub AppHandle, pub String);

    impl pipeline::Screen for AndroidScreen {
        fn next(&self) -> Option<pipeline::Event> {
            loop {
                match call::<Next>(&self.0, "next", json!({})) {
                    Ok(n) if n.kind == "toggle" => return Some(pipeline::Event::Toggle),
                    Ok(n) => {
                        return Some(pipeline::Event::Frame(pipeline::Captured {
                            seq: n.seq,
                            width: n.width,
                            height: n.height,
                            thumb: b64(&n.thumb),
                            captured_at: n.captured_at,
                            button: n.button.map(|b| (b[0], b[1], b[2], b[3])),
                        }))
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            }
        }

        fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
            let d = call::<Detected>(&self.0, "detect", json!({"seq": seq, "top": top, "bottom": bottom, "model": self.1}))
                .map_err(|e| eprintln!("{e}"))
                .map_err(|_| None)?;
            if d.missing {
                return Err(Some("detector_missing"));
            }
            if d.failed {
                return Err(None);
            }
            Ok(d.crops
                .into_iter()
                .map(|c| CropIn { x: c.x, y: c.y, w: c.w, h: c.h, kind: c.kind, score: c.score, image: b64(&c.image) })
                .collect())
        }

        fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
            let b = call::<Bytes>(&self.0, "strip", json!({"seq": seq, "top": top, "bottom": bottom})).map_err(|e| eprintln!("{e}")).ok()?;
            b.data.map(|d| b64(&d))
        }

        fn draw(&self, placements: &[PlacementOut], clear: bool) {
            if let Err(e) = call::<Value>(&self.0, "draw", json!({"placements": placements, "clear": clear})) {
                eprintln!("{e}");
            }
        }
    }
}

impl Platform for AppPlatform {
    fn request_permissions(&self) -> Permissions {
        #[cfg(target_os = "android")]
        {
            // R29: capture first, then drawing over other apps; the plugin
            // puts the floating on/off button up once both are granted.
            return match android::call::<android::Perms>(&self.app, "requestPermissions", serde_json::json!({})) {
                Ok(p) => Permissions { capture: p.capture, overlay: p.overlay },
                Err(e) => {
                    eprintln!("{e}");
                    Permissions::default()
                }
            };
        }
        #[cfg(not(target_os = "android"))]
        self.permissions()
    }

    fn permissions(&self) -> Permissions {
        #[cfg(target_os = "android")]
        {
            let p = android::call::<android::Perms>(&self.app, "permissions", serde_json::json!({}));
            return p.map(|p| Permissions { capture: p.capture, overlay: p.overlay }).unwrap_or_default();
        }
        // Windows has no capture or draw-over-apps prompt; other desktops have no capture.
        #[cfg(not(target_os = "android"))]
        Permissions { capture: cfg!(windows), overlay: cfg!(windows) }
    }

    fn start(&self) -> Result<(), String> {
        #[cfg(target_os = "android")]
        return android::call::<serde_json::Value>(&self.app, "start", serde_json::json!({})).map(|_| ());
        #[cfg(desktop)]
        {
            if self.app.get_webview_window(OVERLAY).is_none() {
                let w = tauri::WebviewWindowBuilder::new(&self.app, OVERLAY, tauri::WebviewUrl::App("index.html?overlay".into()))
                    .transparent(true)
                    .decorations(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .focused(false)
                    .shadow(false)
                    .maximized(true)
                    .build()
                    .map_err(|e| e.to_string())?;
                w.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
                #[cfg(windows)]
                if let Ok(hwnd) = w.hwnd() {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
                    // SAFETY: the handle belongs to the window built above.
                    unsafe { SetWindowDisplayAffinity(hwnd.0 as _, WDA_EXCLUDEFROMCAPTURE) };
                }
            }
            #[cfg(windows)]
            {
                self.screen.set_on(true);
                button::label(&self.app, true);
            }
        }
        #[cfg(not(target_os = "android"))]
        Ok(())
    }

    fn stop(&self) {
        #[cfg(target_os = "android")]
        if let Err(e) = android::call::<serde_json::Value>(&self.app, "stop", serde_json::json!({})) {
            eprintln!("{e}");
        }
        #[cfg(windows)]
        {
            self.screen.set_on(false);
            button::label(&self.app, false);
        }
        if let Some(w) = self.app.get_webview_window(OVERLAY) {
            let _ = w.close();
        }
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        // A translation that came back late is drawn at once (R28).
        #[cfg(target_os = "android")]
        if event == "placements" {
            let _ = android::call::<serde_json::Value>(&self.app, "draw", serde_json::json!({"placements": payload["placements"], "clear": false}));
        }
        let _ = self.app.emit(event, payload);
    }

    fn export_dir(&self) -> PathBuf {
        // Android: written to the cache, then handed to the gallery by `publish`.
        #[cfg(target_os = "android")]
        return self.app.path().app_cache_dir().unwrap_or_else(|_| self.data_dir.join("cache")).join("export");
        #[cfg(not(target_os = "android"))]
        self.app.path().download_dir().unwrap_or_else(|_| self.data_dir.join("export"))
    }

    #[cfg(target_os = "android")]
    fn publish(&self, path: &std::path::Path) -> Result<String, String> {
        android::call::<android::Published>(&self.app, "publish", serde_json::json!({"path": path.to_string_lossy()})).map(|p| p.location)
    }

    // DPAPI on Windows, the Android Keystore through the plugin.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        #[cfg(target_os = "android")]
        {
            let r = android::call::<android::Bytes>(&self.app, "seal", serde_json::json!({"data": android::enc(data)}));
            return r.map_err(|e| eprintln!("{e}")).ok()?.data.map(|d| android::b64(&d));
        }
        #[cfg(not(target_os = "android"))]
        crate::vault::seal(data)
    }

    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        #[cfg(target_os = "android")]
        {
            let r = android::call::<android::Bytes>(&self.app, "unseal", serde_json::json!({"data": android::enc(data)}));
            return r.map_err(|e| eprintln!("{e}")).ok()?.data.map(|d| android::b64(&d));
        }
        #[cfg(not(target_os = "android"))]
        crate::vault::unseal(data)
    }
}

type B<'a> = State<'a, Backend>;

#[tauri::command(async, rename_all = "snake_case")]
fn get_status(b: B<'_>) -> Status {
    b.status()
}

#[tauri::command(async, rename_all = "snake_case")]
fn put_settings_gemini_key(b: B<'_>, key: String) -> ApiResult<KeyOut> {
    b.put_key(KeyIn { key })
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_permissions_request(b: B<'_>) -> PermissionsOut {
    b.request_permissions()
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_translation_start(b: B<'_>) -> ApiResult<StartOut> {
    b.start()
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_translation_stop(b: B<'_>) -> ApiResult<StopOut> {
    b.stop()
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_frames(b: B<'_>, seq: u64, width: u32, height: u32, thumb: Vec<u8>, captured_at: i64) -> ApiResult<FrameOut> {
    b.frame(FrameIn { seq, width, height, thumb, captured_at })
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_crops(b: B<'_>, picture_id: i64, frame_seq: u64, crops: Vec<CropIn>) -> ApiResult<CropsOut> {
    b.crops(CropsIn { picture_id, frame_seq, crops })
}

#[tauri::command(async, rename_all = "snake_case")]
fn get_placements(b: B<'_>, since: Option<u64>) -> ApiResult<PlacementsOut> {
    b.placements(PlacementsIn { since })
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_strips(b: B<'_>, picture_id: i64, frame_seq: u64, top: u32, bottom: u32, image: Vec<u8>) -> ApiResult<StripOut> {
    b.strip(StripIn { picture_id, frame_seq, top, bottom, image })
}

#[tauri::command(async, rename_all = "snake_case")]
fn get_books(b: B<'_>, limit: Option<u32>, before: Option<i64>) -> ApiResult<BooksOut> {
    b.books(BooksIn { limit: limit.unwrap_or(20), before })
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_books_export(b: B<'_>, book_id: i64, format: Option<String>) -> ApiResult<ExportOut> {
    b.export(ExportIn { book_id, format: format.unwrap_or_else(|| "png".into()) })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_rst_screen::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let handle = app.handle().clone();
            // R25: the pinned balloon model, fetched once and checked.
            crate::models::fetch_in_background(crate::models::path(&data_dir));
            #[cfg(windows)]
            let screen = Arc::new(crate::wgc::WinScreen::new(handle.clone(), data_dir.clone()));
            let platform = Arc::new(AppPlatform {
                app: handle.clone(),
                data_dir: data_dir.clone(),
                #[cfg(windows)]
                screen: screen.clone(),
            });
            #[cfg(target_os = "android")]
            let model = crate::models::path(&data_dir);
            let backend = Backend::open(data_dir, std::env::var("RST_GEMINI_BASE_URL").ok(), platform)?;
            app.manage(backend);
            // Nothing to ask for on Windows (R29): the button is up from the start.
            #[cfg(windows)]
            if let Err(e) = button::show(&handle) {
                eprintln!("on/off button: {e}");
            }
            // The capture loop: frames, detection and strips while translation is on.
            #[cfg(any(windows, target_os = "android"))]
            std::thread::spawn(move || {
                let backend = handle.state::<Backend>();
                #[cfg(windows)]
                pipeline::run(&backend, &*screen);
                #[cfg(target_os = "android")]
                pipeline::run(&backend, &android::AndroidScreen(handle.clone(), model.to_string_lossy().into_owned()));
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            put_settings_gemini_key,
            post_permissions_request,
            post_translation_start,
            post_translation_stop,
            post_frames,
            post_crops,
            get_placements,
            post_strips,
            get_books,
            post_books_export
        ])
        .run(tauri::generate_context!())
        .expect("the Tauri app failed to run");
}
