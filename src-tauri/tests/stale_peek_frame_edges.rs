//! Edge case of the forty-fourth review, checked on the Kotlin sources (no
//! Android toolchain runs here): an image the mirror composed before the
//! shapes were hidden, however late it came, is never taken as one without
//! them. Frames are timed by when the mirror composed them, and a peek is
//! taken only once the shapes' own Korean and white are gone from it.

use std::path::Path;

fn read(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/screen/android/src/main/java").join(name)).unwrap().replace("\r\n", "\n")
}

/// The text of `fun name(` in `src`, up to its closing brace at four spaces.
fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src.find(&format!("fun {name}(")).unwrap_or_else(|| panic!("no fun {name}"));
    let rest = &src[at..];
    &rest[..rest.find("\n    }\n").unwrap()]
}

#[test]
fn frames_are_timed_by_when_the_mirror_composed_them() {
    let service = read("CaptureService.kt");
    let reader = body(&service, "newReader");
    assert!(reader.contains("heldAt = composedAt(img)"), "a frame is timed by when it came: {reader}");
    assert!(!reader.contains("heldAt = SystemClock.uptimeMillis()"));
    let composed = body(&service, "composedAt");
    assert!(composed.contains("img.timestamp / 1_000_000"), "the image's own timestamp is not used");
    assert!(composed.contains("if (t in now - 5_000..now) t else now"), "a timestamp of another clock is taken as uptime");
}

#[test]
fn a_peek_is_taken_only_once_the_shapes_are_gone_from_it() {
    let overlay = read("Overlay.kt");
    let hide = body(&overlay, "hideShapesNow");
    let (seen, hidden) = (hide.find("seen = pointsOf(shapes)").expect("the shapes are not looked at before hiding"), hide.find("shapes.forEach { it.visibility = View.INVISIBLE }").unwrap());
    assert!(seen < hidden, "the shapes are looked at after they are hidden");
    let points = body(&overlay, "pointsOf");
    assert!(points.contains("v.draw(Canvas(b))") && points.contains("if (Color.alpha(c) != 255) continue"), "{points}");
    let gone = body(&overlay, "shapesGone");
    assert!(gone.contains("share(ink) { it < 128 } >= 2.0 / 3 && share(fill) { it > 180 } >= 2.0 / 3"), "{gone}");
    let capture = read("Capture.kt");
    let peek = body(&capture, "peekFrame");
    assert!(peek.contains("newestSince(hiddenAt) { it in aside && frames[it]?.let(Overlay::shapesGone) == true }"), "a peek may still show the shapes: {peek}");
}
