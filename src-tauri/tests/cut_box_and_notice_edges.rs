//! Edge cases of the tenth review: a text area cut by the screen edge is
//! read once it is seen whole, an answer with anything after its JSON
//! failed, and `detector_missing` goes once the model runs.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::detect::{self, Detection, DIALOGUE};
use realtime_screen_translator::export::Rgb;
use realtime_screen_translator::gemini::{self, TRANSLATE_PREFILL};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use support::*;

fn area(y: f32, h: f32) -> Detection {
    Detection { x: 200.0, y, w: 300.0, h, label: DIALOGUE, score: 0.9 }
}

#[test]
fn a_text_area_cut_by_the_screen_bottom_is_read_whole_after_the_scroll() {
    let img = Rgb::new(960, 2400);
    // Screen rows 2100 .. 2500: cut off by the bottom edge, so not read yet.
    assert!(detect::crops(&img, &[area(2100.0, 400.0)], 0, 2400).is_empty(), "a cut-off text area was read");
    // 300 px later it is whole at 1800 .. 2200 and reaches into the new rows 2100 .. 2400.
    let c = detect::crops(&img, &[area(1800.0, 400.0), area(700.0, 400.0)], 2100, 2400);
    assert_eq!(c.len(), 1, "{c:?}");
    assert_eq!((c[0].y, c[0].h), (1800, 400), "the text area was not read whole");
    // One that ended just above the edge waited too, and is taken the same way.
    assert!(detect::crops(&img, &[area(1990.0, 400.0)], 0, 2400).is_empty());
    assert_eq!(detect::crops(&img, &[area(1690.0, 400.0)], 2100, 2400).len(), 1);
    // A cut one at the top edge waits for a scroll up.
    assert!(detect::crops(&img, &[area(-100.0, 400.0)], 0, 2400).is_empty());
    assert_eq!(detect::crops(&img, &[area(200.0, 400.0)], 0, 300).len(), 1);
}

#[test]
fn an_answer_with_anything_after_its_json_is_not_read() {
    assert_eq!(gemini::parse(TRANSLATE_PREFILL, "안녕\"]} trailing", "ko", 1), None);
    assert_eq!(gemini::parse(TRANSLATE_PREFILL, "안녕\"]}{\"ko\":[\"x\"]}", "ko", 1), None);
    assert_eq!(gemini::parse(TRANSLATE_PREFILL, "안녕\"]}  \n", "ko", 1), Some(vec!["안녕".to_string()]));
}

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

/// The model is missing for the first detection and in place after it.
struct FakeScreen {
    scene: Scene,
    offsets: Mutex<Vec<(u64, i64)>>,
    events: Mutex<VecDeque<Event>>,
    detections: Mutex<u32>,
}

impl FakeScreen {
    fn show(&self, seq: &mut u64, offset: i64, n: usize) {
        for _ in 0..n {
            *seq += 1;
            self.offsets.lock().unwrap().push((*seq, offset));
            let thumb = self.scene.thumb(offset, SCREEN_W, SCREEN_H);
            let f = Captured { seq: *seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: *seq as i64 * 100, button: None, bars: vec![] };
            self.events.lock().unwrap().push_back(Event::Frame(f));
        }
    }
}

impl Screen for FakeScreen {
    fn next(&self) -> Option<Event> {
        self.events.lock().unwrap().pop_front()
    }
    fn detect(&self, _: u64, _: u32, _: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        let mut n = self.detections.lock().unwrap();
        *n += 1;
        if *n == 1 { Err(Some("detector_missing")) } else { Ok(vec![]) }
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        let off = self.offsets.lock().unwrap().iter().find(|f| f.0 == seq)?.1;
        Some(self.scene.screen_png(off, SCREEN_W, SCREEN_H, top, bottom))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
}

#[test]
fn the_missing_model_notice_goes_once_detection_runs() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let screen = FakeScreen { scene: Scene::new(1001), offsets: Mutex::default(), events: Mutex::default(), detections: Mutex::new(0) };
    let mut seq = 0;
    screen.show(&mut seq, 0, 2);
    pipeline::run(&backend, &screen);
    assert_eq!(backend.status().notice, Some("detector_missing"));
    // The model is in place now; the screen scrolls and new rows are detected.
    screen.show(&mut seq, 300, 2);
    pipeline::run(&backend, &screen);
    assert_eq!(*screen.detections.lock().unwrap(), 2, "no second detection was asked for");
    assert_eq!(backend.status().notice, None, "detector_missing stayed after the model ran");
}
