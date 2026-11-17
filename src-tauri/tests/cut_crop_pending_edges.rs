//! Edge case of the forty-fifth review: a balloon cut by the screen's
//! bottom edge is asked for, and before Gemini answers a scroll shows it
//! whole. The whole crop is not joined to the cut one's request, however
//! alike their fingerprints: its words are read, and its own translation
//! is placed, not the cut one's.

mod support;

use support::*;

/// A crop of one flat gray: its hash is 0 whatever its shape.
fn flat(x: i64, y: i64, w: u32, h: u32) -> Crop {
    Crop { rect: Rect::new(x, y, w as i64, h as i64), kind: "dialogue".into(), image: png_rgb(w, h, |_, _| [128; 3]) }
}

#[test]
fn a_whole_crop_shown_while_its_cut_one_waits_is_read_and_placed() {
    let mut app = App::on();
    app.gemini.read_as((160, 200), "あ");
    app.gemini.translate_as("あ", "가");
    // Not "あい": the fake answers a translation with every word it read
    // that the request holds, and "あ" is in it.
    app.gemini.read_as((160, 220), "いう");
    app.gemini.translate_as("いう", "가나");
    app.gemini.delay(std::time::Duration::from_millis(1500));
    let scene = Scene::new(4501);
    app.settle(&scene, 0);
    // The review's crops: cut at the bottom edge, then whole after a 100 px scroll.
    app.crops_ok(&[flat(200, 2200, 160, 200)]);
    app.settle(&scene, 100);
    app.crops_ok(&[flat(200, 2100, 160, 220)]);
    wait_until(20, "the whole balloon was never read", || app.gemini.requests().iter().any(|r| r.images().contains(&(160, 220))));
    wait_until(20, "the whole balloon's translation was not placed", || app.placements().iter().any(|p| p.ko == "가나"));
    // The cut one's answer came first, and placed nothing.
    let list = app.placements();
    assert!(list.iter().all(|p| p.ko != "가"), "the cut balloon's translation was placed: {list:?}");
}
