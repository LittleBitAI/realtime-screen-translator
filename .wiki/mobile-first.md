---
scope: project
severity: contract
repeat: rule
triggers: ["모바일", "mobile", "갤럭시", "galaxy", "s21", "휴대폰", "폰에서", "최적화", "성능", "performance", "반응형", "브라우저", "PC"]
slots: []
sources: [README.md]
links: [project]
---

# The app is a web page, judged first on a Galaxy S21

Rule. The product runs in a web browser on both mobile and PC, with no
native install. Mobile is the primary target and the Galaxy S21 is the
reference device: performance, layout and touch interaction are measured
against it first, and a change is not finished because it works on a
desktop browser. Prefer work that keeps per-frame capture and recognition
cheap on that phone over features that only pay off on PC.

Why. `README.md` says mobile is where the app is used most, names the
Galaxy S21 as the reference device, and puts mobile optimisation first.

What goes wrong. A pipeline tuned on a desktop GPU drops frames or heats
the phone, overlays lag behind scrolling, and the main use case degrades
while every desktop check still looks fine.
