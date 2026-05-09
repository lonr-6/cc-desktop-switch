# P38 Release Manifest macOS x64 Default Gate Summary

Date: 2026-05-09

## Goal

Make macOS x64 a hard requirement not only in the Rust release gate and workflow call site, but also in the PowerShell release manifest generator defaults.

## Changes

- Updated `scripts/New-ReleaseManifest.ps1`:
  - default `RequiredPlatforms` now includes `macos-x64`
  - required asset names are derived from `RequiredPlatforms`
  - macOS required platforms require both `.pkg` and `.dmg`
- Updated `docs/testing/eval-harness.md`, `PLANS.md`, and `project-docs/status.md`.

## Verification

| Command | Result |
|---|---|
| Missing x64 fixture smoke under `target/release-manifest-smoke/missing-x64` | Passed; script failed as expected when macOS x64 pkg/dmg were absent |
| Complete fixture smoke under `target/release-manifest-smoke/complete` | Passed; script generated `latest.json`, `latest.json.sha256`, `latest.json.sig`, public key, and per-asset `.sha256`/`.sig` for Windows x64, macOS arm64, and macOS x64 |
| `cargo xtask verify --stage release` | Passed |
| Python YAML parse for `.github/workflows/rust-mainline-platform-smoke.yml` and `.github/workflows/release.yml` | Passed |
| `cargo xtask verify --all` | Passed after P38; 95 tests passed, 1 ignored real Desktop smoke, plus clippy, UI release build, and current-platform Tauri build |

## Notes

The smoke used small fixture files and a local signing key under `target/release-manifest-smoke`, which is ignored build output. It did not publish or upload any release.

## Next Minimum Task

Run the macOS platform smoke workflow to produce real macOS arm64/x64 artifacts, then run the manifest gate on those real artifacts before any release upload.
