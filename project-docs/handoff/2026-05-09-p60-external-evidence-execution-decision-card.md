# P60 External Evidence Execution Decision Card

Date: 2026-05-09

## Purpose

This card turns the remaining `v1.1.0-rc1` blockers into explicit user decisions.

It is not pass evidence. Do not use this file to satisfy `rc-readiness`; only collector-generated handoffs from real runs should satisfy the final evidence checks.

## Current Blocking Gates

| Gate | Why blocked | Required decision |
|---|---|---|
| Windows real Claude Desktop local config smoke | Current profile still has `HKCU\SOFTWARE\Policies\Claude` with old CC Desktop Switch managed policy values | Approve cleanup in current Windows profile, or use an unmanaged Windows profile |
| macOS arm64/x64 platform workflow smoke | Workflow exists locally but has not been run remotely | Approve pushing/triggering the non-publishing workflow, or run equivalent commands on two macOS machines |
| macOS real Claude Desktop local config smoke | Requires unmanaged macOS profiles and temporary writes to Claude Desktop local `configLibrary` | Approve real smoke on arm64 and x64 macOS profiles |

## Decision 1: Windows Managed Policy Cleanup And Real Smoke

### Risk

Cleanup modifies the current user's Claude Desktop policy key:

```text
HKCU\SOFTWARE\Policies\Claude
```

Impact:

- Claude Desktop may stop using the old managed gateway policy.
- If the key has restrictive ACLs, non-elevated cleanup may fail with Access denied.

### Safer Alternative

Use a fresh unmanaged Windows profile and run the smoke there. This avoids modifying the current profile.

### Rollback

The cleanup runbook exports a `.reg` backup before deletion. Restore with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode restore -BackupPath "C:\path\to\claude-policy.reg" -IUnderstandThisModifiesClaudePolicy
```

### Commands After Approval

Read-only status:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode status
```

Cleanup:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode cleanup -IUnderstandThisModifiesClaudePolicy
```

Real smoke:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```

### Required Artifact

A collector-generated handoff under `project-docs/handoff/` containing the Windows real smoke fingerprint, test name, evidence path, log path, loopback gateway marker, and restored marker.

## Decision 2: macOS arm64/x64 Platform Workflow Smoke

### Risk

Running the GitHub workflow requires the branch/workflow to exist on GitHub and consumes CI minutes. It does not publish a GitHub Release.

Impact:

- Remote CI executes the Rust/Tauri build on `macos-14` and `macos-15-intel`.
- Artifacts are retained as workflow artifacts only.

### Safer Alternative

Run equivalent commands manually on one arm64 Mac and one Intel Mac, then archive the same evidence fields.

### Rollback

No release artifacts are published. If a remote branch/workflow was pushed solely for smoke, delete the remote branch after evidence is collected if desired.

### Commands After Approval

After both workflow artifacts are downloaded:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-PlatformSmokeEvidence.ps1 -InputDirectory <downloaded-artifacts-directory>
```

### Required Artifact

A collector-generated handoff under `project-docs/handoff/` containing:

- `platform.macos_arm64_x64_smoke_path`
- `workflow_run_arm64:`
- `workflow_run_x64:`
- `artifact_arm64: rust-mainline-macos-arm64`
- `artifact_x64: rust-mainline-macos-x64`
- `macos-14`
- `macos-15-intel`

## Decision 3: macOS Real Claude Desktop Local Config Smoke

### Risk

The smoke temporarily writes these files on each macOS test profile:

```text
~/Library/Application Support/Claude-3p/configLibrary/_meta.json
~/Library/Application Support/Claude-3p/configLibrary/cc-desktop-switch-local-gateway.json
```

Impact:

- Claude Desktop local config is temporarily replaced during the test.
- The test backs up and restores the files, but it should run only on an unmanaged test profile.

### Safer Alternative

Create a clean macOS user profile for the smoke instead of using a daily profile.

### Rollback

The Rust ignored test backs up `_meta.json` and `cc-desktop-switch-local-gateway.json`, stops the gateway, restores original files, and verifies restoration. If manual rollback is needed, restore the backup files from the test backup directory captured in logs.

### Commands After Approval

On each macOS architecture:

```bash
scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write
```

Then collect evidence:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\macos\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```

### Required Artifact

A collector-generated handoff under `project-docs/handoff/` containing the macOS real smoke fingerprint, test name, `platform: Darwin`, evidence path, log path, configLibrary marker, and safe route marker.

## Completion Check

After all three decisions have produced collector handoffs:

```powershell
cargo xtask verify --stage rc-readiness
```

Only then should the main goal be considered for completion audit.
