# P47 macOS Real Desktop Smoke Harness Summary

Date: 2026-05-09

## Goal

Add a repeatable opt-in test path for macOS real Claude Desktop local configLibrary smoke without claiming that macOS real evidence has passed.

## Changes

- Updated `src-tauri/src/state.rs`:
  - extracted the Windows real Desktop smoke body into `run_real_desktop_local_config_smoke`
  - kept the same backup, write, readback, loopback gateway smoke, stop-gateway, restore, and restore-check sequence
  - added `macos_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores`
  - kept both real smoke tests `#[ignore]` and gated by `CCDS_ALLOW_REAL_DESKTOP_WRITE=1`
- Updated `docs/testing/eval-harness.md`:
  - added `desktop.real_macos_local_config_smoke`
  - recorded that only the Windows guard run has executed so far
- Updated `project-docs/runbooks/macos-rust-mainline-smoke.md`:
  - added the macOS real Claude Desktop local config smoke command and pass criteria
- Updated status and planning docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo test -p cc-desktop-switch --lib macos_real_desktop_local_config_smoke -- --ignored --nocapture` | Passed on Windows by skipping as macOS-only |
| `cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture` | Passed on Windows by skipping because `CCDS_ALLOW_REAL_DESKTOP_WRITE=1` was not set |
| `cargo test --workspace` | Passed; 110 passed, 2 ignored real Desktop smoke tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; still reports 3 missing evidence checks |

## Result

Partial.

The macOS real Desktop smoke harness exists, but real macOS execution evidence is still missing. This handoff must not be treated as an RC pass record.

## Remaining Gaps

- Windows real Desktop smoke remains blocked by the existing managed policy.
- macOS arm64/x64 workflow smoke has not been run.
- macOS real Desktop smoke must still run on unmanaged macOS arm64 and x64 profiles.

## Next Minimum Task

Run the full workspace test/clippy verification after this harness change, then keep the RC readiness stage failing closed until real Windows/macOS evidence exists.
