# P36 Windows Managed Policy Cleanup Runbook Summary

Date: 2026-05-09

## Goal

Turn the Windows real local config smoke blocker into a reproducible export-first cleanup path.

## Changes

- Added `scripts/windows/ccds-managed-policy-maintenance.ps1`.
- Added `project-docs/runbooks/windows-managed-policy-cleanup.md`.
- Updated `docs/testing/eval-harness.md`, `PLANS.md`, and `project-docs/status.md`.

## Safety Rules

- `status` mode is read-only.
- `cleanup` refuses to run unless:
  - `-IUnderstandThisModifiesClaudePolicy` is supplied
  - `ccds_managed=true` exists
- `cleanup` exports a `.reg` backup before deletion.
- `restore` requires `-BackupPath` and explicit opt-in.
- Output prints registry value names only, not values. This avoids leaking gateway keys or model policy contents.

## Verification

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode status` | Passed; reported `exists=True`, `ccdsManaged=True`, and value names only |
| `rg -n -e ccds_ -e sk- -e inferenceGatewayApiKey -e Authorization -e Cookie scripts/windows project-docs/runbooks/windows-managed-policy-cleanup.md` | Passed for secret values: only marker names and documented value names were present; no secret values were present |

## Current Blocker

The current Windows profile still has `HKCU\SOFTWARE\Policies\Claude` and earlier direct delete returned `Access is denied`.

This stage does not claim the Windows real smoke has passed. It supplies the safe cleanup path needed to rerun it.

## Next Minimum Task

Run one of these:

- elevated PowerShell cleanup using the runbook, then rerun `windows_real_desktop_local_config_smoke`
- a fresh unmanaged Windows profile, then rerun `windows_real_desktop_local_config_smoke`
