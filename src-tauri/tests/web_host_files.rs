//! The browser host (`web-host`) gives the reader the files the API names:
//! book previews and exported pictures, and nothing else from its folders.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A web host on a free port with its own data folder; stopped on drop.
struct Host {
    child: Child,
    port: u16,
    data: PathBuf,
    root: PathBuf,
}

impl Host {
    fn start(name: &str) -> Host {
        let root = std::env::temp_dir().join(format!("rst-web-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data = root.join("data");
        let dist = root.join("dist");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<!doctype html><title>screens</title>").unwrap();
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let child = Command::new(env!("CARGO_BIN_EXE_web-host"))
            .env("PORT", port.to_string())
            .env("APP_DATA_DIR", &data)
            .env("DIST_DIR", &dist)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let host = Host { child, port, data, root };
        let deadline = Instant::now() + Duration::from_secs(30);
        while host.try_get("/status").map(|r| r.status) != Some(200) {
            assert!(Instant::now() < deadline, "web host did not answer");
            thread::sleep(Duration::from_millis(50));
        }
        host
    }

    /// One request sent byte for byte, so the path reaches the host unchanged.
    fn send(&self, method: &str, path: &str, body: &[u8]) -> Option<Reply> {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(30))).ok()?;
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        s.write_all(head.as_bytes()).ok()?;
        s.write_all(body).ok()?;
        let mut raw = vec![];
        s.read_to_end(&mut raw).ok()?;
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let status = head.split(' ').nth(1)?.parse().ok()?;
        Some(Reply { status, head, body: raw[split + 4..].to_vec() })
    }

    fn try_get(&self, path: &str) -> Option<Reply> {
        self.send("GET", path, b"")
    }

    fn get(&self, path: &str) -> Reply {
        self.try_get(path).expect("no answer")
    }

    fn call(&self, method: &str, path: &str, body: &serde_json::Value) -> serde_json::Value {
        let raw = if body.is_null() { vec![] } else { body.to_string().into_bytes() };
        let r = self.send(method, path, &raw).expect("no answer");
        assert_eq!(r.status, 200, "{method} {path}: {}", String::from_utf8_lossy(&r.body));
        serde_json::from_slice(&r.body).unwrap()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

/// Fills the host the way `e2e/seed.json` does: two books, each with a page.
fn seed(host: &Host) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../e2e/seed.json");
    let calls: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for c in calls {
        host.call(c["method"].as_str().unwrap(), c["path"].as_str().unwrap(), &c["body"]);
    }
}

/// The address the reader fetches an exported file from: its part from `export/` on.
fn link(file: &str) -> String {
    let file = file.replace('\\', "/");
    format!("/files/{}", &file[file.rfind("/export/").expect("export folder") + 1..])
}

#[test]
fn a_browser_export_can_be_fetched_by_the_reader_in_each_format() {
    let host = Host::start("export");
    seed(&host);
    for (format, magic) in [("png", &b"\x89PNG"[..]), ("webp", &b"RIFF"[..])] {
        let out = host.call("POST", "/books/1/export", &serde_json::json!({ "format": format }));
        let files: Vec<&str> = out["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        assert_eq!(files.len() as u64, out["count"].as_u64().unwrap());
        assert!(!files.is_empty());
        for f in files {
            let r = host.get(&link(f));
            assert_eq!(r.status, 200, "{format} export not served");
            assert!(r.body.starts_with(magic), "{format} export is not the picture");
            assert_eq!(r.body, std::fs::read(f).unwrap(), "{format} export differs from the file written");
            let head = r.head.to_ascii_lowercase();
            assert!(head.contains("content-disposition: attachment"), "{format} export is not offered as a download");
        }
    }
}

#[test]
fn the_browser_host_never_reports_a_capture_it_cannot_do() {
    let host = Host::start("capture");
    let s = host.call("GET", "/status", &serde_json::Value::Null);
    assert_eq!((s["capture_supported"].clone(), s["capture_allowed"].clone(), s["overlay_allowed"].clone()), (false.into(), false.into(), false.into()), "{s}");
    host.call("PUT", "/settings/gemini-key", &serde_json::json!({ "key": "test_key_ABCDEFGHIJKLMNOP" }));
    let p = host.call("POST", "/permissions/request", &serde_json::Value::Null);
    assert_eq!((p["capture_allowed"].clone(), p["overlay_allowed"].clone()), (false.into(), false.into()), "permissions are not granted: {p}");
    let r = host.send("POST", "/translation/start", b"").unwrap();
    assert_eq!((r.status, String::from_utf8_lossy(&r.body).to_string()), (409, r#"{"error":"capture_unsupported"}"#.to_string()));
    assert_eq!(host.call("GET", "/status", &serde_json::Value::Null)["on"], false, "translation is not reported on");

    // Only the stand-in device the e2e seed attaches makes it act as a capturing phone.
    host.call("POST", "/test/simulated-device", &serde_json::Value::Null);
    assert_eq!(host.call("GET", "/status", &serde_json::Value::Null)["capture_supported"], true);
    assert_eq!(host.call("POST", "/permissions/request", &serde_json::Value::Null)["capture_allowed"], true);
    assert_eq!(host.call("POST", "/translation/start", &serde_json::Value::Null)["on"], true);
}

#[test]
fn the_files_route_serves_previews_and_exports_and_denies_every_other_form() {
    let host = Host::start("files");
    let planted = b"planted-not-for-the-reader";
    for rel in ["startup.rc", "strips/startup.rc.d/x", "../outside/target.txt"] {
        let p = host.data.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, planted).unwrap();
    }
    std::fs::create_dir_all(host.data.join("strips/1")).unwrap();
    std::fs::write(host.data.join("strips/1/0-1.webp"), b"RIFF-preview").unwrap();
    std::fs::create_dir_all(host.data.join("export")).unwrap();
    std::fs::write(host.data.join("export/page-1.png"), b"\x89PNG-export").unwrap();

    for (path, body) in [("/files/strips/1/0-1.webp", &b"RIFF-preview"[..]), ("/files/export/page-1.png", &b"\x89PNG-export"[..])] {
        let r = host.get(path);
        assert_eq!((r.status, r.body.as_slice()), (200, body), "allowed form {path}");
    }

    // Rule: only a plain relative path inside `strips/` or `export/` is served.
    let denied = [
        "/files/startup.rc",
        "/files//startup.rc",
        "/files/strips/../startup.rc",
        "/files/export/../startup.rc",
        "/files/export/../../outside/target.txt",
        "/files/../outside/target.txt",
        "/files/export/..%2Fstartup.rc",
        "/files/export/%2e%2e/startup.rc",
        "/files/export\\..\\startup.rc",
        "/files/strips\\..\\..\\outside\\target.txt",
        "/files/C:/outside/target.txt",
        "/files/translator.sqlite3",
        "/files/export",
        "/files/strips/",
    ];
    for path in denied {
        let r = host.get(path);
        assert_eq!(r.status, 404, "form {path} was served");
        assert!(!r.body.windows(planted.len()).any(|w| w == planted), "form {path} leaked a planted file");
    }
}
