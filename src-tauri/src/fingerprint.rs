//! A3: a crop's fingerprint is a 64-bit difference hash of the crop shrunk
//! to 9x8 grayscale. Same balloon = at most 6 bits apart and aspect ratios
//! within 10 %.

/// Hash of a grayscale picture (`gray.len() == w * h`).
pub fn dhash(gray: &[u8], w: usize, h: usize) -> u64 {
    let cells = shrink(gray, w, h, 9, 8);
    let mut hash = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            hash = hash << 1 | (cells[y * 9 + x] > cells[y * 9 + x + 1]) as u64;
        }
    }
    hash
}

/// Box-average `gray` (w x h) down to `tw` x `th`.
fn shrink(gray: &[u8], w: usize, h: usize, tw: usize, th: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(tw * th);
    for j in 0..th {
        let (y0, y1) = (j * h / th, ((j + 1) * h / th).max(j * h / th + 1).min(h));
        for i in 0..tw {
            let (x0, x1) = (i * w / tw, ((i + 1) * w / tw).max(i * w / tw + 1).min(w));
            let mut sum = 0u64;
            for y in y0..y1 {
                sum += gray[y * w + x0..y * w + x1].iter().map(|&v| v as u64).sum::<u64>();
            }
            out.push(sum as f32 / ((y1 - y0) * (x1 - x0)).max(1) as f32);
        }
    }
    out
}

pub const MAX_BITS: u32 = 6;
pub const MAX_ASPECT_DIFF: f32 = 0.10;

pub fn same_balloon(a: u64, aspect_a: f32, b: u64, aspect_b: f32) -> bool {
    let aspect_close = (aspect_a - aspect_b).abs() <= MAX_ASPECT_DIFF * aspect_a.max(aspect_b);
    (a ^ b).count_ones() <= MAX_BITS && aspect_close
}

pub fn to_hex(fp: u64) -> String {
    format!("{fp:016x}")
}

/// Stored `Translation.fingerprint`: the hash plus the aspect ratio in
/// thousandths. Two crops with the same hash but different shapes are
/// different balloons, so they need different keys.
pub fn key(fp: u64, aspect: f32) -> String {
    format!("{fp:016x}-{}", (aspect * 1000.0).round() as u32)
}

/// The hash part of a stored fingerprint.
pub fn from_hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s.get(..16)?, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifted_gradient_stays_close_and_flipped_one_does_not() {
        let (w, h) = (120, 80);
        let img: Vec<u8> = (0..w * h).map(|i| ((i % w) * 2 + (i / w)) as u8).collect();
        let shifted: Vec<u8> = (0..w * h).map(|i| ((i % w + 2) * 2 + (i / w)) as u8).collect();
        let flipped: Vec<u8> = (0..w * h).map(|i| ((w - i % w) * 2) as u8).collect();
        assert!(same_balloon(dhash(&img, w, h), 1.5, dhash(&shifted, w, h), 1.5));
        assert!(!same_balloon(dhash(&img, w, h), 1.5, dhash(&flipped, w, h), 1.5));
    }

    #[test]
    fn same_hash_with_other_shape_gets_its_own_key() {
        assert_ne!(key(0, 0.6), key(0, 2.4));
        assert_eq!(from_hex(&key(0xabc, 0.6)), Some(0xabc));
    }
}
