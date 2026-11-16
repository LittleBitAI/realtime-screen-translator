//! Edge case of the thirteenth review: a fling that moves most of the
//! screen between two frames is still a scroll, not a page turn.

mod support;

use support::*;

#[test]
fn a_scroll_of_three_quarters_of_the_screen_between_frames_is_followed() {
    let mut app = App::on();
    let scene = Scene::new(1301);
    app.read_and_save(&scene, 0);
    let picture = app.picture_id;
    // 1800 of 2400 px in one frame: 600 px of the screen are still in common.
    let f = app.show(&scene, 1800);
    assert_eq!((f.motion.as_str(), f.dy, f.clear), ("scroll", 1800, false), "the fling was taken for a page turn");
    assert_eq!(app.picture_id, picture, "the stitched picture was closed");
    let rest = app.settle(&scene, 1800);
    let f = rest.iter().find(|f| f.save.is_some()).expect("P6 asked for nothing after the fling");
    assert_eq!(f.save, Some((600, 2400)), "only the new rows are saved");
}
