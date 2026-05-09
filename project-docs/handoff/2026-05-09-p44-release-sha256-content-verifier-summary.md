# P44 Release Sha256 Content Verifier Summary

Date: 2026-05-09

## Goal

Strengthen the release directory verifier so `.sha256` sidecars must match the actual bytes of `latest.json` and every manifest-referenced release asset.

## Changes

- Updated `src-tauri/src/release_gate.rs`:
  - `validate_release_directory` now reads `latest.json.sha256` and per-asset `.sha256` files.
  - The verifier computes sha256 from actual file bytes and compares it to the sidecar digest.
  - Invalid sidecars fail with `release.latest_json_sha256_invalid` or `release.asset_sha256_invalid`.
  - Hash mismatches fail with `release.latest_json_sha256_mismatch` or `release.asset_sha256_mismatch`.
  - The complete release directory fixture now writes real sha256 sidecars instead of placeholder `"hash"` content.
- Updated release/eval docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`
  - `docs/testing/release-and-regression-gates.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage release` | Passed; 10 release gate tests passed |
| `cargo test --workspace` | Passed; 106 passed, 1 ignored real Desktop smoke |
| `trunk build --release` | Passed |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo tauri build` | Passed; Windows x64 app, MSI, and NSIS bundles produced under `target\release` |

Covered fixtures:

- complete release directory passes with real sha256 sidecars
- `latest.json.sha256` mismatch fails
- asset `.sha256` mismatch fails
- invalid `latest.json.sha256` fails
- invalid asset `.sha256` fails
- previous P39 cases still pass: missing manifest-referenced asset, missing sidecars/public key, invalid `latest.json`, and missing macOS x64 assets

## Notes

This did not publish or upload a release. It does not replace the missing macOS arm64/x64 workflow run or real Desktop smoke evidence.

## Next Minimum Task

Continue with the two real blockers: Windows managed-policy cleanup plus real local config smoke, and macOS arm64/x64 workflow smoke evidence.
