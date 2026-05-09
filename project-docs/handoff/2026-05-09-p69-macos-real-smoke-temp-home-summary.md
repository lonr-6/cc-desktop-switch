# P69 macOS Real Smoke Temp HOME Summary

## Result

Pass.

## What Changed

- Preserved the original `CARGO_HOME` and `RUSTUP_HOME` while the macOS workflow sets `HOME` to a disposable real-smoke directory.
- Changed the real-smoke workflow step to stage evidence/logs before returning the original wrapper exit code.
- Made macOS smoke artifact upload run with `if: always()` so failures keep their diagnostic evidence.
- Added failure log tail output to `scripts/macos/run-real-desktop-smoke.sh`.
- Recorded the failure in `project-docs/bugs/2026-05-09-macos-real-smoke-temp-home-ci-failure.md`.

## Remote Evidence

### Failed Run

- Workflow run: `25598537347`.
- Event: `push`.
- Workflow: `Rust Mainline Platform Smoke`.
- arm64 job: `75148536967`, passed Rust gate, Leptos build, Tauri build, bundle smoke, failed in `Run macOS real Desktop local config smoke`.
- x64 job: `75148536978`, passed Rust gate, Leptos build, Tauri build, bundle smoke, failed in `Run macOS real Desktop local config smoke`.
- Visible failure output: `result=Fail`, `platform=Darwin`, `configLibraryExists=False`.

### Passing Run

- Workflow run: `25599626985`.
- Event: `push`.
- Workflow: `Rust Mainline Platform Smoke`.
- arm64 job: passed Rust gate, Leptos build, Tauri build, bundle smoke, and macOS real Desktop local config smoke.
- x64 job: passed Rust gate, Leptos build, Tauri build, bundle smoke, and macOS real Desktop local config smoke.
- Artifacts downloaded under `target/github-artifacts/macos-smoke-25599626985-v2`.
- Platform collector wrote `project-docs/handoff/2026-05-09-macos-platform-smoke-evidence-summary.md`.
- Real Desktop collector wrote `project-docs/handoff/2026-05-09-macos-real-desktop-smoke-evidence-summary.md`.

## Verification

- `cargo fmt --all -- --check` passed.
- Git Bash syntax check for `scripts/macos/run-real-desktop-smoke.sh` passed.
- `cargo xtask verify --stage rc-readiness` passed after the two macOS collector handoffs were generated.

## Next Step

- Commit and push the P69 pass evidence.
- Continue the UI/UX consistency pass and final human-test preparation.
