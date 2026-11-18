//! Edge case of the sixteenth review: an image the mirror composed before
//! the button was hidden, arriving late, is handed over as the clean frame.
//! It still shows the button, so its rows behind the button are not saved.

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

/// Every frame holds the button, the one `clean_frame` hands over too (the
/// late image from before the hiding); `clean_holds_button` false makes it
/// a truly clean one.
struct FakeScreen {
    scene: Scene,
    clean_holds_button: bool,
    seq: Mutex<u64>,
    /// seq -> holds the button
    frames: Mutex<Vec<(u64, bool)>>,
    events: Mutex<VecDeque<Event>>,
    strips: Mutex<Vec<(u64, u32, u32)>>,
}

impl FakeScreen {
    fn new(seed: u64, clean_holds_button: bool) -> FakeScreen {
        let screen = FakeScreen {
            scene: Scene::new(seed),
            clean_holds_button,
            seq: Mutex::new(0),
            frames: Mutex::default(),
            events: Mutex::default(),
            strips: Mutex::default(),
        };
        for _ in 0..2 {
            let f = screen.capture(true);
            screen.events.lock().unwrap().push_back(Event::Frame(f));
        }
        screen
    }
    fn capture(&self, button: bool) -> Captured {
        let mut seq = self.seq.lock().unwrap();
        *seq += 1;
        self.frames.lock().unwrap().push((*seq, button));
        let mut thumb = self.scene.thumb(0, SCREEN_W, SCREEN_H);
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
        let (_, button) = *self.frames.lock().unwrap().iter().find(|f| f.0 == seq)?;
        Some(png_rgb(SCREEN_W, bottom - top, |x, y| {
            let y = y + top;
            if button && x >= BUTTON.0 && x < BUTTON.0 + BUTTON.2 && y >= BUTTON.1 && y < BUTTON.1 + BUTTON.3 {
                MAGENTA
            } else {
                let v = self.scene.screen_px(0, SCREEN_W, x, y);
                [v, v, v]
            }
        }))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
    fn clean_frame(&self) -> Option<Captured> {
        Some(self.capture(self.clean_holds_button))
    }
}

fn run(screen: &FakeScreen) -> Backend {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    pipeline::run(&backend, screen);
    std::mem::forget(dirs);
    backend
}

fn export(backend: &Backend) -> Rgb {
    let book = backend.stop().unwrap().book_id.unwrap();
    let out = backend.export(serde_json::from_value(serde_json::json!({"book_id": book, "format": "png"})).unwrap()).unwrap();
    decode_png(&std::fs::read(&out.files[0]).unwrap())
}

#[test]
fn a_late_image_that_still_shows_the_button_is_not_saved_as_clean() {
    let screen = FakeScreen::new(1601, true);
    let backend = run(&screen);
    let strips = screen.strips.lock().unwrap().clone();
    assert!(strips.iter().all(|&(_, t, b)| b <= BUTTON.1 || t >= BUTTON.1 + BUTTON.3), "a strip holds the button: {strips:?}");
    let img = export(&backend);
    assert!((0..img.h).all(|y| (0..img.w).all(|x| img.at(x as i64, y as i64) != MAGENTA)), "the button was saved over the page");
}

#[test]
fn a_frame_with_the_button_gone_still_fills_the_rows_behind_it() {
    let screen = FakeScreen::new(1602, false);
    let backend = run(&screen);
    let img = export(&backend);
    assert_eq!(img.h, SCREEN_H);
    for (x, y) in [(870, 2200), (5, 2200), (500, 100)] {
        assert_eq!(img.at(x, y)[0], screen.scene.screen_px(0, SCREEN_W, x as u32, y as u32), "picture ({x}, {y}) is not the content");
    }
}
