/// Every command `app::run` registers, as `allow-<command>` permissions:
/// the app's screens get them all, a site in the reader only its own two
/// (`capabilities/`).
#[cfg(feature = "app")]
const COMMANDS: &[&str] = &[
    "get_status",
    "put_settings_gemini_key",
    "post_permissions_request",
    "post_translation_start",
    "post_translation_stop",
    "post_frames",
    "post_crops",
    "get_placements",
    "post_strips",
    "get_books",
    "post_books_export",
    "get_preview",
    "put_settings_source_lang",
    "post_reader_open",
    "post_reader_close",
    "post_reader_page",
];

fn main() {
    #[cfg(feature = "app")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS))).expect("tauri-build");
}
