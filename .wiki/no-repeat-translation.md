---
scope: project
severity: contract
repeat: rule
triggers: ["이미 (했던|한) 번역", "중복 ?번역", "재번역", "다시 번역", "캐시", "cache", "dedup", "중복 제거", "번역 기록"]
slots: []
sources: [README.md]
links: [project, page-turn-tracking, translation-model]
---

# A region already translated is never sent to the model again

Rule. Before a text region goes to the translation model, check it against
what this session has already translated, and reuse the stored result
instead of calling the model. The identity of a region must survive the
region moving on screen (scrolling, sliding back to a previous page,
re-rendering at a different zoom), so it cannot be screen coordinates
alone; tie it to the page/scroll position the tracker reports and to the
region's content. A change that adds a model call path must route it
through this check.

Why. `README.md` requires that translations already done are not redone,
and asks explicitly how a screen knows a translation is "already done".
Readers flip back and forth constantly.

What goes wrong. Every back-swipe or scroll-up re-bills the API, adds
latency on the phone, and can make the same bubble's translation flicker
between two wordings.
