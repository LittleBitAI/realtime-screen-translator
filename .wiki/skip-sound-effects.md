---
scope: project
severity: contract
repeat: rule
triggers: ["효과음", "의성어", "의태어", "sfx", "sound effect", "오노마토페", "onomatopoeia"]
slots: []
sources: [README.md]
links: [project, no-repeat-translation]
---

# Sound effects are detected and left untranslated

Rule. Text recognised as a sound effect (the stylised onomatopoeia drawn
into manga art, outside speech bubbles and narration boxes) is not
translated and gets no overlay. The filter runs before the translation
call, not after, so a sound effect never costs a model request. Dialogue,
narration and signs are still translated.

Why. `README.md` asks to avoid translating sound effects. They are part of
the artwork; overlaying them covers the drawing and spends requests on
text nobody needs.

What goes wrong. Overlays pile up over action panels, the page becomes hard
to read on a small phone screen, and request volume grows with every
action scene.
