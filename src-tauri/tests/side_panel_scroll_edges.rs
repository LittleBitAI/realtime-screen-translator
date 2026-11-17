//! Edge case of the thirty-seventh review: a fixed side panel, or on a PC
//! another window beside the manga, stays put while only the manga
//! scrolls. The still columns are left out of the shift search, so the
//! scroll is told as a scroll, not a page turn.

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

/// The review's screen: columns 0 .. 64 a still panel, the rest a page
/// scrolled by `o` thumbnail rows.
fn thumb(o: usize) -> Vec<u8> {
    let w = THUMB_W as usize;
    let mut t = vec![0u8; w * 240];
    for r in 0..240 {
        for c in 0..w {
            let bit = if c < 64 { (r >> (c / 8)) & 1 } else { ((r + o) >> ((c - 64) / 4)) & 1 };
            t[r * w + c] = 255 * bit as u8;
        }
    }
    t
}

#[test]
fn only_the_manga_scrolling_beside_a_still_panel_is_a_scroll() {
    let dirs = Dirs::new();
    let backend = Backend::open(dirs.data.clone(), None, Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let mut seq = 0;
    let mut frame = |o: usize| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb(o), captured_at: seq as i64 * 100 }).unwrap()
    };
    let first = frame(0);
    frame(0);
    let moved = frame(12);
    assert_eq!((moved.motion, moved.dy, moved.clear), ("scroll", 120, false), "the scroll beside a still panel was not told as one");
    assert_eq!(moved.picture_id, first.picture_id, "the picture was closed");
    let rest = frame(12);
    assert_eq!(rest.motion, "still");
    assert_eq!(rest.picture_id, first.picture_id);
    std::mem::forget(dirs);
}
