//! The commands P1..P11. One `Backend` per app; every command locks the
//! state briefly and never waits on Gemini: crops that need Gemini go to a
//! worker thread whose result is dropped unless it belongs to the session
//! that is still running (its generation).

use crate::api::*;
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
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

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
    /// Range saved as strips.
    covered: Option<(i64, i64)>,
    /// Range already sent to the local model for detection.
    detected: Option<(i64, i64)>,
    placements: Vec<PlacementRow>,
    /// Settled thumbnail of a page, to recognise it when the reader comes back.
    page_thumb: Option<Vec<u8>>,
}

/// A balloon on its way through Gemini, with every place it was seen while waiting.
struct Pending {
    fp: u64,
    aspect: f32,
    /// (picture id, text area in picture coordinates)
    targets: Vec<(i64, Rect)>,
}

type Rect = (i64, i64, u32, u32);

struct Session {
    gen: u64,
    book_id: i64,
    pics: HashMap<i64, Pic>,
    /// Picture on screen; `None` while a page change is under way.
    current: Option<i64>,
    /// Last picture shown, kept for `picture_id` during a page change.
    last_pic: Option<i64>,
    prev: Option<Vec<u8>>,
    screen: (u32, u32),
    fixed: (usize, usize),
    /// Picture y of the screen's top row.
    scroll_y: i64,
    turning: bool,
    mode: Option<&'static str>,
    last_seq: Option<u64>,
    /// Recent frames: seq -> (picture id, scroll_y), to place crops of older frames.
    frames: VecDeque<(u64, i64, i64)>,
    pending: Vec<Pending>,
    revision: u64,
}

impl Session {
    fn scale(&self) -> f64 {
        self.screen.0 as f64 / THUMB_W as f64
    }
    fn pic(&mut self) -> Option<&mut Pic> {
        let id = self.current?;
        self.pics.get_mut(&id)
    }
    fn frame_offset(&self, seq: u64, picture_id: i64) -> i64 {
        self.frames
            .iter()
            .rev()
            .find(|(s, p, _)| *s == seq && *p == picture_id)
            .map(|f| f.2)
            .unwrap_or(self.scroll_y)
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
    platform: Arc<dyn Platform>,
    data_dir: PathBuf,
    gemini_base: String,
    agent: OnceLock<ureq::Agent>,
}

/// `a..b` minus `have`, the part of the screen not handled yet.
fn new_part(vis: (i64, i64), have: Option<(i64, i64)>) -> Option<(i64, i64)> {
    let Some(have) = have else { return Some(vis) };
    if vis.1 <= have.0 || vis.0 >= have.1 {
        return Some(vis);
    }
    if vis.1 > have.1 {
        Some((have.1.max(vis.0), vis.1))
    } else if vis.0 < have.0 {
        Some((vis.0, have.0.min(vis.1)))
    } else {
        None
    }
}

fn hull(a: Option<(i64, i64)>, b: (i64, i64)) -> (i64, i64) {
    a.map(|a| (a.0.min(b.0), a.1.max(b.1))).unwrap_or(b)
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
            platform,
            data_dir,
            gemini_base: gemini_base.unwrap_or_else(|| gemini::DEFAULT_BASE.to_string()),
            agent: OnceLock::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- P1 .. P5

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
        let book_id = store::create_book(&st.db, &title(now), now)?;
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
            screen: (0, 0),
            fixed: (0, 0),
            scroll_y: 0,
            turning: false,
            mode: None,
            last_seq: None,
            frames: VecDeque::new(),
            pending: vec![],
            revision: 0,
        });
        Ok(StartOut { on: true, book_id })
    }

    pub fn stop(&self) -> ApiResult<StopOut> {
        let mut st = self.lock();
        let Some(s) = st.session.take() else { return Err(ApiError("not_on")) };
        // Results of requests still in flight belong to this generation and get dropped.
        st.generation += 1;
        let now = now_ms();
        for id in s.pics.keys() {
            store::close_picture(&st.db, *id, now)?;
        }
        store::end_book(&st.db, s.book_id, now)?;
        let picture_count = store::pictures(&st.db, s.book_id)?.len() as u32;
        drop(st);
        self.platform.stop();
        Ok(StopOut { on: false, book_id: Some(s.book_id), picture_count })
    }

    // ---- P6

    pub fn frame(&self, req: FrameIn) -> ApiResult<FrameOut> {
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
        let rows = thumb.len() / THUMB_W;
        if s.screen != (req.width, req.height) || s.prev.as_ref().is_some_and(|p| p.len() != thumb.len()) {
            s.screen = (req.width, req.height);
            s.prev = None;
            s.fixed = (0, 0);
        }
        let scale = s.scale();
        let now = now_ms();

        if tracker::is_blocked(&thumb) {
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
        let mut clear = false;
        let mut settled = false;
        match s.prev.take() {
            None => {
                if s.current.is_none() && !s.turning {
                    let id = store::create_picture(db, s.book_id, "page", req.width)?;
                    s.pics.insert(id, new_pic(id));
                    s.current = Some(id);
                    s.last_pic = Some(id);
                    s.scroll_y = 0;
                }
            }
            Some(prev) => {
                let (top, bottom) = s.fixed;
                if tracker::still_diff(&prev, &thumb, 0, rows) <= tracker::STILL {
                    settled = true;
                } else {
                    let (ft, fb) = tracker::fixed_rows(&prev, &thumb, rows);
                    let shift = tracker::best_shift(&prev, &thumb, rows, ft, fb)
                        .map(|m| (m, (ft, fb)))
                        .or_else(|| tracker::best_shift(&prev, &thumb, rows, top, bottom).map(|m| (m, (top, bottom))));
                    match shift {
                        Some((rows_moved, fixed)) => {
                            s.fixed = fixed;
                            if s.turning {
                                self.finish_turn(db, s, &prev)?;
                            }
                            motion = "scroll";
                            dy = (rows_moved as f64 * scale).round() as i32;
                            s.scroll_y += dy as i64;
                            s.mode = Some("scroll");
                            if let Some(pic) = s.pic() {
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
                            s.scroll_y = 0;
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
            let (h, ft, fb) = (req.height as i64, (s.fixed.0 as f64 * scale) as i64, (s.fixed.1 as f64 * scale) as i64);
            let scroll_y = s.scroll_y;
            if let Some(pic) = s.pic() {
                let vis = (scroll_y + ft, scroll_y + h - fb);
                if let Some(d) = new_part(vis, pic.detected) {
                    pic.detected = Some(hull(pic.detected, d));
                    detect = Some(Range { top: (d.0 - scroll_y) as u32, bottom: (d.1 - scroll_y) as u32 });
                }
                if let Some(d) = new_part(vis, pic.covered) {
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
            s.frames.push_back((req.seq, id, s.scroll_y));
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
    fn finish_turn(&self, db: &Db, s: &mut Session, thumb: &[u8]) -> ApiResult<()> {
        s.turning = false;
        s.scroll_y = 0;
        let rows = thumb.len() / THUMB_W;
        let seen = s.pics.values().find(|p| {
            p.page_thumb.as_ref().is_some_and(|t| t.len() == thumb.len() && tracker::still_diff(t, thumb, 0, rows) <= tracker::MATCH)
        });
        let id = match seen {
            Some(p) => {
                store::reopen_picture(db, p.id)?;
                p.id
            }
            None => {
                let id = store::create_picture(db, s.book_id, "page", s.screen.0)?;
                s.pics.insert(id, new_pic(id));
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
        let offset = s.frame_offset(req.frame_seq, req.picture_id);
        let mut result = CropsOut { placed: vec![], pending: 0, skipped_effects: 0, discarded: 0 };
        let mut jobs: Vec<Job> = vec![];
        let now = now_ms();
        for c in req.crops {
            if c.kind == "effect" {
                result.skipped_effects += 1;
                continue;
            }
            let Some(img) = export::decode_png(&c.image) else {
                result.discarded += 1;
                continue;
            };
            let fp = fingerprint::dhash(&img.gray(), img.w as usize, img.h as usize);
            let aspect = c.w.max(1) as f32 / c.h.max(1) as f32;
            let rect = (c.x as i64, c.y as i64 + offset, c.w, c.h);
            let s = st.session.as_mut().unwrap();
            let pic = s.pics.get_mut(&req.picture_id).unwrap();
            // Our own overlay is in the capture: a crop over a placement is either
            // the same balloon (already shown) or our Korean text (discarded).
            if let Some(p) = pic.placements.iter().find(|p| overlap(rect, (p.x, p.y, p.w, p.h)) > 0.5) {
                let same = fingerprint::from_hex(&p.fingerprint).is_some_and(|f| (f ^ fp).count_ones() <= fingerprint::MAX_BITS);
                if same {
                    result.placed.push(out(p, s.scroll_y));
                } else {
                    result.discarded += 1;
                }
                continue;
            }
            if let Some(k) = st.find_known(fp, aspect).cloned() {
                store::touch_translation(&st.db, &k.hex, now)?;
                if k.status == "ready" {
                    let ko = k.ko.unwrap_or_default();
                    let row = place(&st.db, req.picture_id, &k.hex, rect, &ko)?;
                    let s = st.session.as_mut().unwrap();
                    result.placed.push(out(&row, s.scroll_y));
                    s.pics.get_mut(&req.picture_id).unwrap().placements.push(row);
                    s.revision += 1;
                }
                continue;
            }
            let s = st.session.as_mut().unwrap();
            result.pending += 1;
            // Already asked (earlier or in this batch): this place gets the same answer.
            if let Some(p) = s.pending.iter_mut().find(|p| fingerprint::same_balloon(fp, aspect, p.fp, p.aspect)) {
                p.targets.push((req.picture_id, rect));
                continue;
            }
            s.pending.push(Pending { fp, aspect, targets: vec![(req.picture_id, rect)] });
            jobs.push(Job { fp, aspect, png: c.image });
        }
        if !jobs.is_empty() {
            let s = st.session.as_mut().unwrap();
            let gen = s.gen;
            let model = st.model.clone();
            drop(guard);
            self.spawn_jobs(gen, key, model, jobs);
        }
        Ok(result)
    }

    fn spawn_jobs(&self, gen: u64, key: String, model: String, jobs: Vec<Job>) {
        let state = self.state.clone();
        let platform = self.platform.clone();
        let base = self.gemini_base.clone();
        let agent = self.agent.get_or_init(gemini::agent).clone();
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
        let offset = s.frame_offset(req.frame_seq, req.picture_id);
        let scale = s.scale();
        // The fixed band does not move with the content: never stored after the first screen.
        let fixed_top = (s.fixed.0 as f64 * scale) as u32;
        let pic = s.pics.get_mut(&req.picture_id).unwrap();
        let mut top = req.top;
        if pic.covered.is_some() {
            top = top.max(fixed_top);
        }
        let mut range = (offset + top as i64, offset + req.bottom as i64);
        if let Some(c) = pic.covered {
            if range.0 >= c.0 && range.1 <= c.1 {
                return Err(ApiError("already_covered"));
            }
            if range.0 < c.1 && range.1 > c.1 {
                range.0 = c.1;
            } else if range.1 > c.0 && range.0 < c.0 {
                range.1 = c.0;
            }
        }
        // The strip must really be screen rows top..bottom, or the export is wrong.
        let img = export::decode_png(&req.image).ok_or(ApiError("bad_image"))?;
        if img.w != s.screen.0 || img.h != req.bottom - req.top {
            return Err(ApiError("bad_image"));
        }
        let (a, b) = ((range.0 - offset) as u32, (range.1 - offset) as u32);
        let bytes = if (a, b) == (req.top, req.bottom) {
            req.image
        } else {
            export::encode_png(&export::rows(&img, a - req.top, b - req.top)).map_err(encode_error)?
        };
        let dir = self.data_dir.join("strips").join(req.picture_id.to_string());
        let name = format!("{}-{}.png", range.0, now_ms());
        let path = dir.join(name);
        std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &bytes)).map_err(storage_error)?;
        let strip_id = store::insert_strip(&st.db, req.picture_id, range.0, (range.1 - range.0) as u32, &path.to_string_lossy(), now_ms())?;
        let s = st.session.as_mut().unwrap();
        let pic = s.pics.get_mut(&req.picture_id).unwrap();
        let covered = hull(pic.covered, range);
        pic.covered = Some(covered);
        store::set_covered(&st.db, req.picture_id, covered.0, covered.1)?;
        Ok(StripOut { strip_id, picture_height: (covered.1 - covered.0) as u32 })
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
                let preview = store::strips(&st.db, p.id)?.into_iter().next().map(|s| s.file_path).unwrap_or_default();
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
                let strips = store::strips(&st.db, p.id)?;
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
            let img = export::render(p.width, p.covered_top, p.covered_bottom, strips, placements).map_err(storage_error)?;
            // A long scroll picture is taller than WebP allows: it goes out in parts.
            let parts = if ext == "png" { vec![img] } else { export::split_rows(img, export::WEBP_MAX) };
            let many = parts.len() > 1;
            for (k, part) in parts.iter().enumerate() {
                let bytes = if ext == "png" { export::encode_png(part) } else { export::encode_webp(part) }.map_err(encode_error)?;
                let stem = if many { format!("translated-{stem}-{}-{}", i + 1, k + 1) } else { format!("translated-{stem}-{}", i + 1) };
                let path = export::free_path(&dir, &stem, ext);
                std::fs::write(&path, bytes).map_err(storage_error)?;
                files.push(path.to_string_lossy().to_string());
            }
        }
        Ok(ExportOut { count: files.len() as u32, files })
    }
}

fn new_pic(id: i64) -> Pic {
    Pic { id, kind: "page", covered: None, detected: None, placements: vec![], page_thumb: None }
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

/// Store a placement of `ko` over the text area `rect` (picture coordinates).
fn place(db: &Db, picture_id: i64, fingerprint: &str, rect: (i64, i64, u32, u32), ko: &str) -> ApiResult<PlacementRow> {
    let shape = layout::fit(rect.0, rect.1, rect.2, rect.3, ko);
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
    Ok(row)
}

type Outcome = Result<(Vec<String>, Option<Result<Vec<String>, ()>>), GeminiError>;

fn finish_jobs(state: &Mutex<State>, platform: &dyn Platform, gen: u64, model: &str, jobs: Vec<Job>, outcome: Outcome) {
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
    let st = &mut *guard;
    // A result for a session that has stopped is dropped without touching state.
    let Some(s) = st.session.as_mut().filter(|s| s.gen == gen) else { return };
    // Every place each balloon was seen while it was being asked.
    let mut targets: Vec<Vec<(i64, Rect)>> = vec![];
    for j in &jobs {
        let i = s.pending.iter().position(|p| p.fp == j.fp && p.aspect == j.aspect);
        targets.push(i.map(|i| s.pending.remove(i).targets).unwrap_or_default());
    }
    let now = now_ms();
    let rows: Vec<(Translation, &Job)> = match outcome {
        Err(GeminiError::Cancelled) => return,
        Err(GeminiError::KeyInvalid) => {
            st.notice = Some("key_invalid");
            return;
        }
        Err(GeminiError::Unreachable) => {
            st.notice = Some("gemini_unreachable");
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
        let Some(ko) = ko else { continue };
        for (picture_id, rect) in targets {
            let s = st.session.as_mut().unwrap();
            let Some(pic) = s.pics.get(&picture_id) else { continue };
            if pic.placements.iter().any(|p| overlap(rect, (p.x, p.y, p.w, p.h)) > 0.5) {
                continue;
            }
            match place(&st.db, picture_id, &t.fingerprint, rect, &ko) {
                Ok(row) => {
                    let s = st.session.as_mut().unwrap();
                    s.pics.get_mut(&picture_id).unwrap().placements.push(row);
                    changed = true;
                }
                Err(e) => eprintln!("cannot place translation: {e}"),
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
        assert_eq!(new_part((300, 2700), Some((0, 2400))), Some((2400, 2700)));
        assert_eq!(new_part((700, 3100), Some((0, 3600))), None);
        assert_eq!(new_part((0, 2400), None), Some((0, 2400)));
        assert_eq!(new_part((-300, 2100), Some((0, 2400))), Some((-300, 0)));
    }

    #[test]
    fn title_is_the_utc_start_time() {
        assert_eq!(title(0), "1970-01-01 00:00");
        assert_eq!(title(1_763_265_600_000), "2025-11-16 04:00");
    }
}
