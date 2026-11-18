//! Edge case of the forty-third review: on a still screen with nothing
//! placed, a small part changes much (words that came up late, or a page
//! drawn the same but for a small balloon). The mean stays under the still
//! limit, but the screen is detected again.

mod support;

use realtime_screen_translator::api::{FrameIn, FrameOut, KeyIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::Arc;
use support::*;

struct Device(PathBuf);

impl Platform for Device {
    fn request_permissions(&self) -> Permissions {
        self.permissions()
    }
    fn permissions(&self) -> Permissions {
        Permissions { capture: true, overlay: true }
    }
    fn start(&self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&self) {}
    fn emit(&self, _: &str, _: serde_json::Value) {}
    fn export_dir(&self) -> PathBuf {
        self.0.clone()
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}


#[test]
fn a_small_part_changing_on_a_still_screen_is_detected_again() {
    let dirs = Dirs::new();
    let backend = Backend::open(dirs.data.clone(), None, Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let scene = Scene::new(4301);
    let page = scene.thumb(0, SCREEN_W, SCREEN_H);
    // The review's frame: thumbnail rows 100 .. 110, columns 40 .. 50 inverted.
    let mut words = page.clone();
    let w = THUMB_W as usize;
    for r in 100..110 {
        for c in 40..50 {
            words[r * w + c] = 255 - words[r * w + c];
        }
    }
    let mut seq = 0;
    let mut frame = |thumb: &Vec<u8>| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb.clone(), captured_at: seq as i64 * 100 }).unwrap()
    };
    // The whole screen is detected; no crop is sent.
    let mut detected = false;
    for _ in 0..3 {
        detected |= frame(&page).detect.is_some();
    }
    assert!(detected, "the first screen was never detected");
    assert!(frame(&page).detect.is_none(), "a screen that did not change was detected again");

    let mut again = None;
    for _ in 0..4 {
        let f = frame(&words);
        assert_eq!(f.motion, "still");
        again = again.or(f.detect);
    }
    let again = again.expect("the part of the screen that changed was never detected again");
    assert!(again.top <= 1000 && again.bottom >= 1100, "the changed rows are not in {again:?}");
    std::mem::forget(dirs);
}
