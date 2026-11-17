//! Edge case of the thirtieth review: a crop that fails in the middle of a
//! P7 batch does not strand the ones queued before it. They are still sent
//! to Gemini and translated, and the failed one goes through once sent again.

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

fn settle(backend: &Backend, scene: &Scene, seq: &mut u64) -> FrameOut {
    let mut last = None;
    for _ in 0..2 {
        *seq += 1;
        let thumb = scene.thumb(0, SCREEN_W, SCREEN_H);
        last = Some(backend.frame(FrameIn { seq: *seq, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: *seq as i64 * 100 }).unwrap());
    }
    last.unwrap()
}

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_crop_failing_in_a_batch_does_not_strand_the_ones_before_it() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    let (a, b) = (Balloon::new(200, 400, 120, 200, 3001), Balloon::new(500, 1200, 140, 220, 3002));
    gemini.read_as(a.size(), "うん");
    gemini.translate_as("うん", "응");
    gemini.read_as(b.size(), "そうだね");
    gemini.translate_as("そうだね", "그렇지");
    let crop = |t: &Balloon| CropIn { x: t.x as u32, y: t.y as u32, w: t.w, h: t.h, kind: "dialogue".into(), score: 0.9, image: t.png(0, 0) };
    let shown = |ko: &str| backend.placements(Default::default()).unwrap().placements.iter().any(|p| p.ko == ko);
    let scene = Scene::new(3001);
    let mut seq = 0;

    // First session: B is translated and stored.
    backend.start().unwrap();
    let f = settle(&backend, &scene, &mut seq);
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(&b)] }).unwrap();
    wait_for("B was not translated", || shown("그렇지"));
    backend.stop().unwrap();

    // Next session: storing that B was seen again fails (the database is
    // held by another writer), after A was queued for Gemini.
    backend.start().unwrap();
    let f = settle(&backend, &scene, &mut seq);
    let other = rusqlite::Connection::open(dirs.data.join("translator.sqlite3")).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    let r = backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(&a), crop(&b)] });
    other.execute_batch("ROLLBACK").unwrap();
    assert!(r.is_err(), "the held database did not fail the batch: {r:?}");
    wait_for("A, queued before the crop that failed, was never translated", || shown("응"));

    // B, sent again, comes from what was stored.
    let asked = gemini.count();
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(&b)] }).unwrap();
    wait_for("B was not placed from what was stored", || shown("그렇지"));
    assert_eq!(gemini.count(), asked, "B was asked of Gemini again");
    std::mem::forget(dirs);
}
