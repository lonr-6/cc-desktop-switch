# P75 Frontend Scroll / Installer / Parity Root Cause

## Symptoms

- Windows NSIS `.exe` installer did not inherit the previous install directory while MSI did.
- Rust/Leptos UI pages could not scroll in the desktop window.
- The Rust UI had visual parity but still missed many current frontend actions, button details, and command-backed flows.

## Evidence Read

- Latest frontend reference was read-only from `D:\cc desktop swtich\frontend`.
- Latest frontend pages:
  - `dashboard`
  - `providers/add`
  - `providers`
  - hidden/detail `desktop`
  - `proxy`
  - `settings`
  - `guide`
- Latest frontend actions include:
  - provider actions: enable, test, usage, edit, copy URL, proxy jump, delete
  - provider form actions: base URL menu, auth scheme menu, key reveal, protocol detection, model fetch/check, add/remove mapping row, apply, save, cancel
  - proxy actions: start, stop, clear logs, diagnostics, export diagnostics, auto scroll
  - settings actions: proxy detect, compatibility check, backup/export/import, CC-Switch detect/import, update check/install, feedback
- Latest frontend command surface had 41 command names. Rust mainline had a smaller command surface and several old names were still missing or intentionally not migrated.

## Root Cause

1. NSIS path inheritance only checked stable NSIS keys and the legacy `CC Desktop Switch` uninstall key. MSI installers often register under GUID uninstall keys, so NSIS could miss an MSI-installed location before the directory page.
2. The Rust UI root used `min-height: 100vh` inside a document with `html, body { overflow: hidden; }`. In WebView this can leave no constrained scroll container. The old frontend used a fixed-height shell and scrollable main area.
3. P74 recreated the desktop visual shell but still left some controls as visual placeholders or generic diagnostics actions instead of command-backed equivalents.

## Fix Strategy

- Make `.app-shell` a fixed `height: 100vh` flex column and keep `.app-main` as the only vertical scroll area.
- Extend `RestorePreviousInstallLocation` to read:
  - `${MANUPRODUCTKEY}`
  - `${UNINSTKEY}`
  - legacy HKLM `CC Desktop Switch`
  - HKLM/HKCU uninstall entries with matching `DisplayName`, including MSI/GUID keys.
- Add Rust command/state coverage for settings, config snapshot, proxy status/logs/clear logs, manual backup, clipboard copy, and `configure_desktop` compatibility alias.
- Replace several fake UI actions with real command-backed actions: settings save, backup, proxy logs, copy URL, mapping edits, provider delete, start gateway with saved port.

## Remaining Gaps

- Update check/install, feedback submit, CC-Switch auto-detect/import list, provider usage query, saved-provider-specific speed test, model fetch/autofill, and restart/clear Claude Desktop are not fully reimplemented in Rust yet.
- NSIS inheritance now builds and contains the scan logic, but the actual old-directory preselection still needs Windows in-place GUI installer smoke.
- Final UI parity still needs a full old-frontend action-by-action checklist pass, not only the first P75 repair slice.

## Post-Push CI Finding

- GitHub macOS run `25722210583` failed on arm64 during `cargo test --workspace`.
- Failure: `state::tests::apply_flow_fixture_blocks_port_conflict_before_write` asserted that the Desktop root directory did not exist.
- Root cause: the test used the same timestamp-based name for the config parent and Desktop root. On a fast runner they can be created in the same millisecond, making the directory existence assertion flaky.
- Fix: use a distinct Desktop temp root and assert the actual Desktop config files (`_meta.json` and `cc-desktop-switch-local-gateway.json`) were not written. This keeps the Apply behavior check strict without depending on directory nonexistence.

## Regression

- `trunk build --release`
- Playwright desktop smoke with mocked Tauri bridge:
  - Settings scroll: `scrollTop 0 -> 466`, `scrollHeight 1182`, `clientHeight 716`
  - Add Provider scroll: `scrollTop 0 -> 700`, `scrollHeight 1896`, `clientHeight 716`
  - console errors: `0`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p cc-desktop-switch --lib apply_flow_fixture_blocks_port_conflict_before_write -- --nocapture`
- `cargo test --workspace`
- `cargo tauri build`
- `cargo xtask verify --stage rc-readiness`
