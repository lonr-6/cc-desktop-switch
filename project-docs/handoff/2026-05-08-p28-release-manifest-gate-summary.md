# P28 Release Manifest Gate Summary

## Scope

- Added a local release manifest validation module.
- Added focused `cargo xtask verify --stage release`.
- Did not publish or upload any GitHub Release assets.

## Implemented

- `latest.json` must exist and parse as JSON.
- `latest.json.sha256`, `latest.json.sig`, and update public key must exist.
- Required asset IDs must be present:
  - `windows-setup`
  - `windows-portable-zip`
  - `windows-x64-exe`
  - `macos-arm64-pkg`
  - `macos-arm64-dmg`
  - `macos-x64-pkg`
  - `macos-x64-dmg`
- Every asset must have a file name, `.sha256`, and `.sig`.
- Missing macOS x64 pkg/dmg fails the gate.

## Verification

- `cargo fmt --all`
- `cargo xtask verify --stage release`
- `cargo xtask verify --all` passed: 91 workspace tests, clippy, UI release build, and Windows x64 Tauri build.

## Current Limits

- This is a manifest/content gate, not a real artifact upload or signing command.
- It does not yet compute sha256 from files or verify cryptographic signatures.
- macOS arm64/x64 build and smoke are still pending.

## Next Minimum Task

1. Add local file-backed release artifact verification.
2. Establish macOS arm64/x64 build/smoke path.
3. Keep release publishing blocked until the user explicitly asks.
