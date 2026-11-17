//! Support code for the backend contract tests in `tests/contract.rs`.
//!
//! # How the tests reach the application
//!
//! The tests never link against the app crate. They start the package's
//! `contract-host` binary (`[[bin]] name = "contract-host"` in
//! `src-tauri/Cargo.toml`) and talk to it over its standard streams, so they
//! compile and run before the application exists. Cargo builds the binary
//! before the integration tests and hands its path to them through
//! `CARGO_BIN_EXE_contract-host`.
//!
//! `contract-host` runs the same Rust command handlers the Tauri app registers
//! (A1: `POST /frames` is the `post_frames` command), with the platform layer
//! (capture, permission prompts, overlay, gallery or Downloads folder)
//! replaced by the fakes described below. The test plays the part of the
//! Kotlin plugin or the Windows capture module: it sends the frames and crops.
//!
//! ## Environment the host reads
//!
//! | variable | meaning |
//! | --- | --- |
//! | `RST_DATA_DIR` | the app-private folder: SQLite database and strip files |
//! | `RST_EXPORT_DIR` | stands in for the Android gallery or the Windows Downloads folder |
//! | `RST_GEMINI_BASE_URL` | Gemini base URL; requests go to `{base}/v1beta/models/{model}:generateContent` |
//! | `RST_PLATFORM` | `android` or `windows` |
//! | `RST_FAKE_CAPTURE_PERMISSION` | `grant` or `deny`: what the Android capture prompt answers |
//! | `RST_FAKE_OVERLAY_PERMISSION` | `grant` or `deny`: what the Android draw-over-apps prompt answers |
//!
//! The standard proxy variables (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`,
//! `NO_PROXY`) are honoured as usual.
//!
//! ## Wire protocol
//!
//! * stdin: one JSON object per line, `{"id": 7, "cmd": "post_frames", "args": {...}}`.
//!   `args` holds the request fields of `docs/design/api.yaml` by name.
//! * stdout: one JSON object per line. A reply carries the request's `id` and
//!   either `"ok": <response body>` or `"err": "<error code>"` (an object
//!   `{"code": "<error code>"}` is accepted too). Lines without a known `id`
//!   (events, anything else) are ignored; logs belong on stderr.
//! * Closing stdin asks the host to finish pending writes and exit.
//! * Bytes (`thumb`, `image`) are JSON arrays of numbers 0..=255, the way serde
//!   writes `Vec<u8>`. `thumb` is raw 8-bit grayscale, row-major, 96 pixels a
//!   row; `width` and `height` of a frame are the screen size in pixels. Crop
//!   and strip `image` bytes are PNG files.
//!
//! Command names: `get_status` (P1), `put_settings_gemini_key` (P2),
//! `post_permissions_request` (P3), `post_translation_start` (P4),
//! `post_translation_stop` (P5), `post_frames` (P6), `post_crops` (P7),
//! `get_placements` (P8), `post_strips` (P9), `get_books` (P10),
//! `post_books_export` (P11, args `book_id` and `format`).
//!
//! One host-only query, `__overlay`, answers `{"open": bool,
//! "click_through": bool}` for the overlay surface the platform layer holds.
#![allow(dead_code)]

use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Phone screen used by the Android tests. 960 px wide makes one thumbnail
/// row exactly 10 screen pixels, so every shift the tests use is exact.
pub const SCREEN_W: u32 = 960;
pub const SCREEN_H: u32 = 2400;
pub const THUMB_W: u32 = 96;
pub const TEST_KEY: &str = "AIzaSyContractTestKey00000000000000wxyz";

// ---------------------------------------------------------------- dirs

struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The app-private folder and the export folder of one simulated device.
#[derive(Clone)]
pub struct Dirs {
    pub data: PathBuf,
    pub export: PathBuf,
    _root: Arc<TempRoot>,
}

impl Dirs {
    pub fn new() -> Dirs {
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join("rst-contract").join(format!(
            "{}-{}-{}",
            std::process::id(),
            nanos,
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let data = root.join("data");
        let export = root.join("export");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&export).unwrap();
        Dirs { data, export, _root: Arc::new(TempRoot(root)) }
    }
}

// ---------------------------------------------------------------- host

pub fn err_code(e: &Value) -> String {
    match e {
        Value::String(s) => s.clone(),
        Value::Object(m) => m
            .get("code")
            .or_else(|| m.get("error"))
            .or_else(|| m.get("kind"))
            .map(err_code)
            .unwrap_or_else(|| e.to_string()),
        other => other.to_string(),
    }
}

pub struct Host {
    child: Child,
    stdin: Option<ChildStdin>,
    replies: Receiver<Value>,
    next_id: u64,
}

impl Host {
    pub fn spawn(env: &[(String, String)]) -> Host {
        let bin = option_env!("CARGO_BIN_EXE_contract-host").unwrap_or_else(|| {
            panic!(
                "the `contract-host` binary of src-tauri is not built yet; \
                 the application under test does not exist"
            )
        });
        let mut cmd = Command::new(bin);
        for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            cmd.env_remove(k);
        }
        cmd.env("NO_PROXY", "127.0.0.1,localhost").env("no_proxy", "127.0.0.1,localhost");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot start contract-host at {bin}: {e}"));
        let stdout = child.stdout.take().unwrap();
        let stdin = child.stdin.take();
        let (tx, rx) = channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                    if v.get("id").is_some() && tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        Host { child, stdin, replies: rx, next_id: 0 }
    }

    pub fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = json!({"id": id, "cmd": cmd, "args": args}).to_string();
        let stdin = self.stdin.as_mut().expect("contract-host stdin already closed");
        if let Err(e) = writeln!(stdin, "{line}").and_then(|_| stdin.flush()) {
            panic!("contract-host stopped accepting commands before `{cmd}`: {e}");
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match self.replies.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(v) if v["id"].as_u64() == Some(id) => {
                    if let Some(ok) = v.get("ok") {
                        return Ok(ok.clone());
                    }
                    if let Some(e) = v.get("err") {
                        return Err(err_code(e));
                    }
                    panic!("reply to `{cmd}` has neither `ok` nor `err`: {v}");
                }
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => panic!("no reply to `{cmd}` within 60 s"),
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("contract-host exited before replying to `{cmd}`")
                }
            }
        }
    }

    pub fn ok(&mut self, cmd: &str, args: Value) -> Value {
        self.call(cmd, args).unwrap_or_else(|e| panic!("`{cmd}` failed with `{e}`"))
    }

    pub fn err(&mut self, cmd: &str, args: Value) -> String {
        match self.call(cmd, args) {
            Ok(v) => panic!("`{cmd}` was expected to fail but answered {v}"),
            Err(e) => e,
        }
    }

    fn close(&mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.close();
    }
}

// ---------------------------------------------------------------- app

#[derive(Clone)]
pub struct Launch {
    pub platform: &'static str,
    pub capture: &'static str,
    pub overlay: &'static str,
    pub screen: (u32, u32),
    pub env: Vec<(String, String)>,
    pub dirs: Option<Dirs>,
}

impl Default for Launch {
    fn default() -> Launch {
        Launch {
            platform: "android",
            capture: "grant",
            overlay: "grant",
            screen: (SCREEN_W, SCREEN_H),
            env: vec![],
            dirs: None,
        }
    }
}

/// One simulated device: the host process, its Gemini stand-in and folders.
pub struct App {
    pub host: Host,
    pub gemini: Gemini,
    pub dirs: Dirs,
    pub screen: (u32, u32),
    pub picture_id: Option<i64>,
    pub book_id: Option<i64>,
    pub seq: u64,
    pub last_height: Option<u32>,
    /// Strips P9 accepted through `read_and_save`.
    pub strips: usize,
    launch: Launch,
    clock: i64,
}

impl App {
    pub fn launch(launch: Launch) -> App {
        let dirs = launch.dirs.clone().unwrap_or_else(Dirs::new);
        let gemini = Gemini::start();
        let mut env = vec![
            ("RST_DATA_DIR".to_string(), dirs.data.display().to_string()),
            ("RST_EXPORT_DIR".to_string(), dirs.export.display().to_string()),
            ("RST_GEMINI_BASE_URL".to_string(), gemini.url.clone()),
            ("RST_PLATFORM".to_string(), launch.platform.to_string()),
            ("RST_FAKE_CAPTURE_PERMISSION".to_string(), launch.capture.to_string()),
            ("RST_FAKE_OVERLAY_PERMISSION".to_string(), launch.overlay.to_string()),
        ];
        env.extend(launch.env.iter().cloned());
        let host = Host::spawn(&env);
        App {
            host,
            gemini,
            dirs,
            screen: launch.screen,
            picture_id: None,
            book_id: None,
            seq: 0,
            last_height: None,
            strips: 0,
            launch,
            clock: 1_700_000_000_000,
        }
    }

    /// Freshly installed app: no key yet, permission prompts would grant.
    pub fn new() -> App {
        App::launch(Launch::default())
    }

    /// Key entered and both permissions granted, translation still off.
    pub fn ready() -> App {
        let mut app = App::new();
        app.make_ready();
        app
    }

    /// Ready and switched on.
    pub fn on() -> App {
        let mut app = App::ready();
        app.start();
        app
    }

    pub fn make_ready(&mut self) {
        let r = self.set_key(TEST_KEY);
        assert_eq!(r["key_set"], true, "P2 did not store the key: {r}");
        let p = self.permissions();
        assert_eq!(p["capture_allowed"], true, "P3: {p}");
        assert_eq!(p["overlay_allowed"], true, "P3: {p}");
    }

    /// Close the app and open it again on the same device (same folders).
    pub fn restart(self) -> App {
        let mut launch = self.launch.clone();
        launch.dirs = Some(self.dirs.clone());
        drop(self);
        App::launch(launch)
    }

    /// Close the app, waiting for the host to exit.
    pub fn close(self) -> Dirs {
        let dirs = self.dirs.clone();
        drop(self);
        dirs
    }

    pub fn status(&mut self) -> Value {
        self.host.ok("get_status", json!({}))
    }

    pub fn gemini_requests(&mut self) -> u64 {
        let s = self.status();
        s["gemini_requests"].as_u64().unwrap_or_else(|| panic!("P1 has no gemini_requests: {s}"))
    }

    pub fn set_key(&mut self, key: &str) -> Value {
        self.host.ok("put_settings_gemini_key", json!({ "key": key }))
    }

    pub fn permissions(&mut self) -> Value {
        self.host.ok("post_permissions_request", json!({}))
    }

    pub fn try_start(&mut self) -> Result<Value, String> {
        let r = self.host.call("post_translation_start", json!({}));
        if let Ok(v) = &r {
            self.book_id = v["book_id"].as_i64();
        }
        r
    }

    pub fn start(&mut self) -> Value {
        let v = self.try_start().unwrap_or_else(|e| panic!("P4 failed with `{e}`"));
        assert_eq!(v["on"], true, "P4: {v}");
        v
    }

    pub fn stop(&mut self) -> Value {
        self.host.ok("post_translation_stop", json!({}))
    }

    pub fn overlay(&mut self) -> Value {
        self.host.ok("__overlay", json!({}))
    }

    // ---- frames

    pub fn frame_bytes(&mut self, thumb: Vec<u8>) -> Result<Value, String> {
        self.seq += 1;
        self.clock += 100;
        let (w, h) = self.screen;
        let r = self.host.call(
            "post_frames",
            json!({"seq": self.seq, "width": w, "height": h, "thumb": thumb, "captured_at": self.clock}),
        );
        if let Ok(v) = &r {
            if let Some(id) = v["picture_id"].as_i64() {
                self.picture_id = Some(id);
            }
        }
        r
    }

    pub fn frame(&mut self, thumb: Vec<u8>) -> Frame {
        let v = self.frame_bytes(thumb).unwrap_or_else(|e| panic!("P6 failed with `{e}`"));
        Frame::parse(v, self.seq)
    }

    /// Send one frame of `scene` scrolled by `offset` screen pixels.
    pub fn show(&mut self, scene: &Scene, offset: i64) -> Frame {
        let thumb = scene.thumb(offset, self.screen.0, self.screen.1);
        self.frame(thumb)
    }

    /// Send the same frame twice: the screen came to rest there.
    pub fn settle(&mut self, scene: &Scene, offset: i64) -> [Frame; 2] {
        let a = self.show(scene, offset);
        let b = self.show(scene, offset);
        [a, b]
    }

    pub fn black(&mut self) -> Frame {
        let (w, h) = self.screen;
        self.frame(vec![0u8; (THUMB_W * thumb_rows(w, h)) as usize])
    }

    /// Settle on `offset` and store every strip P6 asks for.
    pub fn read_and_save(&mut self, scene: &Scene, offset: i64) -> [Frame; 2] {
        let frames = self.settle(scene, offset);
        let mut done: Vec<(u32, u32)> = vec![];
        for f in &frames {
            if let Some(range) = f.save {
                if done.contains(&range) {
                    continue;
                }
                let r = self
                    .strip(scene, offset, range, f.seq)
                    .unwrap_or_else(|e| panic!("P9 refused the range P6 asked for: `{e}`"));
                done.push(range);
                self.strips += 1;
                self.last_height = r["picture_height"].as_u64().map(|h| h as u32);
            }
        }
        frames
    }

    pub fn strip(&mut self, scene: &Scene, offset: i64, (top, bottom): (u32, u32), seq: u64) -> Result<Value, String> {
        let image = scene.screen_png(offset, self.screen.0, self.screen.1, top, bottom);
        let picture_id = self.picture_id.expect("no picture_id from P6 yet");
        self.host.call(
            "post_strips",
            json!({"picture_id": picture_id, "frame_seq": seq, "top": top, "bottom": bottom, "image": image}),
        )
    }

    // ---- crops and placements

    pub fn crops(&mut self, crops: &[Crop]) -> Result<Value, String> {
        let picture_id = self.picture_id.expect("no picture_id from P6 yet");
        let list: Vec<Value> = crops.iter().map(Crop::to_json).collect();
        self.host.call(
            "post_crops",
            json!({"picture_id": picture_id, "frame_seq": self.seq, "crops": list}),
        )
    }

    pub fn crops_ok(&mut self, crops: &[Crop]) -> Value {
        self.crops(crops).unwrap_or_else(|e| panic!("P7 failed with `{e}`"))
    }

    pub fn placements(&mut self) -> Vec<Placement> {
        let v = self.host.ok("get_placements", json!({"since": null}));
        Placement::list(&v["placements"])
    }

    /// Poll P8 until at least `n` placements are there.
    pub fn wait_placements(&mut self, n: usize) -> Vec<Placement> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let list = self.placements();
            if list.len() >= n {
                return list;
            }
            if Instant::now() > deadline {
                panic!("P8 still has {} placements after 20 s, expected {n}", list.len());
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Send crops of `balloons` and wait until each has a placement.
    pub fn translate(&mut self, balloons: &[Balloon]) -> Vec<Placement> {
        let before = self.placements().len();
        let crops: Vec<Crop> = balloons.iter().map(Balloon::dialogue).collect();
        self.crops_ok(&crops);
        let list = self.wait_placements(before + balloons.len());
        balloons
            .iter()
            .map(|b| {
                list.iter()
                    .find(|p| p.rect().covers(&b.rect()))
                    .unwrap_or_else(|| panic!("no placement covers balloon {:?}: {list:?}", b.rect()))
                    .clone()
            })
            .collect()
    }

    // ---- books

    pub fn books(&mut self) -> Vec<Value> {
        let v = self.host.ok("get_books", json!({"limit": 50, "before": null}));
        v["books"].as_array().unwrap_or_else(|| panic!("P10 has no books list: {v}")).clone()
    }

    pub fn book(&mut self, id: i64) -> Value {
        self.books()
            .into_iter()
            .find(|b| b["id"].as_i64() == Some(id))
            .unwrap_or_else(|| panic!("P10 does not list book {id}"))
    }

    pub fn pictures(&mut self, book_id: i64) -> Vec<Value> {
        self.book(book_id)["pictures"].as_array().cloned().unwrap_or_default()
    }

    pub fn export(&mut self, book_id: i64) -> Result<Value, String> {
        self.host.call("post_books_export", json!({"book_id": book_id, "format": "png"}))
    }
}

pub fn thumb_rows(w: u32, h: u32) -> u32 {
    assert_eq!(h * THUMB_W % w, 0, "screen {w}x{h} does not give whole thumbnail rows");
    h * THUMB_W / w
}

/// Wait until `cond` holds or panic after `secs` seconds.
pub fn wait_until(secs: u64, what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while !cond() {
        if Instant::now() > deadline {
            panic!("timed out after {secs} s waiting for {what}");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

// ---------------------------------------------------------------- P6 / P8 shapes

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
}

impl Rect {
    pub fn new(x: i64, y: i64, w: i64, h: i64) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn covers(&self, o: &Rect) -> bool {
        self.x <= o.x && self.y <= o.y && self.x + self.w >= o.x + o.w && self.y + self.h >= o.y + o.h
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
    pub fn center(&self) -> (f64, f64) {
        (self.x as f64 + self.w as f64 / 2.0, self.y as f64 + self.h as f64 / 2.0)
    }
}

#[derive(Clone, Debug)]
pub struct Placement {
    pub id: i64,
    pub x: i64,
    pub y: i64,
    pub w: i64,
    pub h: i64,
    pub radius: i64,
    pub ko: String,
    pub font_px: i64,
}

impl Placement {
    pub fn parse(v: &Value) -> Placement {
        let n = |k: &str| v[k].as_i64().unwrap_or_else(|| panic!("placement has no integer `{k}`: {v}"));
        Placement {
            id: n("id"),
            x: n("x"),
            y: n("y"),
            w: n("w"),
            h: n("h"),
            radius: n("radius"),
            ko: v["ko"].as_str().unwrap_or_else(|| panic!("placement has no `ko`: {v}")).to_string(),
            font_px: n("font_px"),
        }
    }
    pub fn list(v: &Value) -> Vec<Placement> {
        v.as_array().unwrap_or_else(|| panic!("placements is not a list: {v}")).iter().map(Placement::parse).collect()
    }
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

pub struct Frame {
    pub seq: u64,
    pub motion: String,
    pub dy: i64,
    pub mode: String,
    pub picture_id: Option<i64>,
    pub detect: Option<(u32, u32)>,
    pub save: Option<(u32, u32)>,
    pub clear: bool,
    pub placements: Vec<Placement>,
    pub raw: Value,
}

fn range(v: &Value) -> Option<(u32, u32)> {
    if v.is_null() {
        return None;
    }
    let top = v["top"].as_u64().unwrap_or_else(|| panic!("range has no top: {v}"));
    let bottom = v["bottom"].as_u64().unwrap_or_else(|| panic!("range has no bottom: {v}"));
    Some((top as u32, bottom as u32))
}

impl Frame {
    pub fn parse(v: Value, seq: u64) -> Frame {
        Frame {
            seq,
            motion: v["motion"].as_str().unwrap_or_else(|| panic!("P6 has no motion: {v}")).to_string(),
            dy: v["dy"].as_i64().unwrap_or(0),
            mode: v["mode"].as_str().unwrap_or("").to_string(),
            picture_id: v["picture_id"].as_i64(),
            detect: range(&v["detect"]),
            save: range(&v["save"]),
            clear: v["clear"].as_bool().unwrap_or(false),
            placements: Placement::list(&v["placements"]),
            raw: v,
        }
    }
}

// ---------------------------------------------------------------- synthetic screens

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn noise(seed: u64, a: i64, b: i64) -> f64 {
    let h = mix(seed ^ mix(a as u64 ^ mix(b as u64).rotate_left(17)));
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// A long manga page, one value per thumbnail pixel. Scroll offsets are in
/// screen pixels; the page content is smooth value noise so every shift
/// matches at exactly one distance and two scenes never match.
#[derive(Clone, Copy)]
pub struct Scene {
    pub seed: u64,
    /// Thumbnail rows at the top that stay put while scrolling (status bar).
    pub fixed_rows: u32,
}

impl Scene {
    pub fn new(seed: u64) -> Scene {
        Scene { seed, fixed_rows: 0 }
    }

    pub fn with_fixed_top(seed: u64, rows: u32) -> Scene {
        Scene { seed, fixed_rows: rows }
    }

    fn value(&self, col: i64, row: i64) -> u8 {
        const CELL: i64 = 4;
        let (cx, fx) = (col.div_euclid(CELL), col.rem_euclid(CELL) as f64 / CELL as f64);
        let (cy, fy) = (row.div_euclid(CELL), row.rem_euclid(CELL) as f64 / CELL as f64);
        let n = |a, b| noise(self.seed, a, b);
        let top = n(cx, cy) * (1.0 - fx) + n(cx + 1, cy) * fx;
        let bot = n(cx, cy + 1) * (1.0 - fx) + n(cx + 1, cy + 1) * fx;
        let v = top * (1.0 - fy) + bot * fy;
        (40.0 + v * 180.0).round() as u8
    }

    /// Thumbnail pixel at thumbnail column `col`, screen thumbnail row `row`.
    fn thumb_px(&self, col: i64, row: i64, offset_rows: i64) -> u8 {
        if row < self.fixed_rows as i64 {
            (200.0 + noise(self.seed ^ 0xF1, col / 8, row) * 50.0) as u8
        } else {
            self.value(col, offset_rows + row)
        }
    }

    pub fn thumb(&self, offset_px: i64, w: u32, h: u32) -> Vec<u8> {
        let rows = thumb_rows(w, h) as i64;
        assert_eq!(offset_px * THUMB_W as i64 % w as i64, 0, "offset {offset_px} is not a whole thumbnail row");
        let off = offset_px * THUMB_W as i64 / w as i64;
        let mut out = Vec::with_capacity((rows * THUMB_W as i64) as usize);
        for r in 0..rows {
            for c in 0..THUMB_W as i64 {
                out.push(self.thumb_px(c, r, off));
            }
        }
        out
    }

    /// Full-resolution gray pixel of the screen scrolled by `offset_px`.
    pub fn screen_px(&self, offset_px: i64, w: u32, x: u32, y: u32) -> u8 {
        let scale = w as i64 / THUMB_W as i64;
        let off = offset_px * THUMB_W as i64 / w as i64;
        self.thumb_px(x as i64 / scale, y as i64 / scale, off)
    }

    /// PNG of screen rows `top..bottom` (a clean strip, no overlay).
    pub fn screen_png(&self, offset_px: i64, w: u32, _h: u32, top: u32, bottom: u32) -> Vec<u8> {
        assert!(bottom > top, "empty strip range {top}..{bottom}");
        png_rgb(w, bottom - top, |x, y| {
            let g = self.screen_px(offset_px, w, x, top + y);
            [g, g, g]
        })
    }
}

/// A speech balloon's text area on screen. The crop picture is a smooth
/// pattern drawn in the balloon's own coordinates, so cropping the same
/// balloon a few pixels off gives a near-identical picture and two balloons
/// with different seeds never look alike.
#[derive(Clone, Copy, Debug)]
pub struct Balloon {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
    pub seed: u64,
}

impl Balloon {
    pub fn new(x: i64, y: i64, w: u32, h: u32, seed: u64) -> Balloon {
        Balloon { x, y, w, h, seed }
    }
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w as i64, self.h as i64)
    }
    pub fn size(&self) -> (u32, u32) {
        (self.w, self.h)
    }
    /// The same balloon seen somewhere else on screen.
    pub fn at(&self, x: i64, y: i64) -> Balloon {
        Balloon { x, y, ..*self }
    }
    fn pixel(&self, u: f64, v: f64) -> u8 {
        let r = |i: i64| noise(self.seed.wrapping_mul(0x51ED), i, 7);
        let fx = 0.3 + 0.3 * r(1);
        let fy = (0.6 + 0.8 * r(2)) * if r(3) < 0.5 { -1.0 } else { 1.0 };
        let ph = r(4);
        let fy2 = 0.3 + 0.4 * r(5);
        let ph2 = r(6);
        let tau = std::f64::consts::TAU;
        let a = (tau * (fx * u / self.w as f64 + fy * v / self.h as f64 + ph)).sin();
        let b = (tau * (fy2 * v / self.h as f64 + ph2)).sin();
        (128.0 + 85.0 * a + 35.0 * b).clamp(0.0, 255.0) as u8
    }
    /// PNG of the balloon cropped with its frame moved by (`dx`, `dy`).
    pub fn png(&self, dx: i64, dy: i64) -> Vec<u8> {
        png_rgb(self.w, self.h, |x, y| {
            let g = self.pixel(x as f64 + dx as f64, y as f64 + dy as f64);
            [g, g, g]
        })
    }
    pub fn crop(&self, kind: &str) -> Crop {
        Crop { rect: self.rect(), kind: kind.to_string(), image: self.png(0, 0) }
    }
    pub fn dialogue(&self) -> Crop {
        self.crop("dialogue")
    }
    pub fn effect(&self) -> Crop {
        self.crop("effect")
    }
    /// A dialogue crop of this balloon whose frame is `d` pixels off.
    pub fn misaligned(&self, d: i64) -> Crop {
        Crop {
            rect: Rect::new(self.x + d, self.y + d, self.w as i64, self.h as i64),
            kind: "dialogue".to_string(),
            image: self.png(d, d),
        }
    }
}

pub struct Crop {
    pub rect: Rect,
    pub kind: String,
    pub image: Vec<u8>,
}

impl Crop {
    pub fn to_json(&self) -> Value {
        json!({
            "x": self.rect.x, "y": self.rect.y, "w": self.rect.w, "h": self.rect.h,
            "kind": self.kind, "score": 0.92, "image": self.image,
        })
    }
}

// ---------------------------------------------------------------- pictures

pub fn png_rgb(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            data.extend_from_slice(&f(x, y));
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
    }
    out
}

/// Decoded picture as RGB pixels, read with the `png` crate.
pub struct Rgb {
    pub w: u32,
    pub h: u32,
    pub px: Vec<[u8; 3]>,
}

impl Rgb {
    pub fn at(&self, x: i64, y: i64) -> [u8; 3] {
        self.px[(y as u32 * self.w + x as u32) as usize]
    }
}

pub fn decode_png(bytes: &[u8]) -> Rgb {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().expect("not a PNG file");
    let mut buf = vec![0; reader.output_buffer_size().expect("PNG too large")];
    let info = reader.next_frame(&mut buf).expect("cannot decode PNG");
    let (w, h) = (info.width, info.height);
    let ch = info.color_type.samples();
    let px = buf[..info.buffer_size()]
        .chunks(ch)
        .map(|p| match ch {
            1 | 2 => [p[0], p[0], p[0]],
            _ => [p[0], p[1], p[2]],
        })
        .collect();
    Rgb { w, h, px }
}

/// Width and height of a PNG, JPEG or WebP file.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() > 24 && b.starts_with(b"\x89PNG\r\n\x1a\n") {
        let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        return Some((be(16), be(20)));
    }
    if b.len() > 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let m = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + len;
        }
        return None;
    }
    if b.len() > 30 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        let le16 = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as u32;
        let le24 = |i: usize| b[i] as u32 | (b[i + 1] as u32) << 8 | (b[i + 2] as u32) << 16;
        return match &b[12..16] {
            b"VP8 " => Some((le16(26) & 0x3FFF, le16(28) & 0x3FFF)),
            b"VP8L" => {
                let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
                Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
            }
            b"VP8X" => Some((le24(24) + 1, le24(27) + 1)),
            _ => None,
        };
    }
    None
}

// ---------------------------------------------------------------- Gemini stand-in

/// One HTTP request the stand-in received.
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub target: String,
    pub body: Vec<u8>,
}

fn is_inline(k: &str) -> bool {
    matches!(k, "inline_data" | "inlineData" | "file_data" | "fileData")
}

fn walk_strings(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| walk_strings(x, out)),
        Value::Object(m) => m.iter().filter(|(k, _)| !is_inline(k)).for_each(|(_, x)| walk_strings(x, out)),
        _ => {}
    }
}

fn walk_images(v: &Value, out: &mut Vec<(u32, u32)>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| walk_images(x, out)),
        Value::Object(m) => {
            for (k, x) in m {
                if is_inline(k) {
                    let data = x["data"].as_str().unwrap_or("");
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(data)
                        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(data))
                        .unwrap_or_default();
                    out.push(image_size(&bytes).unwrap_or((0, 0)));
                } else {
                    walk_images(x, out);
                }
            }
        }
        _ => {}
    }
}

impl Recorded {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    /// Sizes of every picture in the request (inline or by file reference).
    pub fn images(&self) -> Vec<(u32, u32)> {
        let mut out = vec![];
        walk_images(&self.json(), &mut out);
        out
    }
    pub fn is_read(&self) -> bool {
        !self.images().is_empty()
    }
    /// Every string in the request body apart from picture data.
    pub fn strings(&self) -> Vec<String> {
        let mut out = vec![];
        walk_strings(&self.json(), &mut out);
        out
    }
    /// Role and joined text of the last `contents` turn.
    pub fn last_turn(&self) -> (String, String) {
        let body = self.json();
        let turns = body["contents"].as_array().cloned().unwrap_or_default();
        let last = turns.last().cloned().unwrap_or(Value::Null);
        let text: String = last["parts"]
            .as_array()
            .map(|ps| ps.iter().filter_map(|p| p["text"].as_str()).collect())
            .unwrap_or_default();
        (last["role"].as_str().unwrap_or("").to_string(), text)
    }
    /// (category, threshold) of every `safetySettings` entry.
    pub fn safety(&self) -> Vec<(String, String)> {
        let body = self.json();
        let list = body.get("safetySettings").or_else(|| body.get("safety_settings")).cloned();
        list.and_then(|l| l.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|s| {
                (
                    s["category"].as_str().unwrap_or("").to_string(),
                    s["threshold"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    }
}

#[derive(Default)]
struct Stub {
    requests: Vec<Recorded>,
    completed: usize,
    ja_by_size: HashMap<(u32, u32), String>,
    ko_by_ja: HashMap<String, String>,
    emitted: Vec<String>,
    delay: Duration,
    fail: Option<(u16, String)>,
    refuse_translation: Option<usize>,
    translations: usize,
}

/// A local stand-in for the Gemini API. Reads answer one Japanese line per
/// picture (chosen by picture size), translations answer one Korean line per
/// Japanese line found in the request. Answers continue the prefill, the way
/// Gemini continues a trailing `model` turn.
pub struct Gemini {
    pub url: String,
    st: Arc<Mutex<Stub>>,
}

pub const REFUSAL: &str = "죄송하지만 이 요청은 도와드릴 수 없습니다.";

impl Gemini {
    pub fn start() -> Gemini {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let st = Arc::new(Mutex::new(Stub::default()));
        let shared = st.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let st = shared.clone();
                thread::spawn(move || serve(stream, st));
            }
        });
        Gemini { url, st }
    }

    pub fn read_as(&self, size: (u32, u32), ja: &str) {
        self.st.lock().unwrap().ja_by_size.insert(size, ja.to_string());
    }
    pub fn translate_as(&self, ja: &str, ko: &str) {
        self.st.lock().unwrap().ko_by_ja.insert(ja.to_string(), ko.to_string());
    }
    pub fn delay(&self, d: Duration) {
        self.st.lock().unwrap().delay = d;
    }
    pub fn fail_with(&self, status: u16, body: &str) {
        self.st.lock().unwrap().fail = Some((status, body.to_string()));
    }
    /// The `n`-th translation request (1-based) gets a refusal instead of JSON.
    pub fn refuse_translation(&self, n: usize) {
        self.st.lock().unwrap().refuse_translation = Some(n);
    }
    pub fn requests(&self) -> Vec<Recorded> {
        self.st.lock().unwrap().requests.clone()
    }
    pub fn count(&self) -> usize {
        self.st.lock().unwrap().requests.len()
    }
    /// Requests answered so far.
    pub fn completed(&self) -> usize {
        self.st.lock().unwrap().completed
    }
}

fn continuation(key: &str, list: &[String]) -> String {
    let full = json!({ key: list }).to_string();
    let prefix = format!("{{\"{key}\":[\"");
    full.strip_prefix(&prefix).map(str::to_string).unwrap_or(full)
}

fn answer(st: &Arc<Mutex<Stub>>, req: &Recorded) -> (u16, String, Duration) {
    let mut s = st.lock().unwrap();
    let delay = s.delay;
    if req.method == "CONNECT" || !req.target.contains(":generateContent") {
        return (404, "{}".to_string(), delay);
    }
    if let Some((code, body)) = s.fail.clone() {
        return (code, body, delay);
    }
    let images = req.images();
    let text = if !images.is_empty() {
        let list: Vec<String> = images
            .iter()
            .map(|sz| s.ja_by_size.get(sz).cloned().unwrap_or_else(|| format!("セリフ{}x{}", sz.0, sz.1)))
            .collect();
        s.emitted.extend(list.iter().cloned());
        continuation("ja", &list)
    } else {
        s.translations += 1;
        if s.refuse_translation == Some(s.translations) {
            REFUSAL.to_string()
        } else {
            let joined = req.strings().join("\n");
            let mut found: Vec<(usize, String)> = vec![];
            for ja in s.emitted.iter().filter(|j| !j.is_empty()) {
                if let Some(i) = joined.find(ja.as_str()) {
                    if !found.iter().any(|(_, f)| f == ja) {
                        found.push((i, ja.clone()));
                    }
                }
            }
            found.sort();
            let list: Vec<String> = found
                .iter()
                .map(|(_, ja)| s.ko_by_ja.get(ja).cloned().unwrap_or_else(|| format!("번역 {ja}")))
                .collect();
            continuation("ko", &list)
        }
    };
    let body = json!({
        "candidates": [{
            "content": {"role": "model", "parts": [{"text": text}]},
            "finishReason": "STOP",
            "index": 0
        }],
        "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 10, "totalTokenCount": 20},
        "modelVersion": "gemini-3.1-flash-lite"
    });
    (200, body.to_string(), delay)
}

fn serve(stream: TcpStream, st: Arc<Mutex<Stub>>) {
    let Ok(read_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let mut length = 0usize;
    let mut chunked = false;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if k == "content-length" {
                length = v.parse().unwrap_or(0);
            } else if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            if reader.read_line(&mut size).unwrap_or(0) == 0 {
                break;
            }
            let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
            if n == 0 {
                let mut end = String::new();
                let _ = reader.read_line(&mut end);
                break;
            }
            let mut chunk = vec![0; n];
            if reader.read_exact(&mut chunk).is_err() {
                break;
            }
            body.extend_from_slice(&chunk);
            let mut crlf = [0u8; 2];
            let _ = reader.read_exact(&mut crlf);
        }
    } else if method != "CONNECT" {
        body.resize(length, 0);
        let _ = reader.read_exact(&mut body);
    }
    let req = Recorded { method, target, body };
    st.lock().unwrap().requests.push(req.clone());
    let (code, text, delay) = answer(&st, &req);
    if !delay.is_zero() {
        thread::sleep(delay);
    }
    let reason = match code {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let mut out = stream;
    let _ = write!(
        out,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: application/json; charset=UTF-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = out.flush();
    st.lock().unwrap().completed += 1;
}

// ---------------------------------------------------------------- stored state

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

pub fn all_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    files_under(dir, &mut out);
    out
}

/// The SQLite database under `dir` that has table `table`.
pub fn database_with(dir: &Path, table: &str) -> rusqlite::Connection {
    for f in all_files(dir) {
        let head = std::fs::read(&f).map(|b| b.into_iter().take(16).collect::<Vec<u8>>()).unwrap_or_default();
        if head != b"SQLite format 3\0" {
            continue;
        }
        let Ok(conn) = rusqlite::Connection::open(&f) else { continue };
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1 COLLATE NOCASE",
                [table],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if n > 0 {
            return conn;
        }
    }
    panic!("no SQLite database under {} has a `{table}` table", dir.display());
}

pub struct TranslationRow {
    pub status: String,
    pub source_ja: Option<String>,
    pub translation_ko: Option<String>,
}

pub fn translations(dir: &Path) -> Vec<TranslationRow> {
    let conn = database_with(dir, "Translation");
    let mut stmt = conn.prepare("SELECT status, source_ja, translation_ko FROM Translation").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(TranslationRow { status: r.get(0)?, source_ja: r.get(1)?, translation_ko: r.get(2)? })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}

pub fn setting_values(dir: &Path) -> Vec<Vec<u8>> {
    let conn = database_with(dir, "Setting");
    let mut stmt = conn.prepare("SELECT value FROM Setting").unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok(match r.get::<_, rusqlite::types::Value>(0)? {
                rusqlite::types::Value::Blob(b) => b,
                rusqlite::types::Value::Text(t) => t.into_bytes(),
                other => format!("{other:?}").into_bytes(),
            })
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows
}

pub fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

// ---------------------------------------------------------------- registered commands

fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    const SKIP: [&str; 9] = ["target", "node_modules", ".git", "build", ".gradle", "tests", "xtask", "docs", ".wiki"];
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if !SKIP.contains(&name.as_str()) {
                source_files(&p, out);
            }
        } else if name.ends_with(".rs") || name.ends_with(".kt") {
            out.push(p);
        }
    }
}

fn ident_after(text: &str, from: usize, keyword: &str) -> Option<String> {
    let rest = &text[from..];
    let i = rest.find(keyword)? + keyword.len();
    let name: String = rest[i..]
        .trim_start()
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    (!name.is_empty()).then_some(name)
}

/// Names of every Tauri command the app registers: entries of
/// `generate_handler!`, `#[tauri::command]` functions and Kotlin plugin
/// `@Command` functions anywhere in the repository's sources.
pub fn registered_commands(repo: &Path) -> Vec<String> {
    let mut files = vec![];
    source_files(repo, &mut files);
    let mut names = vec![];
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        if f.extension().map(|e| e == "kt").unwrap_or(false) {
            for (i, _) in text.match_indices("@Command") {
                names.extend(ident_after(&text, i, "fun "));
            }
            continue;
        }
        for (i, _) in text.match_indices("generate_handler!") {
            let rest = &text[i..];
            let (Some(open), Some(close)) = (rest.find(['[', '(']), rest.find([']', ')'])) else { continue };
            if close <= open {
                continue;
            }
            for item in rest[open + 1..close].split(',') {
                let name = item.trim().rsplit("::").next().unwrap_or("").trim().to_string();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
        for pat in ["#[tauri::command", "#[command"] {
            for (i, _) in text.match_indices(pat) {
                names.extend(ident_after(&text, i, "fn "));
            }
        }
    }
    names
}
