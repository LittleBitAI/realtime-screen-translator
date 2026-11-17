//! Edge cases of the twentieth review: a small thing creeping a row a frame
//! over a flat page is a scroll, not a rest; and a text area at the screen's
//! edge is read at once, its translation giving way to the whole one if a
//! scroll later shows it was cut.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, FrameOut, KeyIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::backend::OVERLAY_LAG;
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
fn a_whole_text_area_at_the_edge_of_the_first_screen_is_read_at_once() {
    let mut run = Run::new();
    let scene = Scene::new(2004);
    run.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = run.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    // Nothing has moved since translation was switched on.
    run.edge_crop(&f, run.seq);
    run.wait_for("the text area at the edge of a resting screen was not read", || run.placements_of(f.picture_id) == 1);
    std::mem::forget(run.dirs);
}

#[test]
fn a_text_area_cut_by_the_edge_gives_way_to_the_whole_one_a_scroll_shows() {
    let mut run = Run::new();
    let scene = Scene::new(2006);
    run.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = run.send(scene.thumb(0, SCREEN_W, SCREEN_H));
    // Screen rows 2200 .. 2400 of a text area 400 rows tall: the bottom edge cuts it.
    let cut = Balloon::new(200, 2200, 120, 200, 2007);
    let whole = Balloon::new(200, 1900, 120, 400, 2008);
    run.gemini.read_as(cut.size(), "うん");
    run.gemini.translate_as("うん", "응");
    run.gemini.read_as(whole.size(), "そうだね");
    run.gemini.translate_as("そうだね", "그렇지");
    let crop = |b: &Balloon, y: u32| CropIn { x: 200, y, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    run.backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: run.seq, crops: vec![crop(&cut, 2200)] }).unwrap();
    run.wait_for("the cut text area was not read", || run.placements_of(f.picture_id) == 1);
    // 300 px further down the whole text area is on screen at 1900 .. 2300.
    assert_eq!(run.send(scene.thumb(300, SCREEN_W, SCREEN_H)).motion, "scroll");
    let g = run.send(scene.thumb(300, SCREEN_W, SCREEN_H));
    assert_eq!(g.picture_id, f.picture_id);
    // This frame shows the cut one's white shape over the balloon (the
    // capture holds the overlay): it goes, and the Japanese is read from a
    // frame taken once the shape is gone.
    let rows = |f: &FrameOut| f.detect.as_ref().is_some_and(|r| r.top <= 1900 && r.bottom >= 2300);
    let held = run.backend.crops(CropsIn { picture_id: g.picture_id, frame_seq: run.seq, crops: vec![crop(&whole, 1900)] }).unwrap();
    assert_eq!(held.pending, 1);
    assert!(run.backend.placements(Default::default()).unwrap().placements.is_empty(), "the cut translation stayed");
    // The next frames were taken before the capture caught up with the
    // overlay drawn without the shape.
    for k in 1..=OVERLAY_LAG {
        let h = run.send(scene.thumb(300, SCREEN_W, SCREEN_H));
        assert!(h.placements.is_empty() && rows(&h), "frame {k}: the text area was not asked for again: {:?}", h.detect);
        run.backend.crops(CropsIn { picture_id: h.picture_id, frame_seq: run.seq, crops: vec![crop(&whole, 1900)] }).unwrap();
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(run.gemini.count(), 2, "a crop that may show the white shape went to Gemini");
    let i = run.send(scene.thumb(300, SCREEN_W, SCREEN_H));
    assert!(rows(&i), "the text area was not asked for again: {:?}", i.detect);
    run.backend.crops(CropsIn { picture_id: i.picture_id, frame_seq: run.seq, crops: vec![crop(&whole, 1900)] }).unwrap();
    run.wait_for("the cut translation was not replaced by the whole one", || {
        let shown = run.backend.placements(Default::default()).unwrap().placements;
        shown.len() == 1 && shown[0].ko == "그렇지"
    });
    assert_eq!(run.placements_of(f.picture_id), 1, "the cut one stayed in the translated picture");
    std::mem::forget(run.dirs);
}
