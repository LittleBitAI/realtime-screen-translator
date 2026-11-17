---
scope: project
severity: contract
repeat: rule
triggers: ["스크롤", "scroll", "슬라이드", "슬라이딩", "스와이프", "swipe", "페이지 ?(넘기|전환|이동)", "이전 ?페이지", "다음 ?페이지", "트래킹", "tracking", "웹툰"]
slots: []
sources: [README.md]
links: [project, no-repeat-translation]
---

# Vertical scrolling and back/forward sliding are tracked as two separate modes

Rule. The app distinguishes two ways a reader moves through a work:
continuous vertical scrolling, and sliding back/forward between discrete
pages. It detects which mode is in use and tracks position with the method
that fits it; one mode must not be treated as a special case of the other.
In scroll mode, position is an offset that changes continuously and
regions enter and leave at the edges. In slide mode, position is a page
index and the whole screen is replaced at once, including when going back.

Why. `README.md` makes this the first of its key requirements and asks how
tracking works while scrolling. The duplicate check in
[[no-repeat-translation]] depends on the position this tracker reports.

What goes wrong. Treating a slide as a long scroll re-translates every
returned-to page; treating a scroll as page changes translates
half-visible regions and loses them at the edge.
