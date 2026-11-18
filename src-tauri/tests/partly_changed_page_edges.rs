//! Edge case of the thirty-eighth review: on Android a page turned without
//! a slide, drawn the same, whose words changed in only one of its two
//! balloons, is still found by the look under the opaque shapes. The new
//! page shows the new balloon's translation, and the balloon whose words
//! stayed gets its stored one back.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::export::{self, Rgb};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
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

/// An Android screen whose thumbnails never change (the opaque shapes hide
/// the balloons); only a frame taken with the shapes hidden shows them.
struct Reader {
    scene: Scene,
    before: Vec<Balloon>,
    after: Vec<Balloon>,
    turned: AtomicBool,
    touched: AtomicBool,
    peek_seq: AtomicU64,
    drawn: Mutex<Vec<Vec<PlacementOut>>>,
}

impl Reader {
    fn shown(&self) -> &[Balloon] {
        if self.turned.load(Ordering::Relaxed) {
            &self.after
        } else {
            &self.before
        }
    }
}

impl Screen for Reader {
    fn next(&self) -> Option<Event> {
        None
    }
    fn detect(&self, _: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        Ok(self
            .shown()
            .iter()
            .filter(|b| top as i64 <= b.y && bottom as i64 >= b.y + b.h as i64)
            .map(|b| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) })
            .collect())
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        if seq != self.peek_seq.load(Ordering::Relaxed) {
            return None;
        }
        let mut rows = Rgb::new(SCREEN_W, bottom - top);
        for b in self.shown() {
            let words = export::decode_png(&b.png(0, 0)).unwrap();
            for y in 0..rows.h as i64 {
                let sy = y + top as i64 - b.y;
                if !(0..b.h as i64).contains(&sy) {
                    continue;
                }
                for x in 0..b.w as i64 {
                    let (src, dst) = ((sy * b.w as i64 + x) as usize * 3, (y * SCREEN_W as i64 + b.x + x) as usize * 3);
                    rows.px[dst..dst + 3].copy_from_slice(&words.px[src..src + 3]);
                }
            }
        }
        export::encode_png(&rows).ok()
    }
    fn draw(&self, placements: &[PlacementOut], _: bool) {
        self.drawn.lock().unwrap().push(placements.to_vec());
    }
    fn touched(&self) -> bool {
        self.touched.swap(false, Ordering::Relaxed)
    }
    fn peek(&self) -> Option<Captured> {
        let seq = self.peek_seq.load(Ordering::Relaxed);
        Some(Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: self.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: 0, button: None, bars: vec![] })
    }
}

#[test]
fn a_page_whose_words_changed_in_one_of_two_balloons_is_turned_to() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let (a, b) = (Balloon::new(200, 500, 160, 220, 3801), Balloon::new(560, 1400, 140, 200, 3802));
    let c = Balloon::new(560, 1400, 140, 200, 3803);
    gemini.read_as(a.size(), "あ");
    gemini.translate_as("あ", "가");
    gemini.read_as(b.size(), "い");
    gemini.translate_as("い", "나");
    let reader = Reader {
        scene: Scene::new(3801),
        before: vec![a, b],
        after: vec![a, c],
        turned: AtomicBool::new(false),
        touched: AtomicBool::new(false),
        peek_seq: AtomicU64::new(0),
        drawn: Mutex::default(),
    };
    let mut seq = 0;
    let mut frame = |r: &Reader| {
        seq += 1;
        r.peek_seq.store(seq * 1000, Ordering::Relaxed);
        let f = Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: r.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: seq as i64 * 100, button: None, bars: vec![] };
        pipeline::step(&backend, r, Event::Frame(f));
    };
    let shown = |b: &Backend| {
        let mut ko: Vec<String> = b.placements(Default::default()).unwrap().placements.iter().map(|p| p.ko.clone()).collect();
        ko.sort();
        ko
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while shown(&backend) != ["가", "나"] {
        assert!(Instant::now() < deadline, "the first page was not translated: {:?}", shown(&backend));
        frame(&reader);
        std::thread::sleep(Duration::from_millis(20));
    }

    // The tap turns to the page whose second balloon says something else
    // (the same size as the old one: the fake now reads that size as "う").
    gemini.read_as(c.size(), "う");
    gemini.translate_as("う", "다");
    reader.turned.store(true, Ordering::Relaxed);
    reader.touched.store(true, Ordering::Relaxed);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut cleared = false;
    while shown(&backend) != ["가", "다"] {
        assert!(Instant::now() < deadline, "the page with one balloon changed was not turned to: {:?}, cleared {cleared}", shown(&backend));
        frame(&reader);
        cleared |= reader.drawn.lock().unwrap().last().is_some_and(|d| d.is_empty());
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(cleared, "the overlay was not cleared for the turn");
    std::mem::forget(dirs);
}
