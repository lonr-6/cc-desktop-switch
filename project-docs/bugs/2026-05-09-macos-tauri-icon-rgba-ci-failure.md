# macOS Tauri Icon RGBA CI Failure

## Symptom

`Rust Mainline Platform Smoke` run `25596145265` triggered successfully on `codex/rust-mainline-rewrite`, but both macOS jobs failed in `Rust workspace gate`.

## Evidence

- arm64 job `75142311169` and x64 job `75142311182` both passed runner architecture verification.
- Both jobs failed while compiling `cc-desktop-switch`.
- Failure text: `icon ... frontend/assets/app-icon.png is not RGBA`.

## Root Cause

`frontend/assets/app-icon.png` was PNG color type 2 (RGB). Tauri's macOS context generation requires an RGBA PNG icon and panics during `tauri::generate_context!()` when the icon is RGB.

## Fix

- Converted `frontend/assets/app-icon.png` to PNG color type 6 (RGBA).
- Gated the Windows-only `std::process::Command` test import in `src-tauri/src/release_gate.rs` with `#[cfg(windows)]`.

## Regression Test

- Local PNG header check reports `bit_depth 8` and `color_type 6`.
- Next required verification is a rerun of `Rust Mainline Platform Smoke` on macOS arm64 and x64.
