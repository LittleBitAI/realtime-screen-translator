//! Edge cases of the fourth review: stop when the database cannot be
//! written, a picture whose stored range lags its strips, our own overlay in
//! the capture, and line breaks in a translation.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, ExportIn, FrameIn, FrameOut, KeyIn, StripIn};
use realtime_screen_translator::layout::{self, PAD};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

#[derive(Default)]
struct Device {
    export: PathBuf,
    stopped: AtomicBool,
}

impl Platform for Device {
    fn request_permissions(&self) -> Permissions {
        self.permissions()
    }
    fn permissions(&self) -> Permissions {
        Permissions { capture: true, overlay: true }
    }
    fn start(&self) -> Result<(), String> {
        self.stopped.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
    fn emit(&self, _: &str, _: serde_json::Value) {}
    fn export_dir(&self) -> PathBuf {
        self.export.clone()
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}

struct On {
    backend: Backend,
    device: Arc<Device>,
    seq: u64,
}

fn on(dirs: &Dirs, gemini_url: &str) -> On {
    let device = Arc::new(Device { export: dirs.export.clone(), ..Default::default() });
    let backend = Backend::open(dirs.data.clone(), Some(gemini_url.to_string()), device.clone()).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    On { backend, device, seq: 0 }
}

impl On {
    fn send(&mut self, thumb: Vec<u8>) -> FrameOut {
        self.seq += 1;
        let f = FrameIn { seq: self.seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: self.seq as i64 * 100 };
        self.backend.frame(f).unwrap()
    }
}

fn db(dirs: &Dirs) -> rusqlite::Connection {
    rusqlite::Connection::open(dirs.data.join("translator.sqlite3")).unwrap()
}

#[test]
fn stop_takes_capture_and_overlay_down_even_when_the_book_cannot_be_closed() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let mut app = on(&dirs, &gemini.url);
    let scene = Scene::new(401);
    app.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    // Another writer holds the database: closing the book fails.
    let other = db(&dirs);
    other.execute_batch("BEGIN EXCLUSIVE").unwrap();
    assert!(app.backend.stop().is_err(), "the book was closed through a locked database");
    assert!(app.device.stopped.load(Ordering::SeqCst), "capture and the overlay stayed up");
    assert!(!app.backend.status().on);
    other.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn a_picture_whose_stored_range_lags_its_strips_is_exported_whole() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let mut app = on(&dirs, &gemini.url);
    let scene = Scene::new(402);
    app.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = app.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let r = f.save.expect("P6 asked for no strip");
    let image = scene.screen_png(0, SCREEN_W, SCREEN_H, r.top, r.bottom);
    app.backend.strip(StripIn { picture_id: f.picture_id, frame_seq: app.seq, top: r.top, bottom: r.bottom, image }).unwrap();
    let book_id = app.backend.stop().unwrap().book_id.unwrap();
    // As left by a kill between the strip and the range (before they were one commit).
    db(&dirs).execute("UPDATE Picture SET covered_top = 0, covered_bottom = 0", []).unwrap();

    let books = app.backend.books(serde_json::from_value(serde_json::json!({"limit": 5})).unwrap()).unwrap();
    assert_eq!(books.books[0].pictures[0].height, 2400);
    let out = app.backend.export(ExportIn { book_id, format: "png".into() }).unwrap();
    let img = decode_png(&std::fs::read(&out.files[0]).unwrap());
    assert_eq!((img.w, img.h), (SCREEN_W, 2400));
}

/// `thumb` with our overlay drawn in it: white over each placement (screen px).
fn with_overlay(mut thumb: Vec<u8>, placements: &[(i32, i32, u32, u32)]) -> Vec<u8> {
    let s = (SCREEN_W / THUMB_W) as i32;
    let rows = thumb.len() as i32 / THUMB_W as i32;
    for &(x, y, w, h) in placements {
        for r in (y / s).max(0)..((y + h as i32) / s).min(rows) {
            for c in (x / s).max(0)..((x + w as i32) / s).min(THUMB_W as i32) {
                thumb[(r * THUMB_W as i32 + c) as usize] = 255;
            }
        }
    }
    thumb
}

#[test]
fn our_overlay_in_the_capture_is_neither_a_page_turn_nor_a_scroll() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let mut app = on(&dirs, &gemini.url);
    let scene = Scene::new(403);
    app.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = app.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let balloons = [Balloon::new(100, 300, 400, 600, 4031), Balloon::new(500, 1300, 420, 580, 4032)];
    let crops = balloons
        .iter()
        .map(|b| CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) })
        .collect();
    app.backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: app.seq, crops }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let placed = loop {
        let p = app.backend.placements(Default::default()).unwrap().placements;
        if p.len() == 2 {
            break p;
        }
        assert!(Instant::now() < deadline, "no translations came back");
        std::thread::sleep(Duration::from_millis(50));
    };
    let rects: Vec<_> = placed.iter().map(|p| (p.x, p.y, p.w, p.h)).collect();

    // The translations appear on a still screen.
    let shown = app.send(with_overlay(scene.thumb(0, SCREEN_W, SCREEN_H), &rects));
    assert_eq!((shown.motion, shown.clear), ("still", false), "the arriving overlay was taken for a change");
    assert_eq!(shown.picture_id, f.picture_id);
    // The page scrolls by 100 px; the overlay in this capture is still where it was drawn.
    let moved = app.send(with_overlay(scene.thumb(100, SCREEN_W, SCREEN_H), &rects));
    assert_eq!((moved.motion, moved.dy, moved.picture_id), ("scroll", 100, f.picture_id));
    let after: Vec<_> = moved.placements.iter().map(|p| (p.x, p.y, p.w, p.h)).collect();
    let rest = app.send(with_overlay(scene.thumb(100, SCREEN_W, SCREEN_H), &after));
    assert_eq!(rest.motion, "still");
}

#[test]
fn line_breaks_in_a_translation_count_as_lines_of_the_shape() {
    let ko = "가\n나\n다\n라\n마";
    let s = layout::fit(0, 0, 120, 40, ko);
    let (font, inner_w) = (s.font_px as u32, s.w - 2 * PAD);
    let lines = layout::lines(ko, font, inner_w);
    assert_eq!(lines.len(), 5);
    assert!(layout::text_height(lines.len(), font) <= s.h - 2 * PAD, "{s:?} holds no five lines");
    assert!(lines.iter().all(|l| l.len() as u32 * font <= inner_w));
}
