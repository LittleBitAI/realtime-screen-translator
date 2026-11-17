//! Edge cases of the third review: scrolls that are not whole thumbnail
//! rows, rows tried again after a failure, the screen turning, crops that
//! are not what they claim, and stop cutting off Gemini.

mod support;

use realtime_screen_translator::api::{CropIn, CropsIn, FrameIn, KeyIn, Range};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::{tracker, Backend};
use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};
use support::*;

struct Device {
    export: PathBuf,
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
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}

fn backend_on(dirs: &Dirs, gemini_url: &str) -> Backend {
    let platform = Arc::new(Device { export: dirs.export.clone() });
    let b = Backend::open(dirs.data.clone(), Some(gemini_url.to_string()), platform).unwrap();
    b.put_key(KeyIn { key: TEST_KEY.into() }).unwrap();
    b.start().unwrap();
    b
}

struct Feed<'a> {
    backend: &'a Backend,
    seq: u64,
}

impl Feed<'_> {
    fn send(&mut self, w: u32, h: u32, thumb: Vec<u8>) -> realtime_screen_translator::api::FrameOut {
        self.seq += 1;
        self.backend.frame(FrameIn { seq: self.seq, width: w, height: h, thumb, captured_at: self.seq as i64 * 100 }).unwrap()
    }
}

/// Smooth value noise over a long page, in screen pixels.
fn page_px(x: i64, y: i64) -> u8 {
    const CELL: i64 = 36;
    let n = |a: i64, b: i64| {
        let mut z = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        z = (z ^ (z >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        ((z ^ (z >> 32)) >> 11) as f64 / (1u64 << 53) as f64
    };
    let (cx, cy) = (x.div_euclid(CELL), y.div_euclid(CELL));
    let s = |t: f64| t * t * (3.0 - 2.0 * t);
    let (fx, fy) = (s(x.rem_euclid(CELL) as f64 / CELL as f64), s(y.rem_euclid(CELL) as f64 / CELL as f64));
    let top = n(cx, cy) * (1.0 - fx) + n(cx + 1, cy) * fx;
    let bot = n(cx, cy + 1) * (1.0 - fx) + n(cx + 1, cy + 1) * fx;
    (30.0 + 200.0 * (top * (1.0 - fy) + bot * fy)) as u8
}

/// The thumbnail a Galaxy S21 (1080 x 2400) sends with the page scrolled by `offset` px.
fn s21_thumb(offset: i64) -> Vec<u8> {
    let (w, h) = (1080usize, 2400usize);
    let gray: Vec<u8> = (0..w * h).map(|i| page_px((i % w) as i64, (i / w) as i64 + offset)).collect();
    tracker::thumbnail(&gray, w, h)
}

#[test]
fn a_scroll_of_fractional_rows_adds_up_and_is_set_right_at_rest() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = backend_on(&dirs, &gemini.url);
    let mut feed = Feed { backend: &backend, seq: 0 };
    feed.send(1080, 2400, s21_thumb(0));
    feed.send(1080, 2400, s21_thumb(0));
    // 120 px a frame is 10.65 thumbnail rows: rounding each step gave 124 px.
    let mut moved = 0;
    for k in 1..=10 {
        let f = feed.send(1080, 2400, s21_thumb(120 * k));
        assert_eq!(f.motion, "scroll", "step {k}");
        moved += f.dy as i64;
    }
    assert!((moved - 1200).abs() <= 12, "the steps summed to {moved} px for 1200");
    feed.send(1080, 2400, s21_thumb(1200));
    let rest = backend.placements(Default::default()).unwrap().offset_y;
    assert!((rest - 1200).abs() <= 2, "at rest the screen was put at {rest} for 1200");
}

#[test]
fn rows_whose_detection_failed_are_detected_again_after_the_screen_moves() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let backend = backend_on(&dirs, &gemini.url);
    let scene = Scene::new(301);
    let mut feed = Feed { backend: &backend, seq: 0 };
    feed.send(SCREEN_W, SCREEN_H, scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = feed.send(SCREEN_W, SCREEN_H, scene.thumb(0, SCREEN_W, SCREEN_H));
    assert_eq!(f.detect, Some(Range { top: 0, bottom: 2400 }));
    // The model could not look at rows 500 .. 700 of that frame.
    backend.detect_failed(f.picture_id, feed.seq, Range { top: 500, bottom: 700 });
    let still = feed.send(SCREEN_W, SCREEN_H, scene.thumb(0, SCREEN_W, SCREEN_H));
    assert_eq!(still.detect, None, "asked again at once, with nothing changed");

    feed.send(SCREEN_W, SCREEN_H, scene.thumb(100, SCREEN_W, SCREEN_H));
    let rest = feed.send(SCREEN_W, SCREEN_H, scene.thumb(100, SCREEN_W, SCREEN_H));
    assert_eq!(rest.detect, Some(Range { top: 400, bottom: 600 }), "the failed rows were not asked again");
    let next = feed.send(SCREEN_W, SCREEN_H, scene.thumb(100, SCREEN_W, SCREEN_H));
    assert_eq!(next.detect, Some(Range { top: 2300, bottom: 2400 }));
}

#[test]
fn a_balloon_gemini_failed_on_is_detected_again_when_the_reader_scrolls() {
    let mut app = App::on();
    let scene = Scene::new(302);
    app.settle(&scene, 0);
    app.gemini.fail_with(500, "{}");
    app.crops_ok(&[Balloon::new(200, 500, 160, 200, 3021).dialogue()]);
    wait_until(20, "gemini_unreachable", || app.status()["notice"] == "gemini_unreachable");

    app.show(&scene, 100);
    let rest = app.settle(&scene, 100);
    assert!(rest.iter().any(|f| f.detect == Some((400, 600))), "the balloon's rows were not detected again: {:?}", rest.map(|f| f.detect));
}

#[test]
fn a_turned_screen_starts_a_new_picture_in_its_own_size() {
    let mut app = App::on();
    let scene = Scene::new(303);
    let before = app.read_and_save(&scene, 0)[1].picture_id;
    app.screen = (1920, 960);
    let turned = app.settle(&scene, 0);
    assert!(turned[0].clear, "the old overlay stayed up");
    assert_ne!(turned[1].picture_id, before, "the turned screen went on in the old picture");
    assert_eq!((turned[1].detect, turned[1].save), (Some((0, 960)), Some((0, 960))));
    let saved = app.strip(&scene, 0, (0, 960), turned[1].seq).expect("the turned screen's strip was refused");
    assert_eq!(saved["picture_height"], 960);
}

#[test]
fn a_crop_whose_picture_is_not_its_size_is_not_sent() {
    let mut app = App::on();
    app.settle(&Scene::new(304), 0);
    let whole_screen = png_rgb(SCREEN_W, SCREEN_H, |x, y| [(x % 251) as u8, (y % 241) as u8, 90]);
    let crop = Crop { rect: Rect::new(200, 500, 160, 200), kind: "dialogue".into(), image: whole_screen };
    let r = app.crops_ok(&[crop]);
    assert_eq!((r["discarded"].as_u64(), r["pending"].as_u64()), (Some(1), Some(0)), "{r}");
    sleep(Duration::from_millis(300));
    assert_eq!(app.gemini.count(), 0);
}

#[test]
fn stop_cuts_off_a_gemini_request_in_flight_and_nothing_goes_out_after() {
    // A Gemini that takes the request and never answers.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (got, request_in) = mpsc::channel();
    let (closed_tx, closed) = mpsc::channel();
    let server = listener.try_clone().unwrap();
    std::thread::spawn(move || {
        let (mut conn, _) = server.accept().unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut buf = [0u8; 65536];
        let mut seen = 0;
        // Read until the request is in and the client goes quiet.
        loop {
            match conn.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => seen += n,
                Err(_) if seen > 0 => break,
                Err(_) => {}
            }
        }
        got.send(()).unwrap();
        conn.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let t = Instant::now();
        let _ = conn.read(&mut buf);
        closed_tx.send(t.elapsed()).unwrap();
    });

    let dirs = Dirs::new();
    let backend = backend_on(&dirs, &url);
    let scene = Scene::new(305);
    let mut feed = Feed { backend: &backend, seq: 0 };
    feed.send(SCREEN_W, SCREEN_H, scene.thumb(0, SCREEN_W, SCREEN_H));
    let f = feed.send(SCREEN_W, SCREEN_H, scene.thumb(0, SCREEN_W, SCREEN_H));
    let b = Balloon::new(200, 500, 160, 200, 3051);
    let crop = CropIn { x: 200, y: 500, w: 160, h: 200, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) };
    backend.crops(CropsIn { picture_id: f.picture_id, frame_seq: feed.seq, crops: vec![crop] }).unwrap();
    request_in.recv_timeout(Duration::from_secs(10)).expect("the request never came");

    let t = Instant::now();
    backend.stop().unwrap();
    assert!(t.elapsed() < Duration::from_secs(1), "stop waited for Gemini");
    let open_for = closed.recv_timeout(Duration::from_secs(5)).expect("the request was not cut off");
    assert!(open_for < Duration::from_secs(2), "the request stayed open {open_for:?} after stop");
    listener.set_nonblocking(true).unwrap();
    sleep(Duration::from_millis(500));
    assert!(listener.accept().is_err(), "a request went out after stop");
}
