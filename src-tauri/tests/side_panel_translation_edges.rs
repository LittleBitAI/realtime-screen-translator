//! Edge case of the fortieth review: beside a still side panel only the
//! manga scrolls, and only its translations move; one in the panel keeps
//! its place on the screen.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, FrameOut, KeyIn};
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
    // A PC: the overlay is not in the capture.
    fn overlay_in_capture(&self) -> bool {
        false
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}


fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
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
fn a_translation_in_the_still_side_panel_stays_while_the_rest_scrolls() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let (panel, page) = (Balloon::new(100, 600, 160, 220, 3901), Balloon::new(750, 1200, 140, 200, 3902));
    gemini.read_as(panel.size(), "あ");
    gemini.translate_as("あ", "가");
    gemini.read_as(page.size(), "い");
    gemini.translate_as("い", "나");
    let mut seq = 0;
    let mut frame = |o: usize| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb(o), captured_at: seq as i64 * 100 }).unwrap()
    };
    frame(0);
    let f = frame(0);
    let crop = |b: &Balloon| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop(&panel), crop(&page)] }).unwrap();
    let at = |ko: &str| backend.placements(Default::default()).unwrap().placements.into_iter().find(|p| p.ko == ko).map(|p| p.y);
    wait_for("the two balloons were not translated", || at("가").is_some() && at("나").is_some());
    let (panel_y, page_y) = (at("가").unwrap(), at("나").unwrap());

    let moved = frame(12);
    assert_eq!((moved.motion, moved.dy), ("scroll", 120));
    assert_eq!(at("나"), Some(page_y - 120), "the translation on the scrolled page did not follow it");
    assert_eq!(at("가"), Some(panel_y), "the translation in the still panel moved with the scroll");
    std::mem::forget(dirs);
}

/// Edge case of the forty-second review: the scroll comes while the panel
/// balloon's translation is still on its way (`late` false), or before the
/// crops of the frame before it are even sent (`late` true). The
/// translation comes where the balloon stands on the screen.
fn translated_after_the_scroll(late: bool) {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let (panel, page) = (Balloon::new(100, 600, 160, 220, 4201), Balloon::new(750, 1200, 140, 200, 4202));
    gemini.read_as(panel.size(), "あ");
    gemini.translate_as("あ", "가");
    gemini.read_as(page.size(), "い");
    gemini.translate_as("い", "나");
    gemini.delay(Duration::from_millis(800));
    let mut seq = 0;
    let mut frame = |o: usize| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb(o), captured_at: seq as i64 * 100 }).unwrap()
    };
    frame(0);
    let f = frame(0);
    let crop = |b: &Balloon| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    let send = || backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop(&panel), crop(&page)] }).unwrap();
    if !late {
        send();
    }
    assert_eq!(frame(12).motion, "scroll");
    if late {
        send();
    }
    let at = |ko: &str| backend.placements(Default::default()).unwrap().placements.into_iter().find(|p| p.ko == ko);
    wait_for("the two balloons were not translated", || at("가").is_some() && at("나").is_some());
    let (p, q) = (at("가").unwrap(), at("나").unwrap());
    let covers = |p: &realtime_screen_translator::api::PlacementOut, top: i64, h: u32| p.y as i64 <= top && p.y as i64 + p.h as i64 >= top + h as i64;
    assert!(covers(&p, panel.y, panel.h), "the panel's translation is not on its balloon: {p:?}");
    assert!(covers(&q, page.y - 120, page.h), "the page's translation is not on its balloon: {q:?}");
    std::mem::forget(dirs);
}

#[test]
fn a_panel_translation_coming_after_the_scroll_is_on_its_balloon() {
    translated_after_the_scroll(false);
}

#[test]
fn a_panel_crop_of_a_frame_before_the_scroll_is_placed_on_its_balloon() {
    translated_after_the_scroll(true);
}
