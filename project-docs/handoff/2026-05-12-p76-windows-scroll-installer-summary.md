# P76 Windows Scroll / Installer Follow-up Summary

## Scope

P76 is a Windows-first correction pass after manual feedback that P75 still failed installed-app scroll behavior and NSIS `.exe` install location inheritance.

Note: P76 only bound Proxy stats/log UI to command data. P77 supersedes the Proxy stats/log backend portion by adding real gateway request stats and request logs.

## Changed Files

- `ui/src/app.rs`
- `ui/styles.css`
- `windows/nsis-installer.nsi`
- `windows/nsis-hooks.nsh`
- `project-docs/bugs/2026-05-12-p76-windows-scroll-installer-follow-up.md`
- `project-docs/status.md`
- `PLANS.md`

## Main Changes

- Reworked the Rust UI shell so `body`, `.app-shell`, and `.app-main` match the old desktop app scroll model more closely.
- Replaced Proxy page placeholder stats with `get_proxy_status` data.
- Replaced Proxy log placeholder body with command-backed log rows from `get_proxy_logs`.
- Hardened NSIS old-path restore:
  - validates candidate directories before using them;
  - reads `InstallLocation`, `DisplayIcon`, and executable-style uninstall strings;
  - ignores `MsiExec.exe /X{GUID}` as a path source;
  - scans both 64-bit and 32-bit uninstall views;
  - removes `$INSTDIR` mutation from the preinstall hook.

## Validation

- `trunk build --release`: pass.
- `cargo fmt --all -- --check`: pass.
- `cargo test --workspace`: pass, 111 passed, 2 ignored real Desktop smoke tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo tauri build`: pass.
- Generated Windows bundles:
  - `target/release/bundle/nsis/CC Desktop Switch_1.1.0_x64-setup.exe`
  - `target/release/bundle/msi/CC Desktop Switch_1.1.0_x64_en-US.msi`
- Generated NSIS template check:
  - `ValidateInstallLocation` present.
  - `DisplayIcon` restore reads present.
  - `msiexec` ignore logic present.
- Playwright mocked desktop UI smoke:
  - Settings scroll: `0 -> 473`.
  - Add Provider scroll: `0 -> 727`.
  - Proxy stats: `42 / 39 / 3 / 9`.
  - Proxy log rows after refresh: `2`.
  - Console errors: `0`.

## Staged Artifacts

- Desktop folder: `C:\Users\15618\Desktop\CCDS-P76-Windows-Manual-Test-20260512`
- Contents:
  - `CC Desktop Switch_1.1.0_x64-setup.exe`
  - `CC Desktop Switch_1.1.0_x64_en-US.msi`
  - `playwright-result.json`
  - `proxy-log-result.json`

## Not Finished

- The new NSIS `.exe` path inheritance has been build-verified but not manually confirmed in the GUI installer.
- Real installed-app scroll still needs the human Windows smoke because browser smoke uses a mocked Tauri bridge.
- macOS was intentionally not re-run in this pass because the current priority is Windows first.
- Proxy request stats/log backend completion is tracked in `project-docs/handoff/2026-05-12-p77-proxy-stats-request-log-summary.md`.

## Next Minimum Task

Run the P76 Windows manual smoke from the desktop folder:

1. Launch the NSIS `.exe` over the existing old install and confirm the directory page preselects the existing `D:` install directory.
2. Launch/install the MSI only if NSIS passes or if MSI comparison is needed.
3. Open the installed app and verify Dashboard / Provider / Proxy / Settings / Guide can scroll where content exceeds the window.
4. Verify Proxy stats/log buttons, taskbar icon, tray icon, and saved Provider config inheritance.
