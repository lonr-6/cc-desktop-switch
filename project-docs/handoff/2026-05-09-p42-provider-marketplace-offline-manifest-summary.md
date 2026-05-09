# P42 Provider Marketplace Offline Manifest Summary

Date: 2026-05-09

## Goal

Add a minimum safe boundary for Provider marketplace metadata without enabling network fetch or trusting remote templates blindly.

## Changes

- Updated `src-tauri/src/config.rs`:
  - added `ccds.providerMarketplace` import source
  - marketplace source URL must start with `https://`
  - source URL must not contain query, fragment, or userinfo
  - embedded `ccds.providerTemplate` package must match `templateSha256`
  - after hash verification, import reuses P40 template checks for secret-bearing fields, safe routes, `Default`, duplicate IDs, and HTTP(S) provider base URL
- Updated `src-tauri/Cargo.toml`:
  - added direct `sha2` dependency for deterministic template package hash validation
- Updated docs:
  - `project-docs/status.md`
  - `PLANS.md`
  - `docs/testing/eval-harness.md`

## Verification

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo xtask verify --stage provider-import` | Passed; 13 provider import tests passed |
| `cargo test --workspace` | Passed; 103 passed, 1 ignored real Desktop smoke |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed |
| `cargo tauri build` | Passed; produced Windows MSI and NSIS bundles |

## Notes

This is still not a signed network marketplace. It only defines the first offline manifest gate. A real remote marketplace still needs signature trust policy, public key handling, fetch/cache strategy, and UI wording before it can be enabled.

## Next Minimum Task

External gates remain blocking for RC completion: Windows managed policy cleanup plus real local config smoke, macOS arm64/x64 workflow run, and macOS real Claude Desktop smoke.
