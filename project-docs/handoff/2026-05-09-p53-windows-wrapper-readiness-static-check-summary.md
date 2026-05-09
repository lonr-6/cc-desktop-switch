# P53 Windows Wrapper Readiness Static Check Summary

Date: 2026-05-09

## Goal

Make `cargo xtask verify --stage rc-readiness` statically verify the Windows real Desktop smoke wrapper, matching the macOS wrapper coverage added earlier.

## Changes

- Updated `xtask/src/main.rs`:
  - added a Windows wrapper static check for `scripts/windows/run-real-desktop-smoke.ps1`
  - requires `desktop.real_windows_local_config_smoke`
  - requires `windows_real_desktop_local_config_smoke`
  - requires `-AllowRealDesktopWrite`
  - requires `CCDS_ALLOW_REAL_DESKTOP_WRITE`
  - requires `Readiness Markers`
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 10 pass / 3 missing, exits non-zero because real Windows/macOS evidence is still absent |

## Result

Partial.

The Windows wrapper static readiness gate is now covered. The RC goal is still not complete because real smoke evidence is missing.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

Run the real Windows smoke only after the old managed policy blocker is cleared or an unmanaged Windows profile is available:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
```
