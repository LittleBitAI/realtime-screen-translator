//! Edge cases found in review, beyond the contract tests T1..T37.

mod support;

use serde_json::json;
use std::thread::sleep;
use std::time::Duration;
use support::*;

/// A crop of one flat gray: its hash is 0 whatever its shape.
fn flat(x: i64, y: i64, w: u32, h: u32) -> Crop {
    Crop { rect: Rect::new(x, y, w as i64, h as i64), kind: "dialogue".into(), image: png_rgb(w, h, |_, _| [128; 3]) }
}

#[test]
fn same_hash_with_another_shape_keeps_both_translations() {
    let mut app = App::on();
    app.settle(&Scene::new(41), 0);
    app.gemini.read_as((60, 100), "はい");
    app.gemini.translate_as("はい", "네");
    app.gemini.read_as((240, 100), "いいえ");
    app.gemini.translate_as("いいえ", "아니");

    app.crops_ok(&[flat(100, 300, 60, 100)]);
    app.wait_placements(1);
    app.crops_ok(&[flat(400, 1200, 240, 100)]);
    let list = app.wait_placements(2);
    let mut kos: Vec<String> = list.iter().map(|p| p.ko.clone()).collect();
    kos.sort();
    assert_eq!(kos, vec!["네", "아니"]);

    let dirs = app.close();
    let mut rows: Vec<String> = translations(&dirs.data).into_iter().filter_map(|r| r.translation_ko).collect();
    rows.sort();
    assert_eq!(rows, vec!["네", "아니"], "one translation overwrote the other");
}

#[test]
fn strip_that_is_not_the_screen_rows_is_refused() {
    let mut app = App::on();
    let scene = Scene::new(42);
    let frames = app.settle(&scene, 0);
    let (f, (top, bottom)) = frames.iter().find_map(|f| f.save.map(|r| (f, r))).expect("P6 asked for no strip");
    let picture_id = app.picture_id.unwrap();
    let header_only = png_rgb(SCREEN_W, bottom - top, |_, _| [0; 3])[..24].to_vec();
    for image in [png_rgb(1, 1, |_, _| [0; 3]), header_only] {
        let err = app.host.err(
            "post_strips",
            json!({"picture_id": picture_id, "frame_seq": f.seq, "top": top, "bottom": bottom, "image": image}),
        );
        assert_eq!(err, "bad_image");
    }
    app.strip(&scene, 0, (top, bottom), f.seq).expect("the real strip was refused");
}

#[test]
fn failed_translation_is_asked_again() {
    let mut app = App::on();
    app.settle(&Scene::new(43), 0);
    let b = Balloon::new(200, 500, 160, 200, 431);
    app.gemini.read_as(b.size(), "まって");
    app.gemini.translate_as("まって", "기다려");
    app.gemini.refuse_translation(1);

    app.crops_ok(&[b.dialogue()]);
    let dir = app.dirs.data.clone();
    wait_until(20, "a failed Translation", || translations(&dir).iter().any(|r| r.status == "failed"));

    app.crops_ok(&[b.dialogue()]);
    let list = app.wait_placements(1);
    assert_eq!(list[0].ko, "기다려");
}

#[test]
fn same_balloon_seen_in_several_places_while_waiting_is_placed_everywhere() {
    let mut app = App::on();
    app.settle(&Scene::new(44), 0);
    app.gemini.delay(Duration::from_millis(600));
    let a = Balloon::new(100, 300, 160, 200, 441);
    let b = a.at(500, 900);
    let c = a.at(300, 1600);

    app.crops_ok(&[a.dialogue(), b.dialogue()]);
    app.crops_ok(&[c.dialogue()]);
    let list = app.wait_placements(3);
    for balloon in [a, b, c] {
        assert!(list.iter().any(|p| p.rect().covers(&balloon.rect())), "nothing over {:?}: {list:?}", balloon.rect());
    }
    assert_eq!(app.gemini.requests().iter().filter(|r| r.is_read()).count(), 1);
}

#[test]
fn no_retry_goes_out_after_stop() {
    let mut app = App::on();
    app.settle(&Scene::new(45), 0);
    app.gemini.delay(Duration::from_millis(800));
    app.gemini.fail_with(400, "{\"error\":{\"message\":\"Invalid threshold OFF\"}}");

    app.crops_ok(&[Balloon::new(200, 500, 160, 200, 451).dialogue()]);
    let gemini = &app.gemini;
    wait_until(20, "the first request", || gemini.count() == 1);
    app.stop();
    sleep(Duration::from_millis(1500));
    assert_eq!(app.gemini.count(), 1, "the BLOCK_NONE retry went out after stop");
}
