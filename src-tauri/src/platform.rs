//! What the backend needs from the device. Android: the Kotlin plugin
//! (MediaProjection capture, draw-over-apps overlay, MediaStore gallery,
//! Android Keystore). Windows: Windows Graphics Capture, a click-through
//! transparent Tauri window, the Downloads folder, DPAPI.

use std::path::{Path, PathBuf};

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
    /// Folder exported pictures are written to: the Downloads folder, or on
    /// Android a cache folder they leave for the gallery through [`Self::publish`].
    fn export_dir(&self) -> PathBuf;
    /// Hand a written export over to where the person finds it (Android:
    /// MediaStore, the gallery) and say where it is now. Elsewhere the file
    /// is already there. An error that names `storage_full` means there was
    /// no room for it (P11 `storage_full`); any other is `storage_denied`.
    fn publish(&self, path: &Path) -> Result<String, String> {
        Ok(path.to_string_lossy().into_owned())
    }
    /// Whether captured frames hold this app's overlay. Android's do (the
    /// screen is mirrored as shown); Windows leaves the overlay out of
    /// capture, so frames show the page under the translations.
    fn overlay_in_capture(&self) -> bool {
        true
    }
    /// Seal and unseal secrets with the device's secure storage.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>>;
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>>;
}
