# P41 Leptos Readiness Dashboard Summary

Date: 2026-05-09

## Goal

Make the pure Rust UI closer to the intended old-layout workflow: a dashboard with visible readiness layers, one-click Apply, and issue reporting, while keeping details in diagnostics.

## Changes

- Updated `ui/src/app.rs`:
  - added a `readiness_snapshot` signal fed by the existing `health` command
  - dashboard status cards now reflect latest health for Provider configured, Desktop readback, and gateway running state
  - readiness list now shows static config, Desktop readback, provider smoke, and gateway smoke as pass/check/pending
  - added a dashboard common-action bar for Health, Apply, and Report issue
  - reused existing Rust command handlers; no hand-written JS business logic was added
- Updated `ui/styles.css`:
  - added compact dashboard action bar styles
  - added pass/fail/pending readiness badges and issue strip styles
- Updated docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `trunk build --release` | Passed |
| `cargo test --workspace` | Passed; 100 passed, 1 ignored real Desktop smoke |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo tauri build` | Passed; produced Windows MSI and NSIS bundles |
| `trunk serve --address 127.0.0.1 --port 1421 --open false` + Playwright desktop/mobile screenshots | Passed; no console errors, one known Trunk/SRI Chromium warning |

## Notes

This is a UI clarity improvement only. It does not complete Windows real Claude Desktop local config smoke or macOS arm64/x64 smoke evidence.

## Next Minimum Task

Handle the remaining external gates: Windows managed policy cleanup with explicit approval or unmanaged profile, then real local config smoke; macOS arm64/x64 workflow execution; macOS real Claude Desktop smoke.
