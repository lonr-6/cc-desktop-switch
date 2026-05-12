# P74 UI, Installer Identity, Tray Icon, and Auth Scheme Root Cause

Date: 2026-05-12

## Symptoms

- Rust UI still did not visually match the current CC Desktop Switch screenshots and did not absorb the useful compact CC Switch design.
- New installer did not appear able to inherit the previous install directory.
- Saved Provider information could partially degrade after migration.
- Taskbar/tray thumbnail could appear blank or transparent.

## Root Causes

1. UI drift was caused by treating P71-P73 as incremental CSS correction layers instead of making the current desktop screenshots and current frontend code the contract. Mobile viewport work also diluted the desktop target, although this is a desktop app.
2. Install directory inheritance was broken by app identity drift. Rust mainline used `app.ccdesktopswitch.mainline`, while the older Tauri app used `io.github.lonr6.ccdesktopswitch`; the Rust installer also lacked the old per-machine NSIS template/hooks that read the previous uninstall registry `InstallLocation`.
3. Saved Provider migration had a hidden gap: old config parsing handled `authScheme`, but `Provider`, `ProviderSummary`, save commands, and gateway upstream headers did not preserve it end to end.
4. Tray/taskbar icon was under-bound at runtime. The tray was created without the old stable id/tooltip/default icon binding, so Windows shell integration could show a blank/transparent thumbnail even though bundle icon resources existed.

## Fix Strategy

- Rebuild the Leptos UI as a desktop-only contract based on the supplied screenshots:
  - CC Desktop Switch header, five icon tabs, provider cards, add-provider form, proxy page, settings page, and guide page.
  - CC Switch-inspired compact provider card density, empty state, icon navigation, and orange add action.
- Restore stable app identity and installer behavior:
  - Tauri `identifier`: `io.github.lonr6.ccdesktopswitch`.
  - Windows NSIS: `perMachine`, old template, old installer hooks, and install-location lookup.
- Preserve Provider `AuthScheme` through config, commands, UI, and gateway upstream headers.
- Bind tray id, tooltip, and app default icon through `TrayIconBuilder`.
- Add static checks to `cargo xtask verify --stage rc-readiness` so identity/hooks/tray regressions are caught before RC.

## Verification

- `cargo fmt --all -- --check`: pass
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `cargo test --workspace`: pass, 111 passed, 2 ignored real Desktop smoke tests
- `trunk build --release`: pass
- `cargo tauri build`: pass, produced Windows MSI and NSIS bundles
- `cargo xtask verify --stage app-shell`: pass
- `cargo xtask verify --stage rc-readiness`: pass
- Playwright desktop smoke with mocked Tauri bridge: 0 console errors, 1 existing Chromium/Trunk SRI warning

Screenshots:

- `target/ui-smoke/p74/dashboard-desktop-final2.png`
- `target/ui-smoke/p74/providers-desktop-final2.png`
- `target/ui-smoke/p74/provider-add-desktop-final2.png`
- `target/ui-smoke/p74/proxy-desktop-final2.png`
- `target/ui-smoke/p74/settings-desktop-final2.png`
- `target/ui-smoke/p74/guide-desktop-final2.png`

## Remaining Risk

- Real Windows install-location inheritance depends on the old uninstall registry entry containing `InstallLocation`; static checks cannot prove the user's specific machine state.
- The P74 Windows installer has not been manually run over an existing install in this turn, to avoid mutating the user's installed app without explicit smoke steps.
- P74 changed artifacts after the previous macOS workflow evidence; refresh the non-publishing macOS arm64/x64 workflow after pushing P74.

## Regression Requirements

- Keep `installer.old_dir_not_detected`, `installer.identity_mismatch`, `provider.auth_scheme_lost`, `app.tray_default_icon_missing`, and `ui.current_frontend_drift` in the eval/bug registers.
- Do not reintroduce mobile-first UI validation as a release gate for this desktop app.
- Do not change `identifier` again unless a deliberate migration plan is written first.
