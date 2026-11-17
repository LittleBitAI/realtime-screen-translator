//! Edge cases of the seventeenth review: a blocked capture that still holds
//! this app's white shapes (Android) is blocked, and a database that meets a
//! full disk answers `storage_full`, not `internal`.

mod support;

use realtime_screen_translator::api::{ApiError, CropIn, CropsIn, FrameIn, FrameOut, KeyIn, PlacementOut};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
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

const ROWS: usize = (SCREEN_H * THUMB_W / SCREEN_W) as usize;
const W: usize = THUMB_W as usize;
const CELL: i32 = (SCREEN_W / THUMB_W) as i32;

/// Black but for a white balloon at screen 300..600 x 900..1200.
fn dark_page() -> Vec<u8> {
    let mut t = vec![0u8; W * ROWS];
    for r in 90..120 {
        for c in 30..60 {
            t[r * W + c] = 250;
        }
    }
    t
}

/// Black but for the cells wholly under the shapes: what the Android
/// capture holds once the reader's app blocks it.
fn shapes_only(shown: &[PlacementOut]) -> Vec<u8> {
    let mut t = vec![0u8; W * ROWS];
    for p in shown {
        let (r0, r1) = ((p.y + CELL - 1) / CELL, (p.y + p.h as i32) / CELL);
        let (c0, c1) = ((p.x + CELL - 1) / CELL, (p.x + p.w as i32) / CELL);
        for r in r0.max(0) as usize..(r1.max(0) as usize).min(ROWS) {
            for c in c0.max(0) as usize..(c1.max(0) as usize).min(W) {
                t[r * W + c] = 250;
            }
        }
    }
    t
}

#[test]
fn a_blocked_capture_that_holds_only_the_white_shapes_is_blocked() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let mut seq = 0;
    let mut send = |thumb: Vec<u8>| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: seq as i64 * 100 }).unwrap()
    };
    send(dark_page());
    let f = send(dark_page());
    assert!(f.detect.is_some(), "the dark page was not read");
    // The text in the balloon, well inside it.
    let text = Balloon::new(380, 980, 140, 140, 1701);
    gemini.read_as(text.size(), "うん");
    gemini.translate_as("うん", "응");
    let crop = CropIn { x: 380, y: 980, w: 140, h: 140, kind: "dialogue".into(), score: 0.9, image: text.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let shown = loop {
        let p = backend.placements(Default::default()).unwrap().placements;
        if !p.is_empty() {
            break p;
        }
        assert!(Instant::now() < deadline, "no translation came");
        std::thread::sleep(Duration::from_millis(20));
    };

    // The page itself, its shape drawn over the balloon: the balloon's edge
    // shows around the shape, so it is a picture.
    let mut page = dark_page();
    for (i, v) in shapes_only(&shown).into_iter().enumerate() {
        page[i] = page[i].max(v);
    }
    for _ in 0..3 {
        assert_ne!(send(page.clone()).motion, "blocked", "the dark page with its shape was taken for a blocked capture");
    }
    assert_eq!(backend.status().notice, None);

    // The reader's app blocks the capture: nothing is left but the shapes.
    let frames: Vec<FrameOut> = (0..3).map(|_| send(shapes_only(&shown))).collect();
    assert_eq!(frames[2].motion, "blocked", "the shapes made a blocked capture a picture");
    assert_eq!(backend.status().notice, Some("capture_blocked"));
    std::mem::forget(dirs);
}

#[test]
fn a_full_disk_met_by_the_database_is_storage_full() {
    let full = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL), None);
    assert_eq!(ApiError::from(full), ApiError("storage_full"));
    let busy = rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY), None);
    assert_eq!(ApiError::from(busy), ApiError("internal"));
}
