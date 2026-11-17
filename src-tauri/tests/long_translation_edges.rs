//! Edge case of the thirty-first review: a shape grown for a long
//! translation stays on the screen its text area was seen on, so the first
//! (or last) lines of the Korean are not cut off live or in the export.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, KeyIn};
use realtime_screen_translator::layout;
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

#[test]
fn a_grown_shape_is_kept_on_the_screen_and_still_covers_its_text_area() {
    let ko = "가".repeat(80);
    let screen = (0, 0, SCREEN_W as i64, SCREEN_H as i64);
    // The review's case: grown from y 5 it would start at y -31.
    assert!(layout::fit(200, 5, 120, 20, &ko).y < 0);
    for (x, y) in [(200, 5), (200, SCREEN_H as i64 - 25), (0, 900), (SCREEN_W as i64 - 6, 900)] {
        let w = if x == 200 { 120 } else { 6 };
        let s = layout::fit_within(x, y, w, 20, &ko, screen);
        let grown = layout::fit(x, y, w, 20, &ko);
        assert_eq!((s.w, s.h, s.font_px), (grown.w, grown.h, grown.font_px), "the shape itself changed");
        assert!(s.y >= 0 && s.y + s.h as i64 <= SCREEN_H as i64, "rows {} .. {} are not on screen", s.y, s.y + s.h as i64);
        assert!(s.x >= 0 && s.x + s.w as i64 <= SCREEN_W as i64, "columns {} .. {} are not on screen", s.x, s.x + s.w as i64);
        assert!(s.y <= y && s.y + s.h as i64 >= y + 20, "the shape no longer covers the text area at {y}");
        assert!(s.x <= x && s.x + s.w as i64 >= x + w as i64, "the shape no longer covers the text area at {x}");
    }
}

#[test]
fn a_long_translation_at_the_screen_edge_is_placed_whole_on_screen() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let scene = Scene::new(3101);
    let mut f = None;
    for seq in 1..=2 {
        let thumb = scene.thumb(0, SCREEN_W, SCREEN_H);
        f = Some(backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: seq as i64 * 100 }).unwrap());
    }
    let f = f.unwrap();
    let (top, bottom) = (Balloon::new(200, 5, 120, 20, 3101), Balloon::new(600, SCREEN_H as i64 - 26, 120, 21, 3102));
    gemini.read_as(top.size(), "あ");
    gemini.read_as(bottom.size(), "い");
    gemini.translate_as("あ", &"가".repeat(80));
    gemini.translate_as("い", &"나".repeat(80));
    let crop = |b: &Balloon| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop(&top), crop(&bottom)] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let shown = loop {
        let shown = backend.placements(Default::default()).unwrap().placements;
        if shown.len() == 2 {
            break shown;
        }
        assert!(Instant::now() < deadline, "the translations did not come: {shown:?}");
        std::thread::sleep(Duration::from_millis(20));
    };
    for p in &shown {
        assert!(p.y >= 0 && p.y + p.h as i32 <= SCREEN_H as i32, "{p:?} is not wholly on screen");
    }
    std::mem::forget(dirs);
}
