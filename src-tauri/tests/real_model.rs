//! R25 with the shipped model: a drawn manga page with a balloon of vertical
//! Japanese dialogue is found as dialogue (label 1) by the real pinned model.
//! Downloads 11 MB once, so it is opt-in:
//! `cargo test --release -p realtime-screen-translator --features app --test real_model -- --ignored`.
#![cfg(all(feature = "app", windows))]

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use realtime_screen_translator::detect::{Onnx, DIALOGUE};
use realtime_screen_translator::export::Rgb;
use realtime_screen_translator::models;

fn put(img: &mut Rgb, x: i64, y: i64, v: u8) {
    if x >= 0 && y >= 0 && (x as u32) < img.w && (y as u32) < img.h {
        let i = (y as usize * img.w as usize + x as usize) * 3;
        img.px[i..i + 3].copy_from_slice(&[v, v, v]);
    }
}

fn text(img: &mut Rgb, font: &FontVec, size: f32, x: f32, y: f32, s: &str, vertical: bool) {
    let scale = PxScale::from(size);
    let ascent = font.as_scaled(scale).ascent();
    let (mut cx, mut cy) = (x, y);
    for ch in s.chars() {
        let g = font.glyph_id(ch).with_scale_and_position(scale, ab_glyph::point(cx, cy + ascent));
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, c| {
                if c > 0.5 {
                    put(img, b.min.x as i64 + gx as i64, b.min.y as i64 + gy as i64, 0)
                }
            });
        }
        if vertical {
            cy += size * 1.1
        } else {
            cx += size * 1.05
        }
    }
}

#[test]
#[ignore = "downloads the 11 MB model"]
fn the_real_model_finds_dialogue_in_a_balloon() {
    let path = models::path(&std::env::temp_dir().join("rst-real-model"));
    let agent: ureq::Agent = ureq::Agent::config_builder().proxy(ureq::Proxy::try_from_env()).build().into();
    models::ensure(&agent, models::URL, models::SHA256, &path).unwrap();
    let model = Onnx::load(&path).unwrap();
    let font = FontVec::try_from_vec_and_index(std::fs::read("C:/Windows/Fonts/YuGothB.ttc").unwrap(), 0).unwrap();
    let mut img = Rgb::new(1080, 2400);
    for y in 300..1500 {
        for x in 60..1020 {
            if x < 66 || x > 1013 || y < 306 || y > 1493 {
                put(&mut img, x, y, 0)
            }
        }
    }
    // A white balloon with a black outline around three vertical columns.
    let (cx, cy, rx, ry) = (700.0f32, 650.0f32, 190.0f32, 260.0f32);
    for y in 360..960 {
        for x in 480..920 {
            let d = ((x as f32 - cx) / rx).powi(2) + ((y as f32 - cy) / ry).powi(2);
            if d <= 1.0 {
                put(&mut img, x, y, if d > 0.95 { 0 } else { 255 })
            }
        }
    }
    for (i, col) in ["そんなこと", "言わないで", "くれよ…"].iter().enumerate() {
        text(&mut img, &font, 56.0, 770.0 - i as f32 * 70.0, 470.0, col, true);
    }
    let t = std::time::Instant::now();
    let d = model.detect(&img).unwrap();
    println!("detect {:?}: {d:?}", t.elapsed());
    let hit = d.iter().find(|b| b.label == DIALOGUE).expect("no dialogue found");
    // The text columns span about x 630 .. 830, y 470 .. 800.
    assert!(hit.x < 660.0 && hit.x + hit.w > 800.0 && hit.y < 500.0 && hit.y + hit.h > 760.0, "{hit:?}");
}
