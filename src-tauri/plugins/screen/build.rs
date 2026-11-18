// The plugin is driven from Rust only (no frontend commands); the build links
// its Android library into the app's Gradle project.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).android_path("android").build();
}
