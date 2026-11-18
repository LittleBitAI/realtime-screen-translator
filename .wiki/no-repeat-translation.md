---
scope: project
severity: contract
repeat: rule
triggers: ["이미 (했던|한) 번역", "중복 ?번역", "재번역", "다시 번역", "캐시", "cache", "dedup", "중복 제거", "지문", "fingerprint", "번역 기록"]
slots: []
sources: [README.md, docs/design/backend.md]
links: [project, page-turn-tracking, translation-model]
---

# A page or balloon already translated is answered from the library, never sent to Gemini again

Rule. Every path that can call Gemini first looks up what is already
stored, and answers from it when it finds a match. In the reader, a page
is keyed by `sha256(bytes):lang` in the `ReaderPage` table. A known key
costs no request, whether the reader goes back, reopens the chapter, or
the site recreates its `<img>`. Copies of one page read at the same time
take turns on that key. In the capture path, a crop's 64-bit difference
hash matches a stored `Translation.fingerprint` when it is within 6 bits
and the aspect ratio is within 10%. In the reader, the key includes the
source language, so switching between Japanese and English is a new page.
A new path to Gemini must go through these lookups, not around them.

Why. The original request (`README.md`) requires not redoing translations.
`docs/design/backend.md` gives the reader keying in its "Never twice"
paragraph and the fingerprint tolerance in A3: crop borders shift by a few
pixels with scroll and zoom, so exact matching would miss repeats.

What goes wrong. Every back-swipe or reopened chapter is billed again and
waits on the network on the phone. Two simultaneous copies of one page
each call Gemini. The same balloon can show two different wordings.
