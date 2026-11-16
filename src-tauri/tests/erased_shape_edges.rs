//! Edge case of the twenty-sixth review: the white shape of a translation
//! taken away (a balloon the edge had cut, seen whole after a scroll) stays
//! in the Android capture for a frame or two. Its going is not the screen
//! changing: the stitched picture goes on, and nothing reads as a page turn.

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

/// The scene scrolled by `offset`, with the white shape of `shape` (screen
/// coordinates of the screen at offset 0) drawn over it, as the capture holds it.
fn frame(scene: &Scene, offset: i64, shape: Option<&PlacementOut>) -> Vec<u8> {
    let mut t = scene.thumb(offset, SCREEN_W, SCREEN_H);
    if let Some(p) = shape {
        let s = (SCREEN_W / THUMB_W) as i32;
        let rows = t.len() as i32 / THUMB_W as i32;
        let y = p.y - offset as i32;
        for r in (y / s).max(0)..((y + p.h as i32) / s).min(rows) {
            for c in p.x / s..(p.x + p.w as i32) / s {
                t[(r * THUMB_W as i32 + c) as usize] = 255;
            }
        }
    }
    t
}

#[test]
fn the_going_of_an_erased_shape_is_no_page_turn() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let mut seq = 0u64;
    let mut send = |thumb: Vec<u8>| -> (u64, FrameOut) {
        seq += 1;
        (seq, backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: seq as i64 * 100 }).unwrap())
    };
    let scene = Scene::new(2601);
    send(frame(&scene, 0, None));
    let (s0, f) = send(frame(&scene, 0, None));
    // Rows 1800 .. 2400 of a text area 900 rows tall: the bottom edge cuts it.
    let (cut, whole) = (Balloon::new(200, 1800, 400, 600, 2602), Balloon::new(200, 1200, 400, 900, 2603));
    gemini.read_as(cut.size(), "うん");
    gemini.translate_as("うん", "응");
    gemini.read_as(whole.size(), "そうだね");
    gemini.translate_as("そうだね", "그렇지");
    let crop = |b: &Balloon| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: s0, crops: vec![crop(&cut)] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let shape = loop {
        if let Some(p) = backend.placements(Default::default()).unwrap().placements.first() {
            break p.clone();
        }
        assert!(Instant::now() < deadline, "the cut text area was not read");
        std::thread::sleep(Duration::from_millis(20));
    };
    // The shape is drawn; then 600 px further down the whole text area shows.
    assert_eq!(send(frame(&scene, 0, Some(&shape))).1.motion, "still");
    assert_eq!(send(frame(&scene, 600, Some(&shape))).1.motion, "scroll");
    let (s1, g) = send(frame(&scene, 600, Some(&shape)));
    assert_eq!((g.motion, g.picture_id), ("still", f.picture_id));
    backend.crops(CropsIn { picture_id: g.picture_id, frame_seq: s1, crops: vec![crop(&whole.at(200, 1200))] }).unwrap();
    // The capture holds the erased shape for a frame, then not.
    let frames = [Some(&shape), None, None, None];
    for (k, s) in frames.into_iter().enumerate() {
        let (_, out) = send(frame(&scene, 600, s));
        assert_eq!(out.motion, "still", "frame {k} after the shape was erased");
        assert_eq!(out.picture_id, f.picture_id, "frame {k}: the stitched picture was closed");
        assert!(!out.clear, "frame {k}: the overlay was cleared");
    }
    std::mem::forget(dirs);
}
