//! The Tauri app shell: every backend command registered under its A1 name
//! (`POST /frames` is `post_frames`), argument names as in `api.yaml`.
//!
//! Desktop platform layer: nothing to ask for on Windows, the overlay is a
//! transparent, always-on-top, click-through window left out of screen
//! capture, and exports go to the Downloads folder. Screen capture (Windows
//! Graphics Capture) and balloon detection (ONNX) feed `post_frames` and
//! `post_crops`; on Android the Kotlin plugin does the same.

use crate::api::*;
use crate::platform::{Permissions, Platform};
use crate::{vault, Backend};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

struct AppPlatform {
    app: AppHandle,
    data_dir: PathBuf,
}

const OVERLAY: &str = "overlay";

impl Platform for AppPlatform {
    fn request_permissions(&self) -> Permissions {
        self.permissions()
    }

    fn permissions(&self) -> Permissions {
        // Windows has no capture or draw-over-apps prompt.
        let desktop = cfg!(desktop);
        Permissions { capture: desktop, overlay: desktop }
    }

    fn start(&self) -> Result<(), String> {
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
        }
        Ok(())
    }

    fn stop(&self) {
        if let Some(w) = self.app.get_webview_window(OVERLAY) {
            let _ = w.close();
        }
    }

    fn emit(&self, event: &str, payload: serde_json::Value) {
        let _ = self.app.emit(event, payload);
    }

    fn export_dir(&self) -> PathBuf {
        self.app.path().download_dir().unwrap_or_else(|_| self.data_dir.join("export"))
    }

    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        vault::seal(data, &self.data_dir)
    }

    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        vault::unseal(data, &self.data_dir)
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
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let platform = Arc::new(AppPlatform { app: app.handle().clone(), data_dir: data_dir.clone() });
            let backend = Backend::open(data_dir, std::env::var("RST_GEMINI_BASE_URL").ok(), platform)?;
            app.manage(backend);
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
