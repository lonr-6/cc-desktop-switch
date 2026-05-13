# P81 Update Runtime Root Cause

## Problem

P78 identified that Rust mainline had release metadata validation but no user-visible update runtime. The UI exposed an update address field, but there was no command-backed flow to check update metadata, download an installer, verify sidecars/signatures, and launch the installer.

## Root Cause

Update safety existed only as release/staging gates:

- `release_gate` could validate full release directories.
- UI settings held an update URL but could not execute an update flow.
- Tauri commands did not expose update check/download/install actions.

That meant the app could pass release metadata tests while still lacking the installed-app update behavior users expect.

## Fix

- Added `src-tauri/src/update.rs` for runtime update check/download/verify/install.
- Reused release verification through `validate_update_bundle`, so runtime downloads must include `latest.json`, `latest.json.sha256`, `latest.json.sig`, release public key, installer asset, asset `.sha256`, and asset `.sig`.
- Added current-platform asset selection for `windows-x64`, `macos-arm64`, and `macos-x64`.
- Added staged error codes for invalid URL, request failure, invalid manifest, unsupported platform, missing asset, download failure, verification failure, and install launch failure.
- Added Tauri commands and Rust/WASM UI command bindings for check, download/verify, and install.
- Added Settings UI buttons for checking, downloading/verifying, and launching the verified installer.
- Added update bundle validation that verifies only the selected runtime installer without requiring every release-platform asset during a user update.
- Added default release public key fallback and Windows `.msi` launch through `msiexec.exe /i`.

## Verification

- `cargo test -p cc-desktop-switch --lib update -- --nocapture` passed.
- `cargo test -p cc-desktop-switch --lib update_bundle_verifies_selected_asset_without_requiring_all_platform_assets -- --nocapture` passed.
- `cargo test --workspace` passed: 121 passed, 2 ignored real Desktop smoke tests.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `trunk build --release` passed.
- `cargo tauri build` passed and generated Windows MSI/NSIS bundles.
- `cargo xtask verify --stage rc-readiness` passed.

## Remaining Risk

- End-to-end network update against a public release URL was not run because this is not a publish/release step.
- Human Windows upgrade smoke is still required for installer inheritance, existing config retention, taskbar/tray icon display, and real installer launch behavior.
- macOS update/install behavior still needs final macOS real-machine smoke before RC.
