# P80 Gateway API Key Auth Root Cause

## Symptom

The P78 GitHub latest parity review found that Rust mainline wrote a gateway key into the Desktop plan but did not validate gateway credentials on the local gateway endpoints.

## Root Cause

`/v1/models` and `/v1/messages` used the same router state for all callers and did not inspect `Authorization` or `x-api-key` headers. The Tauri runtime started the gateway with Provider/catalog state, but no auth state.

## Fix

- Added optional gateway auth state to the Axum router.
- Tauri runtime now generates and persists a per-config `gateway_api_key` when missing.
- Runtime gateway startup injects the generated key into the router.
- `/v1/models` and `/v1/messages` validate:
  - `Authorization: Bearer <gateway_key>`
  - `x-api-key: <gateway_key>`
- Missing key returns structured `401` with `gateway.auth_missing`.
- Invalid key returns structured `401` with `gateway.auth_invalid`.
- Auth failures are counted in Proxy stats and logged without recording secret values.
- Gateway config fingerprint now includes the gateway key, so key changes restart the runtime gateway.

## Regression Tests

- `router_models_endpoint_requires_gateway_auth_when_configured`
- `router_models_endpoint_accepts_bearer_gateway_auth`
- `router_messages_endpoint_rejects_invalid_gateway_auth`
- `router_messages_endpoint_accepts_x_api_key_gateway_auth`
- `gateway_lifecycle_rejects_missing_or_invalid_gateway_auth`
- `gateway_lifecycle_records_request_stats_and_logs`
- `smoke_checks_cover_static_and_gateway_models`

## Verification

- `cargo test -p cc-desktop-switch --lib gateway_auth -- --nocapture`: pass.
- `cargo test -p cc-desktop-switch --lib gateway -- --nocapture`: pass, 38 passed, 2 ignored.
- `cargo test --workspace`: pass, 117 passed, 2 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo fmt --all -- --check`: pass.
- `trunk build --release`: pass.
- `cargo tauri build`: pass.
- `cargo xtask verify --stage rc-readiness`: pass.

## Residual Risk

The gateway key is visible to Claude Desktop because Claude Desktop must use it to call the local gateway. This does not defend against a same-user process that can read Claude Desktop's local config, but it does block unauthenticated blind local calls to the gateway.
