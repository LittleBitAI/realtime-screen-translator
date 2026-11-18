//! Edge cases of the twelfth review: strips are kept as lossless WebP under
//! a path relative to the app's folder (E5), and a white shape grown to hold
//! a long translation never lands over half of one already shown (E6).

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, ExportIn, FrameIn, KeyIn, StripIn};
use realtime_screen_translator::export;
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

fn on(dirs: &Dirs, gemini: &Gemini) -> Backend {
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    backend.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    backend.start().unwrap();
    backend
}

fn still(backend: &Backend, scene: &Scene) -> realtime_screen_translator::api::FrameOut {
    let thumb = scene.thumb(0, SCREEN_W, SCREEN_H);
    backend.frame(FrameIn { seq: 1, width: SCREEN_W, height: SCREEN_H, thumb: thumb.clone(), captured_at: 100 }).unwrap();
    backend.frame(FrameIn { seq: 2, width: SCREEN_W, height: SCREEN_H, thumb, captured_at: 200 }).unwrap()
}

#[test]
fn a_strip_is_kept_as_lossless_webp_under_a_relative_path() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = on(&dirs, &gemini);
    let scene = Scene::new(1201);
    let f = still(&backend, &scene);
    let r = f.save.expect("P6 asked for no strip");
    let image = scene.screen_png(0, SCREEN_W, SCREEN_H, r.top, r.bottom);
    backend.strip(StripIn { picture_id: f.picture_id, frame_seq: 2, top: r.top, bottom: r.bottom, image: image.clone() }).unwrap();

    let db = rusqlite::Connection::open(dirs.data.join("translator.sqlite3")).unwrap();
    let path: String = db.query_row("SELECT file_path FROM Strip", [], |r| r.get(0)).unwrap();
    assert!(PathBuf::from(&path).is_relative() && path.ends_with(".webp"), "stored as {path}");
    let bytes = std::fs::read(dirs.data.join(&path)).unwrap();
    assert!(bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" && &bytes[12..16] == b"VP8L", "not a lossless WebP");
    let kept = export::decode_webp(&bytes).unwrap();
    let sent = export::decode_png(&image).unwrap();
    assert_eq!((kept.w, kept.h), (sent.w, sent.h));
    assert!(kept.px == sent.px, "the strip lost pixels");

    let book = backend.stop().unwrap().book_id.unwrap();
    let out = backend.export(ExportIn { book_id: book, format: "png".into() }).unwrap();
    let img = decode_png(&std::fs::read(&out.files[0]).unwrap());
    assert_eq!(img.at(5, 1000)[0], scene.screen_px(0, SCREEN_W, 5, 1000));
}

#[test]
fn a_large_capture_is_saved_and_reopened_for_export_in_both_strip_formats() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = on(&dirs, &gemini);
    let (width, height) = (5120, 2880);
    let thumb = Scene::new(1203).thumb(0, width, height);
    backend.frame(FrameIn { seq: 1, width, height, thumb: thumb.clone(), captured_at: 100 }).unwrap();
    let frame = backend.frame(FrameIn { seq: 2, width, height, thumb, captured_at: 200 }).unwrap();
    let range = frame.save.expect("capture asked for no strip");
    assert_eq!((range.top, range.bottom), (0, height));

    let pixel = |x, y| [if x < width / 2 { 30 } else { 60 }, if y < export::BAND as u32 { 100 } else { 140 }, 200];
    let png = png_rgb(width, height, pixel);
    backend.strip(StripIn { picture_id: frame.picture_id, frame_seq: 2, top: range.top, bottom: range.bottom, image: png.clone() }).unwrap();
    let book = backend.stop().unwrap().book_id.unwrap();
    drop(backend);

    let db = rusqlite::Connection::open(dirs.data.join("translator.sqlite3")).unwrap();
    let webp: String = db.query_row("SELECT file_path FROM Strip", [], |row| row.get(0)).unwrap();
    assert!(webp.ends_with(".webp"));
    let legacy = "legacy.png";
    std::fs::write(dirs.data.join(legacy), png).unwrap();
    for path in [webp.as_str(), legacy] {
        // Older versions saved PNG strips in the same database.
        assert_eq!(db.execute("UPDATE Strip SET file_path = ?1", [path]).unwrap(), 1);
        let reopened = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
        let out = reopened.export(ExportIn { book_id: book, format: "png".into() }).unwrap();
        assert_eq!(out.files.len(), 1);
        let image = decode_png(&std::fs::read(&out.files[0]).unwrap());
        assert_eq!((image.w, image.h), (width, height));
        for (x, y) in [(0, 0), (width / 2, 0), (0, export::BAND as u32), (width - 1, height - 1)] {
            assert_eq!(image.at(x.into(), y.into()), pixel(x, y), "{path} at {x},{y}");
        }
    }
}

#[test]
fn a_shape_grown_for_a_long_translation_does_not_cover_one_already_shown() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = on(&dirs, &gemini);
    let f = still(&backend, &Scene::new(1202));
    let first = Balloon::new(100, 500, 120, 20, 12021);
    let second = Balloon::new(100, 540, 121, 20, 12022);
    gemini.read_as(first.size(), "うん");
    gemini.translate_as("うん", "네");
    gemini.read_as(second.size(), "ああ");
    gemini.translate_as("ああ", &"가".repeat(80));
    let send = |b: &Balloon| {
        let crop = CropIn { x: b.x as u32, y: b.y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
        backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: 2, crops: vec![crop] }).unwrap()
    };
    let wait = |requests: usize| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while gemini.count() < requests {
            assert!(Instant::now() < deadline, "Gemini was not asked");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    send(&first);
    wait(2);
    assert_eq!(backend.placements(Default::default()).unwrap().placements.len(), 1);
    send(&second);
    wait(4);
    let shown = backend.placements(Default::default()).unwrap().placements;
    assert_eq!(shown.len(), 1, "the grown shape was placed over the first one: {:?}", shown.iter().map(|p| (p.y, p.h)).collect::<Vec<_>>());
    assert_eq!(shown[0].ko, "네", "the translation shown first was replaced");
}
