# P76 Windows Scroll / Proxy Metrics / NSIS Path Follow-up

## Symptoms

- Manual Windows test still reported that the Rust UI could not scroll.
- Proxy/status page still did not show the expected dynamic stats/log details.
- NSIS `.exe` installer still selected or detected a `C:` location even when existing CC Desktop Switch installs lived under `D:\cc desktop swtich\...`.
- User direction changed the immediate priority: finish Windows first, then return to macOS.

## Evidence Read

- Current worktree was clean before the fix.
- Existing Windows uninstall metadata was read-only inspected and showed multiple installer lineages:
  - MSI `CC Desktop Switch` under a GUID uninstall key with `InstallLocation = D:\cc desktop swtich\CC Desktop Switch\`.
  - NSIS/current-user `CC Desktop Switch` with `InstallLocation = D:\cc desktop swtich\新建文件夹\CC Desktop Switch`.
  - older WOW6432Node `CC Desktop Switch` with `InstallLocation = D:\cc desktop swtich\CC-Desktop-Switch`.
  - unrelated upstream `CC Switch` installed under `C:\Users\15618\AppData\Local\Programs\CC Switch\`.
- P75 NSIS logic could still derive a parent folder from `MsiExec.exe /X{GUID}` and the preinstall hook could overwrite `$INSTDIR` after the main restore function.
- Proxy UI still rendered static metric values instead of binding to `get_proxy_status`.

## Root Cause

1. `.app-shell` still relied on the document root while `body` itself was not the flex container. In WebView this could leave `.app-main` without the same constrained scroll behavior as the old frontend.
2. Proxy metrics/logs had command wrappers but the UI state was still mostly placeholder text; stats cards always rendered `0`.
3. NSIS install path restoration had two competing paths:
   - main template restore logic,
   - `NSIS_HOOK_PREINSTALL` path override.
   The hook used `GetParent` on raw uninstall commands and could reintroduce a bad `$INSTDIR`.
4. MSI uninstall entries often expose `UninstallString = MsiExec.exe /X{GUID}`. Treating that command as a filesystem path can point restoration back to a system/default location instead of the existing app directory.

## Fix

- Made `body` a flex column and made `.app-shell` fill the viewport with `width: 100%`, `min-width: 0`, and `.app-main { height: 0; overflow-y: auto; }`.
- Added `proxy_status` and `proxy_logs` Leptos signals.
- Bound Proxy status label/dot, stats cards, and log rows to `get_proxy_status` / `get_proxy_logs`.
- Reworked `RestorePreviousInstallLocation` to validate candidate install directories before assigning `$INSTDIR`.
- Added parsing from `DisplayIcon` and quoted executable paths, while explicitly ignoring `msiexec`.
- Scanned 64-bit and 32-bit uninstall views for `DisplayName == CC Desktop Switch`.
- Removed install-location mutation from `NSIS_HOOK_PREINSTALL`; the hook now only closes running legacy processes.

## Regression

- `trunk build --release`: pass.
- `cargo fmt --all -- --check`: pass.
- `cargo test --workspace`: pass, 111 passed, 2 ignored real Desktop smoke tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo tauri build`: pass, produced Windows MSI and NSIS bundles.
- Generated NSIS template contains `ValidateInstallLocation`, `DisplayIcon` restore reads, and `msiexec` ignore logic.
- Playwright mocked desktop UI smoke:
  - Settings scroll: `scrollTop 0 -> 473`, `scrollHeight 1182`, `clientHeight 709`.
  - Add Provider scroll: `scrollTop 0 -> 727`, `scrollHeight 1436`, `clientHeight 709`.
  - Proxy metrics: `42 / 39 / 3 / 9`.
  - Proxy log rows after refresh: `2`.
  - Console errors: `0`.

## Remaining

- P76 only proved that the UI can render Proxy stats/log command data. P77 supersedes this part by adding real gateway request stats/log recording.
- Actual Windows GUI installer preselection still needs manual smoke using the new P76 `.exe` and `.msi` packages.
- Full current frontend action parity is still not complete. Remaining work includes update check/install, feedback, provider usage, saved-provider test, model fetch/autofill, restart Claude Desktop, and clear Desktop config.
