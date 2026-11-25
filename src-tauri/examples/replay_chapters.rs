//! Development replay: reads image folders through the reader's own backend,
//! page by page in order, and writes each page's answer as JSON.
//!
//! cargo run --example replay_chapters -- <data dir> <out dir> <lang> <folder>...
//!
//! Each folder is imported once as a chapter (its book id is kept in
//! `<out>/<folder name>.book`), then every page is read in order. A page the
//! library already holds is laid out again from its kept answer with no
//! Gemini request, so replaying after a layout change costs nothing. New
//! pages go to `RST_GEMINI_BASE_URL` (point it at `scripts/gemini-ledger.mjs`
//! to cap and count the requests) with the key in `GEMINI_API_KEY`.

use realtime_screen_translator::api::{KeyIn, LangIn};
use realtime_screen_translator::platform::{Permissions, Platform};
use realtime_screen_translator::Backend;
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct Desk(PathBuf);

impl Platform for Desk {
    fn request_permissions(&self) -> Permissions {
        self.permissions()
    }
    fn permissions(&self) -> Permissions {
        Permissions { capture: false, overlay: false }
    }
    fn start(&self) -> Result<(), String> {
        Err("no capture here".into())
    }
    fn stop(&self) {}
    fn emit(&self, _: &str, _: serde_json::Value) {}
    fn export_dir(&self) -> PathBuf {
        self.0.clone()
    }
    fn seal(&self, d: &[u8]) -> Option<Vec<u8>> {
        Some(d.to_vec())
    }
    fn unseal(&self, d: &[u8]) -> Option<Vec<u8>> {
        Some(d.to_vec())
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [data, out, lang, folders @ ..] = args.as_slice() else {
        eprintln!("usage: replay_chapters <data dir> <out dir> <ja|en> <folder>...");
        std::process::exit(2);
    };
    let (data, out) = (PathBuf::from(data), PathBuf::from(out));
    // Never straight to Gemini: a page missing from the library would be
    // billed with nothing to count or cap it.
    let Ok(base) = std::env::var("RST_GEMINI_BASE_URL") else {
        eprintln!("set RST_GEMINI_BASE_URL to the ledger (node scripts/gemini-ledger.mjs <dir>)");
        std::process::exit(2);
    };
    let b = Backend::open(data.clone(), Some(base), Arc::new(Desk(data.join("export")))).expect("open the library");
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        b.put_key(KeyIn { key }).expect("keep the key");
    }
    b.put_source_lang(LangIn { lang: lang.clone() }).expect("set the language");
    for folder in folders {
        let folder = Path::new(folder);
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        let book_file = out.join(format!("{name}.book"));
        let book = match std::fs::read_to_string(&book_file) {
            Ok(id) => id.trim().parse().expect("a book id"),
            Err(_) => {
                let id = b.import_folder(folder).expect("import the folder");
                std::fs::create_dir_all(&out).unwrap();
                std::fs::write(&book_file, id.to_string()).unwrap();
                id
            }
        };
        let dir = out.join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        for (n, page) in b.chapter(book).expect("the chapter").pages.iter().enumerate() {
            let file = dir.join(format!("{:02}.json", n + 1));
            match b.read_chapter_page(page.id) {
                Ok(answer) => {
                    println!("{name} {:02}: {} balloons{}", n + 1, answer.boxes.len(), if answer.saved { " (kept)" } else { "" });
                    std::fs::write(&file, serde_json::to_string_pretty(&answer).unwrap()).unwrap();
                }
                // Not retried: a second request for a page waits for approval.
                Err(e) => {
                    println!("{name} {:02}: failed: {}", n + 1, e.0);
                    let _ = std::fs::remove_file(&file);
                }
            }
        }
    }
    println!("gemini requests: {}", b.status().gemini_requests);
}
