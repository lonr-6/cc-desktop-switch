# P75 Frontend Parity / Scroll / Installer Summary

## Scope

P75 is a root-cause correction pass after manual feedback that P74 still did not fully restore the current frontend.

## Changed Files

- `src-tauri/src/config.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `ui/styles.css`
- `windows/nsis-installer.nsi`
- `project-docs/bugs/2026-05-12-p75-frontend-scroll-installer-parity-root-cause.md`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/eval-harness.md`

## Main Changes

- Read the latest frontend reference from the old Tauri worktree and captured the page/action/command surface as the parity baseline.
- Fixed desktop scrolling by using a fixed-height flex app shell and a constrained scrollable `.app-main`.
- Added command-backed Rust API coverage for:
  - redacted config snapshot
  - settings read/update
  - proxy status/logs/clear logs aliases
  - manual config backup
  - generic clipboard copy
  - `configure_desktop` alias over the transaction apply flow
- Wired more UI controls to real actions:
  - settings save
  - start gateway after saving port
  - proxy log refresh/clear
  - manual backup
  - provider URL copy
  - provider delete buttons
  - visible model mapping rows for Default / Opus / Sonnet / Haiku
- Hardened NSIS install location restoration to scan MSI/GUID uninstall entries, not only old NSIS keys.

## Validation

- `cargo fmt --all -- --check`: pass
- `trunk build --release`: pass
- `cargo check --workspace`: pass
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `cargo test --workspace`: pass, 111 passed, 2 ignored real Desktop smoke tests
- Post-push macOS run `25722210583` exposed a flaky arm64-only test assumption in `apply_flow_fixture_blocks_port_conflict_before_write`; the follow-up fix now checks that Desktop config files were not written instead of requiring the temp directory itself to be absent.
- `cargo test -p cc-desktop-switch --lib apply_flow_fixture_blocks_port_conflict_before_write -- --nocapture`: pass
- Follow-up `cargo test --workspace`: pass, 111 passed, 2 ignored real Desktop smoke tests
- Follow-up `cargo clippy --workspace --all-targets -- -D warnings`: pass
- Playwright mocked desktop UI smoke: pass, no console errors, scroll verified on Settings and Add Provider
- `cargo tauri build`: pass, produced Windows MSI and NSIS bundles
- `cargo xtask verify --stage rc-readiness`: pass

## Not Finished

- Full current-frontend parity is still not complete. Remaining features include update install flow, feedback modal/submit, CC-Switch auto-detect list/import, provider usage query, provider-specific saved test, model fetch/autofill, restart Claude Desktop, and clear Desktop config.
- Windows NSIS old-directory selection must still be confirmed by manual GUI upgrade smoke over an existing MSI/old install.
- New P75 artifacts still need push-triggered macOS arm64/x64 non-publishing workflow evidence before final RC packaging.

## Next Minimum Task

Run a second current-frontend parity slice that implements the remaining modal/action flows in Rust, then execute Windows in-place installer smoke with both MSI and NSIS paths.
