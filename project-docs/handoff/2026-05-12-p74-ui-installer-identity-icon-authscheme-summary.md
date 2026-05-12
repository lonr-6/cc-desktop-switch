# P74 UI, Installer Identity, Tray Icon, and Auth Scheme Summary

Date: 2026-05-12

## Scope

P74 responds to the latest screenshot review and installation/runtime shell issues.

This phase is not a feature migration. It is a root-cause correction pass for:

- current desktop UI parity,
- old install directory inheritance,
- saved Provider/auth scheme continuity,
- taskbar/tray icon binding.

## Implemented

- Rebuilt the pure Rust Leptos UI around the supplied desktop screenshots:
  - Dashboard/provider switch-board with CC Switch-style empty state.
  - Provider list with compact cards, provider icons, selected border, and right-side icon actions.
  - Add/Edit Provider two-column layout with quick presets, compatibility box, model mapping rows, and orange one-click apply action.
  - Proxy page with status strip, log toolbar, and metrics cards.
  - Settings page with row-based layout matching the current screenshot density.
  - Guide page without duplicate title and with the current step-card structure.
- Removed the accumulated layered CSS override approach and replaced it with a single desktop-first visual contract in `ui/styles.css`.
- Restored Tauri app identity to `io.github.lonr6.ccdesktopswitch`.
- Restored Windows NSIS install identity behavior:
  - `installMode = perMachine`
  - `windows/nsis-installer.nsi`
  - `windows/nsis-hooks.nsh`
- Added Provider `AuthScheme` to provider model, UI command types, config roundtrip, and gateway upstream headers.
- Updated tray creation to use stable id, tooltip, and bundled app default icon.
- Extended `rc-readiness` static checks for installer identity, NSIS hooks, and tray icon binding.

## Why Install Inheritance Is Possible

It is possible when these conditions are true:

1. The new build uses the same app identity as the previous installed Tauri app: `io.github.lonr6.ccdesktopswitch`.
2. The Windows installer uses the old per-machine NSIS template/hooks.
3. The old install wrote a usable uninstall registry entry with `InstallLocation`.
4. Saved app data remains at the shared config path: `~/.cc-desktop-switch/config.json`.
5. Config migration preserves schema details such as `authScheme`.

If a user manually moved the app folder outside installer control, or the old uninstall entry has no `InstallLocation`, the installer cannot infer that path automatically; manual directory selection or a dedicated migration prompt is then required.

## Verification

- `cargo check -p cc-desktop-switch`: pass
- `cargo fmt --all`: pass
- `cargo fmt --all -- --check`: pass
- `cargo clippy --workspace --all-targets -- -D warnings`: pass
- `cargo test --workspace`: pass, 111 passed, 2 ignored real Desktop smoke tests
- `trunk build --release`: pass
- `cargo tauri build`: pass
- `cargo xtask verify --stage app-shell`: pass
- `cargo xtask verify --stage rc-readiness`: pass

Browser smoke:

- Target: `trunk serve --address 127.0.0.1 --port 1425 --open false`
- Bridge: mocked `window.__TAURI__.core.invoke`
- Console: 0 errors, 1 existing Chromium/Trunk SRI warning
- Screenshots:
  - `target/ui-smoke/p74/dashboard-desktop-final2.png`
  - `target/ui-smoke/p74/providers-desktop-final2.png`
  - `target/ui-smoke/p74/provider-add-desktop-final2.png`
  - `target/ui-smoke/p74/proxy-desktop-final2.png`
  - `target/ui-smoke/p74/settings-desktop-final2.png`
  - `target/ui-smoke/p74/guide-desktop-final2.png`

Windows package outputs:

- `target/release/bundle/msi/CC Desktop Switch_1.1.0_x64_en-US.msi`
- `target/release/bundle/nsis/CC Desktop Switch_1.1.0_x64-setup.exe`

## Not Run

- Windows real in-place upgrade installer smoke was not run in this turn because it mutates the user's installed app and install registry.
- macOS arm64/x64 workflow was not rerun after P74 because these changes are local until pushed.
- macOS real physical-machine manual smoke was not run from this Windows machine.

## Remaining Blockers

1. Human Windows upgrade smoke:
   - install over a previous version,
   - confirm previous directory is preselected,
   - confirm `~/.cc-desktop-switch/config.json` and Provider data are preserved,
   - confirm taskbar/tray icon is visible.
2. Push P74 and rerun the non-publishing macOS arm64/x64 workflow.
3. Run final human smoke on Windows and macOS before any release metadata or publishing.

## Next Minimum Task

Prepare the P74 Windows packages for manual testing on the desktop, then run the Windows upgrade smoke checklist. After that, push P74 and trigger the non-publishing macOS workflow for fresh artifacts.
