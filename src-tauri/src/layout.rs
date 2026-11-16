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
const MAX_FONT: u32 = 40;
const MIN_FONT: u32 = 10;

/// Characters per line and line count for `chars` at `font` in `inner_w`.
pub fn wrap(chars: usize, font: u32, inner_w: u32) -> (usize, usize) {
    let per_line = (inner_w / font.max(1)).max(1) as usize;
    (per_line, chars.max(1).div_ceil(per_line))
}

pub fn text_height(lines: usize, font: u32) -> u32 {
    (lines as u32 * font * 6).div_ceil(5)
}

fn fits(chars: usize, font: u32, inner_w: u32, inner_h: u32) -> bool {
    let (per_line, lines) = wrap(chars, font, inner_w);
    per_line.min(chars) as u32 * font <= inner_w && text_height(lines, font) <= inner_h
}

/// Shape covering the text area (`x`, `y`, `w`, `h`) with `ko` fitted inside.
pub fn fit(x: i64, y: i64, w: u32, h: u32, ko: &str) -> Shape {
    let chars = ko.chars().count();
    let (sw, mut sh) = (w + 2 * PAD, h + 2 * PAD);
    let (inner_w, inner_h) = (w, h);
    let font = (MIN_FONT..=MAX_FONT.min(inner_w)).rev().find(|&f| fits(chars, f, inner_w, inner_h));
    let font = match font {
        Some(f) => f,
        None => {
            // Even the smallest size overflows: grow the shape downwards and upwards.
            let f = MIN_FONT.min(inner_w.max(1));
            let (_, lines) = wrap(chars, f, inner_w);
            let need = text_height(lines, f);
            sh = need + 2 * PAD;
            f
        }
    };
    let grow = (sh - (h + 2 * PAD)) as i64;
    Shape {
        x: x - PAD as i64,
        y: y - PAD as i64 - grow / 2,
        w: sw,
        h: sh,
        radius: (sw.min(sh) / 4).min(24) as u16,
        font_px: font as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_line_in_narrow_balloon_fits() {
        let s = fit(10, 10, 120, 200, &"가".repeat(40));
        assert!(s.x <= 10 && s.y <= 10 && s.w >= 120 && s.h >= 200);
        let (per_line, lines) = wrap(40, s.font_px as u32, s.w);
        assert!(per_line as u32 * s.font_px as u32 <= s.w);
        assert!(text_height(lines, s.font_px as u32) <= s.h);
    }

    #[test]
    fn tiny_balloon_grows_instead_of_overflowing() {
        let s = fit(0, 100, 30, 12, &"가".repeat(30));
        let (_, lines) = wrap(30, s.font_px as u32, s.w - 2 * PAD);
        assert!(text_height(lines, s.font_px as u32) <= s.h - 2 * PAD);
    }
}
