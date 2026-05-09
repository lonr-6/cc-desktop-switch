# P32 Windows PDB Collision Cleanup Summary

## Scope

P32 removes the repeated Windows release build warning where the Tauri bin target and lib target generated the same `cc_desktop_switch.pdb` filename.

This is a packaging hygiene cleanup. It does not change the product executable name, command bridge, gateway behavior, Provider behavior, or Desktop config behavior.

## Implemented

- Renamed the internal Rust lib crate:
  - from `cc_desktop_switch`
  - to `cc_desktop_switch_lib`
- Updated `src-tauri/src/main.rs` to call `cc_desktop_switch_lib::run()`.
- Kept the package name and binary name unchanged:
  - package: `cc-desktop-switch`
  - exe: `cc-desktop-switch.exe`

## Verification

- `cargo fmt --all`
- `cargo check -p cc-desktop-switch`
- `cargo xtask verify --all`

Latest full gate result on Windows x64:

- `cargo fmt --all -- --check`: pass
- `cargo test --workspace`: pass, 92 tests
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `trunk build --release`: pass
- `cargo tauri build`: pass, produced local MSI and NSIS bundles
- PDB filename collision warning: resolved

## Limits

- This does not establish macOS build or smoke.
- This does not touch signing, `latest.json`, or release upload.

## Next

1. Do Windows Claude Desktop local config smoke only after explicit approval because it touches user app config.
2. Establish macOS arm64/x64 build and smoke path.
3. Add external preset marketplace / template import.
