---
scope: project
severity: preference
triggers: []
slots: []
sources: []
links: [gate, live-web-host, translation-model, no-repeat-translation, skip-sound-effects, page-turn-tracking, mobile-first]
---

# realtime-screen-translator — a Tauri app that draws Korean over Japanese or English manga balloons

## What it is for

`README.md` describes an app for phones and PCs. It opens a manga site's
page inside the app, reads each page picture, and draws white shapes with
Korean text over the speech balloons. The Galaxy S21 is the reference
device. Translation is done by Gemini 3.1 Flash Lite, using the user's own
API key. Each page is translated once per source language, sound effects
are skipped, and the translated pages pile up in an on-device library that
can be exported as PNG or WebP. The original request is quoted at the
bottom of `README.md` and at the top of `docs/design/backend.md`.

There are two translation paths. The in-app reader is what the app's
screens use; its design, chosen on 2026-11-17, is in the "In-app reader"
section of `docs/design/backend.md`. The older screen-capture pipeline,
which drew an overlay on top of other apps, stays in the backend under its
contract tests, but the screens no longer start it.

## Top-level layout

| Path | What it does |
| --- | --- |
| `src-tauri/` | Rust backend and Tauri 2 shell (crate `realtime-screen-translator`) |
| `src-tauri/src/backend.rs` | The commands listed in `docs/design/api.yaml` |
| `src-tauri/src/app.rs` | Tauri app shell, behind the `app` feature; Windows overlay and toggle window |
| `src-tauri/src/reader.rs`, `src-tauri/src/reader.js` | In-app reader: the script injected into site pages, and its backend side |
| `src-tauri/src/gemini.rs` | Gemini requests: model name, safety settings, prefills |
| `src-tauri/src/pipeline.rs`, `tracker.rs`, `fingerprint.rs`, `detect.rs`, `wgc.rs` | Capture pipeline: frames, scroll/page judgement, balloon fingerprints, ONNX detection, Windows Graphics Capture |
| `src-tauri/src/store.rs`, `export.rs`, `vault.rs`, `file_vault.rs` | SQLite library, picture export, API-key storage |
| `src-tauri/src/models.rs` | Pinned URL, revision and SHA-256 of the balloon detector model |
| `src-tauri/src/bin/web-host.rs`, `contract-host.rs` | The same backend over HTTP, for the browser build and for the contract tests |
| `src-tauri/plugins/screen/` | Android Kotlin plugin: MediaProjection capture, ONNX detection, overlay, gallery export, Keystore |
| `src-tauri/gen/android/` | Committed Android Gradle project |
| `src-tauri/tests/` | Rust contract test (`contract.rs`), many edge-case suites, shared helpers in `support/mod.rs`, fixtures |
| `src/` | Svelte 5 + Vite + Tailwind screens: `/` (`Home.svelte`), `/library`, `/options`; `src/lib/api.js` is the API client |
| `xtask/` | Runs the Rust tests with cargo-nextest and writes the JUnit report |
| `scripts/live.mjs` | Builds the screens and starts `web-host` |
| `e2e/` | `seed.json` (API calls that fill an empty data folder) and `make_seed.py`, which generates it |
| `docs/design/` | `backend.md`, `frontend.md`, `api.yaml`, screen mocks and screenshots |
| `docs/research/` | Draft research notes from review rounds of PR #1; not verified rules |
| `.github/workflows/ci.yml` | Runs the gate on Windows for every push and pull request |

## How the parts talk

Inside the Tauri app, every API call in `src/lib/api.js` becomes a Tauri
command. In the browser, the same calls go over HTTP to `web-host` on one
port. Site pages opened in the reader may call only two commands,
`post_reader_page` and `post_reader_close` (`src-tauri/capabilities/reader.json`).
The key, settings and export commands are reachable only from the app's own
pages (`src-tauri/capabilities/default.json`). In the capture path, the
Android plugin or `wgc.rs` hands frames and crops to `pipeline.rs`. Gemini
requests and the duplicate lookup are in Rust, so both platforms share them.

## Build, run, test

The gate and its prerequisites are in [[gate]]. The live browser build and
its port are in [[live-web-host]]. The Android APK build and the
Windows-only tests that need a desktop session are listed in `README.md`
under "Android APK (Windows)" and "구조".
