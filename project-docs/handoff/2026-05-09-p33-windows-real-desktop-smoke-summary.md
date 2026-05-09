# P33 Windows Real Desktop Smoke Summary

Date: 2026-05-09

## Goal

Establish a reproducible Windows real Claude Desktop local config smoke that can prove the local configLibrary path, Desktop readback, safe routes, and loopback gateway work outside the temp fixture tests.

## Changes

- Added an ignored Rust test:
  - `state::tests::windows_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores`
- The test is explicitly opt-in with `CCDS_ALLOW_REAL_DESKTOP_WRITE=1`.
- The test backs up/restores only the affected real Desktop files:
  - `_meta.json`
  - `cc-desktop-switch-local-gateway.json`
- The test checks:
  - current Desktop path probe is Windows
  - managed config is absent before write
  - Apply succeeds only after gateway + Desktop write + readback pass
  - written models are `claude-*` safe routes
  - `Default` is not exposed
  - loopback gateway `/v1/models` smoke passes
  - original Desktop files are restored, or the created empty configLibrary directory is removed
- The Leptos UI result text now explains `desktop.managed_config_detected` in beginner-readable terms instead of only showing the issue code.

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Failed before formatting because the new test needed rustfmt line wrapping |
| `cargo fmt --all` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture` | Passed; guard skipped because `CCDS_ALLOW_REAL_DESKTOP_WRITE` was not set |
| `$env:CCDS_ALLOW_REAL_DESKTOP_WRITE='1'; cargo test -p cc-desktop-switch --lib windows_real_desktop_local_config_smoke -- --ignored --nocapture` | Failed before write with `desktop.managed_config_detected` |
| `reg query HKCU\SOFTWARE\Policies\Claude /s` | Found existing managed policy. Secret values were not copied into docs |
| `reg query HKLM\SOFTWARE\Policies\Claude /s` | Missing |
| Temporary `reg export` / `reg delete` / `reg import` wrapper | Export succeeded; delete failed with `Access is denied`; no real smoke ran under unmanaged state; temp `.reg` backup was removed |
| `Test-Path "$env:LOCALAPPDATA\Claude-3p\configLibrary"` | `False` after the blocked runs |
| `trunk build --release` | Passed after the UI managed-policy note |
| `cargo xtask verify --all` | Passed after all P33 code/docs updates; includes fmt, workspace tests, clippy, UI release build, and current-platform Tauri build |

## Result

The smoke harness is ready, but the Windows real local config smoke is not passed on this machine.

Current blocker:

- `HKCU\SOFTWARE\Policies\Claude` exists.
- The key includes CC Desktop Switch managed-policy markers and gateway settings.
- The real smoke correctly refuses to write local configLibrary while managed policy is detected.
- A temporary cleanup attempt could not delete the key because Windows returned `Access is denied`.
- `%LOCALAPPDATA%\Claude-3p\configLibrary` did not exist after the attempt, so there was no real local config residue to roll back.

## Rollback Notes

- No workspace rollback is required for the failed real smoke attempt.
- No Claude local configLibrary files were left behind.
- The temporary exported `.reg` backup was removed after confirming the original registry key still exists.
- Do not manually delete the policy key without a user-visible migration/backup flow or a separate elevated maintenance step.

## Next Minimum Task

Decide and implement the old managed-policy migration path:

1. Detect `ccds_managed=true` under `HKCU\SOFTWARE\Policies\Claude`.
2. Explain that local configLibrary is blocked by existing managed policy.
3. Offer a safe backup/export and cleanup flow, or document an elevated/manual cleanup runbook.
4. Rerun `windows_real_desktop_local_config_smoke` only after the profile is unmanaged.
