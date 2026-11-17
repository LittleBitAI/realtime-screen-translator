//! The app in a browser: the built screens (`dist/`) and the API of
//! `docs/design/api.yaml` on one port (`PORT`), its data under
//! `APP_DATA_DIR`. The device layer is a stand-in: nothing is captured or
//! drawn over other apps, permissions are granted once asked for (as on a
//! phone that says yes), and exports go to `APP_DATA_DIR/export`. The phone
//! and the PC run the same screens inside the Tauri app (`app.rs`).

use realtime_screen_translator::api::*;
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server};

struct Browser {
    granted: Mutex<Permissions>,
    data_dir: PathBuf,
}

impl Platform for Browser {
    fn request_permissions(&self) -> Permissions {
        let p = Permissions { capture: true, overlay: true };
        *self.granted.lock().unwrap() = p;
        p
    }
    fn permissions(&self) -> Permissions {
        *self.granted.lock().unwrap()
    }
    fn start(&self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&self) {}
    fn emit(&self, _: &str, _: Value) {}
    fn export_dir(&self) -> PathBuf {
        self.data_dir.join("export")
    }
    fn overlay_in_capture(&self) -> bool {
        false
    }
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        #[cfg(windows)]
        return realtime_screen_translator::vault::seal(data);
        #[cfg(not(windows))]
        return realtime_screen_translator::file_vault::seal(data, &self.data_dir);
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        #[cfg(windows)]
        return realtime_screen_translator::vault::unseal(data);
        #[cfg(not(windows))]
        return realtime_screen_translator::file_vault::unseal(data, &self.data_dir);
    }
}

/// The HTTP status an error code of the design is answered with.
fn status_of(code: &str) -> u16 {
    match code {
        "bad_request" | "key_format" | "bad_frame" | "bad_image" | "bad_format" | "stale_seq" | "frame_unknown" => 400,
        "storage_denied" => 403,
        "not_found" => 404,
        "key_missing" | "permission_required" | "not_on" | "already_on" | "picture_closed" | "already_covered" | "empty_book" => 409,
        "storage_full" => 507,
        _ => 500,
    }
}

fn json_response(status: u16, body: Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.to_string().into_bytes())
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "application/json; charset=utf-8").unwrap())
}

fn reply<T: serde::Serialize>(r: ApiResult<T>) -> Response<std::io::Cursor<Vec<u8>>> {
    match r {
        Ok(v) => json_response(200, serde_json::to_value(v).unwrap_or(Value::Null)),
        Err(e) => json_response(status_of(e.0), json!({"error": e.0})),
    }
}

fn body<T: serde::de::DeserializeOwned>(raw: &[u8]) -> ApiResult<T> {
    let raw = if raw.iter().all(u8::is_ascii_whitespace) { b"{}".as_slice() } else { raw };
    serde_json::from_slice(raw).map_err(|e| {
        eprintln!("bad request body: {e}");
        ApiError("bad_request")
    })
}

/// `name=value` pairs of a query string (values are numbers or `null` here).
fn query(q: &str, name: &str) -> Option<String> {
    q.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
}

/// A path under `dir` from the URL path `rel`, never outside it.
fn inside(dir: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel.trim_start_matches('/'));
    rel.components().all(|c| matches!(c, Component::Normal(_))).then(|| dir.join(rel))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn file_response(path: &Path) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    let data = std::fs::read(path).ok()?;
    Some(Response::from_data(data).with_header(Header::from_bytes("Content-Type", content_type(path)).unwrap()))
}

fn api(b: &Backend, method: &Method, path: &str, q: &str, raw: &[u8]) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    use Method::*;
    Some(match (method, path) {
        (Get, "/status") => reply(Ok(b.status())),
        (Put, "/settings/gemini-key") => reply(body(raw).and_then(|k| b.put_key(k))),
        (Post, "/permissions/request") => reply(Ok(b.request_permissions())),
        (Post, "/translation/start") => reply(b.start()),
        (Post, "/translation/stop") => reply(b.stop()),
        (Post, "/frames") => reply(body(raw).and_then(|f| b.frame(f))),
        (Post, "/crops") => reply(body(raw).and_then(|c| b.crops(c))),
        (Get, "/placements") => {
            let since = query(q, "since").and_then(|v| v.parse().ok());
            reply(b.placements(PlacementsIn { since }))
        }
        (Post, "/strips") => reply(body(raw).and_then(|s| b.strip(s))),
        (Get, "/books") => {
            let limit = query(q, "limit").and_then(|v| v.parse().ok()).unwrap_or(20);
            let before = query(q, "before").and_then(|v| v.parse().ok());
            reply(b.books(BooksIn { limit, before }))
        }
        (Post, p) if p.starts_with("/books/") && p.ends_with("/export") => {
            let Some(book_id) = p["/books/".len()..p.len() - "/export".len()].parse().ok() else {
                return Some(json_response(404, json!({"error": "not_found"})));
            };
            let format = body::<Value>(raw).ok().and_then(|v| v["format"].as_str().map(str::to_string)).unwrap_or_else(|| "png".into());
            reply(b.export(ExportIn { book_id, format }))
        }
        _ => return None,
    })
}

fn handle(b: &Backend, dist: &Path, data_dir: &Path, mut req: Request) {
    let url = req.url().to_string();
    let (path, q) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.to_string();
    let mut raw = vec![];
    let _ = req.as_reader().read_to_end(&mut raw);
    let method = req.method().clone();
    let response = if let Some(r) = api(b, &method, &path, q, &raw) {
        r
    } else if method != Method::Get && method != Method::Head {
        json_response(405, json!({"error": "method_not_allowed"}))
    } else if let Some(rel) = path.strip_prefix("/files/") {
        // The files the API names, from the app's own folder only: book
        // previews (`preview_path`, under `strips/`) and exported pictures
        // (`files` of an export, under `export/`), the latter as downloads,
        // since in a browser the reader has no other way to get them.
        let file = inside(data_dir, rel).filter(|p| p.is_file());
        let response = match file {
            Some(p) if p.starts_with(data_dir.join("strips")) => file_response(&p),
            Some(p) if p.starts_with(data_dir.join("export")) => file_response(&p).map(|r| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("picture");
                r.with_header(Header::from_bytes("Content-Disposition", format!("attachment; filename=\"{name}\"")).unwrap())
            }),
            _ => None,
        };
        response.unwrap_or_else(|| json_response(404, json!({"error": "not_found"})))
    } else {
        // A built file, or a screen route (`/`, `/library`, `/options`):
        // every route loads the app, which shows the screen for its path.
        let file = inside(dist, &path).filter(|p| p.is_file());
        match file.or_else(|| Some(dist.join("index.html"))).and_then(|p| file_response(&p)) {
            Some(r) => r,
            None => json_response(404, json!({"error": "not_built"})),
        }
    };
    if let Err(e) = req.respond(response) {
        eprintln!("cannot answer {path}: {e}");
    }
}

fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    let data_dir = std::env::var_os("APP_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("rst-web"));
    let dist = std::env::var_os("DIST_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist"));
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("cannot make {}: {e}", data_dir.display());
        std::process::exit(1);
    }
    // `absolute`, not `canonicalize`: no `\\?\` before the paths the screens show.
    let data_dir = std::path::absolute(&data_dir).unwrap_or(data_dir);
    let platform = Arc::new(Browser { granted: Mutex::new(Permissions::default()), data_dir: data_dir.clone() });
    let backend = match Backend::open(data_dir.clone(), std::env::var("RST_GEMINI_BASE_URL").ok(), platform) {
        Ok(b) => Arc::new(b),
        Err(e) => {
            eprintln!("cannot open the backend: {e}");
            std::process::exit(1);
        }
    };
    let server = match Server::http(("0.0.0.0", port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot listen on port {port}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("listening on http://localhost:{port}");
    let (dist, data_dir) = (Arc::new(dist), Arc::new(data_dir));
    for req in server.incoming_requests() {
        let (b, dist, data_dir) = (backend.clone(), dist.clone(), data_dir.clone());
        std::thread::spawn(move || handle(&b, &dist, &data_dir, req));
    }
}
