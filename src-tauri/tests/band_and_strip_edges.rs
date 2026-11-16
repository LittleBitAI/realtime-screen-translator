//! Edge cases of the fifth review: the rows a status bar held on the first
//! screen, and strips P6 never asked for.

mod support;

use serde_json::json;
use support::*;

#[test]
fn content_scrolled_into_the_first_screens_status_bar_rows_is_saved() {
    let mut app = App::on();
    // 80 px of status bar over the content.
    let scene = Scene::with_fixed_top(36, 8);
    app.read_and_save(&scene, 0);
    // Scrolling 300 px up shows the content that was under the status bar
    // at screen 300 .. 380, besides the 220 px above the first screen.
    app.show(&scene, -300);
    let rest = app.settle(&scene, -300);
    let f = rest.iter().find(|f| f.save.is_some()).expect("P6 asked for nothing");
    assert_eq!(f.save, Some((80, 380)), "the rows under the first status bar were not asked for");
    assert_eq!(f.detect, Some((80, 380)), "the rows under the first status bar were not detected");
    app.strip(&scene, -300, (80, 380), f.seq).expect("the strip was refused");

    let book = app.book_id.unwrap();
    app.stop();
    let out = app.export(book).expect("export failed");
    let img = decode_png(&std::fs::read(out["files"][0].as_str().unwrap()).unwrap());
    assert_eq!(img.h, 2620, "picture rows -220 .. 2400");
    // Picture row 40 is screen row 340 at offset -300: content, not the status bar.
    for x in [5, 300, 700] {
        let want = scene.screen_px(-300, SCREEN_W, x, 340);
        assert_eq!(img.at(x as i64, 260)[0], want, "picture row 40 still shows the status bar at x {x}");
    }
}

#[test]
fn a_strip_outside_what_p6_asked_for_is_not_stored() {
    let mut app = App::on();
    let scene = Scene::new(502);
    let saved = app.read_and_save(&scene, 0);
    let picture_id = app.picture_id.unwrap();
    // Rows below the captured screen.
    let image = png_rgb(SCREEN_W, 100, |_, _| [9, 9, 9]);
    let e = app.host.err("post_strips", json!({"picture_id": picture_id, "frame_seq": saved[1].seq, "top": 2400, "bottom": 2500, "image": image}));
    assert_eq!(e, "already_covered");
    // New rows, but from a frame P6 asked nothing of (the screen was moving).
    let moving = app.show(&scene, 600);
    assert_eq!(moving.save, None);
    let e = app.strip(&scene, 600, (1800, 2400), moving.seq).expect_err("a strip P6 did not ask for was stored");
    assert_eq!(e, "already_covered");
    let rest = app.settle(&scene, 600);
    let f = rest.iter().find(|f| f.save.is_some()).unwrap();
    let r = app.strip(&scene, 600, f.save.unwrap(), f.seq).expect("the asked strip was refused");
    assert_eq!(r["picture_height"], 3000);
}
