//! Edge cases of the eighteenth review: a page seen on a screen of another
//! size is not reopened for the same thumbnail, and a dark page whose shapes
//! cover all that is lit on it is not a blocked capture.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, FrameOut, KeyIn, PlacementOut};
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

fn on(dirs: &Dirs, gemini: &Gemini) -> Backend {
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    backend
}

struct Sender<'a> {
    backend: &'a Backend,
    seq: u64,
}

impl Sender<'_> {
    fn send(&mut self, (w, h): (u32, u32), thumb: Vec<u8>) -> FrameOut {
        self.seq += 1;
        self.backend.frame(FrameIn { seq: self.seq, width: w, height: h, thumb, captured_at: self.seq as i64 * 100 }).unwrap()
    }
}

#[test]
fn a_page_seen_on_a_screen_of_another_size_is_not_reopened() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = on(&dirs, &gemini);
    let mut tx = Sender { backend: &backend, seq: 0 };
    let (small, large) = ((1280, 720), (2560, 1440));
    // Both are 96 x 54 thumbnails: the same page scaled up looks the same.
    let a = Scene::new(1801).thumb(0, small.0, small.1);
    let b = Scene::new(1802).thumb(0, large.0, large.1);
    tx.send(small, a.clone());
    let a_id = tx.send(small, a.clone()).picture_id;
    tx.send(large, b.clone());
    let b_id = tx.send(large, b).picture_id;
    assert_ne!(a_id, b_id);
    tx.send(large, a.clone());
    let back = tx.send(large, a);
    assert_ne!(back.picture_id, a_id, "the page saved at 1280 px was reopened on a 2560 px screen");
    assert_ne!(back.picture_id, b_id);
    assert!(back.save.is_some(), "the new page is not saved");
    std::mem::forget(dirs);
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

/// The page with the shapes drawn over it (white), as the Android capture holds it.
fn with_shapes(mut t: Vec<u8>, shown: &[PlacementOut]) -> Vec<u8> {
    for p in shown {
        let (r0, r1) = ((p.y / CELL).max(0) as usize, (((p.y + p.h as i32 + CELL - 1) / CELL).max(0) as usize).min(ROWS));
        let (c0, c1) = ((p.x / CELL).max(0) as usize, (((p.x + p.w as i32 + CELL - 1) / CELL).max(0) as usize).min(W));
        for r in r0..r1 {
            t[r * W + c0..r * W + c1].fill(250);
        }
    }
    t
}

#[test]
fn a_dark_page_whose_shape_covers_its_only_balloon_is_not_blocked() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = on(&dirs, &gemini);
    let mut tx = Sender { backend: &backend, seq: 0 };
    let screen = (SCREEN_W, SCREEN_H);
    tx.send(screen, dark_page());
    let f = tx.send(screen, dark_page());
    assert!(f.detect.is_some());
    // The text area fills the balloon: its shape covers all of it.
    let text = Balloon::new(304, 904, 292, 292, 1803);
    gemini.read_as(text.size(), "うん");
    gemini.translate_as("うん", "응");
    let crop = CropIn { x: 304, y: 904, w: 292, h: 292, kind: "dialogue".into(), score: 0.9, image: text.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: tx.seq, crops: vec![crop] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let shown = loop {
        let p = backend.placements(Default::default()).unwrap().placements;
        if !p.is_empty() {
            break p;
        }
        assert!(Instant::now() < deadline, "no translation came");
        std::thread::sleep(Duration::from_millis(20));
    };
    let page = with_shapes(dark_page(), &shown);
    for _ in 0..3 {
        let f = tx.send(screen, page.clone());
        assert_ne!(f.motion, "blocked", "the translated dark page was taken for a blocked capture");
    }
    assert_eq!(backend.status().notice, None);
    // Without the shapes nothing is lit at all: that is still blocked.
    let black = vec![0u8; W * ROWS];
    let f = (0..3).map(|_| tx.send(screen, black.clone())).last().unwrap();
    assert_eq!(f.motion, "blocked");
    std::mem::forget(dirs);
}
