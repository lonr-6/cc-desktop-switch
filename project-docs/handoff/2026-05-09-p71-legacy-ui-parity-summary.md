# P71 Legacy UI Parity Summary

## Result

Pass for local UI parity smoke.

## Goal

Respond to the visual mismatch reported after P70. P70 split the Rust UI into the intended main sections, but it did not yet preserve the old CC Desktop Switch layout feel closely enough. P71 keeps the four-section Rust mainline structure while restoring the legacy dashboard and provider setup visual baseline.

## What Changed

- Reworked the Leptos header to use the real app icon, legacy brand scale, language toggle, and theme button.
- Moved primary navigation below the header and restyled it as legacy pill tabs.
- Rebuilt Dashboard around the old three-card status layout:
  - Claude Desktop status
  - Gateway status
  - Current Provider
- Restored the old large quick-action row:
  - Configure Desktop
  - Start
  - Switch Provider
- Added a legacy-style recent operations panel with health/report actions.
- Reworked Provider into the old two-column layout:
  - left: add/edit Provider form
  - right: quick presets
- Kept advanced Provider import/export, model mapping, and backup tools below the primary form so ordinary flow stays close to the old app.
- Kept all UI behavior in Rust/Leptos and CSS. No handwritten JavaScript business logic was added.

## Boundary Notes

- The old seven-tab UI is not copied one-for-one because `docs/product/ui-ux-rust-mainline.md` keeps the Rust mainline information architecture to four main areas: Dashboard, Provider, Diagnostics, Settings.
- The old Desktop / Proxy / Model Mapping / Guide concepts are still present, but grouped under Provider and Diagnostics rather than exposed as separate top-level tabs.
- This was a UI/UX pass only. It did not change `ModelCatalog`, Desktop writer, gateway routing, Apply semantics, release gates, or update behavior.

## Visual Verification

- Local dev URL: `http://127.0.0.1:1421/?p71=rerender`
- Desktop viewport: `1366x900`
- Mobile viewport: `390x900`
- Screenshots saved under `target/ui-smoke/p71/`:
  - `ccds-ui-p71-dashboard-desktop-v2.png`
  - `ccds-ui-p71-provider-desktop-v2.png`
  - `ccds-ui-p71-provider-mobile.png`
- Console check: 0 errors, 1 existing Chromium/Trunk SRI warning.

## Verification Commands

- `cargo fmt --all -- --check`
- `trunk build --release`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo tauri build`
- `cargo xtask verify --stage rc-readiness`

## Follow-Up

- Push the branch to trigger the non-publishing macOS arm64/x64 platform smoke workflow for the new UI commit.
- After automated gates remain green, run the final human RC smoke checklist on Windows x64, macOS arm64, and macOS x64.
