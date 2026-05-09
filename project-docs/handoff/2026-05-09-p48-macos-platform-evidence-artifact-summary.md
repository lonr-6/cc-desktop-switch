# P48 macOS Platform Evidence Artifact Summary

Date: 2026-05-09

## Goal

Make the non-publishing macOS platform smoke workflow produce reusable evidence artifacts so a successful arm64/x64 run can be archived without relying on ambiguous green workflow status.

## Changes

- Updated `.github/workflows/rust-mainline-platform-smoke.yml`:
  - each matrix job now writes `platform-smoke-evidence.md`
  - the evidence file records the `platform.macos_arm64_x64_smoke_path` fingerprint, runner label, expected and actual `uname -m`, macOS version, workflow run URL, commit, verified gates, and pkg/dmg paths
  - the evidence file is uploaded with each non-release workflow artifact
- Updated `xtask/src/main.rs`:
  - `cargo xtask verify --stage rc-readiness` now statically checks that the macOS platform smoke workflow emits the evidence artifact
  - this does not mark macOS smoke as passed; real handoff evidence is still required
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/runbooks/macos-rust-mainline-smoke.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Result

Partial.

The workflow path is now better prepared to produce evidence, but it has not been run on `macos-14` or `macos-15-intel` in this workspace. This handoff must not be treated as `platform.macos_arm64_x64_smoke_path` pass evidence.

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo test --workspace` | Passed; 110 passed, 2 ignored real Desktop smoke tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; new evidence-artifact static check passes, real evidence checks still report 3 missing |

## Expected Evidence After Real Workflow Run

A later pass handoff should combine both uploaded `platform-smoke-evidence.md` files and include:

- `## Result`
- `Pass`
- `platform.macos_arm64_x64_smoke_path`
- `macos-14`
- `macos-15-intel`
- workflow run URL
- artifact names `rust-mainline-macos-arm64` and `rust-mainline-macos-x64`

## Remaining Gaps

- The workflow has not been run on GitHub-hosted macOS arm64/x64 runners.
- Windows real Desktop smoke remains blocked by the existing managed policy.
- macOS real Claude Desktop local config smoke still needs unmanaged macOS arm64/x64 profiles.

## Next Minimum Task

Run verification for the workflow/static audit changes, then keep `rc-readiness` failing closed until real platform evidence exists.
