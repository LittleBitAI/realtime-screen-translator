//! Edge case of the seventh review: two different pages whose only overlap
//! at some distance is a flat margin are a page turn, not a scroll.

mod support;

use realtime_screen_translator::api::{FrameIn, KeyIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::tracker;
use realtime_screen_translator::Backend;
use std::path::PathBuf;
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
fn pages_that_line_up_only_on_a_blank_margin_are_a_page_turn() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let device = std::sync::Arc::new(Device(dirs.export.clone()));
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), device).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();

    let rows = (SCREEN_H * THUMB_W / SCREEN_W) as usize;
    let w = THUMB_W as usize;
    let a: Vec<u8> = (0..rows * w).map(|i| if i / w < rows * 2 / 5 { 255 } else { 128 }).collect();
    let b: Vec<u8> = (0..rows * w).map(|i| if i / w < rows * 3 / 5 { 64 } else { 255 }).collect();
    let send = |seq: u64, thumb: &Vec<u8>| {
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb.clone(), captured_at: seq as i64 * 100 }).unwrap()
    };
    send(1, &a);
    let first = send(2, &a);
    let turned = send(3, &b);
    assert_eq!(turned.motion, "page", "a blank margin matched as a scroll by {}", turned.dy);
    assert!(turned.clear);
    let rest = send(4, &b);
    assert_ne!(rest.picture_id, first.picture_id, "the second page went into the first page's picture");
    assert!(tracker::best_shift(&a, &b, rows, 0, 0).is_none());
}
