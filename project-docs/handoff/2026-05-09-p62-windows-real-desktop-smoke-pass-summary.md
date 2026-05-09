# P62 Windows Real Desktop Smoke Pass Summary

Date: 2026-05-09

## Result

Pass

## Scope

This phase completed the Windows real Claude Desktop local config smoke evidence chain for the Rust/Tauri mainline. It stayed inside `D:\ccds-build\cc-desktop-switch-rust-mainline`, except for the intended current-user Claude Desktop registry policy cleanup and temporary real smoke writes under `%LOCALAPPDATA%\Claude-3p\configLibrary`.

## What Changed

- Fixed `scripts/windows/run-real-desktop-smoke.ps1` so cargo stderr progress output is captured and judged by `$LASTEXITCODE`, not by PowerShell `NativeCommandError`.
- Generated collector evidence at `project-docs/handoff/2026-05-09-windows-real-desktop-smoke-evidence-summary.md`.
- Updated status, plan, eval harness, and readiness audit docs to mark Windows real Desktop smoke as passed while keeping macOS gates open.

## Registry Cleanup Evidence

The first non-elevated cleanup attempt exported:

```text
C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-20260509152525.reg
```

The elevated cleanup exported:

```text
C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-elevated-20260509153014.reg
```

After elevated cleanup:

```text
exists=False
ccdsManaged=False
valueNames=
```

## Smoke Evidence

Collector handoff:

```text
D:\ccds-build\cc-desktop-switch-rust-mainline\project-docs\handoff\2026-05-09-windows-real-desktop-smoke-evidence-summary.md
```

Wrapper evidence:

```text
D:\ccds-build\cc-desktop-switch-rust-mainline\target\real-desktop-smoke\windows-real-desktop-smoke-evidence.md
```

Cargo log:

```text
D:\ccds-build\cc-desktop-switch-rust-mainline\target\real-desktop-smoke\windows-real-desktop-smoke-20260509-153050.log
```

The cargo log includes:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 111 filtered out
```

## Commands Run

| Command | Result |
|---|---|
| `Start-Process powershell -Verb RunAs ... ccds-managed-policy-maintenance.ps1 -Mode cleanup ...` | Passed; elevated exit code 0 |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode status` | Passed; `exists=False` |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode preflight` | Passed; policy absent |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite` | Passed |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke` | Passed; generated collector handoff |
| `cargo xtask verify --all` | Passed; fmt, workspace tests, clippy, UI release build, and Tauri build all passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; Windows pass matched, 2 macOS evidence gates still missing |

## Verification Result

`cargo xtask verify --stage rc-readiness` now reports Windows real Desktop smoke as pass evidence:

```text
[PASS] Windows real Claude Desktop local config smoke passed with backup/readback/gateway/restore evidence
```

The same command still exits non-zero because macOS platform workflow evidence and macOS real Desktop smoke evidence are not recorded yet.

## Rollback

If the old managed policy must be restored, import one of the exported `.reg` backups through:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode restore -BackupPath "C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-elevated-20260509153014.reg" -IUnderstandThisModifiesClaudePolicy
```

Do not restore it while pursuing the RC smoke gate, because the managed policy would block local configLibrary Apply again.

## Next Minimum Task

Run the non-publishing macOS arm64/x64 platform workflow, collect both artifacts with `scripts/macos/Collect-PlatformSmokeEvidence.ps1`, then run the macOS real Claude Desktop local config smoke and collector on macOS.
