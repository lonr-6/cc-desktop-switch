# P49 Current Platform Full Gate Refresh Summary

Date: 2026-05-09

## Goal

Refresh the current Windows x64 local verification evidence after P47/P48 and re-check the Windows real Desktop smoke blocker in read-only mode.

## Result

Partial.

The current-platform automated gate passed, including UI and Tauri packaging. RC1 is still not complete because the required real Windows/macOS smoke evidence remains missing.

## Verification

| Command | Result |
|---|---|
| `cargo xtask verify --all` | Passed |
| `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\ccds-managed-policy-maintenance.ps1 -Mode status` | Passed as read-only status check; `exists=True`, `ccdsManaged=True` |
| `Test-Path $env:LOCALAPPDATA\Claude-3p\configLibrary` | `False` |

`cargo xtask verify --all` covered:

- `cargo fmt --all -- --check`
- `cargo test --workspace`: 110 passed, 2 ignored real Desktop smoke tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`

The Tauri build produced:

- `target\release\bundle\msi\CC Desktop Switch_1.1.0_x64_en-US.msi`
- `target\release\bundle\nsis\CC Desktop Switch_1.1.0_x64-setup.exe`

## Blocker Evidence

The Windows managed policy still exists:

```text
exists=True
ccdsManaged=True
valueNames=ccds_managed,inferenceGatewayApiKey,inferenceGatewayAuthScheme,inferenceGatewayBaseUrl,inferenceGatewayHeaders,inferenceModels,inferenceProvider,isClaudeCodeForDesktopEnabled
```

The local configLibrary path still does not exist:

```text
configLibraryExists=False
configLibraryPath=C:\Users\15618\AppData\Local\Claude-3p\configLibrary
```

## Remaining Gaps

- Windows real Claude Desktop local config smoke has not passed.
- macOS arm64/x64 workflow smoke has not run.
- macOS real Claude Desktop local config smoke has not run on macOS.

## Next Minimum Task

Get explicit approval for the P36 Windows managed-policy cleanup, or run the real Windows smoke on an unmanaged profile. In parallel, run the non-publishing macOS platform workflow and archive both `platform-smoke-evidence.md` files.
