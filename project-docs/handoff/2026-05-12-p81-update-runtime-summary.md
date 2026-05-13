# P81 Update Runtime Summary

## Scope

Implemented the P78 P0 update runtime gap without publishing, tagging, or uploading release assets.

## Changed Files

- `src-tauri/src/update.rs`
- `src-tauri/src/release_gate.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/commands.rs`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `project-docs/bugs/2026-05-12-p81-update-runtime-root-cause.md`
- `project-docs/handoff/2026-05-12-p81-update-runtime-summary.md`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/python-rust-parity-matrix.md`
- `docs/testing/eval-harness.md`
- `project-docs/handoff/2026-05-12-p78-github-latest-feature-parity-gap-matrix.md`

## Behavior

- `check_update` downloads and parses `latest.json`, picks the current-platform installer, and reports availability.
- `download_update` stages `latest.json`, latest sidecars, public key, selected installer, selected installer hash, and selected installer signature, then verifies the bundle through release-gate logic.
- `install_update` launches the verified installer path; Windows setup `.exe` launches directly and `.msi` uses `msiexec.exe /i`.
- UI Settings exposes command-backed actions for checking update, downloading/verifying, and launching the staged installer.

## Cleanup

- Deleted old desktop package folders:
  - `C:\Users\15618\Desktop\CCDS-P80-Windows-Gateway-Auth-Manual-Test-20260512`
  - `C:\Users\15618\Desktop\CCDS-TestBuilds`
- Created latest manual-test bundle:
  - `C:\Users\15618\Desktop\CCDS-P81-Windows-Update-Runtime-Manual-Test-20260512`
- `C:\Users\15618\Desktop\CCDS-P74-Manual-Test-20260512` is empty but Windows still reports the directory itself is used by another process, so it remains.

## Verification

- `cargo test -p cc-desktop-switch --lib update -- --nocapture` passed.
- `cargo test -p cc-desktop-switch --lib update_bundle_verifies_selected_asset_without_requiring_all_platform_assets -- --nocapture` passed.
- `cargo test --workspace` passed: 121 passed, 2 ignored real Desktop smoke tests.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `trunk build --release` passed.
- `cargo tauri build` passed.
- `cargo xtask verify --stage rc-readiness` passed.

## Not Run

- Public update URL end-to-end test, because this work intentionally did not publish or alter release metadata.
- Windows installer GUI upgrade smoke, because it requires human interaction with the generated installer.
- macOS install/update smoke, because no macOS real machine is attached to this Windows session.

## Next Minimum Task

Continue P78 P0 by implementing command-backed clear Claude Desktop config and restart Claude Desktop flows. Then refresh the final human smoke runbook and release workflow guard before any RC packaging. Do not leave visible no-op buttons in the UI.
