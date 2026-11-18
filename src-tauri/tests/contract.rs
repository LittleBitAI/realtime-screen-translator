//! Backend contract tests T1..T37 of `docs/design/backend.md`.
//!
//! Every test drives the application through the `contract-host` binary
//! (see `tests/support/mod.rs` for the protocol) and a local Gemini
//! stand-in. Screens are 960x2400 unless a test says otherwise, so one
//! thumbnail row is exactly 10 screen pixels.

mod support;

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, Instant};
use support::*;

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn kos(list: &[Placement]) -> Vec<String> {
    sorted(list.iter().map(|p| p.ko.clone()).collect())
}

fn all_pictures(app: &mut App) -> Vec<Value> {
    app.books()
        .iter()
        .flat_map(|b| b["pictures"].as_array().cloned().unwrap_or_default())
        .collect()
}

#[test]
fn test_start_then_frame_and_crop_yields_korean_placement() {
    let mut app = App::ready();
    let balloon = Balloon::new(300, 600, 160, 220, 11);
    app.gemini.read_as(balloon.size(), "こんにちは");
    app.gemini.translate_as("こんにちは", "안녕");

    // The only thing the person does is switch translation on. Frames and
    // crops come from the capture side, P8 is what the overlay reads.
    app.start();
    app.settle(&Scene::new(1), 0);
    app.crops_ok(&[balloon.dialogue()]);

    let list = app.wait_placements(1);
    assert_eq!(list.len(), 1, "P8: {list:?}");
    assert_eq!(list[0].ko, "안녕");
    assert!(list[0].rect().covers(&balloon.rect()), "placement {:?} is not on the crop {:?}", list[0], balloon.rect());
}

#[test]
fn test_stop_removes_overlay_and_halts_gemini() {
    let mut app = App::on();
    app.settle(&Scene::new(2), 0);
    app.translate(&[Balloon::new(200, 500, 150, 200, 21)]);
    let before = app.gemini_requests();
    let sent = app.gemini.count();

    let stopped = app.stop();
    assert_eq!(stopped["on"], false, "P5: {stopped}");
    let refused = app
        .crops(&[Balloon::new(400, 1200, 170, 190, 22).dialogue()])
        .expect_err("P7 took a crop after translation was switched off");
    assert_eq!(refused, "not_on");

    let status = app.status();
    assert_eq!(status["on"], false, "P1: {status}");
    assert_eq!(status["gemini_requests"].as_u64(), Some(before), "P1: {status}");
    sleep(Duration::from_millis(500));
    assert_eq!(app.gemini.count(), sent, "a Gemini request went out after stop");
}

#[test]
fn test_vertical_shift_is_judged_scroll() {
    let mut app = App::on();
    let scene = Scene::new(3);
    app.settle(&scene, 0);
    let placed = app.translate(&[Balloon::new(150, 900, 180, 200, 31), Balloon::new(500, 1500, 200, 160, 32)]);

    let f = app.show(&scene, 120);
    assert_eq!(f.motion, "scroll", "P6: {}", f.raw);
    assert_eq!(f.dy, 120, "P6: {}", f.raw);

    let now = app.placements();
    assert_eq!(now.len(), 2, "P8: {now:?}");
    for p in &placed {
        let q = now.iter().find(|q| q.id == p.id).unwrap_or_else(|| panic!("placement {} was removed", p.id));
        assert_eq!(q.y, p.y - 120, "placement {} did not move up by 120 px", p.id);
        assert_eq!(q.x, p.x, "placement {} moved sideways", p.id);
    }
}

#[test]
fn test_whole_change_is_judged_page_and_clears() {
    let mut app = App::on();
    app.settle(&Scene::new(41), 0);
    app.translate(&[
        Balloon::new(100, 300, 150, 180, 41),
        Balloon::new(500, 900, 170, 200, 42),
        Balloon::new(250, 1600, 190, 150, 43),
    ]);

    let [first, second] = app.settle(&Scene::new(42), 0);
    for f in [&first, &second] {
        assert_ne!(f.motion, "scroll", "a different picture was taken for a scroll: {}", f.raw);
    }
    assert!(
        [&first, &second].iter().any(|f| f.motion == "page" && f.clear),
        "no P6 answer said page with clear: {} / {}",
        first.raw,
        second.raw
    );
    assert_eq!(second.mode, "slide", "P6: {}", second.raw);
    assert!(app.placements().is_empty(), "the previous page's placements are still on the overlay");
    assert_eq!(app.status()["mode"], "slide");
}

#[test]
fn test_scroll_detects_only_new_region() {
    let mut app = App::on();
    let scene = Scene::new(5);
    app.settle(&scene, 0);
    let p = app.translate(&[Balloon::new(300, 1200, 180, 220, 51)])[0].clone();

    let [moving, rest] = app.settle(&scene, 300);
    assert_eq!(moving.motion, "scroll", "P6: {}", moving.raw);
    let (top, bottom) = rest.detect.unwrap_or_else(|| panic!("no detect once the scroll stopped: {}", rest.raw));
    assert_eq!(bottom, SCREEN_H, "detect: {top}..{bottom}");
    assert!((2090..=2110).contains(&top), "detect covers more than the new 300 px: {top}..{bottom}");

    let q = app.placements().into_iter().find(|q| q.id == p.id).expect("the placement was removed by the scroll");
    assert_eq!(q.y, p.y - 300, "the placement did not follow the balloon");
}

#[test]
fn test_scroll_back_sends_no_gemini_request() {
    let mut app = App::on();
    let scene = Scene::new(6);
    let balloons = [
        Balloon::new(100, 300, 150, 200, 61),
        Balloon::new(500, 800, 170, 180, 65),
        Balloon::new(250, 1400, 200, 160, 63),
    ];
    let lines = [("おはよう", "좋은 아침"), ("どこへ行くの", "어디 가"), ("待ってて", "기다려")];
    for (b, (ja, ko)) in balloons.iter().zip(lines) {
        app.gemini.read_as(b.size(), ja);
        app.gemini.translate_as(ja, ko);
    }
    app.settle(&scene, 0);
    let first = app.translate(&balloons);
    assert_eq!(app.gemini_requests(), 2, "given: three lines translated with two requests");
    app.show(&scene, 600);
    app.settle(&scene, 1200);

    app.show(&scene, 600);
    app.settle(&scene, 0);
    let crops: Vec<Crop> = balloons.iter().map(Balloon::dialogue).collect();
    let r = app.crops_ok(&crops);
    let placed = Placement::list(&r["placed"]);
    assert_eq!(placed.len(), 3, "P7: {r}");
    assert_eq!(kos(&placed), kos(&first));
    assert_eq!(app.gemini_requests(), 2);
}

#[test]
fn test_previous_page_restores_without_detect() {
    let mut app = App::on();
    let (page1, page2) = (Scene::new(71), Scene::new(72));
    app.settle(&page1, 0);
    let first = app.translate(&[Balloon::new(120, 400, 160, 200, 71), Balloon::new(520, 1300, 180, 170, 77)]);
    app.settle(&page2, 0);
    app.translate(&[Balloon::new(300, 700, 150, 210, 79), Balloon::new(450, 1700, 200, 150, 74)]);
    let before = app.gemini_requests();

    let [a, b] = app.settle(&page1, 0);
    assert!(a.detect.is_none(), "P6 asked to detect a page it has seen: {}", a.raw);
    assert!(b.detect.is_none(), "P6 asked to detect a page it has seen: {}", b.raw);
    assert_eq!(kos(&b.placements), kos(&first), "P6: {}", b.raw);
    assert_eq!(app.gemini_requests(), before);
}

#[test]
fn test_effect_crop_gets_no_placement() {
    let mut app = App::on();
    app.settle(&Scene::new(8), 0);
    let d1 = Balloon::new(100, 500, 160, 200, 81);
    let d2 = Balloon::new(500, 900, 180, 170, 82);
    let sound = Balloon::new(300, 1500, 220, 140, 83);

    let r = app.crops_ok(&[d1.dialogue(), sound.effect(), d2.dialogue()]);
    assert_eq!(r["skipped_effects"], 1, "P7: {r}");
    app.wait_placements(2);
    sleep(Duration::from_millis(500));

    let list = app.placements();
    assert_eq!(list.len(), 2, "P8: {list:?}");
    for d in [&d1, &d2] {
        assert!(list.iter().any(|p| p.rect().covers(&d.rect())), "no placement on dialogue {:?}", d.rect());
    }
    assert!(
        list.iter().all(|p| !p.rect().intersects(&sound.rect())),
        "a placement sits on the sound effect: {list:?}"
    );
    let pictures: usize = app.gemini.requests().iter().map(|q| q.images().len()).sum();
    assert_eq!(pictures, 2, "the sound effect went to Gemini");
}

#[test]
fn test_effect_inside_balloon_is_stored_as_skip() {
    let mut app = App::on();
    app.settle(&Scene::new(9), 0);
    let b = Balloon::new(200, 800, 150, 190, 91);
    app.gemini.read_as(b.size(), "");

    app.crops_ok(&[b.dialogue()]);
    let gemini = &app.gemini;
    wait_until(20, "the read request to be answered", || gemini.completed() >= 1);
    sleep(Duration::from_millis(500));
    let sent = app.gemini.count();

    let second = app.crops_ok(&[b.dialogue()]);
    sleep(Duration::from_millis(1000));
    assert_eq!(app.gemini.count(), sent, "the second crop went to Gemini again");
    assert!(Placement::list(&second["placed"]).is_empty(), "P7: {second}");
    assert!(app.placements().is_empty(), "a placement was made for an empty reading");

    let dirs = app.close();
    let rows = translations(&dirs.data);
    assert_eq!(rows.len(), 1, "Translation rows: {}", rows.len());
    assert_eq!(rows[0].status, "skip");
}

#[test]
fn test_still_screen_sends_nothing() {
    let mut app = App::on();
    let scene = Scene::new(10);
    app.settle(&scene, 0);
    app.translate(&[Balloon::new(300, 1000, 160, 200, 101)]);
    let before = app.gemini_requests();
    let sent = app.gemini.count();

    for i in 0..5 {
        let f = app.show(&scene, 0);
        assert_eq!(f.motion, "still", "frame {i}: {}", f.raw);
        assert!(f.detect.is_none(), "frame {i} asked to detect again: {}", f.raw);
    }
    assert_eq!(app.gemini_requests(), before);
    assert_eq!(app.gemini.count(), sent);
}

#[test]
fn test_scroll_continues_while_gemini_is_slow() {
    let mut app = App::on();
    let scene = Scene::new(11);
    app.settle(&scene, 0);
    app.gemini.delay(Duration::from_secs(3));

    let t0 = Instant::now();
    let r = app.crops_ok(&[Balloon::new(300, 1500, 160, 200, 111).dialogue()]);
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(50), "P7 took {took:?}");
    assert_eq!(r["pending"], 1, "P7: {r}");

    for (i, offset) in [200, 400, 600].into_iter().enumerate() {
        let f = app.show(&scene, offset);
        assert_eq!(f.motion, "scroll", "frame {i}: {}", f.raw);
    }
    assert_eq!(app.gemini.completed(), 0, "a frame waited for the Gemini answer");
    assert!(t0.elapsed() < Duration::from_millis(2500), "frames took {:?}", t0.elapsed());
}

#[test]
fn test_gemini_requests_turn_guardrails_off() {
    const JA: &str = "ぶっ殺すぞ、このクソ野郎！";
    const KO: &str = "죽여 버린다, 이 개자식아!";
    let mut app = App::on();
    app.settle(&Scene::new(12), 0);
    let b = Balloon::new(250, 700, 170, 200, 121);
    app.gemini.read_as(b.size(), JA);
    app.gemini.translate_as(JA, KO);

    let placed = app.translate(&[b]);
    assert_eq!(placed[0].ko, KO);

    let reqs = app.gemini.requests();
    assert_eq!(reqs.len(), 2, "expected one read and one translation request");
    assert_eq!(reqs.iter().filter(|q| q.is_read()).count(), 1);
    for (i, q) in reqs.iter().enumerate() {
        let safety = q.safety();
        for cat in [
            "HARM_CATEGORY_HARASSMENT",
            "HARM_CATEGORY_HATE_SPEECH",
            "HARM_CATEGORY_SEXUALLY_EXPLICIT",
            "HARM_CATEGORY_DANGEROUS_CONTENT",
        ] {
            assert!(safety.iter().any(|(c, _)| c == cat), "request {i} leaves {cat} at its default: {safety:?}");
        }
        assert!(safety.iter().all(|(_, t)| t == "OFF"), "request {i}: {safety:?}");
    }

    let dirs = app.close();
    let rows = translations(&dirs.data);
    let row = rows.iter().find(|r| r.source_ja.as_deref() == Some(JA)).expect("no Translation row for the line");
    assert_eq!(row.status, "ready");
    assert_eq!(row.translation_ko.as_deref(), Some(KO));
}

#[test]
fn test_gemini_requests_carry_prefill() {
    let mut app = App::on();
    app.settle(&Scene::new(13), 0);
    let b1 = Balloon::new(150, 500, 160, 190, 131);
    let b2 = Balloon::new(500, 1300, 190, 160, 132);
    app.gemini.read_as(b1.size(), "おかえり");
    app.gemini.translate_as("おかえり", "어서 와");
    app.gemini.read_as(b2.size(), "さようなら");
    app.gemini.translate_as("さようなら", "잘 가");
    app.gemini.refuse_translation(2);

    app.translate(&[b1]);
    app.crops_ok(&[b2.dialogue()]);
    let gemini = &app.gemini;
    wait_until(20, "four Gemini answers", || gemini.completed() >= 4);
    sleep(Duration::from_millis(500));

    let list = app.placements();
    assert_eq!(list.len(), 1, "P8: {list:?}");
    assert_eq!(list[0].ko, "어서 와");
    assert!(!list.iter().any(|p| p.rect().intersects(&b2.rect())), "the refused line was placed");

    let reqs = app.gemini.requests();
    assert_eq!(reqs.len(), 4);
    for (i, q) in reqs.iter().enumerate() {
        let (role, text) = q.last_turn();
        assert_eq!(role, "model", "request {i} does not end with a model turn");
        let prefill = if q.is_read() { "{\"ja\":[\"" } else { "{\"ko\":[\"" };
        assert!(text.starts_with(prefill), "request {i} prefill is {text:?}");
    }

    let dirs = app.close();
    let rows = translations(&dirs.data);
    assert_eq!(rows.len(), 2, "Translation rows: {}", rows.len());
    let ready = rows.iter().find(|r| r.status == "ready").expect("no ready Translation");
    assert_eq!(ready.source_ja.as_deref(), Some("おかえり"));
    assert_eq!(ready.translation_ko.as_deref(), Some("어서 와"));
    assert_eq!(rows.iter().filter(|r| r.status == "failed").count(), 1);
}

#[test]
fn test_windows_starts_without_permission_prompt() {
    // The Android prompts would deny: a Windows build must not ask them.
    let mut app = App::launch(Launch {
        platform: "windows",
        capture: "deny",
        overlay: "deny",
        screen: (2560, 1440),
        ..Launch::default()
    });
    app.set_key(TEST_KEY);
    let p = app.permissions();
    assert_eq!(p["capture_allowed"], true, "P3: {p}");
    assert_eq!(p["overlay_allowed"], true, "P3: {p}");

    app.start();
    app.settle(&Scene::new(14), 0);
    let b = Balloon::new(1800, 300, 220, 160, 141);
    app.crops_ok(&[b.dialogue()]);
    let list = app.wait_placements(1);
    let r = list[0].rect();
    assert!(r.covers(&b.rect()), "placement {r:?} is not on the crop {:?}", b.rect());
    let ((px, py), (bx, by)) = (r.center(), b.rect().center());
    assert!((px - bx).abs() <= 2.0 && (py - by).abs() <= 2.0, "placement {r:?} is off the crop {:?}", b.rect());

    let overlay = app.overlay();
    assert_eq!(overlay["open"], true, "overlay: {overlay}");
    assert_eq!(overlay["click_through"], true, "overlay: {overlay}");
}

#[test]
fn test_status_tells_on_and_off() {
    let mut app = App::ready();
    assert_eq!(app.status()["on"], false);
    app.start();
    assert_eq!(app.status()["on"], true);
    app.stop();
    assert_eq!(app.status()["on"], false);
}

#[test]
fn test_no_login_command_exists() {
    let mut app = App::new();
    let status = app.host.call("get_status", json!({}));
    assert!(status.is_ok(), "P1 failed without login: {status:?}");
    let key = app.host.call("put_settings_gemini_key", json!({ "key": TEST_KEY }));
    assert!(key.is_ok(), "P2 failed without login: {key:?}");

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let names = registered_commands(repo);
    for needed in ["get_status", "put_settings_gemini_key"] {
        assert!(names.iter().any(|n| n == needed), "`{needed}` is not a registered Tauri command: {names:?}");
    }
    let bad: Vec<&String> = names
        .iter()
        .filter(|n| {
            let l = n.to_lowercase();
            ["login", "session", "password"].iter().any(|w| l.contains(w))
        })
        .collect();
    assert!(bad.is_empty(), "account commands are registered: {bad:?}");
}

#[test]
fn test_start_without_key_is_refused() {
    let mut app = App::new();
    app.permissions();
    let e = app.try_start().expect_err("P4 started without a Gemini API key");
    assert_eq!(e, "key_missing");
    assert_eq!(app.status()["on"], false);
}

#[test]
fn test_wrong_key_raises_notice() {
    let mut app = App::on();
    app.settle(&Scene::new(18), 0);
    app.gemini.fail_with(
        403,
        r#"{"error":{"code":403,"message":"API key not valid. Please pass a valid API key.","status":"PERMISSION_DENIED"}}"#,
    );
    app.crops_ok(&[Balloon::new(300, 900, 160, 200, 181).dialogue()]);

    let mut notice = Value::Null;
    wait_until(20, "P1 notice key_invalid", || {
        notice = app.status()["notice"].clone();
        notice == "key_invalid"
    });
    sleep(Duration::from_millis(300));
    assert!(app.placements().is_empty(), "a placement was made without a translation");
}

#[test]
fn test_key_is_stored_encrypted_and_stays_on_device() {
    let outside = Gemini::start();
    let mut launch = Launch::default();
    for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        launch.env.push((k.to_string(), outside.url.clone()));
    }
    launch.env.push(("NO_PROXY".to_string(), String::new()));
    launch.env.push(("no_proxy".to_string(), String::new()));
    let mut app = App::launch(launch);
    let key = "AIzaSyDeviceOnlySecretValue1234567x9f3e";

    let put = app.set_key(key);
    assert_eq!(put["key_set"], true, "P2: {put}");
    let status = app.status();
    assert_eq!(status["key_set"], true, "P1: {status}");
    assert_eq!(status["key_tail"], "9f3e", "P1: {status}");
    assert!(!status.to_string().contains(&key[..key.len() - 4]), "P1 gives away the key: {status}");
    sleep(Duration::from_millis(500));
    let gemini_seen = app.gemini.count();

    let dirs = app.close();
    let values = setting_values(&dirs.data);
    assert!(!values.is_empty(), "no Setting row holds the key");
    for v in &values {
        assert!(!contains(v, key.as_bytes()), "a Setting value holds the key in plain text");
    }
    for f in all_files(&dirs.data) {
        let bytes = std::fs::read(&f).unwrap_or_default();
        assert!(!contains(&bytes, key.as_bytes()), "{} holds the key in plain text", f.display());
    }
    assert_eq!(gemini_seen + outside.count(), 0, "requests left the device");
}

#[test]
fn test_books_remain_after_restart() {
    let mut app = App::on();
    let older = app.book_id.expect("P4 gave no book_id");
    app.read_and_save(&Scene::new(201), 0);
    app.stop();

    app.start();
    let book = app.book_id.expect("P4 gave no book_id");
    assert_ne!(book, older);
    let scene = Scene::new(202);
    let strips = app.strips;
    for offset in [0, 300, 600] {
        app.read_and_save(&scene, offset);
    }
    assert_eq!(app.strips - strips, 3, "given: three strips saved");
    app.stop();

    let mut app = app.restart();
    let books = app.books();
    assert!(!books.is_empty(), "P10 lists no books after restart");
    assert_eq!(books[0]["id"].as_i64(), Some(book), "the last book is not on top: {books:?}");
    let pictures = books[0]["pictures"].as_array().cloned().unwrap_or_default();
    assert_eq!(pictures.len(), 1, "book pictures: {pictures:?}");
}

#[test]
fn test_export_writes_picture_files() {
    let mut app = App::on();
    let book = app.book_id.expect("P4 gave no book_id");
    app.read_and_save(&Scene::new(211), 0);
    app.read_and_save(&Scene::new(212), 0);
    app.stop();
    assert_eq!(app.pictures(book).len(), 2, "given: a book with two pictures");

    let r = app.export(book).unwrap_or_else(|e| panic!("P11 failed with `{e}`"));
    assert_eq!(r["count"], 2, "P11: {r}");
    let files: Vec<String> = r["files"]
        .as_array()
        .unwrap_or_else(|| panic!("P11 has no files: {r}"))
        .iter()
        .map(|f| f.as_str().expect("file path is not a string").to_string())
        .collect();
    assert_eq!(files.len(), 2, "P11: {r}");
    let folder = app.dirs.export.canonicalize().unwrap();
    for f in files {
        let path = PathBuf::from(&f).canonicalize().unwrap_or_else(|e| panic!("{f} does not exist: {e}"));
        assert!(path.starts_with(&folder), "{f} is not in the gallery or Downloads folder");
        let bytes = std::fs::read(&path).unwrap();
        assert!(image_size(&bytes).is_some(), "{f} is not a picture file");
    }
}

#[test]
fn test_export_bakes_white_shape_and_korean() {
    let mut app = App::on();
    let book = app.book_id.expect("P4 gave no book_id");
    let scene = Scene::new(22);
    let frames = app.read_and_save(&scene, 0);
    let (top, bottom) = frames.iter().find_map(|f| f.save).expect("P6 asked for no strip");
    let b = Balloon::new(300, 900, 200, 240, 221);
    app.gemini.read_as(b.size(), "ありがとう");
    app.gemini.translate_as("ありがとう", "고마워");
    let p = app.translate(&[b])[0].clone();
    app.stop();

    let r = app.export(book).unwrap_or_else(|e| panic!("P11 failed with `{e}`"));
    let file = r["files"][0].as_str().unwrap_or_else(|| panic!("P11 has no file: {r}"));
    let img = decode_png(&std::fs::read(file).unwrap());
    assert_eq!((img.w, img.h), (SCREEN_W, bottom - top), "exported picture size");

    let (x, y, w, h) = (p.x, p.y - top as i64, p.w, p.h);
    let white = |c: [u8; 3]| c.iter().all(|&v| v >= 250);
    for (px, py) in [(x + w / 2, y + 2), (x + w / 2, y + h - 3), (x + 2, y + h / 2), (x + w - 3, y + h / 2)] {
        assert!(white(img.at(px, py)), "pixel ({px},{py}) just inside the placement is {:?}", img.at(px, py));
    }
    let mut glyph = 0;
    for yy in y + 4..y + h - 4 {
        for xx in x + 4..x + w - 4 {
            let c = img.at(xx, yy);
            if (c[0] as u32 + c[1] as u32 + c[2] as u32) < 3 * 128 {
                glyph += 1;
            }
        }
    }
    assert!(glyph > 0, "no text pixels inside the white shape");

    let margin = 3;
    let mut changed = 0;
    let mut first = None;
    for yy in 0..img.h as i64 {
        for xx in 0..img.w as i64 {
            let near = xx >= x - margin && xx < x + w + margin && yy >= y - margin && yy < y + h + margin;
            if near {
                continue;
            }
            let g = scene.screen_px(0, SCREEN_W, xx as u32, top + yy as u32);
            if img.at(xx, yy) != [g, g, g] {
                changed += 1;
                first.get_or_insert((xx, yy));
            }
        }
    }
    assert_eq!(changed, 0, "{changed} pixels outside the placement differ from the strip, first at {first:?}");
}

#[test]
fn test_crop_over_existing_placement_is_discarded() {
    let mut app = App::on();
    app.settle(&Scene::new(23), 0);
    let p = app.translate(&[Balloon::new(300, 800, 180, 220, 231)])[0].clone();
    let before = app.gemini_requests();
    let sent = app.gemini.count();

    // The capture now shows our own Korean: the local model crops it again.
    let echo = Balloon::new(p.x + 10, p.y + 10, 160, 200, 232);
    let r = app.crops_ok(&[echo.dialogue()]);
    assert_eq!(r["discarded"], 1, "P7: {r}");
    sleep(Duration::from_millis(500));

    let list = app.placements();
    assert_eq!(list.len(), 1, "P8: {list:?}");
    assert_eq!(list[0].id, p.id);
    assert_eq!(app.gemini_requests(), before);
    assert_eq!(app.gemini.count(), sent);
}

#[test]
fn test_fast_scroll_keeps_placements_and_aligns_on_stop() {
    let mut app = App::on();
    let scene = Scene::new(24);
    app.settle(&scene, 0);
    let p = app.translate(&[Balloon::new(390, 1100, 180, 200, 241)])[0].clone();

    // 15 % of 2400 px per frame, then the screen stops.
    let mut frames = vec![];
    for offset in [360, 720, 1080, 1080] {
        frames.push(app.show(&scene, offset));
    }
    for (i, f) in frames.iter().enumerate() {
        assert!(f.placements.iter().any(|q| q.id == p.id), "response {i} hid the placement: {}", f.raw);
    }
    let q = app.placements().into_iter().find(|q| q.id == p.id).expect("the placement is gone after the scroll");
    assert!((q.y - (p.y - 1080)).abs() <= 2, "placement at y {} but the balloon is at {}", q.y, p.y - 1080);
}

#[test]
fn test_fingerprint_matches_after_restart_and_move() {
    let scene = Scene::new(25);
    let b = Balloon::new(200, 900, 300, 400, 251);
    let mut app = App::on();
    app.gemini.read_as(b.size(), "また明日");
    app.gemini.translate_as("また明日", "내일 또 봐");
    app.settle(&scene, 0);
    let ko = app.translate(&[b])[0].ko.clone();
    app.stop();

    let mut app = app.restart();
    app.permissions();
    app.start();
    app.settle(&scene, 700);
    let r = app.crops_ok(&[b.at(200, 200).misaligned(3)]);
    let placed = Placement::list(&r["placed"]);
    assert_eq!(placed.len(), 1, "P7: {r}");
    assert_eq!(placed[0].ko, ko);
    assert_eq!(app.gemini_requests(), 0);
    assert_eq!(app.gemini.count(), 0);
}

#[test]
fn test_frames_never_call_gemini_and_one_crop_per_balloon() {
    let mut app = App::on();
    let scene = Scene::new(26);
    let sent = app.gemini.count();
    for offset in [0, 0, 200, 400, 400, 600, 800, 800, 1000, 1000] {
        app.show(&scene, offset);
    }
    assert_eq!(app.gemini.count(), sent, "P6 sent requests to Gemini");

    let crops = [
        Balloon::new(100, 300, 150, 180, 261).dialogue(),
        Balloon::new(500, 900, 170, 200, 262).dialogue(),
        Balloon::new(250, 1600, 190, 150, 263).dialogue(),
    ];
    let r = app.crops_ok(&crops);
    assert_eq!(r["pending"], 3, "P7: {r}");
}

#[test]
fn test_read_request_carries_only_crops() {
    let mut app = App::on();
    app.settle(&Scene::new(27), 0);
    app.translate(&[Balloon::new(100, 600, 140, 180, 271), Balloon::new(500, 1200, 200, 120, 272)]);

    let reqs = app.gemini.requests();
    let mut read: Vec<(u32, u32)> = reqs.iter().filter(|q| q.is_read()).flat_map(|q| q.images()).collect();
    read.sort();
    assert_eq!(read, vec![(140, 180), (200, 120)], "pictures in the read request");
    let all: Vec<(u32, u32)> = reqs.iter().flat_map(|q| q.images()).collect();
    assert_eq!(all.len(), 2, "pictures in all requests: {all:?}");
}

#[test]
fn test_read_and_translate_go_separately() {
    let mut app = App::on();
    app.settle(&Scene::new(28), 0);
    let b = Balloon::new(300, 900, 160, 200, 281);
    app.gemini.read_as(b.size(), "いってきます");
    app.gemini.translate_as("いってきます", "다녀오겠습니다");
    app.translate(&[b]);

    let reqs = app.gemini.requests();
    assert_eq!(reqs.len(), 2, "Gemini requests: {}", reqs.len());
    assert_eq!(reqs[0].images(), vec![(160, 200)], "the first request does not carry the crop");
    assert!(reqs[1].images().is_empty(), "the second request carries a picture");
    assert!(
        reqs[1].strings().iter().any(|s| s.contains("いってきます")),
        "the second request does not carry the Japanese text"
    );
    for q in &reqs {
        assert!(
            q.target.contains("gemini-3.1-flash-lite") && q.target.contains(":generateContent"),
            "request went to {}",
            q.target
        );
    }
}

#[test]
fn test_korean_stays_inside_white_shape() {
    let mut app = App::on();
    app.settle(&Scene::new(29), 0);
    let b = Balloon::new(400, 900, 120, 200, 291);
    let ko: String = "가나다라마바사아자차카타파하".chars().cycle().take(40).collect();
    app.gemini.read_as(b.size(), "とても長いセリフ");
    app.gemini.translate_as("とても長いセリフ", &ko);

    let p = app.translate(&[b])[0].clone();
    assert_eq!(p.ko, ko);
    assert!(p.rect().covers(&b.rect()), "placement {:?} leaves part of {:?} uncovered", p.rect(), b.rect());

    // Hangul is one em wide; lines are 1.2 em apart.
    assert!(p.font_px > 0, "font_px is {}", p.font_px);
    let per_line = p.w / p.font_px;
    assert!(per_line >= 1, "not one character fits a {} px line at {} px", p.w, p.font_px);
    let lines = (40 + per_line - 1) / per_line;
    let text_w = per_line.min(40) * p.font_px;
    let text_h = (lines * p.font_px * 6 + 4) / 5;
    assert!(text_w <= p.w && text_h <= p.h, "{text_w}x{text_h} text in a {}x{} shape", p.w, p.h);
}

#[test]
fn test_start_without_permission_is_refused() {
    let mut app = App::launch(Launch { overlay: "deny", ..Launch::default() });
    app.set_key(TEST_KEY);
    let p = app.permissions();
    assert_eq!(p["overlay_allowed"], false, "P3: {p}");
    let e = app.try_start().expect_err("P4 started without the overlay permission");
    assert_eq!(e, "permission_required");
    assert_eq!(app.status()["on"], false);
}

#[test]
fn test_blocked_capture_raises_notice() {
    let mut app = App::on();
    let frames: Vec<Frame> = (0..3).map(|_| app.black()).collect();
    assert_eq!(frames[2].motion, "blocked", "P6: {}", frames[2].raw);
    assert_eq!(app.status()["notice"], "capture_blocked");
}

#[test]
fn test_scroll_reading_makes_one_long_picture() {
    let mut app = App::on();
    let book = app.book_id.expect("P4 gave no book_id");
    let scene = Scene::new(32);
    app.read_and_save(&scene, 0);
    for i in 1..=4 {
        app.read_and_save(&scene, 300 * i);
    }
    let pictures = app.pictures(book);
    assert_eq!(pictures.len(), 1, "pictures: {pictures:?}");
    assert_eq!(pictures[0]["kind"], "stitched");
    assert_eq!(pictures[0]["height"], 3600);
}

#[test]
fn test_slide_reading_makes_picture_per_page() {
    let mut app = App::on();
    let book = app.book_id.expect("P4 gave no book_id");
    for seed in [331, 332, 333] {
        app.read_and_save(&Scene::new(seed), 0);
    }
    let pictures = app.pictures(book);
    assert_eq!(pictures.len(), 3, "pictures: {pictures:?}");
    assert!(pictures.iter().all(|p| p["kind"] == "page"), "pictures: {pictures:?}");
}

#[test]
fn test_scroll_back_does_not_append_twice() {
    let mut app = App::on();
    let book = app.book_id.expect("P4 gave no book_id");
    let scene = Scene::new(34);
    app.read_and_save(&scene, 0);
    for i in 1..=4 {
        app.read_and_save(&scene, 300 * i);
    }
    assert_eq!(app.pictures(book)[0]["height"], 3600, "given: one 3600 px picture");

    let mut frames = vec![];
    frames.extend(app.settle(&scene, 700));
    frames.extend(app.settle(&scene, 1200));
    for f in &frames {
        assert!(f.save.is_none(), "P6 asked to save an already stored range: {}", f.raw);
    }
    let seq = app.seq;
    let e = app.strip(&scene, 1200, (2100, 2400), seq).expect_err("P9 stored a range twice");
    assert_eq!(e, "already_covered");
    assert_eq!(app.pictures(book)[0]["height"], 3600);
}

#[test]
fn test_stop_and_start_opens_new_picture() {
    let mut app = App::on();
    let scene = Scene::new(35);
    app.read_and_save(&scene, 0);
    app.read_and_save(&scene, 300);
    let first = app.picture_id.expect("P6 gave no picture_id");

    app.stop();
    app.start();
    app.read_and_save(&scene, 300);
    app.read_and_save(&scene, 600);

    let pictures = all_pictures(&mut app);
    assert_eq!(pictures.len(), 2, "pictures: {pictures:?}");
    let old = pictures.iter().find(|p| p["id"] == first).expect("the first picture is gone");
    assert_eq!(old["closed"], true, "the first picture is still open: {old}");
}

#[test]
fn test_fixed_band_is_not_repeated() {
    let mut app = App::on();
    // 8 thumbnail rows = an 80 px status bar that does not move.
    let scene = Scene::with_fixed_top(36, 8);
    app.read_and_save(&scene, 0);
    let mut height = app.last_height.expect("P9 gave no picture_height");
    for i in 1..=5 {
        let frames = app.read_and_save(&scene, 300 * i);
        assert!(frames.iter().any(|f| f.save.is_some()), "scroll {i}: P6 asked to save nothing");
        for f in &frames {
            if let Some((top, bottom)) = f.save {
                assert!(top >= 80, "scroll {i}: save {top}..{bottom} includes the status bar");
            }
        }
        let now = app.last_height.expect("P9 gave no picture_height");
        assert!(now <= height + 310, "scroll {i}: the picture grew {} px for a 300 px scroll", now - height);
        height = now;
    }
}

#[test]
fn test_page_change_closes_stitched_picture() {
    let mut app = App::on();
    let book = app.book_id.expect("P4 gave no book_id");
    let (strip, other) = (Scene::new(371), Scene::new(372));
    app.read_and_save(&strip, 0);
    app.read_and_save(&strip, 300);
    let stitched = app.picture_id.expect("P6 gave no picture_id");

    let turned = app.read_and_save(&other, 0);
    let new_id = turned[1].picture_id.expect("P6 gave no picture_id");
    assert_ne!(new_id, stitched, "the page change did not start a new picture");
    app.read_and_save(&other, 300);

    let pictures = app.pictures(book);
    assert_eq!(pictures.len(), 2, "pictures: {pictures:?}");
    let old = pictures.iter().find(|p| p["id"] == stitched).expect("the stitched picture is gone");
    assert_eq!(old["closed"], true, "the stitched picture is still open: {old}");
    assert!(pictures.iter().any(|p| p["id"] == new_id), "the new picture is not listed");
}
