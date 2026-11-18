//! Edge cases of the nineteenth review, checked on the sources (no Android
//! toolchain runs here): every plugin command the Rust side calls is a
//! command of the Kotlin plugin, none of them hides a command of Tauri's
//! `Plugin` (which does not compile without `override`), and the capture
//! prompt asks for the whole screen, not a single app.

use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap().replace("\r\n", "\n")
}

/// The command names in `call::<T>(&x, "name"` on the Rust side.
fn called(app: &str) -> Vec<String> {
    let mut out = vec![];
    for (i, _) in app.match_indices("call::<") {
        let rest = &app[i..];
        let Some(open) = rest.find(", \"") else { continue };
        if rest[..open].contains('\n') {
            continue;
        }
        let name = &rest[open + 3..];
        out.push(name[..name.find('"').unwrap()].to_string());
    }
    out
}

#[test]
fn every_command_rust_calls_is_a_plugin_command_and_none_hides_tauris() {
    let app = read("src/app.rs");
    let kt = read("plugins/screen/android/src/main/java/ScreenPlugin.kt");
    let names = called(&app);
    assert!(names.len() >= 10, "the calls were not found: {names:?}");
    for name in &names {
        assert!(kt.contains(&format!("@Command\n    fun {name}(invoke: Invoke)")), "`{name}` is called from Rust but is no command of ScreenPlugin");
    }
    // Commands `app.tauri.plugin.Plugin` already has: declaring one again
    // without `override` does not compile.
    for taken in ["requestPermissions", "checkPermissions", "getPermissionState", "load"] {
        assert!(!kt.contains(&format!("    fun {taken}(")), "ScreenPlugin hides Plugin.{taken}");
    }
}

#[test]
fn the_capture_prompt_asks_for_the_whole_screen() {
    let kt = read("plugins/screen/android/src/main/java/ScreenPlugin.kt");
    assert!(kt.contains("createScreenCaptureIntent(MediaProjectionConfig.createConfigForDefaultDisplay())"), "Android 14+ may offer a single app");
}
