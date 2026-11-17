//! Edge cases of the thirty-fifth review, checked on the Kotlin sources (no
//! Android toolchain runs here): the frame queue never drops the frame just
//! stored, so a delivered frame, its clean frame and a peek all pinned cannot
//! stall delivery; and frames taken while the shapes were hidden for a look
//! under them are kept for that look only, never handed on as the screen.

use std::path::Path;

fn capture() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/screen/android/src/main/java/Capture.kt")).unwrap().replace("\r\n", "\n")
}

/// The body of `fun name(` in `src`, up to its closing brace at four spaces.
fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src.find(&format!("fun {name}(")).unwrap_or_else(|| panic!("no fun {name}"));
    let rest = &src[at..];
    &rest[..rest.find("\n    }\n").unwrap()]
}

#[test]
fn three_pinned_frames_do_not_stall_delivery() {
    let src = capture();
    let store = body(&src, "store");
    assert!(
        store.contains("frames.keys.firstOrNull { it !in pinned && it != seq } ?: break"),
        "the frame just stored can be dropped, or a full set of pins loops: {store}"
    );
    let evict = store.find("while (frames.size > KEEP)").unwrap();
    assert!(store.find("fresh = seq").unwrap() > evict, "fresh is set before the queue is trimmed");
}

#[test]
fn frames_without_the_shapes_are_not_handed_on_as_the_screen() {
    let src = capture();
    let store = body(&src, "store");
    let apart = store.find("if (apart) return").expect("a frame taken without the shapes is made fresh");
    assert!(apart < store.find("fresh = seq").unwrap() && apart < store.find("deliver()").unwrap());
    assert!(store.contains("val apart = at >= asideFrom && at < asideUntil"));
    let peek = body(&src, "peekFrame");
    let (from, hide) = (peek.find("asideFrom = SystemClock.uptimeMillis()").expect("no window opened"), peek.find("Overlay.hideShapesNow()").unwrap());
    assert!(from < hide, "the window opens after the shapes are hidden");
    assert!(peek.contains("newestSince(hiddenAt) { it in aside && frames[it]?.let(Overlay::shapesGone) == true }"), "the look takes a frame that may show the shapes");
    assert!(peek.contains("Overlay.restoreShapes { shownAt -> asideUntil = shownAt + SETTLE_MS }"), "the window closes before the shapes are back");
    assert!(body(&src, "repeatLast").contains("frames.keys.lastOrNull { it !in aside }"), "a frame without the shapes is repeated as the screen");
    assert!(body(&src, "clear").contains("aside.clear()"));
}
