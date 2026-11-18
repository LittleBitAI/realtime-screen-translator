//! Edge case of the thirty-sixth review: a translation that was stored but
//! whose placement could not be written is not lost. Its rows are detected
//! again, and the crop then places the stored translation without asking
//! Gemini again.

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
    // The overlay is not in the capture: a frame shows the page as it is.
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

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_placement_that_could_not_be_written_is_placed_once_its_rows_come_again() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    let balloon = Balloon::new(300, 700, 160, 220, 3601);
    gemini.read_as(balloon.size(), "あ");
    gemini.translate_as("あ", "가");
    let scene = Scene::new(3601);
    let mut seq = 0;
    let mut frame = |offset: i64| -> FrameOut {
        seq += 1;
        backend.frame(FrameIn { seq, width: SCREEN_W, height: SCREEN_H, thumb: scene.thumb(offset, SCREEN_W, SCREEN_H), captured_at: seq as i64 * 100 }).unwrap()
    };
    frame(0);
    let f = frame(0);

    // The review's case: every placement write fails while the answer comes.
    let db = rusqlite::Connection::open(dirs.data.join("translator.sqlite3")).unwrap();
    db.execute_batch("CREATE TRIGGER fail_placement BEFORE INSERT ON Placement BEGIN SELECT RAISE(FAIL, 'placement write failed'); END;").unwrap();
    let crop = |y: i64| CropIn { x: balloon.x as u32, y: y as u32, w: balloon.w, h: balloon.h, kind: "dialogue".into(), score: 0.9, image: balloon.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop(balloon.y)] }).unwrap();
    let stored = || db.query_row("SELECT count(*) FROM Translation WHERE status = 'ready'", [], |r| r.get::<_, i64>(0)).unwrap() == 1;
    wait_for("the translation was not stored", stored);
    // The placement is tried in the same step, right after.
    std::thread::sleep(Duration::from_millis(300));
    assert!(backend.placements(Default::default()).unwrap().placements.is_empty(), "the failing trigger did not fail the placement");
    db.execute_batch("DROP TRIGGER fail_placement").unwrap();
    let asked = gemini.count();

    // A small scroll: the balloon's rows are detected again.
    let offset = 10;
    let mut again = None;
    for _ in 0..6 {
        let f = frame(offset);
        if let Some(d) = f.detect.filter(|d| (d.top as i64) <= balloon.y - offset && (d.bottom as i64) >= balloon.y - offset + balloon.h as i64) {
            again = Some((f, d));
            break;
        }
    }
    let (f, _) = again.expect("the rows of the placement that failed were never detected again");
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(balloon.y - offset)] }).unwrap();
    wait_for("the stored translation was not placed", || backend.placements(Default::default()).unwrap().placements.iter().any(|p| p.ko == "가"));
    assert_eq!(gemini.count(), asked, "Gemini was asked again for a stored translation");
    std::mem::forget(dirs);
}
