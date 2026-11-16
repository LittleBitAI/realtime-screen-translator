//! What the backend needs from the device. Android: the Kotlin plugin
//! (MediaProjection capture, draw-over-apps overlay, MediaStore gallery,
//! Android Keystore). Windows: Windows Graphics Capture, a click-through
//! transparent Tauri window, the Downloads folder, DPAPI.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    pub capture: bool,
    pub overlay: bool,
}

pub trait Platform: Send + Sync {
    /// Ask for screen capture and drawing over other apps, in that order.
    fn request_permissions(&self) -> Permissions;
    /// What is granted now, without asking.
    fn permissions(&self) -> Permissions;
    /// Start capture and put the overlay and the on/off button over other apps.
    fn start(&self) -> Result<(), String>;
    /// Take capture and the overlay down.
    fn stop(&self);
    /// Tell the overlay that placements changed (a Tauri event in the app).
    fn emit(&self, event: &str, payload: serde_json::Value);
    /// Folder exported pictures go to: the gallery or the Downloads folder.
    fn export_dir(&self) -> PathBuf;
    /// Seal and unseal secrets with the device's secure storage.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>>;
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>>;
}
