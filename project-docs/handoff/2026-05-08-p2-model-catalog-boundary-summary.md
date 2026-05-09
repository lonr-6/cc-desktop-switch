# Stage Summary: P2 ModelCatalog Boundary

Date: 2026-05-08

## Stage

P2 initial `model_catalog` boundary.

## Completed

- Expanded `ModelCatalog` from a single hard-coded route into an explicit mapping based catalog.
- Added `ModelSlot`, `ModelMapping`, `RouteCapabilities`, `RequestOptions`, and richer route resolution output.
- Kept `Default` as a config/form-only mapping: it is skipped from Desktop-visible routes and cannot be resolved at runtime.
- Added Claude-safe route generation using provider/model slugs, with `claude-*` route validation for explicit route IDs.
- Added 1M and Max capability fields to route resolution and Desktop model output.
- Added Max request validation returning `provider.max_not_supported` before gateway adapter work exists.
- Added `cargo xtask verify --stage model-catalog` as the local focused gate for this module.

## Changed Files

- `src-tauri/src/model_catalog.rs`
- `src-tauri/src/desktop.rs` via existing `ModelCatalog::for_provider()` output
- `xtask/src/main.rs`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/eval-harness.md`
- `project-docs/handoff/2026-05-08-p2-model-catalog-boundary-summary.md`

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace` passed: 9 tests.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo xtask verify --stage model-catalog` passed: 7 `model_catalog` tests.
- `trunk build --release` passed from `ui/`.
- `cargo tauri build` passed on Windows x64 after rerunning without a concurrent `trunk build`.

## Covered Rules

- Desktop-visible model IDs are Claude-safe `claude-*` routes.
- Raw upstream model IDs such as `deepseek-v4-pro` are rejected if used as Desktop route IDs.
- `Default` does not enter Desktop models and does not resolve as a runtime fallback.
- Unmapped routes return `gateway.unmapped_model_route`.
- 1M capability is attached only to explicit mapped routes.
- Max requests are rejected with `provider.max_not_supported` unless the selected route supports Max.

## Deferred

- Real provider config schema and migration from Python stable-line config.
- Provider CRUD and persisted model mappings.
- Route collision handling across multiple providers in one catalog.
- Desktop readback health for `supports1m` mismatches.
- Gateway HTTP `/v1/models` and `/v1/messages` handlers.

## Next Step

Build the config schema and migration layer around stable `providerId`, old Python config loading, backup-before-save, and route identity preservation.
