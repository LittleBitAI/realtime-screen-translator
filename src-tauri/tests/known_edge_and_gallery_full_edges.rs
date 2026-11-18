//! Edge cases of the twenty-first review: a text area at the screen's edge
//! whose translation is already stored goes on at once, and a gallery with
//! no room left answers P11 with `storage_full`, not `storage_denied`.

mod support;

use realtime_screen_translator::api::{ApiError, CropIn, CropsIn, ExportIn, FrameIn, FrameOut, KeyIn, StripIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use support::*;

/// A device whose gallery answers with `publish_error`, if any.
struct Device {
    export: PathBuf,
    publish_error: Option<&'static str>,
}

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
        self.export.clone()
    }
    fn publish(&self, path: &Path) -> Result<String, String> {
        match self.publish_error {
            Some(e) => Err(e.to_string()),
            None => Ok(path.to_string_lossy().into_owned()),
        }
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}

fn open(dirs: &Dirs, gemini: &Gemini, publish_error: Option<&'static str>) -> Backend {
    let device = Device { export: dirs.export.clone(), publish_error };
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(device)).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend
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

#[test]
fn a_text_area_at_the_edge_already_translated_goes_on_at_once() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = open(&dirs, &gemini, None);
    let scene = Scene::new(2101);
    let text = Balloon::new(200, 800, 120, 200, 2101);
    gemini.read_as(text.size(), "うん");
    gemini.translate_as("うん", "응");
    let crop = |y: u32| CropIn { x: 200, y, w: 120, h: 200, kind: "dialogue".into(), score: 0.9, image: text.png(0, 0) };

    // First session: read in the middle of the screen and stored.
    backend.start().unwrap();
    let mut seq = 0;
    let f = settle(&backend, &scene, &mut seq);
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(800)] }).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while backend.placements(Default::default()).unwrap().placements.is_empty() {
        assert!(Instant::now() < deadline, "no translation came");
        std::thread::sleep(Duration::from_millis(20));
    }
    backend.stop().unwrap();
    let asked = gemini.count();

    // Next session, nothing moved yet: the same balloon at the top edge.
    backend.start().unwrap();
    let f = settle(&backend, &scene, &mut seq);
    let out = backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: seq, crops: vec![crop(5)] }).unwrap();
    assert_eq!(out.placed.len(), 1, "the stored translation of a text area at the edge was held");
    assert_eq!(gemini.count(), asked, "Gemini was asked again");
    std::mem::forget(dirs);
}

fn book_with_a_strip(backend: &Backend) -> i64 {
    backend.start().unwrap();
    let scene = Scene::new(2102);
    let mut seq = 0;
    let f = settle(backend, &scene, &mut seq);
    let r = f.save.expect("P6 asked for no strip");
    let image = scene.screen_png(0, SCREEN_W, SCREEN_H, r.top, r.bottom);
    backend.strip(StripIn { picture_id: f.picture_id, frame_seq: seq, top: r.top, bottom: r.bottom, image }).unwrap();
    backend.stop().unwrap().book_id.unwrap()
}

#[test]
fn a_gallery_with_no_room_left_is_storage_full() {
    for (error, code) in [("publish: storage_full", "storage_full"), ("publish: the person refused", "storage_denied")] {
        let dirs = Dirs::new();
        let gemini = Gemini::start();
        let backend = open(&dirs, &gemini, Some(error));
        let book_id = book_with_a_strip(&backend);
        let e = backend.export(ExportIn { book_id, format: "png".into() }).err();
        assert_eq!(e, Some(ApiError(code)), "{error}");
        std::mem::forget(dirs);
    }
}
