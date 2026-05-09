# Stage Summary: P4 Provider Service Boundary

Date: 2026-05-08

## Stage

P4 persisted provider service and Tauri command boundary.

## Completed

- Extended `ProviderDraft` with optional stable `provider_id`.
- Reworked `AppState` to load/save `AppConfig` through a configurable config path.
- Added persisted provider save/list/set-active behavior.
- Kept provider summaries secret-safe: API keys are not returned to UI summaries.
- Built active-provider snapshots with model mappings for downstream Desktop/gateway planning.
- Added Tauri commands for `list_providers` and `set_active_provider`.
- Added `cargo xtask verify --stage provider-service` as the focused local gate.

## Changed Files

- `src-tauri/src/provider.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `xtask/src/main.rs`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/eval-harness.md`
- `project-docs/handoff/2026-05-08-p4-provider-service-boundary-summary.md`

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace` passed.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo xtask verify --stage provider-service` passed.
- `trunk build --release` passed from `ui/`.
- `cargo tauri build` passed on Windows x64.

## Covered Rules

- Provider save is persisted instead of only held in memory.
- UI-facing provider summaries do not echo secrets.
- Active provider and model mappings are available to planner/gateway code.
- The config repository remains below state/commands; UI does not own provider policy.

## Deferred

- Provider update/delete and collision-aware import.
- Backup list and restore UI.
- Full model mapping editor.
- Secret storage beyond config-file persistence.

## Next Step

Use the active provider snapshot to build the platform-neutral Desktop apply plan and health/readback comparison.
