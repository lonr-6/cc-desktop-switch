# Stage Summary: P1 Rust UI Spike

Date: 2026-05-08

## Stage

P1 Skeleton / pure Rust UI spike.

## Completed

- Added the minimum Cargo workspace with `src-tauri`, `ui`, and `xtask`.
- Added a Tauri v2 app shell that opens a desktop window.
- Added a Leptos + Trunk UI skeleton that preserves the old dashboard/provider/status/mapping shape.
- Wired Leptos to Rust Tauri commands for `save_provider`, `health`, and `apply_dry_run`.
- Added first boundary tests for provider redaction, Claude-safe route output, unmapped route rejection, and dry-run apply not claiming success.
- Kept full Provider, Desktop writer, real gateway, diagnostics package, update flow, and release pipeline migration out of P1 scope.

## Changed Files

- `Cargo.toml`
- `Cargo.lock`
- `.cargo/config.toml`
- `.gitignore`
- `src-tauri/Cargo.toml`
- `src-tauri/build.rs`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/*`
- `ui/Cargo.toml`
- `ui/Trunk.toml`
- `ui/index.html`
- `ui/styles.css`
- `ui/src/*`
- `xtask/Cargo.toml`
- `xtask/src/main.rs`
- `docs/product/rust-ui-spike-exit-criteria.md`
- `docs/testing/release-and-regression-gates.md`
- `project-docs/status.md`

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test --workspace` passed: 4 tests.
- `trunk build --release` passed from `ui/`.
- `cargo tauri build` passed on Windows x64 and produced:
  - `target/release/bundle/msi/CC Desktop Switch_1.1.0_x64_en-US.msi`
  - `target/release/bundle/nsis/CC Desktop Switch_1.1.0_x64-setup.exe`
- Manual window smoke passed: launched `target/release/cc-desktop-switch.exe`, observed window title `CC Desktop Switch`, then closed it.
- `cargo clippy --workspace --all-targets -- -D warnings` passed after installing the `clippy` component.
- `cargo xtask verify --stage ui-spike` runs through the cargo alias and prints the stage gate commands.

## Spike Criteria

See `docs/product/rust-ui-spike-exit-criteria.md` for the pass/partial table.

## Bugs Or Learnings

- `trunk`, `wasm32-unknown-unknown`, `cargo-tauri`, and `clippy` were missing on this machine and were installed as local toolchain prerequisites.
- Tauri/Windows MSI rejects `1.1.0-rc1` as app version metadata. Tauri bundle metadata now uses numeric `1.1.0`; `v1.1.0-rc1` remains the release candidate name in project docs and later release metadata.
- Cargo emits a PDB filename collision warning because the package has both lib and bin targets with equivalent normalized names. It did not block build/test; revisit if release symbols become part of the gate.

## Deferred

- Full Provider CRUD, presets, import/export, config migration.
- Real DesktopApplyFlow platform writer and readback health.
- Real local gateway server and upstream adapters.
- Diagnostics package and redaction module beyond readiness skeleton.
- Update flow and release manifest pipeline.
- macOS arm64/x64 package builds and manual smoke.
- Full accessibility pass for apply/check/report flows.

## Next Step

Expand `model_catalog` with fixtures and tests for Claude-safe routes, `Default`, unmapped route 400, 1M, and Max before implementing config migration.
