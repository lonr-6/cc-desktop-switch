# P82 Clear And Restart Desktop Root Cause

## Problem

P78 and the P81 follow-up reviews found two remaining P0 user-visible gaps:

- The header action for clearing Claude Desktop configuration only navigated to the Desktop page.
- The UI only told the user to restart Claude Desktop; there was no command-backed restart action.

Visible no-op controls are high risk because users may believe Claude Desktop has been reset or restarted when nothing happened.

## Root Cause

The Rust mainline had a solid apply/write/readback path, but `desktop_writer` only supported writing and reading the `configLibrary` profile. There was no inverse operation for removing the CCDS-managed local gateway profile, and no platform command for a restart request.

## Fix

- Added `DesktopClearResult` and `clear_local_config_library`.
- Clear only removes `cc-desktop-switch-local-gateway.json` and removes that profile from `_meta.json` when it is active.
- Unrelated `configLibrary` profiles and unknown metadata are preserved.
- The clear command probes the current Desktop path first and marks the result unsuccessful if managed policy evidence remains.
- Added `DesktopRestartResult` and `restart_claude_desktop`.
- Windows restart requests stop `Claude.exe` if running, then launch the first known installed `Claude.exe` path; `.msi` or installer flows are not involved.
- macOS restart requests use bundle id `com.anthropic.claudefordesktop`.
- The UI now asks for confirmation before clear/restart, shows a restart reminder after successful apply, and exposes real buttons for clear and restart.

## Verification

- `cargo test -p cc-desktop-switch --lib desktop_writer -- --nocapture` passed.
- `cargo test --workspace` passed: 123 passed, 2 ignored real Desktop smoke tests.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `trunk build --release` passed.
- `cargo tauri build` passed.
- `cargo xtask verify --stage rc-readiness` passed.

## Remaining Risk

- Windows restart was not run against a real installed Claude Desktop in this session.
- macOS restart requires real macOS verification.
- If Claude Desktop is installed outside known Windows locations, restart will fail clearly instead of pretending success.
