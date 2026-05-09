# P39 Release Directory Verifier Summary

Date: 2026-05-09

## Goal

Strengthen the release metadata gate so it can reject a staged `latest.json` that references missing asset files, not only malformed in-memory release metadata.

## Changes

- Updated `src-tauri/src/release_gate.rs`:
  - added `validate_release_directory`
  - reads `latest.json` from a staging directory
  - checks `latest.json.sha256`, `latest.json.sig`, public key, manifest-referenced asset files, per-asset `.sha256`, per-asset `.sig`, and required macOS x64 assets
  - added `release.latest_json_asset_missing` for missing files referenced by `latest.json`
- Updated release/eval docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`
  - `docs/testing/release-and-regression-gates.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage release` | Passed; 7 release gate tests passed |
| `cargo test --workspace` | Passed; 99 passed, 1 ignored real Desktop smoke |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |

Covered fixtures:

- complete release directory passes
- `latest.json` referencing a missing asset fails
- missing `latest.json.sig`, public key, asset `.sha256`, and asset `.sig` fail
- invalid `latest.json` fails
- missing macOS x64 assets still fail

## Notes

This did not publish or upload a release. It does not replace the missing macOS arm64/x64 workflow run or real Desktop smoke evidence.

## Next Minimum Task

Run Windows managed-policy cleanup with explicit user approval or an unmanaged profile, then rerun the opt-in Windows real Claude Desktop local config smoke. In parallel, run the macOS arm64/x64 smoke workflow and later feed real artifacts through the release gate.
