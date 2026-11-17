//! Edge cases of the twenty-fifth review: a scroll a quarter of a thumbnail
//! row a frame adds up instead of being taken as still every frame; and
//! without a frame free of the button, a crop the button lies over is not
//! sent to Gemini.

mod support;

use realtime_screen_translator::api::{CropIn, FrameIn, KeyIn, PlacementOut};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::collections::VecDeque;
use std::path::PathBuf;
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

fn open(dirs: &Dirs, gemini: &Gemini) -> Backend {
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    backend
}

const ROWS: usize = (SCREEN_H * THUMB_W / SCREEN_W) as usize;
const W: usize = THUMB_W as usize;

/// White with a black 20 x 20 square whose top is at thumbnail row `top`
/// (a fraction: the rows it partly covers are averaged, as A2 does).
fn square(top: f64) -> Vec<u8> {
    let mut t = vec![255u8; W * ROWS];
    for r in 0..ROWS {
        let cover = ((r as f64 + 1.0).min(top + 20.0) - (r as f64).max(top)).clamp(0.0, 1.0);
        t[r * W + 38..r * W + 58].fill((255.0 * (1.0 - cover)).round() as u8);
    }
    t
}

#[test]
fn a_scroll_a_quarter_row_a_frame_adds_up() {
    let (dirs, gemini) = (Dirs::new(), Gemini::start());
    let backend = open(&dirs, &gemini);
    let mut seq = 0u64;
    let mut send = |thumb: Vec<u8>| {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: seq as i64 * 100 }).unwrap()
    };
    send(square(110.0));
    send(square(110.0));
    // 40 frames of a quarter row (2.5 px) each: 100 px.
    for k in 1..=40 {
        send(square(110.0 - 0.25 * k as f64));
    }
    let offset = backend.placements(Default::default()).unwrap().offset_y;
    assert!((offset - 100).abs() <= 10, "a slow 100 px scroll was tracked as {offset} px");
    std::mem::forget(dirs);
}

/// The button: right edge, screen rows 2150 .. 2250.
const BUTTON: (u32, u32, u32, u32) = (800, 2150, 140, 100);

/// A resting screen that holds the button; no frame without it comes. The
/// detector finds one text area clear of the button and one under it.
struct FakeScreen {
    scene: Scene,
    seq: Mutex<u64>,
    events: Mutex<VecDeque<()>>,
    found: Mutex<Option<Vec<CropIn>>>,
}

impl Screen for FakeScreen {
    fn next(&self) -> Option<Event> {
        self.events.lock().unwrap().pop_front()?;
        let mut seq = self.seq.lock().unwrap();
        *seq += 1;
        let thumb = self.scene.thumb(0, SCREEN_W, SCREEN_H);
        Some(Event::Frame(Captured { seq: *seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: *seq as i64 * 100, button: Some(BUTTON), bars: vec![] }))
    }
    fn detect(&self, _: u64, _: u32, _: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        Ok(self.found.lock().unwrap().take().unwrap_or_default())
    }
    fn strip(&self, _: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        Some(png_rgb(SCREEN_W, bottom - top, |_, _| [128, 128, 128]))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
}

#[test]
fn without_a_frame_free_of_the_button_a_crop_it_lies_over_is_not_sent() {
    let (dirs, gemini) = (Dirs::new(), Gemini::start());
    let backend = open(&dirs, &gemini);
    let (clear, under) = (Balloon::new(200, 900, 120, 200, 2501), Balloon::new(760, 2100, 120, 180, 2502));
    gemini.read_as(clear.size(), "うん");
    gemini.translate_as("うん", "응");
    gemini.read_as(under.size(), "ええ");
    gemini.translate_as("ええ", "네");
    let crop = |b: &Balloon| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    let screen = FakeScreen {
        scene: Scene::new(2501),
        seq: Mutex::new(0),
        events: Mutex::new(VecDeque::from([(), ()])),
        found: Mutex::new(Some(vec![crop(&clear), crop(&under)])),
    };
    pipeline::run(&backend, &screen);
    let deadline = Instant::now() + Duration::from_secs(20);
    while backend.placements(Default::default()).unwrap().placements.is_empty() {
        assert!(Instant::now() < deadline, "the text area clear of the button was not read");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    let shown = backend.placements(Default::default()).unwrap().placements;
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert!(shown[0].x < 400, "the crop under the button was read: {shown:?}");
    std::mem::forget(dirs);
}
