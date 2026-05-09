# P56 Windows Real Desktop Evidence Collector Summary

Date: 2026-05-09

## Goal

Make Windows real Claude Desktop local config smoke pass evidence easy to validate after the opt-in wrapper runs, without treating read-only preflight evidence as RC evidence.

## Changes

- Added `scripts/windows/Collect-RealDesktopSmokeEvidence.ps1`:
  - recursively finds `windows-real-desktop-smoke-evidence.md`
  - requires `## Result` / `Pass`
  - requires `fingerprint: desktop.real_windows_local_config_smoke`
  - requires `test_name: windows_real_desktop_local_config_smoke`
  - requires `mode: run`
  - requires `exit_code: 0`
  - requires the wrapper readiness markers: `windows_real_desktop_local_config_smoke`, `loopback gateway`, and `restored`
  - resolves and reads the referenced cargo test log
  - requires the cargo test log to include `windows_real_desktop_local_config_smoke` and `test result: ok`
  - writes a combined handoff summary that `rc-readiness` can match
- Updated `scripts/windows/run-real-desktop-smoke.ps1`:
  - added a top-level `configLibraryPath:` evidence field for the collector
- Updated `cargo xtask verify --stage rc-readiness`:
  - added a static check for the Windows evidence collector and its required markers
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/runbooks/windows-managed-policy-cleanup.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\windows-real-desktop-smoke-evidence-fixture -OutputPath target\windows-real-desktop-smoke-evidence-fixture\combined-windows-real-desktop-smoke-evidence-summary.md` | Passed with generated fixture evidence and log |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode preflight` | Passed; read-only evidence still writes successfully and current blocker remains `ccdsManaged=True` |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke` | Expected failure; rejected preflight evidence because it is missing `## Result` / `Pass` |
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 13 pass / 3 missing, exits non-zero because real Windows/macOS evidence is still absent |

## Result

Partial.

The Windows real Desktop evidence collector is ready and self-tested with fixture evidence. The RC goal is still not complete because real Windows/macOS smoke evidence has not been produced yet.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed because the old managed policy blocker still needs explicit cleanup approval or an unmanaged Windows profile.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

After explicit approval for managed-policy cleanup or on an unmanaged profile, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```
