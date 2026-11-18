//! Edge case of the twenty-seventh review, checked on the sources (no
//! Android toolchain runs here): the overlay draws the Korean at the line
//! height `layout.rs` fitted it with, and makes it smaller while the
//! device's own font still runs over the shape, so no line is cut off.

use realtime_screen_translator::layout;
use std::path::Path;

fn overlay() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/screen/android/src/main/java/Overlay.kt");
    std::fs::read_to_string(path).unwrap().replace("\r\n", "\n")
}

#[test]
fn the_overlay_sets_lines_1_2_em_apart_as_the_fit_assumes() {
    // The review's case: four lines in a 120 x 192 text area.
    let shape = layout::fit(0, 0, 120, 192, "가\n나\n다\n라");
    let lines = layout::lines("가\n나\n다\n라", shape.font_px as u32, shape.w - 2 * layout::PAD).len();
    assert!(layout::text_height(lines, shape.font_px as u32) <= shape.h - 2 * layout::PAD);
    let kt = overlay();
    assert!(kt.contains(".setLineSpacing(1.2f * size - text.fontSpacing, 1f)"), "the lines are not set 1.2 em apart");
    assert!(!kt.contains(".setLineSpacing(0f, 1.2f)"), "the font's own line height is still scaled by 1.2");
}

#[test]
fn the_overlay_makes_the_korean_smaller_while_it_runs_over_the_shape() {
    let kt = overlay();
    let draw = &kt[kt.find("override fun onDraw").expect("no onDraw")..];
    let draw = &draw[..draw.find("layout.draw(canvas)").expect("no layout.draw")];
    assert!(draw.contains("val room = maxOf(1, s.h - 2 * PAD)"), "{draw}");
    assert!(draw.contains("while (layout.height > room && size > MIN_TEXT_PX)"), "the drawn height is not checked against the shape");
    assert!(draw.contains("size -= 1f") && draw.contains("layout = textLayout(s.ko, size, inner)"), "the size is not brought down");
}
