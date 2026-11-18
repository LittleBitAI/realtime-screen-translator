---
scope: project
severity: contract
repeat: rule
triggers: ["모바일", "mobile", "갤럭시", "galaxy", "s21", "휴대폰", "폰에서", "안드로이드", "android", "apk", "최적화", "성능", "performance", "반응형"]
slots: []
sources: [README.md, docs/design/backend.md]
links: [project, live-web-host]
---

# The phone comes first: judged on a Galaxy S21, in the installed Android app

Rule. The product is a Tauri app for Android and Windows, plus a limited
browser build. The phone is the primary target and the Galaxy S21 is the
reference device. Layout, start-up time and per-page cost are judged
there first, and a change is not finished because it looks right on a
desktop or in the browser build. The Kotlin plugin's capture, detection
and overlay (`src-tauri/plugins/screen/android/`) are not covered by the
gate and have to be checked on a real S21. Prefer work that keeps the
phone's per-page cost low: one Gemini request per page, nothing sent
twice, and no full-length picture held in memory.

Why. `README.md` says the phone is used most and was fitted first, with
the Galaxy S21 as the reference. The "스택" table in
`docs/design/backend.md` says the Kotlin plugin's work has to be confirmed
on a real Galaxy S21, and A6 explains why saved pages are kept in strips.

What goes wrong. A change tuned on a desktop browser can lag, heat the
phone or run out of memory on long webtoon strips. The browser build
cannot even open the reader, so passing there says nothing about the
main use case.
