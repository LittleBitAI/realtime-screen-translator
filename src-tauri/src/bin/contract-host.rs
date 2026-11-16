//! Drives the backend over stdin/stdout for the contract tests, with the
//! platform layer faked from the environment. The protocol is described in
//! `tests/support/mod.rs`.

use realtime_screen_translator::api::*;
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::{vault, Backend};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Overlay {
    open: bool,
    click_through: bool,
}

struct FakePlatform {
    windows: bool,
    answers: Permissions,
    granted: Mutex<Permissions>,
    overlay: Mutex<Overlay>,
    data_dir: PathBuf,
    export_dir: PathBuf,
}

impl Platform for FakePlatform {
    fn request_permissions(&self) -> Permissions {
        if self.windows {
            return self.permissions();
        }
        *self.granted.lock().unwrap() = self.answers;
        self.answers
    }

    fn permissions(&self) -> Permissions {
        if self.windows {
            return Permissions { capture: true, overlay: true };
        }
        *self.granted.lock().unwrap()
    }

    fn start(&self) -> Result<(), String> {
        // Touches go through to the app underneath (A7: click-through window on Windows).
        *self.overlay.lock().unwrap() = Overlay { open: true, click_through: true };
        Ok(())
    }

    fn stop(&self) {
        *self.overlay.lock().unwrap() = Overlay::default();
    }

    fn emit(&self, event: &str, payload: Value) {
        let line = json!({"event": event, "payload": payload}).to_string();
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    fn export_dir(&self) -> PathBuf {
        self.export_dir.clone()
    }

    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        if cfg!(windows) {
            vault::seal(data)
        } else {
            fake_keystore::seal(data, &self.data_dir)
        }
    }

    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        if cfg!(windows) {
            vault::unseal(data)
        } else {
            fake_keystore::unseal(data, &self.data_dir)
        }
    }
}

/// Test-only stand-in for a device keystore where DPAPI is missing (CI on
/// Linux): ChaCha20-Poly1305 under a random key in the fake device folder.
#[cfg(not(windows))]
mod fake_keystore {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    use std::path::Path;

    fn device_key(dir: &Path) -> Option<[u8; 32]> {
        let path = dir.join("fake-keystore.key");
        if let Ok(b) = std::fs::read(&path) {
            return b.try_into().ok();
        }
        let mut k = [0u8; 32];
        getrandom::getrandom(&mut k).ok()?;
        std::fs::create_dir_all(dir).ok()?;
        std::fs::write(&path, k).ok()?;
        Some(k)
    }

    pub fn seal(data: &[u8], dir: &Path) -> Option<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key(dir)?));
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).ok()?;
        let mut out = nonce.to_vec();
        out.extend(cipher.encrypt(Nonce::from_slice(&nonce), data).ok()?);
        Some(out)
    }

    pub fn unseal(data: &[u8], dir: &Path) -> Option<Vec<u8>> {
        if data.len() < 12 {
            return None;
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key(dir)?));
        cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()
    }
}

#[cfg(windows)]
mod fake_keystore {
    use std::path::Path;
    pub fn seal(_: &[u8], _: &Path) -> Option<Vec<u8>> {
        None
    }
    pub fn unseal(_: &[u8], _: &Path) -> Option<Vec<u8>> {
        None
    }
}

fn args<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, ApiError> {
    serde_json::from_value(v).map_err(|e| {
        eprintln!("bad arguments: {e}");
        ApiError("bad_request")
    })
}

fn ok<T: serde::Serialize>(r: Result<T, ApiError>) -> Result<Value, ApiError> {
    r.map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
}

fn dispatch(b: &Backend, platform: &FakePlatform, cmd: &str, a: Value) -> Result<Value, ApiError> {
    match cmd {
        "get_status" => ok(Ok(b.status())),
        "put_settings_gemini_key" => ok(b.put_key(args(a)?)),
        "post_permissions_request" => ok(Ok(b.request_permissions())),
        "post_translation_start" => ok(b.start()),
        "post_translation_stop" => ok(b.stop()),
        "post_frames" => ok(b.frame(args(a)?)),
        "post_crops" => ok(b.crops(args(a)?)),
        "get_placements" => ok(b.placements(args(a).unwrap_or_default())),
        "post_strips" => ok(b.strip(args(a)?)),
        "get_books" => ok(b.books(args(a)?)),
        "post_books_export" => ok(b.export(args(a)?)),
        "__overlay" => {
            let o = platform.overlay.lock().unwrap();
            Ok(json!({"open": o.open, "click_through": o.click_through}))
        }
        _ => Err(ApiError("unknown_command")),
    }
}

fn env_path(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name).map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join(default))
}

fn main() {
    let grant = |name: &str| std::env::var(name).map(|v| v == "grant").unwrap_or(true);
    let data_dir = env_path("RST_DATA_DIR", "rst-data");
    let platform = Arc::new(FakePlatform {
        windows: std::env::var("RST_PLATFORM").map(|p| p == "windows").unwrap_or(false),
        answers: Permissions { capture: grant("RST_FAKE_CAPTURE_PERMISSION"), overlay: grant("RST_FAKE_OVERLAY_PERMISSION") },
        granted: Mutex::new(Permissions::default()),
        overlay: Mutex::new(Overlay::default()),
        data_dir: data_dir.clone(),
        export_dir: env_path("RST_EXPORT_DIR", "rst-export"),
    });
    let backend = match Backend::open(data_dir, std::env::var("RST_GEMINI_BASE_URL").ok(), platform.clone()) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot open the backend: {e}");
            std::process::exit(1);
        }
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("not JSON: {e}");
                continue;
            }
        };
        let id = msg["id"].clone();
        let cmd = msg["cmd"].as_str().unwrap_or("");
        let reply = match dispatch(&backend, &platform, cmd, msg.get("args").cloned().unwrap_or(json!({}))) {
            Ok(v) => json!({"id": id, "ok": v}),
            Err(e) => json!({"id": id, "err": e.0}),
        };
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
}
