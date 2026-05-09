# P57 macOS Real Desktop Evidence Collector Summary

Date: 2026-05-09

## Goal

Make macOS real Claude Desktop local config smoke pass evidence easy to validate after the opt-in wrapper runs, without treating read-only preflight or off-platform `UnsupportedPlatform` evidence as RC evidence.

## Changes

- Added `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1`:
  - recursively finds `macos-real-desktop-smoke-evidence.md`
  - requires `## Result` / `Pass`
  - requires `fingerprint: desktop.real_macos_local_config_smoke`
  - requires `description: macOS real Claude Desktop local config smoke`
  - requires `test_name: macos_real_desktop_local_config_smoke`
  - requires `platform: Darwin`
  - requires `arch: arm64` or `arch: x86_64`
  - requires `mode: run`
  - requires `exit_code: 0`
  - requires the wrapper readiness markers: `macOS real Claude Desktop local config smoke`, `configLibrary`, and `safe route`
  - resolves and reads the referenced cargo test log
  - requires the cargo test log to include `macos_real_desktop_local_config_smoke` and `test result: ok`
  - writes a combined handoff summary that `rc-readiness` can match
- Updated `scripts/macos/run-real-desktop-smoke.sh`:
  - added top-level `platform:`, `arch:`, and `configLibraryPath:` evidence fields for the collector
- Updated `cargo xtask verify --stage rc-readiness`:
  - added a static check for the macOS real Desktop evidence collector and its required markers
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/runbooks/macos-rust-mainline-smoke.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\macos-real-desktop-smoke-evidence-fixture -OutputPath target\macos-real-desktop-smoke-evidence-fixture\combined-macos-real-desktop-smoke-evidence-summary.md` | Passed with generated fixture evidence and log |
| `C:\Program Files\Git\bin\bash.exe scripts/macos/run-real-desktop-smoke.sh --mode preflight` | Passed on Windows Git Bash by writing `UnsupportedPlatform` evidence without Desktop writes |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke` | Expected failure; rejected `UnsupportedPlatform` evidence because it is missing `## Result` / `Pass` |
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 14 pass / 3 missing, exits non-zero because real Windows/macOS evidence is still absent |

## Result

Partial.

The macOS real Desktop evidence collector is ready and self-tested with fixture evidence. The RC goal is still not complete because real Windows/macOS smoke evidence has not been produced yet.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed because the old managed policy blocker still needs explicit cleanup approval or an unmanaged Windows profile.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

On each unmanaged macOS profile after the wrapper reports `result=Pass`, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```
