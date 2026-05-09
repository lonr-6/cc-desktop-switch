# P20 Diagnostics Export Package Summary

Date: 2026-05-08
Target: `v1.1.0-rc1`
Worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`

## Result

P20 established the initial diagnostics export package boundary.

Implemented:

- Structured diagnostics package schema with app, config, gateway, Desktop, readiness, issue code, and redacted config sections.
- Redacted config JSON for export debugging without API key or gateway key leakage.
- Copy-summary formatter that names false-green readiness layers.
- Desktop config probe evidence/error is included without blocking package export.
- Tauri commands:
  - `export_diagnostics_package`
  - `copy_diagnostics_summary`
- Diagnostics package serialization is covered by secret-leak tests.

Not implemented:

- UI button for copying summary to the system clipboard.
- UI save/export file picker for diagnostics package.
- Open GitHub Issue integration.
- Runtime gateway log capture inside the package.

## Files Changed

- `src-tauri/src/diagnostics.rs`
- `src-tauri/src/state.rs`
- `src-tauri/src/commands.rs`
- `src-tauri/src/lib.rs`
- `docs/testing/eval-harness.md`
- `PLANS.md`
- `project-docs/status.md`
- `project-docs/handoff/2026-05-08-p20-diagnostics-export-package-summary.md`

## Verification

Passed:

```powershell
cargo fmt --all
cargo xtask verify --stage diagnostics
cargo xtask verify --all
```

`cargo xtask verify --all` passed on Windows x64 and included:

- `cargo fmt --all -- --check`
- `cargo test --workspace` with 73 tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Known warning:

- `cargo tauri build` still reports the existing Cargo PDB filename collision warning between the package bin and lib target, but the Windows x64 app/MSI/NSIS bundles build successfully.

## Eval Coverage

Updated diagnostics coverage:

- `diagnostics.false_green_readiness`: summary names provider/Desktop/provider-smoke/gateway-smoke readiness layers instead of reporting full readiness.
- `diagnostics.secret_leak`: diagnostics package export does not leak API keys, gateway keys, Authorization, cookies, URL tokens, or redacted config secrets.

Focused command:

```powershell
cargo xtask verify --stage diagnostics
```

## Blockers

- UI copy/save/report issue flow is still missing.
- Real gateway runtime logs are not yet included in the diagnostics package.
- Real Windows Claude Desktop local config smoke is still pending.
- macOS arm64 and macOS x64 build/smoke remain hard gates for `v1.1.0-rc1`.

## Next Minimum Task

Expand the Leptos UI from spike controls into usable Provider and Diagnostics flows:

1. Add Provider list state, selection, edit/delete/reorder controls, and import/export preview wiring.
2. Add diagnostics copy-summary/export-package buttons wired to the new commands.
3. Keep Apply success strictly tied to readback and gateway success.
4. Preserve pure Rust UI; do not add handwritten JS business logic.
