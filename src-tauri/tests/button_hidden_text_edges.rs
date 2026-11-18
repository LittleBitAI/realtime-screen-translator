//! Edge case of the twenty-third review: a text area behind the floating
//! button is not taken as looked at. It is looked for in the frame taken
//! with the button hidden, or, without one, asked for again once a scroll
//! brings it out from behind the button.

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

/// Frames at the queued scroll offsets, every one holding the button. The
/// frame `clean_frame` hands over is truly clean only if `clean_works`.
struct FakeScreen {
    scene: Scene,
    clean_works: bool,
    seq: Mutex<u64>,
    offset: Mutex<i64>,
    /// seq -> holds the button
    frames: Mutex<Vec<(u64, bool)>>,
    offsets: Mutex<VecDeque<i64>>,
    /// (seq of the frame looked at, holds the button, top, bottom)
    detects: Mutex<Vec<(u64, bool, u32, u32)>>,
}

impl FakeScreen {
    fn new(seed: u64, clean_works: bool, offsets: &[i64]) -> FakeScreen {
        FakeScreen {
            scene: Scene::new(seed),
            clean_works,
            seq: Mutex::new(0),
            offset: Mutex::new(0),
            frames: Mutex::default(),
            offsets: Mutex::new(offsets.iter().copied().collect()),
            detects: Mutex::default(),
        }
    }
    fn capture(&self, button: bool) -> Captured {
        let mut seq = self.seq.lock().unwrap();
        *seq += 1;
        self.frames.lock().unwrap().push((*seq, button));
        let mut thumb = self.scene.thumb(*self.offset.lock().unwrap(), SCREEN_W, SCREEN_H);
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
        let o = self.offsets.lock().unwrap().pop_front()?;
        *self.offset.lock().unwrap() = o;
        Some(Event::Frame(self.capture(true)))
    }
    fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        let (_, button) = *self.frames.lock().unwrap().iter().find(|f| f.0 == seq).unwrap();
        self.detects.lock().unwrap().push((seq, button, top, bottom));
        Ok(vec![])
    }
    fn strip(&self, _: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        Some(png_rgb(SCREEN_W, bottom - top, |_, _| [128, 128, 128]))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
    fn clean_frame(&self) -> Option<Captured> {
        Some(self.capture(!self.clean_works))
    }
}

fn run(screen: &FakeScreen) {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    pipeline::run(&backend, screen);
    std::mem::forget(dirs);
}

#[test]
fn the_rows_behind_the_button_are_looked_at_in_the_frame_without_it() {
    let screen = FakeScreen::new(2301, true, &[0, 0]);
    run(&screen);
    let detects = screen.detects.lock().unwrap().clone();
    let first = *detects.first().expect("nothing was looked at");
    assert!(first.2 <= BUTTON.1 && first.3 >= BUTTON.1 + BUTTON.3, "{detects:?}");
    assert!(!first.1, "the text areas were looked for in a frame holding the button");
}

#[test]
fn without_a_clean_frame_the_rows_behind_the_button_are_looked_at_once_scrolled_out() {
    // Rest, then 400 px down: what was behind the button is at 1750 .. 1850.
    let screen = FakeScreen::new(2302, false, &[0, 0, 200, 400, 400, 400]);
    run(&screen);
    let detects = screen.detects.lock().unwrap().clone();
    let rest = detects.iter().position(|d| d.2 <= BUTTON.1 && d.3 >= BUTTON.1 + BUTTON.3).expect("the first screen was not looked at");
    let (top, bottom) = (BUTTON.1 - 400, BUTTON.1 + BUTTON.3 - 400);
    assert!(
        detects[rest + 1..].iter().any(|d| d.2 <= top && d.3 >= bottom),
        "the rows that were behind the button were not looked at once out: {detects:?}"
    );
}
