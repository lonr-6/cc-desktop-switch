# P61 Windows Managed Policy Cleanup Attempt Summary

Date: 2026-05-09

## Result

Blocked

## Scope

This phase executed the user-authorized local Windows cleanup path for the old Claude Desktop managed policy blocker. It did not publish, push, upload a release, or modify any worktree outside `D:\ccds-build\cc-desktop-switch-rust-mainline`.

## What Happened

- Read-only status confirmed `HKCU\SOFTWARE\Policies\Claude` exists and has `ccdsManaged=True`.
- Cleanup was run through the guarded maintenance script.
- The script exported a backup before attempting deletion:

```text
C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-20260509152525.reg
```

- The deletion step failed:

```text
ERROR: Access is denied.
reg delete failed with exit code 1
```

- Preflight after the failed cleanup still reports the managed policy and no local Claude 3P config library.
- `cargo xtask verify --stage rc-readiness` still fails closed with 14 pass checks and 3 missing evidence checks.

## Commands Run

| Command | Result |
|---|---|
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode status` | Passed; reported `exists=True`, `ccdsManaged=True`, and value names only |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode cleanup -IUnderstandThisModifiesClaudePolicy` | Blocked after backup export; `reg delete` returned `Access is denied` |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode preflight` | Passed as read-only preflight; still reported `ccdsManaged=True` and `configLibraryExists=False` |
| `cargo xtask verify --stage rc-readiness` | Failed as expected; 14 pass / 3 missing |

## Why This Is Not Pass Evidence

The Windows real Desktop smoke pass requires a real run-mode wrapper result, cargo test success, readback, loopback gateway smoke, and restore evidence. This phase only produced cleanup-failure and preflight evidence, so no collector handoff was generated.

## Current Blocker

The current Windows profile cannot delete `HKCU\SOFTWARE\Policies\Claude` from this non-elevated process. Because the runtime probe blocks on the presence of that key, the real local config smoke cannot proceed in this profile until the key is removed or an unmanaged profile is used.

## Next Minimum Task

Run the same cleanup command from an elevated PowerShell window, or use an unmanaged Windows profile. After the key is gone, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
cargo xtask verify --stage rc-readiness
```

Keep the backup `.reg` file until the real smoke passes.
