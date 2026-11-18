---
scope: project
severity: contract
repeat: rule
triggers: ["실행", "띄워", "라이브", "live", "서버", "server", "포트", "port", "8080", "localhost", "web-host", "브라우저에서 (열|확인|보)"]
slots: [live_cmd, server_stop]
sources: [.wiki/adapter.toml, scripts/live.mjs, README.md]
links: [project, gate]
---

# `node scripts/live.mjs` serves the browser build on port 8080; it is not the real reader

Rule. To see the program run, use `live_cmd`: `node scripts/live.mjs`.
It installs packages if they are missing, builds the screens into
`dist/`, then builds and starts `src-tauri/src/bin/web-host.rs`, which
serves the screens and the API on one port. Wait for
`listening on http://localhost:8080`. `PORT` changes the port and
`APP_DATA_DIR` the data folder; the default data folder is `rst-web` under
the system temp folder. Stop it with Ctrl+C. `server_stop` is empty, so
stop only the process you started. The browser build cannot attach
translations to another site's pages: its first screen tells the user to
use the installed app, and only settings, the library and export are real
there. Reader behaviour has to be checked in the Tauri app on Windows or
Android.

Why. `scripts/live.mjs` and `README.md` (section "실행") describe this
launcher and its limit. `e2e/seed.json`, posted through
`POST /test/simulated-device`, attaches a stand-in capture device so the
browser build can be driven like a phone.

What goes wrong. The first run spends minutes on the Rust build, which
looks like a hang. Judging reader translation from the browser build shows
only the "use the app" notice. Two hosts on the same `PORT` collide.
