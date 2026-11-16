//! A2: frames arrive as 96-px-wide grayscale thumbnails. Two frames that
//! match row for row are still; a frame that matches the previous one at
//! some vertical distance is a scroll by that distance; a frame that matches
//! at no distance starts a page change. Rows at the top or bottom that stay
//! put while the rest scrolls are a fixed band (status bar, toolbar).

pub const THUMB_W: usize = 96;
/// Mean absolute difference per pixel below which two rows are the same.
pub const STILL: f32 = 2.0;
/// Mean absolute difference per pixel below which a shifted frame matches.
pub const MATCH: f32 = 6.0;

/// A capture blocked by the app shows up as an (almost) all-black frame.
pub fn is_blocked(thumb: &[u8]) -> bool {
    thumb.iter().all(|&v| v <= 16)
}

fn row(t: &[u8], r: usize) -> &[u8] {
    &t[r * THUMB_W..(r + 1) * THUMB_W]
}

fn row_diff(a: &[u8], b: &[u8]) -> u32 {
    a.iter().zip(b).map(|(&x, &y)| x.abs_diff(y) as u32).sum()
}

/// Mean difference of the two frames at the same position, rows `from..to`.
pub fn still_diff(prev: &[u8], cur: &[u8], from: usize, to: usize) -> f32 {
    if to <= from {
        return 0.0;
    }
    let sum: u64 = (from..to).map(|r| row_diff(row(prev, r), row(cur, r)) as u64).sum();
    sum as f32 / ((to - from) * THUMB_W) as f32
}

/// Rows at the top and at the bottom that did not change. Bands longer than
/// a quarter of the screen are not taken as fixed.
pub fn fixed_rows(prev: &[u8], cur: &[u8], rows: usize) -> (usize, usize) {
    let same = |r: usize| (row_diff(row(prev, r), row(cur, r)) as f32) <= STILL * THUMB_W as f32;
    let top = (0..rows).take_while(|&r| same(r)).count();
    let bottom = (0..rows).rev().take_while(|&r| same(r)).count();
    let cap = rows / 4;
    (if top <= cap { top } else { 0 }, if bottom <= cap { bottom } else { 0 })
}

/// Mean difference of `cur` against `prev` moved up by `s` rows, within
/// rows `top..end`; `None` once it is over `limit`.
fn shifted_diff(prev: &[u8], cur: &[u8], top: usize, end: usize, s: i32, limit: f32) -> Option<f32> {
    let from = top as i32 + (-s).max(0);
    let to = end as i32 - s.max(0);
    if to <= from {
        return None;
    }
    let n = ((to - from) as usize * THUMB_W / 2) as f32;
    let mut sum = 0u64;
    for r in from..to {
        let (a, b) = (row(cur, r as usize), row(prev, (r + s) as usize));
        sum += a.iter().zip(b).step_by(2).map(|(&x, &y)| x.abs_diff(y) as u64).sum::<u64>();
        if sum as f32 > limit * n {
            return None;
        }
    }
    Some(sum as f32 / n)
}

/// The best whole-row shift and its difference; `zero` lets "not moved" win.
fn search(prev: &[u8], cur: &[u8], rows: usize, top: usize, bottom: usize, zero: bool) -> Option<(i32, f32)> {
    let end = rows.saturating_sub(bottom);
    if end <= top + 4 {
        return None;
    }
    let span = end - top;
    let max_shift = (span - (span * 3 / 10).max(4)) as i32;
    let mut best: Option<(i32, f32)> = None;
    for s in -max_shift..=max_shift {
        if s == 0 && !zero {
            continue;
        }
        let limit = best.map(|b| b.1).unwrap_or(MATCH);
        if let Some(d) = shifted_diff(prev, cur, top, end, s, limit) {
            if best.is_none_or(|b| d < b.1) {
                best = Some((s, d));
            }
        }
    }
    best.filter(|b| b.1 <= MATCH)
}

/// Shift `s` (in thumbnail rows, content moved up by `s`) that best maps the
/// previous frame onto the current one within rows `top..rows-bottom`, if it
/// matches well enough.
pub fn best_shift(prev: &[u8], cur: &[u8], rows: usize, top: usize, bottom: usize) -> Option<i32> {
    search(prev, cur, rows, top, bottom, false).map(|b| b.0)
}

/// [`best_shift`] to a fraction of a row: a real scroll is rarely a whole
/// thumbnail row (11.25 screen px on a 1080-px-wide phone), and rounding
/// every frame adds up. The fraction comes from the differences one row to
/// either side (an equiangular line fit); an exact match stays whole.
pub fn fine_shift(prev: &[u8], cur: &[u8], rows: usize, top: usize, bottom: usize, zero: bool) -> Option<f64> {
    let (s, d0) = search(prev, cur, rows, top, bottom, zero)?;
    let end = rows.saturating_sub(bottom);
    let at = |k: i32| shifted_diff(prev, cur, top, end, k, f32::MAX);
    if d0 < 0.25 {
        return Some(s as f64);
    }
    let (Some(a), Some(b)) = (at(s - 1), at(s + 1)) else { return Some(s as f64) };
    let rise = a.max(b) - d0;
    if rise <= 0.0 {
        return Some(s as f64);
    }
    Some(s as f64 + ((a - b) / (2.0 * rise)).clamp(-0.5, 0.5) as f64)
}

/// A2: the 96-px-wide grayscale thumbnail of a `w` x `h` grayscale screen,
/// `h * 96 / w` rows, each pixel the average of the screen pixels under it.
pub fn thumbnail(gray: &[u8], w: usize, h: usize) -> Vec<u8> {
    let rows = h * THUMB_W / w.max(1);
    let mut out = Vec::with_capacity(rows * THUMB_W);
    for j in 0..rows {
        let (y0, y1) = (j * h / rows, ((j + 1) * h / rows).max(j * h / rows + 1));
        for i in 0..THUMB_W {
            let (x0, x1) = (i * w / THUMB_W, ((i + 1) * w / THUMB_W).max(i * w / THUMB_W + 1));
            let sum: u64 = (y0..y1).map(|y| gray[y * w + x0..y * w + x1].iter().map(|&v| v as u64).sum::<u64>()).sum();
            out.push((sum / ((y1 - y0) * (x1 - x0)) as u64) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(seed: u32, rows: usize, off: usize) -> Vec<u8> {
        (0..rows * THUMB_W)
            .map(|i| {
                let (r, c) = ((i / THUMB_W + off) as u32, (i % THUMB_W) as u32);
                (r.wrapping_mul(2654435761) ^ c.wrapping_mul(40503) ^ seed).wrapping_mul(2246822519) as u8 / 2 + 60
            })
            .collect()
    }

    #[test]
    fn shift_is_found_and_other_page_is_not() {
        let a = page(1, 240, 0);
        assert_eq!(best_shift(&a, &page(1, 240, 12), 240, 0, 0), Some(12));
        assert_eq!(best_shift(&page(1, 240, 30), &page(1, 240, 0), 240, 0, 0), Some(-30));
        assert_eq!(best_shift(&a, &page(2, 240, 0), 240, 0, 0), None);
        assert!(is_blocked(&[0; 96 * 4]) && !is_blocked(&a));
    }

    #[test]
    fn thumbnail_is_96_wide_and_averages() {
        let (w, h) = (960, 2400);
        let gray: Vec<u8> = (0..w * h).map(|i| if (i / w) < h / 2 { 0 } else { 200 }).collect();
        let t = thumbnail(&gray, w, h);
        assert_eq!(t.len(), 96 * 240);
        assert_eq!((t[0], t[96 * 239]), (0, 200));
    }
}
