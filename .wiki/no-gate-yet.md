---
scope: project
severity: contract
repeat: rule
triggers: ["게이트", "gate", "테스트", "test", "빌드", "build", "검증", "lint", "CI", "실행 방법", "어떻게 (돌|실행)"]
slots: []
sources: [.wiki/adapter.toml]
links: [project]
---

# There is no gate, build or run command yet; do not claim one passed

Rule. Until `.wiki/adapter.toml` sets `gate_cmd` and `live_cmd`, this
repository has no build, test, lint or launch command. Do not report a
check as passed that does not exist, and do not invent one from guesses
about the stack. The change that introduces the first code also chooses
the stack's check and writes it into `gate_cmd`, and updates
[[project]] with how to build, run and test.

Why. `.wiki/adapter.toml` leaves `gate_cmd`, `live_cmd` and `server_stop`
empty, and the repository holds no source or manifest; the `.gitignore`
covers several ecosystems, so it does not reveal a stack either.

What goes wrong. A report that says "tests pass" for a repository with no
tests gives the person false confidence, and later agents inherit a
command that was never real.
