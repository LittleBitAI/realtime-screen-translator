//! P12, the reader: a page picture goes to Gemini once per source language,
//! its Korean comes back as shapes on the picture, and it is kept in the
//! library like any other book, its export included.

mod support;

use realtime_screen_translator::api::{BooksIn, ExportIn, LangIn, LibraryEdit, PageIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use serde_json::{json, Value};
use std::fs;
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
                let ko = if body.contains("English dialogue") { "반가워" } else { "안녕" };
                seen.lock().unwrap().push(serde_json::from_str(&body).unwrap_or(Value::Null));
                std::thread::sleep(delay);
                // The page's text at 20 %..30 % across, 15 %..25 % down, in a balloon around it.
                let text = json!({"box_2d":[150,200,250,300],"balloon_2d":[50,100,350,500],"src":"HELLO","ko":ko}).to_string() + "]}";
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
    assert_eq!(books[0].url.as_deref(), Some("https://manga.example/c/1"));
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
fn imported_chapter_survives_restart_and_shares_the_url_cache_by_language() {
    let dirs = Dirs::new();
    let gemini = PageGemini::start();
    let b = keyed(&gemini, &dirs);
    let folder = dirs.data.with_file_name("Fixture");
    fs::create_dir_all(folder.join("nested")).unwrap();
    fs::write(folder.join("nested/3.jpg"), page()).unwrap();
    fs::write(folder.join("notes.txt"), "skip").unwrap();
    let images: Vec<_> = [1, 2, 10].into_iter().map(|n| {
        let mut bytes = page();
        bytes.push(n);
        fs::write(folder.join(format!("{n}.jpg")), &bytes).unwrap();
        bytes
    }).collect();

    let book = b.import_folder(&folder).unwrap();
    let pages = |b: &Backend| b.chapter(book).unwrap().pages.into_iter().map(|p| p.id).collect::<Vec<_>>();
    let [one, two, ten] = pages(&b).try_into().unwrap();
    assert_eq!(gemini.count(), 0, "import alone sends nothing");
    assert_eq!(b.books(BooksIn { limit: 20, before: None }).unwrap().books[0].title, "Fixture");
    let from_url = read(&b, images[1].clone()).unwrap();
    assert_eq!(gemini.count(), 1);
    drop(b);

    fs::rename(&folder, folder.with_file_name("moved")).unwrap();
    let b = keyed(&gemini, &dirs);
    assert_eq!(pages(&b), [one, two, ten]);
    for (id, bytes) in [one, two, ten].into_iter().zip(images) {
        assert_eq!(b.chapter_image(id).unwrap(), bytes);
    }
    assert_eq!(gemini.count(), 1, "opening stored images sends nothing");
    let first = b.read_chapter_page(one).unwrap();
    assert_eq!((first.saved, first.boxes[0].ko.as_str()), (false, "안녕"));
    assert_eq!(gemini.count(), 2);
    let cached = b.read_chapter_page(two).unwrap();
    assert!(cached.saved);
    assert_eq!(cached.boxes, from_url.boxes);
    assert_eq!(gemini.count(), 2, "cache hit from a site page");

    for name in ["Read later", "Favorites"] {
        b.edit_library(LibraryEdit::Create { name: name.into() }).unwrap();
    }
    let lists = b.lists().unwrap();
    for list in &lists {
        b.edit_library(LibraryEdit::Add { list: list.id, book }).unwrap();
    }
    b.edit_library(LibraryEdit::Remove { list: lists[0].id, book }).unwrap();
    b.edit_library(LibraryEdit::Delete { list: lists[0].id }).unwrap();
    let kept = b.browse(BooksIn { limit: 1, before: None }, Some(lists[1].id), "fix").unwrap();
    assert_eq!(kept.books[0].id, book);
    assert_eq!(gemini.count(), 2);

    b.put_source_lang(LangIn { lang: "en".into() }).unwrap();
    assert_eq!(b.read_chapter_page(one).unwrap().boxes[0].ko, "반가워");
    assert_eq!(gemini.count(), 3);
    b.put_source_lang(LangIn { lang: "ja".into() }).unwrap();
    drop(b);
    let b = keyed(&gemini, &dirs);
    assert_eq!(b.read_chapter_page(one).unwrap().boxes, first.boxes);
    assert!(read(&b, b.chapter_image(one).unwrap()).unwrap().saved);
    assert_eq!(gemini.count(), 3, "both readers reuse the imported page's persisted cache");

    let out = b.export(ExportIn { book_id: book, format: "png".into() }).unwrap();
    assert_eq!(out.count, 3);
    let png = realtime_screen_translator::export::decode_png(&fs::read(&out.files[1]).unwrap()).unwrap();
    let original = realtime_screen_translator::export::decode_strip(&page()).unwrap();
    let pixel = |x, y| (y * 600usize + x) * 3;
    assert!(png.px[pixel(150, 112)] > 200, "the cached dialogue is overlaid in the export");
    assert_eq!(png.px[pixel(550, 400)], original.px[pixel(550, 400)]);
}

#[test]
fn saved_links_share_book_identity_with_the_reader_and_lists_keep_their_order() {
    use LibraryEdit::*;
    let dirs = Dirs::new();
    let gemini = PageGemini::start();
    let b = keyed(&gemini, &dirs);
    let first = "https://manga.example/c/1";
    for (url, title) in [(first, "One"), ("manga.example/c/2?q=50%25", "Two"), (first, "One")] {
        b.edit_library(SaveLink { url: url.into(), title: title.into() }).unwrap();
    }
    let cursor = |before| BooksIn { limit: 1, before };
    let one = b.browse(cursor(None), None, "ONE").unwrap().books[0].id;
    let two = b.browse(cursor(None), None, "50%25").unwrap().books[0].id;
    assert_ne!(one, two);
    for name in ["A", "B"] {
        b.edit_library(Create { name: name.into() }).unwrap();
    }
    let lists = b.lists().unwrap();
    let (a, b_id) = (lists[0].id, lists[1].id);
    for book in [one, two, two] {
        b.edit_library(Add { list: a, book }).unwrap();
    }
    b.edit_library(MoveEntry { list: a, book: two, up: true }).unwrap();
    b.edit_library(MoveList { list: b_id, up: true }).unwrap();
    b.edit_library(Rename { list: a, name: "Queue".into() }).unwrap();
    assert_eq!(gemini.count(), 0);
    drop(b);
    let b = keyed(&gemini, &dirs);
    let lists = b.lists().unwrap();
    assert_eq!((lists[0].id, lists[1].name.as_str()), (b_id, "Queue"));
    assert_eq!(lists[1].entries, [two, one]);
    let next = b.browse(cursor(None), Some(a), "").unwrap();
    assert_eq!(next.books[0].id, two);
    let last = b.browse(cursor(next.next_before), Some(a), "").unwrap();
    assert_eq!((last.books[0].url.as_deref(), last.next_before), (Some(first), None));
    assert_eq!(gemini.count(), 0);

    read(&b, page()).unwrap();
    let books = b.books(BooksIn { limit: 20, before: None }).unwrap().books;
    assert_eq!(books.len(), 2);
    let translated = books.iter().find(|book| book.id == one).unwrap();
    assert_eq!((translated.title.as_str(), translated.pictures.len()), ("One", 1));
    assert!(read(&b, page()).unwrap().saved);
    assert_eq!(gemini.count(), 1);
}

#[test]
fn empty_and_failed_imports_leave_no_chapter_or_copied_files() {
    let dirs = Dirs::new();
    let gemini = PageGemini::start();
    let b = keyed(&gemini, &dirs);
    let folder = dirs.data.with_file_name("Fixture");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("notes.txt"), "skip").unwrap();
    assert_eq!(b.import_folder(&folder).unwrap_err().0, "empty_folder");

    let pages: Vec<_> = ["1.jpg", "2.jpg"].into_iter().map(|name| {
        let path = folder.join(name);
        fs::write(&path, page()).unwrap();
        (name.to_owned(), path)
    }).collect();
    let originals = pages.clone();
    let err = b.import_chapter("Fixture", pages, |source, dest| {
        fs::copy(&source, dest).unwrap();
        if source.ends_with("2.jpg") {
            return Err(realtime_screen_translator::api::ApiError("storage_full"));
        }
        Ok(())
    }).unwrap_err();
    assert_eq!(err.0, "storage_full");
    assert!(b.books(BooksIn { limit: 20, before: None }).unwrap().books.is_empty());
    assert_eq!(fs::read_dir(dirs.data.join("strips")).unwrap().count(), 0);
    for (_, path) in originals {
        assert_eq!(fs::read(path).unwrap(), page());
    }
    assert_eq!(gemini.count(), 0);
}

#[test]
fn oversize_pages_are_rejected_across_formats_before_a_chapter_is_published() {
    use realtime_screen_translator::export::{self, Rgb};

    let gemini = PageGemini::start();
    let small = Rgb::new(64, 64);
    let cases = [
        ("jpg", include_bytes!("fixtures/wide-page.jpg").as_slice(), page()),
        ("png", include_bytes!("fixtures/wide-page.png").as_slice(), export::encode_png(&small).unwrap()),
        ("webp", include_bytes!("fixtures/wide-page.webp").as_slice(), export::encode_webp(&small).unwrap()),
    ];
    for (ext, large, regular) in cases {
        let dirs = Dirs::new();
        let b = keyed(&gemini, &dirs);
        let folder = dirs.data.with_file_name("Fixture");
        let source = folder.join(ext);
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("1.jpg"), page()).unwrap();
        let second = source.join(format!("2.{ext}"));
        fs::write(&second, large).unwrap();
        assert_eq!(b.import_folder(&source).unwrap_err().0, "image_dimensions_too_large", "{ext}");
        assert_eq!(read(&b, large.to_vec()).unwrap_err(), "image_dimensions_too_large", "{ext}");
        assert_eq!(fs::read(&second).unwrap(), large, "{ext} original");
        assert!(b.books(BooksIn { limit: 20, before: None }).unwrap().books.is_empty());
        assert_eq!(fs::read_dir(dirs.data.join("strips")).unwrap().count(), 0);

        let regular_folder = folder.join(format!("regular-{ext}"));
        fs::create_dir(&regular_folder).unwrap();
        fs::write(regular_folder.join(format!("1.{ext}")), regular).unwrap();
        let id = b.import_folder(&regular_folder).unwrap();
        assert_eq!(b.chapter(id).unwrap().pages.len(), 1);
    }
    assert_eq!(gemini.count(), 0);
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
