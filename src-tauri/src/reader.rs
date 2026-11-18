//! P12: the reader. A site opened in the app hands over each page picture
//! (`read_page`); one Gemini request finds its balloons and translates them,
//! and the page is saved with its translation, so the same page is never
//! sent again and shows in the library like any other book. The shapes are
//! in the picture's own pixels: the page draws them on the picture itself,
//! so they move with it however the site scrolls, slides or zooms.

use super::*;
use crate::api::{LangIn, PageBox, PageIn, PageOut};
use crate::gemini::PageItem;
use crate::balloon;
use crate::layout::{self, PAD};
use sha2::{Digest, Sha256};

const LANG_SETTING: &str = "source_lang";
/// How far into a balloon, from its centre, the shape may grow: a balloon is
/// about an ellipse, and a rectangle spanning 0.6 of it keeps its corners
/// well inside the outline (0.6² + 0.6² < 1), spiky outlines included.
const INNER: f64 = 0.6;
/// How far past its text, as a share of the text's own size, the shape may
/// grow: a balloon box Gemini draws too large (taking in the art next to
/// it) must not have the shape cover that art.
const SLACK: f64 = 0.35;
/// The text boxes Gemini draws run a little short of the letters' edges (seen
/// on mangafire pages): each side is pushed out by this share of the box, and
/// by at least `GROW_MIN_PX`, before the shape is made.
const GROW: f64 = 0.06;
const GROW_MIN_PX: f64 = 4.0;
/// How far, in letters, a cover with no balloon around it may reach past
/// the lettering found into Gemini's box: far enough for a trailing "?"
/// the page shows apart (ch256 p21), not across the hair beside a shout
/// Gemini boxed large (ch256 p7, "WH...?!").
const WIDEN: f64 = 1.5;

#[derive(Clone, Copy)]
pub(super) enum ReaderSource<'a> {
    Site(&'a PageIn),
    Chapter(i64),
}

impl Backend {
    /// The language the reader translates from: `ja` unless set to `en`.
    pub fn source_lang(&self) -> String {
        let st = self.lock();
        match store::setting(&st.db, LANG_SETTING).ok().flatten().as_deref() {
            Some(b"en") => "en".into(),
            _ => "ja".into(),
        }
    }

    pub fn put_source_lang(&self, req: LangIn) -> ApiResult<LangIn> {
        if req.lang != "ja" && req.lang != "en" {
            return Err(ApiError("bad_lang"));
        }
        store::put_setting(&self.lock().db, LANG_SETTING, req.lang.as_bytes(), now_ms())?;
        Ok(req)
    }

    /// The Korean shapes of one page picture: saved ones when this picture
    /// was read before in this language, otherwise one Gemini request.
    pub fn read_page(&self, req: PageIn) -> ApiResult<PageOut> {
        if self.lock().key.is_none() {
            return Err(ApiError("key_missing"));
        }
        let bytes = match &req.image {
            Some(b) if !b.is_empty() => b.clone(),
            _ => fetch(&req.src, &req.page_url)?,
        };
        self.read_picture(ReaderSource::Site(&req), &bytes)
    }

    pub(super) fn read_picture(&self, source: ReaderSource<'_>, bytes: &[u8]) -> ApiResult<PageOut> {
        let lang = self.source_lang();
        let mime = export::mime(bytes).ok_or(ApiError("bad_image"))?;
        // The shapes are laid out on the page's own pixels, every time, so a
        // page read again shows the layout of this version of the app.
        let page = export::decode_page(bytes)?;
        let (w, h) = (page.w, page.h);
        let hash: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        let page_key = format!("{hash}:{lang}");
        // Two copies of one picture read at once: the second waits here for
        // the first's answer instead of sending the page again.
        let _turn = self.pages.turn(&page_key);
        let kept = {
            let st = self.lock();
            match store::reader_page(&st.db, &page_key)? {
                Some(_) => Some(store::reader_items(&st.db, &page_key)?),
                None => None,
            }
        };
        if let Some(items) = kept {
            let items: Option<Vec<PageItem>> = items.and_then(|j| serde_json::from_str(&j).ok());
            // Laid out outside the lock: other pages keep reading meanwhile.
            let fresh = items.map(|items| shapes(&items, &page, &lang));
            drop(page);
            let st = self.lock();
            let rows = store::reader_placements(&st.db, &page_key)?;
            let boxes = match fresh {
                Some(boxes) => {
                    let stale = rows.iter().zip(&boxes).any(|(r, b)| page_box(r) != *b);
                    if stale && rows.len() == boxes.len() {
                        // Laid out by an earlier version: the library (and
                        // its export) gets this one. A failure here costs
                        // only the stale export, so the page is still shown.
                        let new: Vec<_> = rows.iter().zip(&boxes).map(|(r, b)| row(r.fingerprint.clone(), b)).collect();
                        {
                            let tx = st.db.unchecked_transaction()?;
                            if let Err(e) = store::replace_reader_placements(&tx, &page_key, &new).and_then(|()| tx.commit()) {
                                eprintln!("reader: shapes not updated: {e}");
                            }
                        }
                    }
                    boxes
                }
                // Kept before its answer was: the shapes saved then.
                None => rows.into_iter().map(|p| page_box(&p)).collect(),
            };
            let save_error = match source {
                ReaderSource::Chapter(pic) => store::set_chapter_translation(&st.db, pic, &page_key).err().map(|e| ApiError::from(e).0),
                ReaderSource::Site(_) => None,
            };
            return Ok(PageOut { width: w, height: h, saved: true, boxes, save_error });
        }

        let model = self.lock().model.clone();
        let items = match self.pages.unsaved(&page_key) {
            // Read before, but not kept: only the saving is tried again.
            Some(items) => items,
            None => {
                let key = self.lock().key.clone().ok_or(ApiError("key_missing"))?;
                let context = self.context(source)?;
                self.ask_page(bytes, mime, &lang, &context, &model, &key)?
            }
        };
        let boxes = shapes(&items, &page, &lang);
        drop(page);

        let mut guard = self.lock();
        let st = &mut *guard;
        let now = now_ms();
        let ext = mime.trim_start_matches("image/");
        let mut written = None;
        let saved = (|| -> ApiResult<()> {
            let tx = st.db.unchecked_transaction()?;
            let (pic, url) = match source {
                ReaderSource::Chapter(pic) => (pic, ""),
                ReaderSource::Site(req) => {
                    let book = match store::reader_book(&tx, &req.page_url)? {
                        Some(b) => b,
                        None => store::create_book(&tx, &title(req), now)?,
                    };
                    let pic = store::create_picture(&tx, book, "page", w)?;
                    store::set_picture_seq(&tx, pic, req.index)?;
                    store::set_covered(&tx, pic, 0, h as i64)?;
                    store::close_picture(&tx, pic, now)?;
                    let rel_dir = format!("strips/{pic}");
                    std::fs::create_dir_all(self.data_dir.join(&rel_dir)).map_err(storage_error)?;
                    // A picture id freed by a failed save can be reused.
                    let rel = format!("{rel_dir}/page-{}-{lang}.{ext}", &hash[..16]);
                    written = Some(self.data_dir.join(&rel));
                    std::fs::write(self.data_dir.join(&rel), bytes).map_err(storage_error)?;
                    store::insert_strip(&tx, pic, 0, h, &rel, now)?;
                    store::end_book(&tx, book, now)?;
                    (pic, req.page_url.as_str())
                }
            };
            for (i, (it, b)) in items.iter().zip(&boxes).enumerate() {
                // Not a balloon fingerprint (no hex): screen capture never matches it.
                let fingerprint = format!("page:{page_key}:{i}");
                let t = store::Translation { fingerprint: fingerprint.clone(), aspect: b.w as f32 / b.h.max(1) as f32, source_ja: Some(it.src.clone()), translation_ko: Some(it.ko.clone()), status: "ready".into() };
                store::put_translation(&tx, &t, &model, now)?;
                store::insert_placement(&tx, pic, &row(fingerprint, b))?;
            }
            let answer = serde_json::to_string(&items).map_err(|_| ApiError("internal"))?;
            store::insert_reader_page(&tx, &page_key, pic, url, &answer)?;
            if let ReaderSource::Chapter(pic) = source {
                store::set_chapter_translation(&tx, pic, &page_key)?;
            }
            tx.commit()?;
            Ok(())
        })();
        // Cleaned up before the lock goes: the rolled-back picture id is free
        // again, and the next page to save may take it (and its folder).
        let save_error = match saved {
            Ok(()) => {
                self.pages.forget(&page_key);
                None
            }
            Err(e) => {
                // Shown anyway, and told: the answer waits in memory for the
                // same call to save it again.
                if let Some(p) = written {
                    let _ = std::fs::remove_file(p);
                }
                eprintln!("reader: page not saved: {}", e.0);
                self.pages.keep(&page_key, items);
                Some(e.0)
            }
        };
        self.let_go(guard);
        Ok(PageOut { width: w, height: h, saved: false, boxes, save_error })
    }

    /// Gives up the lock after a page save. Under test, it checks what the
    /// next save to take the lock would find on disk.
    fn let_go(&self, guard: MutexGuard<'_, State>) {
        drop(guard);
        #[cfg(test)]
        tests::after_let_go(self);
    }

    /// The lines of the pages kept just before this one in its book, oldest
    /// first, so the next request keeps names and voices as they were shown.
    fn context(&self, source: ReaderSource<'_>) -> ApiResult<Vec<PageItem>> {
        let st = self.lock();
        let place = match source {
            ReaderSource::Site(req) => store::reader_book(&st.db, &req.page_url)?.map(|b| (b, req.index)),
            ReaderSource::Chapter(pic) => store::picture_place(&st.db, pic)?,
        };
        let Some((book, seq)) = place else { return Ok(vec![]) };
        let pages = store::earlier_items(&st.db, book, seq, 3)?;
        Ok(pages.iter().filter_map(|j| serde_json::from_str::<Vec<PageItem>>(j).ok()).flatten().collect())
    }

    /// One Gemini request for the whole page (R9, R10 in `gemini::read_page`).
    fn ask_page(&self, bytes: &[u8], mime: &str, lang: &str, context: &[PageItem], model: &str, key: &str) -> ApiResult<Vec<PageItem>> {
        let agent = gemini::agent();
        let client = gemini::Client { agent: &agent, base: &self.gemini_base, model, key };
        let may_send = || {
            self.lock().gemini_requests += 1;
            true
        };
        match client.read_page(bytes, mime, lang, context, &may_send) {
            Ok(items) => {
                let mut st = self.lock();
                if matches!(st.notice, Some("key_invalid" | "gemini_unreachable")) {
                    st.notice = None;
                }
                Ok(items)
            }
            Err(e) => {
                let code = match e {
                    GeminiError::KeyInvalid => "key_invalid",
                    GeminiError::Unreadable => "gemini_unreadable",
                    GeminiError::Unreachable | GeminiError::Cancelled => "gemini_unreachable",
                };
                if code != "gemini_unreadable" {
                    self.lock().notice = Some(code);
                }
                Err(ApiError(code))
            }
        }
    }

}

/// P12's pages in flight: one read per page key at a time, and the answers
/// whose saving failed, kept until saving them works.
#[derive(Default)]
pub(crate) struct Pages {
    state: Mutex<PagesState>,
    done: std::sync::Condvar,
}

#[derive(Default)]
struct PagesState {
    busy: std::collections::HashSet<String>,
    // ponytail: unbounded, but only pages whose saving failed (a full or
    // locked disk) land here, each a few hundred bytes of text.
    unsaved: HashMap<String, Vec<PageItem>>,
}

/// The right to read one page key; giving it up wakes the copies waiting.
pub(crate) struct Turn<'a> {
    pages: &'a Pages,
    key: String,
}

impl Pages {
    fn lock(&self) -> MutexGuard<'_, PagesState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn turn(&self, key: &str) -> Turn<'_> {
        let mut st = self.lock();
        while st.busy.contains(key) {
            st = self.done.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        st.busy.insert(key.to_string());
        Turn { pages: self, key: key.to_string() }
    }

    fn unsaved(&self, key: &str) -> Option<Vec<PageItem>> {
        self.lock().unsaved.get(key).cloned()
    }

    fn keep(&self, key: &str, items: Vec<PageItem>) {
        self.lock().unsaved.insert(key.to_string(), items);
    }

    fn forget(&self, key: &str) {
        self.lock().unsaved.remove(key);
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.pages.lock().busy.remove(&self.key);
        self.pages.done.notify_all();
    }
}

/// The book's name: the site page's title, or its address.
fn title(req: &PageIn) -> String {
    let t = req.title.trim();
    let t = if t.is_empty() { req.page_url.as_str() } else { t };
    t.chars().take(120).collect()
}

/// The page picture from the site itself, sent as from the site page.
fn fetch(src: &str, page_url: &str) -> ApiResult<Vec<u8>> {
    if !(src.starts_with("https://") || src.starts_with("http://")) {
        return Err(ApiError("image_unreachable"));
    }
    // The site's address only, as a browser sends it to another host.
    let origin: String = page_url.split('/').take(3).collect::<Vec<_>>().join("/") + "/";
    let mut resp = gemini::agent()
        .get(src)
        .header("referer", &origin)
        .header("user-agent", "Mozilla/5.0 (Linux; Android 15) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36")
        .call()
        .map_err(|_| ApiError("image_unreachable"))?;
    if resp.status().as_u16() != 200 {
        return Err(ApiError("image_unreachable"));
    }
    resp.body_mut().with_config().limit(MAX_PAGE_BYTES).read_to_vec().map_err(|_| ApiError("image_unreachable"))
}

/// Picture pixels `[x0, y0, x1, y1]` of a fractional `[top, left, bottom, right]` box.
fn px(b: [f64; 4], w: u32, h: u32) -> [f64; 4] {
    [b[1] * w as f64, b[0] * h as f64, b[3] * w as f64, b[2] * h as f64]
}

/// The white shapes of a page's balloons. Each follows its balloon's printed
/// outline where the page shows one (`balloon::find`); otherwise it is the
/// rounded rectangle of [`shape`]. Either way the Korean is sized from the
/// original lettering under it.
pub(crate) fn shapes(items: &[PageItem], page: &export::Rgb, lang: &str) -> Vec<PageBox> {
    let (w, h) = (page.w, page.h);
    let gray = page.gray();
    let pg = balloon::Page { w, h, px: &gray };
    let vertical = lang == "ja";
    // Gemini's boxes say roughly where; the lettering on the page says exactly.
    let blocks = balloon::blocks(&pg, vertical);
    let letters = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).count();
    let wanted: Vec<([f64; 4], usize)> = items.iter().map(|it| (px(it.text, w, h), letters(&it.src))).collect();
    let on = balloon::assign(&wanted, &blocks, w);
    let pieces = balloon::gather(&wanted, &blocks, &on);
    let union = |p: &[[f64; 4]]| p.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]);
    let texts: Vec<[f64; 4]> = pieces.iter().zip(&wanted).map(|(p, &(t, _))| if p.is_empty() { t } else { union(p) }).collect();
    items
        .iter()
        .enumerate()
        .map(|(i, it)| {
            let others: Vec<[f64; 4]> = texts.iter().enumerate().filter(|&(j, _)| j != i).map(|(_, &t)| t).collect();
            // Each piece of lettering gathered from beside the item's own,
            // under a rectangle of its own, cut back from other items'.
            let beside = |cover: [f64; 4]| -> Vec<Vec<[i64; 2]>> {
                pieces[i]
                    .iter()
                    .skip(1)
                    .filter(|p| !(p[0] >= cover[0] && p[1] >= cover[1] && p[2] <= cover[2] && p[3] <= cover[3]))
                    .map(|p| {
                        let (mx, my) = (((p[2] - p[0]) * GROW).max(GROW_MIN_PX), ((p[3] - p[1]) * GROW).max(GROW_MIN_PX));
                        let r = apart([p[0] - mx, p[1] - my, p[2] + mx, p[3] + my], *p, &others);
                        rect_outline([r[0] as i64, r[1] as i64, r[2].ceil() as i64, r[3].ceil() as i64])
                    })
                    .collect()
            };
            let around = |t: [f64; 4], glyph: f64| balloon::find(&pg, t, glyph, &others).map(|f| balloon::Found { glyph: Some(glyph), ..f });
            let found = on[i].and_then(|j| {
                let (own, glyph) = (blocks[j].bounds, blocks[j].glyph);
                if texts[i] == own {
                    return around(own, glyph);
                }
                // Lettering gathered from beside its block goes under one
                // cover only if one balloon holds it all; else the block's
                // own balloon is covered, and the art beside it is not.
                let t = texts[i];
                around(t, glyph)
                    .filter(|f| {
                        let b = f.bounds;
                        b[0] as f64 <= t[0] && b[1] as f64 <= t[1] && b[2] as f64 >= t[2] && b[3] as f64 >= t[3]
                    })
                    .or_else(|| around(own, glyph))
            });
            match found {
                Some(f) => {
                    let [x0, y0, x1, y1] = f.text;
                    let (tw, th) = ((x1 - x0) as u32, (y1 - y0) as u32);
                    let b = f.bounds;
                    let (outline, b) = joined(std::iter::once(f.outline).chain(beside(b.map(|v| v as f64))).collect());
                    PageBox {
                        x: b[0],
                        y: b[1],
                        w: (b[2] - b[0]) as u32,
                        h: (b[3] - b[1]) as u32,
                        radius: 0,
                        font_px: layout::font_for(&it.ko, tw, th, f.glyph),
                        ko: it.ko.clone(),
                        outline,
                        text: [x0, y0, tw as i64, th as i64],
                    }
                }
                // No closed balloon: a rounded rectangle over the lettering
                // found, or over Gemini's box when none was.
                // No lettering found for it, and none in Gemini's box either
                // (a shout fused with the panel frame, the box on the hair
                // below it): a white box there would wipe the art and leave
                // the lettering. The line is kept, with nothing drawn.
                None if on[i].is_none() && balloon::marks_in(&pg, wanted[i].0) < (wanted[i].1 / 2).max(1) => {
                    let t = wanted[i].0;
                    let (x, y) = (t[0].max(0.0) as i64, t[1].max(0.0) as i64);
                    PageBox { x, y, w: 0, h: 0, radius: 0, font_px: 0, ko: it.ko.clone(), outline: vec![], text: [x, y, 0, 0] }
                }
                None => {
                    // The lettering found, and as much of Gemini's box as lies
                    // within a letter or two of it: lettering the page shows
                    // faintly (white letters on a gray balloon, outlined on
                    // foliage) or in pieces is still covered, and a box
                    // Gemini put off to one side is not.
                    let t = match on[i] {
                        Some(j) => {
                            let (t, g, r) = (blocks[j].bounds, WIDEN * blocks[j].glyph, wanted[i].0);
                            let clip = [r[0].max(t[0] - g), r[1].max(t[1] - g), r[2].min(t[2] + g), r[3].min(t[3] + g)];
                            if clip[0] < clip[2] && clip[1] < clip[3] {
                                [t[0].min(clip[0]), t[1].min(clip[1]), t[2].max(clip[2]), t[3].max(clip[3])]
                            } else {
                                t
                            }
                        }
                        None => texts[i],
                    };
                    let glyph = match on[i] {
                        Some(j) => Some(blocks[j].glyph),
                        None => {
                            let strokes: Vec<(i64, i64)> = (t[1].max(0.0) as i64..(t[3] as i64).min(h as i64))
                                .flat_map(|y| (t[0].max(0.0) as i64..(t[2] as i64).min(w as i64)).map(move |x| (x, y)))
                                .filter(|&(x, y)| gray[y as usize * w as usize + x as usize] < 128)
                                .collect();
                            balloon::glyph(&strokes, vertical)
                        }
                    };
                    let placed = match on[i] {
                        Some(_) => PageItem { text: [t[1] / h as f64, t[0] / w as f64, t[3] / h as f64, t[2] / w as f64], balloon: None, ..it.clone() },
                        None => it.clone(),
                    };
                    let mut s = shape(&placed, w, h);
                    let r = apart([s.x as f64, s.y as f64, (s.x + s.w as i64) as f64, (s.y + s.h as i64) as f64], t, &others);
                    (s.x, s.y, s.w, s.h) = (r[0] as i64, r[1] as i64, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32);
                    s.radius = s.radius.min((s.w.min(s.h) / 4) as u16);
                    let pad = PAD as i64;
                    s.text = [s.x + pad, s.y + pad, (s.w as i64 - 2 * pad).max(1), (s.h as i64 - 2 * pad).max(1)];
                    s.font_px = layout::font_for(&it.ko, s.text[2] as u32, s.text[3] as u32, glyph);
                    let own = [s.x, s.y, s.x + s.w as i64, s.y + s.h as i64];
                    let more = beside(own.map(|v| v as f64));
                    if !more.is_empty() {
                        let (outline, b) = joined(std::iter::once(rect_outline(own)).chain(more).collect());
                        (s.outline, s.radius) = (outline, 0);
                        (s.x, s.y, s.w, s.h) = (b[0], b[1], (b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
                    }
                    s
                }
            }
        })
        .collect()
}

/// A rectangle cover `c` over lettering `own`, cut back from the other
/// items' lettering it reaches into, on whichever side gives up least: two
/// captions side by side each keep their own and neither white box hides
/// the other's Korean.
fn apart(mut c: [f64; 4], own: [f64; 4], others: &[[f64; 4]]) -> [f64; 4] {
    for o in others {
        if c[0] >= o[2] || o[0] >= c[2] || c[1] >= o[3] || o[1] >= c[3] {
            continue;
        }
        let cuts = [
            (o[0] >= own[2]).then(|| [c[0], c[1], o[0], c[3]]),
            (o[2] <= own[0]).then(|| [o[2], c[1], c[2], c[3]]),
            (o[1] >= own[3]).then(|| [c[0], c[1], c[2], o[1]]),
            (o[3] <= own[1]).then(|| [c[0], o[3], c[2], c[3]]),
        ];
        let area = |r: &[f64; 4]| (r[2] - r[0]) * (r[3] - r[1]);
        if let Some(best) = cuts.into_iter().flatten().max_by(|a, b| area(a).total_cmp(&area(b))) {
            c = best;
        }
    }
    c
}

/// The outline of rectangle `r` (x0, y0, x1, y1), clockwise from its top left
/// as [`balloon::find`]'s outlines run.
fn rect_outline(r: [i64; 4]) -> Vec<[i64; 2]> {
    vec![[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
}

/// One outline of several parts, and its bounds: each part after the first
/// is reached from the first one's first corner along a bridge of no width,
/// walked there and back, so the cover is just its parts (filled nonzero, as
/// `clip-path` and the export fill it).
fn joined(parts: Vec<Vec<[i64; 2]>>) -> (Vec<[i64; 2]>, [i64; 4]) {
    let mut out: Vec<[i64; 2]> = vec![];
    for p in parts.iter().filter(|p| !p.is_empty()) {
        if let Some(&start) = out.first() {
            out.push(start);
            out.extend(p.iter().copied());
            out.push(p[0]);
        } else {
            out.extend(p.iter().copied());
        }
    }
    let b = out.iter().fold([i64::MAX, i64::MAX, i64::MIN, i64::MIN], |b, &[x, y]| [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)]);
    (out, b)
}

/// The library row of a shape.
fn row(fingerprint: String, b: &PageBox) -> store::PlacementRow {
    let cover = layout::Cover { outline: b.outline.clone(), text: b.text };
    let cover = serde_json::to_string(&cover).ok();
    store::PlacementRow { id: 0, fingerprint, x: b.x, y: b.y, w: b.w, h: b.h, radius: b.radius, font_px: b.font_px, ko: b.ko.clone(), cover }
}

/// The shape of a library row: a rounded rectangle unless its cover says otherwise.
fn page_box(p: &store::PlacementRow) -> PageBox {
    let pad = PAD as i64;
    let cover: Option<layout::Cover> = p.cover.as_deref().and_then(|c| serde_json::from_str(c).ok());
    let (outline, text) = match cover {
        Some(c) => (c.outline, c.text),
        None => (vec![], [p.x + pad, p.y + pad, (p.w as i64 - 2 * pad).max(1), (p.h as i64 - 2 * pad).max(1)]),
    };
    PageBox { x: p.x, y: p.y, w: p.w, h: p.h, radius: p.radius, font_px: p.font_px, ko: p.ko.clone(), outline, text }
}

/// The white shape for one balloon: its text, grown towards the middle of
/// the balloon so the source text is hidden even where the box Gemini gave
/// is a little short, and never past the balloon's outline. Used where the
/// page shows no closed balloon around the text.
pub(crate) fn shape(it: &PageItem, w: u32, h: u32) -> PageBox {
    let t = px(it.text, w, h);
    let (mx, my) = (((t[2] - t[0]) * GROW).max(GROW_MIN_PX), ((t[3] - t[1]) * GROW).max(GROW_MIN_PX));
    let g = [t[0] - mx, t[1] - my, t[2] + mx, t[3] + my];
    let (cover, area) = match it.balloon.map(|b| px(b, w, h)) {
        Some(b) => {
            let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
            let (hw, hh) = ((b[2] - b[0]) / 2.0 * INNER, (b[3] - b[1]) / 2.0 * INNER);
            // The balloon, widened to the text should Gemini have cut it short.
            let area = [b[0].min(t[0]), b[1].min(t[1]), b[2].max(t[2]), b[3].max(t[3])];
            let (sx, sy) = ((t[2] - t[0]) * SLACK, (t[3] - t[1]) * SLACK);
            let cover = [
                g[0].min((cx - hw).max(t[0] - sx)),
                g[1].min((cy - hh).max(t[1] - sy)),
                g[2].max((cx + hw).min(t[2] + sx)),
                g[3].max((cy + hh).min(t[3] + sy)),
            ];
            (cover, area)
        }
        None => (g, [0.0, 0.0, w as f64, h as f64]),
    };
    let pad = PAD as f64;
    let (x0, y0) = (cover[0].round() as i64 + PAD as i64, cover[1].round() as i64 + PAD as i64);
    let (cw, ch) = (((cover[2] - cover[0]) - 2.0 * pad).max(1.0) as u32, ((cover[3] - cover[1]) - 2.0 * pad).max(1.0) as u32);
    let area = (area[0].max(0.0) as i64, area[1].max(0.0) as i64, (area[2].min(w as f64)) as i64, (area[3].min(h as f64)) as i64);
    let s = layout::fit_within(x0, y0, cw, ch, &it.ko, area);
    PageBox { x: s.x, y: s.y, w: s.w, h: s.h, radius: s.radius, font_px: s.font_px, ko: it.ko.clone(), outline: vec![], text: [s.x, s.y, s.w as i64, s.h as i64] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: [f64; 4], balloon: Option<[f64; 4]>) -> PageItem {
        PageItem { text, balloon, speaker: String::new(), src: String::new(), ko: "안녕".into() }
    }

    struct Quiet(PathBuf);

    impl Platform for Quiet {
        fn request_permissions(&self) -> crate::platform::Permissions {
            self.permissions()
        }
        fn permissions(&self) -> crate::platform::Permissions {
            crate::platform::Permissions { capture: false, overlay: false }
        }
        fn start(&self) -> Result<(), String> {
            Ok(())
        }
        fn stop(&self) {}
        fn emit(&self, _: &str, _: serde_json::Value) {}
        fn export_dir(&self) -> PathBuf {
            self.0.clone()
        }
        fn seal(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.to_vec())
        }
        fn unseal(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.to_vec())
        }
    }

    /// What the save after a page save finds when it takes the lock: every
    /// page image no kept strip names, by data folder (from `after_let_go`).
    static STRAYS: Mutex<Vec<String>> = Mutex::new(vec![]);

    pub(super) fn after_let_go(b: &Backend) {
        let st = b.lock();
        let mut q = st.db.prepare("SELECT file_path FROM Strip").unwrap();
        let kept: std::collections::HashSet<String> = q.query_map([], |r| r.get(0)).unwrap().map(|p| p.unwrap()).collect();
        for d in std::fs::read_dir(b.data_dir.join("strips")).into_iter().flatten() {
            let d = d.unwrap();
            for f in std::fs::read_dir(d.path()).unwrap() {
                let rel = format!("strips/{}/{}", d.file_name().to_string_lossy(), f.unwrap().file_name().to_string_lossy());
                if !kept.contains(&rel) {
                    STRAYS.lock().unwrap().push(format!("{}|{rel}", b.data_dir.display()));
                }
            }
        }
    }

    /// The next save to take the lock finds on disk exactly the page images
    /// the library keeps: a failed save removes its image before letting go,
    /// so the page that takes its freed picture id never meets that file.
    #[test]
    fn the_next_save_to_take_the_lock_finds_only_kept_page_images() {
        let dir = std::env::temp_dir().join(format!("rst-reader-{}-{}", std::process::id(), now_ms()));
        let b = Backend::open(dir.clone(), Some("http://127.0.0.1:9".into()), Arc::new(Quiet(dir.join("export")))).unwrap();
        b.lock().key = Some("k".into());
        // A save for a site page under `outside/` fails after its image is written.
        b.lock().db.execute_batch("CREATE TRIGGER refuse BEFORE INSERT ON ReaderPage WHEN NEW.page_url LIKE '%/outside/%' BEGIN SELECT RAISE(ABORT, 'refused'); END;").unwrap();
        let fixture = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page.jpg")).unwrap();
        let lang = b.source_lang();
        // Failing and kept pages in turn: each kept page takes the picture id
        // the failed one before it gave back.
        for n in 0..6u8 {
            let mut page = fixture.clone();
            page.extend([0, n]); // past the picture's end: another content key
            let url = if n % 2 == 0 { "https://m.example/outside/target.txt" } else { "https://m.example/c/kept" };
            // Read before and not kept: no Gemini is needed here.
            let hash: String = Sha256::digest(&page).iter().map(|b| format!("{b:02x}")).collect();
            b.pages.keep(&format!("{hash}:{lang}"), vec![item([0.2, 0.2, 0.3, 0.3], None)]);
            let req = PageIn { src: "https://cdn.example/p.jpg".into(), image: Some(page), page_url: url.into(), title: "t".into(), index: 0 };
            assert_eq!(b.read_page(req).unwrap().save_error.is_some(), n % 2 == 0, "{url}");
        }
        let ids: Vec<i64> = b.lock().db.prepare("SELECT picture_id FROM ReaderPage ORDER BY picture_id").unwrap().query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(ids, [1, 2, 3], "each kept page took the id a failed save freed");
        let mine = format!("{}|", dir.display());
        let strays: Vec<String> = STRAYS.lock().unwrap().iter().filter(|s| s.starts_with(&mine)).cloned().collect();
        assert!(strays.is_empty(), "the next save found images no page keeps: {strays:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_shape_covers_its_text_and_stays_in_its_balloon() {
        // A 1000 x 2000 page; text 200..300 wide, 400..600 tall in a balloon 100..400, 300..700.
        let s = shape(&item([0.2, 0.2, 0.3, 0.3], Some([0.15, 0.1, 0.35, 0.4])), 1000, 2000);
        assert!(s.x <= 200 && s.y <= 400 && s.x + s.w as i64 >= 300 && s.y + s.h as i64 >= 600, "{s:?}");
        assert!(s.x >= 100 && s.y >= 300 && s.x + s.w as i64 <= 400 && s.y + s.h as i64 <= 700, "{s:?}");
    }

    #[test]
    fn a_balloon_box_too_large_does_not_spread_the_shape_over_the_art() {
        // Text 200..300 x 400..500 in a "balloon" Gemini drew over half the page.
        let s = shape(&item([0.2, 0.2, 0.25, 0.3], Some([0.0, 0.0, 0.5, 1.0])), 1000, 2000);
        assert!(s.x <= 200 && s.y <= 400 && s.x + s.w as i64 >= 300 && s.y + s.h as i64 >= 500, "{s:?}");
        assert!(s.x >= 200 - 35 - 1 && s.x + s.w as i64 <= 300 + 35 + 1 && s.y >= 400 - 35 - 1 && s.y + s.h as i64 <= 500 + 35 + 1, "{s:?}");
    }

    /// ch256 p6: two captions lettered side by side on the art, each under
    /// a rectangle; the first one's reached into the second's lettering and
    /// the second's white box hid the end of the first's Korean.
    #[test]
    fn a_rectangle_cover_stops_short_of_the_next_caption() {
        let (own, next) = ([620.0, 95.0, 840.0, 215.0],[845.0, 20.0, 1130.0, 175.0]);
        let c = apart([610.0, 85.0, 860.0, 225.0], own, &[next]);
        assert_eq!(c, [610.0, 85.0, 845.0, 225.0]);
        // Lettering it does not reach leaves it as it was.
        assert_eq!(apart(c, own, &[[0.0, 600.0, 100.0, 700.0]]), c);
    }

    /// ch257 p8: "WHAT DO I..." and "OH, I KNOW!" are one item lettered on
    /// either side of Rentarou's face; one rectangle over both wiped the
    /// face. Each is covered, and the art between them is not.
    #[test]
    fn lettering_in_two_places_is_covered_in_two_and_the_art_between_kept() {
        let parts = vec![rect_outline([10, 10, 40, 40]), rect_outline([30, 30, 50, 50]), rect_outline([70, 20, 90, 60])];
        let (outline, b) = joined(parts);
        assert_eq!(b, [10, 10, 90, 60]);
        let shape = PageBox { x: b[0], y: b[1], w: 80, h: 50, radius: 0, font_px: 10, ko: String::new(), outline, text: [12, 12, 26, 26] };
        let mut img = export::Rgb { w: 100, h: 100, px: vec![0; 100 * 100 * 3] };
        export::draw_placement(&mut img, &row(String::new(), &shape), 0);
        let white = |x: usize, y: usize| img.px[(y * 100 + x) * 3] == 255;
        // Both parts, and where two overlap.
        assert!(white(20, 20) && white(45, 45) && white(35, 35) && white(80, 50));
        // The face between them, and along the bridge.
        assert!(!white(60, 30) && !white(20, 55) && !white(55, 15));
    }

    #[test]
    fn a_text_box_without_its_balloon_is_covered_a_little_past_its_edges() {
        // Text 200..300 x 400..600: 6 px more each side across, 12 px down.
        let s = shape(&item([0.2, 0.2, 0.3, 0.3], None), 1000, 2000);
        assert_eq!((s.x, s.y, s.w, s.h), (194, 388, 112, 224));
    }
}
