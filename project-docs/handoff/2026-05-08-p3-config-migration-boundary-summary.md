# Stage Summary: P3 Config Migration Boundary

Date: 2026-05-08

## Stage

P3 initial config schema and migration boundary.

## Completed

- Added `src-tauri/src/config.rs` as the Rust schema/migration module.
- Introduced `schemaVersion = 1` Rust config shape with providers, settings, active provider, gateway key, and model mappings.
- Implemented old Python stable-line config loading for configs without `schemaVersion`.
- Migrated old provider fields from `id/name/baseUrl/authScheme/apiFormat/apiKey/models/modelCapabilities`.
- Converted old `models` into explicit `ModelMapping` entries and generated Claude-safe route IDs for non-`Default` slots.
- Preserved existing Rust route IDs when loading a Rust schema after Provider display-name rename.
- Implemented `backup_then_save_config()` so an existing config is copied into `backups/` before replacement.
- Added `cargo xtask verify --stage config` as the focused local gate.

## Changed Files

- `src-tauri/src/config.rs`
- `src-tauri/src/lib.rs`
- `xtask/src/main.rs`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/eval-harness.md`
- `project-docs/handoff/2026-05-08-p3-config-migration-boundary-summary.md`

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace` passed: 13 tests.
- `cargo clippy --workspace --all-targets -- -D warnings` passed.
- `cargo xtask verify --stage config` passed: 4 config tests.
- `trunk build --release` passed from `ui/`.
- `cargo tauri build` passed on Windows x64 and produced MSI/NSIS bundles.

## Covered Rules

- Old Python stable-line configs without `schemaVersion` load into Rust schema.
- Migration is in-memory first; replacement save uses backup-before-save.
- `providerId` is stable and independent from display name.
- Existing route IDs survive Provider display-name rename.
- `Default` remains config/form-only and does not become a runtime route.

## Deferred

- Real app data path integration for `~/.cc-desktop-switch/config.json`.
- Provider CRUD commands backed by the config repository.
- Import/export UI and full backup listing.
- Duplicate provider ID collision handling during large imports.
- Gateway and Desktop writer use of persisted config.

## Next Step

Wire the config/provider schema into Tauri commands: list providers, save/update provider, set active provider, and build `ModelCatalog` from persisted config rather than the built-in fixture.
