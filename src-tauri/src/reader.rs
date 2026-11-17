//! P12: the reader. A site opened in the app hands over each page picture
//! (`read_page`); one Gemini request finds its balloons and translates them,
//! and the page is saved with its translation, so the same page is never
//! sent again and shows in the library like any other book. The shapes are
//! in the picture's own pixels: the page draws them on the picture itself,
//! so they move with it however the site scrolls, slides or zooms.

use super::*;
use crate::api::{LangIn, PageBox, PageIn, PageOut};
use crate::gemini::PageItem;
use crate::layout::{self, PAD};
use sha2::{Digest, Sha256};

const LANG_SETTING: &str = "source_lang";
/// A page picture larger than this is not a manga page.
const MAX_PAGE_BYTES: u64 = 20 << 20;
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
        let lang = self.source_lang();
        let key = self.lock().key.clone().ok_or(ApiError("key_missing"))?;
        let bytes = match req.image {
            Some(ref b) if !b.is_empty() => b.clone(),
            _ => fetch(&req.src, &req.page_url)?,
        };
        let mime = export::mime(&bytes).ok_or(ApiError("bad_image"))?;
        let img = export::decode_strip(&bytes).ok_or(ApiError("bad_image"))?;
        let (w, h) = (img.w, img.h);
        drop(img);
        let hash: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        let page_key = format!("{hash}:{lang}");
        // Two copies of one picture read at once: the second waits here for
        // the first's answer instead of sending the page again.
        let _turn = self.pages.turn(&page_key);
        // Bound first: an `if let` would hold the lock through `saved_page`.
        let known = store::reader_page(&self.lock().db, &page_key)?;
        if let Some(id) = known {
            return self.saved_page(id, w, h);
        }

        let model = self.lock().model.clone();
        let items = match self.pages.unsaved(&page_key) {
            // Read before, but not kept: only the saving is tried again.
            Some(items) => items,
            None => self.ask_page(&bytes, mime, &lang, &model, &key)?,
        };
        let boxes: Vec<PageBox> = items.iter().map(|it| shape(it, w, h)).collect();

        let mut guard = self.lock();
        let st = &mut *guard;
        let now = now_ms();
        let ext = mime.trim_start_matches("image/");
        let mut written = None;
        let saved = (|| -> ApiResult<()> {
            let tx = st.db.unchecked_transaction()?;
            let book = match store::reader_book(&tx, &req.page_url)? {
                Some(b) => b,
                None => store::create_book(&tx, &title(&req), now)?,
            };
            let pic = store::create_picture(&tx, book, "page", w)?;
            store::set_picture_seq(&tx, pic, req.index)?;
            store::set_covered(&tx, pic, 0, h as i64)?;
            store::close_picture(&tx, pic, now)?;
            // The picture as the site sent it, in the app's own folder (E5).
            let rel_dir = format!("strips/{pic}");
            std::fs::create_dir_all(self.data_dir.join(&rel_dir)).map_err(storage_error)?;
            let rel = format!("{rel_dir}/page.{ext}");
            written = Some(self.data_dir.join(&rel));
            std::fs::write(self.data_dir.join(&rel), &bytes).map_err(storage_error)?;
            store::insert_strip(&tx, pic, 0, h, &rel, now)?;
            for (i, (it, b)) in items.iter().zip(&boxes).enumerate() {
                // Not a balloon fingerprint (no hex): screen capture never matches it.
                let fingerprint = format!("page:{page_key}:{i}");
                let t = store::Translation { fingerprint: fingerprint.clone(), aspect: b.w as f32 / b.h.max(1) as f32, source_ja: Some(it.src.clone()), translation_ko: Some(it.ko.clone()), status: "ready".into() };
                store::put_translation(&tx, &t, &model, now)?;
                let row = store::PlacementRow { id: 0, fingerprint, x: b.x, y: b.y, w: b.w, h: b.h, radius: b.radius, font_px: b.font_px, ko: b.ko.clone() };
                store::insert_placement(&tx, pic, &row)?;
            }
            store::insert_reader_page(&tx, &page_key, pic, &req.page_url)?;
            store::end_book(&tx, book, now)?;
            tx.commit()?;
            Ok(())
        })();
        drop(guard);
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
        Ok(PageOut { width: w, height: h, saved: false, boxes, save_error })
    }

    /// One Gemini request for the whole page (R9, R10 in `gemini::read_page`).
    fn ask_page(&self, bytes: &[u8], mime: &str, lang: &str, model: &str, key: &str) -> ApiResult<Vec<PageItem>> {
        let agent = gemini::agent();
        let client = gemini::Client { agent: &agent, base: &self.gemini_base, model, key };
        let may_send = || {
            self.lock().gemini_requests += 1;
            true
        };
        match client.read_page(bytes, mime, lang, &may_send) {
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

    fn saved_page(&self, picture_id: i64, width: u32, height: u32) -> ApiResult<PageOut> {
        let rows = store::placements(&self.lock().db, picture_id)?;
        let boxes = rows.into_iter().map(|p| PageBox { x: p.x, y: p.y, w: p.w, h: p.h, radius: p.radius, font_px: p.font_px, ko: p.ko }).collect();
        Ok(PageOut { width, height, saved: true, boxes, save_error: None })
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

/// The white shape for one balloon: its text, grown towards the middle of
/// the balloon so the source text is hidden even where the box Gemini gave
/// is a little short, and never past the balloon's outline.
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
    PageBox { x: s.x, y: s.y, w: s.w, h: s.h, radius: s.radius, font_px: s.font_px, ko: it.ko.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: [f64; 4], balloon: Option<[f64; 4]>) -> PageItem {
        PageItem { text, balloon, src: String::new(), ko: "안녕".into() }
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

    #[test]
    fn a_text_box_without_its_balloon_is_covered_a_little_past_its_edges() {
        // Text 200..300 x 400..600: 6 px more each side across, 12 px down.
        let s = shape(&item([0.2, 0.2, 0.3, 0.3], None), 1000, 2000);
        assert_eq!((s.x, s.y, s.w, s.h), (194, 388, 112, 224));
    }
}
