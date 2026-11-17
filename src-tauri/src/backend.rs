//! The commands P1..P11. One `Backend` per app; every command locks the
//! state briefly and never waits on Gemini: crops that need Gemini go to a
//! worker thread whose result is dropped unless it belongs to the session
//! that is still running (its generation).

use crate::api::*;
use crate::detect;
use crate::export;
use crate::fingerprint;
use crate::gemini::{self, GeminiError};
use crate::layout;
use crate::platform::Platform;
use crate::store::{self, Db, PlacementRow, Translation};
use crate::tracker::{self, THUMB_W};
use crate::now_ms;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

const KEY_SETTING: &str = "gemini_api_key";
const MODEL_SETTING: &str = "gemini_model";

#[derive(Clone)]
struct Known {
    fp: u64,
    hex: String,
    aspect: f32,
    status: String,
    ko: Option<String>,
}

/// A picture of the running session, in picture coordinates.
struct Pic {
    id: i64,
    kind: &'static str,
    /// The screen size it was made in: its rows, strips and placements are in
    /// that screen's pixels, so it goes on only on a screen of that size.
    screen: (u32, u32),
    /// Ranges saved as strips (sorted, disjoint). A part scrolled past without
    /// stopping stays a gap and is asked for when the screen rests on it.
    covered: Vec<(i64, i64)>,
    /// Ranges already sent to the local model for detection (sorted, disjoint).
    detected: Vec<(i64, i64)>,
    placements: Vec<PlacementRow>,
    /// The text area (picture coordinates) each placement of this session
    /// was made for, by placement id: what its fingerprint was taken of.
    texts: Vec<(i64, Rect)>,
    /// Settled thumbnail of a page, to recognise it when the reader comes back.
    page_thumb: Option<Vec<u8>>,
    /// Rows whose detection or translation failed. They are detected again
    /// once the screen moves or comes back here, or after [`RETRY_MS`].
    retry: Vec<(i64, i64)>,
    retry_at: i64,
    /// `scroll_y` of the screen rows handled before any fixed band was known
    /// (the first resting screen). A status bar or toolbar there was taken as
    /// content; once a scroll shows the band, those rows are handled again.
    unbanded: Option<i64>,
    /// Text areas read (picture coordinates), with the screen each was seen
    /// on (x0, y0, x1, y1). One at that screen's top or bottom edge, or at a
    /// fixed band's edge (known then or found by a later scroll), may have
    /// been cut there: one seen larger later replaces what was placed for it
    /// (`replace_cut`).
    edge_rects: Vec<(Rect, (i64, i64, i64, i64))>,
    /// Placements taken away while the capture holds the overlay (A6), with
    /// the first frame seq taken after the overlay stopped drawing them: the
    /// white shape stays in the frames before, over the Japanese.
    erased: Vec<(Rect, u64)>,
}

const RETRY_MS: i64 = 15_000;

impl Pic {
    fn failed(&mut self, rows: (i64, i64), now: i64) {
        if self.retry.is_empty() {
            self.retry_at = now + RETRY_MS;
        }
        add(&mut self.retry, rows);
    }
    fn reopen(&mut self) {
        for r in std::mem::take(&mut self.retry) {
            subtract(&mut self.detected, r);
        }
    }
}

/// A balloon on its way through Gemini, with every place it was seen while waiting.
struct Pending {
    fp: u64,
    aspect: f32,
    targets: Vec<Target>,
}

type Rect = (i64, i64, u32, u32);
/// Picture id, text area and the screen it was seen on (x0, y0, x1, y1), in
/// picture coordinates: the shape stays on that screen (`layout::fit_within`).
type Target = (i64, Rect, (i64, i64, i64, i64));

struct Session {
    gen: u64,
    book_id: i64,
    pics: HashMap<i64, Pic>,
    /// Picture on screen; `None` while a page change is under way.
    current: Option<i64>,
    /// Last picture shown, kept for `picture_id` during a page change.
    last_pic: Option<i64>,
    prev: Option<Vec<u8>>,
    /// Thumbnail pixels the last frame hid (`Backend::frame_hiding`).
    hidden_prev: Vec<bool>,
    screen: (u32, u32),
    /// Thumbnail rows of a frame of this screen.
    rows: usize,
    fixed: (usize, usize),
    /// Thumbnail columns at the left and right that stay put while the rest
    /// scrolls (`tracker::fixed_cols`): left out of the comparisons.
    side: (usize, usize),
    /// Picture y of the screen's top row: `scroll_f` rounded. The scroll is
    /// summed with sub-pixel precision so frame-by-frame rounding does not add up.
    scroll_y: i64,
    scroll_f: f64,
    /// Last resting thumbnail and its `scroll_f`: a scroll is measured again
    /// against it when the screen rests, so what the steps got wrong is undone.
    /// With the seq of the frame it is.
    anchor: Option<(Vec<u8>, f64, u64)>,
    /// `scroll_y` the overlay was drawn at before the last frame.
    drawn_before: i64,
    /// (`revision`, `scroll_y`) as the last two frames came in, the newer
    /// first: the overlay each of them holds was drawn from about these.
    entries: [Option<(u64, i64)>; 2],
    turning: bool,
    /// A look under the shapes found other words there (`words_under`): the
    /// next frame of that picture turns the page, and the turn does not come
    /// back to it.
    words_changed: Option<i64>,
    left_for_words: Option<i64>,
    mode: Option<&'static str>,
    last_seq: Option<u64>,
    /// Recent frames: seq -> (picture id, scroll_y), to place crops of older frames.
    /// The fourth field is the range P6 asked to save, in picture rows.
    frames: VecDeque<(u64, i64, i64, Option<(i64, i64)>)>,
    pending: Vec<Pending>,
    revision: u64,
    /// This session's Gemini connections; shut down by P5 so nothing is in
    /// flight or goes out after stop.
    agent: ureq::Agent,
    /// The capture holds this app's overlay (`Platform::overlay_in_capture`).
    overlay_in_capture: bool,
}

impl Session {
    /// Screen pixels per thumbnail row.
    fn scale(&self) -> f64 {
        self.screen.1 as f64 / self.rows.max(1) as f64
    }
    fn set_scroll(&mut self, f: f64) {
        self.scroll_f = f;
        self.scroll_y = f.round() as i64;
    }
    /// Thumbnail pixels under the current picture's placements as drawn with
    /// the screen at each of `scrolls`, a pixel of margin around: the capture
    /// holds our own overlay on Android, and its translations are not the
    /// screen changing. Empty when nothing is placed.
    /// Whether `thumb` differs from page `pic`'s settled thumbnail under any
    /// of its placements. Where the capture does not hold the overlay
    /// (Windows) that is the balloon's own text: another page, drawn alike
    /// but for its words, is not this one. (Where it holds the overlay the
    /// white shapes are there instead, and nothing can be told.)
    fn other_words(&self, pic: &Pic, thumb: &[u8]) -> bool {
        !self.overlay_in_capture && pic.page_thumb.as_ref().is_some_and(|page| words_differ(pic, 0, page, thumb))
    }

    fn overlay_mask(&self, scrolls: &[i64]) -> Vec<bool> {
        // Where the capture does not hold the overlay (Windows), the pixels
        // under it are the page's: a change there is the screen changing.
        if !self.overlay_in_capture {
            return vec![];
        }
        let Some(pic) = self.current.and_then(|id| self.pics.get(&id)) else { return vec![] };
        if (pic.placements.is_empty() && pic.erased.is_empty()) || self.rows == 0 {
            return vec![];
        }
        let (sx, sy) = (self.screen.0 as f64 / THUMB_W as f64, self.scale());
        let mut mask = vec![false; self.rows * THUMB_W];
        // Shapes erased lately may still be in the frames compared (`erased`).
        let shapes = pic.placements.iter().map(|p| (p.x, p.y, p.w, p.h)).chain(pic.erased.iter().map(|e| e.0));
        for (x, py, w, h) in shapes {
            for &off in scrolls {
                let y = (py - off) as f64;
                let r0 = ((y / sy).floor() as i64 - 1).clamp(0, self.rows as i64) as usize;
                let r1 = (((y + h as f64) / sy).ceil() as i64 + 1).clamp(0, self.rows as i64) as usize;
                let c0 = ((x as f64 / sx).floor() as i64 - 1).clamp(0, THUMB_W as i64) as usize;
                let c1 = (((x + w as i64) as f64 / sx).ceil() as i64 + 1).clamp(0, THUMB_W as i64) as usize;
                for r in r0..r1 {
                    mask[r * THUMB_W + c0..r * THUMB_W + c1].fill(true);
                }
            }
        }
        mask
    }
    fn pic(&mut self) -> Option<&mut Pic> {
        let id = self.current?;
        self.pics.get_mut(&id)
    }
    /// Where frame `seq` stood. A frame too old to remember cannot be placed:
    /// the current position would put its crops and strips in the wrong rows.
    fn frame_offset(&self, seq: u64, picture_id: i64) -> ApiResult<i64> {
        self.frames
            .iter()
            .rev()
            .find(|f| f.0 == seq && f.1 == picture_id)
            .map(|f| f.2)
            .ok_or(ApiError("frame_unknown"))
    }
    /// Placements of the current picture that are on screen, in screen coordinates.
    fn visible(&self) -> Vec<PlacementOut> {
        let Some(pic) = self.current.and_then(|id| self.pics.get(&id)) else { return vec![] };
        let h = self.screen.1 as i64;
        pic.placements
            .iter()
            .filter(|p| p.y - self.scroll_y + p.h as i64 > 0 && p.y - self.scroll_y < h)
            .map(|p| out(p, self.scroll_y))
            .collect()
    }
}

fn out(p: &PlacementRow, scroll_y: i64) -> PlacementOut {
    PlacementOut {
        id: p.id,
        x: p.x as i32,
        y: (p.y - scroll_y) as i32,
        w: p.w,
        h: p.h,
        radius: p.radius,
        ko: p.ko.clone(),
        font_px: p.font_px,
    }
}

struct State {
    db: Db,
    key: Option<String>,
    model: String,
    known: Vec<Known>,
    session: Option<Session>,
    generation: u64,
    gemini_requests: u32,
    notice: Option<&'static str>,
}

impl State {
    fn find_known(&self, fp: u64, aspect: f32) -> Option<&Known> {
        self.known
            .iter()
            // A failed attempt is not a translation: the balloon is asked again.
            .filter(|k| k.status != "failed" && fingerprint::same_balloon(fp, aspect, k.fp, k.aspect))
            .min_by_key(|k| (k.fp ^ fp).count_ones())
    }
}

pub struct Backend {
    state: Arc<Mutex<State>>,
    // Held through a whole P4 or P5, platform calls included, so one stop
    // never takes down the capture and overlay of a start that came after it.
    // Taken before `state`; nothing else takes it.
    lifecycle: Mutex<()>,
    platform: Arc<dyn Platform>,
    data_dir: PathBuf,
    gemini_base: String,
}

/// The parts of `r` outside `have` (sorted, disjoint), top to bottom.
fn missing(r: (i64, i64), have: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut out = vec![];
    let mut a = r.0;
    for &(t, b) in have {
        if b <= a {
            continue;
        }
        if t >= r.1 {
            break;
        }
        if t > a {
            out.push((a, t));
        }
        a = b;
        if a >= r.1 {
            break;
        }
    }
    if a < r.1 {
        out.push((a, r.1));
    }
    out
}

/// The biggest part of the screen not handled yet; the rest is asked for on the next resting frame.
fn new_part(vis: (i64, i64), have: &[(i64, i64)]) -> Option<(i64, i64)> {
    missing(vis, have).into_iter().max_by_key(|p| (p.1 - p.0, -p.0))
}

/// Take `r` out of the sorted, disjoint `have`.
fn subtract(have: &mut Vec<(i64, i64)>, r: (i64, i64)) {
    let mut out = vec![];
    for &(t, b) in have.iter() {
        if t < r.0 {
            out.push((t, b.min(r.0)));
        }
        if b > r.1 {
            out.push((t.max(r.1), b));
        }
    }
    *have = out;
}

/// Add `r` to the sorted, disjoint `have`, merging what touches.
fn add(have: &mut Vec<(i64, i64)>, r: (i64, i64)) {
    have.push(r);
    have.sort();
    let mut merged: Vec<(i64, i64)> = vec![];
    for &(t, b) in have.iter() {
        match merged.last_mut() {
            Some(last) if t <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((t, b)),
        }
    }
    *have = merged;
}

fn hull(have: &[(i64, i64)]) -> Option<(i64, i64)> {
    Some((have.first()?.0, have.last()?.1))
}

fn overlap(a: (i64, i64, u32, u32), b: (i64, i64, u32, u32)) -> f64 {
    let w = (a.0 + a.2 as i64).min(b.0 + b.2 as i64) - a.0.max(b.0);
    let h = (a.1 + a.3 as i64).min(b.1 + b.3 as i64) - a.1.max(b.1);
    if w <= 0 || h <= 0 {
        return 0.0;
    }
    let smaller = (a.2 as f64 * a.3 as f64).min(b.2 as f64 * b.3 as f64).max(1.0);
    (w * h) as f64 / smaller
}

fn title(now: i64) -> String {
    // Civil date from days since the epoch (UTC).
    let days = now.div_euclid(86_400_000);
    let secs = now.rem_euclid(86_400_000) / 1000;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", secs / 3600, secs / 60 % 60)
}

fn key_ok(key: &str) -> bool {
    key.len() >= 20 && key.len() <= 200 && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn tail(key: &str) -> String {
    key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect()
}

/// One crop on its way to Gemini.
struct Job {
    fp: u64,
    aspect: f32,
    png: Vec<u8>,
}

impl Backend {
    pub fn open(data_dir: PathBuf, gemini_base: Option<String>, platform: Arc<dyn Platform>) -> Result<Backend, String> {
        let db = store::open(&data_dir, now_ms()).map_err(|e| e.to_string())?;
        let key = store::setting(&db, KEY_SETTING)
            .map_err(|e| e.to_string())?
            .and_then(|sealed| platform.unseal(&sealed))
            .and_then(|k| String::from_utf8(k).ok());
        let model = store::setting(&db, MODEL_SETTING)
            .ok()
            .flatten()
            .and_then(|m| String::from_utf8(m).ok())
            .unwrap_or_else(|| gemini::DEFAULT_MODEL.to_string());
        let known = store::translations(&db)
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|t| {
                Some(Known { fp: fingerprint::from_hex(&t.fingerprint)?, hex: t.fingerprint, aspect: t.aspect, status: t.status, ko: t.translation_ko })
            })
            .collect();
        let state = State { db, key, model, known, session: None, generation: 0, gemini_requests: 0, notice: None };
        Ok(Backend {
            state: Arc::new(Mutex::new(state)),
            lifecycle: Mutex::new(()),
            platform,
            data_dir,
            gemini_base: gemini_base.unwrap_or_else(|| gemini::DEFAULT_BASE.to_string()),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- P1 .. P5

    /// A notice from the screen side, shown by P1 (`detector_missing`).
    pub fn notify(&self, notice: &'static str) {
        self.lock().notice = Some(notice);
    }

    /// The screen side works again: its notice, if still shown, goes.
    pub fn clear_notice(&self, notice: &'static str) {
        let mut st = self.lock();
        if st.notice == Some(notice) {
            st.notice = None;
        }
    }

    /// The local model could not look at screen rows `range` of frame
    /// `frame_seq`, which P6 had marked as detected: they are asked again later.
    pub fn detect_failed(&self, picture_id: i64, frame_seq: u64, range: Range) {
        let mut st = self.lock();
        let Some(s) = st.session.as_mut() else { return };
        let Ok(offset) = s.frame_offset(frame_seq, picture_id) else { return };
        if let Some(pic) = s.pics.get_mut(&picture_id) {
            pic.failed((offset + range.top as i64, offset + range.bottom as i64), now_ms());
        }
    }

    pub fn status(&self) -> Status {
        let st = self.lock();
        let perms = self.platform.permissions();
        Status {
            on: st.session.is_some(),
            mode: st.session.as_ref().and_then(|s| s.mode).unwrap_or("idle"),
            key_set: st.key.is_some(),
            key_tail: st.key.as_deref().map(tail),
            capture_allowed: perms.capture,
            overlay_allowed: perms.overlay,
            gemini_requests: st.gemini_requests,
            notice: st.notice,
        }
    }

    pub fn put_key(&self, req: KeyIn) -> ApiResult<KeyOut> {
        let key = req.key.trim().to_string();
        let mut st = self.lock();
        if key.is_empty() {
            store::delete_setting(&st.db, KEY_SETTING)?;
            st.key = None;
            return Ok(KeyOut { key_set: false, key_tail: None });
        }
        if !key_ok(&key) {
            return Err(ApiError("key_format"));
        }
        // No secure store on this device: refuse rather than keep the key weakly.
        let sealed = self.platform.seal(key.as_bytes()).ok_or(ApiError("secure_storage_unavailable"))?;
        store::put_setting(&st.db, KEY_SETTING, &sealed, now_ms())?;
        let tail = tail(&key);
        st.key = Some(key);
        if st.notice == Some("key_invalid") {
            st.notice = None;
        }
        Ok(KeyOut { key_set: true, key_tail: Some(tail) })
    }

    pub fn request_permissions(&self) -> PermissionsOut {
        let p = self.platform.request_permissions();
        PermissionsOut { capture_allowed: p.capture, overlay_allowed: p.overlay }
    }

    pub fn start(&self) -> ApiResult<StartOut> {
        let _life = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = self.lock();
        if st.session.is_some() {
            return Err(ApiError("already_on"));
        }
        if st.key.is_none() {
            return Err(ApiError("key_missing"));
        }
        let perms = self.platform.permissions();
        if !perms.capture || !perms.overlay {
            return Err(ApiError("permission_required"));
        }
        self.platform.start().map_err(|e| {
            eprintln!("platform start failed: {e}");
            ApiError("permission_required")
        })?;
        let now = now_ms();
        let book_id = match store::create_book(&st.db, &title(now), now) {
            Ok(id) => id,
            Err(e) => {
                // No session to stop later: capture and the overlay come down now.
                drop(st);
                self.platform.stop();
                return Err(e.into());
            }
        };
        st.generation += 1;
        st.gemini_requests = 0;
        st.notice = None;
        st.session = Some(Session {
            gen: st.generation,
            book_id,
            pics: HashMap::new(),
            current: None,
            last_pic: None,
            prev: None,
            hidden_prev: vec![],
            screen: (0, 0),
            rows: 0,
            fixed: (0, 0),
            side: (0, 0),
            scroll_y: 0,
            scroll_f: 0.0,
            anchor: None,
            drawn_before: 0,
            entries: [None; 2],
            turning: false,
            words_changed: None,
            left_for_words: None,
            mode: None,
            last_seq: None,
            frames: VecDeque::new(),
            pending: vec![],
            revision: 0,
            agent: gemini::agent(),
            overlay_in_capture: self.platform.overlay_in_capture(),
        });
        Ok(StartOut { on: true, book_id })
    }

    pub fn stop(&self) -> ApiResult<StopOut> {
        let _life = self.lifecycle.lock().unwrap_or_else(|e| e.into_inner());
        let mut st = self.lock();
        let Some(s) = st.session.take() else { return Err(ApiError("not_on")) };
        // Requests in flight are cut off and the agent refuses new ones, so a
        // worker past its last check sends nothing; anything that still comes
        // back belongs to this generation and gets dropped.
        st.generation += 1;
        s.agent.clone().shutdown();
        let now = now_ms();
        let saved = (|| -> ApiResult<u32> {
            for id in s.pics.keys() {
                store::close_picture(&st.db, *id, now)?;
            }
            store::end_book(&st.db, s.book_id, now)?;
            Ok(store::pictures(&st.db, s.book_id)?.len() as u32)
        })();
        drop(st);
        // Capture and the overlay come down whether or not the book could be
        // closed (a book left open is closed on the next start, `store::open`).
        self.platform.stop();
        Ok(StopOut { on: false, book_id: Some(s.book_id), picture_count: saved? })
    }

    // ---- P6

    pub fn frame(&self, req: FrameIn) -> ApiResult<FrameOut> {
        self.frame_hiding(req, vec![])
    }

    /// [`Backend::frame`] of a thumbnail whose pixels `hidden` (one per
    /// pixel, empty: none) hold something drawn over every app that stays
    /// put: this app's own floating button, blacked out by the capture loop.
    /// They are left out of the comparisons with the frames before, as the
    /// overlay is, so a fixed black block cannot outweigh the content that
    /// scrolls. (Fixed bands are still found with them: a row the button is
    /// in can still be a toolbar's.)
    pub fn frame_hiding(&self, req: FrameIn, hidden: Vec<bool>) -> ApiResult<FrameOut> {
        let mut guard = self.lock();
        let st = &mut *guard;
        let s = st.session.as_mut().ok_or(ApiError("not_on"))?;
        if s.last_seq.is_some_and(|l| req.seq <= l) {
            return Err(ApiError("stale_seq"));
        }
        let thumb = req.thumb;
        if req.width == 0 || req.height == 0 || thumb.is_empty() || thumb.len() % THUMB_W != 0 {
            return Err(ApiError("bad_frame"));
        }
        s.last_seq = Some(req.seq);
        // An erased shape may be in this frame, the one before it or the
        // resting screen scrolls are measured against: left out of the
        // comparisons until none of them was taken before it went.
        let anchor_seq = s.anchor.as_ref().map_or(u64::MAX, |a| a.2);
        if let Some(pic) = s.pic() {
            pic.erased.retain(|&(_, from)| req.seq <= from || anchor_seq < from);
        }
        let rows = thumb.len() / THUMB_W;
        let now = now_ms();
        let mut clear = false;
        if s.screen != (req.width, req.height) || s.rows != rows {
            // A new screen size (the phone turned): its rows and coordinates do
            // not continue the picture on screen, so a new picture starts.
            if s.screen != (0, 0) {
                if let Some(id) = s.current.take() {
                    store::close_picture(&st.db, id, now)?;
                }
                s.turning = false;
                clear = true;
            }
            s.screen = (req.width, req.height);
            s.rows = rows;
            s.prev = None;
            s.hidden_prev = vec![];
            s.fixed = (0, 0);
            s.side = (0, 0);
            s.anchor = None;
        }
        let scale = s.scale();
        // What this frame or the one before hid (the button may have moved).
        let hidden = if hidden.len() == thumb.len() { hidden } else { vec![] };
        let hide = joined(std::mem::replace(&mut s.hidden_prev, hidden.clone()), &hidden);

        // The overlay is left out as in the comparisons below: on Android the
        // capture holds the white shapes over a blocked app as well.
        // A dark page whose shapes cover all that is lit looks the same, but
        // it looked so a frame before too: blocked is the screen going black
        // around the shapes, or black with no shapes at all.
        let shapes = s.overlay_mask(&[s.scroll_y, s.drawn_before]);
        let went_dark = s.prev.as_ref().is_none_or(|p| p.len() != thumb.len() || !tracker::is_blocked(p, &shapes));
        if tracker::is_blocked(&thumb, &[]) || (tracker::is_blocked(&thumb, &shapes) && went_dark) {
            st.notice = Some("capture_blocked");
            let s = st.session.as_mut().unwrap();
            return Ok(FrameOut {
                motion: "blocked",
                dy: 0,
                mode: s.mode.unwrap_or("scroll"),
                picture_id: s.current.or(s.last_pic).unwrap_or(0),
                detect: None,
                save: None,
                clear: false,
                placements: s.visible(),
            });
        }
        if st.notice == Some("capture_blocked") {
            st.notice = None;
        }
        let s = st.session.as_mut().unwrap();
        let db = &st.db;

        let mut motion = "still";
        let mut dy = 0i32;
        let mut settled = false;
        // The overlay in this frame was drawn for the last frame, the one in
        // the previous frame for the frame before: both are left out.
        let entry_scroll = s.scroll_y;
        let shapes = s.overlay_mask(&[entry_scroll, s.drawn_before]);
        let mask = joined(joined(shapes.clone(), &hide), &tracker::col_mask(s.side, rows));
        s.drawn_before = entry_scroll;
        // Nothing placed, taken away or moved since the frame before last:
        // this frame and the last hold the same overlay, so a change under
        // it is the page under it changing (the overlay lets it through, or
        // is not in the capture).
        let entry = Some((s.revision, entry_scroll));
        let same_overlay = s.entries == [entry, entry];
        s.entries = [entry, s.entries[0]];
        match s.prev.take() {
            None => {
                if s.current.is_none() && !s.turning {
                    let id = store::create_picture(db, s.book_id, "page", req.width)?;
                    s.pics.insert(id, new_pic(id, s.screen));
                    s.current = Some(id);
                    s.last_pic = Some(id);
                    s.set_scroll(0.0);
                }
            }
            Some(prev) => {
                let (top, bottom) = s.fixed;
                let diff = tracker::still_diff(&prev, &thumb, &mask, 0, rows);
                // A small thing on a flat page moving a row a frame changes
                // little on average: it is still only if no shift matches it
                // better than none.
                let crept = diff > 0.0
                    && diff <= tracker::STILL
                    && tracker::fine_shift(&prev, &thumb, &mask, rows, top, bottom, true).is_some_and(|m| m.abs() >= 0.5);
                // A page alike but for the words in its balloons: a page turn.
                // Only a screen that has not moved is told by its words: the
                // places compared are where the balloons were, and a scroll
                // moves other drawing into them.
                let still = diff <= tracker::STILL && !crept;
                let shown = s.current.and_then(|id| s.pics.get(&id));
                // Or the words under the shapes were looked at and are others.
                let looked = s.words_changed.take().is_some_and(|id| s.current == Some(id));
                let other = still
                    && (looked
                        || (((s.scroll_y == 0 && shown.is_some_and(|p| s.other_words(p, &thumb)))
                            || (same_overlay && shown.is_some_and(|p| words_differ(p, s.scroll_y, &prev, &thumb))))
                            && tracker::fine_shift(&prev, &thumb, &mask, rows, top, bottom, true).is_none_or(|m| m.abs() < 0.1)));
                if looked && other {
                    s.left_for_words = s.current;
                }
                if still && !other {
                    settled = true;
                } else {
                    let (ft, fb) = tracker::fixed_rows(&prev, &thumb, &shapes, rows);
                    // Side columns that stayed put (a panel, another window
                    // beside the one that scrolls) would keep every shift
                    // from matching: they are left out of the search.
                    let side = tracker::fixed_cols(&prev, &thumb, &shapes, rows);
                    let mask = joined(mask.clone(), &tracker::col_mask(side, rows));
                    let shift = if other {
                        None
                    } else {
                        tracker::fine_shift(&prev, &thumb, &mask, rows, ft, fb, false)
                            .map(|m| (m, (ft, fb)))
                            .or_else(|| tracker::fine_shift(&prev, &thumb, &mask, rows, top, bottom, false).map(|m| (m, (top, bottom))))
                    };
                    match shift {
                        Some((rows_moved, fixed)) => {
                            s.fixed = fixed;
                            s.side = side;
                            if s.turning {
                                self.finish_turn(db, s, &prev)?;
                            }
                            motion = "scroll";
                            let before = s.scroll_y;
                            s.set_scroll(s.scroll_f + rows_moved * scale);
                            dy = (s.scroll_y - before) as i32;
                            s.mode = Some("scroll");
                            let h = req.height as i64;
                            let band = ((s.fixed.0 as f64 * scale) as i64, (s.fixed.1 as f64 * scale) as i64);
                            if let Some(pic) = s.pic() {
                                pic.reopen();
                                // Rows saved and detected while no band was known held
                                // the band, not content: the content that scrolls
                                // into them is asked for like any new rows.
                                if band != (0, 0) {
                                    if let Some(y0) = pic.unbanded.take() {
                                        for r in [(y0, y0 + band.0), (y0 + h - band.1, y0 + h)].into_iter().filter(|r| r.1 > r.0) {
                                            subtract(&mut pic.covered, r);
                                            subtract(&mut pic.detected, r);
                                        }
                                    }
                                }
                                if pic.kind == "page" {
                                    pic.kind = "stitched";
                                    pic.page_thumb = None;
                                    let id = pic.id;
                                    store::set_picture_kind(db, id, "stitched")?;
                                    store::set_picture_thumb(db, id, None)?;
                                }
                            }
                        }
                        None => {
                            // Matches at no distance: a page is turning. Clear the overlay first.
                            motion = "page";
                            clear = true;
                            s.mode = Some("slide");
                            if let Some(id) = s.current.take() {
                                store::close_picture(db, id, now)?;
                            }
                            s.turning = true;
                            s.set_scroll(0.0);
                            s.anchor = None;
                            // The band belonged to the screen before: a new page is
                            // taken whole until a scroll shows a band again.
                            s.fixed = (0, 0);
                            s.side = (0, 0);
                        }
                    }
                }
            }
        }

        let mut detect = None;
        let mut save = None;
        if settled {
            if s.turning {
                self.finish_turn(db, s, &thumb)?;
            }
            // Measured again from the last resting screen in one step.
            let mut kept = None;
            if let Some((a, f, at)) = s.anchor.take() {
                let mask = joined(joined(s.overlay_mask(&[f.round() as i64, s.scroll_y, s.drawn_before]), &hide), &tracker::col_mask(s.side, rows));
                match tracker::fine_shift(&a, &thumb, &mask, rows, s.fixed.0, s.fixed.1, true) {
                    // Not a tenth of a row from it: still that screen. It is
                    // kept, so a scroll too slow to tell frame by frame adds
                    // up against it instead of being lost every frame.
                    Some(m) if m.abs() < 0.1 => {
                        s.set_scroll(f);
                        kept = Some((a, f, at));
                    }
                    Some(m) => s.set_scroll(f + m * scale),
                    None => {}
                }
            }
            s.anchor = Some(kept.unwrap_or_else(|| (thumb.clone(), s.scroll_f, req.seq)));
            let (h, ft, fb) = (req.height as i64, (s.fixed.0 as f64 * scale) as i64, (s.fixed.1 as f64 * scale) as i64);
            let scroll_y = s.scroll_y;
            let no_band = s.fixed == (0, 0);
            if let Some(pic) = s.pic() {
                let vis = (scroll_y + ft, scroll_y + h - fb);
                if no_band && pic.unbanded.is_none() {
                    pic.unbanded = Some(scroll_y);
                }
                if now >= pic.retry_at {
                    pic.reopen();
                }
                if let Some(d) = new_part(vis, &pic.detected) {
                    add(&mut pic.detected, d);
                    detect = Some(Range { top: (d.0 - scroll_y) as u32, bottom: (d.1 - scroll_y) as u32 });
                }
                if let Some(d) = new_part(vis, &pic.covered) {
                    save = Some(Range { top: (d.0 - scroll_y) as u32, bottom: (d.1 - scroll_y) as u32 });
                }
                if pic.kind == "page" && scroll_y == 0 && pic.page_thumb.is_none() {
                    pic.page_thumb = Some(thumb.clone());
                    let fp = fingerprint::to_hex(fingerprint::dhash(&thumb, THUMB_W, rows));
                    store::set_picture_thumb(db, pic.id, Some(&fp))?;
                }
            }
        }
        if motion != "still" {
            s.revision += 1;
        }
        s.prev = Some(thumb);
        if let Some(id) = s.current {
            let asked = save.map(|r| (s.scroll_y + r.top as i64, s.scroll_y + r.bottom as i64));
            s.frames.push_back((req.seq, id, s.scroll_y, asked));
            if s.frames.len() > 64 {
                s.frames.pop_front();
            }
        }
        Ok(FrameOut {
            motion,
            dy,
            mode: s.mode.unwrap_or("scroll"),
            picture_id: s.current.or(s.last_pic).unwrap_or(0),
            detect,
            save,
            clear,
            placements: s.visible(),
        })
    }

    /// The page change settled on `thumb`: back on a page seen before, or a new page.
    /// The text areas (screen pixels, by placement id) of picture
    /// `picture_id` wholly on the screen now, where the capture holds the
    /// overlay (A6): there the shapes hide the words under them, and a page
    /// turned without any slide, its drawing the same, changes nothing the
    /// capture sees. The capture loop looks under the shapes after a tap that
    /// left the screen still (`pipeline::step`) and sends what it saw to
    /// [`Backend::words_under`]. Empty where nothing needs looking at.
    pub fn text_areas(&self, picture_id: i64) -> Vec<(i64, (u32, u32, u32, u32))> {
        let st = self.lock();
        let Some(s) = st.session.as_ref().filter(|s| s.overlay_in_capture && !s.turning && s.current == Some(picture_id)) else { return vec![] };
        let Some(pic) = s.pics.get(&picture_id) else { return vec![] };
        pic.texts
            .iter()
            .filter(|(id, _)| pic.placements.iter().any(|p| p.id == *id))
            .filter_map(|&(id, (x, y, w, h))| {
                let y = y - s.scroll_y;
                let fits = x >= 0 && y >= 0 && x + w as i64 <= s.screen.0 as i64 && y + h as i64 <= s.screen.1 as i64;
                fits.then_some((id, (x as u32, y as u32, w, h)))
            })
            .collect()
    }

    /// What is under the shapes now: for each placement id from
    /// [`Backend::text_areas`], a PNG of the screen rows of its text area,
    /// from a frame taken with the shapes hidden. If most of them no longer
    /// hold what their translation was made for (the fingerprint, as R23),
    /// another page is showing: the next frame turns to it. Answered with
    /// whether it does.
    pub fn words_under(&self, picture_id: i64, seen: &[(i64, Vec<u8>)]) -> ApiResult<bool> {
        let mut st = self.lock();
        let s = st.session.as_mut().ok_or(ApiError("not_on"))?;
        if s.current != Some(picture_id) || s.turning {
            return Ok(false);
        }
        let Some(pic) = s.pics.get(&picture_id) else { return Ok(false) };
        let (mut looked, mut other) = (0, 0);
        for (id, png) in seen {
            let (Some(&(_, (x, _, w, h))), Some(p)) = (pic.texts.iter().find(|t| t.0 == *id), pic.placements.iter().find(|p| p.id == *id)) else { continue };
            let Some(img) = export::decode_png(png).filter(|i| i.h == h && x >= 0 && x as u32 + w <= i.w) else { continue };
            let gray = img.gray();
            let x = x as usize;
            let area: Vec<u8> = gray.chunks(img.w as usize).flat_map(|row| row[x..x + w as usize].iter().copied()).collect();
            let fp = fingerprint::dhash(&area, w as usize, h as usize);
            let same = fingerprint::from_hex(&p.fingerprint).is_some_and(|f| (f ^ fp).count_ones() <= fingerprint::MAX_BITS);
            looked += 1;
            if !same {
                other += 1;
            }
        }
        let turned = looked > 0 && other * 2 > looked;
        if turned {
            s.words_changed = Some(picture_id);
        }
        Ok(turned)
    }

    fn finish_turn(&self, db: &Db, s: &mut Session, thumb: &[u8]) -> ApiResult<()> {
        s.turning = false;
        s.set_scroll(0.0);
        s.anchor = None;
        let rows = thumb.len() / THUMB_W;
        // Only a page seen on a screen of this size: the same thumbnail on a
        // screen of another resolution would put its placements and strips
        // in the wrong pixels.
        let screen = s.screen;
        // A page left because its words were seen to change is not this one,
        // however alike the drawing (`words_under`).
        let left = s.left_for_words.take();
        let seen = s.pics.values().find(|p| {
            Some(p.id) != left
                && p.screen == screen
                && p.page_thumb.as_ref().is_some_and(|t| t.len() == thumb.len() && tracker::still_diff(t, thumb, &[], 0, rows) <= tracker::MATCH)
                // The overlay was cleared for the turn (A6): this frame shows
                // the page's own words wherever its translations were.
                && !words_differ(p, 0, p.page_thumb.as_deref().unwrap_or_default(), thumb)
        });
        let id = match seen.map(|p| p.id) {
            Some(id) => {
                store::reopen_picture(db, id)?;
                // Back on this page: what failed here is tried again.
                s.pics.get_mut(&id).unwrap().reopen();
                id
            }
            None => {
                let id = store::create_picture(db, s.book_id, "page", s.screen.0)?;
                s.pics.insert(id, new_pic(id, s.screen));
                id
            }
        };
        s.current = Some(id);
        s.last_pic = Some(id);
        s.revision += 1;
        Ok(())
    }

    // ---- P7

    pub fn crops(&self, req: CropsIn) -> ApiResult<CropsOut> {
        let mut guard = self.lock();
        let st = &mut *guard;
        let s = st.session.as_mut().ok_or(ApiError("not_on"))?;
        if s.current != Some(req.picture_id) {
            return Err(ApiError("picture_closed"));
        }
        let key = st.key.clone().ok_or(ApiError("key_missing"))?;
        let s = st.session.as_mut().unwrap();
        let offset = s.frame_offset(req.frame_seq, req.picture_id)?;
        let mut result = CropsOut { placed: vec![], pending: 0, skipped_effects: 0, discarded: 0 };
        let mut jobs: Vec<Job> = vec![];
        let now = now_ms();
        // A crop that fails does not hold up the rest: what was queued before
        // it is asked for all the same (it is pending, and the same crop sent
        // again only waits for that answer), and its own rows are looked at
        // again later (on a scroll, the page's return, or after a while).
        let mut failed = None;
        for c in req.crops {
            let rows = (c.y as i64 + offset, c.y as i64 + c.h as i64 + offset);
            if let Err(e) = take_crop(st, req.picture_id, c, (req.frame_seq, offset), now, &mut result, &mut jobs) {
                eprintln!("crop of picture {} at rows {rows:?}: {e}", req.picture_id);
                if let Some(pic) = st.session.as_mut().and_then(|s| s.pics.get_mut(&req.picture_id)) {
                    pic.failed(rows, now);
                }
                failed.get_or_insert(e);
            }
        }
        self.send_jobs(guard, key, jobs);
        failed.map_or(Ok(result), Err)
    }

    fn send_jobs(&self, guard: MutexGuard<'_, State>, key: String, jobs: Vec<Job>) {
        let Some(s) = guard.session.as_ref().filter(|_| !jobs.is_empty()) else { return };
        let (gen, agent) = (s.gen, s.agent.clone());
        let model = guard.model.clone();
        drop(guard);
        self.spawn_jobs(gen, agent, key, model, jobs);
    }

    fn spawn_jobs(&self, gen: u64, agent: ureq::Agent, key: String, model: String, jobs: Vec<Job>) {
        let state = self.state.clone();
        let platform = self.platform.clone();
        let base = self.gemini_base.clone();
        std::thread::spawn(move || {
            // Checked and counted under the lock right before each send, so
            // nothing goes out once translation is switched off.
            let may_send = || {
                let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                if st.generation != gen {
                    return false;
                }
                st.gemini_requests += 1;
                true
            };
            let client = gemini::Client { agent: &agent, base: &base, model: &model, key: &key };
            let pngs: Vec<Vec<u8>> = jobs.iter().map(|j| j.png.clone()).collect();
            let outcome = client.read(&pngs, &may_send).and_then(|ja| {
                let lines: Vec<String> = ja.iter().filter(|j| !j.is_empty()).cloned().collect();
                if lines.is_empty() {
                    return Ok((ja, None));
                }
                match client.translate(&lines, &may_send) {
                    Ok(ko) => Ok((ja, Some(Ok(ko)))),
                    Err(GeminiError::Unreadable) => Ok((ja, Some(Err(())))),
                    Err(e) => Err(e),
                }
            });
            finish_jobs(&state, platform.as_ref(), gen, &model, jobs, outcome);
        });
    }

    // ---- P8

    pub fn placements(&self, _req: PlacementsIn) -> ApiResult<PlacementsOut> {
        let st = self.lock();
        let s = st.session.as_ref().ok_or(ApiError("not_on"))?;
        Ok(PlacementsOut { revision: s.revision, offset_y: s.scroll_y, placements: s.visible() })
    }

    // ---- P9

    pub fn strip(&self, req: StripIn) -> ApiResult<StripOut> {
        let mut guard = self.lock();
        let st = &mut *guard;
        let s = st.session.as_mut().ok_or(ApiError("not_on"))?;
        if s.current != Some(req.picture_id) {
            return Err(ApiError("picture_closed"));
        }
        if req.bottom <= req.top {
            return Err(ApiError("already_covered"));
        }
        let offset = s.frame_offset(req.frame_seq, req.picture_id)?;
        // Only rows P6 asked this frame for: never rows outside the captured
        // screen or a fixed band, whatever range is sent.
        let asked = s.frames.iter().rev().find(|f| f.0 == req.frame_seq && f.1 == req.picture_id).and_then(|f| f.3);
        let scale = s.scale();
        // The fixed band does not move with the content: never stored after the first screen.
        let fixed_top = (s.fixed.0 as f64 * scale) as u32;
        let pic = s.pics.get_mut(&req.picture_id).unwrap();
        if pic.screen != s.screen {
            // Never reached while P6 closes a picture when the screen size
            // changes: its strips must all be the picture's width.
            return Err(ApiError("picture_closed"));
        }
        let mut top = req.top;
        if !pic.covered.is_empty() {
            top = top.max(fixed_top);
        }
        let range = asked.map(|a| ((offset + top as i64).max(a.0), (offset + req.bottom as i64).min(a.1)));
        // Only what is not saved yet is kept, gaps included.
        let parts = range.filter(|r| r.1 > r.0).map(|r| missing(r, &pic.covered)).unwrap_or_default();
        if parts.is_empty() {
            return Err(ApiError("already_covered"));
        }
        // The strip must really be screen rows top..bottom, or the export is wrong.
        let img = export::decode_png(&req.image).ok_or(ApiError("bad_image"))?;
        if img.w != s.screen.0 || img.h != req.bottom - req.top {
            return Err(ApiError("bad_image"));
        }
        // E5: lossless WebP in the app's own folder, kept as a path relative to it.
        let rel_dir = format!("strips/{}", req.picture_id);
        std::fs::create_dir_all(self.data_dir.join(&rel_dir)).map_err(storage_error)?;
        // The strips and the picture's range are one commit: a kill or a full
        // disk in between leaves neither, never a strip outside the range.
        let mut covered = pic.covered.clone();
        let mut written = vec![];
        let saved = (|| -> ApiResult<i64> {
            let tx = st.db.unchecked_transaction()?;
            let mut first_id = None;
            for &range in &parts {
                let (a, b) = ((range.0 - offset) as u32, (range.1 - offset) as u32);
                let bytes = export::encode_webp(&export::rows(&img, a - req.top, b - req.top)).map_err(encode_error)?;
                let rel = format!("{rel_dir}/{}-{}.webp", range.0, now_ms());
                let path = self.data_dir.join(&rel);
                written.push(path.clone());
                std::fs::write(&path, &bytes).map_err(storage_error)?;
                let id = store::insert_strip(&tx, req.picture_id, range.0, (range.1 - range.0) as u32, &rel, now_ms())?;
                first_id.get_or_insert(id);
                add(&mut covered, range);
            }
            let (top, bottom) = hull(&covered).unwrap_or_default();
            store::set_covered(&tx, req.picture_id, top, bottom)?;
            tx.commit()?;
            Ok(first_id.unwrap_or_default())
        })();
        let strip_id = match saved {
            Ok(id) => id,
            Err(e) => {
                for p in written {
                    let _ = std::fs::remove_file(p);
                }
                return Err(e);
            }
        };
        let s = st.session.as_mut().unwrap();
        let hull = hull(&covered).unwrap_or_default();
        s.pics.get_mut(&req.picture_id).unwrap().covered = covered;
        Ok(StripOut { strip_id, picture_height: (hull.1 - hull.0) as u32 })
    }

    // ---- P10, P11

    pub fn books(&self, req: BooksIn) -> ApiResult<BooksOut> {
        let st = self.lock();
        let limit = req.limit.clamp(1, 200);
        let mut rows = store::books(&st.db, limit + 1, req.before)?;
        let next_before = if rows.len() > limit as usize {
            rows.truncate(limit as usize);
            rows.last().map(|b| b.id)
        } else {
            None
        };
        let mut books = vec![];
        for b in rows {
            let mut pictures = vec![];
            for p in store::pictures(&st.db, b.id)? {
                let preview = self.strips(&st.db, p.id)?.into_iter().next().map(|s| s.file_path).unwrap_or_default();
                pictures.push(PictureOut {
                    id: p.id,
                    kind: p.kind,
                    width: p.width,
                    height: (p.covered_bottom - p.covered_top).max(0) as u32,
                    closed: p.closed,
                    preview_path: preview,
                });
            }
            books.push(BookOut { id: b.id, title: b.title, started_at: b.started_at, ended_at: b.ended_at, pictures });
        }
        Ok(BooksOut { books, next_before })
    }

    pub fn export(&self, req: ExportIn) -> ApiResult<ExportOut> {
        let ext = match req.format.as_str() {
            "png" => "png",
            "webp" => "webp",
            _ => return Err(ApiError("bad_format")),
        };
        let (book, pictures) = {
            let st = self.lock();
            let book = store::book(&st.db, req.book_id)?.ok_or(ApiError("not_found"))?;
            let mut pictures = vec![];
            for p in store::pictures(&st.db, book.id)? {
                let strips = self.strips(&st.db, p.id)?;
                let placements = store::placements(&st.db, p.id)?;
                pictures.push((p, strips, placements));
            }
            (book, pictures)
        };
        if pictures.is_empty() {
            return Err(ApiError("empty_book"));
        }
        let dir = self.platform.export_dir();
        std::fs::create_dir_all(&dir).map_err(storage_error)?;
        let stem = book.title.replace([':', ' ', '/', '\\'], "-");
        let mut files = vec![];
        for (i, (p, strips, placements)) in pictures.iter().enumerate() {
            let (top, bottom) = (p.covered_top, p.covered_bottom.max(p.covered_top + 1));
            if ext == "png" {
                // Streamed band by band: a long scroll picture never sits in memory whole.
                let (path, file) = export::create_free(&dir, &format!("translated-{stem}-{}", i + 1), ext).map_err(storage_error)?;
                let written = export::write_png(std::io::BufWriter::new(file), p.width, top, bottom, strips, placements);
                if let Err(e) = written {
                    let _ = std::fs::remove_file(&path);
                    return Err(storage_error(e));
                }
                files.push(self.publish(&path)?);
                continue;
            }
            // WebP is at most 16383 rows: a longer picture goes out in parts.
            let many = bottom - top > export::WEBP_MAX as i64;
            for (k, t) in (top..bottom).step_by(export::WEBP_MAX as usize).enumerate() {
                let part = export::render(p.width, t, (t + export::WEBP_MAX as i64).min(bottom), strips, placements).map_err(storage_error)?;
                let bytes = export::encode_webp(&part).map_err(encode_error)?;
                let stem = if many { format!("translated-{stem}-{}-{}", i + 1, k + 1) } else { format!("translated-{stem}-{}", i + 1) };
                let (path, mut file) = export::create_free(&dir, &stem, ext).map_err(storage_error)?;
                if let Err(e) = std::io::Write::write_all(&mut file, &bytes) {
                    drop(file);
                    let _ = std::fs::remove_file(&path);
                    return Err(storage_error(e));
                }
                drop(file);
                files.push(self.publish(&path)?);
            }
        }
        Ok(ExportOut { count: files.len() as u32, files })
    }

    /// The picture's strips with their files as full paths (stored relative
    /// to the app's folder; a full path stored earlier stays as it is).
    fn strips(&self, db: &Db, picture_id: i64) -> ApiResult<Vec<store::StripRow>> {
        let mut rows = store::strips(db, picture_id)?;
        for s in &mut rows {
            s.file_path = self.data_dir.join(&s.file_path).to_string_lossy().into_owned();
        }
        Ok(rows)
    }

    fn publish(&self, path: &std::path::Path) -> ApiResult<String> {
        self.platform.publish(path).map_err(|e| {
            eprintln!("cannot hand {} over: {e}", path.display());
            let _ = std::fs::remove_file(path);
            // The platform names a full disk (`Platform::publish`); anything else is a refusal.
            if e.contains("storage_full") {
                ApiError("storage_full")
            } else {
                ApiError("storage_denied")
            }
        })
    }
}

/// Mask `m` with the pixels of `extra` added (either may be empty: none).
fn joined(mut m: Vec<bool>, extra: &[bool]) -> Vec<bool> {
    if m.is_empty() {
        return extra.to_vec();
    }
    if m.len() == extra.len() {
        m.iter_mut().zip(extra).for_each(|(a, &b)| *a |= b);
    }
    m
}

fn new_pic(id: i64, screen: (u32, u32)) -> Pic {
    Pic { id, kind: "page", screen, covered: vec![], detected: vec![], placements: vec![], texts: vec![], page_thumb: None, retry: vec![], retry_at: 0, unbanded: None, edge_rects: vec![], erased: vec![] }
}

fn storage_error(e: std::io::Error) -> ApiError {
    eprintln!("storage error: {e}");
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => ApiError("storage_denied"),
        std::io::ErrorKind::InvalidData => ApiError("strip_unreadable"),
        _ => ApiError("storage_full"),
    }
}

fn encode_error(e: String) -> ApiError {
    eprintln!("cannot encode a picture: {e}");
    ApiError("encode_failed")
}

/// Store a placement of `ko` over the text area `rect` (picture coordinates),
/// unless its white shape, grown to hold the text, would cover more than
/// half of a placement already there (or be more than half covered): the
/// translation shown first stays (E6).
/// Whether `big` holds all of `small` (give or take [`detect::EDGE`]) and more of it.
fn grows(small: (i64, i64, u32, u32), big: (i64, i64, u32, u32)) -> bool {
    let e = detect::EDGE as i64;
    let holds = big.0 <= small.0 + e
        && big.1 <= small.1 + e
        && big.0 + big.2 as i64 + e >= small.0 + small.2 as i64
        && big.1 + big.3 as i64 + e >= small.1 + small.3 as i64;
    holds && big.3 as i64 >= small.3 as i64 + e
}

/// A text area taken at the screen's edge or a fixed band's that `rect`
/// shows larger was cut there: its placements and the answer still awaited for it go. Where the
/// capture holds the overlay their shapes are remembered as erased.
fn replace_cut(st: &mut State, picture_id: i64, rect: (i64, i64, u32, u32)) -> ApiResult<()> {
    let s = st.session.as_mut().unwrap();
    // A status bar, navigation bar or toolbar that stays put cuts the
    // content at its edge as the screen's edge does.
    let scale = s.scale();
    let (bt, bb) = ((s.fixed.0 as f64 * scale) as i64, (s.fixed.1 as f64 * scale) as i64);
    let e = detect::EDGE as i64;
    let at_edge = |r: Rect, a: (i64, i64, i64, i64)| r.1 <= a.1 + bt + e || r.1 + r.3 as i64 >= a.3 - bb - e;
    // This frame and the next still show the overlay as it was drawn before
    // (`Backend::frame`): the frame after them is the first without the shape.
    let clean_from = s.last_seq.unwrap_or(0) + 2;
    let in_capture = s.overlay_in_capture;
    let pic = s.pics.get_mut(&picture_id).unwrap();
    let cut: Vec<_> = pic.edge_rects.iter().filter(|&&(r, a)| at_edge(r, a) && grows(r, rect)).map(|&(r, _)| r).collect();
    if cut.is_empty() {
        return Ok(());
    }
    pic.edge_rects.retain(|(r, _)| !cut.contains(r));
    let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut pic.placements).into_iter().partition(|p| cut.iter().any(|&e| overlap(e, (p.x, p.y, p.w, p.h)) > 0.5));
    pic.placements = kept;
    if in_capture {
        pic.erased.extend(gone.iter().map(|p| ((p.x, p.y, p.w, p.h), clean_from)));
    }
    for p in s.pending.iter_mut() {
        p.targets.retain(|t| !(t.0 == picture_id && cut.contains(&t.1)));
    }
    if !gone.is_empty() {
        s.revision += 1;
    }
    for p in gone {
        store::delete_placement(&st.db, p.id)?;
    }
    Ok(())
}

/// Whether thumbnails `a` and `b` of a screen at `scroll_y` differ in the
/// cells wholly under one of page `pic`'s placements: the words of its
/// balloons, where neither holds the overlay or both hold it alike.
fn words_differ(pic: &Pic, scroll_y: i64, a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let rows = b.len() / THUMB_W;
    let (sx, sy) = (pic.screen.0 as f64 / THUMB_W as f64, pic.screen.1 as f64 / rows.max(1) as f64);
    pic.placements.iter().any(|p| {
        let y = (p.y - scroll_y) as f64;
        let r = ((y / sy).ceil().max(0.0) as usize, (((y + p.h as f64) / sy).floor().max(0.0) as usize).min(rows));
        let c = ((p.x as f64 / sx).ceil().max(0.0) as usize, (((p.x + p.w as i64) as f64 / sx).floor().max(0.0) as usize).min(THUMB_W));
        let (mut sum, mut n) = (0u64, 0u64);
        for row in r.0..r.1 {
            for col in c.0..c.1 {
                let i = row * THUMB_W + col;
                sum += a[i].abs_diff(b[i]) as u64;
                n += 1;
            }
        }
        n > 0 && sum as f32 > tracker::MATCH * n as f32
    })
}

/// One P7 crop of picture `picture_id` (frame at `offset`): the stored
/// translation placed at once, or a Gemini job queued in `jobs`.
fn take_crop(st: &mut State, picture_id: i64, c: CropIn, (seq, offset): (u64, i64), now: i64, result: &mut CropsOut, jobs: &mut Vec<Job>) -> ApiResult<()> {
    let screen = st.session.as_ref().ok_or(ApiError("not_on"))?.screen;
    if c.kind == "effect" {
        result.skipped_effects += 1;
        return Ok(());
    }
    // Only the two kinds the model names, and only a text area on the
    // captured screen: anything else is no balloon to translate.
    let on_screen = c.w > 0 && c.h > 0 && c.x as u64 + c.w as u64 <= screen.0 as u64 && c.y as u64 + c.h as u64 <= screen.1 as u64;
    if c.kind != "dialogue" || !on_screen {
        result.discarded += 1;
        return Ok(());
    }
    // The picture must be the text area it says it is, or it is not
    // the crop it claims (a whole screen sent as a small crop).
    let Some(img) = export::decode_png(&c.image).filter(|i| (i.w, i.h) == (c.w, c.h)) else {
        result.discarded += 1;
        return Ok(());
    };
    let fp = fingerprint::dhash(&img.gray(), img.w as usize, img.h as usize);
    let aspect = c.w.max(1) as f32 / c.h.max(1) as f32;
    let rect = (c.x as i64, c.y as i64 + offset, c.w, c.h);
    let area = (0, offset, screen.0 as i64, offset + screen.1 as i64);
    // A text area at the screen's top or bottom edge, or at a fixed band's,
    // may be cut by it, or be whole there (a page that does not scroll): it
    // is read as it is, and remembered. Seen larger later, once a scroll
    // shows more of it, it was cut: what was placed or asked for it gives
    // way to the whole one.
    replace_cut(st, picture_id, rect)?;
    // A frame that may still show an erased shape over this text area shows
    // our white, not the Japanese under it: its rows are looked at again in
    // a later frame (A6).
    let pic = st.session.as_mut().unwrap().pics.get_mut(&picture_id).unwrap();
    if pic.erased.iter().any(|&(e, from)| seq < from && overlap(e, rect) > 0.0) {
        subtract(&mut pic.detected, (rect.1, rect.1 + rect.3 as i64));
        result.pending += 1;
        return Ok(());
    }
    // Every one is remembered: the band that cut it may be found only by a
    // later scroll.
    st.session.as_mut().unwrap().pics.get_mut(&picture_id).unwrap().edge_rects.push((rect, area));
    let s = st.session.as_mut().unwrap();
    let pic = s.pics.get_mut(&picture_id).unwrap();
    // Our own overlay is in the capture: a crop over a placement is either
    // the same balloon (already shown) or our Korean text (discarded).
    if let Some(p) = pic.placements.iter().find(|p| overlap(rect, (p.x, p.y, p.w, p.h)) > 0.5) {
        let same = fingerprint::from_hex(&p.fingerprint).is_some_and(|f| (f ^ fp).count_ones() <= fingerprint::MAX_BITS);
        if same {
            result.placed.push(out(p, s.scroll_y));
        } else {
            result.discarded += 1;
        }
        return Ok(());
    }
    if let Some(k) = st.find_known(fp, aspect).cloned() {
        store::touch_translation(&st.db, &k.hex, now)?;
        if k.status == "ready" {
            let ko = k.ko.unwrap_or_default();
            let placed = &st.session.as_ref().unwrap().pics[&picture_id].placements;
            let Some(row) = place(&st.db, picture_id, &k.hex, rect, area, &ko, placed)? else {
                result.discarded += 1;
                return Ok(());
            };
            let s = st.session.as_mut().unwrap();
            result.placed.push(out(&row, s.scroll_y));
            let pic = s.pics.get_mut(&picture_id).unwrap();
            pic.texts.push((row.id, rect));
            pic.placements.push(row);
            s.revision += 1;
        }
        return Ok(());
    }
    let s = st.session.as_mut().unwrap();
    result.pending += 1;
    // Already asked (earlier or in this batch): this place gets the same answer.
    if let Some(p) = s.pending.iter_mut().find(|p| fingerprint::same_balloon(fp, aspect, p.fp, p.aspect)) {
        p.targets.push((picture_id, rect, area));
        return Ok(());
    }
    s.pending.push(Pending { fp, aspect, targets: vec![(picture_id, rect, area)] });
    jobs.push(Job { fp, aspect, png: c.image });
    Ok(())
}

fn place(db: &Db, picture_id: i64, fingerprint: &str, rect: Rect, area: (i64, i64, i64, i64), ko: &str, placed: &[PlacementRow]) -> ApiResult<Option<PlacementRow>> {
    let shape = layout::fit_within(rect.0, rect.1, rect.2, rect.3, ko, area);
    if placed.iter().any(|p| overlap((shape.x, shape.y, shape.w, shape.h), (p.x, p.y, p.w, p.h)) > 0.5) {
        return Ok(None);
    }
    let mut row = PlacementRow {
        id: 0,
        fingerprint: fingerprint.to_string(),
        x: shape.x,
        y: shape.y,
        w: shape.w,
        h: shape.h,
        radius: shape.radius,
        font_px: shape.font_px,
        ko: ko.to_string(),
    };
    row.id = store::insert_placement(db, picture_id, &row)?;
    Ok(Some(row))
}

type Outcome = Result<(Vec<String>, Option<Result<Vec<String>, ()>>), GeminiError>;

fn finish_jobs(state: &Mutex<State>, platform: &dyn Platform, gen: u64, model: &str, jobs: Vec<Job>, outcome: Outcome) {
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
    let st = &mut *guard;
    // A result for a session that has stopped is dropped without touching state.
    let Some(s) = st.session.as_mut().filter(|s| s.gen == gen) else { return };
    // Every place each balloon was seen while it was being asked.
    let mut targets: Vec<Vec<Target>> = vec![];
    for j in &jobs {
        let i = s.pending.iter().position(|p| p.fp == j.fp && p.aspect == j.aspect);
        targets.push(i.map(|i| s.pending.remove(i).targets).unwrap_or_default());
    }
    let now = now_ms();
    let retry = |s: &mut Session, targets: &[Target]| {
        for &(picture_id, r, _) in targets {
            if let Some(pic) = s.pics.get_mut(&picture_id) {
                pic.failed((r.1, r.1 + r.3 as i64), now);
            }
        }
    };
    let rows: Vec<(Translation, &Job)> = match outcome {
        Err(GeminiError::Cancelled) => return,
        Err(e @ (GeminiError::KeyInvalid | GeminiError::Unreachable)) => {
            targets.iter().for_each(|t| retry(s, t));
            st.notice = Some(if e == GeminiError::KeyInvalid { "key_invalid" } else { "gemini_unreachable" });
            return;
        }
        Err(GeminiError::Unreadable) => jobs.iter().map(|j| (translation(j, None, None, "failed"), j)).collect(),
        Ok((ja, ko)) => {
            let mut ko_iter = match &ko {
                Some(Ok(list)) => list.clone().into_iter(),
                _ => Vec::new().into_iter(),
            };
            jobs.iter()
                .zip(ja)
                .map(|(j, ja)| {
                    if ja.is_empty() {
                        (translation(j, Some(ja), None, "skip"), j)
                    } else {
                        match (&ko, ko_iter.next()) {
                            (Some(Ok(_)), Some(k)) if !k.is_empty() => (translation(j, Some(ja), Some(k), "ready"), j),
                            _ => (translation(j, Some(ja), None, "failed"), j),
                        }
                    }
                })
                .collect()
        }
    };
    if matches!(st.notice, Some("key_invalid") | Some("gemini_unreachable")) {
        st.notice = None;
    }
    let mut changed = false;
    for ((t, job), targets) in rows.into_iter().zip(targets) {
        let ko = match store::put_translation(&st.db, &t, model, now) {
            Ok(true) => {
                st.known.retain(|k| k.hex != t.fingerprint);
                st.known.push(Known { fp: job.fp, hex: t.fingerprint.clone(), aspect: t.aspect, status: t.status.clone(), ko: t.translation_ko.clone() });
                (t.status == "ready").then(|| t.translation_ko.clone().unwrap_or_default())
            }
            // A ready translation under this key is never overwritten: show that one.
            Ok(false) => st.known.iter().find(|k| k.hex == t.fingerprint && k.status == "ready").and_then(|k| k.ko.clone()),
            Err(e) => {
                eprintln!("cannot store translation: {e}");
                None
            }
        };
        let Some(ko) = ko else {
            if t.status != "skip" {
                retry(st.session.as_mut().unwrap(), &targets);
            }
            continue;
        };
        for (picture_id, rect, area) in targets {
            let s = st.session.as_mut().unwrap();
            let Some(pic) = s.pics.get(&picture_id) else { continue };
            if pic.placements.iter().any(|p| overlap(rect, (p.x, p.y, p.w, p.h)) > 0.5) {
                continue;
            }
            match place(&st.db, picture_id, &t.fingerprint, rect, area, &ko, &pic.placements) {
                Ok(None) => {}
                Ok(Some(row)) => {
                    let s = st.session.as_mut().unwrap();
                    let pic = s.pics.get_mut(&picture_id).unwrap();
                    pic.texts.push((row.id, rect));
                    pic.placements.push(row);
                    changed = true;
                }
                // The translation is stored; its place is not. The rows are
                // detected again (on a scroll, a return, or after RETRY_MS),
                // and the crop then finds the stored translation.
                Err(e) => {
                    eprintln!("cannot place translation: {e}");
                    retry(st.session.as_mut().unwrap(), &[(picture_id, rect, area)]);
                }
            }
        }
    }
    let s = st.session.as_mut().unwrap();
    if changed {
        s.revision += 1;
        let payload = serde_json::json!({"revision": s.revision, "offset_y": s.scroll_y, "placements": s.visible()});
        platform.emit("placements", payload);
    }
}

fn translation(j: &Job, ja: Option<String>, ko: Option<String>, status: &str) -> Translation {
    Translation {
        fingerprint: fingerprint::key(j.fp, j.aspect),
        aspect: j.aspect,
        source_ja: ja,
        translation_ko: ko,
        status: status.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_part_is_what_the_screen_adds() {
        assert_eq!(new_part((300, 2700), &[(0, 2400)]), Some((2400, 2700)));
        assert_eq!(new_part((700, 3100), &[(0, 3600)]), None);
        assert_eq!(new_part((0, 2400), &[]), Some((0, 2400)));
        assert_eq!(new_part((-300, 2100), &[(0, 2400)]), Some((-300, 0)));
    }

    #[test]
    fn a_range_scrolled_past_stays_a_gap_until_saved() {
        let mut have = vec![];
        add(&mut have, (0, 2400));
        add(&mut have, (3600, 6000));
        assert_eq!(have, vec![(0, 2400), (3600, 6000)]);
        assert_eq!(new_part((2400, 4800), &have), Some((2400, 3600)));
        assert_eq!(missing((2000, 4000), &have), vec![(2400, 3600)]);
        add(&mut have, (2400, 3600));
        assert_eq!(have, vec![(0, 6000)]);
        assert_eq!(new_part((2400, 4800), &have), None);
    }

    #[test]
    fn title_is_the_utc_start_time() {
        assert_eq!(title(0), "1970-01-01 00:00");
        assert_eq!(title(1_763_265_600_000), "2025-11-16 04:00");
    }
}
