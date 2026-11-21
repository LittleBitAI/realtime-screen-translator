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

    fn lighten(&mut self, x: i64, y: i64, coverage: f32) {
        if x >= 0 && y >= 0 && (x as u32) < self.w && (y as u32) < self.h {
            let i = (y as usize * self.w as usize + x as usize) * 3;
            let k = coverage.clamp(0.0, 1.0);
            for v in &mut self.px[i..i + 3] {
                *v = (*v as f32 + (255.0 - *v as f32) * k) as u8;
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

/// How far round a pixel the tone fill looks for art already known.
const TONE_REACH: i64 = 3;

/// How many passes smooth the tone fill (`tone`), and how far each carries a
/// pixel past its neighbours' mean (over-relaxed): enough to settle across a
/// lettering block, an even tone or gradient between its edges, where a few
/// plain passes left the fill's facets (ch256 p9, "BENEATH FATHER'S...").
const TONE_RELAX: usize = 300;
const TONE_OVER: f32 = 1.9;

/// How far (px) round a letter's strokes the tone fill covers: their soft edge.
const STROKE_RIM: usize = 2;

/// How far (0–255) a pixel's gray is from the art's ground for it to be part
/// of the lettering's halo, its white edging or gray shadow, not the ground:
/// past JPEG noise, short of the soft edge of white edging on a light tone
/// (ch256 p6, 240 on 225) and of the ringing round letters on white paper
/// (ch258 p7, 236–244 on 252: a dotted outline of "YOU MEAN" round the
/// cover).
const OFF_GROUND: f32 = 8.0;

/// Darker than this (0–255), a pixel is part of a letter's strokes.
const DARK_ART: u16 = 128;

/// Ink lighter than this (0–255) is the soft rim between white edging and
/// dark art, not the art.
const RIM_GRAY: u16 = 64;

/// How far (0–255) the relaxed fill may stray from the nearest ground before
/// it goes over to it, wholly at twice this.
const TONE_SPLIT: f32 = 40.0;

/// Pixels the halo may reach past the edging asked for.
const HALO_PAST: i64 = 2;

/// The art round the letters in `rects` (`x0, y0, x1, y1`) carried in over
/// them from their edges inwards, ring by ring, each pixel the mean of the
/// known pixels near it, nearer ones counting more (as Telea's fill): what
/// covers lettering with no balloon, under the Korean's white rim. Only the
/// letters' strokes are covered, not the art between them. Where the art
/// next to them is dark (hair, a dark texture) their white edging, light
/// pixels out to `edging` from them, is taken in too: carried in from it,
/// the fill is a light smudge on the dark. Where it is light it is not:
/// taken in, the fill is carried in from the dark lines it stops at. A PNG
/// opaque on what is covered, at `x, y, w, h`; None for no area.
pub(crate) fn tone(page: &Rgb, rects: &[[i64; 4]], edging: i64) -> Option<layout::Patch> {
    let (w, h) = (page.w as i64, page.h as i64);
    let rs: Vec<[i64; 4]> = rects.iter().map(|r| [r[0].max(0), r[1].max(0), r[2].min(w), r[3].min(h)]).filter(|r| r[0] < r[2] && r[1] < r[3]).collect();
    let b = rs.iter().fold([i64::MAX, i64::MAX, i64::MIN, i64::MIN], |b, r| [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[2]), b[3].max(r[3])]);
    if rs.is_empty() {
        return None;
    }
    // The frame: the rects' bounds, an edging round them, and the art an
    // edging and a reach beyond that.
    let m = 2 * edging + TONE_REACH;
    let f = [(b[0] - m).max(0), (b[1] - m).max(0), (b[2] + m).min(w), (b[3] + m).min(h)];
    let (fw, fh) = ((f[2] - f[0]) as usize, (f[3] - f[1]) as usize);
    let mut known = vec![true; fw * fh];
    for r in &rs {
        for y in r[1]..r[3] {
            let row = (y - f[1]) as usize * fw;
            known[row + (r[0] - f[0]) as usize..row + (r[2] - f[0]) as usize].fill(false);
        }
    }
    let gray = |i: usize| {
        let p = ((f[1] + (i / fw) as i64) * w + f[0] + (i % fw) as i64) as usize * 3;
        page.px[p..p + 3].iter().map(|&v| v as u16).sum::<u16>() / 3
    };
    // The pixels round a pixel, corners too: growth by them is square, as a
    // letter's edging is.
    let round = |i: usize| {
        let (x, y) = ((i % fw) as i64, (i / fw) as i64);
        (-1..=1i64).flat_map(move |dy| (-1..=1i64).map(move |dx| (x + dx, y + dy))).filter(|&(nx, ny)| nx >= 0 && ny >= 0 && nx < fw as i64 && ny < fh as i64).map(|(nx, ny)| ny as usize * fw + nx as usize)
    };
    // Within the rects, only the dark strokes and `STROKE_RIM` round them.
    // The light art between letters and lines stays as drawn: carried in
    // over a whole band, the fill smears the light on one side of a panel's
    // border into the dark on the other (ch258 p17, "OCEAN" under a panel's
    // top edge). A dark mark wholly within them is a letter, covered whole;
    // one running on out of them is art, a speed line through the lettering
    // or one a letter touches, and only its thick strokes within them are
    // covered (ch257 p11, "HAKARI"): its thin line stays drawn, not fogged.
    // Thick strokes themselves lying mostly out of them are art as well: a
    // panel's border a rect beside the lettering reaches (ch257 p11, left
    // of "OHH, NOOO!"), not "LPS" in "ALPS!" joined to a speed line.
    let band: Vec<bool> = known.iter().map(|k| !k).collect();
    let ink: Vec<bool> = (0..fw * fh).map(|i| gray(i) < DARK_ART).collect();
    let mut thick = vec![false; fw * fh];
    for (mark, ..) in crate::balloon::pieces(&crate::balloon::opened(&ink, fw, fh, 1), fw, fh) {
        if 2 * mark.iter().filter(|&&i| band[i]).count() >= mark.len() {
            mark.into_iter().for_each(|i| thick[i] = true);
        }
    }
    let mut letter = vec![false; fw * fh];
    for (mark, ..) in crate::balloon::pieces(&ink, fw, fh) {
        let whole = mark.iter().all(|&i| band[i]);
        mark.into_iter().filter(|&i| band[i] && (whole || thick[i])).for_each(|i| letter[i] = true);
    }
    for _ in 0..STROKE_RIM {
        let was = letter.clone();
        for i in (0..fw * fh).filter(|&i| band[i] && !was[i]) {
            letter[i] = round(i).any(|j| was[j]);
        }
    }
    (0..fw * fh).filter(|&i| band[i] && !letter[i]).for_each(|i| known[i] = true);
    // The art's ground at the known pixels next to the unknown, what the
    // fill is carried in from: the median of the known art within an
    // edging, channel by channel, thin lines through it gone. Carried in
    // from the speed lines themselves the fill was a gray fog over white
    // paper (ch257 p11, "HAKARI"); closed over, white edging on a tone
    // spread white over it (ch256 p4, "COURAGE").
    let ground = |known: &[bool]| -> Vec<[f32; 3]> {
        let k = edging.max(3);
        (0..fw * fh)
            .map(|i| {
                let (x, y) = ((i % fw) as i64, (i / fw) as i64);
                let near = (-TONE_REACH..=TONE_REACH).any(|dy| (-TONE_REACH..=TONE_REACH).any(|dx| {
                    let (nx, ny) = (x + dx, y + dy);
                    nx >= 0 && ny >= 0 && nx < fw as i64 && ny < fh as i64 && !known[ny as usize * fw + nx as usize]
                }));
                if !known[i] || !near {
                    return [255.0; 3];
                }
                let mut v: [Vec<u8>; 3] = [vec![], vec![], vec![]];
                for ny in (y - k).max(0)..(y + k + 1).min(fh as i64) {
                    for nx in (x - k).max(0)..(x + k + 1).min(fw as i64) {
                        if known[ny as usize * fw + nx as usize] {
                            let p = ((f[1] + ny) * w + f[0] + nx) as usize * 3;
                            (0..3).for_each(|n| v[n].push(page.px[p + n]));
                        }
                    }
                }
                v.map(|mut c| {
                    let m = c.len() / 2;
                    *c.select_nth_unstable(m).1 as f32
                })
            })
            .collect()
    };
    // The pixels `!known` filled from the known ones' `src`, ring by ring
    // inwards, then relaxed: ring by ring the fill is faceted, a mosaic of
    // near copies; relaxed to the mean of its four neighbours, it is an even
    // tone between its edges.
    let fill = |known: &[bool], src: &[[f32; 3]], relax: usize| -> Vec<[f32; 3]> {
        let mut known = known.to_vec();
        let inside: Vec<usize> = (0..fw * fh).filter(|&i| !known[i]).collect();
        let mut c: Vec<[f32; 3]> = (0..fw * fh).map(|i| if known[i] { src[i] } else { [255.0; 3] }).collect();
        let mut todo = inside.clone();
        while !todo.is_empty() {
            let ring: Vec<(usize, [f32; 3])> = todo
                .iter()
                .filter_map(|&i| {
                    let (x, y) = ((i % fw) as i64, (i / fw) as i64);
                    let (mut sum, mut wt) = ([0f32; 3], 0f32);
                    for dy in -TONE_REACH..=TONE_REACH {
                        for dx in -TONE_REACH..=TONE_REACH {
                            let (nx, ny) = (x + dx, y + dy);
                            let d2 = (dx * dx + dy * dy) as f32;
                            if nx < 0 || ny < 0 || nx >= fw as i64 || ny >= fh as i64 || d2 == 0.0 || d2 > (TONE_REACH * TONE_REACH) as f32 {
                                continue;
                            }
                            let j = ny as usize * fw + nx as usize;
                            if known[j] {
                                let k = 1.0 / d2;
                                (0..3).for_each(|n| sum[n] += k * c[j][n]);
                                wt += k;
                            }
                        }
                    }
                    // Only pixels next to the known art take a value this ring.
                    let next = round(i).any(|j| known[j]);
                    (next && wt > 0.0).then(|| (i, sum.map(|s| s / wt)))
                })
                .collect();
            if ring.is_empty() {
                break;
            }
            for &(i, v) in &ring {
                c[i] = v;
                known[i] = true;
            }
            todo.retain(|&i| !known[i]);
        }
        for _ in 0..relax {
            for &i in &inside {
                let (x, y) = (i % fw, i / fw);
                let n = [if x > 0 { i - 1 } else { i }, if x + 1 < fw { i + 1 } else { i }, if y > 0 { i - fw } else { i }, if y + 1 < fh { i + fw } else { i }];
                c[i] = [0, 1, 2].map(|k| c[i][k] + TONE_OVER * (n.iter().map(|&j| c[j][k]).sum::<f32>() / 4.0 - c[i][k]));
            }
        }
        c
    };
    // The lettering's halo: the pixels out to `edging` from the cover that
    // stand off the ground there, the ground carried in from beyond them.
    // White edging on a tone or dark art (ch257 p6, "THAT?!"), a gray shadow
    // on white paper (ch257 p11, "GIRL"). White edging on white paper does
    // not show and is left; across a panel's edge it is taken in on the dark
    // side only (ch258 p17, "OCEAN"). Ink only on from ink: a letter's
    // strokes run on into the speed lines it touches (ch257 p11, "PS" in
    // "ALPS!"), but a panel's border past the white edging is art (ch256 p12,
    // "WHAT IS SHE EVEN ON ABOUT?!"). The edging's soft outer rim, gray
    // from `RIM_GRAY` up to ink, goes with it, or it stays a light contour
    // on the dark art (ch257 p8, "AGAIN, DAMN IT!" on hair); ink reached
    // through it is not ink on from ink.
    let mut zone: Vec<bool> = known.clone();
    for _ in 0..edging {
        let was = zone.clone();
        for i in (0..fw * fh).filter(|&i| was[i]) {
            zone[i] = round(i).all(|j| was[j]);
        }
    }
    // Unrelaxed, the ground nearest: across a panel's edge, the white on
    // one side and the dark on the other, not a blend of them.
    let under = fill(&zone, &ground(&zone), 0);
    let off = |i: usize| (gray(i) as f32 - under[i].iter().sum::<f32>() / 3.0).abs() > OFF_GROUND;
    let mut edge: Vec<usize> = (0..fw * fh).filter(|&i| !known[i]).collect();
    // Ink reached from a letter's strokes by ink alone.
    let mut inked: Vec<bool> = (0..fw * fh).map(|i| !known[i] && gray(i) < DARK_ART).collect();
    // A little past `edging`: the edging is lettered by eye, and its last
    // light pixels left were a faint contour round the cover on a gray tone
    // (ch256 p12, "WHAT IS SHE EVEN ON ABOUT?!").
    for _ in 0..if edging > 0 { edging + HALO_PAST } else { 0 } {
        let mut next = vec![];
        for &i in &edge {
            for j in round(i) {
                let on = inked[i] && gray(j) < DARK_ART;
                if known[j] && off(j) && (gray(j) >= RIM_GRAY || on) {
                    known[j] = false;
                    inked[j] = on;
                    next.push(j);
                }
            }
        }
        edge = next;
    }
    let inside = known.iter().map(|k| !k).collect::<Vec<bool>>();
    let src = ground(&known);
    let smooth = fill(&known, &src, TONE_RELAX);
    // Across a hard edge, relaxed, the fill is a gray blur from one side
    // into the other (ch258 p7, "MEAN" touching black hair on white paper):
    // there it goes over to the nearest ground, the edge kept as drawn. Only
    // near the cover's edge, out to an edging and fading over the next: the
    // nearest ground carried deep in was a wedge of an ear's white far up
    // into black hair (ch258 p18, "WHAT INCREDIBLE SPEED—").
    let near = fill(&known, &src, 0);
    let mut depth = vec![usize::MAX; fw * fh];
    let mut q: std::collections::VecDeque<usize> = (0..fw * fh).filter(|&i| known[i]).collect();
    q.iter().for_each(|&i| depth[i] = 0);
    while let Some(i) = q.pop_front() {
        for j in round(i) {
            if depth[j] == usize::MAX {
                depth[j] = depth[i] + 1;
                q.push_back(j);
            }
        }
    }
    let e = edging.max(1) as f32;
    let c: Vec<[f32; 3]> = (0..fw * fh)
        .map(|i| {
            let (s, n) = (smooth[i], near[i]);
            let d = (0..3).map(|k| (s[k] - n[k]).abs()).fold(0f32, f32::max);
            let t = ((d - TONE_SPLIT) / TONE_SPLIT).clamp(0.0, 1.0) * ((2.0 * e - depth[i] as f32) / e).clamp(0.0, 1.0);
            [0, 1, 2].map(|k| s[k] + t * (n[k] - s[k]))
        })
        .collect();
    let (pw, ph) = (fw as u32, fh as u32);
    let mut px = Vec::with_capacity(fw * fh * 4);
    for y in f[1]..f[3] {
        for x in f[0]..f[2] {
            let i = (y - f[1]) as usize * fw + (x - f[0]) as usize;
            let [r, g, bl] = c[i].map(|v| v.round().clamp(0.0, 255.0) as u8);
            px.extend_from_slice(&[r, g, bl, if inside[i] { 255 } else { 0 }]);
        }
    }
    patch_png([f[0], f[1], pw as i64, ph as i64], &px)
}

/// A pixel `v` lifted against a dark or midtone `ground`: the ground is
/// paper (255), what is darker than it too, and what is lighter goes to ink
/// (0 at white), so light letters on it are dark letters on paper. On a
/// black ground it is the negative.
pub(crate) fn lift(v: u8, ground: u8) -> u8 {
    let span = (255 - ground as i32).max(1);
    (255 - ((v as i32 - ground as i32) * 255 / span).clamp(0, 255)) as u8
}

/// [`lift`] turned back: paper is the ground again.
fn unlift(n: u8, ground: u8) -> u8 {
    (ground as i32 + (255 - n as i32) * (255 - ground as i32) / 255) as u8
}

/// [`tone`] for letters lighter than their dark or midtone `ground`: toned
/// on the page lifted against it, where they are dark letters on paper, and
/// the fill turned back.
pub(crate) fn tone_light(page: &Rgb, rects: &[[i64; 4]], edging: i64, ground: u8) -> Option<layout::Patch> {
    let lifted = Rgb { w: page.w, h: page.h, px: page.px.iter().map(|&v| lift(v, ground)).collect() };
    let p = tone(&lifted, rects, edging)?;
    let (_, mut px) = patch_px(&p)?;
    px.chunks_mut(4).for_each(|q| q[..3].iter_mut().for_each(|v| *v = unlift(*v, ground)));
    patch_png(p.at, &px)
}

/// A patch at `at` (`x, y, w, h`) of RGBA pixels `px`, as a PNG.
fn patch_png(at: [i64; 4], px: &[u8]) -> Option<layout::Patch> {
    use base64::Engine;
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, at[2] as u32, at[3] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().ok()?;
    wr.write_image_data(px).ok()?;
    wr.finish().ok()?;
    Some(layout::Patch { at, png: base64::engine::general_purpose::STANDARD.encode(out) })
}

/// The pixels of a patch's PNG (RGBA) and its width.
pub(crate) fn patch_px(p: &layout::Patch) -> Option<(u32, Vec<u8>)> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(&p.png).ok()?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    (info.color_type == png::ColorType::Rgba).then(|| (info.width, buf[..info.buffer_size()].to_vec()))
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

/// Whether `(x, y)` is inside the polygon `poly` (even-odd).
fn inside_outline(poly: &[[i64; 2]], x: f64, y: f64) -> bool {
    // Nonzero winding, as the reader's `clip-path` fills it: an outline made
    // of several parts (a balloon and lettering beside it) joined by a
    // zero-width bridge covers where its parts overlap too.
    let mut winding = 0;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        let (ax, ay, bx, by) = (a[0] as f64, a[1] as f64, b[0] as f64, b[1] as f64);
        let left = (bx - ax) * (y - ay) - (x - ax) * (by - ay);
        if ay <= y && by > y && left > 0.0 {
            winding += 1;
        } else if ay > y && by <= y && left < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}

/// White shape and Korean text of placement `p` (picture coordinates),
/// into `img` whose first row is picture row `top`. The shape is the
/// balloon's outline where the reader drew one, else a rounded rectangle;
/// the Korean is wrapped in the text area the reader used.
pub fn draw_placement(img: &mut Rgb, p: &PlacementRow, top: i64) {
    draw_cover(img, p, top);
    draw_words(img, p, top);
}

/// The white of placement `p`.
fn draw_cover(img: &mut Rgb, p: &PlacementRow, top: i64) {
    // A line the reader keeps without drawing (no lettering found for it).
    if p.w == 0 || p.h == 0 {
        return;
    }
    let cover: Option<layout::Cover> = p.cover.as_deref().and_then(|c| serde_json::from_str(c).ok());
    let outline = cover.as_ref().map_or(&[][..], |c| &c.outline[..]);
    let patch = cover.as_ref().and_then(|c| c.patch.as_ref());
    if let Some((pw, px)) = patch.and_then(patch_px) {
        let at = patch.unwrap().at;
        for (i, q) in px.chunks(4).enumerate().filter(|(_, q)| q[3] == 255) {
            img.put(at[0] + (i as u32 % pw) as i64, at[1] + (i as u32 / pw) as i64 - top, [q[0], q[1], q[2]]);
        }
        // With no outline, the patch is all the cover there is.
        if outline.is_empty() {
            return;
        }
    }
    for y in p.y..p.y + p.h as i64 {
        for x in p.x..p.x + p.w as i64 {
            let on = if outline.is_empty() { inside_rounded(x, y, p) } else { inside_outline(outline, x as f64 + 0.5, y as f64 + 0.5) };
            if on {
                img.put(x, y - top, [255, 255, 255]);
            }
        }
    }
}

/// The Korean of placement `p`, each letter in a white rim of its own: set
/// over the art beside lettering with no balloon, it stays legible.
fn draw_words(img: &mut Rgb, p: &PlacementRow, top: i64) {
    if p.w == 0 || p.h == 0 {
        return;
    }
    let cover: Option<layout::Cover> = p.cover.as_deref().and_then(|c| serde_json::from_str(c).ok());
    let text = cover.as_ref().map_or([p.x + PAD as i64, p.y + PAD as i64, p.w.saturating_sub(2 * PAD) as i64, p.h.saturating_sub(2 * PAD) as i64], |c| c.text);
    match cover.as_ref().filter(|c| !c.parts.is_empty()) {
        // A line lettered in several balloons: each share in its own.
        Some(c) => c.parts.iter().for_each(|part| draw_text(img, &part.ko, part.font_px, part.text, top, true)),
        None => draw_text(img, &p.ko, p.font_px, text, top, cover.is_some()),
    }
}

/// The lines [`draw_text`] sets `ko` in.
fn text_lines(ko: &str, size: u32, width: u32, by_words: bool) -> Vec<Vec<char>> {
    if by_words {
        layout::wrap(ko, size as f64, width as f64).iter().map(|l| l.chars().collect()).collect()
    } else {
        layout::lines(ko, size, width)
    }
}

/// The white rim round each Korean letter, as a share of the letter's size
/// (the reader's own rim is the same).
const HALO: f32 = 0.14;

/// `ko` at `font_px`, centred in text area `x, y, w, h` (page pixels; the
/// image starts at page row `top`). A reader placement (`by_words`) is
/// wrapped as its size was chosen ([`layout::wrap`]), the capture overlay's
/// as [`layout::fit`] sized it.
fn draw_text(img: &mut Rgb, ko: &str, font_px: u16, [tx, ty, tw, th]: [i64; 4], top: i64, by_words: bool) {
    let size = font_px.max(1) as u32;
    let lines = text_lines(ko, size, tw.max(1) as u32, by_words);
    let line_h = size as f32 * 1.2;
    let top = (ty - top) as f32 + (th as f32 - lines.len() as f32 * line_h) / 2.0;
    let cx = tx as f32 + tw as f32 / 2.0;
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
                        // The rim: the letter stamped white a little way off
                        // in eight directions, then the letter itself.
                        let r = (size as f32 * HALO).max(1.0);
                        for k in 0..8 {
                            let a = k as f32 * std::f32::consts::FRAC_PI_4;
                            let (dx, dy) = ((a.cos() * r).round() as i64, (a.sin() * r).round() as i64);
                            g.draw(|gx, gy, cov| img.lighten(b.min.x as i64 + gx as i64 + dx, b.min.y as i64 + gy as i64 + dy, cov));
                        }
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
    // Every white first: no cover drawn later hides another's Korean.
    let here: Vec<&PlacementRow> = placements.iter().filter(|p| p.y < top + height as i64 && p.y + p.h as i64 > top).collect();
    here.iter().for_each(|p| draw_cover(&mut img, p, top));
    here.iter().for_each(|p| draw_words(&mut img, p, top));
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
        let p = PlacementRow { id: 1, fingerprint: String::new(), x: 40, y: 20, w: 120, h: 80, radius: 12, font_px: 20, ko: "안녕하세요".into(), cover: None };
        draw_placement(&mut img, &p, 0);
        let at = |x: u32, y: u32| img.px[((y * 200 + x) * 3) as usize];
        assert_eq!(at(100, 22), 255);
        assert_eq!(at(10, 10), 90);
        let dark = (24..96).flat_map(|y| (44..156).map(move |x| (x, y))).filter(|&(x, y)| at(x, y) < 128).count();
        assert!(dark > 0);
    }

    /// A reader line is drawn in the lines its size was chosen for: short
    /// words set three a line, not one word a line past the text area.
    #[test]
    fn a_reader_line_is_drawn_inside_the_area_its_size_was_chosen_for() {
        let (ko, area) = ("가 나 다 라 마 바", [20, 20, 76, 48]);
        let font = layout::font_for(ko, 76, 48, Some(40.0));
        assert_eq!((font, layout::wrap(ko, font as f64, 76.0).len()), (20, 2));
        assert_eq!(text_lines(ko, font as u32, 76, true).len(), 2);
        let mut img = Rgb { w: 120, h: 120, px: vec![255; 120 * 120 * 3] };
        draw_text(&mut img, ko, font, area, 0, true);
        let rows: Vec<u32> = (0..120).filter(|&y| (0..120).any(|x| img.px[((y * 120 + x) * 3) as usize] < 128)).collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|&y| (20..=68).contains(&y)), "{rows:?}");
    }

    /// The export draws what the reader shows: the balloon's outline (a
    /// staircase, here two steps) rather than its bounding box, in the band
    /// it falls in; a line kept without a shape draws nothing.
    #[test]
    fn placement_follows_its_balloon_outline_in_its_band() {
        let mut img = Rgb { w: 200, h: 120, px: vec![90; 200 * 120 * 3] };
        // Picture rows 100..220: a wide step 120..160, a narrow one 160..200.
        let outline = vec![[160, 120], [160, 160], [130, 160], [130, 200], [70, 200], [70, 160], [40, 160], [40, 120]];
        let cover = serde_json::to_string(&layout::Cover { outline, text: [75, 125, 50, 70], parts: vec![], patch: None, flow_px: None }).unwrap();
        let p = PlacementRow { id: 1, fingerprint: String::new(), x: 40, y: 120, w: 120, h: 80, radius: 0, font_px: 14, ko: "안녕".into(), cover: Some(cover) };
        draw_placement(&mut img, &p, 100);
        let at = |x: u32, y: u32| img.px[((y * 200 + x) * 3) as usize];
        assert_eq!(at(45, 25), 255, "the wide step is white");
        assert_eq!(at(45, 80), 90, "beside the narrow step the art stays");
        assert_eq!(at(100, 90), 255, "the narrow step is white");
        let mut blank = Rgb { w: 200, h: 120, px: vec![90; 200 * 120 * 3] };
        draw_placement(&mut blank, &PlacementRow { w: 0, h: 0, cover: None, ..p }, 0);
        assert!(blank.px.iter().all(|&v| v == 90));
    }

    /// The user, on the phone: the tone carried in over lettering on dark art
    /// was a light smudge, carried in from the letters' white edging (ch258
    /// p17, ch257 p6), and on a gray tone the edging stayed as letter-shaped
    /// white (ch257 p6, "THAT?!"). The edging is taken in, and the fill is
    /// the art's own tone, wherever the art round it is not white paper.
    #[test]
    fn tone_takes_in_white_edging_off_white_paper() {
        let mut page = Rgb { w: 180, h: 40, px: vec![255; 180 * 40 * 3] };
        // White paper, a gray tone, dark art; a black letter on each, the
        // two off the paper edged in white 5 px wide.
        for y in 0..40 {
            for x in 60..180 {
                page.put(x, y, if x < 120 { [200; 3] } else { [30; 3] });
            }
        }
        for x0 in [75, 135] {
            for y in 10..30 {
                for x in x0..x0 + 20 {
                    page.put(x, y, [255; 3]);
                }
            }
        }
        for x0 in [20, 80, 140] {
            for y in 15..25 {
                for x in x0..x0 + 10 {
                    page.put(x, y, [0; 3]);
                }
            }
        }
        let patch = tone(&page, &[[18, 13, 32, 27], [78, 13, 92, 27], [138, 13, 152, 27]], 4).unwrap();
        let (pw, px) = patch_px(&patch).unwrap();
        let at = |x: i64, y: i64| &px[((y - patch.at[1]) as usize * pw as usize + (x - patch.at[0]) as usize) * 4..][..4];
        // On paper: the letter is paper, the paper round it left as it is.
        assert_eq!(at(25, 20), &[255, 255, 255, 255]);
        assert_eq!(at(15, 20)[3], 0);
        // On the tone and the dark: the edging covered, filled with the art.
        for (x, art) in [(76, 200u8), (85, 200), (136, 30), (145, 30)] {
            assert!(at(x, 20)[3] == 255 && at(x, 20)[0].abs_diff(art) < 30, "({x}, 20): {:?}", at(x, 20));
        }
        assert!(at(85, 11)[3] == 255 && at(153, 22)[3] == 255);
    }

    /// The user, on the phone: lettering across a panel's edge, the white
    /// gutter above and dark art below, still left a light smudge (ch258 p17, "OCEAN").
    /// Its edging is taken in on the dark side only.
    #[test]
    fn tone_takes_in_edging_on_the_dark_side_of_a_panel_edge() {
        let mut page = Rgb { w: 100, h: 40, px: vec![255; 100 * 40 * 3] };
        for y in 20..40 {
            for x in 0..100 {
                page.put(x, y, [30, 30, 30]);
            }
        }
        for y in 12..29 {
            for x in 47..64 {
                page.put(x, y, [255, 255, 255]);
            }
        }
        for y in 15..26 {
            for x in 50..61 {
                page.put(x, y, [0, 0, 0]);
            }
        }
        let patch = tone(&page, &[[48, 14, 63, 27]], 4).unwrap();
        let (pw, px) = patch_px(&patch).unwrap();
        let at = |x: i64, y: i64| &px[((y - patch.at[1]) as usize * pw as usize + (x - patch.at[0]) as usize) * 4..][..4];
        assert!(at(55, 27)[3] == 255 && at(55, 27)[0] < 100, "{:?}", at(55, 27));
        assert_eq!(at(55, 12)[3], 0);
    }

    /// The user, on the phone: the fill looked like a mosaic, facets of near
    /// copies ring by ring. Over a smooth gradient it is smooth too.
    #[test]
    fn tone_is_smooth_not_a_mosaic() {
        let mut page = Rgb { w: 80, h: 60, px: vec![0; 80 * 60 * 3] };
        for y in 0..60 {
            for x in 0..80 {
                let v = (60 + 2 * x + y) as u8;
                page.put(x, y, if (25..45).contains(&x) && (20..40).contains(&y) { [0; 3] } else { [v; 3] });
            }
        }
        let patch = tone(&page, &[[23, 18, 47, 42]], 0).unwrap();
        let (pw, px) = patch_px(&patch).unwrap();
        let at = |x: i64, y: i64| px[((y - patch.at[1]) as usize * pw as usize + (x - patch.at[0]) as usize) * 4] as i32;
        let jump = (25..44).flat_map(|x| (20..39).map(move |y| (x, y))).map(|(x, y)| (at(x + 1, y) - at(x, y)).abs().max((at(x, y + 1) - at(x, y)).abs())).max().unwrap();
        assert!(jump <= 4, "largest step between neighbours {jump}");
    }

    /// ch257 p11: a speed line through lettering was fogged white over its
    /// cover. A letter touching it is covered whole; the line, thin, stays.
    #[test]
    fn tone_covers_a_letter_but_not_the_speed_line_through_it() {
        let mut page = Rgb { w: 100, h: 40, px: vec![255; 100 * 40 * 3] };
        for x in 0..100 {
            page.put(x, 20, [0; 3]);
        }
        for y in 15..25 {
            for x in 40..50 {
                page.put(x, y, [0; 3]);
            }
        }
        let patch = tone(&page, &[[30, 13, 60, 27]], 0).unwrap();
        let (pw, px) = patch_px(&patch).unwrap();
        let at = |x: i64, y: i64| &px[((y - patch.at[1]) as usize * pw as usize + (x - patch.at[0]) as usize) * 4..][..4];
        assert!(at(45, 18)[3] == 255 && at(45, 18)[0] > 200, "{:?}", at(45, 18));
        assert_eq!(at(33, 20)[3], 0);
    }

    /// ch257 p11, left of "OHH, NOOO!": a speck's rect reached the panel's
    /// 5 px border, and the border was wiped there with the speck.
    #[test]
    fn tone_leaves_a_panel_border_its_rect_reaches() {
        let mut page = Rgb { w: 60, h: 100, px: vec![255; 60 * 100 * 3] };
        for y in 0..100 {
            for x in 10..15 {
                page.put(x, y, [0; 3]);
            }
        }
        for y in 45..55 {
            for x in 20..24 {
                page.put(x, y, [0; 3]);
            }
        }
        let patch = tone(&page, &[[8, 40, 30, 60]], 10).unwrap();
        let (pw, px) = patch_px(&patch).unwrap();
        let at = |x: i64, y: i64| &px[((y - patch.at[1]) as usize * pw as usize + (x - patch.at[0]) as usize) * 4..][..4];
        assert!(at(22, 50)[3] == 255 && at(22, 50)[0] > 200, "{:?}", at(22, 50));
        assert_eq!(at(12, 50)[3], 0);
    }

    /// ch258 p18, "WHAT INCREDIBLE SPEED—": lettering edged in white on
    /// black hair, an ear's white under it; the ear's white was carried far
    /// up into the cover, a white wedge in the hair. Deep in, it is the
    /// blend of all round it; at the edge, the ear's edge as drawn.
    #[test]
    fn tone_keeps_a_light_edge_from_running_deep_into_dark() {
        let mut page = Rgb { w: 140, h: 150, px: vec![20; 140 * 150 * 3] };
        for (x0, y0, x1, y1) in [(23, 13, 108, 107), (45, 107, 95, 150)] {
            for y in y0..y1 {
                for x in x0..x1 {
                    page.put(x, y, [255; 3]);
                }
            }
        }
        for y in 18..102 {
            for x in 28..103 {
                page.put(x, y, [0; 3]);
            }
        }
        let patch = tone(&page, &[[26, 16, 105, 104]], 8).unwrap();
        let (pw, px) = patch_px(&patch).unwrap();
        let at = |x: i64, y: i64| &px[((y - patch.at[1]) as usize * pw as usize + (x - patch.at[0]) as usize) * 4..][..4];
        assert!(at(70, 75)[3] == 255 && at(70, 75)[0] < 180, "deep in: {:?}", at(70, 75));
        assert!(at(70, 101)[3] == 255 && at(70, 101)[0] > 200, "at the ear: {:?}", at(70, 101));
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
