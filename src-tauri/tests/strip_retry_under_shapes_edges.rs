//! Edge case of the fortieth review: on Android the capture holds the
//! translation shapes. Rows whose strip could not be saved, asked for again
//! once a translation is drawn over them, are saved from a frame taken with
//! the shapes hidden, so the book keeps the page as it is.

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


/// An Android screen that never moves. Its frames hold the overlay; only a
/// peek (`Screen::peek`) gives one without the shapes. Strips cannot be
/// taken until `saving`; those taken are kept as (seq, top, bottom).
struct Reader {
    scene: Scene,
    balloon: Balloon,
    saving: AtomicBool,
    peek_seq: AtomicU64,
    taken: Mutex<Vec<(u64, u32, u32)>>,
}

impl Screen for Reader {
    fn next(&self) -> Option<Event> {
        None
    }
    fn detect(&self, _: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        let b = self.balloon;
        let inside = top as i64 <= b.y && bottom as i64 >= b.y + b.h as i64;
        Ok(inside.then(|| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) }).into_iter().collect())
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        if !self.saving.load(Ordering::Relaxed) {
            return None;
        }
        self.taken.lock().unwrap().push((seq, top, bottom));
        export::encode_png(&Rgb::new(SCREEN_W, bottom - top)).ok()
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
    fn peek(&self) -> Option<Captured> {
        let seq = self.peek_seq.load(Ordering::Relaxed);
        Some(Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: self.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: 0, button: None, bars: vec![] })
    }
}

#[test]
fn rows_saved_again_under_a_shape_come_from_a_frame_without_the_shapes() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let balloon = Balloon::new(300, 700, 160, 220, 3903);
    gemini.read_as(balloon.size(), "あ");
    gemini.translate_as("あ", "가");
    let reader = Reader { scene: Scene::new(3903), balloon, saving: AtomicBool::new(false), peek_seq: AtomicU64::new(0), taken: Mutex::default() };
    let mut seq = 0;
    let mut frame = |r: &Reader| {
        seq += 1;
        r.peek_seq.store(seq * 1000, Ordering::Relaxed);
        let f = Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: r.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: seq as i64 * 100, button: None, bars: vec![] };
        pipeline::step(&backend, r, Event::Frame(f));
    };
    // The review's case: the first strip cannot be saved, and the
    // translation comes and is drawn before saving works again.
    let deadline = Instant::now() + Duration::from_secs(20);
    let shape = loop {
        assert!(Instant::now() < deadline, "the balloon was not translated");
        frame(&reader);
        if let Some(p) = backend.placements(Default::default()).unwrap().placements.into_iter().find(|p| p.ko == "가") {
            break p;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    reader.saving.store(true, Ordering::Relaxed);
    for _ in 0..4 {
        frame(&reader);
    }
    let taken = reader.taken.lock().unwrap().clone();
    let (y0, y1) = (shape.y as u32, shape.y as u32 + shape.h);
    let under: Vec<_> = taken.iter().filter(|t| t.1 < y1 && t.2 > y0).collect();
    assert!(!under.is_empty(), "the rows under the shape were never saved: {taken:?}");
    assert!(under.iter().all(|t| t.0 % 1000 == 0), "rows under the shape were saved from a frame that holds it: {taken:?}");
    std::mem::forget(dirs);
}
