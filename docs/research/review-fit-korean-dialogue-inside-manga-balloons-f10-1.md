# Review research — F10, cycle 1

Draft research before the fix of review round 4 of PR #9, written 2026-11-21. It is not a verified fact or a shared rule: the review reads it with the fix.

## Problem

F10: an imported chapter's later page can be translated and cached without the earlier page's lines (speaker names, forms of address) in its prompt. The invariant: a page's Gemini request is built only after every earlier page in range that will be translated has been saved. The code instead has the backend make a later page wait only for earlier pages it already knows are being read (`Pages::after_earlier`, src-tauri/src/reader.rs:378). That only works if every earlier page reaches the backend before any later page checks.

Trigger: recurrence, rounds 1, 2, 3, 4.

- [P1] [P1] src-tauri/src/reader.js:234 — Selecting the nearest pages before sorting can skip an earlier page’s dialogue.

## Failed fixes

- Round 1 (30af69c): added the JS `sending` promise chain so calls go out in reading order, plus the backend's `reading` list and `after_earlier` wait. It held only when page 1 was already registered when page 2 checked. Page 1's file read and decode happened before it registered, so page 2 could overtake it.
- Round 2 (29dbf21): moved `self.pages.reading(&place)` before `load()`/decode (reader.rs:87-90). That closed the gap inside the backend but not the earlier one in the JS: a page whose image was still loading in reader.js was never sent at all, so the backend had nothing to wait for.
- Round 3 (a5821bf): added `held` in `scan()` (reader.js:187-212), so a page in range whose image is still loading blocks later pages from entering the queue. It covered image loading only. `pump()` still chooses which queued pages to send by distance from the view (reader.js:234) and sorts by reading order only the pages it picked (reader.js:242). With three loaded pages and two slots, pages 2 and 3 are sent and page 1 is not: round 4.
- Each fix moved the 'page is in progress' signal one step earlier; none made reading order a rule every step must follow. Still open: two `invoke` calls sent in order run as separate Tauri async tasks, so page 2's task can reach `after_earlier` before page 1's task calls `reading()`. The code admits this in a comment at reader.js:285-286 ('two calls sent in order still race into the app's threads').

## Sources

### S1 — Calling Rust from the Frontend (Tauri v2)

- URL: https://v2.tauri.app/develop/calling-rust/
- Retrieved: 2026-11-21
- Locator: Async Commands section
- Fragment: "Async commands are executed on a separate async task using `async_runtime::spawn`." "Commands without the async keyword are executed on the main thread unless defined with `#[tauri::command(async)]`." The page says nothing about the order in which commands execute or whether calls are queued; its only ordering statement is about event listeners, which "are called in the order they are registered".
- Applicability: post_chapter_page and post_reader_page are `#[tauri::command(async)]` (src-tauri/src/app.rs:574, 681), so each invoke runs as its own task with no documented order between them.
- Counterevidence: The page does not say the tasks are reordered; it is only silent on order. Any reordering is inferred from the multithreaded runtime.
- Validation: With one chapter call in flight at a time, the order Tauri runs commands in no longer matters; the fixture asserts no later call starts before the earlier one resolves.

### S2 — Probabilistic Fair Ordering of Events (arXiv 2602.09148), abstract

- URL: https://arxiv.org/abs/2602.09148
- Retrieved: 2026-11-21
- Locator: Abstract
- Fragment: "Tommy is a sequencer that uses a statistical model of per-clock synchronization error" ... "the probabilistic comparator is intransitive, making global ordering non-trivial" ... "Tommy produces a partial order of events". The paper concerns distributed clocks; its general point is that ordering by observed arrival is unreliable without a single ordering authority.
- Applicability: Only as general background: arrival order at the backend is not a reliable order, so the order has to be set at one place that sees every event, here reader.js.
- Counterevidence: The paper is about cross-machine clock error, not threads in one process; the analogy is loose.
- Validation: Not validated by the fix directly; background only.

### S3 — reader.js pump/scan/translate

- URL: src-tauri/src/reader.js
- Retrieved: 2026-11-21
- Locator: lines 22-23, 178-244, 278-299
- Fragment: `const PARALLEL = 2;` ... `// The pictures nearest the view first.` `const img = [...queue].sort((a, b) => distance(a) - distance(b))[0];` ... `picked.sort((a, b) => order(a) - order(b)).forEach(...)` ... `// ponytail: two calls sent in order still race into the app's threads // for a moment; a book-wide turn in the app would close it.` `held` is set only for chapter pages whose image is not loaded (line 207-208).
- Applicability: Pages are picked nearest first, and only the picked pages are sorted into reading order. The send chain resolves when `invoke` is called, not when the backend registers the page.
- Counterevidence: none given
- Validation: The three-page fixture fails on this code and passes after the change.

### S4 — reader.rs read_picture / Pages

- URL: src-tauri/src/reader.rs
- Retrieved: 2026-11-21
- Locator: lines 86-106, 370-384
- Fragment: `let place = self.place(source)?; let _reading = self.pages.reading(&place);` ... `fn after_earlier(...) { ... while st.reading.iter().any(|(b, s)| b == book && s < seq) { st = self.done.wait(st)... } }` A page waits only for earlier pages present in `reading`; a page already kept does not wait.
- Applicability: The backend can only wait for pages that have already been registered. An earlier page that has not been sent yet, or whose task has not yet registered, is not waited for.
- Counterevidence: This wait already makes consecutive uncached pages' Gemini requests run one after the other, so the second slot buys almost nothing for them; this supports, rather than argues against, one call at a time.
- Validation: Unchanged by the fix; stays as a second line of defence.

## Claims

- C1 (interpretation; S3, S4): The ordering guard depends on every earlier page being registered in the backend's `reading` list before a later page checks it, and each reader.js step before that (image load, queue selection) can stop an earlier page from being registered.
- C2 (interpretation; S1, S3): Two invokes sent in order run as separate Tauri async tasks with no documented order between them, so sending in order does not ensure the backend registers pages in order.
- C3 (interpretation; S2): A reliable order needs one place that assigns it and sees every event to be ordered; ordering by arrival at a later point does not give that.
- C4 (interpretation; S4): Limiting chapter translation to one uncached call at a time in reading order costs almost no throughput, because the backend already makes a later page's Gemini request wait for the earlier page to finish.

## Core theory

A sequencer gives incoming events ranks to set one order, and its order is only as good as the events it has already seen. Tauri runs each async command as its own task via `async_runtime::spawn` and documents no execution order between commands, so the order in which `invoke` is called does not set the order in which the backend registers pages. A reliable order needs one place that both assigns the order and holds back what comes later.

## Applicability

reader.js is the only component that knows every page in range, including pages still loading or not yet picked, and in reading order (`order()`, reader.js:272). The backend sees a page only after `invoke` is dispatched (app.rs:681-684, `#[tauri::command(async)]`) and `place()` has run. The ordering point belongs in reader.js `pump()`/`scan()` for chapters: replace the distance sort at reader.js:234 with an `order()` sort, cap uncached chapter calls at one, and block later pages behind any earlier page in range in state 'new'. The backend's `after_earlier` stays as a second line of defence. One call at a time costs almost nothing, because `after_earlier` already makes page 2's Gemini request wait for page 1 to finish.

## Hypothesis

The ordering is decided by which page reaches the backend first, not by a rule. The backend can wait only for pages it has already seen, and a page passes through four steps that can each reorder it before the backend sees it: image load (held), queue selection (pump by distance), `invoke` → Tauri async task, and the backend's `place()` lookup and `reading()` push. Fixing them one at a time leaves the next one as the next finding. The fix that holds is one ordering point in reader.js for imported chapters: at most one uncached page call in flight at a time, always the earliest new page in range by `order()`, with any earlier page in range that is still new (loading, or queued but not picked) blocking every later one. The next call then starts only after the previous one has resolved, which means it has saved, so both the queue-selection gap and the Tauri dispatch race go away.

## Counterevidence

- Picking the nearest pages first was meant to translate the visible page soonest. Ordering by reading order translates an earlier page above the view first. But the backend's wait already forces that order for consecutive uncached pages, so the visible page could not have been answered earlier anyway.
- Pages already in the library (no Gemini request) would queue behind an uncached call instead of being answered in parallel, adding latency when a chapter is reopened. Mitigation: only uncached calls take the single slot, but the JS cannot know which pages are cached until it asks, unless it already has them in the `done` map.
- No source I found says Tauri async commands actually run out of order. The race at the invoke step is inferred from the multithreaded runtime and from the code's own comment, not observed.

## Alternatives

- Sort `queue` by `order()` before filling the slots, and keep PARALLEL=2. This closes round 4 but leaves the Tauri dispatch race acknowledged at reader.js:285, which would be the next finding.
- Backend-side: have `after_earlier` wait for every earlier page of the book without a saved translation. That makes jumping to page 50 hang on pages that are never sent, and the backend cannot know which pages the reader intends to translate.
- Have the reader send a ticket or sequence number on every call so the backend admits calls in ticket order. That works for both readers but needs a new protocol and gap handling for skipped pages. Rejected while one call at a time in the JS gives the same guarantee with less code.
- Site reader (non-chapter): `isPage` requires `img.complete`, so a site page still loading is never a candidate and nothing holds back later pages. This is likely the same gap but not yet raised; fix it there with the same rule if review confirms.

## Experiment

Vitest fixture modelled on src/lib/reader-chapter-order.test.js: three chapter pages, all images loaded, all mocked in range, translation off. Mock the distances so page 3 is visible and page 1 is farthest, then turn translation on. The `post_chapter_page` mock records each call's start time and holds page 1's reply on a deferred promise. Assert that page 2's `post_chapter_page` is not called before page 1's promise resolves, and that page 3's is not called before page 2's resolves. On today's code this fails: pump starts pages 2 and 3 first. The assertion must be 'called only after the earlier page's reply resolved', not 'called in order'. The existing tests check only call order, which is why they passed while this gap stayed open. Keep the existing delayed-image test as a second case.

## Result

Observed in the round-4 fix (the commit "Send reader pages one at a time and close the fourth review's reader defects"). The experiment ran as written: src/lib/reader-chapter-turns.test.js "sends each chapter page only after the earlier page has been answered". Three loaded pages, translation off then on, page 13 in view and page 11 farthest, each reply held until the test releases it. On the previous reader.js it failed: page 11 was never sent, because pages 12 and 13 took both slots first. After the change it passes with the timeline sent 11, answered 11, sent 12, answered 12, sent 13, answered 13. The change follows the hypothesis: `pump()` picks the earliest queued page by `order()`, and only when no call is running. The `sending` chain and `PARALLEL` are removed, so the Tauri dispatch race no longer has two calls to reorder. The delayed-image case (src/lib/reader-chapter-order.test.js) still passes. Round 4 also found a decode failure that held pages back (F18); src/lib/reader-chapter-broken-image.test.js covers it, and the image's `error` event now marks the page unreadable so it holds nothing back. The site reader shares `pump()`, so it gets the same one-at-a-time order. src/lib/reader-order.test.js passes. The 61-page Edge walk at S21 size sent every chapter's pages in increasing order. Not measured: how much slower reopening a fully cached chapter is with one call at a time. A failed page (state `failed`) does not hold later pages back; they go without its lines.

## Unresolved limits

Tauri's command execution order is undocumented, so whether the dispatch race happens is inferred, not observed. Whether one call at a time is noticeably slower on real chapters, especially reopening fully cached ones, is untested. The site-reader version of the gap is unconfirmed. Pages that fail and are re-queued for retry (reader.js:318-322) would block later pages under the new rule. Whether a failed page should keep blocking, or let later pages go without its lines, is a product decision for the person.
