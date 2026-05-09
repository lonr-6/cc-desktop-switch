# P72 Current Frontend Parity Summary

## Result

Pass locally.

P71 is superseded for UI direction. It matched older screenshots too loosely and did not use the latest local frontend implementation as the visual source of truth.

P72 re-aligns the pure Rust Leptos UI with the current CC Desktop Switch frontend code and CSS inspected read-only from:

- `D:\cc desktop swtich\frontend\index.html`
- `D:\cc desktop swtich\frontend\css\style.css`
- `D:\cc desktop swtich\frontend\js\app\20-routes.js`
- `D:\cc desktop swtich\frontend\js\app\10-ui-core.js`

`D:\cc` was also checked read-only. It is a TypeScript/Ink CLI-style codebase in this local checkout, not the browser frontend/style baseline used by CC Desktop Switch.

## Main Changes

- Restored the current top shell pattern: left feedback/import/clear actions, centered five icon tabs, right settings/theme/add actions.
- Replaced the P71 dashboard large status-card layout with the current provider switch-board/card layout.
- Split UI routes into Dashboard / Providers / Add Provider / Desktop / Proxy / Settings / Guide to mirror the current frontend route shape.
- Reworked Add Provider into the current two-column form plus preset panel.
- Added current-form elements: API Base URL manage/test action, API key show toggle, auth-scheme trigger, red bordered third-party compatibility details, format choice cards, model mapping area, and orange one-click apply button.
- Hid old advanced import/export panels from the Add Provider default flow so they no longer visually dominate the current add-provider screen.
- Kept the pure Rust UI constraint: no hand-written JS business logic was added.

## Files Changed

- `ui/src/app.rs`
- `ui/styles.css`
- `docs/product/ui-ux-rust-mainline.md`
- `docs/testing/eval-harness.md`
- `PLANS.md`
- `project-docs/status.md`

## Verification

- `cargo fmt --all -- --check`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `trunk build --release`
- `cargo tauri build`
- `cargo xtask verify --stage rc-readiness`
- Browser smoke via `trunk serve --address 127.0.0.1 --port 1422 --open false` and Playwright:
  - desktop `1366x900`
  - mobile `390x900`
  - Dashboard and Add Provider screenshots
  - no console errors
  - one known Chromium/Trunk preload SRI warning
  - no horizontal overflow
  - red compatibility box present
  - one-click apply button color verified as `rgb(255, 104, 31)`

Screenshots and machine-readable evidence:

- `target/ui-smoke/p72/dashboard-desktop-r2.png`
- `target/ui-smoke/p72/provider-add-desktop-r2.png`
- `target/ui-smoke/p72/dashboard-mobile-r2.png`
- `target/ui-smoke/p72/provider-add-mobile-r2.png`
- `target/ui-smoke/p72/playwright-result-r2.json`

## Not Run

- Real user manual Windows/macOS UI smoke was not run in this step. It remains part of the final human RC smoke.
- macOS workflow was not rerun yet for this P72 commit at the time this handoff was written.

## Blockers

- Need push-triggered non-publishing macOS arm64/x64 workflow evidence for the P72 commit.
- Need final human smoke on Windows x64, macOS arm64, and macOS x64 before RC sign-off.

## Next Minimum Task

Commit and push P72, wait for the non-publishing macOS arm64/x64 workflow, then collect artifacts if the run passes.
