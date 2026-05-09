# P73 Desktop UI Parity Correction Summary

Date: 2026-05-09

## Result

Pass for the current stage.

P73 supersedes P72 for UI direction. The target is desktop CC Desktop Switch UI parity, not mobile UI. Mobile viewport screenshots are no longer an acceptance gate for this desktop app.

## What Changed

- `ui/src/app.rs`
  - Top header uses icon-style actions matching the current frontend: feedback, import, clear, five centered route tabs, settings, theme, add.
  - Dashboard removes the incorrect "选择当前提供商" heading.
  - Dashboard renders provider switch cards and the "继续添加提供商" preset section.
  - Provider and preset cards now use provider assets from `frontend/assets/providers`.
  - Add Provider starts with empty form fields, matching the current frontend reset state.
  - Add Provider restores the two-column form + preset panel structure.
  - Model mapping is shown as row-style mapping controls instead of a large JSON editor in the main flow.

- `ui/styles.css`
  - Adds P73 desktop parity overrides for current frontend tokens, header layout, icon masks, provider cards, preset cards, Add Provider form, and mapping rows.
  - Adds a desktop minimum width so small/mobile layout is not treated as the target experience.

- `ui/index.html`
  - Adds Trunk copy entries for provider logos used by the Rust UI.

- Project docs
  - `project-docs/status.md`, `PLANS.md`, `docs/product/ui-ux-rust-mainline.md`, and `docs/testing/eval-harness.md` now record P73 as desktop-only current frontend parity.

## Verification

- `cargo fmt --all -- --check`
- `trunk build --release`
- Playwright desktop smoke at `1366x900`
  - `target/ui-smoke/p73/dashboard-desktop-r3.png`
  - `target/ui-smoke/p73/provider-add-desktop-r2.png`
  - mocked Tauri bridge console: 0 errors, 1 known Chromium/Trunk SRI warning
- `cargo test --workspace`
  - 110 passed, 2 ignored real Desktop smoke tests
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo tauri build`
  - built Windows app, MSI, and NSIS bundle on the current Windows x64 machine
- `cargo xtask verify --stage rc-readiness`

## Not Run

- Mobile screenshot smoke: intentionally not run; mobile is not an acceptance target for this desktop app.
- Human Windows/macOS manual smoke: deferred until the user is ready to run final manual testing.

## Blockers

- No current code blocker from P73.
- Final release still needs human Windows and macOS app testing before any release decision.

## Next Minimum Task

Push P73 to trigger the non-publishing macOS arm64/x64 workflow for the current UI commit, then review the workflow result. Do not publish, tag, upload GitHub Release assets, or update `latest.json` without explicit user instruction.
