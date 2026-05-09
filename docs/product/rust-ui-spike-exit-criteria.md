# Rust UI Spike Exit Criteria

This document defines when the pure Rust UI choice is proven enough for mainline implementation.

## Definition

Pure Rust UI means:

- UI state, forms, routes, i18n, and interaction logic are written in Rust.
- No React, Vue, Svelte, or hand-written JavaScript business logic.
- Generated WASM glue and a thin bootstrap are allowed.

## Candidate

Default candidate: Tauri v2 + Leptos + Trunk.

## Exit Criteria

The spike passes only if all items below are true:

| Area | Required proof |
|---|---|
| Tauri commands | UI can call Rust commands for provider save, health, and apply dry-run |
| State management | Provider form and model mapping can edit without stale UI state |
| i18n | English, Chinese, and Japanese strings can switch without reload |
| Theme | Light/dark/system theme works |
| Layout | Old layout shape is preserved but smoother |
| Packaging | Windows WebView and macOS bundle can load the UI |
| Dev loop | `trunk serve` or equivalent is usable for local development |
| Bundle | Size and startup time remain acceptable for a desktop utility |
| Accessibility | Keyboard navigation covers primary apply/check/report flows |

## P1 Spike Status - 2026-05-08

| Area | Status | Evidence |
|---|---|---|
| Tauri commands | Pass | Leptos UI calls Rust commands for `save_provider`, `health`, and `apply_dry_run`; command bridge compiled and app window opened. |
| State management | Partial | Provider form fields edit through Leptos signals; complex provider/model mapping state is still deferred to P8 UI. |
| i18n | Pass | English, Chinese, and Japanese strings switch without reload in the skeleton UI. |
| Theme | Partial | Light/dark toggle works in the skeleton; system theme detection is deferred. |
| Layout | Pass | Old dashboard/provider/status/mapping shape is represented without migrating old JS business logic. |
| Packaging | Partial | Windows x64 Tauri build produced MSI/NSIS and the built app opened; macOS bundle smoke remains a release gate. |
| Dev loop | Pass | `trunk build --release` works; `trunk serve` is configured as the Tauri dev command but not manually exercised in this stage. |
| Bundle | Partial | Build succeeds; startup opened within the manual smoke window, but formal startup time and bundle-size budget are not established. |
| Accessibility | Partial | Primary buttons and form controls are keyboard-native; full apply/check/report keyboard path is deferred until the real flows exist. |

P1 conclusion: Leptos + Trunk remains the selected pure Rust UI path. The failed items are scoped follow-ups, not reasons to switch UI stacks.

## Failure Criteria

If Leptos cannot satisfy the exit criteria without heavy custom glue, document the failure and choose a different Rust UI stack before implementing product features.
