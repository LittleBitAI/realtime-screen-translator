---
scope: project
severity: contract
repeat: rule
triggers: ["효과음", "의성어", "의태어", "sfx", "sound effect", "오노마토페", "onomatopoeia", "간판", "배경 글자"]
slots: []
sources: [README.md, docs/design/backend.md]
links: [project, no-repeat-translation, translation-model]
---

# Sound effects and sign text get no translation and no shape

Rule. Only balloon dialogue is translated and covered. In the reader,
Gemini is asked to leave out sound effects, signs and other on-screen
text, so they get no shape. In the capture path, the local model splits
text into inside-balloon and outside-balloon. Outside-balloon text is
treated as a sound effect and never sent to Gemini. A balloon whose read
comes back empty is stored with status `skip`, so it is not asked about
again. In the capture path, sound effects are dropped before any Gemini
request, never after.

Why. The original request (`README.md`) asks for sound effects not to be
translated. The current `README.md` adds signs ("효과음과 간판 글자는
번역하지 않습니다"). `docs/design/backend.md` A4 explains the split by
position and why asking Gemini about every piece of text was rejected.

What goes wrong. Shapes cover the artwork in action panels, the page gets
hard to read on a phone, and requests grow with every action scene.
