# P52 Real Smoke Evidence Marker Alignment Summary

Date: 2026-05-09

## Goal

Make Windows and macOS real Desktop smoke wrapper evidence naturally match the `rc-readiness` handoff keywords after a real pass, while still preventing read-only preflight evidence from being treated as pass evidence.

## Changes

- Updated `scripts/windows/run-real-desktop-smoke.ps1`:
  - added `test_name: windows_real_desktop_local_config_smoke`
  - added `Readiness Markers` for `windows_real_desktop_local_config_smoke`, `loopback gateway`, and `restored`
- Updated `scripts/macos/run-real-desktop-smoke.sh`:
  - added `test_name: macos_real_desktop_local_config_smoke`
  - added `Readiness Markers` for `macOS real Claude Desktop local config smoke`, `configLibrary`, and `safe route`
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode preflight` | Passed; evidence includes Windows readiness markers but result remains `Preflight` |
| `C:\Program Files\Git\bin\bash.exe scripts/macos/run-real-desktop-smoke.sh --mode preflight` | Passed; evidence includes macOS readiness markers but result remains `UnsupportedPlatform` on Windows Git Bash |
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; wrapper markers did not cause preflight evidence to be treated as pass evidence |
| `git diff --check` | Passed; only existing CRLF warnings for tracked baseline files |

## Result

Partial.

Evidence marker alignment is complete. Real Windows/macOS smoke evidence is still missing because the wrappers have not run in real write mode on an eligible profile/platform.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

Run a real smoke wrapper only after the appropriate platform/profile is ready:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
```

```bash
scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write
```
