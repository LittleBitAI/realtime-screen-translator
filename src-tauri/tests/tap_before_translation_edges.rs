//! Edge case of the forty-first review: on Android a tap turns, before
//! Gemini answered, to a page drawn the same whose words differ. Nothing is
//! placed yet, so no shape hides anything, but the answer coming would be
//! placed over the other page's words: the look after the tap compares the
//! balloons still waiting for their translation too, and the page is
//! turned to.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::export::{self, Rgb};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// An Android screen whose thumbnails never change: the page turned to is
/// drawn the same but for the words in its balloon. With no shape drawn,
/// every frame shows the words.
struct Reader {
    scene: Scene,
    before: Balloon,
    after: Balloon,
    turned: AtomicBool,
    touched: AtomicBool,
    peek_seq: AtomicU64,
}

impl Reader {
    fn shown(&self) -> Balloon {
        if self.turned.load(Ordering::Relaxed) {
            self.after
        } else {
            self.before
        }
    }
}

impl Screen for Reader {
    fn next(&self) -> Option<Event> {
        None
    }
    fn detect(&self, _: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        let b = self.shown();
        let inside = top as i64 <= b.y && bottom as i64 >= b.y + b.h as i64;
        Ok(inside.then(|| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) }).into_iter().collect())
    }
    fn strip(&self, _: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        let b = self.shown();
        let words = export::decode_png(&b.png(0, 0)).unwrap();
        let mut rows = Rgb::new(SCREEN_W, bottom - top);
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
        export::encode_png(&rows).ok()
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
    fn touched(&self) -> bool {
        self.touched.swap(false, Ordering::Relaxed)
    }
    fn peek(&self) -> Option<Captured> {
        let seq = self.peek_seq.load(Ordering::Relaxed);
        Some(Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: self.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: 0, button: None, bars: vec![] })
    }
}

#[test]
fn a_tap_to_other_words_before_the_translation_came_turns_the_page() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    // The new page's balloon is in the same place; the fake reads by size.
    let (a, c) = (Balloon::new(300, 700, 160, 220, 4101), Balloon::new(300, 700, 160, 230, 4102));
    gemini.read_as(a.size(), "あ");
    gemini.translate_as("あ", "가");
    gemini.read_as(c.size(), "う");
    gemini.translate_as("う", "다");
    gemini.delay(Duration::from_millis(1500));
    let reader = Reader { scene: Scene::new(4101), before: a, after: c, turned: AtomicBool::new(false), touched: AtomicBool::new(false), peek_seq: AtomicU64::new(0) };
    let mut seq = 0;
    let mut frame = |r: &Reader| {
        seq += 1;
        r.peek_seq.store(seq * 1000, Ordering::Relaxed);
        let f = Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: r.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: seq as i64 * 100, button: None, bars: vec![] };
        pipeline::step(&backend, r, Event::Frame(f));
    };
    let shown = |b: &Backend| b.placements(Default::default()).unwrap().placements.iter().map(|p| p.ko.clone()).collect::<Vec<_>>();
    // The first page's crop is sent, and Gemini takes its time.
    let deadline = Instant::now() + Duration::from_secs(20);
    while gemini.count() == 0 {
        assert!(Instant::now() < deadline, "the first page's balloon was never sent");
        frame(&reader);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(shown(&backend).is_empty());

    // The review's case: the tap turns the page before the answer came.
    reader.turned.store(true, Ordering::Relaxed);
    reader.touched.store(true, Ordering::Relaxed);
    let mut wrong = false;
    while shown(&backend) != ["다"] {
        assert!(Instant::now() < deadline, "the page turned to before the answer was not translated: {:?}", shown(&backend));
        frame(&reader);
        wrong |= shown(&backend).iter().any(|k| k == "가");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!wrong, "the first page's translation was placed over the page turned to");
    std::mem::forget(dirs);
}
