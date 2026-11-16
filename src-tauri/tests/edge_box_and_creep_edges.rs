//! Edge cases of the twentieth review: a small thing creeping a row a frame
//! over a flat page is a scroll, not a rest; and a whole text area at the
//! screen's edge is read on a page that does not scroll, while one a scroll
//! may still show whole waits for it.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, FrameOut, KeyIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::PathBuf;
use std::sync::Arc;
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

struct Run {
    dirs: Dirs,
    gemini: Gemini,
    backend: Backend,
    seq: u64,
}

impl Run {
    fn new() -> Run {
        let dirs = Dirs::new();
        let gemini = Gemini::start();
        let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
        backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
        backend.start().unwrap();
        Run { dirs, gemini, backend, seq: 0 }
    }
    fn send(&mut self, thumb: Vec<u8>) -> FrameOut {
        self.seq += 1;
        self.backend.frame(FrameIn { seq: self.seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: self.seq as i64 * 100 }).unwrap()
    }
    fn kind(&self, picture_id: i64) -> String {
        let db = rusqlite::Connection::open(self.dirs.data.join("translator.sqlite3")).unwrap();
        db.query_row("SELECT kind FROM Picture WHERE id = ?1", [picture_id], |r| r.get(0)).unwrap()
    }
    fn placements_of(&self, picture_id: i64) -> i64 {
        let db = rusqlite::Connection::open(self.dirs.data.join("translator.sqlite3")).unwrap();
        db.query_row("SELECT COUNT(*) FROM Placement WHERE picture_id = ?1", [picture_id], |r| r.get(0)).unwrap()
    }
    /// A dialogue crop at the top edge of the screen, x 200, y 5, 120 x 200.
    fn edge_crop(&self, f: &FrameOut, frame_seq: u64) {
        let text = Balloon::new(200, 5, 120, 200, 2001);
        self.gemini.read_as(text.size(), "うん");
        self.gemini.translate_as("うん", "응");
        let crop = CropIn { x: 200, y: 5, w: 120, h: 200, kind: "dialogue".into(), score: 0.9, image: text.png(0, 0) };
        self.backend.crops(CropsIn { picture_id: f.picture_id, frame_seq, crops: vec![crop] }).unwrap();
    }
    fn wait_for(&self, what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

const ROWS: usize = (SCREEN_H * THUMB_W / SCREEN_W) as usize;
const W: usize = THUMB_W as usize;

/// White with a black 20 x 20 square whose top is at thumbnail row `top`.
fn square(top: usize) -> Vec<u8> {
    let mut t = vec![255u8; W * ROWS];
    for r in top..top + 20 {
        t[r * W + 38..r * W + 58].fill(0);
    }
    t
}

#[test]
fn a_square_creeping_a_row_a_frame_is_a_scroll() {
    let mut run = Run::new();
    run.send(square(110));
    let rest = run.send(square(110));
    assert!(rest.detect.is_some());
    for k in 1..=5 {
        let f = run.send(square(110 - k));
        assert_eq!(f.motion, "scroll", "frame {k} of the creeping scroll was taken for a rest");
        assert_eq!(f.dy, (SCREEN_W / THUMB_W) as i32, "frame {k}");
        assert_eq!(f.mode, "scroll");
        assert!(f.detect.is_none() && f.save.is_none(), "detected or saved while moving");
    }
    assert_eq!(run.kind(rest.picture_id), "stitched", "the scrolled picture stayed a page");
    std::mem::forget(run.dirs);
}

#[test]
fn a_whole_text_area_at_the_edge_of_a_turned_page_is_read() {
    let mut run = Run::new();
    let (a, b) = (Scene::new(2002), Scene::new(2003));
    run.send(a.thumb(0, SCREEN_W, SCREEN_H));
    run.send(a.thumb(0, SCREEN_W, SCREEN_H));
    // Slide to the next page: the screen edges are the page's.
    assert_eq!(run.send(b.thumb(0, SCREEN_W, SCREEN_H)).motion, "page");
    let f = run.send(b.thumb(0, SCREEN_W, SCREEN_H));
    assert_eq!(f.mode, "slide");
    run.edge_crop(&f, run.seq);
    run.wait_for("the text area at the top edge of the page was not read", || !run.backend.placements(Default::default()).unwrap().placements.is_empty());
    std::mem::forget(run.dirs);
}

#[test]
fn a_text_area_at_the_edge_is_read_once_the_page_is_left_without_a_scroll() {
    let mut run = Run::new();
    let (a, b) = (Scene::new(2004), Scene::new(2005));
    run.send(a.thumb(0, SCREEN_W, SCREEN_H));
    let f = run.send(a.thumb(0, SCREEN_W, SCREEN_H));
    // Nothing moved yet: the edge may have cut it, so it waits.
    run.edge_crop(&f, run.seq);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(run.gemini.count(), 0, "a text area that may be cut was read at once");
    // The reader slides on without scrolling: the first page was whole.
    run.send(b.thumb(0, SCREEN_W, SCREEN_H));
    run.wait_for("the text area at the edge of the page left was never read", || run.placements_of(f.picture_id) == 1);
    std::mem::forget(run.dirs);
}

#[test]
fn a_text_area_at_the_edge_waits_for_the_scroll_that_shows_it_whole() {
    let mut run = Run::new();
    let scene = Scene::new(2006);
    run.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = run.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    run.edge_crop(&f, run.seq);
    // A scroll up: the text area comes away from the edge and is detected
    // there again; the cut one held at the edge is dropped.
    assert_eq!(run.send(scene.thumb(-300, SCREEN_W, SCREEN_H)).motion, "scroll");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(run.gemini.count(), 0, "the text area held at the edge was read after the scroll");
    assert_eq!(run.placements_of(f.picture_id), 0);
    std::mem::forget(run.dirs);
}
