# P43 Windows Packaged App Smoke Rerun Summary

Date: 2026-05-09

## Goal

Rerun the Windows packaged app smoke against the latest `cargo tauri build` output after P41/P42 changes.

## Verification

Command shape:

```powershell
cargo tauri build
```

Then a PowerShell smoke launched:

```powershell
target\release\cc-desktop-switch.exe
```

with a temporary `CCDS_CONFIG_FILE` under `target\packaged-app-smoke-p42`.

## Result

Pass.

Observed evidence:

- first launch exposed one visible `CC Desktop Switch` main window
- second launch exited
- only one `cc-desktop-switch.exe` process remained after the second launch
- close request hid the `CC Desktop Switch` main window while the process remained alive
- third launch exited and restored the visible `CC Desktop Switch` main window
- test process was stopped after the smoke

## Notes

The smoke did not write Claude Desktop config and did not touch Windows managed policy. It only used the app's temporary config path.

The raw `MainWindowTitle` after close may point at an internal Tauri window, so the rerun used Win32 window enumeration and checked visibility of the user-facing `CC Desktop Switch` window.

## Remaining Blockers

- Windows real Claude Desktop local config smoke is still blocked by managed policy.
- macOS arm64/x64 build/smoke workflow has not been run.
- macOS real Claude Desktop local config smoke has not been run.
