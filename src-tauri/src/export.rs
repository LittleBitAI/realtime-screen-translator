//! P11 and picture helpers: decode PNG crops and strips, stitch a picture's
//! strips, and bake each placement in as a white rounded shape holding the
//! Korean text (R20).

use crate::layout::{self, PAD};
use crate::store::{PlacementRow, StripRow};
use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// An 8-bit RGB picture.
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Rgb {
    pub fn new(w: u32, h: u32) -> Rgb {
        Rgb { w, h, px: vec![255; (w * h * 3) as usize] }
    }

    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let i = ((y as u32 * self.w + x as u32) * 3) as usize;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }

    fn darken(&mut self, x: i64, y: i64, coverage: f32) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let i = ((y as u32 * self.w + x as u32) * 3) as usize;
            let k = 1.0 - coverage.clamp(0.0, 1.0);
            for v in &mut self.px[i..i + 3] {
                *v = (*v as f32 * k) as u8;
            }
        }
    }

    pub fn gray(&self) -> Vec<u8> {
        self.px.chunks(3).map(|p| ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8).collect()
    }
}

pub fn decode_png(bytes: &[u8]) -> Option<Rgb> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let ch = info.color_type.samples();
    let px = buf[..info.buffer_size()]
        .chunks(ch)
        .flat_map(|p| match ch {
            1 | 2 => [p[0], p[0], p[0]],
            _ => [p[0], p[1], p[2]],
        })
        .collect();
    Some(Rgb { w: info.width, h: info.height, px })
}

/// Width and height from a PNG header, without decoding.
pub fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some((be(16), be(20)))
}

pub fn encode_png(img: &Rgb) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.w, img.h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = enc.write_header() {
        let _ = w.write_image_data(&img.px);
    }
    out
}

pub fn encode_webp(img: &Rgb) -> Vec<u8> {
    let mut out = Vec::new();
    let _ = image_webp::WebPEncoder::new(&mut out).encode(&img.px, img.w, img.h, image_webp::ColorType::Rgb8);
    out
}

/// Rows `top..bottom` of `img`, re-encoded as PNG.
pub fn crop_rows(img: &Rgb, top: u32, bottom: u32) -> Vec<u8> {
    let (a, b) = ((top * img.w * 3) as usize, (bottom.min(img.h) * img.w * 3) as usize);
    encode_png(&Rgb { w: img.w, h: bottom.min(img.h) - top, px: img.px[a..b].to_vec() })
}

const FONT_PATHS: [&str; 8] = [
    "C:\\Windows\\Fonts\\malgun.ttf",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/NotoSansKR-Regular.otf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/Library/Fonts/AppleGothic.ttf",
];

/// A font with Hangul from the device (`RST_FONT` overrides the search).
fn font() -> Option<&'static FontVec> {
    static FONT: OnceLock<Option<FontVec>> = OnceLock::new();
    FONT.get_or_init(|| {
        let env = std::env::var("RST_FONT").ok();
        let paths: Vec<&str> = env.iter().map(String::as_str).chain(FONT_PATHS).collect();
        paths.into_iter().find_map(|p| FontVec::try_from_vec_and_index(std::fs::read(p).ok()?, 0).ok())
    })
    .as_ref()
}

fn inside_rounded(x: i64, y: i64, p: &PlacementRow) -> bool {
    let (r, w, h) = (p.radius as i64, p.w as i64, p.h as i64);
    let (dx, dy) = (x - p.x, y - p.y);
    if dx < 0 || dy < 0 || dx >= w || dy >= h {
        return false;
    }
    let cx = if dx < r { r } else if dx >= w - r { w - r - 1 } else { dx };
    let cy = if dy < r { r } else if dy >= h - r { h - r - 1 } else { dy };
    (dx - cx).pow(2) + (dy - cy).pow(2) <= r * r
}

/// White shape and Korean text of placement `p` (picture coordinates).
pub fn draw_placement(img: &mut Rgb, p: &PlacementRow) {
    for y in p.y..p.y + p.h as i64 {
        for x in p.x..p.x + p.w as i64 {
            if inside_rounded(x, y, p) {
                img.put(x, y, [255, 255, 255]);
            }
        }
    }
    let size = p.font_px.max(1) as u32;
    let chars: Vec<char> = p.ko.chars().collect();
    let inner_w = p.w.saturating_sub(2 * PAD).max(1);
    let (per_line, lines) = layout::wrap(chars.len(), size, inner_w);
    let line_h = size as f32 * 1.2;
    let top = p.y as f32 + (p.h as f32 - lines as f32 * line_h) / 2.0;
    let cx = p.x as f32 + p.w as f32 / 2.0;
    for (i, line) in chars.chunks(per_line).enumerate() {
        let line_top = top + i as f32 * line_h + (line_h - size as f32) / 2.0;
        match font() {
            Some(font) => {
                let scale = PxScale::from(size as f32);
                let sf = font.as_scaled(scale);
                let width: f32 = line.iter().map(|&c| sf.h_advance(font.glyph_id(c))).sum();
                let em = sf.ascent() - sf.descent();
                let baseline = line_top + sf.ascent() * size as f32 / em.max(1.0);
                let mut x = cx - width / 2.0;
                for &c in line {
                    let id = font.glyph_id(c);
                    if let Some(g) = font.outline_glyph(id.with_scale_and_position(scale, ab_glyph::point(x, baseline))) {
                        let b = g.px_bounds();
                        g.draw(|gx, gy, cov| img.darken(b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64, cov));
                    }
                    x += sf.h_advance(id);
                }
            }
            None => {
                // No Hangul font on this device: one box per character.
                let mut x = cx - (line.len() as u32 * size) as f32 / 2.0;
                for _ in line {
                    let (x0, y0, s) = (x as i64 + 1, line_top as i64 + 1, size as i64 - 2);
                    for d in 0..s.max(1) {
                        for (px, py) in [(x0 + d, y0), (x0 + d, y0 + s - 1), (x0, y0 + d), (x0 + s - 1, y0 + d)] {
                            img.put(px, py, [0, 0, 0]);
                        }
                    }
                    x += size as f32;
                }
            }
        }
    }
}

/// One picture: its strips stitched from `top`, then its placements baked in.
pub fn render(width: u32, top: i64, bottom: i64, strips: &[StripRow], placements: &[PlacementRow]) -> std::io::Result<Rgb> {
    let height = (bottom - top).max(1) as u32;
    let mut img = Rgb::new(width, height);
    for s in strips {
        let bytes = std::fs::read(&s.file_path)?;
        let Some(strip) = decode_png(&bytes) else { continue };
        let w = strip.w.min(width) as usize;
        for row in 0..strip.h as i64 {
            let y = s.y_start - top + row;
            if y < 0 || y >= height as i64 {
                continue;
            }
            let src = (row as usize * strip.w as usize) * 3;
            let dst = (y as usize * width as usize) * 3;
            img.px[dst..dst + w * 3].copy_from_slice(&strip.px[src..src + w * 3]);
        }
    }
    for p in placements {
        let mut q = p.clone();
        q.y -= top;
        draw_placement(&mut img, &q);
    }
    Ok(img)
}

/// `dir/<stem>.<ext>`, adding `-2`, `-3`, ... if the name is taken.
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_is_white_with_dark_text_and_leaves_the_rest() {
        let mut img = Rgb { w: 200, h: 120, px: vec![90; 200 * 120 * 3] };
        let p = PlacementRow { id: 1, fingerprint: String::new(), x: 40, y: 20, w: 120, h: 80, radius: 12, font_px: 20, ko: "안녕하세요".into() };
        draw_placement(&mut img, &p);
        let at = |x: u32, y: u32| img.px[((y * 200 + x) * 3) as usize];
        assert_eq!(at(100, 22), 255);
        assert_eq!(at(10, 10), 90);
        let dark = (24..96).flat_map(|y| (44..156).map(move |x| (x, y))).filter(|&(x, y)| at(x, y) < 128).count();
        assert!(dark > 0);
    }
}
