//! Edge cases of the fourteenth and fifteenth reviews: a dark page with one
//! white balloon is a picture, not a blocked capture, wherever the balloon
//! is; a black capture with only the system bars (where the platform
//! reports them) and our own floating button lit still is blocked.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use support::*;

const ROWS: usize = (SCREEN_H * THUMB_W / SCREEN_W) as usize;
const W: usize = THUMB_W as usize;

/// Black with a white balloon at thumbnail `rows`, `cols`, dark text in it.
fn dark_page(rows: std::ops::Range<usize>, cols: std::ops::Range<usize>) -> Vec<u8> {
    let mut t = vec![0u8; W * ROWS];
    for r in rows {
        for c in cols.clone() {
            t[r * W + c] = if r % 4 == 1 && (cols.start + 4..cols.end - 4).contains(&c) { 20 } else { 250 };
        }
    }
    t
}

/// The status bar (screen rows 0..80) and the navigation bar (2300..2400),
/// as the platform reports them.
const BARS: [(u32, u32, u32, u32); 2] = [(0, 0, SCREEN_W, 80), (0, 2300, SCREEN_W, 100)];

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

/// Sends `thumb` with the floating button at `button`; records what P6 asked for.
struct FakeScreen {
    events: Mutex<VecDeque<Event>>,
    detected: Mutex<Vec<(u32, u32)>>,
    strips: Mutex<Vec<(u32, u32)>>,
}

impl Screen for FakeScreen {
    fn next(&self) -> Option<Event> {
        self.events.lock().unwrap().pop_front()
    }
    fn detect(&self, _: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        self.detected.lock().unwrap().push((top, bottom));
        Ok(vec![])
    }
    fn strip(&self, _: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        self.strips.lock().unwrap().push((top, bottom));
        Some(png_rgb(SCREEN_W, bottom - top, |_, _| [0, 0, 0]))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
}

fn run(thumb: Vec<u8>, button: Option<(u32, u32, u32, u32)>, bars: &[(u32, u32, u32, u32)]) -> (Backend, FakeScreen) {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let screen = FakeScreen { events: Mutex::default(), detected: Mutex::default(), strips: Mutex::default() };
    for seq in 1..=3 {
        let f = Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: thumb.clone(), captured_at: seq as i64 * 100, button, bars: bars.to_vec() };
        screen.events.lock().unwrap().push_back(Event::Frame(f));
    }
    pipeline::run(&backend, &screen);
    std::mem::forget(dirs);
    (backend, screen)
}

fn assert_read_and_saved(backend: &Backend, screen: &FakeScreen) {
    assert_eq!(backend.status().notice, None, "the dark page was taken for a blocked capture");
    assert_eq!(*screen.detected.lock().unwrap(), vec![(0, SCREEN_H)], "the dark page was not detected");
    assert_eq!(*screen.strips.lock().unwrap(), vec![(0, SCREEN_H)], "the dark page was not saved");
}

#[test]
fn a_dark_page_with_one_white_balloon_is_read_and_saved() {
    let (backend, screen) = run(dark_page(90..120, 30..50), None, &BARS);
    assert_read_and_saved(&backend, &screen);
}

#[test]
fn a_dark_page_with_its_balloon_at_the_top_edge_is_read_and_saved() {
    // Rows 8..18 of 240: where a status bar would be, but none is reported
    // (a reader app showing full screen).
    let (backend, screen) = run(dark_page(8..18, 30..60), None, &[]);
    assert_read_and_saved(&backend, &screen);
    // A balloon just under a status bar that shows is a picture as well.
    let (backend, screen) = run(dark_page(10..20, 30..60), None, &BARS);
    assert_read_and_saved(&backend, &screen);
}

#[test]
fn a_black_capture_with_the_bars_and_our_button_lit_is_blocked() {
    let mut t = vec![0u8; W * ROWS];
    for r in 1..7 {
        for c in (0..W).step_by(3) {
            t[r * W + c] = 230; // status bar clock and icons
        }
    }
    for r in ROWS - 9..ROWS - 1 {
        for c in (30..66).step_by(2) {
            t[r * W + c] = 200; // navigation bar buttons
        }
    }
    // The floating button, above the navigation bar near the right edge
    // (screen 800..940 x 1990..2090).
    let button = (800, 1990, 140, 100);
    for r in 199..209 {
        for c in 80..94 {
            t[r * W + c] = 180;
        }
    }
    let (backend, screen) = run(t, Some(button), &BARS);
    assert_eq!(backend.status().notice, Some("capture_blocked"));
    assert!(screen.detected.lock().unwrap().is_empty() && screen.strips.lock().unwrap().is_empty(), "a blocked capture was read or saved");
}
