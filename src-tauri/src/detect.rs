//! R25: the light local model that finds the text areas of balloons.
//!
//! The model file is `balloons.onnx` in the app's data folder (`models/`),
//! the same file the Android plugin loads. Its contract is a YOLOv8 detection
//! export: input `[1, 3, 640, 640]` RGB in 0..1, letterboxed on gray 114;
//! output `[1, 4 + classes, n]` holding centre x, centre y, width and height
//! in input pixels, then one score per class. Class 0 is dialogue text,
//! class 1 sound-effect text (R5); a one-class model finds dialogue only.

use crate::api::CropIn;
use crate::export::{self, Rgb};

pub const SIZE: u32 = 640;
pub const MIN_SCORE: f32 = 0.35;
pub const MAX_IOU: f32 = 0.45;
pub const MODEL_FILE: &str = "balloons.onnx";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letterbox {
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
}

/// Model input (CHW floats) for `img`, and how to map boxes back.
pub fn letterbox(img: &Rgb) -> (Vec<f32>, Letterbox) {
    let scale = (SIZE as f32 / img.w as f32).min(SIZE as f32 / img.h as f32);
    let (w, h) = ((img.w as f32 * scale).round() as u32, (img.h as f32 * scale).round() as u32);
    let lb = Letterbox { scale, pad_x: ((SIZE - w) / 2) as f32, pad_y: ((SIZE - h) / 2) as f32 };
    let n = (SIZE * SIZE) as usize;
    let mut out = vec![114.0 / 255.0; 3 * n];
    for y in 0..h {
        let sy = ((y as f32 / scale) as u32).min(img.h - 1);
        for x in 0..w {
            let sx = ((x as f32 / scale) as u32).min(img.w - 1);
            let s = (sy as usize * img.w as usize + sx as usize) * 3;
            let d = (y + lb.pad_y as u32) as usize * SIZE as usize + (x + lb.pad_x as u32) as usize;
            for c in 0..3 {
                out[c * n + d] = img.px[s + c] as f32 / 255.0;
            }
        }
    }
    (out, lb)
}

/// A text area in screen pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub class: usize,
    pub score: f32,
}

/// Boxes of a `[channels, n]` YOLOv8 output scoring at least `min_score`.
pub fn decode(out: &[f32], channels: usize, n: usize, lb: &Letterbox, min_score: f32) -> Vec<Detection> {
    if channels < 5 || out.len() < channels * n {
        return vec![];
    }
    let at = |c: usize, i: usize| out[c * n + i];
    (0..n)
        .filter_map(|i| {
            let (class, score) = (4..channels).map(|c| (c - 4, at(c, i))).max_by(|a, b| a.1.total_cmp(&b.1))?;
            (score >= min_score).then(|| {
                let (w, h) = (at(2, i) / lb.scale, at(3, i) / lb.scale);
                let x = (at(0, i) - lb.pad_x) / lb.scale - w / 2.0;
                let y = (at(1, i) - lb.pad_y) / lb.scale - h / 2.0;
                Detection { x, y, w, h, class, score }
            })
        })
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

/// P7 crops of the boxes whose centre lies in screen rows `top..bottom`.
pub fn crops(img: &Rgb, dets: &[Detection], top: u32, bottom: u32) -> Vec<CropIn> {
    dets.iter()
        .filter(|d| (top as f32..bottom as f32).contains(&(d.y + d.h / 2.0)))
        .filter_map(|d| {
            let (x, y) = (d.x.max(0.0) as u32, d.y.max(0.0) as u32);
            let (w, h) = ((d.x + d.w).min(img.w as f32) as u32, (d.y + d.h).min(img.h as f32) as u32);
            let (w, h) = (w.checked_sub(x)?, h.checked_sub(y)?);
            if w < 4 || h < 4 {
                return None;
            }
            let image = export::encode_png(&export::crop(img, x, y, w, h)).ok()?;
            let kind = if d.class == 1 { "effect" } else { "dialogue" };
            Some(CropIn { x, y, w, h, kind: kind.into(), score: d.score, image })
        })
        .collect()
}

/// The model run with tract (pure Rust ONNX) where Rust does detection (A7: Windows).
#[cfg(all(feature = "app", not(target_os = "android")))]
pub struct Onnx {
    plan: std::sync::Arc<tract_onnx::prelude::TypedRunnableModel>,
}

#[cfg(all(feature = "app", not(target_os = "android")))]
impl Onnx {
    pub fn load(path: &std::path::Path) -> Result<Onnx, String> {
        use tract_onnx::prelude::*;
        let plan = tract_onnx::onnx()
            .model_for_path(path)
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, SIZE as usize, SIZE as usize]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Onnx { plan })
    }

    pub fn detect(&self, img: &Rgb) -> Result<Vec<Detection>, String> {
        use tract_onnx::prelude::*;
        let (input, lb) = letterbox(img);
        let s = SIZE as usize;
        let tensor = Tensor::from_shape(&[1, 3, s, s], &input).map_err(|e| e.to_string())?;
        let out = self.plan.run(tvec!(tensor.into())).map_err(|e| e.to_string())?;
        let view = out[0].to_plain_array_view::<f32>().map_err(|e| e.to_string())?;
        let shape = view.shape().to_vec();
        let (channels, n) = match shape.as_slice() {
            [1, c, n] => (*c, *n),
            _ => return Err(format!("unexpected model output {shape:?}")),
        };
        let flat: Vec<f32> = view.iter().copied().collect();
        Ok(nms(decode(&flat, channels, n, &lb, MIN_SCORE), MAX_IOU))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxes_map_back_to_the_screen_and_overlaps_merge() {
        let img = Rgb::new(960, 2400);
        let (input, lb) = letterbox(&img);
        assert_eq!(input.len(), 3 * 640 * 640);
        assert_eq!(lb.pad_y, 0.0);
        // Two boxes over one balloon, one effect box: [6 channels, 3 boxes].
        let px = |v: f32| v * lb.scale;
        let (cx, cy) = (px(300.0) + lb.pad_x, px(600.0));
        #[rustfmt::skip]
        let out = vec![
            cx, cx + 1.0, px(500.0) + lb.pad_x,
            cy, cy, px(1800.0),
            px(160.0), px(160.0), px(100.0),
            px(200.0), px(200.0), px(100.0),
            0.9, 0.6, 0.1,
            0.0, 0.0, 0.8,
        ];
        let d = nms(decode(&out, 6, 3, &lb, MIN_SCORE), MAX_IOU);
        assert_eq!(d.len(), 2);
        assert!((d[0].x - 220.0).abs() < 1.0 && (d[0].y - 500.0).abs() < 1.0 && d[0].class == 0);
        let c = crops(&img, &d, 0, 1200);
        assert_eq!(c.len(), 1, "only the box whose centre is in the range");
        assert_eq!((c[0].kind.as_str(), c[0].w, c[0].h), ("dialogue", 160, 200));
        assert_eq!(crops(&img, &d, 1200, 2400)[0].kind, "effect");
    }

    #[cfg(all(feature = "app", not(target_os = "android")))]
    #[test]
    fn onnx_model_runs_and_its_box_lands_on_the_screen() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/one-balloon.onnx");
        let model = Onnx::load(&path).unwrap();
        let img = Rgb::new(960, 2400);
        let d = model.detect(&img).unwrap();
        assert_eq!(d.len(), 1, "{d:?}");
        let (_, lb) = letterbox(&img);
        assert!((d[0].x - ((200.0 - lb.pad_x) / lb.scale - 40.0 / lb.scale)).abs() < 1.0);
        assert!((d[0].h - 100.0 / lb.scale).abs() < 1.0 && d[0].class == 0);
    }
}
