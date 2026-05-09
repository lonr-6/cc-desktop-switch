# Runbook: Windows Managed Policy Cleanup

Date: 2026-05-09

## Purpose

Use this only when Windows real Claude Desktop local config smoke is blocked by:

- `desktop.managed_config_detected`
- `desktop.ccds_managed_policy_detected`

This usually means an older CC Desktop Switch flow wrote Claude Desktop settings under:

```text
HKCU\SOFTWARE\Policies\Claude
```

Local configLibrary smoke cannot pass while that managed policy exists, because Apply must stop before writing local user config.

## Safety Boundary

- Default action is read-only status.
- Cleanup refuses to run unless `ccds_managed=true` is present.
- Cleanup always exports a `.reg` backup before deletion.
- The script prints value names only, not gateway keys, API keys, headers, or model values.
- Do not run cleanup while Claude Desktop is applying policy changes.

## Script

```powershell
scripts\windows\ccds-managed-policy-maintenance.ps1
```

## Real Smoke Evidence Wrapper

Default mode is read-only and writes a reusable evidence file under `target\real-desktop-smoke\`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode preflight
```

This captures:

- managed policy status with value names only
- `%LOCALAPPDATA%\Claude-3p\configLibrary` existence
- the evidence path to archive after a real run

The wrapper refuses to run the write smoke unless `-AllowRealDesktopWrite` is explicitly passed:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
```

Do not treat `Preflight` evidence as pass evidence. A real pass still requires the Rust ignored test to report backup, readback, loopback gateway, and restore success.

## Status Check

Read-only:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode status
```

Expected blocked example:

```text
exists=True
ccdsManaged=True
valueNames=ccds_managed,inferenceGatewayApiKey,inferenceGatewayAuthScheme,inferenceGatewayBaseUrl,inferenceGatewayHeaders,inferenceModels,inferenceProvider,isClaudeCodeForDesktopEnabled
```

## Export Backup Only

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode export
```

The default backup directory is:

```text
%LOCALAPPDATA%\CC Desktop Switch\policy-backups
```

Keep the generated `.reg` file until Windows real local config smoke passes.

## Cleanup

This modifies the current user's Claude Desktop policy key.

Risk:

- Claude Desktop may stop using the old managed gateway policy.
- If the key has restrictive ACLs, non-elevated cleanup may fail with Access denied.

Command:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode cleanup -IUnderstandThisModifiesClaudePolicy
```

If it fails with Access denied, rerun the same command from an elevated PowerShell window. If it still fails, do not force ownership changes without a separate approval and backup.

## Real Smoke After Cleanup

Preferred wrapper:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
```

Direct test command:

```powershell
$env:CCDS_ALLOW_REAL_DESKTOP_WRITE='1'
cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture
Remove-Item Env:CCDS_ALLOW_REAL_DESKTOP_WRITE -ErrorAction SilentlyContinue
```

Pass criteria:

- test passes
- `%LOCALAPPDATA%\Claude-3p\configLibrary` is restored to its pre-test state
- gateway smoke passes inside the test

## Collect Real Smoke Evidence

After `run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite` reports `result=Pass`, validate the wrapper evidence and cargo test log before writing a handoff:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```

The collector rejects preflight evidence, failed evidence, missing logs, and logs without `test result: ok`. Its generated handoff contains the `windows_real_desktop_local_config_smoke`, `loopback gateway`, and `restored` markers used by `cargo xtask verify --stage rc-readiness`.

## Restore

Use the backup path printed by export or cleanup:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode restore -BackupPath "C:\path\to\claude-policy.reg" -IUnderstandThisModifiesClaudePolicy
```

Restore if:

- cleanup was accidental
- real smoke fails and old behavior must be put back
- user chooses to keep managed policy instead of local configLibrary

## Documentation Update

After a real smoke pass or blocker:

1. Update `project-docs/status.md`.
2. Add a handoff summary under `project-docs/handoff/`.
3. Update `docs/testing/eval-harness.md` if the expected smoke procedure changes.
