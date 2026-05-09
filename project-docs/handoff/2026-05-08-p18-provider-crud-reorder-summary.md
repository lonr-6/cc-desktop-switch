# P18 Provider CRUD/Reorder Summary

Date: 2026-05-08
Target: `v1.1.0-rc1`
Worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`

## Result

P18 established the initial Provider parity boundary for edit, delete, and reorder.

Implemented:

- Provider edit uses stable `providerId`; metadata changes preserve existing model mappings and sort order.
- Provider delete persists to config, normalizes sort order, and moves `activeProvider` to the first remaining provider when the active provider is deleted.
- Provider reorder requires an exact provider ID set; missing, duplicate, or unknown IDs fail instead of partially reordering.
- Tauri commands expose `delete_provider` and `reorder_providers`.
- Leptos spike can call Provider list/delete/reorder through typed Rust/WASM bindings.
- Running local gateway now restarts when the active provider config or model mappings change, preventing stale runtime provider state.
- Gateway restart fingerprint is stored as a hash, not as raw provider JSON with API keys.

Not implemented:

- Provider import/export schema.
- CC-Switch legacy import beyond existing config migration fixture.
- UI-grade Provider table with selection, editing states, validation messages, and conflict merge UX.
- Backup browser/list UI.

## Files Changed

- `src-tauri/src/config.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `ui/src/commands.rs`
- `ui/src/app.rs`
- `xtask/src/main.rs`
- `docs/testing/eval-harness.md`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/handoff/2026-05-08-p18-provider-crud-reorder-summary.md`

## Verification

Passed:

```powershell
cargo fmt --all
cargo xtask verify --stage provider-parity
cargo fmt --all -- --check
cargo xtask verify --stage provider-service
cargo xtask verify --all
```

`cargo xtask verify --all` passed on Windows x64 and included:

- `cargo fmt --all -- --check`
- `cargo test --workspace` with 65 tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Known warning:

- `cargo tauri build` still reports the existing Cargo PDB filename collision warning between the package bin and lib target, but the Windows x64 app/MSI/NSIS bundles build successfully.

## Eval Coverage

Added `provider.crud_reorder_roundtrip`:

- Edit/delete/reorder persisted roundtrip.
- Summary secret redaction after edit.
- Exact-set reorder validation.
- Active provider reassignment after delete.
- Gateway restart when active provider fingerprint changes.

Focused command:

```powershell
cargo xtask verify --stage provider-parity
```

## Blockers

- Provider import/export and CC-Switch import conflict merge are still missing.
- Real Windows Claude Desktop local config smoke is still pending.
- macOS arm64 and macOS x64 build/smoke remain hard gates for `v1.1.0-rc1`.
- Diagnostics export package and report issue flow are still missing.

## Next Minimum Task

Implement Provider import/export/CC-Switch import boundary:

1. Define import/export JSON schema for Rust Provider config slices.
2. Add parser tests for valid export, duplicate provider IDs, missing fields, and legacy CC-Switch shape.
3. Keep `Default` out of runtime fallback and Desktop model menu during import.
4. Add dry-run import result before writing config.
