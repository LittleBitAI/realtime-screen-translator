---
scope: project
severity: preference
triggers: []
slots: []
sources: []
links: []
---

# realtime-screen-translator — a planned browser app for live manga translation; no code exists yet

## What it is for

`README.md` states the intent (in Korean): a program that recognises the
screen in real time and translates it, running in a web browser so it works
on both mobile and PC. Mobile comes first, with the Galaxy S21 as the
reference device. The representative use is translating Japanese manga into
Korean live. Requirements the README names:

- Tell apart two page-turn styles: vertical scrolling, and sliding
  back/forward between previous and next pages. Track position in each.
- Do not re-translate text already translated (the README asks how a screen
  knows a translation was "already done"), and do not translate sound
  effects.
- Translation model: Gemini 3.1 Flash Lite, with Gemini's default safety
  guardrails turned off and a prefill added.

The README leaves three questions open: how to distinguish scroll from
slide, how to track position while scrolling, and how to recognise an
already-translated region. No design answers any of them yet.

## Top-level layout

As of the first commit (`3c72617`) and the wiring commit (`25e0134`), the
repository holds scaffolding only:

| Path | What it does |
| --- | --- |
| `README.md` | Product intent and requirements, quoted above |
| `LICENSE` | Apache License 2.0 |
| `.gitattributes` | Forces LF line endings for all text files |
| `.gitignore` | Generic ignores for Python, Node, JVM, .NET and front-end build output, plus `.env` and `artifacts/` |
| `.claude/settings.json` | Agent permission denies: in-place `perl`, `dos2unix`/`unix2dos`, `git reset --hard`, `git clean -fdx`, and `request_user_input_async` |
| `.wiki/adapter.toml` | wiki-agent adapter: agents `claude` and `codex`; `gate_cmd`, `live_cmd` and `server_stop` are empty; review output goes to `artifacts/review`, scratch to `artifacts/` |
| `.wiki/` (other files) | wiki-agent state: `corpus.json` (document index), `graph.json`, `decisions/` (one record per merged PR), `trajectory.jsonl` (ignored) |

There is no `docs/` directory, no source tree, no package manifest and no
`verification.toml`.

## How the parts talk

Nothing runs yet, so there are no components or interfaces. The README
implies the eventual shape: a browser front end captures the screen, finds
text regions, sends new non-sound-effect text to Gemini, and overlays the
Korean result, while a tracker keyed to scroll or slide position remembers
which regions are done. Treat that as intent, not as structure; the
stack, capture method and storage are all undecided.

## Build, run, test

None defined. No language or framework is chosen (the `.gitignore` covers
several ecosystems and so does not indicate one), and `.wiki/adapter.toml`
leaves the gate and live commands empty. When the first code lands, this page
and `gate_cmd` should be updated together.
