//! P12, the reader: a page picture goes to Gemini once per source language,
//! its Korean comes back as shapes on the picture, and it is kept in the
//! library like any other book, its export included.

mod support;

use realtime_screen_translator::api::{BooksIn, ExportIn, LangIn, PageIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use support::{Dirs, TEST_KEY};

struct Device(PathBuf);

impl Platform for Device {
    fn request_permissions(&self) -> Permissions {
        self.permissions()
    }
    fn permissions(&self) -> Permissions {
        Permissions { capture: false, overlay: false }
    }
    fn start(&self) -> Result<(), String> {
        Ok(())
    }
    fn stop(&self) {}
    fn emit(&self, _: &str, _: Value) {}
    fn export_dir(&self) -> PathBuf {
        self.0.clone()
    }
    // Test stand-in for the device keystore.
    fn seal(&self, data: &[u8]) -> Option<Vec<u8>> {
        Some(data.iter().map(|b| b ^ 0x5a).collect())
    }
    fn unseal(&self, data: &[u8]) -> Option<Vec<u8>> {
        self.seal(data)
    }
}

/// A Gemini stand-in that finds one balloon on any page, recording each request.
struct PageGemini {
    url: String,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl PageGemini {
    fn start() -> PageGemini {
        PageGemini::answering_after(std::time::Duration::ZERO)
    }
    /// Each answer held back for `delay`, so readers sent at once overlap.
    fn answering_after(delay: std::time::Duration) -> PageGemini {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://{}", server.server_addr().to_ip().unwrap());
        let bodies = Arc::new(Mutex::new(vec![]));
        let seen = bodies.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                seen.lock().unwrap().push(serde_json::from_str(&body).unwrap_or(Value::Null));
                std::thread::sleep(delay);
                // The page's text at 20 %..30 % across, 15 %..25 % down, in a balloon around it.
                let text = r#"{"box_2d":[150,200,250,300],"balloon_2d":[50,100,350,500],"src":"HELLO","ko":"안녕"}]}"#;
                let answer = json!({"candidates": [{"content": {"role": "model", "parts": [{"text": text}]}}]});
                let _ = req.respond(tiny_http::Response::from_string(answer.to_string()));
            }
        });
        PageGemini { url, bodies }
    }
    fn count(&self) -> usize {
        self.bodies.lock().unwrap().len()
    }
    fn last(&self) -> Value {
        self.bodies.lock().unwrap().last().cloned().unwrap()
    }
}

fn page() -> Vec<u8> {
    std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page.jpg")).unwrap()
}

fn read(b: &Backend, image: Vec<u8>) -> Result<realtime_screen_translator::api::PageOut, String> {
    let req = PageIn { src: "https://cdn.example/1.jpg".into(), image: Some(image), page_url: "https://manga.example/c/1".into(), title: "Kurumi - Chapter 54".into(), index: 3 };
    b.read_page(req).map_err(|e| e.0.to_string())
}

#[test]
fn a_page_is_translated_once_per_language_and_kept_as_a_book() {
    let dirs = Dirs::new();
    let gemini = PageGemini::start();
    let b = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    assert_eq!(read(&b, page()).err().as_deref(), Some("key_missing"), "read without a key");
    assert_eq!(gemini.count(), 0);
    b.put_key(realtime_screen_translator::api::KeyIn { key: TEST_KEY.into() }).unwrap();

    let first = read(&b, page()).unwrap();
    assert_eq!((first.width, first.height, first.saved), (600, 800, false));
    assert_eq!(first.boxes.len(), 1);
    let s = &first.boxes[0];
    assert_eq!(s.ko, "안녕");
    // Over its text (120..180, 120..200) and inside its balloon (60..300, 40..280).
    assert!(s.x <= 120 && s.y <= 120 && s.x + s.w as i64 >= 180 && s.y + s.h as i64 >= 200, "{s:?}");
    assert!(s.x >= 60 && s.y >= 40 && s.x + s.w as i64 <= 300 && s.y + s.h as i64 <= 280, "{s:?}");
    // R9, R10: no safety filter, and the answer's start written in.
    let body = gemini.last();
    assert!(body["safetySettings"].as_array().unwrap().iter().all(|s| s["threshold"] == "OFF"), "{body}");
    assert_eq!(body["contents"][1]["role"], "model");
    assert_eq!(body["contents"][1]["parts"][0]["text"], "{\"items\":[");
    assert!(body["contents"][0]["parts"][0]["text"].as_str().unwrap().contains("Japanese dialogue"));
    assert_eq!(body["contents"][0]["parts"][1]["inline_data"]["mime_type"], "image/jpeg");

    // The same picture again, even fetched anew: from the library, no request.
    let again = read(&b, page()).unwrap();
    assert!(again.saved);
    assert_eq!(again.boxes, first.boxes);
    assert_eq!(gemini.count(), 1);
    assert_eq!(b.status().gemini_requests, 1);

    // English is another reading of the same picture.
    b.put_source_lang(LangIn { lang: "en".into() }).unwrap();
    assert_eq!(b.status().source_lang, "en");
    assert!(!read(&b, page()).unwrap().saved);
    assert!(gemini.last()["contents"][0]["parts"][0]["text"].as_str().unwrap().contains("English dialogue"));
    assert_eq!(gemini.count(), 2);
    assert!(b.put_source_lang(LangIn { lang: "fr".into() }).is_err());

    // Kept as one book named after the site page, exported with the Korean on it.
    let books = b.books(BooksIn { limit: 20, before: None }).unwrap().books;
    assert_eq!(books.len(), 1, "both readings belong to the page's one book");
    assert_eq!(books[0].title, "Kurumi - Chapter 54");
    assert_eq!(books[0].pictures.len(), 2);
    let out = b.export(ExportIn { book_id: books[0].id, format: "png".into() }).unwrap();
    assert_eq!(out.count, 2);
    let png = realtime_screen_translator::export::decode_png(&std::fs::read(&out.files[0]).unwrap()).unwrap();
    assert_eq!((png.w, png.h), (600, 800));
    // The shape is white where the page's "text" was dark.
    let at = |x: usize, y: usize| png.px[(y * png.w as usize + x) * 3];
    assert!(at(150, 112) > 200, "the text under the shape still shows");

    assert_eq!(read(&b, b"not a picture".to_vec()).err().as_deref(), Some("bad_image"));
    assert_eq!(gemini.count(), 2);
}

fn keyed(gemini: &PageGemini, dirs: &Dirs) -> Arc<Backend> {
    let b = Backend::open(dirs.data.clone(), Some(gemini.url.clone()), Arc::new(Device(dirs.export.clone()))).unwrap();
    b.put_key(realtime_screen_translator::api::KeyIn { key: TEST_KEY.into() }).unwrap();
    Arc::new(b)
}

#[test]
fn copies_of_one_page_read_at_once_send_it_to_gemini_once() {
    // Each answer takes long enough that every copy is asked for while the
    // first is still with Gemini.
    let dirs = Dirs::new();
    let gemini = PageGemini::answering_after(std::time::Duration::from_millis(400));
    let b = keyed(&gemini, &dirs);
    let copies: Vec<_> = (0..4)
        .map(|_| {
            let b = b.clone();
            std::thread::spawn(move || read(&b, page()).unwrap())
        })
        .collect();
    let answers: Vec<_> = copies.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(gemini.count(), 1, "one page, one request");
    assert_eq!(b.status().gemini_requests, 1);
    assert!(answers.iter().all(|a| a.boxes == answers[0].boxes && a.save_error.is_none()));
    assert_eq!(answers.iter().filter(|a| !a.saved).count(), 1, "the others came from the library");
    let books = b.books(BooksIn { limit: 20, before: None }).unwrap().books;
    assert_eq!(books[0].pictures.len(), 1, "kept once");
}

#[test]
fn a_page_whose_saving_fails_says_so_and_is_saved_by_the_next_call_without_gemini() {
    let dirs = Dirs::new();
    let gemini = PageGemini::start();
    let b = keyed(&gemini, &dirs);
    // A file where the page folders go: the page cannot be kept.
    let blocker = dirs.data.join("strips");
    std::fs::write(&blocker, b"startup.rc").unwrap();

    let failed = read(&b, page()).unwrap();
    assert_eq!(failed.boxes.len(), 1, "the translation is still shown");
    assert!(matches!(failed.save_error, Some("storage_full" | "storage_denied" | "internal")), "{:?}", failed.save_error);
    assert!(b.books(BooksIn { limit: 20, before: None }).unwrap().books.is_empty(), "nothing half kept");

    // Still blocked: told again, still no second request.
    assert!(read(&b, page()).unwrap().save_error.is_some());
    assert_eq!(gemini.count(), 1);

    std::fs::remove_file(&blocker).unwrap();
    let kept = read(&b, page()).unwrap();
    assert_eq!((kept.save_error, &kept.boxes), (None, &failed.boxes));
    assert_eq!(gemini.count(), 1, "saving again asks Gemini nothing");
    let books = b.books(BooksIn { limit: 20, before: None }).unwrap().books;
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].pictures.len(), 1);
    assert!(read(&b, page()).unwrap().saved, "from the library from now on");
}
