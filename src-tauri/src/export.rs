//! P11 and picture helpers: decode PNG crops and strips, stitch a picture's
//! strips, and bake each placement in as a white rounded shape holding the
//! Korean text (R20).

use crate::api::{ApiError, ApiResult};
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
        Rgb { w, h, px: vec![255; w as usize * h as usize * 3] }
    }

    fn put(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let i = (y as usize * self.w as usize + x as usize) * 3;
            self.px[i..i + 3].copy_from_slice(&c);
        }
    }

    fn darken(&mut self, x: i64, y: i64, coverage: f32) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let i = (y as usize * self.w as usize + x as usize) * 3;
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

// Decoders can hold both their output and its RGB copy on the phone.
const MAX_PAGE_PIXELS: u64 = 8 * 1024 * 1024;
const BAD_IMAGE: ApiError = ApiError("bad_image");

#[derive(Clone, Copy)]
enum Dimensions {
    Any,
    Page,
    Exact(u32, u32),
}

impl Dimensions {
    fn check(self, width: u32, height: u32) -> ApiResult<()> {
        match self {
            Self::Page if u64::from(width) * u64::from(height) > MAX_PAGE_PIXELS => {
                Err(ApiError("image_dimensions_too_large"))
            }
            Self::Exact(w, h) if (width, height) != (w, h) => Err(BAD_IMAGE),
            _ => Ok(()),
        }
    }
}

pub fn decode_png(bytes: &[u8]) -> Option<Rgb> {
    decode_png_checked(bytes, Dimensions::Any).ok()
}

pub(crate) fn decode_capture_png(bytes: &[u8], width: u32, height: u32) -> Option<Rgb> {
    decode_png_checked(bytes, Dimensions::Exact(width, height)).ok()
}

fn decode_png_checked(bytes: &[u8], dimensions: Dimensions) -> ApiResult<Rgb> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let header = dec.read_header_info().map_err(|_| BAD_IMAGE)?;
    dimensions.check(header.width, header.height)?;
    let mut reader = dec.read_info().map_err(|_| BAD_IMAGE)?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or(BAD_IMAGE)?];
    let info = reader.next_frame(&mut buf).map_err(|_| BAD_IMAGE)?;
    dimensions.check(info.width, info.height)?;
    let ch = info.color_type.samples();
    let px = buf[..info.buffer_size()]
        .chunks(ch)
        .flat_map(|p| match ch {
            1 | 2 => [p[0], p[0], p[0]],
            _ => [p[0], p[1], p[2]],
        })
        .collect();
    Ok(Rgb { w: info.width, h: info.height, px })
}

pub fn encode_png(img: &Rgb) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, img.w, img.h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&img.px).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// WebP pictures are at most 16383 px on a side.
pub const WEBP_MAX: u32 = 16383;

pub fn encode_webp(img: &Rgb) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out).encode(&img.px, img.w, img.h, image_webp::ColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out)
}

/// A lossless WebP (how strips are kept, E5) as RGB.
pub fn decode_webp(bytes: &[u8]) -> Option<Rgb> {
    decode_webp_checked(bytes, Dimensions::Any).ok()
}

fn decode_webp_checked(bytes: &[u8], dimensions: Dimensions) -> ApiResult<Rgb> {
    let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).map_err(|_| BAD_IMAGE)?;
    let (w, h) = d.dimensions();
    dimensions.check(w, h)?;
    let mut buf = vec![0; d.output_buffer_size().ok_or(BAD_IMAGE)?];
    d.read_image(&mut buf).map_err(|_| BAD_IMAGE)?;
    let px = if d.has_alpha() { buf.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect() } else { buf };
    Ok(Rgb { w, h, px })
}

/// A strip file: lossless WebP, PNG as strips were kept before, or a
/// reader page kept as the site sent it (JPEG too).
pub fn decode_strip(bytes: &[u8]) -> Option<Rgb> {
    decode_image(bytes, Dimensions::Any).ok()
}

pub(crate) fn decode_page(bytes: &[u8]) -> ApiResult<Rgb> {
    decode_image(bytes, Dimensions::Page)
}

fn decode_image(bytes: &[u8], dimensions: Dimensions) -> ApiResult<Rgb> {
    match mime(bytes).ok_or(BAD_IMAGE)? {
        "image/webp" => decode_webp_checked(bytes, dimensions),
        "image/jpeg" => decode_jpeg_checked(bytes, dimensions),
        _ => decode_png_checked(bytes, dimensions),
    }
}

/// The picture type of `bytes` by its signature: JPEG, PNG or WebP only.
pub fn mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub fn decode_jpeg(bytes: &[u8]) -> Option<Rgb> {
    decode_jpeg_checked(bytes, Dimensions::Any).ok()
}

fn decode_jpeg_checked(bytes: &[u8], dimensions: Dimensions) -> ApiResult<Rgb> {
    use zune_jpeg::zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    d.decode_headers().map_err(|_| BAD_IMAGE)?;
    let info = d.info().ok_or(BAD_IMAGE)?;
    let (w, h) = (info.width as u32, info.height as u32);
    dimensions.check(w, h)?;
    let px = d.decode().map_err(|_| BAD_IMAGE)?;
    (px.len() == w as usize * h as usize * 3).then_some(Rgb { w, h, px }).ok_or(BAD_IMAGE)
}

/// The `w` x `h` area of `img` at (`x`, `y`), clipped to the picture.
pub fn crop(img: &Rgb, x: u32, y: u32, w: u32, h: u32) -> Rgb {
    let (x1, y1) = ((x + w).min(img.w), (y + h).min(img.h));
    let (x, y) = (x.min(x1), y.min(y1));
    let mut px = Vec::with_capacity(((x1 - x) * (y1 - y) * 3) as usize);
    for row in y..y1 {
        let start = (row as usize * img.w as usize + x as usize) * 3;
        px.extend_from_slice(&img.px[start..start + (x1 - x) as usize * 3]);
    }
    Rgb { w: x1 - x, h: y1 - y, px }
}

/// Rows `top..bottom` of `img`.
pub fn rows(img: &Rgb, top: u32, bottom: u32) -> Rgb {
    let bottom = bottom.min(img.h);
    let row = img.w as usize * 3;
    Rgb { w: img.w, h: bottom - top, px: img.px[top as usize * row..bottom as usize * row].to_vec() }
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
    let inner_w = p.w.saturating_sub(2 * PAD).max(1);
    let lines = layout::lines(&p.ko, size, inner_w);
    let line_h = size as f32 * 1.2;
    let top = p.y as f32 + (p.h as f32 - lines.len() as f32 * line_h) / 2.0;
    let cx = p.x as f32 + p.w as f32 / 2.0;
    for (i, line) in lines.iter().enumerate() {
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

/// Rows rendered at a time, so a long picture never sits in memory whole.
pub const BAND: i64 = 2048;

/// Picture rows `top..bottom` written as one PNG, rendered and encoded band by band.
pub fn write_png(out: impl std::io::Write + 'static, width: u32, top: i64, bottom: i64, strips: &[StripRow], placements: &[PlacementRow]) -> std::io::Result<()> {
    let bottom = bottom.max(top + 1);
    let mut enc = png::Encoder::new(out, width, (bottom - top) as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?.into_stream_writer().map_err(std::io::Error::other)?;
    let mut t = top;
    while t < bottom {
        let b = (t + BAND).min(bottom);
        std::io::Write::write_all(&mut w, &render(width, t, b, strips, placements)?.px)?;
        t = b;
    }
    w.finish().map_err(std::io::Error::other)
}

/// Picture rows `top..bottom`: the strips over them, then the placements baked in.
pub fn render(width: u32, top: i64, bottom: i64, strips: &[StripRow], placements: &[PlacementRow]) -> std::io::Result<Rgb> {
    let height = (bottom - top).max(1) as u32;
    let mut img = Rgb::new(width, height);
    for s in strips.iter().filter(|s| s.y_start < top + height as i64 && s.y_start + s.height as i64 > top) {
        let bytes = std::fs::read(&s.file_path).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{}: {e}", s.file_path)))?;
        // A strip that cannot be read would leave a white gap in the export.
        let strip = decode_strip(&bytes)
            .filter(|p| p.h == s.height)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unreadable strip {}", s.file_path)))?;
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
    for p in placements.iter().filter(|p| p.y < top + height as i64 && p.y + p.h as i64 > top) {
        let mut q = p.clone();
        q.y -= top;
        draw_placement(&mut img, &q);
    }
    Ok(img)
}

/// Creates `dir/<stem>.<ext>`, or `-2`, `-3`, ... if the name is taken. The
/// file is created exclusively, so two exports at once never share a name.
pub fn create_free(dir: &Path, stem: &str, ext: &str) -> std::io::Result<(PathBuf, std::fs::File)> {
    for n in 1.. {
        let p = if n == 1 { dir.join(format!("{stem}.{ext}")) } else { dir.join(format!("{stem}-{n}.{ext}")) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(f) => return Ok((p, f)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!()
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

    #[test]
    fn too_tall_webp_is_refused() {
        let tall = Rgb::new(4, WEBP_MAX + 100);
        assert!(encode_webp(&tall).is_err());
        assert!(encode_webp(&rows(&tall, 0, WEBP_MAX)).is_ok_and(|b| b.starts_with(b"RIFF")));
    }

    #[test]
    fn long_picture_is_written_band_by_band() {
        let dir = std::env::temp_dir().join(format!("rst-band-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut strips = vec![];
        for (i, shade) in [(0i64, 10u8), (1, 200)] {
            let path = dir.join(format!("{i}.webp"));
            std::fs::write(&path, encode_webp(&Rgb { w: 4, h: 3000, px: vec![shade; 4 * 3000 * 3] }).unwrap()).unwrap();
            strips.push(StripRow { y_start: i * 3000, height: 3000, file_path: path.to_string_lossy().into() });
        }
        let out = dir.join("out.png");
        write_png(std::fs::File::create(&out).unwrap(), 4, 0, 6000, &strips, &[]).unwrap();
        let img = decode_png(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!((img.w, img.h), (4, 6000));
        assert_eq!((img.px[0], img.px[2999 * 12], img.px[3000 * 12], img.px[5999 * 12]), (10, 10, 200, 200));
        std::fs::remove_file(&strips[0].file_path).unwrap();
        assert_eq!(write_png(std::io::sink(), 4, 0, 6000, &strips, &[]).unwrap_err().kind(), std::io::ErrorKind::InvalidData);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
