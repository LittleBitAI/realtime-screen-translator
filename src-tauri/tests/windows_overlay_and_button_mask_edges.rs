//! Edge cases of the thirty-third review: the Windows overlay draws the
//! placements itself (A7, R28) from an inline page Rust runs scripts in, and
//! the floating button's black block, which stays put over a scrolling page,
//! is left out of the comparisons instead of making a scroll look still.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::overlay;
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap().replace("\r\n", "\n")
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn the_windows_overlay_page_draws_white_shapes_with_the_korean_in_them() {
    let url = overlay::data_url();
    let body = url.strip_prefix("data:text/html;charset=utf-8,").expect("not an HTML data URL");
    assert!(!body.contains(' ') && !body.contains('#') && !body.contains('\n'), "the data URL is not encoded whole");
    assert_eq!(percent_decode(body), overlay::HTML, "the URL does not carry the page");
    let page = overlay::HTML;
    for needle in [
        "function draw(list)",
        "background:white",
        "replaceChildren()",
        "window.devicePixelRatio",
        "p.x/r",
        "p.y/r",
        "p.w/r",
        "p.h/r",
        "p.radius/r",
        "t.textContent=p.ko",
        "size=p.font_px",
        "line-height:1.2",
        "word-break:break-all",
        "t.offsetHeight>room",
    ] {
        assert!(page.contains(needle), "the overlay page has no `{needle}`");
    }
    let p = json!([{"id": 1, "x": 10, "y": 20, "w": 120, "h": 60, "radius": 13, "ko": "가나 \"다\"", "font_px": 16}]);
    let s = overlay::script(&p);
    assert_eq!(s, format!("draw({p})"));
    assert_eq!(overlay::script(&json!(null)), "draw([])", "anything but a list does not clear the overlay");
}

#[test]
fn the_app_draws_on_the_windows_overlay_instead_of_only_sending_an_event() {
    let app = read("src/app.rs");
    assert!(!app.contains("index.html?overlay"), "the overlay still waits for a frontend page");
    assert!(app.contains("crate::overlay::data_url()"), "the overlay window does not load its own page");
    assert!(app.contains("w.eval(script)"), "nothing is drawn on the overlay");
    assert!(app.contains("w.eval(last_drawn().clone())"), "what was drawn before the page loaded is lost");
    let emit = &app[app.find("fn emit(&self").unwrap()..];
    assert!(emit[..emit.find("self.app.emit(event").unwrap()].contains("draw_overlay(&self.app, &payload[\"placements\"])"), "a late translation is not drawn on Windows");
    let wgc = read("src/wgc.rs");
    let draw = &wgc[wgc.find("fn draw(&self").unwrap()..];
    assert!(draw[..draw.find("\n    }").unwrap()].contains("crate::app::draw_overlay("), "frames do not draw on the overlay");
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
    // The overlay is not in the capture: only the button is drawn over the page.
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

/// The balloon at screen (100, 400), found in frame 2; what is drawn.
#[derive(Default)]
struct Drawn(Mutex<Vec<Vec<PlacementOut>>>);

fn balloon() -> Balloon {
    Balloon::new(100, 400, 120, 120, 3301)
}

impl Screen for Drawn {
    fn next(&self) -> Option<Event> {
        None
    }
    fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        let b = balloon();
        if seq != 2 || top > 400 || bottom < 520 {
            return Ok(vec![]);
        }
        Ok(vec![CropIn { x: 100, y: 400, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) }])
    }
    fn strip(&self, _: u64, _: u32, _: u32) -> Option<Vec<u8>> {
        None
    }
    fn draw(&self, placements: &[PlacementOut], _: bool) {
        self.0.lock().unwrap().push(placements.to_vec());
    }
}

/// The review's screen: white, with one black block at columns 38 .. 47 and
/// thumbnail rows `top .. top + 10`.
fn white_page(top: usize) -> Vec<u8> {
    let w = THUMB_W as usize;
    let mut t = vec![255u8; w * 240];
    for r in top..top + 10 {
        t[r * w + 38..r * w + 48].fill(0);
    }
    t
}

#[test]
fn the_buttons_black_block_does_not_make_a_scroll_look_still() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let screen = Drawn::default();
    let frame = |seq: u64, top: usize| {
        let f = Captured { seq, width: 960, height: 2400, thumb: white_page(top), captured_at: seq as i64 * 100, button: Some((800, 2150, 140, 100)), bars: vec![] };
        pipeline::step(&backend, &screen, Event::Frame(f));
    };
    gemini.read_as(balloon().size(), "あ");
    gemini.translate_as("あ", "가");
    frame(1, 110);
    frame(2, 110);
    let deadline = Instant::now() + Duration::from_secs(20);
    while backend.placements(Default::default()).unwrap().placements.is_empty() {
        assert!(Instant::now() < deadline, "the translation did not come");
        std::thread::sleep(Duration::from_millis(20));
    }
    let before = backend.placements(Default::default()).unwrap().placements[0].y;
    // The page moves one thumbnail row (10 px) up; the button stays.
    frame(3, 109);
    let drawn = screen.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(drawn.iter().map(|p| p.y).collect::<Vec<_>>(), vec![before - 10], "the translation did not follow the 10 px scroll");
    std::mem::forget(dirs);
}
