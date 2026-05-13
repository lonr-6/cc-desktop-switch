# P80 Gateway API Key Auth Summary

## Scope

P80 implements the first P78 P0 gap: local gateway API key authentication.

## Changed Files

- `src-tauri/Cargo.toml`
- `src-tauri/src/gateway.rs`
- `src-tauri/src/state.rs`
- `project-docs/bugs/2026-05-12-p80-gateway-api-key-auth-root-cause.md`
- `project-docs/handoff/2026-05-12-p79-doc-cleanup-summary.md`
- `project-docs/status.md`
- `PLANS.md`
- `docs/testing/python-rust-parity-matrix.md`
- `docs/testing/eval-harness.md`

## Main Changes

- Added router-level optional auth.
- Runtime gateway now requires a generated config-scoped gateway key.
- `/v1/models` and `/v1/messages` reject missing or invalid auth with structured `401` errors.
- Both `Authorization: Bearer <key>` and `x-api-key: <key>` are accepted.
- Gateway smoke sends bearer auth.
- Auth failures enter Proxy stats/logs without leaking supplied or expected keys.
- Gateway restart fingerprint includes the key.

## Verification

- `cargo test -p cc-desktop-switch --lib gateway_auth -- --nocapture`: pass, 5 passed.
- `cargo test -p cc-desktop-switch --lib gateway -- --nocapture`: pass, 38 passed, 2 ignored.
- `cargo test --workspace`: pass, 117 passed, 2 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo fmt --all -- --check`: pass.
- `trunk build --release`: pass.
- `cargo tauri build`: pass.
- `cargo xtask verify --stage rc-readiness`: pass.

## Staged Artifacts

- `C:\Users\15618\Desktop\CCDS-P80-Windows-Gateway-Auth-Manual-Test-20260512`
  - `CC Desktop Switch_1.1.0_x64-setup.exe`
  - `CC Desktop Switch_1.1.0_x64_en-US.msi`
  - `README-manual-test.txt`

## Cleanup

- Deleted old desktop package folders:
  - `CCDS-P75-Manual-Test-20260512`
  - `CCDS-P76-Windows-Manual-Test-20260512`
  - `CCDS-v1.1.0-rc1-manual-test-20260512`
  - `CCDS-P77-Windows-Manual-Test-20260512`
- `CCDS-P74-Manual-Test-20260512` is empty but could not be removed because Windows reports the directory is still in use.
- Condensed and deleted temporary external-review upload artifacts and the browser snapshot; see `project-docs/handoff/2026-05-12-p79-doc-cleanup-summary.md`.

## Next Minimum Task

Continue P78 P0 with runtime update check/download/verify/install commands. Reuse the existing release metadata verification instead of reintroducing weak updater logic.
