# Runbook: Final Human RC Smoke

Date: 2026-05-09

## Purpose

Use this checklist after automated gates pass and before marking `v1.1.0-rc1` ready for release preparation. This is manual validation only. Do not publish, tag, upload a GitHub Release, or update `latest.json` from this runbook.

## Preconditions

- Branch: `codex/rust-mainline-rewrite`.
- Automated gate: `cargo xtask verify --stage rc-readiness` passes for the current P83-or-later artifacts. If it fails because P83-specific macOS evidence is missing, rerun the non-publishing macOS workflow and collectors before this final smoke.
- Windows bundle exists:
  - `target/release/bundle/msi/CC Desktop Switch_1.1.0_x64_en-US.msi`
  - `target/release/bundle/nsis/CC Desktop Switch_1.1.0_x64-setup.exe`
- Latest P83-or-later non-publishing macOS workflow handoff passed and produced:
  - `rust-mainline-macos-arm64`
  - `rust-mainline-macos-x64`
- Tester has Claude Desktop installed and can restart it.
- Tester has either a real provider API key for smoke or uses the no-network/static checks only.

## Windows x64 Manual Smoke

1. Install or launch the Windows x64 build.
2. Confirm the main window opens without a console window.
3. Confirm the desktop navigation works: Dashboard, Provider, Proxy, Settings, Guide.
4. Confirm the UI keeps the old CC Desktop Switch layout feel:
   - white header with app icon and title;
   - centered icon navigation;
   - Provider cards and Add Provider form match the current CC Desktop Switch desktop layout;
   - Proxy, Settings, and Guide pages are reachable and scroll normally.
5. Save a Provider with an API key.
6. Run Health.
7. Run Gateway smoke.
8. Run Apply.
9. Restart Claude Desktop.
10. Confirm Claude Desktop model routes are `claude-*` safe routes.
11. Confirm `Default` is absent from the Claude Desktop model menu.
12. Confirm no raw upstream model name is visible in Claude Desktop.
13. Confirm the local configLibrary entry is used and no old managed policy path is required for the normal Apply path.
14. Confirm Proxy stats/logs update after local gateway requests and log actions scroll correctly.
15. Check update/download/install with a signed local or staging manifest only; do not publish or use public Latest metadata during smoke.
16. Close the CC Desktop Switch window and confirm the app hides to tray.
17. Launch the app again and confirm the existing instance is restored.
18. Export diagnostics package and confirm it is redacted.
19. Open issue draft and confirm it contains no API key, gateway key, Authorization header, cookie, or URL token.

## macOS arm64 Manual Smoke

1. Download the `rust-mainline-macos-arm64` workflow artifact from the latest P83-or-later handoff.
2. Install or launch the app bundle/DMG on an arm64 Mac.
3. Repeat the Windows checklist items 3 through 19 where applicable.
4. Confirm the app uses `~/Library/Application Support/Claude-3p/configLibrary`.
5. Confirm Apply writes and readback restores the expected local gateway route shape.

## macOS x64 Manual Smoke

1. Download the `rust-mainline-macos-x64` workflow artifact from the latest P83-or-later handoff.
2. Install or launch the app bundle/DMG on an Intel Mac.
3. Repeat the Windows checklist items 3 through 19 where applicable.
4. Confirm the app uses `~/Library/Application Support/Claude-3p/configLibrary`.
5. Confirm Apply writes and readback restores the expected local gateway route shape.

## Pass Criteria

- App window opens on Windows x64, macOS arm64, and macOS x64.
- UI keeps the old CC Switch workflow shape: Provider setup, local gateway, one-click Apply, diagnostics/report issue.
- Apply only reports success after write/readback passes.
- Claude Desktop sees only `claude-*` safe routes.
- `Default` is not shown as a runtime model.
- Unmapped routes return an error instead of fallback.
- Diagnostics are redacted.
- Tray/single-instance behavior works on Windows; macOS app shell has no startup or bundle issue.
- Windows in-place install preselects the existing install directory and preserves existing Rust/Python-compatible saved config where supported.
- Taskbar/tray icon is visible, not transparent.

## Fail Handling

- Record the platform, build artifact, exact step, screenshot if relevant, and diagnostics package.
- Do not publish or upload a release after a failed manual smoke.
- Add a bug note under `project-docs/bugs/` for any reproducible issue.
