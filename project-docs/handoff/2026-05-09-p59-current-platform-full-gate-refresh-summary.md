# P59 Current Platform Full Gate Refresh Summary

Date: 2026-05-09

## Goal

Refresh the current Windows x64 verification record after the P56-P58 evidence collector and `rc-readiness` hardening changes.

## Verification

| Command | Result |
|---|---|
| `cargo xtask verify --all` | Passed |
| `cargo xtask verify --stage rc-readiness` | Expected incomplete; 14 pass / 3 missing, confirming this local full-gate handoff is not mistaken for real Windows/macOS smoke evidence |

Expanded gates run by `cargo xtask verify --all`:

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

Observed test result:

```text
test result: ok. 110 passed; 0 failed; 2 ignored
```

The two ignored tests are the explicit opt-in real Desktop smoke tests:

- `windows_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores`
- `macos_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores`

Tauri build produced current-platform Windows bundles:

```text
target\release\bundle\msi\CC Desktop Switch_1.1.0_x64_en-US.msi
target\release\bundle\nsis\CC Desktop Switch_1.1.0_x64-setup.exe
```

## Result

Pass for current Windows x64 local gates.

This does not complete RC readiness because the real Windows/macOS evidence gates still require external execution.

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

After explicit approval for the Windows managed-policy cleanup or on an unmanaged Windows profile:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Collect-RealDesktopSmokeEvidence.ps1 -InputDirectory target\real-desktop-smoke
```
