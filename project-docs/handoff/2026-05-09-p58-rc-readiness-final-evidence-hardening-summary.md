# P58 RC Readiness Final Evidence Hardening Summary

Date: 2026-05-09

## Goal

Reduce the risk that `cargo xtask verify --stage rc-readiness` accepts a free-text handoff as final RC evidence for real Windows/macOS smoke.

## Changes

- Updated `xtask/src/main.rs` final handoff checks:
  - Windows real Desktop smoke pass evidence now requires:
    - `fingerprint: desktop.real_windows_local_config_smoke`
    - `test_name: windows_real_desktop_local_config_smoke`
    - `evidence:`
    - `log:`
    - existing `windows_real_desktop_local_config_smoke`, `loopback gateway`, and `restored` markers
  - macOS platform smoke pass evidence now requires:
    - `workflow_run_arm64:`
    - `workflow_run_x64:`
    - `artifact_arm64: rust-mainline-macos-arm64`
    - `artifact_x64: rust-mainline-macos-x64`
    - existing platform fingerprint and runner markers
  - macOS real Desktop smoke pass evidence now requires:
    - `fingerprint: desktop.real_macos_local_config_smoke`
    - `test_name: macos_real_desktop_local_config_smoke`
    - `platform: Darwin`
    - `evidence:`
    - `log:`
    - existing `macOS real Claude Desktop local config smoke`, `configLibrary`, and `safe route` markers
- Updated docs:
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 14 pass / 3 missing, exits non-zero because real Windows/macOS evidence is still absent |

## Result

Partial.

The final evidence matcher is stricter, but the RC goal is still not complete because real Windows/macOS smoke evidence has not been produced yet.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

Get the actual external evidence:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```

Then run the macOS workflow and macOS real Desktop smoke collectors as documented in `project-docs/runbooks/macos-rust-mainline-smoke.md`.
