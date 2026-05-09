# P55 macOS Platform Evidence Collector Summary

Date: 2026-05-09

## Goal

Make the macOS arm64/x64 workflow evidence easy to validate and convert into a handoff after the non-publishing GitHub Actions workflow is run.

## Changes

- Added `scripts/macos/Collect-PlatformSmokeEvidence.ps1`:
  - recursively finds downloaded `platform-smoke-evidence.md` files
  - requires one `platform: macOS arm64` evidence file from `macos-14` with `actual_uname: arm64`
  - requires one `platform: macOS x64` evidence file from `macos-15-intel` with `actual_uname: x86_64`
  - requires `## Result` / `Pass`
  - requires `platform.macos_arm64_x64_smoke_path`
  - requires Rust, UI, Tauri, DMG, and PKG smoke markers
  - writes a combined handoff summary that `rc-readiness` can match
- Updated `cargo xtask verify --stage rc-readiness`:
  - added a static check for the collector script and its required markers
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/runbooks/macos-rust-mainline-smoke.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-PlatformSmokeEvidence.ps1 -InputDirectory target\platform-smoke-evidence-fixture -OutputPath target\platform-smoke-evidence-fixture\combined-macos-platform-smoke-evidence-summary.md` | Passed with generated arm64/x64 fixture evidence |
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 12 pass / 3 missing, exits non-zero because real Windows/macOS evidence is still absent |

## Result

Partial.

The macOS platform evidence collector is ready and self-tested with fixtures. The RC goal is still not complete because real workflow artifacts have not been produced yet.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

Run the non-publishing macOS workflow, download both artifacts, then run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-PlatformSmokeEvidence.ps1 -InputDirectory <downloaded-artifacts-directory>
```
