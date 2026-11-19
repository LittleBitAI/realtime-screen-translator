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

/// P12: the reader's page script, run on every page the app's windows load
/// (it leaves the app's own screens alone).
const READER_JS: &str = include_str!("reader.js");
/// The PC's reader window; Android has one window, which the site replaces.
#[cfg(desktop)]
const SITE: &str = "site";

/// The app's own address, where the reader goes back to on Android.
#[cfg(mobile)]
fn home() -> std::sync::MutexGuard<'static, Option<tauri::Url>> {
    static HOME: std::sync::Mutex<Option<tauri::Url>> = std::sync::Mutex::new(None);
    HOME.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where the app's windows may go: web pages and the app's own, never
/// another app (`intent:`, `market:`, a shop's own scheme), which an ad on a
/// manga site redirects to.
fn may_navigate(url: &tauri::Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "tauri" | "asset" | "about")
}

/// The script that drew the overlay last, run again once its page loads.
#[cfg(desktop)]
fn last_drawn() -> std::sync::MutexGuard<'static, String> {
    static LAST: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
    LAST.lock().unwrap_or_else(|e| e.into_inner())
}

/// A7, R28: the Windows overlay shows exactly `placements` (a JSON array of
/// `PlacementOut`, screen pixels) as white shapes with the Korean in them.
#[cfg(desktop)]
pub(crate) fn draw_overlay(app: &AppHandle, placements: &serde_json::Value) {
    let script = crate::overlay::script(placements);
    *last_drawn() = script.clone();
    if let Some(w) = app.get_webview_window(OVERLAY) {
        // The monitor may have changed resolution or turned since (the
        // capture follows it, `wgc::Wgc::grab`): the overlay follows too.
        if let Err(e) = fit_overlay(&w) {
            eprintln!("overlay: {e}");
        }
        if let Err(e) = w.eval(script) {
            eprintln!("overlay: {e}");
        }
    }
}

/// Lays the overlay window over the whole primary monitor, the one
/// captured, unless it is there already: a placement's screen pixels are
/// the window's own (the page divides them by the scale factor). Not
/// maximized, which would leave out the taskbar and move the origin with it.
#[cfg(desktop)]
fn fit_overlay(w: &tauri::WebviewWindow) -> Result<(), String> {
    let Some(m) = w.primary_monitor().map_err(|e| e.to_string())? else { return Ok(()) };
    if w.outer_position().ok() != Some(*m.position()) {
        w.set_position(*m.position()).map_err(|e| e.to_string())?;
    }
    if w.outer_size().ok() != Some(*m.size()) {
        w.set_size(*m.size()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Windows (P4): the label of the on/off button window, should one be up.
/// The reader (P12) replaced screen capture, so the app no longer shows it.
#[cfg(windows)]
mod button {
    use super::*;

    pub const LABEL: &str = "onoff";

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
        /// The reader touched the screen a while before this frame (`Capture.touched`).
        #[serde(default)]
        touched: bool,
        /// The status bar and the navigation bar as [x, y, w, h], while they show.
        #[serde(default)]
        bars: Vec<[u32; 4]>,
    }

    fn rects(r: &[[u32; 4]]) -> Vec<(u32, u32, u32, u32)> {
        r.iter().map(|b| (b[0], b[1], b[2], b[3])).collect()
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
    pub struct AndroidScreen(pub AppHandle, pub String, pub std::sync::atomic::AtomicBool);

    impl pipeline::Screen for AndroidScreen {
        fn next(&self) -> Option<pipeline::Event> {
            loop {
                match call::<Next>(&self.0, "next", json!({})) {
                    Ok(n) if n.kind == "toggle" => return Some(pipeline::Event::Toggle),
                    Ok(n) if n.kind == "lost" => return Some(pipeline::Event::Lost),
                    Ok(n) => {
                        if n.touched {
                            self.2.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        return Some(pipeline::Event::Frame(pipeline::Captured {
                            seq: n.seq,
                            width: n.width,
                            height: n.height,
                            thumb: b64(&n.thumb),
                            captured_at: n.captured_at,
                            button: n.button.map(|b| (b[0], b[1], b[2], b[3])),
                            bars: rects(&n.bars),
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

        fn touched(&self) -> bool {
            self.2.swap(false, std::sync::atomic::Ordering::Relaxed)
        }

        fn peek(&self) -> Option<pipeline::Captured> {
            taken(call::<Next>(&self.0, "peekFrame", json!({})))
        }

        fn clean_frame(&self) -> Option<pipeline::Captured> {
            taken(call::<Next>(&self.0, "cleanFrame", json!({})))
        }
    }

    /// The frame a screen command took for the loop, or none.
    fn taken(n: Result<Next, String>) -> Option<pipeline::Captured> {
        let n = n.map_err(|e| eprintln!("{e}")).ok()?;
        (n.kind == "frame").then(|| pipeline::Captured {
            seq: n.seq,
            width: n.width,
            height: n.height,
            thumb: b64(&n.thumb),
            captured_at: n.captured_at,
            button: n.button.map(|b| (b[0], b[1], b[2], b[3])),
            bars: rects(&n.bars),
        })
    }
}

impl Platform for AppPlatform {
    fn overlay_in_capture(&self) -> bool {
        // Windows: the overlay window is excluded from capture (WDA_EXCLUDEFROMCAPTURE).
        cfg!(target_os = "android")
    }

    fn can_capture(&self) -> bool {
        // Android (MediaProjection) and Windows (Graphics Capture) only; on
        // other desktops P4 says capture is unsupported, not that a permission is missing.
        cfg!(any(windows, target_os = "android"))
    }

    fn request_permissions(&self) -> Permissions {
        #[cfg(target_os = "android")]
        {
            // R29: capture first, then drawing over other apps; the plugin
            // puts the floating on/off button up once both are granted.
            return match android::call::<android::Perms>(&self.app, "askCapture", serde_json::json!({})) {
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
                // A7: the overlay's own inline page (`overlay::HTML`), drawn on by
                // `draw_overlay`; once loaded it shows what was drawn meanwhile.
                let url: tauri::Url = crate::overlay::data_url().parse().map_err(|e| format!("{e}"))?;
                let w = tauri::WebviewWindowBuilder::new(&self.app, OVERLAY, tauri::WebviewUrl::External(url))
                    .transparent(true)
                    .decorations(false)
                    .resizable(false)
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .focused(false)
                    .shadow(false)
                    .visible(false)
                    .on_page_load(|w, p| {
                        if p.event() == tauri::webview::PageLoadEvent::Finished {
                            let _ = w.eval(last_drawn().clone());
                        }
                    })
                    .build()
                    .map_err(|e| e.to_string())?;
                w.set_ignore_cursor_events(true).map_err(|e| e.to_string())?;
                fit_overlay(&w)?;
                #[cfg(windows)]
                if let Ok(hwnd) = w.hwnd() {
                    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE};
                    // SAFETY: the handle belongs to the window built above.
                    unsafe { SetWindowDisplayAffinity(hwnd.0 as _, WDA_EXCLUDEFROMCAPTURE) };
                }
                w.show().map_err(|e| e.to_string())?;
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
        #[cfg(desktop)]
        {
            *last_drawn() = crate::overlay::script(&serde_json::json!([]));
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
        #[cfg(desktop)]
        if event == "placements" {
            draw_overlay(&self.app, &payload["placements"]);
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
fn get_books(b: B<'_>, limit: Option<u32>, before: Option<i64>, list: Option<i64>, search: Option<String>) -> ApiResult<BooksOut> {
    b.browse(BooksIn { limit: limit.unwrap_or(20), before }, list, search.as_deref().unwrap_or_default())
}

#[tauri::command(async, rename_all = "snake_case")]
fn get_lists(b: B<'_>) -> ApiResult<Vec<ReadingList>> {
    b.lists()
}

#[tauri::command(async, rename_all = "snake_case")]
fn edit_library(b: B<'_>, edit: LibraryEdit) -> ApiResult<()> {
    b.edit_library(edit)
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_books_export(b: B<'_>, book_id: i64, format: Option<String>) -> ApiResult<ExportOut> {
    b.export(ExportIn { book_id, format: format.unwrap_or_else(|| "png".into()) })
}

/// A book's preview (`preview_path` of P10, a strip in the app's own
/// folder) as a data URL for the library screen: the web host serves the
/// same file at `/files/<preview_path>`.
#[tauri::command(async, rename_all = "snake_case")]
fn get_preview(app: AppHandle, path: String) -> ApiResult<String> {
    use base64::Engine;
    let rel = std::path::Path::new(&path);
    let plain = rel.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if !plain || !rel.starts_with("strips") {
        return Err(ApiError("not_found"));
    }
    let dir = app.path().app_data_dir().map_err(|_| ApiError("internal"))?;
    let bytes = std::fs::read(dir.join(rel)).map_err(|_| ApiError("not_found"))?;
    // Reader pages are kept as the site sent them (JPEG too).
    let mime = crate::export::mime(&bytes).unwrap_or("image/webp");
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command(async, rename_all = "snake_case")]
fn put_settings_source_lang(b: B<'_>, lang: String) -> ApiResult<LangIn> {
    b.put_source_lang(LangIn { lang })
}

/// P12: the site at `url` in the reader. The PC opens it in a window of its
/// own; Android shows it in the app's one window, the phone's Back or the
/// page's 앱 button coming back.
#[tauri::command(async, rename_all = "snake_case")]
fn post_reader_open(app: AppHandle, url: String) -> ApiResult<()> {
    let url = site_url(&url)?;
    let fail = |e: tauri::Error| {
        eprintln!("reader: {e}");
        ApiError("internal")
    };
    #[cfg(desktop)]
    {
        if let Some(w) = app.get_webview_window(SITE) {
            w.navigate(url).map_err(fail)?;
            let _ = w.show();
            let _ = w.set_focus();
            return Ok(());
        }
        tauri::WebviewWindowBuilder::new(&app, SITE, tauri::WebviewUrl::External(url))
            .initialization_script(READER_JS)
            .on_navigation(may_navigate)
            .title("만화 읽기")
            .inner_size(1100.0, 900.0)
            .build()
            .map_err(fail)?;
    }
    #[cfg(mobile)]
    {
        let w = app.get_webview_window("main").ok_or(ApiError("internal"))?;
        *home() = Some(w.url().map_err(fail)?);
        w.navigate(url).map_err(fail)?;
    }
    Ok(())
}

/// The page's 앱 button: back to the app's screens.
#[tauri::command(async, rename_all = "snake_case")]
fn post_reader_close(app: AppHandle) -> ApiResult<()> {
    #[cfg(desktop)]
    if let Some(w) = app.get_webview_window(SITE) {
        let _ = w.close();
    }
    #[cfg(mobile)]
    {
        let w = app.get_webview_window("main").ok_or(ApiError("internal"))?;
        let home = home().clone().ok_or(ApiError("internal"))?;
        w.navigate(home).map_err(|_| ApiError("internal"))?;
    }
    Ok(())
}

/// P12: one page picture of the site in the reader (`image` in base64, or
/// none for the app to fetch `src` itself).
#[tauri::command(async, rename_all = "snake_case")]
fn post_reader_page(b: B<'_>, src: String, image: Option<String>, page_url: String, title: Option<String>, index: Option<i64>) -> ApiResult<PageOut> {
    use base64::Engine;
    let image = match image {
        Some(s) => Some(base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| ApiError("bad_image"))?),
        None => None,
    };
    b.read_page(PageIn { src, image, page_url, title: title.unwrap_or_default(), index: index.unwrap_or(0) })
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_chapter_import(b: B<'_>, window: tauri::WebviewWindow) -> ApiResult<Option<i64>> {
    #[cfg(windows)]
    {
        let folder = pick_chapter_folder(&window).map_err(|e| {
            eprintln!("folder picker: {e}");
            ApiError("folder_picker_failed")
        })?;
        folder.map(|path| b.import_folder(&path)).transpose()
    }
    #[cfg(target_os = "android")]
    {
        #[derive(serde::Deserialize)]
        struct Folder {
            title: String,
            pages: Vec<Source>,
        }
        #[derive(serde::Deserialize)]
        struct Source {
            name: String,
            uri: String,
        }
        #[derive(serde::Deserialize)]
        struct CopyResult {
            error: Option<String>,
        }
        let app = window.app_handle();
        let folder: Option<Folder> = android::call::<Option<Folder>>(app, "pickChapterFolder", serde_json::json!({}))
            .map_err(|e| {
                eprintln!("folder picker: {e}");
                ApiError("folder_picker_failed")
            })?;
        let Some(folder) = folder else { return Ok(None) };
        let pages = folder.pages.into_iter().map(|p| (p.name, p.uri)).collect();
        b.import_chapter(&folder.title, pages, |uri, path| {
            let copy: CopyResult = android::call(app, "copyChapterPage", serde_json::json!({
                "uri": uri, "path": path, "maxBytes": crate::backend::MAX_PAGE_BYTES,
            })).map_err(|e| {
                eprintln!("chapter copy: {e}");
                ApiError("image_unreachable")
            })?;
            match copy.error.as_deref() {
                None => Ok(()),
                Some("storage_full") => Err(ApiError("storage_full")),
                Some(_) => Err(ApiError("image_unreachable")),
            }
        }).map(Some)
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = (b, window);
        Err(ApiError("folder_picker_failed"))
    }
}

#[cfg(windows)]
fn pick_chapter_folder(window: &tauri::WebviewWindow) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::{
        Foundation::{ERROR_CANCELLED, HWND},
        System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
        UI::Shell::{FileOpenDialog, IFileOpenDialog, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, SIGDN_FILESYSPATH},
    };

    let owner = HWND(window.hwnd()?.0);
    // SAFETY: the dialog and its Shell item are used and released on this COM thread.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let picked = (|| -> windows::core::Result<Option<PathBuf>> {
            let dialog: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)?;
            if let Err(e) = dialog.Show(Some(owner)) {
                if e.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) {
                    return Ok(None);
                }
                return Err(e);
            }
            let path = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let result = PathBuf::from(std::ffi::OsString::from_wide(path.as_wide()));
            CoTaskMemFree(Some(path.0.cast()));
            Ok(Some(result))
        })();
        CoUninitialize();
        Ok(picked?)
    }
}

#[tauri::command(async, rename_all = "snake_case")]
fn get_chapter(b: B<'_>, book_id: i64) -> ApiResult<ChapterOut> {
    b.chapter(book_id)
}

#[tauri::command(async, rename_all = "snake_case")]
fn get_chapter_image(b: B<'_>, picture_id: i64) -> ApiResult<tauri::ipc::Response> {
    b.chapter_image(picture_id).map(tauri::ipc::Response::new)
}

#[tauri::command(async, rename_all = "snake_case")]
fn post_chapter_page(b: B<'_>, picture_id: i64) -> ApiResult<PageOut> {
    b.read_chapter_page(picture_id)
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
            // A phone has no environment to set: a development build may
            // carry the address (the request ledger, over `adb reverse`).
            let gemini_base = std::env::var("RST_GEMINI_BASE_URL").ok().or(option_env!("RST_GEMINI_BASE_URL").map(String::from));
            let backend = Backend::open(data_dir, gemini_base, platform)?;
            app.manage(backend);
            // Built here rather than in tauri.conf.json, so it carries the
            // reader's script: on Android the sites open in this window.
            let main = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
                .initialization_script(READER_JS)
                .on_navigation(may_navigate);
            #[cfg(desktop)]
            let main = main.title("실시간 화면 번역").inner_size(1100.0, 760.0);
            main.build()?;
            // The capture loop: frames, detection and strips while translation is on.
            #[cfg(any(windows, target_os = "android"))]
            std::thread::spawn(move || {
                let backend = handle.state::<Backend>();
                #[cfg(windows)]
                pipeline::run(&backend, &*screen);
                #[cfg(target_os = "android")]
                pipeline::run(&backend, &android::AndroidScreen(handle.clone(), model.to_string_lossy().into_owned(), Default::default()));
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
            get_lists,
            edit_library,
            post_books_export,
            get_preview,
            put_settings_source_lang,
            post_reader_open,
            post_reader_close,
            post_reader_page,
            post_chapter_import,
            get_chapter,
            get_chapter_image,
            post_chapter_page
        ])
        .run(tauri::generate_context!())
        .expect("the Tauri app failed to run");
}
