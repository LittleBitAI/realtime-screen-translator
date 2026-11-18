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
            .env("RST_GEMINI_BASE_URL", format!("http://127.0.0.1:{port}"))
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
    println!("Export HTTP on 127.0.0.1:{} with fresh temporary data and a loopback model endpoint", host.port);
    seed(&host);
    for (format, magic) in [("png", &b"\x89PNG"[..]), ("webp", &b"RIFF"[..])] {
        let out = host.call("POST", "/books/1/export", &serde_json::json!({ "format": format }));
        let files: Vec<&str> = out["files"].as_array().unwrap().iter().map(|f| f.as_str().unwrap()).collect();
        assert_eq!(out["count"], 1, "{format} reported file count");
        assert_eq!(files.len(), 1, "{format} returned file count");
        for f in &files {
            let r = host.get(&link(f));
            assert_eq!(r.status, 200, "{format} export not served");
            assert!(r.body.starts_with(magic), "{format} export is not the picture");
            if format == "webp" {
                assert_eq!(r.body.get(8..12), Some(&b"WEBP"[..]));
            }
            assert_eq!(r.body, std::fs::read(f).unwrap(), "{format} export differs from the file written");
            let head = r.head.to_ascii_lowercase();
            assert!(head.contains("content-disposition: attachment"), "{format} export is not offered as a download");
        }
        println!("POST /books/1/export ({format}) and GET the returned files: expected 200 and matching {format} downloads, observed 200 and {} matching files", files.len());
    }
    assert_eq!(host.call("GET", "/status", &serde_json::Value::Null)["gemini_requests"], 0);
}

#[test]
fn saved_list_memberships_preserve_the_book_through_the_http_api() {
    use serde_json::{json, Value};

    let host = Host::start("saved-lists");
    println!("Saved-list HTTP on 127.0.0.1:{} with fresh temporary data and a loopback model endpoint", host.port);
    let get = |path: &str| host.call("GET", path, &Value::Null);
    let edit = |body| host.call("POST", "/library/edit", &body);
    assert_eq!(get("/lists"), json!([]));
    assert_eq!(get("/books")["books"], json!([]));
    println!("GET /lists and /books: expected 200 and empty arrays, observed 200 and empty arrays");

    let address = "https://example.org/chapter?q=paper&part=2";
    edit(json!({"action":"save_link", "title":"Paper Moon", "url":address}));
    let books = get("/books?search=Moon")["books"].as_array().unwrap().clone();
    assert_eq!(books.len(), 1);
    let book = books[0]["id"].as_i64().unwrap();
    assert_eq!(books[0]["title"], "Paper Moon");
    assert_eq!(books[0]["url"], address);
    println!("POST /library/edit (save link) and GET /books by title: expected 200 and one matching title and address, observed 200 and one exact match");
    for name in ["Later", "Favorites"] {
        edit(json!({"action":"create", "name":name}));
    }
    let lists = get("/lists");
    let later = lists[0]["id"].as_i64().unwrap();
    let favorites = lists[1]["id"].as_i64().unwrap();
    assert_eq!(get(&format!("/books?list={later}"))["books"], json!([]));
    println!("GET /books for the newly created list: expected 200 and zero titles, observed 200 and zero titles");
    for list in [later, favorites] {
        edit(json!({"action":"add", "list":list, "book":book}));
    }
    for list in [later, favorites] {
        assert_eq!(get(&format!("/books?list={list}"))["books"], json!(books));
    }
    println!("GET /books for each populated list: expected 200 and the saved book, observed 200 and the same book in both lists");
    edit(json!({"action":"remove", "list":later, "book":book}));
    assert_eq!(get(&format!("/books?list={later}"))["books"], json!([]));
    edit(json!({"action":"delete", "list":later}));
    assert_eq!(get("/lists"), json!([{"id":favorites,"name":"Favorites","entries":[book]}]));
    for term in ["Paper%20Moon", "q%3Dpaper%26part%3D2"] {
        let found = get(&format!("/books?list={favorites}&search={term}"));
        assert_eq!(found["books"], json!(books));
    }
    println!("POST /library/edit (add, remove, delete), then GET /lists and /books by title and address: expected 200, one remaining membership and the original book, observed 200 and one exact match for both searches");
    assert_eq!(get("/books?search=unlisted")["books"], json!([]));
    println!("GET /books for an unmatched query: expected 200 and zero titles, observed 200 and zero titles");
    assert_eq!(get("/status")["gemini_requests"], 0);
    println!("GET /status: expected 200 and zero model requests, observed 200 and zero model requests");
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
