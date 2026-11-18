//! Edge case of the forty-third review, checked on the Kotlin sources (no
//! Android toolchain runs here): hiding the button or the shapes waits for
//! the main thread half a second at most, and a hide it had not started by
//! then is called off, so a late main thread never leaves them hidden with
//! nobody to show them again.

use std::path::Path;

fn overlay() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/screen/android/src/main/java/Overlay.kt")).unwrap().replace("\r\n", "\n")
}

/// The text of `fun name(` in `src`, up to its closing brace at four spaces
/// (or the end of its line, for a one-expression body).
fn body<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src.find(&format!("fun {name}(")).unwrap_or_else(|| panic!("no fun {name}"));
    let rest = &src[at..];
    &rest[..rest.find("\n    }\n").unwrap()]
}

#[test]
fn a_hide_the_main_thread_comes_to_late_is_called_off() {
    let src = overlay();
    let wait = body(&src, "hideOnMain");
    let (run, timeout, off) = (
        wait.find("if (state.compareAndSet(WAITING, RUNNING)) at = hide()").expect("the hide runs without checking it was called off"),
        wait.find("if (done.await(500, java.util.concurrent.TimeUnit.MILLISECONDS)) return at").expect("no bounded wait"),
        wait.find("if (state.compareAndSet(WAITING, CALLED_OFF)) return null").expect("a timed-out hide is not called off"),
    );
    assert!(run < timeout && timeout < off);
    // One that started just as the wait ended is waited for: what it hid is
    // answered, so the caller shows it again.
    assert!(wait[off..].contains("done.await()\n        return at"));
    for name in ["hideButtonNow", "hideShapesNow"] {
        assert!(body(&src, name).starts_with(&format!("fun {name}(): Long? = hideOnMain {{")), "{name} does not go through hideOnMain");
    }
}
