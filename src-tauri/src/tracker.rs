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

/// A capture blocked by the app shows up as a black frame with only what the
/// system draws over it: the status bar and the navigation bar or taskbar,
/// in [`EDGE_BAND`] of the rows at the top and at the bottom. (This app's
/// own button is blacked out by the capture loop, `pipeline::step`.)
/// Anything else lit, beyond a few stray pixels (a cursor), is a picture:
/// a dark page with one white balloon on it is not blocked.
pub fn is_blocked(thumb: &[u8]) -> bool {
    let rows = thumb.len() / THUMB_W;
    let band = (rows * EDGE_BAND).div_ceil(100);
    let middle = if rows > 2 * band { &thumb[band * THUMB_W..(rows - band) * THUMB_W] } else { thumb };
    let lit = middle.iter().filter(|&&v| v > 16).count();
    // ponytail: a page that is black but for text in its top or bottom band reads as blocked.
    rows > 0 && lit * 1000 <= middle.len() * 2
}

/// Rows (percent of the screen) at the top and at the bottom where the
/// system draws over every app: status bar, navigation bar, taskbar.
pub const EDGE_BAND: usize = 8;

/// Thumbnail pixels left out of a comparison, one per pixel (empty: none).
/// On Android the capture holds our own overlay: a translation that appears
/// or moves is not the screen changing.
pub type Mask<'a> = &'a [bool];

fn row<T>(t: &[T], r: usize) -> &[T] {
    &t[r * THUMB_W..(r + 1) * THUMB_W]
}

/// Sum of differences of `cur` row `rc` against `prev` row `rp`, every
/// `step`-th pixel, and how many pixels were compared: a pixel masked in
/// either row is left out.
fn row_cmp(prev: &[u8], cur: &[u8], mask: Mask, rp: usize, rc: usize, step: usize) -> (u64, u64) {
    let (a, b) = (row(cur, rc), row(prev, rp));
    if mask.is_empty() {
        let sum = a.iter().zip(b).step_by(step).map(|(&x, &y)| x.abs_diff(y) as u64).sum();
        return (sum, THUMB_W.div_ceil(step) as u64);
    }
    let (ma, mb) = (row(mask, rc), row(mask, rp));
    let (mut sum, mut n) = (0, 0);
    for i in (0..THUMB_W).step_by(step) {
        if !ma[i] && !mb[i] {
            sum += a[i].abs_diff(b[i]) as u64;
            n += 1;
        }
    }
    (sum, n)
}

/// Mean difference of the two frames at the same position, rows `from..to`.
pub fn still_diff(prev: &[u8], cur: &[u8], mask: Mask, from: usize, to: usize) -> f32 {
    let (sum, n) = (from..to).map(|r| row_cmp(prev, cur, mask, r, r, 1)).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    if n == 0 {
        return 0.0;
    }
    sum as f32 / n as f32
}

/// Rows at the top and at the bottom that did not change. Bands longer than
/// a quarter of the screen are not taken as fixed, nor rows under the mask.
pub fn fixed_rows(prev: &[u8], cur: &[u8], mask: Mask, rows: usize) -> (usize, usize) {
    let same = |r: usize| {
        let (sum, n) = row_cmp(prev, cur, mask, r, r, 1);
        n == THUMB_W as u64 && sum as f32 <= STILL * THUMB_W as f32
    };
    let top = (0..rows).take_while(|&r| same(r)).count();
    let bottom = (0..rows).rev().take_while(|&r| same(r)).count();
    let cap = rows / 4;
    (if top <= cap { top } else { 0 }, if bottom <= cap { bottom } else { 0 })
}

/// Mean difference of `cur` against `prev` moved up by `s` rows, within
/// rows `top..end`; `None` once it is over `limit`, or when the mask leaves
/// too little to judge by.
fn shifted_diff(prev: &[u8], cur: &[u8], mask: Mask, top: usize, end: usize, s: i32, limit: f32) -> Option<f32> {
    let from = top as i32 + (-s).max(0);
    let to = end as i32 - s.max(0);
    if to <= from {
        return None;
    }
    // At most this many pixels are compared, so the mean is at least sum / most.
    let most = ((to - from) as usize * THUMB_W.div_ceil(2)) as f32;
    let (mut sum, mut n) = (0u64, 0u64);
    for r in from..to {
        let (d, k) = row_cmp(prev, cur, mask, (r + s) as usize, r as usize, 2);
        sum += d;
        n += k;
        if sum as f32 > limit * most {
            return None;
        }
    }
    if (n as f32) < most / 4.0 {
        return None;
    }
    Some(sum as f32 / n as f32)
}

/// Detail (summed steps between neighbouring pixels) an overlap needs before
/// it can tell one shift from another: flat rows (a white gutter, a black
/// band) match at any distance. As much as 48 steps of 16.
const MIN_DETAIL: u64 = 16 * THUMB_W as u64 / 2;

/// Per row of `t`, as running sums: the steps to the right neighbour (`h`)
/// and to the row above (`v`), leaving out masked pixels.
fn detail_sums(t: &[u8], mask: Mask, rows: usize) -> (Vec<u64>, Vec<u64>) {
    let free = |i: usize| mask.is_empty() || !mask[i];
    let (mut h, mut v) = (vec![0u64; rows + 1], vec![0u64; rows + 1]);
    for r in 0..rows {
        let i = r * THUMB_W;
        let hr: u64 = (i..i + THUMB_W - 1).filter(|&k| free(k) && free(k + 1)).map(|k| t[k].abs_diff(t[k + 1]) as u64).sum();
        let vr: u64 = if r == 0 { 0 } else { (i..i + THUMB_W).filter(|&k| free(k) && free(k - THUMB_W)).map(|k| t[k].abs_diff(t[k - THUMB_W]) as u64).sum() };
        h[r + 1] = h[r] + hr;
        v[r + 1] = v[r] + vr;
    }
    (h, v)
}

/// The best whole-row shift and its difference; `zero` lets "not moved" win.
/// A shift counts only if the rows it lays over each other carry detail and
/// the match is closer than that detail: a flat margin that agrees cannot
/// outweigh the drawing that does not (a page turn is not a scroll).
fn search(prev: &[u8], cur: &[u8], mask: Mask, rows: usize, top: usize, bottom: usize, zero: bool) -> Option<(i32, f32)> {
    let end = rows.saturating_sub(bottom);
    if end <= top + 4 {
        return None;
    }
    let (h, v) = detail_sums(cur, mask, rows);
    // Mean detail per pixel of the overlap at shift `s`, if there is enough.
    let detail = |s: i32| {
        let from = (top as i32 + (-s).max(0)) as usize;
        let to = (end as i32 - s.max(0)) as usize;
        let sum = (h[to] - h[from]) + (v[to] - v[from + 1]);
        (sum >= MIN_DETAIL).then(|| sum as f32 / ((to - from) * THUMB_W) as f32)
    };
    let span = end - top;
    // A fast fling moves most of a screen between frames: shifts that leave a
    // tenth of it in common are still tried (the detail check above keeps a
    // thin overlap from matching on a flat margin).
    let max_shift = (span - (span / 10).max(8).min(span)) as i32;
    let mut best: Option<(i32, f32)> = None;
    for s in -max_shift..=max_shift {
        if s == 0 && !zero {
            continue;
        }
        let mut limit = best.map(|b| b.1).unwrap_or(MATCH);
        if s != 0 {
            let Some(g) = detail(s) else { continue };
            limit = limit.min(g);
        }
        if let Some(d) = shifted_diff(prev, cur, mask, top, end, s, limit).filter(|&d| d <= limit) {
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
    search(prev, cur, &[], rows, top, bottom, false).map(|b| b.0)
}

/// [`best_shift`] to a fraction of a row, leaving out `mask`: a real scroll
/// is rarely a whole thumbnail row (11.25 screen px on a 1080-px-wide
/// phone), and rounding every frame adds up. The fraction comes from the
/// differences one row to either side (an equiangular line fit); an exact
/// match stays whole.
pub fn fine_shift(prev: &[u8], cur: &[u8], mask: Mask, rows: usize, top: usize, bottom: usize, zero: bool) -> Option<f64> {
    let (s, d0) = search(prev, cur, mask, rows, top, bottom, zero)?;
    let end = rows.saturating_sub(bottom);
    let at = |k: i32| shifted_diff(prev, cur, mask, top, end, k, f32::MAX);
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
