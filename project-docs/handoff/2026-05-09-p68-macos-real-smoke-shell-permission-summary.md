# P68 macOS Real Smoke Shell Permission Summary

## Result

In progress.

## What Changed

- Updated `.github/workflows/rust-mainline-platform-smoke.yml` to invoke the real-smoke wrapper through `bash`.
- Set `scripts/macos/run-real-desktop-smoke.sh` executable in git.
- Recorded the failure in `project-docs/bugs/2026-05-09-macos-real-smoke-shell-permission-ci-failure.md`.
- Updated `project-docs/status.md` and `PLANS.md`.

## Remote Evidence

- Workflow run: `25597520723`.
- Event: `push`.
- Workflow: `Rust Mainline Platform Smoke`.
- arm64 job: `75145871129`, passed Rust gate, Leptos build, Tauri build, bundle smoke, failed in `Run macOS real Desktop local config smoke`.
- x64 job: `75145871130`, passed Rust gate, Leptos build, Tauri build, bundle smoke, failed in `Run macOS real Desktop local config smoke`.
- Failure: `scripts/macos/run-real-desktop-smoke.sh: Permission denied`.

## Verification

- A fresh `rc-readiness` static check is required after this workflow change.
- A fresh macOS workflow rerun is required before this can become pass evidence.

## Next Step

- Commit and push P68.
- Watch the next `Rust Mainline Platform Smoke` run.
