//! Edge case of the eleventh review: a fixed band learnt on one screen does
//! not hold back the top rows of the next page.

mod support;

use support::*;

#[test]
fn a_new_page_is_detected_and_saved_whole_after_a_band_was_learnt() {
    let mut app = App::on();
    // 80 px of status bar over a scrolled screen: the band is learnt.
    let scrolled = Scene::with_fixed_top(36, 8);
    app.read_and_save(&scrolled, 0);
    app.show(&scrolled, 300);
    app.settle(&scrolled, 300);

    // Another page altogether, content in its top rows too.
    let page = Scene::new(372);
    let turned = app.show(&page, 0);
    assert_eq!(turned.motion, "page");
    let rest = app.settle(&page, 0);
    let f = rest.iter().find(|f| f.save.is_some()).expect("P6 asked for nothing on the new page");
    assert_eq!(f.detect, Some((0, 2400)), "the new page was not detected whole");
    assert_eq!(f.save, Some((0, 2400)), "the new page was not saved whole");
}
