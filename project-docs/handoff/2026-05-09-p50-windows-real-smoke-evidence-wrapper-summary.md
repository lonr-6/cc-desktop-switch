# P50 Windows Real Smoke Evidence Wrapper Summary

Date: 2026-05-09

## Goal

Add a safe Windows wrapper for real Claude Desktop local config smoke evidence capture. The wrapper must default to read-only preflight and require an explicit opt-in before any real Desktop write test can run.

## Changes

- Added `scripts/windows/run-real-desktop-smoke.ps1`:
  - `-Mode preflight` is the default and is read-only
  - preflight records managed policy status with value names only
  - preflight records whether `%LOCALAPPDATA%\Claude-3p\configLibrary` exists
  - evidence is written to `target\real-desktop-smoke\windows-real-desktop-smoke-evidence.md`
  - `-Mode run` refuses to execute unless `-AllowRealDesktopWrite` is passed
  - run mode sets `CCDS_ALLOW_REAL_DESKTOP_WRITE=1` only for the ignored Rust test process
- Updated docs:
  - `project-docs/runbooks/windows-managed-policy-cleanup.md`
  - `docs/testing/eval-harness.md`
  - `project-docs/status.md`
  - `PLANS.md`
  - `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`

## Verification

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode preflight` | Passed; wrote preflight evidence under `target\real-desktop-smoke\` |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run` | Expected failure; refused without `-AllowRealDesktopWrite` |
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; P50 preflight evidence was not misread as real smoke pass evidence |
| `git diff --check` | Passed; only existing CRLF warnings for tracked baseline files |

Preflight output:

```text
result=Preflight
policyStatus=exists=True;ccdsManaged=True;valueNames=ccds_managed,inferenceGatewayApiKey,inferenceGatewayAuthScheme,inferenceGatewayBaseUrl,inferenceGatewayHeaders,inferenceModels,inferenceProvider,isClaudeCodeForDesktopEnabled
configLibraryExists=False
```

## Result

Partial.

The evidence wrapper is ready, but Windows real Claude Desktop local config smoke still has not passed. `Preflight` must not be treated as pass evidence.

## Remaining Gaps

- Windows managed policy still blocks real local config smoke.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run.

## Next Minimum Task

After explicit approval for managed-policy cleanup or on an unmanaged profile, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
```

Then archive the pass evidence in a handoff with `## Result` / `Pass`, `windows_real_desktop_local_config_smoke`, `loopback gateway`, and `restored`.
