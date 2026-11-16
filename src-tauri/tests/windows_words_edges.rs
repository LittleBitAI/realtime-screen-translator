//! Edge case of the twenty-second review: where the capture leaves the
//! overlay out (Windows), a page drawn alike but for the words in its
//! balloons is another page, not the one still on screen.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, FrameOut, KeyIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

struct Device {
    export: PathBuf,
    overlay_in_capture: bool,
}

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
        self.export.clone()
    }
    fn overlay_in_capture(&self) -> bool {
        self.overlay_in_capture
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}

/// A translated page, then the next one alike but for its balloon's words.
fn turn_to_a_page_with_other_words(overlay_in_capture: bool) -> (FrameOut, FrameOut, FrameOut) {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let device = Device { export: dirs.export.clone(), overlay_in_capture };
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(device)).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let mut seq = 0u64;
    let mut send = |thumb: &Vec<u8>| {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb.clone(), captured_at: seq as i64 * 100 }).unwrap()
    };
    let a = Scene::new(2201).thumb(0, SCREEN_W, SCREEN_H);
    send(&a);
    let f = send(&a);
    let text = Balloon::new(300, 900, 120, 200, 2201);
    gemini.read_as(text.size(), "うん");
    gemini.translate_as("うん", "응");
    let crop = CropIn { x: 300, y: 900, w: 120, h: 200, kind: "dialogue".into(), score: 0.9, image: text.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let placed = loop {
        let shown = backend.placements(Default::default()).unwrap().placements;
        if let Some(p) = shown.first() {
            break p.clone();
        }
        assert!(Instant::now() < deadline, "no translation came");
        std::thread::sleep(Duration::from_millis(20));
    };
    // Page B: page A with other pixels under the translation only.
    let scale = (SCREEN_W / THUMB_W) as i32;
    let mut b = a.clone();
    for row in placed.y / scale..(placed.y + placed.h as i32) / scale {
        for col in placed.x / scale..(placed.x + placed.w as i32) / scale {
            let v = &mut b[(row * THUMB_W as i32 + col) as usize];
            *v = if *v < 128 { *v + 60 } else { *v - 60 };
        }
    }
    let first = send(&b);
    let second = send(&b);
    std::mem::forget(dirs);
    (f, first, second)
}

#[test]
fn a_page_alike_but_for_its_words_is_a_page_turn_where_the_overlay_is_not_captured() {
    let (a, first, second) = turn_to_a_page_with_other_words(false);
    assert_eq!(first.motion, "page", "the next page was taken for the one still on screen");
    assert_ne!(second.picture_id, a.picture_id, "the next page kept the translated page's picture");
    assert!(second.placements.is_empty(), "the old translation stayed on the next page");
}

#[test]
fn where_the_overlay_is_captured_its_place_is_left_out_of_the_comparison() {
    let (a, first, second) = turn_to_a_page_with_other_words(true);
    assert_eq!(first.motion, "still");
    assert_eq!(second.picture_id, a.picture_id);
}

#[test]
fn where_the_overlay_is_not_captured_the_whole_text_area_is_read_at_once() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let device = Device { export: dirs.export.clone(), overlay_in_capture: false };
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(device)).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let scene = Scene::new(2202);
    let mut seq = 0u64;
    let mut send = |offset: i64| {
        seq += 1;
        let thumb = scene.thumb(offset, SCREEN_W, SCREEN_H);
        (seq, backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: seq as i64 * 100 }).unwrap())
    };
    send(0);
    let (s0, f) = send(0);
    let (cut, whole) = (Balloon::new(200, 2200, 120, 200, 2203), Balloon::new(200, 1900, 120, 400, 2204));
    gemini.read_as(cut.size(), "うん");
    gemini.translate_as("うん", "응");
    gemini.read_as(whole.size(), "そうだね");
    gemini.translate_as("そうだね", "그렇지");
    let crop = |b: &Balloon, y: u32| CropIn { x: 200, y, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    let shown = |ko: &str| backend.placements(Default::default()).unwrap().placements.iter().any(|p| p.ko == ko);
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: s0, crops: vec![crop(&cut, 2200)] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !shown("응") {
        assert!(Instant::now() < deadline, "the cut text area was not read");
        std::thread::sleep(Duration::from_millis(20));
    }
    send(300);
    let (s1, g) = send(300);
    // The frame shows the page under the translations: nothing to wait for.
    backend.crops(CropsIn { picture_id: g.picture_id, frame_seq: s1, crops: vec![crop(&whole, 1900)] }).unwrap();
    while !shown("그렇지") {
        assert!(Instant::now() < deadline, "the whole text area was not read from the frame it was seen in");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::mem::forget(dirs);
}
