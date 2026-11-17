# Review research — F1, cycle 1

Draft research before the fix of review round 4 of PR #1, written 2026-11-17. It is not a verified fact or a shared rule: the review reads it with the fix.

## Problem

F1: when the browser host is launched the documented way (`node scripts/live.mjs` → `src-tauri/src/bin/web-host.rs`), it grants both permissions whenever asked (lines 22-25), and `Browser::start()` returns Ok(()) without capturing (lines 30-32). So P4 opens a session, P1 reports translation on, and no frames, translations or saved pictures ever follow. This breaks two things. First, the review contract (spec revision 1, `out: []`): it says the product runs in a web browser on mobile and PC. Second, a basic rule: P1/P4 must not report an active translation when nothing is being captured.

Trigger: recurrence, rounds 1, 2, 3, 4.

- [P1] [P1] src-tauri/src/bin/web-host.rs:30 — Browser start reports success without capturing or translating

## Failed fixes

- Round 1 (0ac3c49): the README was rewritten and now says the browser version does not capture and that the Tauri apps do the real work (README.md:79). That documents the gap. It does not close it, and web-host.rs still fakes the grant and the start.
- Round 2 (f27aa9b): the browser version can now download exported pictures, and the disposition cited R14 and the native pipeline. The code the finding names did not change, and the review still had revision 1, which has the browser sentence and excludes nothing. The disposition contradicted the contract, so the review was right to reject it.
- Round 3: the disposition pointed to a proposed spec update awaiting approval. A proposal is not the contract. The review still got revision 1 and said so.
- Round 4: the disposition recorded an explicit decision to keep the native apps and a revision 2 that was never applied. Again, revision 1 was what reached the review. The review now says the conflicting scope must be settled in the governing review instruction itself.

## Sources

### S1 — Screen capture — Web Platform DX feature explorer

- URL: https://web-platform-dx.github.io/web-features-explorer/features/screen-capture/
- Retrieved: 2026-11-17
- Locator: Browser support table
- Fragment: "The navigator.mediaDevices.getDisplayMedia() method asks the user to choose a screen or portion of a screen"; Baseline status: "Limited availability"; Chrome Android: "Not supported" (bug: 40418135); Firefox for Android: "Not supported"; Safari on iOS: "Safari on iOS 27 Released on 2026-09-14"; Chrome desktop: "Chrome 72 Released on 2019-01-29".
- Applicability: The reference device runs Android Chrome, so a browser page cannot capture the screen there; web-host's Browser::start cannot become truthful for mobile.
- Counterevidence: Desktop browsers and iOS 27 Safari do support it, so the claim is limited to Android.
- Validation: Call getDisplayMedia in Chrome on the S21 and observe the rejection.

### S2 — [mediacapture-screen-share] Should getDisplayMedia be absent on platforms with no support? (#127)

- URL: https://lists.w3.org/Archives/Public/public-webrtc/2019Nov/0038.html
- Retrieved: 2026-11-17
- Locator: issue body
- Fragment: "mobile browsers like Chrome for Android and Firefox for Android define the API surface" "but don't actually implement it, instead always rejecting with NotAllowedError". The issue is from 2019; current status comes from S1.
- Applicability: Feature detection will look positive in the browser frontend while capture always fails, so a browser capture path would silently fail too.
- Counterevidence: Dated 2019; behaviour could change, though S1 still marks Chrome Android unsupported.
- Validation: Same check as S1: the error name is NotAllowedError.

### S3 — MediaDevices API: getDisplayMedia() — Can I use

- URL: https://caniuse.com/mdn-api_mediadevices_getdisplaymedia
- Retrieved: 2026-11-17
- Locator: Chrome for Android row
- Fragment: Chrome for Android, latest version (150): Not supported. Read from search results; the table itself was not fetched.
- Applicability: Second independent table agreeing with S1 for the reference platform.
- Counterevidence: Read from a search summary, not a direct fetch.
- Validation: As S1.

### S4 — WindowManager.LayoutParams — TYPE_APPLICATION_OVERLAY

- URL: https://developer.android.com/reference/android/view/WindowManager.LayoutParams#TYPE_APPLICATION_OVERLAY
- Retrieved: 2026-11-17
- Locator: TYPE_APPLICATION_OVERLAY summary
- Fragment: "Window type: Application overlay windows are displayed above all activity windows (types between FIRST_APPLICATION_WINDOW and LAST_APPLICATION_WINDOW) but below critical system windows like the status bar or IME." The fetched part did not state the required permission.
- Applicability: Drawing translations over the manga app is an Android app window type used by the Kotlin plugin (README.md:117), not something a web page can create.
- Counterevidence: This page does not say outright that browsers cannot use it; that is an inference.
- Validation: The native Android overlay path exists only in src-tauri/plugins/screen/android/.

### S5 — web-host.rs (repository)

- URL: src-tauri/src/bin/web-host.rs
- Retrieved: 2026-11-17
- Locator: lines 1-6, 22-32
- Fragment: "The device layer is a stand-in: nothing is captured or drawn over other apps, permissions are granted once asked for"; request_permissions returns Permissions { capture: true, overlay: true }; start returns Ok(()).
- Applicability: The exact place the finding names; it has not changed across rounds 1-4.
- Counterevidence: none given
- Validation: After the fix, /permissions/request in the browser must not report capture_allowed=true by default and /translation/start must not succeed.

### S6 — Backend design — backend.md (repository)

- URL: docs/design/backend.md
- Retrieved: 2026-11-17
- Locator: line 18; stack table line 31; R14 line 153; A7 line 41
- Fragment: Line 18 (translated): the product is a 'form that runs in a web browser' so it can be used on both mobile and PC. Line 31 (translated): deployment is an Android APK and a Windows installer, 'an installed app, so no server to run'. R14 (translated): 'no in-app viewer and no form opened in a web browser; manga is read in the app or site it is normally read in'.
- Applicability: The spec contradicts itself; the review reads line 18 and out: [] as binding.
- Counterevidence: R14 may be read narrowly as 'no in-app manga viewer' rather than 'no browser product'.
- Validation: The applied revision removes or qualifies line 18 and lists browser capture as out of scope.

### S7 — README.md — original request and browser limitation (repository)

- URL: README.md
- Retrieved: 2026-11-17
- Locator: lines 79 and 94-109
- Fragment: The quoted original request (lines 94-109) asks for a real-time screen-recognition translator, mobile optimisation on a Galaxy S21, scroll/slide tracking, no re-translation, no sound effects, Gemini 3.1 Flash Lite. It does not mention a web browser. Line 79 (translated): 'The browser version does not capture the screen or draw over other apps. Permissions count as granted when asked.'
- Applicability: The browser requirement comes from a restatement, not your own words; line 79 is what the review cites as confirming the stub.
- Counterevidence: The person may have confirmed the restatement in a requirements conversation not stored here (A7 mentions 'the confirmed restatement').
- Validation: You confirm the scope in the spec revision.

### S8 — Backend::start (repository)

- URL: src-tauri/src/backend.rs
- Retrieved: 2026-11-17
- Locator: lines 481-497
- Fragment: start checks key and platform permissions, then `self.platform.start().map_err(|e| { eprintln!("platform start failed: {e}"); ApiError("permission_required") })?;`
- Applicability: If web-host refuses start, the screen would wrongly tell the user to grant permissions; a distinct error code is needed.
- Counterevidence: none given
- Validation: P4 in the browser answers a capture-unsupported code and P1 stays off.

## Claims

- C1 (source; S1, S3): Chrome for Android does not support the Screen Capture API (getDisplayMedia).
- C2 (source; S1, S2): On Chrome/Firefox for Android the method exists but always rejects with NotAllowedError, so feature detection does not reveal the gap.
- C3 (interpretation; S4): Drawing over other apps on Android is an app overlay window type, which a browser page cannot create, so the browser host cannot provide the overlay on the reference device.
- C4 (source; S5): web-host's stand-in reports granted permissions and a successful start without capturing; this is unchanged at f27aa9b.
- C5 (interpretation; S6, S7): The governing spec contradicts itself (browser product on line 18 vs installed apps and R14). The four fixes argued the native side in dispositions while the review kept getting revision 1, which is why F1 recurred.
- C6 (source; S7): The browser requirement comes from the agent's restatement; your verbatim request does not mention a browser.
- C7 (interpretation; S5, S6, S8, S1): A durable fix needs both an applied spec revision that drops or qualifies the browser requirement, and a web-host that refuses to claim capture (with a distinct error code, and the fake grant kept only behind a test-only switch for the e2e seed).

## Core theory

The Screen Capture API (getDisplayMedia) has 'Limited availability'. Chrome Android and Firefox for Android are marked 'Not supported'. Mobile Chrome and Firefox expose the method but always reject it with NotAllowedError. Drawing over other apps on Android uses the app window type TYPE_APPLICATION_OVERLAY, 'displayed above all activity windows', which a web page cannot create. So a browser page cannot capture other apps' screens or overlay them on the reference device. The browser requirement cannot be met there by any code change; it has to be resolved in scope.

## Applicability

The reference device is a Galaxy S21 running Android (.wiki/mobile-first.md; docs/design/backend.md:18). The real pipeline needs MediaProjection capture and a SYSTEM_ALERT_WINDOW/accessibility overlay (README.md:115-117; src-tauri/plugins/screen/android/). A browser host cannot get either on Android, so `Browser::start` cannot be made truthful by adding capture for the primary target. Desktop Chrome supports getDisplayMedia, so a PC-only browser capture path is technically possible. It still could not draw over other apps, and it would not cover mobile. The code side of the fix sits in web-host.rs:22-32 and in how backend.rs:494-497 maps a platform start error to `permission_required`. e2e/make_seed.py:55,68 currently relies on the fake grant/start.

## Hypothesis

Two causes stack up, and every fix touched neither. (1) The scope conflict is inside the governing spec. Revision 1 says the product 'runs in a web browser on mobile and PC' (docs/design/backend.md:18; main checkout README.md:3) and excludes nothing. The same spec chooses installed Tauri apps (stack table, line 31) and R14 forbids 'a form opened in a web browser'. Your verbatim request (README.md:94-109) never mentions a browser; the sentence is an agent's restatement that spread into the spec and into .wiki/mobile-first.md ('no native install'). Dispositions and README text cannot override a contract the review receives unchanged. Only an applied spec revision can. (2) Separately from scope, web-host's stand-in platform reports a capture it never does. That stays a fault under either scope: under the browser scope it is the missing feature, and under the native scope it is a false 'on' in the documented live command.

## Counterevidence

- The person may really want a browser product, and the restatement may reflect a later clarification not visible in this repo. If so, the native stack is what is wrong, not the spec sentence.
- Desktop Chrome/Edge/Firefox/Safari support getDisplayMedia, so the PC half of the browser requirement could be partly met (capture of a chosen window, translations shown inside the page, not over the other app).
- Safari on iOS 27 is listed as supporting getDisplayMedia (released 2026-09-14), so 'mobile browsers cannot capture' is not universal, only true for Android, which is the reference platform.

## Alternatives

- Build browser capture with getDisplayMedia: works only on desktop, fails on Android Chrome (the reference device), and cannot overlay other apps. It does not satisfy revision 1's mobile requirement, so it would not clear F1 and adds a third capture pipeline.
- Another disposition or README note: tried four times. The review grades against the supplied spec revision, so prose cannot clear it.
- Make web-host honest only (no grant, refuse start): needed, but under revision 1 the review would still flag that the browser does not translate. It removes the false success, not the scope conflict.
- Delete web-host entirely: loses the Linux/mac/browser preview and the e2e seed harness, and the live_cmd in .wiki/adapter.toml would need a replacement. More disruption than gating the stand-in.

## Experiment

1) In Chrome on an Android phone (ideally a Galaxy S21), open any page and call navigator.mediaDevices.getDisplayMedia({video:true}). Expected: rejected with NotAllowedError. That confirms the browser scope cannot be met on the reference device. 2) After the spec revision is applied and web-host is changed (default: permissions denied or start refused with a distinct code such as capture_unsupported; the fake grant only behind an explicit test-only switch used by e2e/make_seed.py), run the gate and request a review that is supplied the new revision. F1 should not recur. If it recurs with revision 2 supplied, the hypothesis is wrong.

## Result

Draft, not verified on a device. The commit that adds this line, "Refuse to turn translation on where the host cannot capture", changes web-host so that by default it answers what a browser can actually do. P1 reports `capture_supported`, `capture_allowed` and `overlay_allowed` as false. P3 grants nothing. P4 answers 409 `{"error":"capture_unsupported"}`, and `on` stays false. Only `POST /test/simulated-device`, which `e2e/make_seed.py` calls first, attaches the stand-in device. `Backend::start` now refuses on any host whose `Platform::can_capture` is false, and the native app returns false off Windows and Android. That covers a second place where the same rule applies: the app on macOS or Linux. contract-host keeps the default of true. S1 says why it cannot be turned on and points to the installed app, without asking for permissions. Tests run on Windows 11:
- `cargo test --test capture_capability_edges`: `start_opens_a_session_only_where_capture_is_possible_and_granted`, a 7-row table over can_capture × capture × overlay. 1 passed.
- `cargo test --test web_host_files`: `the_browser_host_never_reports_a_capture_it_cannot_do`. 3 passed.
- `cargo test --test contract`: 37 passed.
- Vitest `src/screens/screens.test.js` ("where capture is impossible (a browser), turning on says so and asks for nothing") and `src/lib/text.test.js`: 21 passed.

Not done:
- Experiment step 1 (getDisplayMedia on a Galaxy S21) was not run, because no device was available.
- The spec revision that moves capture to the installed apps, which the person chose in round 3, is still not applied. The review still receives rev 1, so step 2's condition ("a review supplied revision 2") has not been met.

## Unresolved limits

Applying the spec revision is your decision and an app-side action; this research cannot make it, and whether the app will deliver revision 2 to the next review was not checked. Browser support was read from compatibility tables checked today, not tested on an S21. The overlay point rests on Android reference docs that describe the window type; they do not say outright that web pages cannot use it. The repo also has no record of why the browser sentence entered the restatement.
