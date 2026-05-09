# macOS Real Smoke Temp HOME CI Failure

## Symptom

`Rust Mainline Platform Smoke` run `25598537347` reached the macOS real Desktop local config smoke on both runners, then failed with only wrapper-level output.

## Evidence

- arm64 job `75148536967` passed Rust gate, Leptos build, Tauri build, bundle smoke, DMG verify, and PKG create/expand.
- x64 job `75148536978` passed Rust gate, Leptos build, Tauri build, bundle smoke, DMG verify, and PKG create/expand.
- Both jobs failed in `Run macOS real Desktop local config smoke`.
- Visible failure output only included `result=Fail`, `platform=Darwin`, architecture, and `configLibraryExists=False`.
- The artifact upload step was skipped after the failing real-smoke step, so the generated cargo test log was not available from the workflow artifacts.

## Root Cause

The workflow intentionally sets `HOME` to a temporary directory so the smoke uses a disposable Claude Desktop `configLibrary` path. That also changes the default home used by Rust tooling in the same process. The failure log was redirected into the wrapper log, but the workflow did not preserve or upload the log on failure.

## Fix

- Preserve the original Rust toolchain homes by setting `CARGO_HOME` and `RUSTUP_HOME` after switching `HOME`.
- Capture the real-smoke exit code, copy evidence/log files into the artifact staging directory, then exit with the original code.
- Run `actions/upload-artifact` with `if: always()` so failed real-smoke attempts still produce downloadable evidence.
- Print the tail of the cargo test log when the macOS wrapper fails.

## Regression Test

- `cargo xtask verify --stage rc-readiness` must still recognize the real-smoke workflow command and artifact path.
- The next `Rust Mainline Platform Smoke` run should either pass both macOS real-smoke jobs or upload enough evidence/logs to identify the next concrete blocker.

## Resolution

Run `25599626985` passed both macOS arm64 and macOS x64 jobs. The downloaded artifacts were validated by both macOS collectors, and `cargo xtask verify --stage rc-readiness` passed.
