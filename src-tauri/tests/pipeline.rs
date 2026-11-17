//! The capture loop (R1, R3, R25, R29, R31) driven by a scripted screen:
//! frames in, detection and strips asked for where P6 says, the overlay
//! redrawn after every frame, the floating button switching translation.

mod support;

use realtime_screen_translator::api::{CropIn, PlacementOut};
use realtime_screen_translator::pipeline::{self, Captured, Event, Screen};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

#[derive(Default)]
struct Script {
    events: VecDeque<Event>,
    detected: Vec<(u32, u32)>,
    strips: Vec<(u32, u32)>,
    drawn: Vec<Vec<PlacementOut>>,
}

struct FakeScreen {
    scene: Scene,
    balloon: Balloon,
    /// seq -> scroll offset of that frame.
    offsets: Mutex<Vec<(u64, i64)>>,
    script: Mutex<Script>,
}

impl FakeScreen {
    fn offset(&self, seq: u64) -> i64 {
        self.offsets.lock().unwrap().iter().find(|(s, _)| *s == seq).unwrap().1
    }
}

impl Screen for FakeScreen {
    fn next(&self) -> Option<Event> {
        self.script.lock().unwrap().events.pop_front()
    }
    fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>> {
        self.script.lock().unwrap().detected.push((top, bottom));
        let y = self.balloon.y - self.offset(seq);
        if y < top as i64 || y >= bottom as i64 {
            return Ok(vec![]);
        }
        let b = self.balloon;
        Ok(vec![CropIn { x: b.x as u32, y: y as u32, w: b.w, h: b.h, kind: "dialogue".into(), score: 0.9, image: b.png(0, 0) }])
    }
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>> {
        self.script.lock().unwrap().strips.push((top, bottom));
        Some(self.scene.screen_png(self.offset(seq), SCREEN_W, SCREEN_H, top, bottom))
    }
    fn draw(&self, placements: &[PlacementOut], _clear: bool) {
        self.script.lock().unwrap().drawn.push(placements.to_vec());
    }
}

fn frames(screen: &FakeScreen, seq: &mut u64, offset: i64, n: usize) -> Vec<Event> {
    (0..n)
        .map(|_| {
            *seq += 1;
            screen.offsets.lock().unwrap().push((*seq, offset));
            Event::Frame(Captured {
                seq: *seq,
                width: SCREEN_W,
                height: SCREEN_H,
                thumb: screen.scene.thumb(offset, SCREEN_W, SCREEN_H),
                captured_at: *seq as i64 * 100,
                button: None,
                bars: vec![],
            })
        })
        .collect()
}

#[test]
fn switched_on_by_the_button_the_loop_reads_follows_and_saves() {
    let dirs = Dirs::new();
    let gemini = Gemini::start();
    let balloon = Balloon::new(300, 700, 160, 220, 91);
    gemini.read_as(balloon.size(), "いくぞ");
    gemini.translate_as("いくぞ", "간다");
    let platform = Arc::new(Device { export: dirs.export.clone() });
    let backend = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), platform).unwrap();
    backend.put_key(serde_json::from_value(serde_json::json!({"key": TEST_KEY})).unwrap()).unwrap();
    backend.request_permissions();

    let screen = FakeScreen { scene: Scene::new(92), balloon, offsets: Mutex::new(vec![]), script: Mutex::default() };
    let mut seq = 0;
    let mut first = vec![Event::Toggle];
    first.extend(frames(&screen, &mut seq, 0, 2));
    screen.script.lock().unwrap().events.extend(first);
    pipeline::run(&backend, &screen);
    assert!(backend.status().on, "the button did not switch translation on");

    let deadline = Instant::now() + Duration::from_secs(20);
    while backend.placements(Default::default()).unwrap().placements.is_empty() {
        assert!(Instant::now() < deadline, "no translation came back");
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut more = frames(&screen, &mut seq, 300, 2);
    more.push(Event::Toggle);
    screen.script.lock().unwrap().events.extend(more);
    pipeline::run(&backend, &screen);

    let s = screen.script.lock().unwrap();
    assert_eq!(s.detected, vec![(0, 2400), (2100, 2400)], "detection was not limited to the new rows");
    assert_eq!(s.strips, vec![(0, 2400), (2100, 2400)]);
    let last = s.drawn.iter().rev().find(|d| !d.is_empty()).expect("nothing was drawn");
    assert_eq!(last[0].ko, "간다");
    assert!(last[0].y <= 700 - 300 && last[0].y + last[0].h as i32 >= 700 - 300 + 220, "the translation did not follow: {last:?}");
    assert!(!backend.status().on, "the button did not switch translation off");
    assert_eq!(gemini.count(), 2, "one read and one translation");
}
