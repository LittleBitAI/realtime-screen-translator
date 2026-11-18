---
scope: project
severity: contract
repeat: rule
triggers: ["게이트", "gate", "테스트", "test", "빌드", "build", "검증", "nextest", "vitest", "xtask", "CI", "junit"]
slots: [gate_cmd]
sources: [.wiki/adapter.toml, .github/workflows/ci.yml, README.md]
links: [project, live-web-host]
---

# The gate is xtask + pnpm test + pnpm build, run from PowerShell 7 with `artifacts/` present

Rule. A change is done only when `gate_cmd` in `.wiki/adapter.toml`
passes:
`cargo run -q -p xtask -- artifacts/junit.xml && corepack pnpm install --frozen-lockfile && corepack pnpm test && corepack pnpm build`.
On Windows, run it in PowerShell 7 (`pwsh`). Windows PowerShell 5.1 does
not understand `&&`. Create `artifacts/` first: Git ignores it, a fresh
checkout lacks it, and xtask does not create it. cargo-nextest must be
installed, and pnpm comes through Corepack at the version pinned in
`package.json`. Report a run as passed only when all four steps ran; a
run cut short is not a pass.

Why. `README.md` (section "테스트") names this command as the repository
gate, and `.github/workflows/ci.yml` runs the same line on `windows-latest`
under `pwsh`, after a separate step that makes `artifacts/`. xtask runs
`cargo nextest run --workspace --exclude xtask --profile ci` and copies
nextest's JUnit report to the path it is given; Vitest writes
`artifacts/vitest-junit.xml`.

What goes wrong. Under 5.1 the line fails to parse before any test runs.
Without `artifacts/`, xtask cannot write the report. Skipping
`corepack pnpm build` misses Svelte and Tailwind errors that tests do not
catch. Gemini is faked inside the Rust tests, so no API key is needed;
the Windows-only `--features app` checks and the `real_model` test are not
part of the gate (`README.md`, "구조").
