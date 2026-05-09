# P19 Provider Import/Export Summary

Date: 2026-05-08
Target: `v1.1.0-rc1`
Worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`

## Result

P19 established the initial Provider import/export boundary.

Implemented:

- Rust Provider export package schema: `kind = ccds.providerExport`, `schemaVersion = 1`.
- Import preview that does not write config.
- Import apply that writes only when the preview allows it.
- Conflict dry-run: existing provider IDs block writes unless `replaceExisting = true`.
- Legacy CC-Switch/Python stable config import through the existing migration parser.
- Duplicate provider IDs inside one import package are rejected.
- Raw route IDs inside import packages are rejected.
- `Default` mappings are forced to `routeId = null`, so they remain config/form convenience only and cannot become runtime fallback or Desktop-visible routes.
- Tauri commands expose `export_providers`, `preview_provider_import`, and `import_providers`.
- Added focused eval stage `provider-import`.

Not implemented:

- Full Provider import/export UI.
- Backup browser/list UI.
- Rich conflict merge UX beyond block-or-replace.
- Preset marketplace/template import.

## Files Changed

- `src-tauri/src/config.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `xtask/src/main.rs`
- `docs/testing/eval-harness.md`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/handoff/2026-05-08-p19-provider-import-export-summary.md`

## Verification

Passed:

```powershell
cargo fmt --all
cargo xtask verify --stage provider-import
cargo xtask verify --all
```

`cargo xtask verify --all` passed on Windows x64 and included:

- `cargo fmt --all -- --check`
- `cargo test --workspace` with 71 tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Known warning:

- `cargo tauri build` still reports the existing Cargo PDB filename collision warning between the package bin and lib target, but the Windows x64 app/MSI/NSIS bundles build successfully.

## Eval Coverage

Added `provider.import_export_roundtrip`:

- Provider export package roundtrip.
- Legacy CC-Switch config import.
- Dry-run preview does not write config.
- Conflict blocks write until replace is requested.
- Duplicate provider IDs are rejected.
- Raw route IDs are rejected.
- `Default` stays non-runtime and is still rejected by `ModelCatalog.resolve_route("Default")`.

Focused command:

```powershell
cargo xtask verify --stage provider-import
```

## Blockers

- Provider import/export UI is still missing.
- Provider backup list and richer conflict merge UX are still missing.
- Diagnostics export package and report issue flow are still missing.
- Real Windows Claude Desktop local config smoke is still pending.
- macOS arm64 and macOS x64 build/smoke remain hard gates for `v1.1.0-rc1`.

## Next Minimum Task

Implement diagnostics export package boundary:

1. Define a diagnostics package schema with app/config/gateway/Desktop health sections.
2. Redact all secrets using the existing redaction core.
3. Add copy-summary and export-package Tauri commands.
4. Add eval fixtures for false-green readiness and secret redaction.
