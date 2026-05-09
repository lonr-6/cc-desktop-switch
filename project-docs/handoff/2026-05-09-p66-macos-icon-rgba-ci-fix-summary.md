# P66 macOS Icon RGBA CI Fix Summary

## Result

In progress.

## What Changed

- Converted `frontend/assets/app-icon.png` from RGB PNG to RGBA PNG.
- Added `#[cfg(windows)]` to the PowerShell `Command` import used only by a Windows release-gate compatibility test.
- Recorded the CI failure in `project-docs/bugs/2026-05-09-macos-tauri-icon-rgba-ci-failure.md`.
- Updated `project-docs/status.md` and `PLANS.md`.

## Remote Evidence

- Workflow run: `25596145265`.
- Event: `push`.
- Workflow: `Rust Mainline Platform Smoke`.
- arm64 job: `75142311169`, runner architecture check passed, failed in `Rust workspace gate`.
- x64 job: `75142311182`, runner architecture check passed, failed in `Rust workspace gate`.
- Failure: `frontend/assets/app-icon.png is not RGBA`.

## Verification

- Local PNG header check now reports color type `6` for `frontend/assets/app-icon.png`.
- A fresh macOS workflow rerun is still required before this can become pass evidence.

## Next Step

- Run local fmt/test/clippy checks.
- Commit and push P66.
- Watch the next `Rust Mainline Platform Smoke` run.
