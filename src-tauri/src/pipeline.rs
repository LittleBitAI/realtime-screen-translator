//! The capture loop both platforms run once translation is switched on
//! (R1, R11): every captured frame goes to P6; when the screen rests, the
//! local model detects balloons in the range P6 names (P7, R25) and the
//! clean rows P6 asks for are saved (P9). The overlay is redrawn after every
//! frame so the translations follow the balloons (R31).
//!
//! The screen side is a [`Screen`]: Windows Graphics Capture with the ONNX
//! detector on Windows, the Kotlin plugin on Android (A7).

use crate::api::{CropIn, CropsIn, FrameIn, PlacementOut, Range, StripIn};
use crate::tracker::{self, THUMB_W};
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
    /// Where the system draws over every app while they show (x, y, w, h):
    /// the status bar and the navigation bar, the taskbar. A capture the
    /// reader's app blocks is black but for these and the button.
    pub bars: Vec<(u32, u32, u32, u32)>,
}

#[derive(Clone, Debug)]
pub enum Event {
    Frame(Captured),
    /// The floating on/off button over other apps was tapped (R29).
    Toggle,
    /// Capture was stopped from outside the app (Android's system UI): the
    /// session ends as if switched off (P5).
    Lost,
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
    /// A new frame of the screen with this app's button hidden for it, when
    /// the capture holds the button (Android); its rows come from [`Screen::strip`].
    fn clean_frame(&self) -> Option<Captured> {
        None
    }
    /// Whether the reader touched the screen (outside this app's button)
    /// since this was last asked, a moment ago at least, so what the touch
    /// did shows by now. Where the capture holds the overlay (Android).
    fn touched(&self) -> bool {
        false
    }
    /// A new frame with the translation shapes hidden for it, shown again
    /// at once; its rows come from [`Screen::strip`].
    fn peek(&self) -> Option<Captured> {
        None
    }
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
        Event::Lost => {
            if let Err(e) = backend.stop() {
                if e.0 != "not_on" {
                    eprintln!("capture lost: {e}");
                }
            }
            return;
        }
        Event::Frame(f) => f,
    };
    let (seq, button, size, thumb) = (f.seq, f.button, (f.width, f.height), f.thumb.clone());
    // P6 sees the screen without what is drawn over every app: this app's
    // own button (the capture holds it on Android) and the system bars. They
    // neither light a blocked capture (`tracker::is_blocked`) nor move with
    // the content.
    let over: Vec<_> = f.bars.iter().copied().chain(button).collect();
    let mut sent = f.thumb;
    for (i, hidden) in cover(size.0, sent.len() / THUMB_W, &over).into_iter().enumerate() {
        if hidden {
            sent[i] = 0;
        }
    }
    // The button stays put over a scrolling page: its black block is left
    // out of the comparisons, not taken as the page standing still.
    let hidden = button.map(|b| cover(size.0, sent.len() / THUMB_W, &[b])).unwrap_or_default();
    let out = match backend.frame_hiding(FrameIn { seq, width: f.width, height: f.height, thumb: sent, captured_at: f.captured_at }, hidden) {
        Ok(out) => out,
        Err(e) => {
            if e.0 != "not_on" {
                eprintln!("frame {seq}: {e}");
            }
            return;
        }
    };
    screen.draw(&out.placements, out.clear);
    // P9 keeps clean rows only. Rows behind the button come from a frame
    // taken with the button hidden, if it shows the same screen and the
    // button is indeed gone from it; else they are left unsaved and asked
    // for again.
    // The same holds for P7: a text area behind the button is looked for in
    // the clean frame, or its rows are asked for again once it may show.
    let mut from = seq;
    let mut parts = out.save.map(|r| clear_of(r, button)).unwrap_or_default();
    let behind = |r: Range| clear_of(r, button) != [(r.top, r.bottom)];
    let clean = if out.save.is_some_and(behind) || out.detect.is_some_and(behind) {
        let ok = |c: &Captured| same_screen(&thumb, size, c, &over) && button.is_some_and(|b| button_gone(&thumb, size.0, c, b));
        screen.clean_frame().filter(ok)
    } else {
        None
    };
    if let (Some(r), Some(c)) = (out.save, &clean) {
        from = c.seq;
        parts = vec![(r.top, r.bottom)];
    }
    for (top, bottom) in parts {
        match screen.strip(from, top, bottom) {
            Some(image) => {
                if let Err(e) = backend.strip(StripIn { picture_id: out.picture_id, frame_seq: seq, top, bottom, image }) {
                    eprintln!("strip of frame {seq}: {e}");
                }
            }
            None => eprintln!("frame {seq} is gone before its strip was taken"),
        }
    }
    // A tap that left the screen still may have turned to a page drawn the
    // same but for the words the shapes cover (the capture holds them): a
    // look under the shapes tells (`Backend::text_areas`).
    if screen.touched() && out.motion == "still" {
        look_under(backend, screen, out.picture_id);
    }
    if let Some(r) = out.detect {
        let shown = clean.as_ref().map_or(seq, |c| c.seq);
        let crops = match screen.detect(shown, r.top, r.bottom) {
            Ok(mut c) => {
                // The model is in place and ran.
                backend.clear_notice("detector_missing");
                // Rows the button hid from it are looked at again later
                // (on a scroll, a page's return, or after a while).
                if let (None, Some(b)) = (&clean, button.filter(|_| behind(r))) {
                    let rows = Range { top: b.1.max(r.top), bottom: (b.1 + b.3).min(r.bottom) };
                    backend.detect_failed(out.picture_id, seq, rows);
                }
                // A crop of a frame that holds the button and meets it shows
                // the button over the balloon: it is not sent, and its rows
                // are looked at again with the rest.
                if let (None, Some(b)) = (&clean, button) {
                    c.retain(|k| {
                        let meets = k.x < b.0 + b.2 && k.x + k.w > b.0 && k.y < b.1 + b.3 && k.y + k.h > b.1;
                        if meets {
                            backend.detect_failed(out.picture_id, seq, Range { top: k.y, bottom: k.y + k.h });
                        }
                        !meets
                    });
                }
                c
            }
            Err(notice) => {
                backend.detect_failed(out.picture_id, seq, r);
                if let Some(n) = notice {
                    backend.notify(n);
                }
                vec![]
            }
        };
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

fn look_under(backend: &Backend, screen: &dyn Screen, picture_id: i64) {
    let areas = backend.text_areas(picture_id);
    if areas.is_empty() {
        return;
    }
    let Some(c) = screen.peek() else { return };
    let seen: Vec<_> = areas.iter().filter_map(|&(id, (_, y, _, h))| screen.strip(c.seq, y, y + h).map(|png| (id, png))).collect();
    if let Err(e) = backend.words_under(picture_id, &seen) {
        eprintln!("words under the shapes: {e}");
    }
}

/// The parts of screen rows `r` the button (x, y, w, h) leaves uncovered.
pub fn clear_of(r: Range, button: Option<(u32, u32, u32, u32)>) -> Vec<(u32, u32)> {
    let Some((_, y, _, h)) = button.filter(|b| b.1 < r.bottom && b.1 + b.3 > r.top) else { return vec![(r.top, r.bottom)] };
    [(r.top, y), (y + h, r.bottom)].into_iter().filter(|p| p.1 > p.0).collect()
}

/// Whether `c` shows the screen of `thumb` (`size`): the same apart from
/// what is drawn over the app (`over`: the button, which one holds and the
/// other does not, and the system bars, whose clock may have moved on).
pub fn same_screen(thumb: &[u8], size: (u32, u32), c: &Captured, over: &[(u32, u32, u32, u32)]) -> bool {
    if (c.width, c.height) != size || c.thumb.len() != thumb.len() || size.0 == 0 {
        return false;
    }
    let rows = thumb.len() / THUMB_W;
    tracker::still_diff(thumb, &c.thumb, &cover(size.0, rows, over), 0, rows) <= tracker::STILL
}

/// Whether the button (x, y, w, h on a screen `width` wide), which `thumb`
/// holds, is gone from `c` (of the same size): the cells wholly under it
/// differ. An image the mirror composed before the button was hidden, however
/// late it came, still shows the button there, and would save it over the page.
pub fn button_gone(thumb: &[u8], width: u32, c: &Captured, button: (u32, u32, u32, u32)) -> bool {
    if c.thumb.len() != thumb.len() {
        return false;
    }
    let scale = width.max(1) as f64 / THUMB_W as f64;
    let rows = thumb.len() / THUMB_W;
    let (x, y, w, h) = button;
    let inside = |from: u32, len: u32, max: usize| {
        let a = (from as f64 / scale).ceil() as usize;
        (a, ((from.saturating_add(len) as f64 / scale).floor() as usize).min(max))
    };
    let ((c0, c1), (r0, r1)) = (inside(x, w, THUMB_W), inside(y, h, rows));
    let (mut sum, mut n) = (0u64, 0u64);
    for r in r0..r1 {
        for i in r * THUMB_W + c0..r * THUMB_W + c1.max(c0) {
            sum += thumb[i].abs_diff(c.thumb[i]) as u64;
            n += 1;
        }
    }
    n > 0 && sum as f32 > tracker::MATCH * n as f32
}

/// The thumbnail cells under the rectangles (x, y, w, h on a screen `width`
/// wide), one cell of margin around each.
fn cover(width: u32, rows: usize, rects: &[(u32, u32, u32, u32)]) -> Vec<bool> {
    let scale = width.max(1) as f64 / THUMB_W as f64;
    let from = |v: u32| (v as f64 / scale).floor() as usize;
    let to = |v: u32, max: usize| ((v as f64 / scale).ceil() as usize + 1).min(max);
    let mut mask = vec![false; rows * THUMB_W];
    for &(x, y, w, h) in rects {
        let (c1, r1) = (to(x.saturating_add(w), THUMB_W), to(y.saturating_add(h), rows));
        let (c0, r0) = (from(x).saturating_sub(1).min(c1), from(y).saturating_sub(1).min(r1));
        for r in r0..r1 {
            mask[r * THUMB_W + c0..r * THUMB_W + c1].fill(true);
        }
    }
    mask
}

/// Run until the screen side ends.
pub fn run(backend: &Backend, screen: &dyn Screen) {
    while let Some(event) = screen.next() {
        step(backend, screen, event);
    }
}
