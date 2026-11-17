//! realtime-screen-translator backend.
//!
//! [`backend::Backend`] holds every command of `docs/design/api.yaml`
//! (A1: `POST /frames` is `post_frames`). The platform layer (capture,
//! permission prompts, overlay, gallery or Downloads folder, key vault) sits
//! behind [`platform::Platform`]: the Kotlin plugin on Android, Rust modules
//! on Windows, fakes in the `contract-host` test binary.

pub mod api;
#[cfg(feature = "app")]
pub mod app;
pub mod backend;
pub mod detect;
pub mod export;
#[cfg(not(windows))]
pub mod file_vault;
pub mod fingerprint;
pub mod gemini;
pub mod layout;
pub mod models;
pub mod overlay;
pub mod pipeline;
pub mod platform;
pub mod store;
pub mod tracker;
pub mod vault;
#[cfg(all(feature = "app", windows))]
pub mod wgc;

pub use backend::Backend;

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
