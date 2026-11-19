//! R28: the white shape over a balloon and the font size that keeps the
//! Korean inside it. Hangul is taken as one em wide and lines as 1.2 em apart.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
    pub radius: u16,
    pub font_px: u16,
}

pub const PAD: u32 = 4;
/// The largest corner radius whose rounding still covers a point PAD in from
/// both edges, so the source text under the shape is hidden to its corners:
/// 2·(r − PAD)² ≤ r² holds up to r = PAD·(2 + √2) ≈ 13.6.
pub const MAX_RADIUS: u32 = 13;
const MAX_FONT: u32 = 40;
const MIN_FONT: u32 = 10;

/// The lines `ko` is drawn in at `font` in `inner_w`: its own line breaks
/// first (the overlay's text layout keeps them too), then each part wrapped.
pub fn lines(ko: &str, font: u32, inner_w: u32) -> Vec<Vec<char>> {
    let per_line = (inner_w / font.max(1)).max(1) as usize;
    let mut out = vec![];
    for part in ko.trim_end_matches(['\n', '\r']).split('\n') {
        let chars: Vec<char> = part.trim_end_matches('\r').chars().collect();
        if chars.is_empty() {
            out.push(vec![]);
        }
        out.extend(chars.chunks(per_line).map(<[char]>::to_vec));
    }
    out
}

pub fn text_height(lines: usize, font: u32) -> u32 {
    (lines as u32 * font * 6).div_ceil(5)
}

fn fits(ko: &str, font: u32, inner_w: u32, inner_h: u32) -> bool {
    let lines = lines(ko, font, inner_w);
    lines.iter().all(|l| l.len() as u32 * font <= inner_w) && text_height(lines.len(), font) <= inner_h
}

/// Shape covering the text area (`x`, `y`, `w`, `h`) with `ko` fitted inside.
pub fn fit(x: i64, y: i64, w: u32, h: u32, ko: &str) -> Shape {
    // A text area narrower than one glyph of the smallest size is widened to it.
    let inner_w = w.max(MIN_FONT);
    let inner_h = h;
    let (sw, mut sh) = (inner_w + 2 * PAD, h + 2 * PAD);
    let font = (MIN_FONT..=MAX_FONT.min(inner_w)).rev().find(|&f| fits(ko, f, inner_w, inner_h));
    let font = match font {
        Some(f) => f,
        None => {
            // Even the smallest size overflows: grow the shape downwards and upwards.
            sh = sh.max(text_height(lines(ko, MIN_FONT, inner_w).len(), MIN_FONT) + 2 * PAD);
            MIN_FONT
        }
    };
    let grow = (sh - (h + 2 * PAD)) as i64;
    let widen = (inner_w - w) as i64;
    Shape {
        x: x - PAD as i64 - widen / 2,
        y: y - PAD as i64 - grow / 2,
        w: sw,
        h: sh,
        radius: (sw.min(sh) / 4).min(MAX_RADIUS) as u16,
        font_px: font as u16,
    }
}

/// [`fit`], kept on `area` (x0, y0, x1, y1): the screen the text area was
/// seen on. A shape grown for a long translation, or padded past the edge,
/// is moved back onto it whole, so no line of the Korean starts off screen
/// or off the saved picture. Moved, it may keep less than [`PAD`] on a side
/// (none at the screen's edge): its corners are rounded only as far as they
/// still cover the text area's corners.
pub fn fit_within(x: i64, y: i64, w: u32, h: u32, ko: &str, area: (i64, i64, i64, i64)) -> Shape {
    let mut s = fit(x, y, w, h, ko);
    s.y = s.y.min(area.3 - s.h as i64).max(area.1);
    s.x = s.x.min(area.2 - s.w as i64).max(area.0);
    // What the shape keeps beyond the text area on each side.
    let (left, top) = (x - s.x, y - s.y);
    let (right, bottom) = (s.x + s.w as i64 - (x + w as i64), s.y + s.h as i64 - (y + h as i64));
    while s.radius > 0 && ![(left, top), (right, top), (left, bottom), (right, bottom)].into_iter().all(|(a, b)| corner_covers(s.radius as i64, a, b)) {
        s.radius -= 1;
    }
    s
}

/// A reader shape that follows its balloon: its outline and where the Korean
/// goes (`x, y, w, h`), in picture pixels. Kept with its placement as JSON.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cover {
    pub outline: Vec<[i64; 2]>,
    pub text: [i64; 4],
    /// A line Gemini gave for several balloons, set a share in each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<Part>,
    /// Lettering on the art with no balloon: the art's own tone painted over
    /// its letters, in place of white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<Patch>,
}

/// The art's tone over lettering with no balloon: an RGBA PNG (base64)
/// covering `at` = `x, y, w, h` (picture pixels), opaque over the letters
/// and clear elsewhere.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Patch {
    pub at: [i64; 4],
    pub png: String,
}

/// One balloon's share of a line lettered in several: where it goes
/// (`x, y, w, h`), its size and its Korean. Drawn in place of the cover's
/// one text area.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Part {
    pub text: [i64; 4],
    pub font_px: u16,
    pub ko: String,
}

/// `ko` cut at sentence ends into `shares.len()` runs of whole sentences,
/// in order, each as near its share of the letters as the sentence ends
/// allow; none when it has fewer sentences than shares
/// ("수영부 부장이야." / "3학년 5반, 누레미 스이무.").
pub fn split(ko: &str, shares: &[f64]) -> Option<Vec<String>> {
    let n = shares.len();
    let ends = |c: char| matches!(c, '.' | '!' | '?' | '…' | '~' | '♪' | '♥' | '—' | '"' | '\'' | '”' | '’' | ')' | ']');
    // Sentences: a run ending in a sentence mark and then a space.
    let chars: Vec<char> = ko.trim().chars().collect();
    let mut sentences: Vec<String> = vec![];
    let mut start = 0;
    for i in 1..chars.len() {
        if chars[i].is_whitespace() && ends(chars[i - 1]) && i > start {
            sentences.push(chars[start..i].iter().collect::<String>().trim().to_string());
            start = i;
        }
    }
    sentences.push(chars[start.min(chars.len())..].iter().collect::<String>().trim().to_string());
    sentences.retain(|s| !s.is_empty());
    if n < 2 || sentences.len() < n {
        return None;
    }
    // Cut i ends sentence `cut[i]`: the first cut nearest its share, each
    // later one past it, leaving a sentence for each share after it.
    let size = |s: &String| s.chars().filter(|c| !c.is_whitespace()).count() as f64;
    let total: f64 = sentences.iter().map(size).sum::<f64>().max(1.0);
    let whole: f64 = shares.iter().sum::<f64>().max(f64::MIN_POSITIVE);
    let mut upto = vec![0.0];
    for s in &sentences {
        upto.push(upto.last().unwrap() + size(s) / total);
    }
    let (mut cuts, mut want) = (vec![0usize], 0.0);
    for k in 0..n - 1 {
        want += shares[k] / whole;
        let lo = cuts[k] + 1;
        let hi = sentences.len() - (n - 1 - k);
        let best = (lo..=hi).min_by(|&a, &b| (upto[a] - want).abs().total_cmp(&(upto[b] - want).abs())).unwrap();
        cuts.push(best);
    }
    cuts.push(sentences.len());
    Some(cuts.windows(2).map(|w| sentences[w[0]..w[1]].join(" ")).collect())
}

/// Korean font size per pixel of the original letters' height: Hangul at
/// this size reads about as large as the lettering it replaces.
pub const KO_SCALE: f64 = 1.15;
/// Without lettering to measure, the size is the largest that fits, up to this.
const FIT_MAX: u32 = 40;
/// CSS `line-height` of the reader's text.
pub const LINE: f64 = 1.2;

/// Width of a character in em, as Korean fonts set it (wide for Hangul and
/// Han, narrow for Latin and spaces), a little generous.
fn em(c: char) -> f64 {
    match c {
        ' ' => 0.3,
        c if c.is_ascii() => 0.62,
        '…' | '—' | '~' | '!' | '?' | '.' | ',' => 0.62,
        _ => 1.0,
    }
}

/// How many lines `ko` takes at `font` px in `width` px, wrapped at spaces
/// as the reader wraps it (`word-break: keep-all`), a word too long for a
/// line broken anywhere.
pub fn wrapped_lines(ko: &str, font: f64, width: f64) -> usize {
    let mut lines = 0;
    for part in ko.trim().split('\n') {
        let mut used = 0.0;
        lines += 1;
        for (i, word) in part.split(' ').enumerate() {
            let w: f64 = word.chars().map(|c| em(c) * font).sum();
            let space = if i == 0 || used == 0.0 { 0.0 } else { em(' ') * font };
            if used > 0.0 && used + space + w > width {
                lines += 1;
                used = 0.0;
            }
            if w > width {
                // Broken across lines like `overflow-wrap: anywhere`.
                for c in word.chars() {
                    let cw = em(c) * font;
                    if used > 0.0 && used + cw > width {
                        lines += 1;
                        used = 0.0;
                    }
                    used += cw;
                }
            } else {
                used += if used == 0.0 { w } else { space + w };
            }
        }
    }
    lines
}

/// The Korean's size in a `w` by `h` text area: the original lettering's
/// (`glyph`, its letter height) scaled by [`KO_SCALE`], so a short reply
/// keeps the space around it, and smaller only as far as the whole text
/// needs to fit. The reader steps it down further if its own lines overflow.
pub fn font_for(ko: &str, w: u32, h: u32, glyph: Option<f64>) -> u16 {
    let fits = |f: u32| wrapped_lines(ko, f as f64, w as f64) as f64 * f as f64 * LINE <= h as f64;
    let target = glyph.map_or(FIT_MAX, |g| (g * KO_SCALE).round().max(1.0) as u32);
    (1..=target.min(h.max(1))).rev().find(|&f| fits(f)).unwrap_or(1) as u16
}

/// Whether a corner rounded by `r` covers the pixel `a` in from its side and
/// `b` in from its top or bottom (as `export::inside_rounded` draws it).
fn corner_covers(r: i64, a: i64, b: i64) -> bool {
    a >= r || b >= r || (r - a).pow(2) + (r - b).pow(2) <= r * r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line of `ko` fits inside the shape's padding.
    fn inside(s: &Shape, ko: &str) -> bool {
        let (font, inner_w) = (s.font_px as u32, s.w - 2 * PAD);
        let l = lines(ko, font, inner_w);
        l.iter().all(|l| l.len() as u32 * font <= inner_w) && text_height(l.len(), font) <= s.h - 2 * PAD
    }

    /// A short reply keeps the original lettering's size (and the room
    /// around it); a long line in the same balloon is set smaller, as far
    /// as it needs to fit.
    #[test]
    fn korean_takes_the_lettering_size_until_it_must_fit() {
        assert_eq!(font_for("응!", 200, 200, Some(20.0)), 23);
        let long = "그러니까 우리 집 구독 스트리밍 서비스에는 그 영화가 없어서 폰으로도 볼 수가 없다는 거야";
        let f = font_for(long, 100, 120, Some(20.0));
        assert!(f < 23 && wrapped_lines(long, f as f64, 100.0) as f64 * f as f64 * LINE <= 120.0, "{f}");
    }

    /// A line Gemini gave for two balloons is cut at a sentence end, nearest
    /// the balloons' shares; one sentence for two balloons is not cut.
    #[test]
    fn a_line_for_several_balloons_is_cut_at_sentence_ends() {
        let two = "수영부 부장이야. 3학년 5반, 누레미 스이무.";
        assert_eq!(split(two, &[1.0, 1.0]).unwrap(), ["수영부 부장이야.", "3학년 5반, 누레미 스이무."]);
        let three = "아. 그래! 그럼 내일 다시 오자... 꼭이야?";
        assert_eq!(split(three, &[3.0, 1.0]).unwrap(), ["아. 그래! 그럼 내일 다시 오자...", "꼭이야?"]);
        assert_eq!(split(three, &[1.0, 1.0, 1.0]).unwrap(), ["아. 그래!", "그럼 내일 다시 오자...", "꼭이야?"]);
        assert_eq!(split("그 모든 것, 아니 그 이상으로, 렌타로 군은 언제나 내게 그런 존재였어.", &[1.0, 1.0]), None);
        assert_eq!(split(two, &[1.0]), None);
    }

    #[test]
    fn long_line_in_narrow_balloon_fits() {
        let ko = "가".repeat(40);
        let s = fit(10, 10, 120, 200, &ko);
        assert!(s.x <= 10 && s.y <= 10 && s.w >= 120 && s.h >= 200);
        assert!(inside(&s, &ko));
    }

    #[test]
    fn tiny_balloon_grows_instead_of_overflowing() {
        let ko = "가".repeat(30);
        assert!(inside(&fit(0, 100, 30, 12, &ko), &ko));
    }

    #[test]
    fn sliver_crop_is_widened_and_stays_on_its_balloon() {
        let s = fit(0, 100, 8, 100, "가나");
        assert!(s.w >= MIN_FONT + 2 * PAD && s.h >= 100 + 2 * PAD);
        assert!(s.x <= 0 && s.x + s.w as i64 >= 8 && s.y <= 100 && s.y + s.h as i64 >= 200);
        assert!(inside(&s, "가나"));
    }
}
