---
scope: project
severity: contract
repeat: rule
triggers: ["스크롤", "scroll", "슬라이드", "슬라이딩", "스와이프", "swipe", "페이지 ?(넘기|전환|이동)", "이전 ?페이지", "다음 ?페이지", "트래킹", "tracking", "tracker"]
slots: []
sources: [README.md, docs/design/backend.md]
links: [project, no-repeat-translation]
---

# The reader follows pictures, not the screen; the scroll/page split lives only in the capture path

Rule. In the in-app reader, translations are drawn on a layer next to
each `<img>`, positioned in percentages of that picture. They scroll,
slide and zoom with the picture, so the reader has no scroll or page
tracker. Adding one is the wrong fix for a shape that drifts; check the
layer's sizing in `src-tauri/src/reader.js` instead. The capture path
keeps the split in `src-tauri/src/tracker.rs`. Frames shrunk to 96 px wide
in grayscale are matched vertically against the previous one: a good
overlap is a scroll, and no overlap followed by two equal frames is a page
turn, which clears the overlay. Keep the two modes distinct there; one is
not a special case of the other.

Why. The original request (`README.md`) asks to tell scrolling from
sliding and to track while scrolling. `docs/design/backend.md` A2 gives
the capture rule. Its "In-app reader" section explains the switch, made
on 2026-11-17 after screen capture kept cancelling translations on a real
site. In `src-tauri/tests/contract.rs`,
`test_vertical_shift_is_judged_scroll` and
`test_whole_change_is_judged_page_and_clears` pin requirement R2.

What goes wrong. A tracker added to the reader fights the browser's own
layout and moves shapes off their balloons. In the capture path, treating
a page turn as a long scroll leaves stale overlays, and treating a scroll
as page turns throws away translations at the screen edge.
