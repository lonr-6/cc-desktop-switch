# macOS Tauri Icon Type CI Failure

## Symptom

`Rust Mainline Platform Smoke` run `25596697486` passed Rust workspace gate and Leptos release build on macOS arm64 and x64, then failed during `cargo tauri build`.

## Evidence

- arm64 job `75143754687` failed in `Build Tauri app`.
- x64 job `75143754690` failed in `Build Tauri app`.
- Failure text: `Failed to create app icon: No matching IconType`.

## Root Cause

The bundle config only supplied PNG and ICO assets. After fixing PNG RGBA, Tauri's macOS bundler still lacked a matching macOS icon asset type.

## Fix

- Resized `frontend/assets/app-icon.png` to 1024x1024 RGBA.
- Added `frontend/assets/app-icon.icns`.
- Added `../frontend/assets/app-icon.icns` to `src-tauri/tauri.conf.json`.

## Regression Test

- Local Windows `cargo xtask verify --all` passed before this `.icns` fix.
- Next required verification is a rerun of `Rust Mainline Platform Smoke` on macOS arm64 and x64.
