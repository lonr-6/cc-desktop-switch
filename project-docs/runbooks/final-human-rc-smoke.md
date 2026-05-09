# Runbook: Final Human RC Smoke

Date: 2026-05-09

## Purpose

Use this checklist after automated gates pass and before marking `v1.1.0-rc1` ready for release preparation. This is manual validation only. Do not publish, tag, upload a GitHub Release, or update `latest.json` from this runbook.

## Preconditions

- Branch: `codex/rust-mainline-rewrite`.
- Automated gate: `cargo xtask verify --stage rc-readiness` passes.
- Windows bundle exists:
  - `target/release/bundle/msi/CC Desktop Switch_1.1.0_x64_en-US.msi`
  - `target/release/bundle/nsis/CC Desktop Switch_1.1.0_x64-setup.exe`
- macOS workflow run `25599626985` passed and produced:
  - `rust-mainline-macos-arm64`
  - `rust-mainline-macos-x64`
- Tester has Claude Desktop installed and can restart it.
- Tester has either a real provider API key for smoke or uses the no-network/static checks only.

## Windows x64 Manual Smoke

1. Install or launch the Windows x64 build.
2. Confirm the main window opens without a console window.
3. Confirm the four tabs work: Dashboard, Provider, Diagnostics, Settings.
4. Confirm the UI keeps the old CC Desktop Switch layout feel:
   - white header with app icon and title;
   - pill navigation below the header;
   - Dashboard has three large status cards and three large action buttons;
   - Provider has an add/edit form on the left and quick presets on the right.
5. Save a Provider with an API key.
6. Run Health.
7. Run Gateway smoke.
8. Run Apply.
9. Restart Claude Desktop.
10. Confirm Claude Desktop model routes are `claude-*` safe routes.
11. Confirm `Default` is absent from the Claude Desktop model menu.
12. Confirm no raw upstream model name is visible in Claude Desktop.
13. Close the CC Desktop Switch window and confirm the app hides to tray.
14. Launch the app again and confirm the existing instance is restored.
15. Export diagnostics package and confirm it is redacted.
16. Open issue draft and confirm it contains no API key, gateway key, Authorization header, cookie, or URL token.

## macOS arm64 Manual Smoke

1. Download the `rust-mainline-macos-arm64` workflow artifact from run `25599626985`.
2. Install or launch the app bundle/DMG on an arm64 Mac.
3. Repeat the Windows checklist items 3 through 16.
4. Confirm the app uses `~/Library/Application Support/Claude-3p/configLibrary`.
5. Confirm Apply writes and readback restores the expected local gateway route shape.

## macOS x64 Manual Smoke

1. Download the `rust-mainline-macos-x64` workflow artifact from run `25599626985`.
2. Install or launch the app bundle/DMG on an Intel Mac.
3. Repeat the Windows checklist items 3 through 16.
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

## Fail Handling

- Record the platform, build artifact, exact step, screenshot if relevant, and diagnostics package.
- Do not publish or upload a release after a failed manual smoke.
- Add a bug note under `project-docs/bugs/` for any reproducible issue.
