//! R25: the light local model that finds the text areas of balloons.
//!
//! The model is the Apache-2.0 "comic-text-and-bubble-detector" (RT-DETR-v2,
//! 11 MB int8), fetched once into the app data folder by [`crate::models`]
//! and run with ONNX Runtime here on Windows and in the Kotlin plugin on
//! Android. Its contract: inputs `images` `[1, 3, 640, 640]` RGB in 0..1,
//! the screen resized (not letterboxed), and `orig_target_sizes` `[1, 2]`
//! (width, height); outputs `labels` `[1, n]`, `boxes` `[1, n, 4]` (x1, y1,
//! x2, y2 in screen pixels) and `scores` `[1, n]`. Label 1 is text inside a
//! balloon (dialogue), label 2 text outside balloons (sound effects, R5);
//! label 0, the balloon outline, is not a text area and is left out.

use crate::api::CropIn;
use crate::export::{self, Rgb};

pub const SIZE: u32 = 640;
pub const MIN_SCORE: f32 = 0.45;
pub const MAX_IOU: f32 = 0.5;
pub const DIALOGUE: i64 = 1;
pub const EFFECT: i64 = 2;

/// Model input (CHW floats): `img` resized to 640 x 640.
pub fn input(img: &Rgb) -> Vec<f32> {
    let n = (SIZE * SIZE) as usize;
    let mut out = vec![0.0; 3 * n];
    for y in 0..SIZE {
        let sy = ((y as u64 * img.h as u64 + img.h as u64 / 2) / SIZE as u64).min(img.h as u64 - 1) as usize;
        for x in 0..SIZE {
            let sx = ((x as u64 * img.w as u64 + img.w as u64 / 2) / SIZE as u64).min(img.w as u64 - 1) as usize;
            let s = (sy * img.w as usize + sx) * 3;
            let d = (y * SIZE + x) as usize;
            for c in 0..3 {
                out[c * n + d] = img.px[s + c] as f32 / 255.0;
            }
        }
    }
    out
}

/// A text area in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub label: i64,
    pub score: f32,
}

/// Text areas among the model's `labels`, `boxes` (x1, y1, x2, y2) and
/// `scores` that score at least `min_score`.
pub fn decode(labels: &[i64], boxes: &[f32], scores: &[f32], min_score: f32) -> Vec<Detection> {
    labels
        .iter()
        .zip(scores)
        .zip(boxes.chunks_exact(4))
        .filter(|((&l, &s), _)| (l == DIALOGUE || l == EFFECT) && s >= min_score)
        .map(|((&label, &score), b)| Detection { x: b[0], y: b[1], w: b[2] - b[0], h: b[3] - b[1], label, score })
        .filter(|d| d.w > 0.0 && d.h > 0.0)
        .collect()
}

fn iou(a: &Detection, b: &Detection) -> f32 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    w * h / (a.w * a.h + b.w * b.h - w * h)
}

/// Best box of each overlapping group.
pub fn nms(mut d: Vec<Detection>, max_iou: f32) -> Vec<Detection> {
    d.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut keep: Vec<Detection> = vec![];
    for x in d {
        if keep.iter().all(|k| iou(k, &x) <= max_iou) {
            keep.push(x);
        }
    }
    keep
}

/// A box this close to the top or bottom of the screen may be cut by it:
/// P7 holds it until it knows (`Backend::crops`).
pub const EDGE: f32 = 12.0;

/// P7 crops of the boxes in screen rows `top..bottom`. A box reaching into
/// those rows, or within [`EDGE`] of them, is taken: a box held at the edge
/// before is sent again once a scroll shows it whole there.
pub fn crops(img: &Rgb, dets: &[Detection], top: u32, bottom: u32) -> Vec<CropIn> {
    let (lo, hi) = (top as f32 - EDGE, bottom as f32 + EDGE);
    dets.iter()
        .filter(|d| d.y < hi && d.y + d.h > lo)
        .filter_map(|d| {
            let (x, y) = (d.x.max(0.0) as u32, d.y.max(0.0) as u32);
            let (w, h) = ((d.x + d.w).min(img.w as f32) as u32, (d.y + d.h).min(img.h as f32) as u32);
            let (w, h) = (w.checked_sub(x)?, h.checked_sub(y)?);
            if w < 4 || h < 4 {
                return None;
            }
            let image = export::encode_png(&export::crop(img, x, y, w, h)).ok()?;
            let kind = if d.label == EFFECT { "effect" } else { "dialogue" };
            Some(CropIn { x, y, w, h, kind: kind.into(), score: d.score, image })
        })
        .collect()
}

/// The model run with ONNX Runtime where Rust does detection (A7: Windows).
#[cfg(all(feature = "app", not(target_os = "android")))]
pub struct Onnx {
    session: std::sync::Mutex<ort::session::Session>,
    _env: ort::environment::Environment,
}

#[cfg(all(feature = "app", not(target_os = "android")))]
impl Onnx {
    pub fn load(path: &std::path::Path) -> Result<Onnx, String> {
        let env = ort::init().with_name("rst-balloons").build().map_err(|e| e.to_string())?;
        let session = ort::session::Session::builder(&env)
            .and_then(|mut b| b.commit_from_file(path))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Onnx { session: std::sync::Mutex::new(session), _env: env })
    }

    pub fn detect(&self, img: &Rgb) -> Result<Vec<Detection>, String> {
        use ort::value::Tensor;
        let s = SIZE as usize;
        let images = Tensor::from_array(([1usize, 3, s, s], input(img))).map_err(|e| e.to_string())?;
        let sizes = Tensor::from_array(([1usize, 2], vec![img.w as i64, img.h as i64])).map_err(|e| e.to_string())?;
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let out = session.run(ort::inputs!["images" => images, "orig_target_sizes" => sizes]).map_err(|e| e.to_string())?;
        let (_, labels) = out["labels"].try_extract_tensor::<i64>().map_err(|e| e.to_string())?;
        let (_, boxes) = out["boxes"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        let (_, scores) = out["scores"].try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
        Ok(nms(decode(labels, boxes, scores, MIN_SCORE), MAX_IOU))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_areas_are_kept_and_balloon_outlines_and_weak_boxes_are_not() {
        let img = Rgb::new(960, 2400);
        assert_eq!(input(&img).len(), 3 * 640 * 640);
        // Two boxes over one line of dialogue, a balloon outline, a weak box, an effect.
        let labels = [1, 1, 0, 1, 2];
        let scores = [0.9, 0.7, 0.95, 0.2, 0.8];
        #[rustfmt::skip]
        let boxes = [
            220.0, 500.0, 380.0, 700.0,
            222.0, 502.0, 380.0, 700.0,
            180.0, 450.0, 420.0, 760.0,
            600.0, 100.0, 700.0, 200.0,
            450.0, 1750.0, 550.0, 1850.0,
        ];
        let d = nms(decode(&labels, &boxes, &scores, MIN_SCORE), MAX_IOU);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!((d[0].x, d[0].y, d[0].w, d[0].h, d[0].label), (220.0, 500.0, 160.0, 200.0, DIALOGUE));
        let c = crops(&img, &d, 0, 1200);
        assert_eq!(c.len(), 1, "only the box in the range");
        assert_eq!((c[0].kind.as_str(), c[0].w, c[0].h), ("dialogue", 160, 200));
        assert_eq!(crops(&img, &d, 1200, 2400)[0].kind, "effect");
    }

    #[cfg(all(feature = "app", not(target_os = "android")))]
    #[test]
    fn onnx_model_runs_and_its_box_lands_on_the_screen() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/one-balloon.onnx");
        let model = Onnx::load(&path).unwrap();
        let d = model.detect(&Rgb::new(960, 2400)).unwrap();
        assert_eq!(d.len(), 1, "{d:?}");
        // The stand-in answers in fractions of `orig_target_sizes`: 0.2 .. 0.4 by 0.2 .. 0.3.
        assert!((d[0].x - 192.0).abs() < 0.5 && (d[0].y - 480.0).abs() < 0.5, "{d:?}");
        assert!((d[0].w - 192.0).abs() < 0.5 && (d[0].h - 240.0).abs() < 0.5 && d[0].label == DIALOGUE);
    }
}
