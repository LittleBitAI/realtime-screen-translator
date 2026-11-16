//! The capture loop both platforms run once translation is switched on
//! (R1, R11): every captured frame goes to P6; when the screen rests, the
//! local model detects balloons in the range P6 names (P7, R25) and the
//! clean rows P6 asks for are saved (P9). The overlay is redrawn after every
//! frame so the translations follow the balloons (R31).
//!
//! The screen side is a [`Screen`]: Windows Graphics Capture with the ONNX
//! detector on Windows, the Kotlin plugin on Android (A7).

use crate::api::{CropIn, CropsIn, FrameIn, PlacementOut, StripIn};
use crate::Backend;

/// A frame as P6 takes it: the screen size and the 96-px-wide grayscale thumbnail.
#[derive(Clone, Debug)]
pub struct Captured {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub thumb: Vec<u8>,
    pub captured_at: i64,
}

#[derive(Clone, Debug)]
pub enum Event {
    Frame(Captured),
    /// The floating on/off button over other apps was tapped (R29).
    Toggle,
}

pub trait Screen: Send + Sync {
    /// Block until the next frame or button tap. `None` ends the loop.
    fn next(&self) -> Option<Event>;
    /// Text areas the local model finds in screen rows `top..bottom` of frame
    /// `seq`. The error is a P1 notice (`detector_missing`).
    fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, &'static str>;
    /// PNG of the clean screen rows `top..bottom` of frame `seq`.
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>>;
    /// Show `placements` (screen coordinates); `clear` empties the overlay first.
    fn draw(&self, placements: &[PlacementOut], clear: bool);
}

/// Handle one event. Errors are logged: a dropped frame is followed by the next one.
pub fn step(backend: &Backend, screen: &dyn Screen, event: Event) {
    let f = match event {
        Event::Toggle => {
            let r = if backend.status().on { backend.stop().map(|_| ()) } else { backend.start().map(|_| ()) };
            if let Err(e) = r {
                eprintln!("on/off button: {e}");
            }
            return;
        }
        Event::Frame(f) => f,
    };
    let seq = f.seq;
    let out = match backend.frame(FrameIn { seq, width: f.width, height: f.height, thumb: f.thumb, captured_at: f.captured_at }) {
        Ok(out) => out,
        Err(e) => {
            if e.0 != "not_on" {
                eprintln!("frame {seq}: {e}");
            }
            return;
        }
    };
    screen.draw(&out.placements, out.clear);
    if let Some(r) = out.save {
        match screen.strip(seq, r.top, r.bottom) {
            Some(image) => {
                if let Err(e) = backend.strip(StripIn { picture_id: out.picture_id, frame_seq: seq, top: r.top, bottom: r.bottom, image }) {
                    eprintln!("strip of frame {seq}: {e}");
                }
            }
            None => eprintln!("frame {seq} is gone before its strip was taken"),
        }
    }
    if let Some(r) = out.detect {
        let crops = screen.detect(seq, r.top, r.bottom).unwrap_or_else(|notice| {
            backend.notify(notice);
            vec![]
        });
        if !crops.is_empty() {
            match backend.crops(CropsIn { picture_id: out.picture_id, frame_seq: seq, crops }) {
                Ok(c) if !c.placed.is_empty() => {
                    if let Ok(p) = backend.placements(Default::default()) {
                        screen.draw(&p.placements, false);
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("crops of frame {seq}: {e}"),
            }
        }
    }
}

/// Run until the screen side ends.
pub fn run(backend: &Backend, screen: &dyn Screen) {
    while let Some(event) = screen.next() {
        step(backend, screen, event);
    }
}
