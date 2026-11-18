//! Edge case of the thirty-ninth review: the columns learned as a still
//! side panel during a scroll are not left out of the still check after
//! it. On a PC a new manga window or new words can come up there while the
//! rest stays put; that change is seen, and the new content is detected.

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

/// Columns 0 .. 64 a still panel (drawn inverted when `changed`), the rest
/// a page scrolled by `o` thumbnail rows.
fn thumb(o: usize, changed: bool) -> Vec<u8> {
    let w = THUMB_W as usize;
    let mut t = vec![0u8; w * 240];
    for r in 0..240 {
        for c in 0..w {
            let bit = if c < 64 { ((r >> (c / 8)) & 1) ^ changed as usize } else { ((r + o) >> ((c - 64) / 4)) & 1 };
            t[r * w + c] = 255 * bit as u8;
        }
    }
    t
}

#[test]
fn a_change_in_the_side_panel_after_a_scroll_is_detected() {
    let dirs = Dirs::new();
    let backend = Backend::open(dirs.data.clone(), None, Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let mut seq = 0;
    let mut frame = |o: usize, changed: bool| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb(o, changed), captured_at: seq as i64 * 100 }).unwrap()
    };
    frame(0, false);
    frame(0, false);
    assert_eq!(frame(12, false).motion, "scroll");
    frame(12, false);

    // The review's case: only the panel's columns change.
    let changed = frame(12, true);
    assert_ne!(changed.motion, "still", "a change in the side panel was taken as a still screen");
    let mut detected = false;
    for _ in 0..4 {
        detected |= frame(12, true).detect.is_some();
    }
    assert!(detected, "the new content in the side panel was never detected");
    std::mem::forget(dirs);
}
