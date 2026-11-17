# Review research — F1, cycle 2

Draft research before the fix of review round 5 of PR #1, written 2026-11-17. It is not a verified fact or a shared rule: the review reads it with the fix.

## Problem

F1: the browser translation flow that spec revision 1 requires does not exist. Launch with `node scripts/live.mjs`, save a key and press the toggle: no screen is captured, and no translations or saved pictures follow. In rounds 1-4 web-host faked a granted permission and a successful start (a false 'on'). Since 03990ff it refuses honestly: `can_capture()` is false unless `/test/simulated-device` was called (web-host.rs:39-48, 220), P4 answers 409 capture_unsupported, and Home.svelte refuses. The invariant: under revision 1 (`out: []`) a browser user must be able to capture, translate and accumulate translations through S1. A host may also never report a capture it does not do.

Trigger: recurrence, rounds 1, 2, 3, 4, 5.

- [P1] [P1] src-tauri/src/bin/web-host.rs:39 — Browser translation remains unavailable through the normal start flow

## Failed fixes

- Round 1 (0ac3c49): README rewritten to say the browser version does not capture and the Tauri apps do (README.md:79). It documented the gap and changed no behaviour.
- Round 2 (f27aa9b): browser download of exported pictures, plus a disposition citing R14 and the native pipeline. The cited code did not change, and revision 1, which the review received, has the browser sentence and excludes nothing.
- Round 3: the disposition pointed to a spec-update awaiting approval. A proposal is not the contract; the review still received revision 1.
- Round 4: the disposition recorded a decision to keep the native apps and a revision 2 that was never applied. The review said the scope conflict must be settled in the governing review instruction itself.
- Cycle 1 fix (03990ff): web-host made honest. Permissions are no longer granted, start is refused with capture_unsupported, and S1 explains why. This removed the false success but left the required capability missing. web_host_files.rs:137 and screens.test.js:129 now pin the refusal as expected behaviour, so the fix encoded the gap as the contract. The spec half of cycle 1's plan (apply revision 2) never happened, so under revision 1 the round-5 review read the honest refusal as F1 itself.

## Sources

### S1 — Screen capture — Web Platform DX feature explorer

- URL: https://web-platform-dx.github.io/web-features-explorer/features/screen-capture/
- Retrieved: 2026-11-17
- Locator: Browser support entries
- Fragment: Baseline status: "Limited availability". Chrome: "Chrome 72 Released on 2019-01-29"; Chrome Android: "Not supported Bugs: 40418135"; Edge: "Edge 79"; Firefox: "Firefox 33"; Firefox for Android: "Not supported"; Safari: "Safari 13"; Safari on iOS: "Safari on iOS 27 Released on 2026-09-14".
- Applicability: Desktop browsers can capture, so a browser capture path is buildable for PC. The reference device's Chrome cannot capture.
- Counterevidence: A compatibility table, not a device test.
- Validation: Experiment step 1 succeeds in desktop Chrome; step 3 fails on the S21.

### S2 — MediaDevices API: getDisplayMedia() — Can I use

- URL: https://caniuse.com/mdn-api_mediadevices_getdisplaymedia
- Retrieved: 2026-11-17
- Locator: Chrome for Android / Samsung Internet / Safari on iOS rows
- Fragment: Chrome for Android: "Not supported" for version 157. Firefox for Android: "Not supported" for version 160. Samsung Internet: "Not supported" for versions 4 - 30 and 31. Safari on iOS: "Not supported" for 3.2 - 26.6. "Supported" for 27.0 and 27.1 - 27.2. Chrome desktop: "Supported" for 72 - 156, 157, and 158 - 160.
- Applicability: A second independent table. It also rules out Samsung Internet, the S21's stock browser.
- Counterevidence: cobaltcapture.com (Sept 2026) rates Chrome Android 'Partial, device-dependent'; that rating came from a search summary and was not fetched.
- Validation: Experiment step 3.

### S3 — MediaDevices: getDisplayMedia() method — MDN

- URL: https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getDisplayMedia
- Retrieved: 2026-11-17
- Locator: Description; Secure context; Transient activation
- Fragment: "prompts the user to select and grant permission to capture the contents of a display or portion thereof (such as a window) as a MediaStream." "Secure context: This feature is available only in secure contexts (HTTPS), in some or all supporting browsers." "Transient user activation is required." "Limited availability. This feature is not Baseline because it does not work in some of the most widely-used browsers."
- Applicability: The toggle press supplies the user activation. localhost is a secure context, but a LAN IP over HTTP is not, which matters for iOS Safari 27 reaching the PC host.
- Counterevidence: The per-browser table did not render in the fetch.
- Validation: The toggle calls getDisplayMedia inside the click handler; over http://<LAN-IP> the call is unavailable.

### S4 — Document Picture-in-Picture — Chrome for Developers

- URL: https://developer.chrome.com/docs/web-platform/document-picture-in-picture
- Retrieved: 2026-11-17
- Locator: Introduction; browser support; limits
- Fragment: "open an always-on-top window that can be populated with arbitrary HTML content." Support: "Chrome: 116." "Edge: 116." "Firefox: 151." "Safari: not supported." Launch status "Complete (Desktop)." The window "never outlives the opening window", "cannot be navigated", the site "cannot" set its position, and opening it requires a user gesture.
- Applicability: On desktop, translations can float above the manga window in a PiP window instead of only inside the page. It cannot sit on the balloon itself.
- Counterevidence: Desktop only, position not controllable, Safari unsupported.
- Validation: Optional; the in-page preview with placements is enough for step 1.

### S5 — Chromium-dev thread: getDisplayMedia under automation with --auto-select-desktop-capture-source

- URL: https://groups.google.com/a/chromium.org/g/chromium-dev/c/FShFi397rvo
- Retrieved: 2026-11-17
- Locator: thread body (via search summary)
- Fragment: A user launched Chrome with `--auto-select-desktop-capture-source` to pick the shared tab automatically under Selenium; the audio-sharing checkbox still needed manual handling. A CEF issue mentions `--auto-select-window-capture-source-by-title`, and notes that with --use-fake-ui-for-media-stream older CEF shared a monitor instead of the window.
- Applicability: Lets the fix turn run the real getDisplayMedia flow without a human at the picker.
- Counterevidence: Read from a search summary. Not confirmed for headless Playwright or the current Chrome; video-only capture avoids the audio caveat.
- Validation: The step-1 run resolves getDisplayMedia with the intended window.

### S6 — web-host.rs (repository)

- URL: src-tauri/src/bin/web-host.rs
- Retrieved: 2026-11-17
- Locator: lines 1-11, 22-48, 135-147, 220
- Fragment: "A browser cannot capture other apps' screens or draw over them, so this host says so: no permission is granted and P4 answers capture_unsupported." can_capture() returns self.simulated; start() returns Err unless can_capture(); simulated starts false and only POST /test/simulated-device sets it. Routes /frames, /crops, /strips and /placements are already served over HTTP.
- Applicability: The host already exposes the whole frame pipeline over HTTP. Only a real capture source and a detector are missing.
- Counterevidence: none
- Validation: After the fix, can_capture is true only while the page reports a live display stream.

### S7 — pipeline.rs Screen trait and api.rs inputs (repository)

- URL: src-tauri/src/pipeline.rs
- Retrieved: 2026-11-17
- Locator: pipeline.rs:42-69, 80-110; api.rs:80-167; Cargo.toml:59,70
- Fragment: Screen: next() -> frame or toggle; detect(seq, top, bottom) -> crops; strip(seq, top, bottom) -> PNG; draw(placements, clear). FrameIn {seq, width, height, thumb (96-px grayscale), captured_at}; CropIn {x, y, w, h, kind, score, image}; StripIn {picture_id, frame_seq, top, bottom, image}. `ort` is optional and only enabled by the `app` feature together with tauri.
- Applicability: The browser path must supply exactly these inputs. Server-side detection needs ort split from the app feature.
- Counterevidence: none
- Validation: Contract tests keep passing; a new test drives the browser source with a recorded frame sequence.

### S8 — Backend design — backend.md (repository)

- URL: docs/design/backend.md
- Retrieved: 2026-11-17
- Locator: lines 18, 22, 31, 41 (A7), 153 (R14), 234 (P1)
- Fragment: Line 18 (translated): built 'in a form that runs in a web browser' so it can be used on both mobile and PC. Line 22 (translated): stack 'chosen in the requirements conversation; to change it, start a new run'. Line 31 (translated): Android APK and Windows installer. R14 (translated): 'do not build an in-app viewer or a form opened in a web browser; manga is read in the app or site it is normally read in'. P1 (translated): capture_supported is 'true for the Android and Windows apps, false for the browser and other desktops'.
- Applicability: Revision 1 contains both the browser requirement and R14. P1's documented capture_supported=false for the browser must change if browser capture lands.
- Counterevidence: R14 can be read as forbidding the very browser path proposed here.
- Validation: The fix updates P1's description of capture_supported via the spec-update route, not a silent doc edit.

### S9 — Cycle 1 research and its result (repository)

- URL: docs/research/review-oneshot-initial-f1-1.md
- Retrieved: 2026-11-17
- Locator: Hypothesis; Result; Not done
- Fragment: The fix made web-host refuse to turn on; 'The spec revision that moves capture to the installed apps ... is still not applied. The review still receives rev 1.'
- Applicability: Shows the cycle-1 plan rested on a scope change that never landed, so its code half alone was bound to be read as F1.
- Counterevidence: none
- Validation: The round-5 finding cites the refusal as the failure.

## Claims

- C1 (source; S1, S3): Desktop Chrome, Edge, Firefox and Safari, and Safari on iOS 27, support getDisplayMedia, which needs a secure context and a user gesture.
- C2 (source; S1, S2): Chrome for Android, Firefox for Android and Samsung Internet do not support getDisplayMedia, so the Galaxy S21's browsers cannot capture the screen.
- C3 (interpretation; S6, S9): Every earlier fix changed reporting or argued scope. 03990ff made the refusal honest without adding a capture source, so under the unchanged revision 1 the refusal itself became the finding.
- C4 (source; S4): On desktop, Document Picture-in-Picture can show translations in an always-on-top window, but not positioned over the balloon.
- C5 (interpretation; S3): A phone reaching the PC host over plain HTTP on a LAN IP is not a secure context, so browser capture from a phone also needs HTTPS.
- C6 (source; S5): Chrome launch switches can pre-select the capture source, which lets an agent run the real getDisplayMedia flow end-to-end without a human.
- C7 (interpretation; S6, S7, S1): The durable code fix is a browser capture source: getDisplayMedia, then a 96-px thumb to /frames, then detection (web-host running detect.rs with ort split from the app feature) to /crops, then PNG rows to /strips, with placements drawn in the page. can_capture is true only while that stream is live.
- C8 (interpretation; S2, S8): Even with that fix, revision 1's mobile half cannot be met on the reference device, and R14 may be read against a browser product. Only the person can settle that in an applied spec revision.

## Core theory

The Screen Capture API's getDisplayMedia() 'prompts the user to select and grant permission to capture the contents of a display or portion thereof (such as a window) as a MediaStream'. It needs a secure context and transient user activation. Its availability is 'Limited': supported in Chrome 72+, Edge 79+, Firefox 33+, Safari 13+ and Safari on iOS 27; 'Not supported' in Chrome Android (crbug 40418135), Firefox for Android and Samsung Internet. Document Picture-in-Picture can 'open an always-on-top window that can be populated with arbitrary HTML content' on desktop Chrome/Edge 116+ and Firefox 151. It requires a user gesture and is 'Complete (Desktop)' only.

## Applicability

On desktop browsers (and iOS Safari 27) a page can capture the window or screen showing the manga, so it can feed the same backend calls the native Screen trait feeds (pipeline.rs:42-69). The frontend draws the frame to a canvas, makes the 96-px grayscale thumb, and posts /frames. When P6 returns `detect`, it crops those rows and either sends them to a new web-host route that runs detect.rs's Onnx, or runs onnxruntime-web on the same model, then posts /crops. When P6 returns `save`, it posts a PNG of those rows to /strips. Detection in web-host needs `ort` decoupled from the `app` feature (Cargo.toml:59,70). Placements are drawn over the live preview in the page, or in a Document PiP window on desktop. web-host's `simulated` flag becomes a 'browser stream attached' signal set by the page after getDisplayMedia resolves, so start succeeds only when capture is real. If the capture includes the browser's own translations (whole-screen capture), overlay_in_capture must be true so P6 covers the placements, as on Android (A6). On Android Chrome, the reference Galaxy S21, the API is unavailable, so the honest refusal stays there.

## Hypothesis

Every fix changed either the reporting (docs, dispositions, honest refusal) or the scope argument. None added a capture source for the browser host. In this repository the review's bar is the supplied revision 1, and it cannot move without an applied spec revision. That has not happened in five rounds, and the person's latest instruction asks for the 'required browser translation flow' (Korean: 필수 브라우저 번역 흐름) to work. So the remaining cause is in code: the browser frontend has no getDisplayMedia path that feeds frames into the existing HTTP pipeline (/frames, /crops, /strips, /placements, which e2e/make_seed.py already drives with an external frame source), and web-host has no detector to answer P6's `detect` ranges. The browser host's can_capture must become true exactly where the page has a live display-capture stream, and stay false otherwise.

## Counterevidence

- R14 (backend.md:153) says not to build 'a form opened in a web browser', and the stack (line 22: 'to change it, start a new run') chose installed Tauri apps. A browser capture path may itself be flagged as contradicting R14.
- Android Chrome, the reference device's browser, cannot capture, so revision 1's mobile half stays unmet. The next review may raise the mobile half as F1 again.
- A browser cannot draw over the other app. Translations appear in the page or a PiP window, not on the balloon itself (R7/R31 placement fidelity).
- getDisplayMedia needs a secure context. A phone reaching http://<LAN-IP>:8080 is not one, so even iOS Safari 27 needs HTTPS to the host; only localhost qualifies over plain HTTP.
- cobaltcapture.com (updated 2026-09) rates Chrome Android 'Partial, device-dependent', which conflicts with caniuse and web-features ('Not supported').

## Alternatives

- Apply revision 2 (native-only scope) through the app's spec-update and approval: it would clear F1 in one step, but it has stalled for five rounds, and the person's latest instruction asks for the browser flow. That decision is the person's, and it cannot be the fix turn's first move.
- Another disposition or README note: tried in rounds 1-4. The review grades against the supplied revision, so prose cannot clear it.
- Keep the honest refusal only: done in 03990ff, and refused in round 5 for exactly that reason.
- Run detection in the browser with onnxruntime-web instead of web-host: workable, but it adds a JS ML dependency and a second copy of detect.rs's pre/post-processing. Reusing the Rust detector in web-host is the smaller diff, because the host already runs on the PC that captures.
- A PWA or WebView wrapper on Android: it is an installed app, not a browser page, so it does not satisfy revision 1's browser wording either.

## Experiment

1) Live run on a desktop. Start web-host with RST_GEMINI_BASE_URL pointing at a fake Gemini server and the real balloon model present. Open http://localhost:8080 in Chrome launched with `--auto-select-desktop-capture-source=<window title>` (or `--auto-select-window-capture-source-by-title`), and have a second window show a manga page from tests/fixtures. Press the toggle. Expect: getDisplayMedia resolves, then P1 on:true and capture_supported:true. /frames posts arrive with growing seq; a `detect` range leads to /crops and non-empty placements drawn in the page; /strips saves rows; /books lists a picture; export downloads a PNG. Scrolling the manga window gives mode 'scroll' and changing pages gives 'slide'. 2) Negative check: in a browser without getDisplayMedia (or with the picker cancelled), start stays refused with capture_unsupported, nothing is granted, and on stays false. 3) Device check: on the Galaxy S21's Chrome, check `typeof navigator.mediaDevices.getDisplayMedia` and call it. Expect it absent or rejected, which confirms the mobile limit. The hypothesis is refuted if step 1 cannot produce a saved translated picture through the browser path, or if round 6 raises F1 against a desktop browser that passes step 1.

## Result

Draft, not verified. The experiment has not been run, and no browser capture path was added.

When asked in round 5, the person did not choose the desktop browser capture path. They asked for an installable Android build so the app's own translation can be run and watched on their phone. While preparing that, I found the Android app had never been built. The machine had no Android SDK, and there was no Gradle app project.

Commit 6f877a6 ("Generate the Android project so the app builds as an APK") commits `src-tauri/gen/android` from `cargo tauri android init`. It also raises the app's minSdk to 26, the screen plugin's level, which the manifest merge required. Observed builds:
- The Rust library for aarch64-linux-android built (release, 15.8 MB).
- `gradlew assembleUniversalDebug` built the Kotlin plugin and app with one deprecation warning.
- The APK was 41.6 MB, sha256 e632fa2e87c4ccfb5d6352cad9d10013c3eec7eb5a65fe89c8b086f01dd45e01.
- aapt reports package ai.littlebit.realtime_screen_translator, sdkVersion 26 and targetSdkVersion 37.
- It requests SYSTEM_ALERT_WINDOW, FOREGROUND_SERVICE_MEDIA_PROJECTION and POST_NOTIFICATIONS.
- Its native code is arm64-v8a, with libonnxruntime.so and librealtime_screen_translator.so.

It is published as the GitHub pre-release `android-test-6f877a6`.

Steps 1-3 of the experiment were not run. Step 3, and an on-device run of capture, detection, translation and the overlay, wait for the person to install the build and connect the phone. Revision 1 is still the supplied contract.

## Unresolved limits

Whether the reviewer accepts a desktop-browser (plus iOS Safari 27) capture path as meeting revision 1's 'mobile and PC' without the Android browser is unknown. Android Chrome support was read from compatibility tables on 2026-11-17, not tested on an S21. Whether the Chrome auto-select capture flags work under Playwright or headless mode on this Chrome version is unverified. R14's intended reading (no in-app manga viewer, or no browser product at all) is not recorded. This research turn could not edit or run anything; the live run is left to the fix turn.
