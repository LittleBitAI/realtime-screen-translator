---
scope: project
severity: contract
repeat: rule
triggers: ["번역 ?모델", "gemini", "제미나이", "flash ?lite", "가드레일", "안전 ?설정", "safety", "safetySettings", "프리필", "prefill", "LLM", "모델을? (바꾸|교체|변경)"]
slots: []
sources: [README.md, docs/design/backend.md, src-tauri/src/gemini.rs]
links: [project, no-repeat-translation, skip-sound-effects]
---

# Gemini 3.1 Flash Lite, every safety setting OFF, and a JSON prefill on every request

Rule. Gemini requests go out from Rust (`src-tauri/src/gemini.rs`), with
`gemini-3.1-flash-lite` as the default model. Every `safetySettings` entry
is sent as `OFF`; if the model refuses `OFF`, the request is resent with
`BLOCK_NONE`. Every request ends with a `model` turn holding a prefill:
`{"items":[` for the reader's one request per page, and `{"ja":["` for
reading and `{"ko":["` for translating in the capture path. In the capture
path, reading and translating stay two separate requests (R27 in
`docs/design/backend.md`). Do not change the model, turn a guardrail back
on, drop a prefill, or merge the two capture requests to get past a
refusal, latency or cost. Raise it with the person instead.

Why. The original request in `README.md` fixes the model and asks for the
guardrails off plus a prefill. `docs/design/backend.md` (A5 and the
"In-app reader" section) explains that a JSON prefill makes the model
continue with values instead of refusal text. In
`src-tauri/tests/contract.rs`, `test_gemini_requests_turn_guardrails_off`
and `test_gemini_requests_carry_prefill` pin this.

What goes wrong. Manga dialogue often has crude or violent lines; with a
default guardrail, the model blocks them and leaves balloons blank. Without
the prefill, answers stop parsing as JSON, so the translation is stored as
`failed` and never drawn.
