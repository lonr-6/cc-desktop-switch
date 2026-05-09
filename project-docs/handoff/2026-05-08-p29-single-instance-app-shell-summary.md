# P29 Single-Instance App Shell Summary

## Scope

P29 establishes the first app shell single-instance boundary for the Rust/Tauri mainline.

This is not a full app shell polish pass. Tray close behavior, background quit rules, startup timing metrics, and manual duplicate-launch smoke remain follow-up work.

## Implemented

- Added official `tauri-plugin-single-instance` integration to the Tauri app.
- Registered the single-instance plugin before other app plugins.
- Added a `single-instance` event payload with launch `args` and `cwd`.
- On duplicate launch, the existing `main` window is shown and focused.
- Added `cargo xtask verify --stage app-shell` as the focused compile-time app shell gate.
- Recorded `app.single_instance` in the local eval harness.

## Verification

- `cargo fmt --all`
- `cargo xtask verify --stage app-shell`
- `cargo xtask verify --all`

Latest full gate result on Windows x64:

- `cargo fmt --all -- --check`: pass
- `cargo test --workspace`: pass, 91 tests
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `trunk build --release`: pass
- `cargo tauri build`: pass, produced local MSI and NSIS bundles

Known warning:

- Cargo reports an output filename collision for `cc_desktop_switch.pdb` between the bin and lib targets. Build still passes. This was resolved later in P32 by renaming the internal lib crate.

## Limits

- No manual duplicate-launch runtime smoke has been run yet.
- No tray close/minimize-to-tray behavior is implemented yet.
- No macOS duplicate-launch smoke has been run yet.

## Next

1. Add tray close behavior with explicit quit semantics.
2. Run a manual duplicate-launch smoke on Windows after the next packaged app build.
3. Establish macOS arm64/x64 app shell smoke before `v1.1.0-rc1`.
