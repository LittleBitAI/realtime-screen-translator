//! Edge cases of the ninth review: the rows behind the Android floating
//! button are saved clean from a frame taken with the button hidden, so a
//! screen never scrolled afterwards is still exported whole.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

/// The button: right edge, screen rows 2150 .. 2250.
const BUTTON: (u32, u32, u32, u32) = (800, 2150, 140, 100);
const MAGENTA: [u8; 3] = [255, 0, 255];

fn in_button(x: u32, y: u32) -> bool {
    x >= BUTTON.0 && x < BUTTON.0 + BUTTON.2 && y >= BUTTON.1 && y < BUTTON.1 + BUTTON.3
}

/// Frames hold the button (magenta) unless taken by `clean_frame`, which
/// shows the screen at `clean_offset`.
struct FakeScreen {
    scene: Scene,
    clean_offset: i64,
    seq: Mutex<u64>,
    /// seq -> (scroll offset, holds the button)
    frames: Mutex<Vec<(u64, i64, bool)>>,
    events: Mutex<VecDeque<Event>>,
    strips: Mutex<Vec<(u64, u32, u32)>>,
}

impl FakeScreen {
    fn new(seed: u64, clean_offset: i64) -> FakeScreen {
        FakeScreen {
            scene: Scene::new(seed),
            clean_offset,
            seq: Mutex::new(0),
            frames: Mutex::default(),
            events: Mutex::default(),
            strips: Mutex::default(),
        }
    }
    fn capture(&self, offset: i64, button: bool) -> Captured {
        let mut seq = self.seq.lock().unwrap();
        *seq += 1;
        self.frames.lock().unwrap().push((*seq, offset, button));
        let mut thumb = self.scene.thumb(offset, SCREEN_W, SCREEN_H);
        if button {
            let s = SCREEN_W / THUMB_W;
            for r in BUTTON.1 / s..(BUTTON.1 + BUTTON.3) / s {
                for c in BUTTON.0 / s..(BUTTON.0 + BUTTON.2) / s {
                    thumb[(r * THUMB_W + c) as usize] = 105;
                }
            }
        }
        Captured { seq: *seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: *seq as i64 * 100, button: Some(BUTTON), bars: vec![] }
    }
    fn show(&self, offset: i64, n: usize) {
        for _ in 0..n {
            let f = self.capture(offset, true);
            self.events.lock().unwrap().push_back(Event::Frame(f));
        }
    }
}

impl Screen for FakeScreen {
    fn next(&self) -> Option<Event> {
        self.events.lock().unwrap().pop_front()
    }
    fn detect(&self, _: u64, _: u32, _: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        Ok(vec![])
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        self.strips.lock().unwrap().push((seq, top, bottom));
        let (_, off, button) = *self.frames.lock().unwrap().iter().find(|f| f.0 == seq)?;
        Some(png_rgb(SCREEN_W, bottom - top, |x, y| {
            if button && in_button(x, y + top) {
                MAGENTA
            } else {
                let v = self.scene.screen_px(off, SCREEN_W, x, y + top);
                [v, v, v]
            }
        }))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
    fn clean_frame(&self) -> Option<Captured> {
        Some(self.capture(self.clean_offset, false))
    }
}

fn run(screen: &FakeScreen) -> Backend {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    pipeline::run(&backend, screen);
    // Keep the temporary folders alive with the backend.
    std::mem::forget(dirs);
    backend
}

fn export(backend: &Backend) -> Rgb {
    let book = backend.stop().unwrap().book_id.unwrap();
    let out = backend.export(serde_json::from_value(serde_json::json!({"book_id": book, "format": "png"})).unwrap()).unwrap();
    decode_png(&std::fs::read(&out.files[0]).unwrap())
}

#[test]
fn a_screen_switched_off_without_scrolling_is_exported_whole_and_without_the_button() {
    let screen = FakeScreen::new(901, 0);
    screen.show(0, 2);
    let backend = run(&screen);
    let img = export(&backend);
    assert_eq!(img.h, SCREEN_H);
    for y in [2150, 2200, 2249] {
        for x in [5, 500, 870] {
            assert_eq!(img.at(x, y)[0], screen.scene.screen_px(0, SCREEN_W, x as u32, y as u32), "picture ({x}, {y}) is not the content");
        }
    }
    assert!((0..img.h).all(|y| (0..img.w).all(|x| img.at(x as i64, y as i64) != MAGENTA)), "the button was saved");
}

#[test]
fn a_hidden_button_frame_of_another_screen_is_not_used() {
    // The reader scrolled between the frame and the one without the button.
    let screen = FakeScreen::new(902, 300);
    screen.show(0, 2);
    run(&screen);
    let strips = screen.strips.lock().unwrap().clone();
    let frames = screen.frames.lock().unwrap().clone();
    assert!(strips.iter().all(|s| frames.iter().any(|f| f.0 == s.0 && f.2)), "rows came from the other screen: {strips:?}");
    assert!(strips.iter().all(|&(_, t, b)| b <= BUTTON.1 || t >= BUTTON.1 + BUTTON.3), "a strip holds the button: {strips:?}");
}
