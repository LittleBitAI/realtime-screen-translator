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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line of `ko` fits inside the shape's padding.
    fn inside(s: &Shape, ko: &str) -> bool {
        let (font, inner_w) = (s.font_px as u32, s.w - 2 * PAD);
        let l = lines(ko, font, inner_w);
        l.iter().all(|l| l.len() as u32 * font <= inner_w) && text_height(l.len(), font) <= s.h - 2 * PAD
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
