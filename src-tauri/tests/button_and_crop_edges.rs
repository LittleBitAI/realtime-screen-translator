//! Edge cases of the eighth review: our own floating button in the Android
//! capture is never saved into a strip, and P7 takes only dialogue crops on
//! the captured screen.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, KeyIn, PlacementOut, Range};
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

fn open(dirs: &Dirs, gemini: &Gemini) -> Backend {
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend
}

/// The button: right edge, screen rows 2150 .. 2250.
const BUTTON: (u32, u32, u32, u32) = (800, 2150, 140, 100);

struct FakeScreen {
    scene: Scene,
    offsets: Mutex<Vec<(u64, i64)>>,
    events: Mutex<VecDeque<Event>>,
    strips: Mutex<Vec<(u32, u32)>>,
}

impl Screen for FakeScreen {
    fn next(&self) -> Option<Event> {
        self.events.lock().unwrap().pop_front()
    }
    fn detect(&self, _: u64, _: u32, _: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        Ok(vec![])
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        self.strips.lock().unwrap().push((top, bottom));
        let off = self.offsets.lock().unwrap().iter().find(|(s, _)| *s == seq).unwrap().1;
        Some(self.scene.screen_png(off, SCREEN_W, SCREEN_H, top, bottom))
    }
    fn draw(&self, _: &[PlacementOut], _: bool) {}
}

impl FakeScreen {
    fn show(&self, seq: &mut u64, offset: i64, n: usize) {
        for _ in 0..n {
            *seq += 1;
            self.offsets.lock().unwrap().push((*seq, offset));
            let thumb = self.scene.thumb(offset, SCREEN_W, SCREEN_H);
            let f = Captured { seq: *seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: *seq as i64 * 100, button: Some(BUTTON) };
            self.events.lock().unwrap().push_back(Event::Frame(f));
        }
    }
}

#[test]
fn rows_behind_the_floating_button_are_saved_only_once_they_scroll_clear_of_it() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = open(&dirs, &gemini);
    backend.start().unwrap();
    let screen = FakeScreen { scene: Scene::new(801), offsets: Mutex::default(), events: Mutex::default(), strips: Mutex::default() };
    let mut seq = 0;
    screen.show(&mut seq, 0, 2);
    // Scrolled 300 px down and resting: three frames of the same screen.
    screen.show(&mut seq, 300, 4);
    pipeline::run(&backend, &screen);

    let strips = screen.strips.lock().unwrap().clone();
    let (b0, b1) = (BUTTON.1, BUTTON.1 + BUTTON.3);
    assert!(strips.iter().all(|&(t, b)| b <= b0 || t >= b1), "a strip holds the button: {strips:?}");
    assert_eq!(&strips[..2], &[(0, b0), (b1, 2400)], "the first screen around the button");
    // Picture rows 2150 .. 2250, behind the button on the first screen, are screen rows 1850 .. 1950 now.
    assert!(strips.contains(&(b0 - 300, b1 - 300)), "the rows the button hid were never saved: {strips:?}");

    let book = backend.stop().unwrap().book_id.unwrap();
    let out = backend.export(serde_json::from_value(serde_json::json!({"book_id": book, "format": "png"})).unwrap()).unwrap();
    let img = decode_png(&std::fs::read(&out.files[0]).unwrap());
    for x in [5, 870] {
        assert_eq!(img.at(x, 2200)[0], screen.scene.screen_px(0, SCREEN_W, x as u32, 2200), "picture row 2200 is not the content at x {x}");
    }
}

#[test]
fn clear_of_keeps_the_rows_around_the_button() {
    let r = Range { top: 2100, bottom: 2400 };
    assert_eq!(pipeline::clear_of(r, None), vec![(2100, 2400)]);
    assert_eq!(pipeline::clear_of(r, Some(BUTTON)), vec![(2100, 2150), (2250, 2400)]);
    assert_eq!(pipeline::clear_of(Range { top: 0, bottom: 2000 }, Some(BUTTON)), vec![(0, 2000)]);
    assert_eq!(pipeline::clear_of(Range { top: 2160, bottom: 2240 }, Some(BUTTON)), vec![]);
}

#[test]
fn crops_of_another_kind_or_off_the_screen_are_not_translated() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = open(&dirs, &gemini);
    backend.start().unwrap();
    let scene = Scene::new(802);
    let mut f = None;
    for seq in 1..=2 {
        f = Some(backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: scene.thumb(0, SCREEN_W, SCREEN_H), captured_at: seq as i64 * 100 }).unwrap());
    }
    let f = f.unwrap();
    let png = png_rgb(16, 16, |x, y| [(x * 13) as u8, (y * 11) as u8, 90]);
    let crop = |x, y, kind: &str| CropIn { x, y, w: 16, h: 16, kind: kind.into(), score: 0.9, image: png.clone() };
    let out = backend
        .crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop(2000, 100, "dialogue"), crop(100, 2390, "dialogue"), crop(100, 100, "other")] })
        .unwrap();
    assert_eq!((out.discarded, out.pending, out.placed.len()), (3, 0, 0));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(gemini.count(), 0, "a crop that is no balloon on the screen went to Gemini");
}
