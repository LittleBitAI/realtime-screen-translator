//! Edge cases of the thirty-fourth review: on Android a page turned without
//! any slide, drawn the same but for the words the opaque shapes cover, is
//! found by a look under the shapes after the tap; a shape moved onto the
//! screen keeps covering its text area's corners; the Windows overlay
//! follows the monitor when it changes size.

mod support;

use realtime_screen_translator::api::{CropIn, KeyIn, PlacementOut};
use realtime_screen_translator::export::{self, Rgb};
use realtime_screen_translator::layout;
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::store::PlacementRow;
use realtime_screen_translator::Backend;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::*;

#[test]
fn a_shape_moved_onto_the_screen_still_covers_its_text_areas_corners() {
    let screen = (0, 0, SCREEN_W as i64, SCREEN_H as i64);
    let (w, h) = (120u32, 200u32);
    // The review's case first (the top edge), then the other three edges and
    // a text area in the open, which keeps its rounding.
    for (x, y) in [(200, 0), (200, SCREEN_H as i64 - h as i64), (0, 900), (SCREEN_W as i64 - w as i64, 900), (200, 900)] {
        let s = layout::fit_within(x, y, w, h, "안녕", screen);
        let row = PlacementRow { id: 1, fingerprint: String::new(), x: s.x, y: s.y, w: s.w, h: s.h, radius: s.radius, font_px: s.font_px, ko: String::new() };
        let mut img = Rgb::new(SCREEN_W, SCREEN_H);
        img.px.fill(0);
        export::draw_placement(&mut img, &row);
        for py in y..y + h as i64 {
            for px in x..x + w as i64 {
                let i = (py as usize * SCREEN_W as usize + px as usize) * 3;
                assert_eq!(img.px[i], 255, "text area pixel ({px}, {py}) shows past the shape {s:?}");
            }
        }
        if (x, y) == (200, 900) {
            assert_eq!(s.radius, 13, "a shape not moved lost its rounding");
        }
    }
}

#[test]
fn the_windows_overlay_follows_the_monitor_when_it_changes() {
    let app = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs")).unwrap().replace("\r\n", "\n");
    let draw = &app[app.find("fn draw_overlay(").unwrap()..];
    let draw = &draw[..draw.find("\n}\n").unwrap()];
    let (fit, eval) = (draw.find("fit_overlay(&w)").expect("drawing does not fit the overlay to the monitor"), draw.find("w.eval(script)").unwrap());
    assert!(fit < eval, "the overlay is fitted after it is drawn on");
    let f = &app[app.find("fn fit_overlay(").unwrap()..];
    let f = &f[..f.find("\n}\n").unwrap()];
    for needle in ["primary_monitor()", "set_position(*m.position())", "set_size(*m.size())"] {
        assert!(f.contains(needle), "fit_overlay has no `{needle}`");
    }
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

/// An Android screen whose page turns, without a slide, to one drawn the
/// same but for the words in its balloon: the capture shows the opaque
/// shape over it either way, so the thumbnails never change. Only a frame
/// taken with the shapes hidden (`peek`) shows the words.
struct Reader {
    scene: Scene,
    before: Balloon,
    after: Balloon,
    turned: AtomicBool,
    touched: AtomicBool,
    peek_seq: AtomicU64,
    drawn: Mutex<Vec<Vec<PlacementOut>>>,
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
        if top as i64 > b.y || (bottom as i64) < b.y + b.h as i64 {
            return Ok(vec![]);
        }
        Ok(vec![CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) }])
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        if seq != self.peek_seq.load(Ordering::Relaxed) {
            return None;
        }
        // The screen rows under the hidden shape: the balloon as it is now.
        let b = self.shown();
        let words = export::decode_png(&b.png(0, 0)).unwrap();
        let mut rows = Rgb::new(SCREEN_W, bottom - top);
        for y in 0..rows.h as i64 {
            for x in 0..b.w as i64 {
                let (src, dst) = (((y + top as i64 - b.y) * b.w as i64 + x) as usize * 3, ((y * SCREEN_W as i64) + b.x + x) as usize * 3);
                if (0..b.h as i64).contains(&(y + top as i64 - b.y)) {
                    rows.px[dst..dst + 3].copy_from_slice(&words.px[src..src + 3]);
                }
            }
        }
        export::encode_png(&rows).ok()
    }
    fn draw(&self, placements: &[PlacementOut], _: bool) {
        self.drawn.lock().unwrap().push(placements.to_vec());
    }
    fn touched(&self) -> bool {
        self.touched.swap(false, Ordering::Relaxed)
    }
    fn peek(&self) -> Option<Captured> {
        let seq = self.peek_seq.load(Ordering::Relaxed);
        Some(Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: self.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: 0, button: None, bars: vec![] })
    }
}

fn reading(turns: bool) -> (Vec<String>, bool) {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let (before, after) = (Balloon::new(300, 700, 160, 220, 3401), Balloon::new(300, 700, 160, 220, 3402));
    gemini.read_as(before.size(), "あ");
    gemini.translate_as("あ", "가");
    let reader = Reader {
        scene: Scene::new(3401),
        before,
        after,
        turned: AtomicBool::new(false),
        touched: AtomicBool::new(false),
        peek_seq: AtomicU64::new(0),
        drawn: Mutex::default(),
    };
    let mut seq = 0;
    let mut frame = |r: &Reader| {
        seq += 1;
        r.peek_seq.store(seq * 1000, Ordering::Relaxed);
        let f = Captured { seq, width: SCREEN_W, height: SCREEN_H, thumb: r.scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: seq as i64 * 100, button: None, bars: vec![] };
        pipeline::step(&backend, r, Event::Frame(f));
    };
    let shown = |b: &Backend| b.placements(Default::default()).unwrap().placements.iter().map(|p| p.ko.clone()).collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_secs(20);
    while shown(&backend) != ["가"] {
        assert!(Instant::now() < deadline, "the first page was not translated: {:?}", shown(&backend));
        frame(&reader);
        std::thread::sleep(Duration::from_millis(20));
    }
    // A tap: on the page with other words, or on nothing.
    if turns {
        gemini.read_as(after.size(), "い");
        gemini.translate_as("い", "나");
        reader.turned.store(true, Ordering::Relaxed);
    }
    reader.touched.store(true, Ordering::Relaxed);
    let want: &[&str] = if turns { &["나"] } else { &["가"] };
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut cleared = false;
    for _ in 0..40 {
        frame(&reader);
        cleared |= reader.drawn.lock().unwrap().last().is_some_and(|d| d.is_empty());
        if turns && shown(&backend) == want {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    std::mem::forget(dirs);
    (shown(&backend), cleared)
}

#[test]
fn a_page_turned_under_opaque_shapes_is_found_by_looking_under_them() {
    let (shown, cleared) = reading(true);
    assert!(cleared, "the overlay was not cleared for the turn");
    assert_eq!(shown, ["나"], "the new page's words were not translated in place of the old");
}

#[test]
fn a_tap_on_the_same_page_turns_nothing() {
    let (shown, cleared) = reading(false);
    assert!(!cleared, "the overlay was cleared though the words under it were the same");
    assert_eq!(shown, ["가"]);
}

#[test]
fn android_tells_of_a_tap_and_hides_the_shapes_for_a_look() {
    let read = |f: &str| std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/screen/android/src/main/java").join(f)).unwrap().replace("\r\n", "\n");
    let overlay = read("Overlay.kt");
    assert!(overlay.contains("FLAG_WATCH_OUTSIDE_TOUCH"), "the button is not told of touches elsewhere");
    assert!(overlay.contains("MotionEvent.ACTION_OUTSIDE) Capture.touched()"), "a touch elsewhere is not passed on");
    assert!(overlay.contains("shapes.forEach { it.visibility = View.INVISIBLE }"), "the shapes are never hidden");
    let capture = read("Capture.kt");
    let peek = &capture[capture.find("fun peekFrame()").expect("no peekFrame")..];
    let peek = &peek[..peek.find("\n    }\n").unwrap()];
    for needle in ["Overlay.hideShapesNow()", "newestSince(hiddenAt) { it in aside }", "Overlay.restoreShapes {"] {
        assert!(peek.contains(needle), "peekFrame has no `{needle}`");
    }
    assert!(capture.contains("json.put(\"touched\", settled)"), "frames do not carry the touch");
    assert!(std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app.rs")).unwrap().contains("if n.touched {"), "Rust does not read the touch");
}
