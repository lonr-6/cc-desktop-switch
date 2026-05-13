# P82 Clear And Restart Desktop Summary

## Scope

Closed the remaining P78 P0 command-backed Desktop action gaps after P81 update runtime.

## Changed Files

- `src-tauri/src/desktop_writer.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `ui/Cargo.toml`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `ui/styles.css`
- `project-docs/bugs/2026-05-13-p82-clear-restart-desktop-root-cause.md`
- `project-docs/handoff/2026-05-13-p82-clear-restart-desktop-summary.md`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/python-rust-parity-matrix.md`
- `docs/testing/eval-harness.md`
- `project-docs/handoff/2026-05-12-p78-github-latest-feature-parity-gap-matrix.md`

## Behavior

- `clear_desktop_config` probes the current local `configLibrary` path, clears only the CCDS-managed local gateway profile, preserves unrelated profiles, and verifies the profile is no longer active.
- `restart_claude_desktop` is a separate command-backed action. It reports a structured failure if Claude Desktop cannot be found or launched.
- UI clear/restart buttons now ask for confirmation and call real Tauri commands.
- Successful apply sets a visible restart reminder.

## Desktop Package Cleanup

- Deleted `C:\Users\15618\Desktop\CCDS-P81-Windows-Update-Runtime-Manual-Test-20260512`.
- Created latest manual-test bundle:
  - `C:\Users\15618\Desktop\CCDS-P82-Windows-Clear-Restart-Manual-Test-20260512`
- `C:\Users\15618\Desktop\CCDS-P74-Manual-Test-20260512` remains as an empty locked directory.

## Verification

- `cargo test -p cc-desktop-switch --lib desktop_writer -- --nocapture` passed.
- `cargo test --workspace` passed: 123 passed, 2 ignored real Desktop smoke tests.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `trunk build --release` passed.
- `cargo tauri build` passed.
- `cargo xtask verify --stage rc-readiness` passed.

## Not Run

- Real Windows Claude Desktop restart from the installed app.
- Windows installer GUI upgrade smoke.
- macOS clear/restart smoke.
- Public update URL end-to-end release smoke.

## Next Minimum Task

Refresh `project-docs/runbooks/final-human-rc-smoke.md` and add a release workflow guard before any RC packaging/publishing work. Then run Windows manual upgrade smoke on the P82 bundle.
