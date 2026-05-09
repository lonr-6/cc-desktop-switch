# P69 macOS Real Smoke Temp HOME Summary

## Result

In progress.

## What Changed

- Preserved the original `CARGO_HOME` and `RUSTUP_HOME` while the macOS workflow sets `HOME` to a disposable real-smoke directory.
- Changed the real-smoke workflow step to stage evidence/logs before returning the original wrapper exit code.
- Made macOS smoke artifact upload run with `if: always()` so failures keep their diagnostic evidence.
- Added failure log tail output to `scripts/macos/run-real-desktop-smoke.sh`.
- Recorded the failure in `project-docs/bugs/2026-05-09-macos-real-smoke-temp-home-ci-failure.md`.

## Remote Evidence

- Workflow run: `25598537347`.
- Event: `push`.
- Workflow: `Rust Mainline Platform Smoke`.
- arm64 job: `75148536967`, passed Rust gate, Leptos build, Tauri build, bundle smoke, failed in `Run macOS real Desktop local config smoke`.
- x64 job: `75148536978`, passed Rust gate, Leptos build, Tauri build, bundle smoke, failed in `Run macOS real Desktop local config smoke`.
- Visible failure output: `result=Fail`, `platform=Darwin`, `configLibraryExists=False`.

## Verification

- Run `cargo fmt --all -- --check`.
- Run `cargo xtask verify --stage rc-readiness`; the expected result before macOS evidence is still incomplete with only the two macOS evidence gaps.
- Push P69 and watch the next `Rust Mainline Platform Smoke` run.

## Next Step

- Commit and push P69.
- If the next run passes, download artifacts and run both macOS collectors.
- If the next run fails, download the now-preserved artifacts and fix the concrete logged blocker.
