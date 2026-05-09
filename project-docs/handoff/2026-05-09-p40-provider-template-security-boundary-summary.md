# P40 Provider Template Security Boundary Summary

Date: 2026-05-09

## Goal

Make the RC-era Provider template import boundary explicit and testable before adding any network marketplace source.

## Changes

- Updated `src-tauri/src/config.rs`:
  - rejects `ccds.providerTemplate` packages containing secret-bearing keys such as `apiKey`, gateway key, Authorization, Cookie, `headers`, `secret`, or `token`
  - rejects template `baseUrl` values that do not start with `http://` or `https://`
  - keeps templates secretless: imported Providers still start with an empty API key
  - keeps existing safe-route, `Default`, duplicate ID, and `openai_chat` alias rules
- Updated docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`
  - `project-docs/handoff/2026-05-09-p34-provider-template-import-summary.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage provider-import` | Passed; 10 provider import tests passed |
| `cargo test --workspace` | Passed; 100 passed, 1 ignored real Desktop smoke |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |

## Notes

This is still not a network marketplace. Remote preset marketplace support remains deferred until signed source metadata, public-key verification, hash pinning, and UI trust wording are designed.

## Next Minimum Task

Continue with external-environment gates: clear or avoid the old Windows managed policy and rerun the real local config smoke, then run the macOS arm64/x64 platform smoke workflow.
