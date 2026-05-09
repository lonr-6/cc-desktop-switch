# P35 Managed Policy Diagnostics Summary

Date: 2026-05-09

## Goal

Make the Windows real-smoke blocker diagnosable without exposing registry secrets.

## Changes

- Windows Desktop config probe now checks whether `HKCU\SOFTWARE\Policies\Claude` / `HKLM\SOFTWARE\Policies\Claude` contains `ccds_managed=true`.
- When that marker exists, probe evidence uses:
  - `desktop.ccds_managed_policy_detected`
- The probe still also emits the generic blocker:
  - `desktop.managed_config_detected`
- The check only queries the marker value and does not read or print gateway keys, API keys, headers, or model values.

## Verification

| Command | Result |
|---|---|
| `cargo xtask verify --stage desktop-config` | Passed; 4 desktop config probe tests now cover the CC Desktop Switch managed-policy issue code |
| `cargo xtask verify --all` | Passed after P35; 95 tests passed, 1 ignored real Desktop smoke, plus clippy, UI release build, and current-platform Tauri build |

## Result

The app can now distinguish a generic managed config from an old CC Desktop Switch managed registry policy. Apply remains blocked before gateway start or Desktop write.

## Next Minimum Task

Implement or document a safe migration/cleanup path:

1. Export the existing policy key.
2. Remove only the CC Desktop Switch-managed key when permission allows.
3. Rerun the real local config smoke.
4. Restore the export if the smoke fails or the user cancels.
