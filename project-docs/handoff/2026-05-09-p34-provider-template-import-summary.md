# P34 Provider Template Import Summary

Date: 2026-05-09

## Goal

Establish the minimum safe boundary for external Provider presets before adding any network marketplace source.

## Changes

- Added `ccds.providerTemplate` package support to the existing Provider import parser.
- Template packages are secretless:
  - no API key field is imported
  - imported Providers start with an empty API key
  - users add or keep keys through the normal Provider form
- Template import reuses the existing import pipeline:
  - dry-run preview
  - conflict blocking
  - replace existing
  - skip-existing import behavior
- Template import safety rules:
  - `routeId` must be a `claude-*` safe route
  - raw upstream model names are rejected as route IDs
  - `Default` is normalized to non-runtime and never Desktop-visible
  - duplicate template IDs are rejected
  - `apiFormat` accepts both `openai_chat` and serde-native `open_ai_chat`
  - P40 follow-up: secret-bearing fields such as `apiKey`, `headers`, Authorization, Cookie, secret, or token are rejected instead of silently ignored
  - P40 follow-up: template `baseUrl` must start with `http://` or `https://`
- Added a Leptos `Template example` button that fills the import textarea with a sample secretless template JSON.

## Verification

| Command | Result |
|---|---|
| `cargo test -p cc-desktop-switch --lib provider_template -- --nocapture` | Passed after adding `openai_chat` alias support |
| `cargo xtask verify --stage provider-import` | Passed; stage now catches 9 provider import tests including template import |
| `trunk build --release` | Passed after adding the Leptos template example button |
| `cargo xtask verify --all` | Passed; 94 tests passed, 1 ignored real Desktop smoke, plus clippy, UI release build, and current-platform Tauri build |

## Notes

This is not a network marketplace. It deliberately avoids remote fetch, signatures, trust policy, or auto-update semantics. Those belong in the next marketplace stage after platform smoke gates are clearer.

P40 keeps that boundary explicit: pasted templates remain local and secretless, and remote marketplace support remains deferred until signed source verification is designed and implemented.

## Next Minimum Task

Design signed/verified marketplace source metadata:

1. Source URL allowlist or user-added source list.
2. Template package signature/public key verification.
3. Hash pinning and diagnostics for stale or tampered templates.
4. UI wording that distinguishes built-in presets, pasted templates, and remote marketplace sources.
