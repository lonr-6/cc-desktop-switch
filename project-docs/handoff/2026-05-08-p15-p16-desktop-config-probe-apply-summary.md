# Stage Summary: P15/P16 Desktop Config Probe And Apply

Date: 2026-05-08

## Stage

P15 Desktop config path probe and P16 user-facing Apply command.

## Completed

- Added `DesktopConfigProbe` and managed config evidence types.
- Added platform local config path detection:
  - Windows: `%LOCALAPPDATA%\Claude-3p\configLibrary`, with `USERPROFILE\AppData\Local` fallback.
  - macOS: `~/Library/Application Support/Claude-3p/configLibrary`.
- Added managed config evidence detection:
  - Windows registry policy probe through read-only `reg query`.
  - macOS managed preference file path checks.
- Added `desktop_config_probe` Tauri command.
- Added `apply_detected_local_config` Tauri command.
- Apply now blocks before gateway start and Desktop write if managed config evidence is present.
- Leptos UI now exposes config probe and Apply buttons and prints step-by-step results.

## Changed Files

- `src-tauri/src/desktop_writer.rs`
- `src-tauri/src/apply_flow.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `xtask/src/main.rs`
- `docs/testing/eval-harness.md`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/handoff/2026-05-08-p15-p16-desktop-config-probe-apply-summary.md`

## Verification

- `cargo xtask verify --stage desktop-config` passed: 3 focused config probe tests.
- `cargo xtask verify --stage apply-flow` passed: 5 focused apply-flow tests.
- `cargo xtask verify --all` passed:
  - `cargo fmt --all -- --check`;
  - `cargo test --workspace`: 59 tests;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `trunk build --release`;
  - `cargo tauri build` on Windows x64.

## Covered Rules

- Apply still cannot show success unless gateway start, Desktop write, and readback all pass.
- Managed config evidence blocks local write rather than claiming local config was applied.
- UI does not decide model routing or Desktop paths; it calls typed commands only.

## Deferred

- Windows/macOS real Claude Desktop smoke is still required.
- Managed registry/mobileconfig export is not implemented.
- Loopback HTTPS/TLS decision remains open.

## Next Step

Run real Windows Claude Desktop local config smoke, then repeat on macOS arm64 and x64.
