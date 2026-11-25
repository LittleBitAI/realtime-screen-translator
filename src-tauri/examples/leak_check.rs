//! Development check: on each replayed page, the source dialogue's letters
//! left showing beside the covers drawn over it.
//!
//! cargo run --example leak_check -- <data dir> <chapters dir> <replay out dir> <lang> <chapter>...
//!
//! `<chapters dir>/<chapter>/NN.jpg` are the page pictures, `<replay out
//! dir>/<chapter>/NN.json` the layouts `replay_chapters` wrote for them, and
//! the library in `<data dir>` holds each page's answer (its dialogue items).
//! Every lettering block on the page (`balloon::blocks`) is split into its
//! letter-sized marks, and a mark shows when most of its ink lies outside
//! every cover (white, or tone). A mark is dialogue when it lies on an
//! item's lettering: inside the item's text box, grown by one letter of the
//! block round it (an end letter lettered just past the box). Marks on no
//! item are the art's (a speed line, a hand) or lettering the app does not
//! translate (sound effects, signs).
//!
//! Prints one line per page, `<chapter> <NN> dialogue <shown>/<marks>`, a
//! line per dialogue mark showing, and `dialogue marks showing: <n>` last.

use realtime_screen_translator::{balloon, export};
use sha2::{Digest, Sha256};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [data, pages, out, lang, chapters @ ..] = a.as_slice() else {
        eprintln!("usage: leak_check <data dir> <chapters dir> <replay out dir> <lang> <chapter>...");
        std::process::exit(2);
    };
    let db = rusqlite::Connection::open(Path::new(data).join("translator.sqlite3")).expect("open the library");
    let mut total = 0;
    for ch in chapters {
        for n in 1.. {
            let Ok(bytes) = std::fs::read(Path::new(pages).join(ch).join(format!("{n:02}.jpg"))) else { break };
            let json = std::fs::read_to_string(Path::new(out).join(ch).join(format!("{n:02}.json"))).expect("the page's layout");
            let layout: serde_json::Value = serde_json::from_str(&json).unwrap();
            let key = format!("{}:{lang}", hex(&Sha256::digest(&bytes)));
            let items: String = db.query_row("SELECT items FROM ReaderPage WHERE key = ?1", [&key], |r| r.get(0)).expect("the page's answer");
            let items: Vec<serde_json::Value> = serde_json::from_str(&items).unwrap();
            let pic = export::decode_jpeg(&bytes).unwrap();
            let (w, h) = (pic.w as usize, pic.h as usize);
            let gray = pic.gray();
            let covered = covers(&layout, w, h);
            // Each item's text box (`[top, left, bottom, right]`, fractions),
            // as `[x0, y0, x1, y1]` page pixels.
            let boxes: Vec<[f64; 4]> = items
                .iter()
                .filter_map(|i| serde_json::from_value::<[f64; 4]>(i["text"].clone()).ok())
                .map(|t| [t[1] * w as f64, t[0] * h as f64, t[3] * w as f64, t[2] * h as f64])
                .collect();
            let (mut marks, mut shown) = (0, 0);
            let page = balloon::Page { w: pic.w, h: pic.h, px: &gray };
            // With RST_LEAK_SHOTS set, a picture of each page to look at:
            // covers tinted blue, item text boxes in green, marks showing red.
            let mut shot = export::Rgb { w: pic.w, h: pic.h, px: pic.px.clone() };
            for (i, _) in covered.iter().enumerate().filter(|(_, c)| **c) {
                let p = &mut shot.px[i * 3..i * 3 + 3];
                (p[0], p[1], p[2]) = (p[0] / 2 + 60, p[1] / 2 + 90, p[2] / 2 + 127);
            }
            for t in &boxes {
                let [x0, y0, x1, y1] = t.map(|v| v as usize);
                for x in x0..x1.min(w) {
                    for y in [y0.min(h - 1), (y1).min(h - 1)] {
                        shot.px[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[0, 200, 0]);
                    }
                }
                for y in y0..y1.min(h) {
                    for x in [x0.min(w - 1), x1.min(w - 1)] {
                        shot.px[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[0, 200, 0]);
                    }
                }
            }
            // Each stroke is counted once, in the first block it is met in.
            let mut seen = vec![false; w * h];
            for b in balloon::blocks(&page, false) {
                for m in letters(&gray, &covered, w, h, &b, &mut seen) {
                    let (cx, cy) = ((m.bb[0] + m.bb[2]) as f64 / 2.0, (m.bb[1] + m.bb[3]) as f64 / 2.0);
                    let g = b.glyph;
                    if !boxes.iter().any(|t| cx >= t[0] - g && cx <= t[2] + g && cy >= t[1] - g && cy <= t[3] + g) {
                        continue;
                    }
                    marks += 1;
                    if m.open * 2 > m.ink {
                        shown += 1;
                        println!("{ch} {n:02} SHOWING mark {:?} glyph {g:.0} of block {:?}", m.bb, b.bounds.map(|v| v as i64));
                        for y in m.bb[1]..=m.bb[3] {
                            for x in m.bb[0]..=m.bb[2] {
                                if gray[y * w + x] < 128 {
                                    shot.px[(y * w + x) * 3..(y * w + x) * 3 + 3].copy_from_slice(&[255, 0, 0]);
                                }
                            }
                        }
                    }
                }
            }
            if let Ok(dir) = std::env::var("RST_LEAK_SHOTS") {
                std::fs::write(Path::new(&dir).join(format!("{ch}-{n:02}.png")), export::encode_png(&shot).unwrap()).unwrap();
            }
            println!("{ch} {n:02} dialogue {shown}/{marks}");
            total += shown;
        }
    }
    println!("dialogue marks showing: {total}");
}

/// The pixels the layout covers: each box's outline (or its whole box
/// where it has none), and each tone patch's opaque pixels.
fn covers(layout: &serde_json::Value, w: usize, h: usize) -> Vec<bool> {
    let mut covered = vec![false; w * h];
    for b in layout["boxes"].as_array().unwrap() {
        let outline: Vec<[i64; 2]> = serde_json::from_value(b["outline"].clone()).unwrap_or_default();
        let at = |k: &str| b[k].as_i64().unwrap();
        let patch = b.get("patch").filter(|p| !p.is_null());
        if let Some(p) = patch {
            use base64::Engine;
            let at: [i64; 4] = serde_json::from_value(p["at"].clone()).unwrap();
            let png = base64::engine::general_purpose::STANDARD.decode(p["png"].as_str().unwrap()).unwrap();
            let mut r = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
            let mut buf = vec![0; r.output_buffer_size().unwrap()];
            r.next_frame(&mut buf).unwrap();
            for (i, _) in buf.chunks(4).enumerate().filter(|(_, q)| q[3] == 255) {
                let (x, y) = (at[0] + i as i64 % at[2], at[1] + i as i64 / at[2]);
                if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                    covered[y as usize * w + x as usize] = true;
                }
            }
        }
        // A patch with no outline is the whole cover: there is no white.
        if patch.is_some() && outline.is_empty() {
            continue;
        }
        let (x, y, bw, bh) = (at("x"), at("y"), at("w"), at("h"));
        for py in y.max(0)..(y + bh).min(h as i64) {
            for px in x.max(0)..(x + bw).min(w as i64) {
                if outline.is_empty() || inside(&outline, px as f64 + 0.5, py as f64 + 0.5) {
                    covered[py as usize * w + px as usize] = true;
                }
            }
        }
    }
    covered
}

struct Mark {
    bb: [usize; 4],
    ink: usize,
    open: usize,
}

/// A block's letter-sized dark marks, each with how much of its ink no
/// cover hides. A mark is its whole 8-connected stroke on the page: a speed
/// line, a strand of hair or an outline running out of the block is longer
/// than a letter, and is not one.
fn letters(gray: &[u8], covered: &[bool], w: usize, h: usize, b: &balloon::Block, seen: &mut [bool]) -> Vec<Mark> {
    let (x0, y0) = (b.bounds[0] as usize, b.bounds[1] as usize);
    let (x1, y1) = ((b.bounds[2] as usize).min(w), (b.bounds[3] as usize).min(h));
    let mut out = vec![];
    for sy in y0..y1 {
        for sx in x0..x1 {
            let start = sy * w + sx;
            if seen[start] || gray[start] >= 128 {
                continue;
            }
            seen[start] = true;
            let mut stack = vec![start];
            let mut m = Mark { bb: [usize::MAX, usize::MAX, 0, 0], ink: 0, open: 0 };
            while let Some(i) = stack.pop() {
                let (x, y) = (i % w, i / w);
                m.ink += 1;
                m.open += !covered[i] as usize;
                m.bb = [m.bb[0].min(x), m.bb[1].min(y), m.bb[2].max(x), m.bb[3].max(y)];
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if !seen[j] && gray[j] < 128 {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            // Not a letter: a stroke cut by the page's edge (a speed line
            // running off it), or a line longer than a letter and thinner
            // than a letter's stroke; a dash is no longer than a letter.
            let (mw, mh) = ((m.bb[2] - m.bb[0] + 1) as f64, (m.bb[3] - m.bb[1] + 1) as f64);
            let (size, thin) = (mw.max(mh), mw.min(mh));
            let edge = m.bb[0] == 0 || m.bb[1] == 0 || m.bb[2] + 1 == w || m.bb[3] + 1 == h;
            if size <= 2.0 * b.glyph && size >= 0.3 * b.glyph && !edge && !(size > b.glyph && thin < 0.15 * b.glyph) {
                out.push(m);
            }
        }
    }
    out
}

fn inside(poly: &[[i64; 2]], x: f64, y: f64) -> bool {
    let mut wn = 0;
    for (i, a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        let (ax, ay, bx, by) = (a[0] as f64, a[1] as f64, b[0] as f64, b[1] as f64);
        let left = (bx - ax) * (y - ay) - (x - ax) * (by - ay);
        if ay <= y && by > y && left > 0.0 {
            wn += 1
        } else if ay > y && by <= y && left < 0.0 {
            wn -= 1
        }
    }
    wn != 0
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}
