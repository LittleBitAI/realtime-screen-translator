//! The capture loop both platforms run once translation is switched on
//! (R1, R11): every captured frame goes to P6; when the screen rests, the
//! local model detects balloons in the range P6 names (P7, R25) and the
//! clean rows P6 asks for are saved (P9). The overlay is redrawn after every
//! frame so the translations follow the balloons (R31).
//!
//! The screen side is a [`Screen`]: Windows Graphics Capture with the ONNX
//! detector on Windows, the Kotlin plugin on Android (A7).

use crate::api::{CropIn, CropsIn, FrameIn, PlacementOut, Range, StripIn};
use crate::Backend;

/// A frame as P6 takes it: the screen size and the 96-px-wide grayscale thumbnail.
#[derive(Clone, Debug)]
pub struct Captured {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub thumb: Vec<u8>,
    pub captured_at: i64,
    /// Where this app's own floating button is in the frame (x, y, w, h in
    /// screen pixels), if the capture holds it: on Android it does (A6); the
    /// Windows windows are left out of capture.
    pub button: Option<(u32, u32, u32, u32)>,
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
    /// `seq`. On failure the rows are tried again later, and the error may
    /// carry a P1 notice (`detector_missing`).
    fn detect(&self, seq: u64, top: u32, bottom: u32) -> Result<Vec<CropIn>, Option<&'static str>>;
    /// PNG of the clean screen rows `top..bottom` of frame `seq`.
    fn strip(&self, seq: u64, top: u32, bottom: u32) -> Option<Vec<u8>>;
    /// Show `placements` (screen coordinates); `clear` empties the overlay first.
    fn draw(&self, placements: &[PlacementOut], clear: bool);
}

/// The floating on/off button was pressed: P4 if translation is off, else P5.
pub fn toggle(backend: &Backend) {
    let r = if backend.status().on { backend.stop().map(|_| ()) } else { backend.start().map(|_| ()) };
    if let Err(e) = r {
        eprintln!("on/off button: {e}");
    }
}

/// Handle one event. Errors are logged: a dropped frame is followed by the next one.
pub fn step(backend: &Backend, screen: &dyn Screen, event: Event) {
    let f = match event {
        Event::Toggle => return toggle(backend),
        Event::Frame(f) => f,
    };
    let (seq, button) = (f.seq, f.button);
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
    // P9 keeps clean rows only: the rows behind the button are left unsaved
    // and asked for again once the content has moved out from under it.
    for (top, bottom) in out.save.map(|r| clear_of(r, button)).unwrap_or_default() {
        match screen.strip(seq, top, bottom) {
            Some(image) => {
                if let Err(e) = backend.strip(StripIn { picture_id: out.picture_id, frame_seq: seq, top, bottom, image }) {
                    eprintln!("strip of frame {seq}: {e}");
                }
            }
            None => eprintln!("frame {seq} is gone before its strip was taken"),
        }
    }
    if let Some(r) = out.detect {
        let crops = screen.detect(seq, r.top, r.bottom).unwrap_or_else(|notice| {
            backend.detect_failed(out.picture_id, seq, r);
            if let Some(n) = notice {
                backend.notify(n);
            }
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

/// The parts of screen rows `r` the button (x, y, w, h) leaves uncovered.
pub fn clear_of(r: Range, button: Option<(u32, u32, u32, u32)>) -> Vec<(u32, u32)> {
    let Some((_, y, _, h)) = button.filter(|b| b.1 < r.bottom && b.1 + b.3 > r.top) else { return vec![(r.top, r.bottom)] };
    [(r.top, y), (y + h, r.bottom)].into_iter().filter(|p| p.1 > p.0).collect()
}

/// Run until the screen side ends.
pub fn run(backend: &Backend, screen: &dyn Screen) {
    while let Some(event) = screen.next() {
        step(backend, screen, event);
    }
}
