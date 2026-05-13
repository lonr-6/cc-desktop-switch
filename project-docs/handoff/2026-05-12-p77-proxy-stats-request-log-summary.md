# P77 Proxy Stats / Request Log Summary

## Scope

P77 supersedes the P76 Proxy stats/log portion. P76 only proved that the UI could render command data; P77 makes the backend generate real local gateway request stats and logs.

## Changed Files

- `src-tauri/src/gateway.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/diagnostics.rs`
- `ui/Cargo.toml`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `project-docs/bugs/2026-05-12-p77-proxy-stats-request-log-root-cause.md`
- `project-docs/status.md`
- `PLANS.md`

## Main Changes

- Added a low-coupling gateway recorder:
  - router tests and library callers still use no-op recorder by default;
  - Tauri runtime injects the AppState recorder when starting the local gateway.
- Added real in-memory request stats:
  - `/v1/models` increments success.
  - `/v1/messages` increments success or failed based on response status.
  - `get_proxy_status` now returns AppState stats instead of fixed zeroes.
- Added request logs for local gateway requests:
  - logs include endpoint, method, status, error code, and safe route id;
  - logs do not expose upstream raw model names.
- Added monotonic log ids and updated UI row keys.
- Implemented the "auto scroll" checkbox through a Leptos signal and `NodeRef`.

## Validation

- `cargo test -p cc-desktop-switch --lib gateway_lifecycle_records_request_stats_and_logs -- --nocapture`: pass.
- `trunk build --release`: pass.
- `cargo test --workspace`: pass, 112 passed, 2 ignored real Desktop smoke tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo fmt --all -- --check`: pass.
- `cargo tauri build`: pass.
- Playwright mocked desktop UI smoke:
  - Proxy stats: `24 / 19 / 5 / 24`.
  - Log rows: `24`.
  - Unique log rows: `24`.
  - Auto-scroll reached bottom.
  - Console errors: `0`.

## Staged Artifacts

- Desktop folder: `C:\Users\15618\Desktop\CCDS-P77-Windows-Manual-Test-20260512`
- Contents:
  - `CC Desktop Switch_1.1.0_x64-setup.exe`
  - `CC Desktop Switch_1.1.0_x64_en-US.msi`
  - `proxy-log-result.json`

## Not Finished

- Windows GUI installer preselection still needs manual upgrade smoke.
- Full latest GitHub CC Desktop Switch feature parity review completed after this fix; see `project-docs/handoff/2026-05-12-p78-github-latest-feature-parity-gap-matrix.md`.
- Update check/install, feedback, provider usage, saved-provider test, model fetch/autofill, restart Claude Desktop, and clear Desktop config are now tracked in the P78 parity gap list.

## Next Minimum Task

Implement the P78 P0 gateway API key auth gap before rebuilding Windows packages for the next manual smoke.
