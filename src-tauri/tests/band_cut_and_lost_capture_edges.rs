//! Edge cases of the thirty-second review: a text area cut by a fixed
//! navigation bar or toolbar (not the screen's edge) is replaced by the whole
//! one a scroll shows, and capture stopped from Android's system UI ends the
//! translation session, so allowing capture again lets it start.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, FrameOut, KeyIn, PlacementOut};
use realtime_screen_translator::pipeline::{self, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
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
    // The overlay is not in the capture here (as on Windows): the whole crop
    // is read at once, not held for a frame without the erased shape.
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

/// 80 px (8 thumbnail rows) of navigation bar at the bottom that stays put.
const BAND_ROWS: usize = 8;

fn thumb(scene: &Scene, offset: i64) -> Vec<u8> {
    let mut t = scene.thumb(offset, SCREEN_W, SCREEN_H);
    let w = THUMB_W as usize;
    let rows = t.len() / w;
    for r in rows - BAND_ROWS..rows {
        for c in 0..w {
            t[r * w + c] = 200 + ((c * 7 + r * 13) % 50) as u8;
        }
    }
    t
}

fn frame(backend: &Backend, scene: &Scene, seq: &mut u64, offset: i64) -> FrameOut {
    *seq += 1;
    backend.frame(FrameIn { seq: *seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb(scene, offset), captured_at: *seq as i64 * 100 }).unwrap()
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_text_area_cut_by_a_fixed_bar_is_replaced_by_the_whole_one_a_scroll_shows() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    // The review's case: the balloon's text runs from screen row 2200 under
    // the bar at 2320; only its first 120 rows are seen.
    let (cut, whole) = (Balloon::new(200, 2200, 120, 120, 3201), Balloon::new(200, 1900, 120, 400, 3202));
    gemini.read_as(cut.size(), "あ");
    gemini.translate_as("あ", "가");
    gemini.read_as(whole.size(), "いいい");
    gemini.translate_as("いいい", "나나나");
    let crop = |b: &Balloon| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    let shown = |ko: &str| backend.placements(Default::default()).unwrap().placements.iter().any(|p| p.ko == ko);
    let scene = Scene::new(3201);
    let mut seq = 0;

    frame(&backend, &scene, &mut seq, 0);
    let f = frame(&backend, &scene, &mut seq, 0);
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(&cut)] }).unwrap();
    wait_for("the cut text area was not translated", || shown("가"));

    // Scrolled 300 px: the bar shows it stays put, and the whole text area
    // starts at the same picture row.
    let mut last = frame(&backend, &scene, &mut seq, 300);
    for _ in 0..3 {
        last = frame(&backend, &scene, &mut seq, 300);
    }
    let r = backend.crops(CropsIn { picture_id: last.picture_id, frame_seq: seq, crops: vec![crop(&whole)] }).unwrap();
    assert_eq!(r.discarded, 0, "the whole text area was discarded over the cut one's translation");
    wait_for("the whole text area was not translated", || shown("나나나"));
    assert!(!shown("가"), "the cut text area's translation stayed");
    std::mem::forget(dirs);
}

struct Lost(Mutex<VecDeque<Event>>);

impl Screen for Lost {
    fn next(&self) -> Option<Event> {
        self.0.lock().unwrap().pop_front()
    }
    fn detect(&self, _: u64, _: u32, _: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        Ok(vec![])
    }
    fn strip(&self, _: u64, _: u32, _: u32) -> Option<Vec<u8>> {
        None
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
}

#[test]
fn capture_stopped_from_the_system_ends_the_session_so_it_can_start_again() {
    let dirs = Dirs::new();
    let backend = Backend::open(dirs.data.clone(), None, Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    let screen = Lost(Mutex::new(VecDeque::from([Event::Toggle, Event::Lost])));
    pipeline::run(&backend, &screen);
    assert!(!backend.status().on, "translation stayed on after capture was stopped");
    // Lost while off: nothing to stop, nothing breaks.
    screen.0.lock().unwrap().push_back(Event::Lost);
    pipeline::run(&backend, &screen);
    backend.start().expect("translation could not start again once capture was allowed");
    std::mem::forget(dirs);
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap().replace("\r\n", "\n")
}

#[test]
fn android_tells_rust_capture_was_lost_and_takes_the_overlay_down() {
    let java = "plugins/screen/android/src/main/java/";
    let service = read(&format!("{java}CaptureService.kt"));
    let stop = &service[service.find("override fun onStop()").expect("no projection callback")..];
    let stop = &stop[..stop.find("stopSelf()").unwrap()];
    for call in ["Capture.paused = true", "Overlay.hideLayer()", "Overlay.setOn(false)", "Capture.lost()"] {
        assert!(stop.contains(call), "the projection's onStop does not call {call}");
    }
    let capture = read(&format!("{java}Capture.kt"));
    assert!(capture.contains("put(\"kind\", \"lost\")"), "`next` never answers \"lost\"");
    assert!(read("src/app.rs").contains("n.kind == \"lost\" => return Some(pipeline::Event::Lost)"), "Rust does not read \"lost\"");
}
