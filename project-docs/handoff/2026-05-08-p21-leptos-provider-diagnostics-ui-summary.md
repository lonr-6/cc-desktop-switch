# P21 Leptos Provider/Diagnostics UI Summary

Date: 2026-05-08
Target: `v1.1.0-rc1`
Worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`

## Result

P21 expanded the pure Rust Leptos UI from command spike buttons into usable Provider and Diagnostics flows.

Implemented:

- Provider list refresh and selection.
- Provider edit/save with stable `providerId`.
- Provider set-active, delete, and move-first reorder controls.
- Provider export package display and import JSON textarea.
- Provider import preview, import, and replace-import buttons.
- Gateway status/start/stop, Desktop config probe, Apply dry-run, and Apply controls remain available.
- Diagnostics summary and diagnostics package buttons wired to Tauri commands.
- Provider update with blank API key preserves the saved API key to avoid accidental clearing from a non-echoing password field.

Not implemented:

- System clipboard copy for diagnostics summary.
- File picker/save dialog for diagnostics package.
- GitHub Issue open flow.
- Provider backup browser.
- Model mapping edit UI.
- Rich import conflict merge UI beyond preview/import/replace.

## Files Changed

- `src-tauri/src/config.rs`
- `ui/Cargo.toml`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `ui/styles.css`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/handoff/2026-05-08-p21-leptos-provider-diagnostics-ui-summary.md`

## Verification

Passed:

```powershell
cargo fmt --all
trunk build --release
cargo xtask verify --all
trunk serve --address 127.0.0.1 --port 1421 --open false
```

`cargo xtask verify --all` passed on Windows x64 and included:

- `cargo fmt --all -- --check`
- `cargo test --workspace` with 74 tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Browser verification:

- Playwright desktop viewport: 1440x1000.
- Playwright mobile viewport: 390x900.
- Page rendered with no console errors.
- Known warning: Chromium reports Trunk-generated preload SRI warning; this is not a runtime app error.

Known Tauri build warning:

- `cargo tauri build` still reports the existing Cargo PDB filename collision warning between the package bin and lib target, but the Windows x64 app/MSI/NSIS bundles build successfully.

## Blockers

- Real Windows Claude Desktop local config smoke is still pending.
- macOS arm64 and macOS x64 build/smoke remain hard gates for `v1.1.0-rc1`.
- GitHub Issue flow and native save/copy actions are still missing.
- Real Provider smoke requires a user-supplied API key or redacted diagnostics package.

## Next Minimum Task

Implement report issue native actions:

1. Copy diagnostics summary to the system clipboard.
2. Save diagnostics package through a Tauri file dialog.
3. Open a prefilled GitHub Issue URL without publishing any release assets.
4. Keep all exported content redacted.
