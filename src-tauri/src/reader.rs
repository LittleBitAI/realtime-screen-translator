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
/// lettering found whole into Gemini's box: far enough for a trailing "?"
/// the page shows apart (ch256 p21), not across the hair beside a shout
/// Gemini boxed large (ch256 p7, "WH...?!"). Lettering found short of its
/// source text's letters reaches [`balloon::REACH`] letters, over the lines
/// the page shows too faintly to find (ch257 p14, outlined "PRETTY!").
const WIDEN: f64 = 1.5;
/// How much larger than the original letters Korean set over the art may
/// be: its white rim hides what the original lettering's area still shows.
const OVER_ART: f64 = 1.5;
/// How far apart, in letters, two letters of one line lettered on the art
/// may lie: a word space and the outline round each.
const LINE_RUN: f64 = 1.0;
/// The share of a balloon's width and height the Korean set along its
/// outline is sized for at first (an oval's text runs about this far).
const FLOW: f64 = 0.85;
/// A letter's size, as a share of the page's width, where no lettering was
/// measured: what a rim round unmeasured lettering is sized from.
const GLYPH_GUESS: f64 = 0.02;

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
        self.read_picture(ReaderSource::Site(&req), || match &req.image {
            Some(b) if !b.is_empty() => Ok(b.clone()),
            _ => fetch(&req.src, &req.page_url),
        })
    }

    /// One page, its picture's bytes loaded by `load`.
    pub(super) fn read_picture(&self, source: ReaderSource<'_>, load: impl FnOnce() -> ApiResult<Vec<u8>>) -> ApiResult<PageOut> {
        // In its book's order before its picture is loaded or decoded, so a
        // later page whose picture is ready sooner still waits for it.
        let place = self.place(source)?;
        let _reading = self.pages.reading(&place);
        let bytes = &load()?[..];
        let lang = self.source_lang();
        let mime = export::mime(bytes).ok_or(ApiError("bad_image"))?;
        // The shapes are laid out on the page's own pixels, every time, so a
        // page read again shows the layout of this version of the app.
        let page = export::decode_page(bytes)?;
        let (w, h) = (page.w, page.h);
        let hash: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        let page_key = format!("{hash}:{lang}");
        // A page to be sent waits for the earlier pages of its book still
        // being read, so its request carries their lines; a page already
        // kept does not wait. Waited for before the turn: a copy of an
        // earlier page later in the book never holds the turn it waits on.
        if store::reader_page(&self.lock().db, &page_key)?.is_none() && self.pages.unsaved(&page_key).is_none() {
            self.pages.after_earlier(&place);
        }
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
            // Saved before its answer was kept (an older library): the answer
            // is made again from what was kept with each line, its source,
            // its Korean and the box drawn round Gemini's, so the page gets
            // this version's layout too, with no request.
            let (items, rebuilt) = match items.and_then(|j| serde_json::from_str::<Vec<PageItem>>(&j).ok()) {
                Some(items) => (items, false),
                None => {
                    let lines = store::reader_lines(&self.lock().db, &page_key)?;
                    (lines.iter().map(|(p, src)| kept_item(p, src, w, h)).collect(), true)
                }
            };
            // Laid out outside the lock: other pages keep reading meanwhile.
            let (boxes, lettering_px) = laid_out(&items, &page, &lang);
            drop(page);
            let st = self.lock();
            let rows = store::reader_placements(&st.db, &page_key)?;
            let stale = rows.iter().zip(&boxes).any(|(r, b)| page_box(r) != *b);
            if (stale || rebuilt) && rows.len() == boxes.len() {
                // Laid out by an earlier version: the library (and its
                // export) gets this one. A failure here costs only the stale
                // export, so the page is still shown.
                let new: Vec<_> = rows.iter().zip(&boxes).map(|(r, b)| row(r.fingerprint.clone(), b)).collect();
                let tx = st.db.unchecked_transaction()?;
                let answer = serde_json::to_string(&items).map_err(|_| ApiError("internal"))?;
                let kept = store::replace_reader_placements(&tx, &page_key, &new)
                    .and_then(|()| if rebuilt { store::set_reader_items(&tx, &page_key, &answer) } else { Ok(()) })
                    .and_then(|()| tx.commit());
                if let Err(e) = kept {
                    eprintln!("reader: shapes not updated: {e}");
                }
            }
            let save_error = match source {
                ReaderSource::Chapter(pic) => store::set_chapter_translation(&st.db, pic, &page_key),
                ReaderSource::Site(req) => store::set_site_page(&st.db, &req.page_url, req.index, &page_key),
            }
            .err()
            .map(|e| ApiError::from(e).0);
            return Ok(PageOut { width: w, height: h, saved: true, boxes, lettering_px, save_error });
        }

        let model = self.lock().model.clone();
        let items = match self.pages.unsaved(&page_key) {
            // Read before, but not kept: only the saving is tried again.
            Some(items) => items,
            None => {
                let key = self.lock().key.clone().ok_or(ApiError("key_missing"))?;
                let context = self.context(source, &lang)?;
                self.ask_page(bytes, mime, &lang, &context, &model, &key)?
            }
        };
        let (boxes, lettering_px) = laid_out(&items, &page, &lang);
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
            match source {
                ReaderSource::Chapter(pic) => store::set_chapter_translation(&tx, pic, &page_key)?,
                ReaderSource::Site(req) => store::set_site_page(&tx, &req.page_url, req.index, &page_key)?,
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
        Ok(PageOut { width: w, height: h, saved: false, boxes, lettering_px, save_error })
    }

    /// Gives up the lock after a page save. Under test, it checks what the
    /// next save to take the lock would find on disk.
    fn let_go(&self, guard: MutexGuard<'_, State>) {
        drop(guard);
        #[cfg(test)]
        tests::after_let_go(self);
    }

    /// Where a page sits in its book: the book (a site page's address, or an
    /// imported chapter's book) and the page's place in it.
    fn place(&self, source: ReaderSource<'_>) -> ApiResult<Option<(String, i64)>> {
        Ok(match source {
            ReaderSource::Site(req) => Some((format!("site:{}", req.page_url), req.index)),
            ReaderSource::Chapter(pic) => store::picture_place(&self.lock().db, pic)?.map(|(book, seq)| (format!("book:{book}"), seq)),
        })
    }

    /// The lines of the pages kept before this one in its book (a chapter's
    /// worth), oldest first, so the next request keeps names and voices as
    /// they were shown: their answers in the source language `lang`, the
    /// page's own, whichever language a page was last shown in.
    fn context(&self, source: ReaderSource<'_>, lang: &str) -> ApiResult<Vec<PageItem>> {
        let st = self.lock();
        let place = match source {
            ReaderSource::Site(req) => store::reader_book(&st.db, &req.page_url)?.map(|b| (b, req.index)),
            ReaderSource::Chapter(pic) => store::picture_place(&st.db, pic)?,
        };
        let mut pages: std::collections::BTreeMap<i64, (String, Option<String>)> = match place {
            Some((book, seq)) => store::earlier_items(&st.db, book, seq, CONTEXT_PAGES, lang)?.into_iter().map(|(s, k, j)| (s, (k, j))).collect(),
            None => Default::default(),
        };
        // A site chapter's pages answered from the library under another
        // address count too, at the place this chapter showed them.
        if let ReaderSource::Site(req) = source {
            pages.extend(store::earlier_site_items(&st.db, &req.page_url, req.index, CONTEXT_PAGES, lang)?.into_iter().map(|(s, k, j)| (s, (k, j))));
        }
        let skip = pages.len().saturating_sub(CONTEXT_PAGES as usize);
        let mut lines = vec![];
        for (key, answer) in pages.values().skip(skip) {
            match answer.as_deref().and_then(|j| serde_json::from_str::<Vec<PageItem>>(j).ok()) {
                Some(items) => lines.extend(items),
                // Saved by an older library without its answer: the source
                // and Korean it kept with each line (the box goes unused here).
                None => lines.extend(store::reader_lines(&st.db, key)?.iter().map(|(p, src)| kept_item(p, src, 1, 1))),
            }
        }
        Ok(lines)
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

/// Earlier pages read for a request's context: more than a chapter holds.
const CONTEXT_PAGES: u32 = 30;

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
    /// The places (book, page) of the pages being read.
    reading: Vec<(String, i64)>,
}

/// A page being read at its place; giving it up wakes the later pages
/// waiting for it.
pub(crate) struct Reading<'a> {
    pages: &'a Pages,
    place: Option<(String, i64)>,
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        let Some(place) = &self.place else { return };
        let mut st = self.pages.lock();
        if let Some(i) = st.reading.iter().position(|p| p == place) {
            st.reading.swap_remove(i);
        }
        drop(st);
        self.pages.done.notify_all();
    }
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

    fn reading(&self, place: &Option<(String, i64)>) -> Reading<'_> {
        if let Some(p) = place {
            self.lock().reading.push(p.clone());
        }
        Reading { pages: self, place: place.clone() }
    }

    /// Waits while an earlier page of the same book is being read.
    fn after_earlier(&self, place: &Option<(String, i64)>) {
        let Some((book, seq)) = place else { return };
        let mut st = self.lock();
        while st.reading.iter().any(|(b, s)| b == book && s < seq) {
            st = self.done.wait(st).unwrap_or_else(|e| e.into_inner());
        }
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

/// A line of a page saved before Gemini's answer was kept, made again from
/// its placement: the box drawn round Gemini's text box (grown by [`GROW`]
/// on each side) taken back to that box, as a fraction of the page.
fn kept_item(p: &store::PlacementRow, src: &str, w: u32, h: u32) -> PageItem {
    let (cx, cy) = (p.x as f64 + p.w as f64 / 2.0, p.y as f64 + p.h as f64 / 2.0);
    let (hw, hh) = (p.w as f64 / (2.0 + 4.0 * GROW), p.h as f64 / (2.0 + 4.0 * GROW));
    let (w, h) = (w as f64, h as f64);
    PageItem { text: [(cy - hh) / h, (cx - hw) / w, (cy + hh) / h, (cx + hw) / w], balloon: None, speaker: String::new(), src: src.to_string(), ko: p.ko.clone() }
}

/// Picture pixels `[x0, y0, x1, y1]` of a fractional `[top, left, bottom, right]` box.
fn px(b: [f64; 4], w: u32, h: u32) -> [f64; 4] {
    [b[1] * w as f64, b[0] * h as f64, b[3] * w as f64, b[2] * h as f64]
}

/// The white shapes of a page's balloons. Each follows its balloon's printed
/// outline where the page shows one (`balloon::find`); otherwise it is the
/// rounded rectangle of [`shape`]. Either way the Korean is sized from the
/// original lettering under it.
#[cfg(test)]
pub(crate) fn shapes(items: &[PageItem], page: &export::Rgb, lang: &str) -> Vec<PageBox> {
    laid_out(items, page, lang).0
}

/// [`shapes`], with the size Korean takes to read as large as the page's
/// usual lettering (page pixels; none where no lettering is found): the
/// reader sets no Korean smaller than that where the page is shown so
/// small that it is under 7 px (a two-page spread across a phone).
pub(crate) fn laid_out(items: &[PageItem], page: &export::Rgb, lang: &str) -> (Vec<PageBox>, Option<f64>) {
    let (w, h) = (page.w, page.h);
    let gray = page.gray();
    let pg = balloon::Page { w, h, px: &gray };
    let vertical = lang == "ja";
    // Gemini's boxes say roughly where; the lettering on the page says exactly.
    let mut blocks = balloon::blocks(&pg, vertical);
    let lettering_px = balloon::usual(&blocks).map(|g| g * layout::KO_SCALE);
    let letters = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).count();
    let mut wanted: Vec<([f64; 4], usize)> = items.iter().map(|it| (px(it.text, w, h), letters(&it.src))).collect();
    let mut on = balloon::assign(&wanted, &blocks, w);
    // Lettering larger than any letter (a shout lettered across a panel,
    // ch257 p15 "IN HER HUH?!"): where a line's own lettering is not found
    // and its box shares out to letters that large, the page is read again
    // for letters up to that size, and blocks of them lying mostly in the
    // box join the page's.
    let letter_max = balloon::LETTER_MAX * w as f64;
    let big: Vec<(usize, f64)> = wanted
        .iter()
        .enumerate()
        .filter(|&(i, &(_, n))| n > 0 && on[i].is_none_or(|j| 2 * blocks[j].letters < n))
        .map(|(i, &(t, n))| (i, ((t[2] - t[0]).max(0.0) * (t[3] - t[1]).max(0.0) / n as f64).sqrt()))
        .filter(|&(_, g)| g > letter_max)
        .collect();
    if let Some(g) = big.iter().map(|&(_, g)| g).reduce(f64::max) {
        let mostly = |b: &[f64; 4], t: &[f64; 4]| (b[2].min(t[2]) - b[0].max(t[0])).max(0.0) * (b[3].min(t[3]) - b[1].max(t[1])).max(0.0) >= 0.5 * (b[2] - b[0]) * (b[3] - b[1]);
        for b in balloon::blocks_up_to(&pg, vertical, balloon::BIG_LETTER * g) {
            if b.glyph > letter_max && big.iter().any(|&(i, _)| mostly(&b.bounds, &wanted[i].0)) {
                blocks.push(b);
            }
        }
        on = balloon::assign(&wanted, &blocks, w);
    }
    balloon::nudge(&mut wanted, &blocks, &mut on);
    // A line lettered in another's block gets its own part of it where
    // the block parts between the two, stacked or side by side (ch256 p12,
    // a narration on the art and the balloon it ends on; p11, two shouts in
    // one balloon): each is then covered as what it is.
    let speakers: Vec<&str> = items.iter().map(|it| it.speaker.as_str()).collect();
    for (i, k) in hosts(&wanted, &blocks, &on, &speakers).into_iter().enumerate().filter_map(|(i, k)| Some((i, k?))) {
        let Some(j) = on[k] else { continue };
        if let Some([own, other]) = balloon::split(&pg, &blocks[j], [wanted[k], wanted[i]]) {
            blocks[j] = own;
            blocks.push(other);
            on[i] = Some(blocks.len() - 1);
        }
    }
    let pieces = balloon::gather(&wanted, &blocks, &on);
    let union = |p: &[[f64; 4]]| p.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]);
    let texts: Vec<[f64; 4]> = pieces.iter().zip(&wanted).map(|(p, &(t, _))| if p.is_empty() { t } else { union(p) }).collect();
    let host = hosts(&wanted, &blocks, &on, &speakers);
    let boxes: Vec<(PageBox, bool)> = items
        .iter()
        .enumerate()
        .map(|(i, own)| {
            // Its Korean, and that of lines lettered in its block.
            let lines: Vec<&str> = items.iter().enumerate().filter(|&(k, _)| k == i || host[k] == Some(i)).map(|(_, x)| x.ko.as_str()).collect();
            let it = &PageItem { ko: lines.join(" "), ..own.clone() };
            let others: Vec<[f64; 4]> = texts.iter().enumerate().filter(|&(j, _)| j != i && host[j].is_none()).map(|(_, &t)| t).collect();
            // The pieces of lettering gathered from beside the item's own
            // that a cover over `cover` leaves out, and the Korean shared
            // out at its sentence ends between the cover (first) and them,
            // as the letters each holds.
            // The line's lettering size: its largest, where a reading is
            // lettered small over it ("nana" over "7!", ch256 p4).
            let glyph = on[i].map(|j| pieces[i].iter().filter_map(|p| blocks.iter().find(|b| b.bounds == *p)).map(|b| b.glyph).fold(blocks[j].glyph, f64::max));
            let within = |p: &[f64; 4], c: [f64; 4]| p[0] >= c[0] && p[1] >= c[1] && p[2] <= c[2] && p[3] <= c[3];
            let letters_in = |c: [f64; 4]| -> f64 {
                pieces[i].iter().filter(|p| within(p, c)).filter_map(|p| blocks.iter().find(|b| b.bounds == *p)).map(|b| b.letters as f64).sum()
            };
            // Beside a balloon of its own, a piece with no balloon round it
            // is the item's only if it lies mostly in Gemini's box: else it
            // is the art, gathered to make up letters the balloon's block
            // missed (ch256 p6, brick lines beside "RIIIN.", whose thin
            // "III" the block left out).
            let mostly_in = |p: &[f64; 4], t: [f64; 4]| (p[2].min(t[2]) - p[0].max(t[0])).max(0.0) * (p[3].min(t[3]) - p[1].max(t[1])).max(0.0) >= 0.5 * (p[2] - p[0]) * (p[3] - p[1]);
            let apart_from = |cover: [f64; 4], ballooned: bool| -> (Vec<[f64; 4]>, Option<Vec<String>>) {
                let sep: Vec<[f64; 4]> = pieces[i]
                    .iter()
                    .skip(1)
                    .filter(|p| !within(p, cover))
                    .filter(|p| !ballooned || mostly_in(p, wanted[i].0) || balloon::find(&pg, **p, glyph.unwrap_or(0.0), &others).is_some())
                    .copied()
                    .collect();
                if sep.is_empty() {
                    return (sep, None);
                }
                let rects: Vec<[f64; 4]> = std::iter::once(cover).chain(sep.iter().copied()).collect();
                let shares: Vec<f64> = rects.iter().map(|&r| letters_in(r).max(1.0)).collect();
                let runs = share_out(&it.ko, &rects, &shares);
                (sep, runs)
            };
            // Each piece apart under its own balloon's outline, or else a
            // rectangle of its own cut back from other items' lettering;
            // with where its share of the Korean goes (`x, y, w, h`).
            // The white over lettering with no balloon round it (in `rs`):
            // its letters and their rim, not a box over the art; none where
            // no letters are found there. Dark letters, or, where none are
            // and the art there is mostly not paper (a dark or a midtone
            // ground), letters lighter than it: sought on the page lifted
            // against that ground ([`export::lift`]), where they are dark
            // letters on paper, and given with the ground. With it, where
            // the letters are, less those of pieces apart (`sep`), which
            // have Korean of their own; only those [`in_line`] with the
            // lettering's block (`from`), if it has one.
            let lettering = |rs: &[[i64; 4]], glyph: Option<f64>, sep: &[[f64; 4]], from: Option<[f64; 4]>| -> Option<(Vec<[i64; 4]>, [i64; 4], Option<u8>)> {
                // Letters of another text the region takes in stay: those
                // centred, outside the item's own lettering, in a block of
                // a word or more that another item took or that lies mostly
                // outside Gemini's box (ch257 p11, the "GRND" balloons
                // under "IT'S ABOUT TO—!", wiped with it). A block in the
                // box nobody took may be the item's own, split off ("4...,"
                // in ch256 p21 "3... 4...? 7...???").
                let theirs = |f: &[i64; 4]| {
                    let (cx, cy) = ((f[0] + f[2]) as f64 / 2.0, (f[1] + f[3]) as f64 / 2.0);
                    let at = |b: &[f64; 4]| cx >= b[0] && cx <= b[2] && cy >= b[1] && cy <= b[3];
                    let another = |j: usize| on.iter().enumerate().any(|(k, o)| k != i && *o == Some(j));
                    !pieces[i].iter().any(at) && blocks.iter().enumerate().any(|(j, b)| b.letters >= balloon::WORD_LETTERS && (another(j) || !mostly_in(&b.bounds, wanted[i].0)) && at(&b.bounds))
                };
                let lettered: Vec<[f64; 4]> = blocks.iter().map(|b| b.bounds).collect();
                let seek = |pg: &balloon::Page| -> Vec<[i64; 4]> { rs.iter().flat_map(|r| balloon::silhouette(pg, r.map(|v| v as f64), glyph.unwrap_or(GLYPH_GUESS * w as f64), &lettered)).filter(|f| !theirs(f)).collect() };
                let mut fill = seek(&pg);
                let light = if fill.is_empty() { ground(&pg, rs) } else { None };
                if let Some(g) = light {
                    let lifted: Vec<u8> = gray.iter().map(|&v| export::lift(v, g)).collect();
                    fill = seek(&balloon::Page { w, h, px: &lifted });
                }
                if let Some(b) = from {
                    fill = in_line(fill, b, (glyph.unwrap_or(GLYPH_GUESS * w as f64) * LINE_RUN) as i64, vertical);
                }
                let mine: Vec<[i64; 4]> = fill.iter().filter(|f| !sep.iter().any(|p| (f[0] as f64) < p[2] && (f[2] as f64) > p[0] && (f[1] as f64) < p[3] && (f[3] as f64) > p[1])).copied().collect();
                let at = union(&if mine.is_empty() { &fill } else { &mine }.iter().map(|f| f.map(|v| v as f64)).collect::<Vec<_>>()).map(|v| v as i64);
                if fill.is_empty() {
                    return None;
                }
                Some((fill, at, light))
            };
            let beside = |sep: &[[f64; 4]]| -> Vec<(Vec<[i64; 2]>, Vec<[i64; 4]>, [i64; 4], Option<u8>)> {
                sep.iter()
                    .map(|p| {
                        if let Some(f) = balloon::find(&pg, *p, glyph.unwrap_or(0.0), &others) {
                            let [x0, y0, x1, y1] = f.text;
                            return (f.outline, vec![], [x0, y0, x1 - x0, y1 - y0], None);
                        }
                        let (mx, my) = (((p[2] - p[0]) * GROW).max(GROW_MIN_PX), ((p[3] - p[1]) * GROW).max(GROW_MIN_PX));
                        let r = apart([p[0] - mx, p[1] - my, p[2] + mx, p[3] + my], *p, &others);
                        let r = [r[0] as i64, r[1] as i64, r[2].ceil() as i64, r[3].ceil() as i64];
                        let pad = PAD as i64;
                        // Its letters sought a letter further along the line:
                        // a block may leave out a line's end letters ("T" and
                        // "S" of "THAT'S IT!", ch258 p18).
                        let g = glyph.unwrap_or(0.0);
                        let (lx, ly) = if vertical { (mx, my.max(g)) } else { (mx.max(g), my) };
                        let line = apart([p[0] - lx, p[1] - ly, p[2] + lx, p[3] + ly], *p, &others);
                        let line = [line[0] as i64, line[1] as i64, line[2].ceil() as i64, line[3].ceil() as i64];
                        // Its Korean goes over all the letters covered, not
                        // the block alone: "이거였어!" set in "THAT'S IT!"'s
                        // block was 1.7 px.
                        let (white, tone, light, a) = lettering(&[line], glyph, &[], None).map_or_else(|| (rect_outline(r), vec![], None, r), |f| (vec![], f.0, f.2, [r[0].min(f.1[0]), r[1].min(f.1[1]), r[2].max(f.1[2]), r[3].max(f.1[3])]));
                        (white, tone, [a[0] + pad, a[1] + pad, (a[2] - a[0] - 2 * pad).max(1), (a[3] - a[1] - 2 * pad).max(1)], light)
                    })
                    .collect()
            };
            let parts = |runs: Option<Vec<String>>, own: [i64; 4], more: &[(Vec<[i64; 2]>, Vec<[i64; 4]>, [i64; 4], Option<u8>)], scale: f64| -> Vec<layout::Part> {
                let areas = std::iter::once(own).chain(more.iter().map(|m| m.2));
                runs.map_or(vec![], |runs| {
                    areas.zip(runs).map(|(a, ko)| layout::Part { text: a, font_px: layout::font_for(&ko, a[2] as u32, a[3] as u32, glyph.map(|g| g * scale)), ko }).collect()
                })
            };
            // The white edging lettering on the art may have: a quarter letter.
            let edging = |glyph: Option<f64>| (0.25 * glyph.unwrap_or(GLYPH_GUESS * w as f64)).round() as i64;
            // The art's tone over letters (`tone`): light ones (on the
            // ground `light`) toned on the page lifted against it and the
            // fill turned back.
            let toned = |tone: &[[i64; 4]], glyph: Option<f64>, light: Option<u8>| match light {
                Some(g) => export::tone_light(page, tone, edging(glyph), g),
                None => export::tone(page, tone, edging(glyph)),
            };
            // Light letters only if every piece toned (`.0`) has light
            // ones (`.1`): toned against the first one's ground.
            let all_light = |pieces: &mut dyn Iterator<Item = (bool, Option<u8>)>| {
                let toned: Vec<Option<u8>> = pieces.filter(|p| p.0).map(|p| p.1).collect();
                if toned.iter().all(Option::is_some) { toned.first().copied().flatten() } else { None }
            };
            let around = |t: [f64; 4], glyph: f64| balloon::find(&pg, t, glyph, &others).map(|f| balloon::Found { glyph: Some(glyph), ..f });
let found = on[i].and_then(|j| {
                let (own, glyph) = (blocks[j].bounds, glyph.unwrap_or(blocks[j].glyph));
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
            let rectish = found.is_none();
            let mut b = match found {
                Some(f) => {
                    let [x0, y0, x1, y1] = f.text;
                    let (tw, th) = ((x1 - x0) as u32, (y1 - y0) as u32);
                    let (sep, runs) = apart_from(f.bounds.map(|v| v as f64), true);
                    let more = beside(&sep);
                    let parts = parts(runs, [x0, y0, tw as i64, th as i64], &more, 1.0);
                    let tone: Vec<[i64; 4]> = more.iter().flat_map(|m| m.1.iter().copied()).collect();
                    let light = all_light(&mut more.iter().map(|m| (!m.1.is_empty(), m.3)));
                    let (outline, b) = joined(std::iter::once(f.outline).chain(more.into_iter().map(|m| m.0)).collect());
                    let b = tone.iter().fold(b, |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]);
                    // A line in one balloon the reader sets along its
                    // outline, wider through the middle than any rectangle
                    // in it: the size starts from the cover's own extent
                    // round the text, and the reader steps it down until
                    // every line is inside.
                    let flow = {
                        let [l, t, r, btm] = f.bounds;
                        let cy = (y0 + y1) / 2;
                        let fh = 2 * (cy - t).min(btm - cy);
                        layout::font_for(&it.ko, (FLOW * (r - l) as f64) as u32, (FLOW * fh as f64) as u32, f.glyph)
                    };
                    let font = layout::font_for(&it.ko, tw, th, f.glyph);
                    PageBox {
                        x: b[0],
                        y: b[1],
                        w: (b[2] - b[0]) as u32,
                        h: (b[3] - b[1]) as u32,
                        radius: 0,
                        font_px: font,
                        flow_px: parts.is_empty().then_some(font.max(flow)),
                        ko: it.ko.clone(),
                        outline,
                        text: [x0, y0, tw as i64, th as i64],
                        parts,
                        patch: toned(&tone, glyph, light),
                    }
                }
                // No closed balloon: a rounded rectangle over the lettering
                // found, or over Gemini's box when none was.
                // No lettering found for it, and none in Gemini's box either
                // (a shout fused with the panel frame, the box on the hair
                // below it): a white box there would wipe the art and leave
                // the lettering. The line is kept, with nothing drawn.
                // Lettered in another item's block: drawn in its cover.
                None if host[i].is_some() => PageBox { x: 0, y: 0, w: 0, h: 0, radius: 0, font_px: 0, ko: it.ko.clone(), outline: vec![], text: [0, 0, 0, 0], parts: vec![], patch: None, flow_px: None },
                None if on[i].is_none() && balloon::marks_in(&pg, wanted[i].0) < (wanted[i].1 / 2).max(1) => {
                    let t = wanted[i].0;
                    let (x, y) = (t[0].max(0.0) as i64, t[1].max(0.0) as i64);
                    PageBox { x, y, w: 0, h: 0, radius: 0, font_px: 0, ko: it.ko.clone(), outline: vec![], text: [x, y, 0, 0], parts: vec![], patch: None, flow_px: None }
                }
                None => {
                    // The lettering found, and as much of Gemini's box as lies
                    // within a letter or two of it: lettering the page shows
                    // faintly (white letters on a gray balloon, outlined on
                    // foliage) or in pieces is still covered, and a box
                    // Gemini put off to one side is not.
                    let t = match on[i] {
                        Some(j) => {
                            // Letters missing from all its pieces are faint
                            // lettering; letters in a piece apart are not, and
                            // the box between is art (ch257 p8, Rentarou's face).
                            let found: usize = pieces[i].iter().filter_map(|p| blocks.iter().find(|b| b.bounds == *p)).map(|b| b.letters).sum();
                            let reach = if found.max(blocks[j].letters) < wanted[i].1 { balloon::REACH } else { WIDEN };
                            let (t, g, r) = (blocks[j].bounds, reach * blocks[j].glyph, wanted[i].0);
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
                            // Lines found over the art (a box Gemini put on the
                            // hair, ch256 p10 "YODEL-AY-HEE!") are as tall as
                            // the box: no letter is taller than a letter can be.
                            balloon::glyph(&strokes, vertical).map(|g| g.min(balloon::LETTER_MAX * w as f64))
                        }
                    };
                    // Sized for its own share of a line shared with pieces apart.
                    let (sep, runs) = apart_from(t, false);
                    let ko = runs.as_ref().map_or(it.ko.clone(), |r| r[0].clone());
                    let placed = match on[i] {
                        Some(_) => PageItem { text: [t[1] / h as f64, t[0] / w as f64, t[3] / h as f64, t[2] / w as f64], balloon: None, ko: ko.clone(), ..it.clone() },
                        None => PageItem { ko: ko.clone(), ..it.clone() },
                    };
                    let mut s = shape(&placed, w, h);
                    s.ko = it.ko.clone();
                    let r = apart([s.x as f64, s.y as f64, (s.x + s.w as i64) as f64, (s.y + s.h as i64) as f64], t, &others);
                    (s.x, s.y, s.w, s.h) = (r[0] as i64, r[1] as i64, (r[2] - r[0]) as u32, (r[3] - r[1]) as u32);
                    s.radius = s.radius.min((s.w.min(s.h) / 4) as u16);
                    let pad = PAD as i64;
                    s.text = [s.x + pad, s.y + pad, (s.w as i64 - 2 * pad).max(1), (s.h as i64 - 2 * pad).max(1)];
                    s.font_px = layout::font_for(&it.ko, s.text[2] as u32, s.text[3] as u32, glyph);
                    // The white goes over the lettering (and a margin of it
                    // grown as Gemini's boxes are), not over the art the
                    // rectangle takes in round it.
                    let (mx, my) = (((t[2] - t[0]) * GROW).max(GROW_MIN_PX), ((t[3] - t[1]) * GROW).max(GROW_MIN_PX));
                    let own = [(t[0] - mx).max(s.x as f64) as i64, (t[1] - my).max(s.y as f64) as i64, ((t[2] + mx).ceil() as i64).min(s.x + s.w as i64), ((t[3] + my).ceil() as i64).min(s.y + s.h as i64)];
                    let rect = [s.x, s.y, s.x + s.w as i64, s.y + s.h as i64];
                    let own = if own[0] < own[2] && own[1] < own[3] { own } else { rect };
                    let more = beside(&sep);
                    // The white goes over every dark letter in the
                    // lettering's area. The Korean goes over the blocks
                    // found there (not the art marks round them: an ear, a
                    // face; ch258 p17), larger than the original letters,
                    // so its own white rim hides what the white leaves.
                    let grown = |p: &[f64; 4]| {
                        let (mx, my) = (((p[2] - p[0]) * GROW).max(GROW_MIN_PX), ((p[3] - p[1]) * GROW).max(GROW_MIN_PX));
                        // Off the page's edges by a pad: a letter set
                        // against the edge reads as cut off.
                        let e = PAD as f64;
                        [(p[0] - mx).max(e), (p[1] - my).max(e), (p[2] + mx).ceil().min(w as f64 - e), (p[3] + my).ceil().min(h as f64 - e)]
                    };
                    let blocks_at: Vec<[f64; 4]> = pieces[i].iter().filter(|p| !sep.contains(p)).map(grown).collect();
                    let fill = lettering(&[own], glyph, &sep, on[i].map(|j| blocks[j].bounds));
                    let scale = if fill.is_some() { OVER_ART } else { 1.0 };
                    // Where the blocks are too small for the line (a "..."
                    // block, ch256 p8), the rectangle's area, if the line
                    // reads larger there.
                    if let Some((_, f, _)) = fill {
                        let f = if blocks_at.is_empty() { f } else { union(&blocks_at).map(|v| v as i64) };
                        let at = [f[0], f[1], f[2] - f[0], f[3] - f[1]];
                        let size = |t: [i64; 4]| layout::font_for(&it.ko, t[2] as u32, t[3] as u32, glyph.map(|g| g * scale));
                        let enough = glyph.map_or(0, |g| (g * layout::KO_SCALE).round() as u16);
                        let (here, there) = (size(at), size(s.text));
                        if here >= enough || here >= there {
                            (s.text, s.font_px) = (at, here);
                        } else {
                            s.font_px = there;
                        }
                    }
                    s.parts = parts(runs, s.text, &more, scale);
                    // A rounded white box only where no letters are found;
                    // under letters with no balloon the art's own tone,
                    // carried in over them from round them.
                    if fill.is_some() || !more.is_empty() {
                        let light = all_light(&mut fill.iter().map(|f| (true, f.2)).chain(more.iter().map(|m| (!m.1.is_empty(), m.3))));
                        let (white, mut tone) = fill.map_or_else(|| (rect_outline(rect), vec![]), |f| (vec![], f.0));
                        tone.extend(more.iter().flat_map(|m| m.1.iter().copied()));
                        let (outline, b) = joined(std::iter::once(white).chain(more.into_iter().map(|m| m.0)).collect());
                        let t = s.text;
                        let b = tone.iter().fold(b, |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]);
                        let b = [b[0].min(t[0]), b[1].min(t[1]), b[2].max(t[0] + t[2]), b[3].max(t[1] + t[3])];
                        (s.outline, s.radius, s.patch) = (outline, 0, toned(&tone, glyph, light));
                        (s.x, s.y, s.w, s.h) = (b[0], b[1], (b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
                    }
                    s
                }
            };
            if lines.len() > 1 && b.parts.is_empty() {
                b.parts = vec![layout::Part { text: b.text, font_px: b.font_px, ko: it.ko.clone() }];
            }
            b.ko = own.ko.clone();
            (b, rectish)
        })
        .collect();
    let (boxes, rectish): (Vec<PageBox>, Vec<bool>) = boxes.into_iter().unzip();
    (clear_texts(boxes, &texts, &rectish), lettering_px)
}

/// The text areas of covers with no balloon (`rectish`) cut back from the
/// other covers' text areas, on the side away from their own lettering
/// (`texts`): two lines set over the art never run into each other (ch256
/// p6, two captions side by side on the phone's wider lines). A cover drawn
/// as a plain rectangle is cut with its text area. The size steps down only
/// as far as the smaller area needs.
fn clear_texts(mut boxes: Vec<PageBox>, texts: &[[f64; 4]], rectish: &[bool]) -> Vec<PageBox> {
    let areas: Vec<[f64; 4]> = boxes.iter().map(|b| [b.text[0] as f64, b.text[1] as f64, (b.text[0] + b.text[2]) as f64, (b.text[1] + b.text[3]) as f64]).collect();
    let pad = PAD as i64;
    for (i, b) in boxes.iter_mut().enumerate() {
        if b.w == 0 || !rectish[i] {
            continue;
        }
        let others: Vec<[f64; 4]> = areas.iter().enumerate().filter(|&(j, a)| j != i && a[2] > a[0]).map(|(_, a)| *a).collect();
        let t = b.text;
        let r = [(t[0] - pad) as f64, (t[1] - pad) as f64, (t[0] + t[2] + pad) as f64, (t[1] + t[3] + pad) as f64];
        let c = apart(r, texts[i], &others);
        if c == r {
            continue;
        }
        let old = b.text;
        let c = [c[0] as i64, c[1] as i64, c[2] as i64, c[3] as i64];
        if b.outline.is_empty() && b.patch.is_none() {
            (b.x, b.y, b.w, b.h) = (c[0], c[1], (c[2] - c[0]) as u32, (c[3] - c[1]) as u32);
            b.radius = b.radius.min((b.w.min(b.h) / 4) as u16);
        }
        b.text = [c[0] + pad, c[1] + pad, (c[2] - c[0] - 2 * pad).max(1), (c[3] - c[1] - 2 * pad).max(1)];
        let size = |ko: &str, font: u16, t: [i64; 4]| layout::font_for(ko, t[2] as u32, t[3] as u32, Some(font as f64 / layout::KO_SCALE)).min(font);
        b.font_px = size(&b.ko, b.font_px, b.text);
        for part in b.parts.iter_mut().filter(|p| p.text == old) {
            (part.text, part.font_px) = (b.text, size(&part.ko, part.font_px, b.text));
        }
    }
    boxes
}

/// The marks of `marks` lettered with `block`: those on its lines or on a
/// line added to it (above, below or beside it), and those on the line of
/// one of these a word `space` from it, where a line runs on past the
/// block's corner (ch257 p14, "SO PRETTY!" wider than "THE PETALS"). A mark
/// off a corner of the block with no letter of a line beside it is art
/// (ch256 p12, the ear beside "WHAT IS SHE EVEN ON ABOUT?!").
fn in_line(marks: Vec<[i64; 4]>, block: [f64; 4], space: i64, vertical: bool) -> Vec<[i64; 4]> {
    let b = block;
    let (mut kept, mut rest): (Vec<[i64; 4]>, Vec<[i64; 4]>) = marks.into_iter().partition(|f| {
        let (cx, cy) = ((f[0] + f[2]) as f64 / 2.0, (f[1] + f[3]) as f64 / 2.0);
        (cx >= b[0] && cx <= b[2]) || (cy >= b[1] && cy <= b[3])
    });
    let (a, c) = if vertical { (1, 0) } else { (0, 1) };
    let beside = |f: &[i64; 4], k: &[i64; 4]| {
        let across = f[c + 2].min(k[c + 2]) - f[c].max(k[c]);
        let gap = f[a].max(k[a]) - f[a + 2].min(k[a + 2]);
        2 * across >= (f[c + 2] - f[c]).min(k[c + 2] - k[c]) && gap <= space
    };
    loop {
        let (more, left): (Vec<[i64; 4]>, Vec<[i64; 4]>) = rest.into_iter().partition(|f| kept.iter().any(|k| beside(f, k)));
        rest = left;
        if more.is_empty() {
            return kept;
        }
        kept.extend(more);
    }
}

/// For each item with no block of its own, the item whose block holds its
/// lettering too: Gemini's box touches that block, and the block's letters
/// come nearer the two lines' letters together than the other's alone
/// (ch256 p12, "SURELY, HER FRIEND..." and "THE PEOPLE DEMAND MORE 13!"
/// lettered as one block, Gemini's boxes off to one side). Never the line
/// of another speaker: drawn in one cover, two speakers' lines would read
/// as one, so such a line keeps a place of its own (`speakers`, blank or
/// "Unknown" where Gemini could not tell, which may be either).
fn hosts(wanted: &[([f64; 4], usize)], blocks: &[balloon::Block], on: &[Option<usize>], speakers: &[&str]) -> Vec<Option<usize>> {
    let known = |s: &str| !s.trim().is_empty() && !s.trim().eq_ignore_ascii_case("unknown");
    let apart = |a: &str, b: &str| known(a) && known(b) && !a.trim().eq_ignore_ascii_case(b.trim());
    let mut host = vec![None; wanted.len()];
    let mut held: Vec<usize> = wanted.iter().map(|w| w.1).collect();
    for i in (0..wanted.len()).filter(|&i| on[i].is_none()) {
        let r = wanted[i].0;
        let overlap = |b: [f64; 4]| (r[2].min(b[2]) - r[0].max(b[0])).max(0.0) * (r[3].min(b[3]) - r[1].max(b[1])).max(0.0);
        let best = (0..wanted.len())
            .filter_map(|k| on[k].map(|j| (k, j)))
            .filter(|&(k, _)| !apart(speakers[k], speakers[i]))
            .filter(|&(k, j)| {
                let n = blocks[j].letters as f64;
                overlap(blocks[j].bounds) > 0.0 && (n - (held[k] + wanted[i].1) as f64).abs() < (n - held[k] as f64).abs()
            })
            .max_by(|a, b| overlap(blocks[a.1].bounds).total_cmp(&overlap(blocks[b.1].bounds)));
        if let Some((k, _)) = best {
            host[i] = Some(k);
            held[k] += wanted[i].1;
        }
    }
    host
}

/// A line Gemini gave for the lettering at `rects` (`x0, y0, x1, y1`),
/// shared out at its sentence ends: a run for each in reading order (top to
/// bottom, right to left across a row), as near its `shares` of the letters
/// as the sentence ends allow, returned in `rects`' order. None when it has
/// fewer sentences than places; the first then holds it all.
fn share_out(ko: &str, rects: &[[f64; 4]], shares: &[f64]) -> Option<Vec<String>> {
    // Insertion by "comes first": wholly above, or level and to the right.
    let first = |a: &[f64; 4], b: &[f64; 4]| a[3] <= b[1] || (b[3] > a[1] && a[0] + a[2] > b[0] + b[2]);
    let mut order: Vec<usize> = (0..rects.len()).collect();
    for k in 1..order.len() {
        let mut m = k;
        while m > 0 && first(&rects[order[m]], &rects[order[m - 1]]) {
            order.swap(m, m - 1);
            m -= 1;
        }
    }
    let runs = layout::split(ko, &order.iter().map(|&k| shares[k]).collect::<Vec<_>>())?;
    let mut out = vec![String::new(); rects.len()];
    for (&k, run) in order.iter().zip(runs) {
        out[k] = run;
    }
    Some(out)
}

/// A rectangle cover `c` over lettering `own`, cut back from the other
/// items' lettering it reaches into, on whichever side gives up least: two
/// captions side by side each keep their own and neither white box hides
/// the other's Korean.
/// The ground of the rectangles `rs` (`x0, y0, x1, y1`): their median
/// pixel, where most of them is not paper (a dark or a midtone ground).
fn ground(page: &balloon::Page, rs: &[[i64; 4]]) -> Option<u8> {
    let (w, h) = (page.w as i64, page.h as i64);
    let mut px: Vec<u8> = rs.iter().flat_map(|r| (r[1].max(0)..r[3].min(h)).flat_map(move |y| (r[0].max(0)..r[2].min(w)).map(move |x| page.px[(y * w + x) as usize]))).collect();
    let papered = px.iter().filter(|&&v| v >= balloon::LIGHT).count();
    let n = px.len();
    (n > 0 && 2 * papered < n).then(|| *px.select_nth_unstable(n / 2).1)
}

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
    let cover = layout::Cover { outline: b.outline.clone(), text: b.text, parts: b.parts.clone(), patch: b.patch.clone(), flow_px: b.flow_px };
    let cover = serde_json::to_string(&cover).ok();
    store::PlacementRow { id: 0, fingerprint, x: b.x, y: b.y, w: b.w, h: b.h, radius: b.radius, font_px: b.font_px, ko: b.ko.clone(), cover }
}

/// The shape of a library row: a rounded rectangle unless its cover says otherwise.
fn page_box(p: &store::PlacementRow) -> PageBox {
    let pad = PAD as i64;
    let cover: Option<layout::Cover> = p.cover.as_deref().and_then(|c| serde_json::from_str(c).ok());
    let (outline, text, parts, patch, flow_px) = match cover {
        Some(c) => (c.outline, c.text, c.parts, c.patch, c.flow_px),
        None => (vec![], [p.x + pad, p.y + pad, (p.w as i64 - 2 * pad).max(1), (p.h as i64 - 2 * pad).max(1)], vec![], None, None),
    };
    PageBox { x: p.x, y: p.y, w: p.w, h: p.h, radius: p.radius, font_px: p.font_px, flow_px, ko: p.ko.clone(), outline, text, parts, patch }
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
    PageBox { x: s.x, y: s.y, w: s.w, h: s.h, radius: s.radius, font_px: s.font_px, ko: it.ko.clone(), outline: vec![], text: [s.x, s.y, s.w as i64, s.h as i64], parts: vec![], patch: None, flow_px: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: [f64; 4], balloon: Option<[f64; 4]>) -> PageItem {
        PageItem { text, balloon, speaker: String::new(), src: String::new(), ko: "안녕".into() }
    }

    /// A Gemini stand-in answering one line ("HELLO" => "안녕") on any page,
    /// keeping each request's prompt.
    fn page_gemini() -> (String, Arc<Mutex<Vec<String>>>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let prompts = Arc::new(Mutex::new(vec![]));
        let seen = prompts.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
                seen.lock().unwrap().push(v["contents"][0]["parts"][0]["text"].as_str().unwrap_or("").to_string());
                let text = serde_json::json!({"box_2d":[150,200,250,300],"balloon_2d":[50,100,350,500],"src":"HELLO","ko":"안녕"}).to_string() + "]}";
                let answer = serde_json::json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}}]});
                let _ = req.respond(tiny_http::Response::from_string(answer.to_string()));
            }
        });
        (url, prompts)
    }

    /// A page whose picture is slow to load (a slow disk, a large file) is
    /// still waited for by the page after it, which joins the book later
    /// but has its picture sooner: its request carries the first page's
    /// lines. For a site chapter and an imported one alike.
    #[test]
    fn a_page_slow_to_load_is_waited_for_by_the_page_after_it() {
        let fixture = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page.jpg")).unwrap();
        let numbered = |n: u8| {
            let mut v = fixture.clone();
            v.push(n);
            v
        };
        for form in ["site", "imported"] {
            let dir = std::env::temp_dir().join(format!("rst-order-{form}-{}-{}", std::process::id(), now_ms()));
            let (url, prompts) = page_gemini();
            let b = Backend::open(dir.clone(), Some(url), Arc::new(Quiet(dir.join("export")))).unwrap();
            b.lock().key = Some("k".into());
            let reqs: Vec<PageIn> = (1..=2).map(|n| PageIn { src: format!("https://cdn.example/{n}.jpg"), image: None, page_url: "https://manga.example/c/1".into(), title: "Fixture".into(), index: n }).collect();
            let pics: Vec<i64> = if form == "imported" {
                let folder = dir.with_extension("pages");
                std::fs::create_dir_all(&folder).unwrap();
                (1..=2u8).for_each(|n| std::fs::write(folder.join(format!("{n}.jpg")), numbered(n)).unwrap());
                let book = b.import_folder(&folder).unwrap();
                b.chapter(book).unwrap().pages.into_iter().map(|p| p.id).collect()
            } else {
                vec![]
            };
            let source = |k: usize| if form == "site" { ReaderSource::Site(&reqs[k]) } else { ReaderSource::Chapter(pics[k]) };
            std::thread::scope(|s| {
                let first = s.spawn(|| b.read_picture(source(0), || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    Ok(numbered(1))
                }));
                std::thread::sleep(std::time::Duration::from_millis(100));
                let second = s.spawn(|| b.read_picture(source(1), || Ok(numbered(2))));
                assert!(!first.join().unwrap().unwrap().saved, "{form}");
                assert!(!second.join().unwrap().unwrap().saved, "{form}");
            });
            let prompts = prompts.lock().unwrap();
            assert_eq!(prompts.len(), 2, "{form}");
            assert!(prompts[1].contains("HELLO => 안녕"), "{form}: {}", prompts[1]);
        }
    }

    /// A library an older version made kept no answer with its pages: the
    /// next page's request still carries an earlier page's lines, from what
    /// was kept with each line, before that page is ever opened again. For a
    /// site chapter and an imported one alike.
    #[test]
    fn an_earlier_page_kept_without_its_answer_still_gives_the_next_request_its_lines() {
        let fixture = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page.jpg")).unwrap();
        let numbered = |n: u8| {
            let mut v = fixture.clone();
            v.push(n);
            v
        };
        for form in ["site", "imported"] {
            let dir = std::env::temp_dir().join(format!("rst-upgraded-{form}-{}-{}", std::process::id(), now_ms()));
            let (url, prompts) = page_gemini();
            let open = || {
                let b = Backend::open(dir.clone(), Some(url.clone()), Arc::new(Quiet(dir.join("export")))).unwrap();
                b.lock().key = Some("k".into());
                b
            };
            let b = open();
            let reqs: Vec<PageIn> = (1..=2).map(|n| PageIn { src: format!("https://cdn.example/{n}.jpg"), image: None, page_url: "https://manga.example/c/1".into(), title: "Fixture".into(), index: n }).collect();
            let pics: Vec<i64> = if form == "imported" {
                let folder = dir.with_extension("pages");
                std::fs::create_dir_all(&folder).unwrap();
                (1..=2u8).for_each(|n| std::fs::write(folder.join(format!("{n}.jpg")), numbered(n)).unwrap());
                let book = b.import_folder(&folder).unwrap();
                b.chapter(book).unwrap().pages.into_iter().map(|p| p.id).collect()
            } else {
                vec![]
            };
            let source = |k: usize| if form == "site" { ReaderSource::Site(&reqs[k]) } else { ReaderSource::Chapter(pics[k]) };
            b.read_picture(source(0), || Ok(numbered(1))).unwrap();
            assert_eq!(b.lock().db.execute("UPDATE ReaderPage SET items = NULL", []).unwrap(), 1, "{form}");
            drop(b);
            let b = open();
            b.read_picture(source(1), || Ok(numbered(2))).unwrap();
            let prompts = prompts.lock().unwrap();
            assert_eq!(prompts.len(), 2, "{form}");
            assert!(prompts[1].contains("HELLO => 안녕"), "{form}: {}", prompts[1]);
            let _ = std::fs::remove_dir_all(&dir);
        }
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

    /// A page saved by an older library kept its rectangles and each line's
    /// source and Korean, but not Gemini's answer. Reopened, it is laid out
    /// again by this version (its cover follows the balloon's outline), its
    /// answer is kept from then on, and no request is sent. The columns that
    /// version draws from stay as it saved them, should it read the library
    /// again.
    #[test]
    fn a_page_saved_by_an_earlier_version_is_laid_out_again_and_keeps_what_that_version_reads() {
        // Saved by a version that kept no cover, and by one that kept a
        // cover without the shape in it: either way the columns an earlier
        // version draws from stay as they were.
        for (form, cover) in [("no cover", None), ("plain cover", Some(r#"{"outline":[],"text":[228,263,144,74]}"#))] {
            let dir = std::env::temp_dir().join(format!("rst-reader-old-{}-{}-{}", form.len(), std::process::id(), now_ms()));
            let b = Backend::open(dir.clone(), Some("http://127.0.0.1:9".into()), Arc::new(Quiet(dir.join("export")))).unwrap();
            b.put_source_lang(LangIn { lang: "en".into() }).unwrap();
            b.lock().key = Some("k".into());
            // A white page with an oval balloon and two lines of letters in it.
            let (w, h) = (600u32, 600u32);
            let mut img = export::Rgb { w, h, px: vec![255; (w * h * 3) as usize] };
            let mut ink = |x: u32, y: u32| img.px[((y * w + x) * 3) as usize..][..3].fill(0);
            for y in 0..h {
                for x in 0..w {
                    let e = ((x as f64 - 300.0) / 150.0).powi(2) + ((y as f64 - 300.0) / 100.0).powi(2);
                    if e > 1.0 && e <= (1.0f64 + 4.0 / 100.0).powi(2) {
                        ink(x, y);
                    }
                }
            }
            // Letters as "L"s, 4 px strokes.
            for y0 in [270, 302] {
                for x0 in [236, 258, 280, 302, 324, 346] {
                    (y0..y0 + 20).for_each(|y| (x0..x0 + 16).filter(|&x| x < x0 + 4 || y >= y0 + 16).for_each(|x| ink(x, y)));
                }
            }
            let bytes = export::encode_png(&img).unwrap();
            let hash: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
            let key = format!("{hash}:en");
            {
                let st = b.lock();
                let book = store::create_book(&st.db, "old", 1).unwrap();
                let pic = store::create_picture(&st.db, book, "page", w).unwrap();
                st.db.execute("INSERT INTO ReaderPage (key, picture_id, page_url, items) VALUES (?1, ?2, 'https://m.example/c/1', NULL)", rusqlite::params![key, pic]).unwrap();
                let fp = format!("page:{key}:0");
                let t = store::Translation { fingerprint: fp.clone(), aspect: 1.0, source_ja: Some("ABCDEF GHIJKL".into()), translation_ko: Some("안녕하세요".into()), status: "ready".into() };
                store::put_translation(&st.db, &t, "m", 1).unwrap();
                store::insert_placement(&st.db, pic, &store::PlacementRow { id: 0, fingerprint: fp, x: 220, y: 255, w: 160, h: 90, radius: 8, font_px: 20, ko: String::new(), cover: cover.map(str::to_string) }).unwrap();
            }
            let columns = |b: &Backend| b.lock().db.query_row("SELECT x, y, w, h, radius, font_px, COUNT(*) OVER () FROM Placement", [], |r| Ok([r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?])).unwrap();
            let read = |b: &Backend| b.read_page(PageIn { src: "https://cdn.example/p.png".into(), image: Some(bytes.clone()), page_url: "https://m.example/c/1".into(), title: "t".into(), index: 0 }).unwrap();
            let out = read(&b);
            assert!(out.saved && out.boxes.len() == 1, "{form}: {out:?}");
            assert!(!out.boxes[0].outline.is_empty(), "{form}: laid out along the balloon, not the old rectangle: {:?}", out.boxes[0]);
            assert_eq!(out.boxes[0].ko, "안녕하세요", "{form}");
            assert_eq!(b.lock().gemini_requests, 0, "{form}");
            let kept: Vec<PageItem> = serde_json::from_str(&store::reader_items(&b.lock().db, &key).unwrap().expect("the answer is kept now")).unwrap();
            assert_eq!((kept.len(), kept[0].src.as_str()), (1, "ABCDEF GHIJKL"), "{form}");
            // What an earlier version draws from is as it saved it, one row;
            // this version reads the new shape back, and again the same.
            assert_eq!(columns(&b), [220, 255, 160, 90, 8, 20, 1], "{form}");
            let rows = store::reader_placements(&b.lock().db, &key).unwrap();
            assert_eq!(page_box(&rows[0]), out.boxes[0], "{form}");
            assert_eq!(read(&b).boxes, out.boxes, "{form}");
            assert_eq!(columns(&b), [220, 255, 160, 90, 8, 20, 1], "{form}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// White letters on a solid black balloon, or black on white, with
    /// Gemini's box loose round them or on their very edge: the line is
    /// lettering, not a box on the art, so it gets a cover over its letters
    /// and its Korean, where white letters were kept with nothing drawn and
    /// the source showed.
    #[test]
    fn letters_in_a_balloon_are_covered_whether_the_box_is_loose_or_tight_on_them() {
        // Light letters on a black balloon and dark ones on a white one;
        // Gemini's box a few pixels round the letters, or on their very
        // edge (rows 270..=321, columns 236..=361).
        for (ground, ink) in [(0u8, 255u8), (255, 0)] {
            for (name, bx) in [("loose", [266.0, 232.0, 326.0, 366.0]), ("tight", [270.0, 236.0, 322.0, 362.0])] {
                let (w, h) = (600u32, 600u32);
                let mut page = export::Rgb { w, h, px: vec![255 - ground; (w * h * 3) as usize] };
                let mut put = |x: u32, y: u32, v: u8| page.px[((y * w + x) * 3) as usize..][..3].fill(v);
                for y in 0..h {
                    for x in 0..w {
                        let d = ((x as f64 - 300.0) / 150.0).powi(2) + ((y as f64 - 300.0) / 100.0).powi(2);
                        if d <= 1.0 {
                            put(x, y, ground);
                        } else if d <= 1.06 {
                            put(x, y, 0);
                        }
                    }
                }
                for y0 in [270, 302] {
                    for x0 in [236, 258, 280, 302, 324, 346] {
                        (y0..y0 + 20).for_each(|y| (x0..x0 + 16).filter(|&x| x < x0 + 4 || y >= y0 + 16).for_each(|x| put(x, y, ink)));
                    }
                }
                let it = PageItem { src: "ABCDEF GHIJKL".into(), ..item([bx[0] / 600.0, bx[1] / 600.0, bx[2] / 600.0, bx[3] / 600.0], None) };
                let b = &shapes(&[it], &page, "en")[0];
                assert!(b.w > 0 && b.h > 0 && b.font_px > 0, "ground {ground}, {name}: drawn: {b:?}");
                assert!(b.x <= 236 && b.y <= 270 && b.x + b.w as i64 >= 362 && b.y + b.h as i64 >= 322, "ground {ground}, {name}: over every letter: {b:?}");
            }
        }
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
        let shape = PageBox { x: b[0], y: b[1], w: 80, h: 50, radius: 0, font_px: 10, ko: String::new(), outline, text: [12, 12, 26, 26], parts: vec![], patch: None, flow_px: None };
        let mut img = export::Rgb { w: 100, h: 100, px: vec![0; 100 * 100 * 3] };
        export::draw_placement(&mut img, &row(String::new(), &shape), 0);
        let white = |x: usize, y: usize| img.px[(y * 100 + x) * 3] == 255;
        // Both parts, and where two overlap.
        assert!(white(20, 20) && white(45, 45) && white(35, 35) && white(80, 50));
        // The face between them, and along the bridge.
        assert!(!white(60, 30) && !white(20, 55) && !white(55, 15));
    }

    /// ch256 p6 on the phone: the left caption's white box reached into the
    /// right caption's text area and hid "린은". Each box is cut back from
    /// the other's text area and keeps its own lettering covered.
    #[test]
    fn a_rectangle_never_covers_another_covers_korean() {
        let rect = |x: i64, y: i64, w: u32, h: u32| PageBox { x, y, w, h, radius: 8, font_px: 20, ko: "이런 걸 하려면 방에 숨어야 하거든.".into(), outline: vec![], text: [x + PAD as i64, y + PAD as i64, w as i64 - 2 * PAD as i64, h as i64 - 2 * PAD as i64], parts: vec![], patch: None, flow_px: None };
        let (left, right) = (rect(560, 60, 160, 90), rect(700, 10, 230, 80));
        let texts = [[580.0, 70.0, 690.0, 140.0], [740.0, 20.0, 900.0, 80.0]];
        let out = clear_texts(vec![left.clone(), right.clone()], &texts, &[true, true]);
        let (l, r) = (&out[0], &out[1]);
        // Neither white box reaches into the other's text area, before or after.
        assert!(l.x + l.w as i64 <= right.text[0] && l.x + l.w as i64 <= r.text[0], "{l:?}");
        assert!(r.x >= left.text[0] + left.text[2], "{r:?}");
        // Each still covers its own lettering.
        assert!(l.x as f64 <= texts[0][0] && (l.x + l.w as i64) as f64 >= texts[0][2]);
        assert!(r.x as f64 <= texts[1][0] && (r.x + r.w as i64) as f64 >= texts[1][2]);
        assert!(l.font_px <= 20 && l.font_px > 0);
    }

    /// ch256 p12: "SURELY, HER FRIEND..." (33 letters) and "THE PEOPLE
    /// DEMAND MORE 13!" (21) are one block of 51 letters; Gemini's second box
    /// lies on the sky beside it. The second line is found lettered in the
    /// first's block: [`balloon::split`] parts the two where the block cuts
    /// between them, and where it does not, the line is drawn in the
    /// first's cover. A line whose box only touches a block that already
    /// fits its owner is not, and neither is another speaker's line: in one
    /// cover the two would read as one speaker, so it keeps its own place.
    /// A speaker Gemini left blank or "Unknown" may be either.
    #[test]
    fn a_line_lettered_in_another_items_block_is_drawn_in_its_cover() {
        let block = |bounds, letters| balloon::Block { bounds, letters, glyph: 23.0, in_balloon: false };
        let blocks = [block([917.0, 719.0, 1141.0, 1066.0], 51), block([583.0, 874.0, 737.0, 1019.0], 13)];
        let wanted = [([1034.0, 815.0, 1170.0, 991.0], 33), ([1026.0, 991.0, 1170.0, 1132.0], 21), ([630.0, 815.0, 786.0, 1132.0], 20), ([700.0, 900.0, 760.0, 1000.0], 8)];
        let on = [Some(0), None, Some(1), None];
        for (owner, line, hosted) in [
            ("Speaker A", "Speaker A", true),
            ("Speaker A", "speaker a ", true),
            ("Speaker A", "", true),
            ("", "Speaker B", true),
            ("Unknown", "Speaker B", true),
            ("Speaker A", "Speaker B", false),
        ] {
            let speakers = [owner, line, "Speaker C", "Speaker A"];
            assert_eq!(hosts(&wanted, &blocks, &on, &speakers), [None, hosted.then_some(0), None, None], "{owner:?} / {line:?}");
        }
    }

    /// ch256 p12: Gemini's box for "WHAT IS SHE EVEN ON ABOUT?!" took in the
    /// ear off the block's top right corner, and the tone blotted it out.
    /// The ear is left; marks over the block's columns, and a line below
    /// running on past its corner a letter at a time (ch257 p14), are covered.
    #[test]
    fn marks_off_a_blocks_corner_are_art_unless_a_line_runs_there() {
        let block = [583.0, 874.0, 737.0, 1019.0];
        let (letter, hair, ear) = ([579, 875, 629, 927], [670, 852, 689, 884], [742, 830, 778, 886]);
        let below = [[700, 1030, 736, 1080], [745, 1030, 780, 1080], [788, 1030, 820, 1080]];
        let marks: Vec<[i64; 4]> = [letter, hair, ear].into_iter().chain(below).collect();
        let mut kept = in_line(marks.clone(), block, 37, false);
        kept.sort();
        let mut want: Vec<[i64; 4]> = [letter, hair].into_iter().chain(below).collect();
        want.sort();
        assert_eq!(kept, want);
        // Lettered down the page, the same with x and y swapped.
        let t = |m: [i64; 4]| [m[1], m[0], m[3], m[2]];
        let mut kept = in_line(marks.into_iter().map(t).collect(), [874.0, 583.0, 1019.0, 737.0], 37, true);
        kept.sort();
        let mut want: Vec<[i64; 4]> = want.into_iter().map(t).collect();
        want.sort();
        assert_eq!(kept, want);
    }

    /// A line Gemini gave for two balloons is set a sentence in each, the
    /// right-hand balloon of a row first, and the shares survive the
    /// library; one sentence stays whole in the first balloon.
    #[test]
    fn a_line_for_two_balloons_is_set_a_sentence_in_each_in_reading_order() {
        let (left, right) = ([10.0, 100.0, 70.0, 140.0], [200.0, 90.0, 260.0, 140.0]);
        let two = "수영부 부장이야. 3학년 5반, 누레미 스이무.";
        assert_eq!(share_out(two, &[left, right], &[1.0, 1.0]).unwrap(), ["3학년 5반, 누레미 스이무.", "수영부 부장이야."]);
        // ch257 p8: cut nearest the letters each piece holds, not a sentence each.
        let p8 = "어떡하지... 뭐 쓸만한 통나무라도 있나...?! 아, 알겠다! 신발을 손에 끼고—";
        assert_eq!(share_out(p8, &[left, right], &[30.0, 30.0]).unwrap(), ["아, 알겠다! 신발을 손에 끼고—", "어떡하지... 뭐 쓸만한 통나무라도 있나...?!"]);
        // A balloon wholly above comes first wherever it is across.
        let above = [300.0, 0.0, 360.0, 40.0];
        assert_eq!(share_out("응. 알았어.", &[left, above], &[1.0, 1.0]).unwrap(), ["알았어.", "응."]);
        assert_eq!(share_out("그 모든 것, 아니 그 이상으로.", &[left, right], &[1.0, 1.0]), None);

        // The shares, and the size set along the outline, survive the library.
        let parts = vec![layout::Part { text: [210, 95, 40, 40], font_px: 12, ko: "수영부 부장이야.".into() }, layout::Part { text: [15, 105, 50, 30], font_px: 11, ko: "3학년 5반, 누레미 스이무.".into() }];
        let (outline, b) = joined(vec![rect_outline([0, 80, 80, 150]), rect_outline([190, 80, 270, 150])]);
        let shape = PageBox { x: b[0], y: b[1], w: 270, h: 70, radius: 0, font_px: 12, ko: two.into(), outline, text: [15, 105, 50, 30], parts, patch: None, flow_px: Some(14) };
        assert_eq!(page_box(&row(String::new(), &shape)), shape);
    }

    /// The user, on the phone: lettering on the art with no balloon had a
    /// white rectangle painted over it and the art around it. Its white is
    /// its letters and their rim, the art between its lines and beside it
    /// stays, and the Korean is set larger over it, its own rim hiding what
    /// the white leaves.
    #[test]
    fn lettering_on_the_art_is_covered_letter_by_letter_under_larger_korean() {
        let (w, h) = (600u32, 600u32);
        // Dark letters on a midtone, and white letters on any ground that is
        // not paper: black, dark, midtone.
        for (art, letter) in [(170u8, 0u8), (0, 255), (50, 255), (120, 255), (170, 255), (185, 255)] {
            let mut page = export::Rgb { w, h, px: vec![art; (w * h * 3) as usize] };
            let mut ink = |x0: u32, y0: u32, x1: u32, y1: u32, v: u8| (y0..y1).for_each(|y| (x0..x1).for_each(|x| page.px[((y * w + x) * 3) as usize..][..3].fill(v)));
            // Two lines of two four-letter words, 16 px letters; art of
            // the letters' shade below.
            for y in [200, 232] {
                for x in [200, 220, 240, 260, 320, 340, 360, 380] {
                    ink(x, y, x + 14, y + 16, letter);
                }
            }
            ink(150, 400, 450, 520, letter);
            let it = PageItem { src: "ABCD EFGH IJKL MNOP".into(), ko: "그래, 그거야!".into(), ..item([190.0 / 600.0, 190.0 / 600.0, 258.0 / 600.0, 404.0 / 600.0], None) };
            let b = &shapes(&[it], &page, "en")[0];
            // No white: the art's tone carried in over the letters, and only them.
            assert!(b.outline.is_empty() && b.radius == 0, "{art}: {b:?}");
            let patch = b.patch.as_ref().expect("a tone patch");
            let (pw, px) = export::patch_px(patch).unwrap();
            let at = |x: i64, y: i64| {
                let (x, y) = (x - patch.at[0], y - patch.at[1]);
                if x < 0 || y < 0 || x >= pw as i64 || y >= patch.at[3] {
                    return None;
                }
                let q = &px[(y as usize * pw as usize + x as usize) * 4..][..4];
                (q[3] == 255).then_some(q[0])
            };
            // Letters at each end; not the art between the words, the lines, or below.
            for (x, y) in [(207, 208), (387, 240), (230, 240), (366, 208)] {
                assert!(at(x, y).is_some_and(|v| v.abs_diff(art) <= 2), "{art} ({x}, {y}): {:?}", at(x, y));
            }
            assert!([(300, 208), (230, 225), (300, 450), (150, 208)].iter().all(|&(x, y)| at(x, y).is_none()), "{art}: {b:?}");
            assert!(b.font_px as f64 > 16.0 * layout::KO_SCALE, "{art}: {b:?}");
        }
    }

    /// ch258 p18: "THAT'S IT!", lettered just under the box Gemini drew
    /// round the line before it, was left in English, and then its end
    /// letters, thinly edged on the dark art and so left out of its block.
    /// The line is the item's, each letter of it covered, ends too.
    #[test]
    fn a_last_line_under_the_box_is_covered_to_its_end_letters() {
        let (w, h) = (600u32, 600u32);
        let mut page = export::Rgb { w, h, px: vec![60; (w * h * 3) as usize] };
        let mut ink = |x0: u32, y0: u32, x1: u32, y1: u32, v: u8| (y0..y1).for_each(|y| (x0..x1).for_each(|x| page.px[((y * w + x) * 3) as usize..][..3].fill(v)));
        // The first line in wide white edging; the second line's four
        // middle letters in wide edging, its end letters in two pixels.
        ink(194, 194, 286, 222, 255);
        ink(212, 228, 296, 254, 255);
        [188u32, 300].iter().for_each(|&x| ink(x, 230, x + 18, 252, 255));
        // Ring letters, 14 x 16 px, strokes 3 px.
        for (x, y) in [(200u32, 200u32), (220, 200), (240, 200), (260, 200), (190, 232), (216, 232), (236, 232), (256, 232), (276, 232), (302, 232)] {
            ink(x, y, x + 14, y + 16, 0);
            ink(x + 3, y + 3, x + 11, y + 13, 255);
        }
        // Gemini's box: the first line only, the second just under it.
        let it = PageItem { src: "ABCD EFGHIJ".into(), ko: "아. 그거구나. 그거야!".into(), ..item([190.0 / 600.0, 190.0 / 600.0, 230.0 / 600.0, 290.0 / 600.0], None) };
        let b = &shapes(&[it], &page, "en")[0];
        let patch = b.patch.as_ref().unwrap_or_else(|| panic!("a tone patch: {b:?}"));
        let (pw, px) = export::patch_px(patch).unwrap();
        let covered = |x: i64, y: i64| {
            let (x, y) = (x - patch.at[0], y - patch.at[1]);
            x >= 0 && y >= 0 && x < pw as i64 && y < patch.at[3] && px[(y as usize * pw as usize + x as usize) * 4 + 3] == 255
        };
        for (x, y) in [(207, 208), (197, 240), (223, 240), (283, 240), (309, 240)] {
            assert!(covered(x, y), "({x}, {y}): {b:?}");
        }
        // Its share of the Korean is set across the whole line, end letters
        // too, not in the block between them.
        let last = b.parts.iter().find(|p| p.text[1] > 225).unwrap_or_else(|| panic!("a part for the last line: {b:?}"));
        assert!(last.text[0] <= 192 && last.text[0] + last.text[2] >= 314 && last.ko == "그거야!", "{last:?}");
    }

    /// ch257 p15: a shout lettered across the panel ("IN HER HUH?!"), its
    /// letters taller than any letter is sought, was left as drawn and its
    /// Korean set in a sliver of the art beside it. Its letters are found
    /// at the size its box gives them, toned over, and the Korean is set
    /// across them.
    #[test]
    fn lettering_larger_than_any_letter_is_covered_where_its_box_says() {
        let (w, h) = (1200u32, 900u32);
        let mut page = export::Rgb { w, h, px: vec![160; (w * h * 3) as usize] };
        let mut fill = |x0: u32, y0: u32, x1: u32, y1: u32, v: u8, ring: bool| {
            for y in y0..y1 {
                for x in x0..x1 {
                    if !ring || x < x0 + 22 || x >= x1 - 22 || y < y0 + 22 || y >= y1 - 22 {
                        page.px[((y * w + x) * 3) as usize..][..3].fill(v);
                    }
                }
            }
        };
        // Two lines of three ring letters 100 x 120 px, strokes 22 px, in
        // white edging on gray art.
        let letters: Vec<(u32, u32)> = [200u32, 360].iter().flat_map(|&y| [100u32, 230, 360].map(|x| (x, y))).collect();
        letters.iter().for_each(|&(x, y)| fill(x - 30, y - 30, x + 130, y + 150, 255, false));
        letters.iter().for_each(|&(x, y)| fill(x, y, x + 100, y + 120, 0, true));
        let it = PageItem { src: "ABC DEF".into(), ko: "그래, 그거야!".into(), ..item([190.0 / 900.0, 90.0 / 1200.0, 490.0 / 900.0, 470.0 / 1200.0], None) };
        let b = &shapes(&[it], &page, "en")[0];
        assert!(b.outline.is_empty() && b.text[2] > 300 && b.text[3] > 200, "{b:?}");
        let patch = b.patch.as_ref().expect("a tone patch");
        let (pw, px) = export::patch_px(patch).unwrap();
        let at = |x: i64, y: i64| {
            let (x, y) = (x - patch.at[0], y - patch.at[1]);
            let q = &px[(y as usize * pw as usize + x as usize) * 4..][..4];
            (q[3] == 255).then_some(q[0])
        };
        // Strokes of the first and last letters, toned as the art round them.
        for (x, y) in [(110, 260), (450, 470), (295, 205)] {
            assert!(at(x, y).is_some_and(|v| v >= 128), "({x}, {y}): {:?}", at(x, y));
        }
        assert!(b.font_px as f64 > 84.0 * layout::KO_SCALE, "{b:?}");
    }

    /// ch256 p6: "RIIIN." in its balloon, the block of it only "R" (the
    /// "III" thin), gathered brick lines beside the balloon that Gemini's box
    /// reached into, and the tone wiped the bricks. The balloon is covered,
    /// and the art beside it is not. The wall is drawn with its joints round
    /// the marks: marks alone on blank paper are lettering with no outline.
    #[test]
    fn art_beside_a_found_balloon_is_not_gathered_into_its_cover() {
        let (w, h) = (600u32, 600u32);
        let mut page = export::Rgb { w, h, px: vec![255; (w * h * 3) as usize] };
        let mut ink = |x: u32, y: u32| page.px[((y * w + x) * 3) as usize..][..3].fill(0);
        for y in 0..h {
            for x in 0..w {
                let e = ((x as f64 - 300.0) / 70.0).powi(2) + ((y as f64 - 200.0) / 50.0).powi(2);
                if e > 1.0 && e <= (1.0f64 + 3.0 / 50.0).powi(2) {
                    ink(x, y);
                }
            }
        }
        // Letters as "C"s, 3 px strokes, open to the paper.
        let mut mark = |x0: u32, y0: u32| (y0..y0 + 20).for_each(|y| (x0..x0 + 18).filter(|&x| x < x0 + 3 || y < y0 + 3 || y >= y0 + 17).for_each(|x| ink(x, y)));
        for x in [280, 302] {
            mark(x, 190);
        }
        for x in [160, 182, 204, 226] {
            mark(x, 255);
        }
        // The rest of the wall: the bricks' joints, short dashes all round.
        for y in [238, 292, 318] {
            for x in (64..232).step_by(24) {
                (x..x + 12).for_each(|x| (y..y + 2).for_each(|y| ink(x, y)));
            }
        }
        let it = PageItem { src: "RIIN".into(), ko: "따르릉.".into(), ..item([140.0 / 600.0, 232.0 / 600.0, 275.0 / 600.0, 380.0 / 600.0], None) };
        let b = &shapes(&[it], &page, "en")[0];
        assert!(!b.outline.is_empty() && b.patch.is_none() && b.x >= 225, "{b:?}");
    }

    #[test]
    fn a_text_box_without_its_balloon_is_covered_a_little_past_its_edges() {
        // Text 200..300 x 400..600: 6 px more each side across, 12 px down.
        let s = shape(&item([0.2, 0.2, 0.3, 0.3], None), 1000, 2000);
        assert_eq!((s.x, s.y, s.w, s.h), (194, 388, 112, 224));
    }
}
