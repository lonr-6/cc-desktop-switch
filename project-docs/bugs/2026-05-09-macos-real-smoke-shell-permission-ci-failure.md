# macOS Real Smoke Shell Permission CI Failure

## Symptom

`Rust Mainline Platform Smoke` run `25597520723` passed macOS platform build and bundle smoke, then failed before running the real Desktop local config smoke.

## Evidence

- arm64 job `75145871129` passed Rust workspace gate, Leptos build, Tauri build, DMG verify, and PKG create/expand.
- x64 job `75145871130` passed Rust workspace gate, Leptos build, Tauri build, DMG verify, and PKG create/expand.
- Both jobs failed in `Run macOS real Desktop local config smoke`.
- Failure text: `scripts/macos/run-real-desktop-smoke.sh: Permission denied`.

## Root Cause

The macOS workflow called the shell script directly, but the script did not have an executable bit after checkout.

## Fix

- Call the script with `bash scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write`.
- Set the script executable bit in git.

## Regression Test

- `cargo xtask verify --stage rc-readiness` must still find the real-smoke workflow command string.
- Next required verification is a rerun of `Rust Mainline Platform Smoke`.
