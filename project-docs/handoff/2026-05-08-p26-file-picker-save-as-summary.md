# P26 File Picker Save-As Summary

## Scope

- Added the official Tauri v2 dialog plugin.
- Added save-as commands for Provider export and diagnostics package.
- Added Leptos UI buttons for Provider export `Save as` and diagnostics package `Save as`.
- Did not add restore-from-backup or arbitrary file write commands.

## Implemented

- `tauri-plugin-dialog` is registered in the Tauri builder.
- `save_provider_export_as` opens a JSON save dialog, writes the Provider export package, and returns `None` when the user cancels.
- `save_diagnostics_package_as` opens a JSON save dialog, writes the redacted diagnostics package, and returns `None` when the user cancels.
- The UI surfaces cancel vs saved path explicitly.

## Verification

- `cargo fmt --all`
- `cargo check --workspace`
- `cargo xtask verify --stage file-picker`
- `trunk build --release` in `ui`
- `cargo xtask verify --all` passed: 87 workspace tests, clippy, UI release build, and Windows x64 Tauri build.
- Playwright via `trunk serve --address 127.0.0.1 --port 1421 --open false`: desktop 1440x1000 and mobile 390x900 rendered Save as / preset / mapping controls, had no horizontal overflow, and had no console errors. The only warning was Chromium's Trunk/SRI preload warning.

## Current Limits

- The file picker is not covered by an automated native dialog interaction test.
- Save-as exists for diagnostics package and Provider export only.
- Gateway runtime log capture remains pending.

## Next Minimum Task

1. Add gateway runtime log capture into diagnostics.
2. Add richer import conflict merge controls.
3. Start real Windows Claude Desktop local config smoke.
