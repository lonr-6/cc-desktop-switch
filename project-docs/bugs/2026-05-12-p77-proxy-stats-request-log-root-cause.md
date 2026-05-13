# P77 Proxy Stats / Request Log Root Cause

## Symptoms

- P76 UI could render Proxy stats/log rows, but the backend `get_proxy_status` still returned fixed zero counters.
- `get_proxy_logs` returned lifecycle logs only, not local gateway request logs.
- Proxy log row keys could collide when multiple logs shared the same millisecond, level, and code.
- The "auto scroll" checkbox was visible but did not control any scroll behavior.

## Root Cause

1. Proxy stats were implemented at the Tauri command boundary as placeholder values instead of being owned by the gateway runtime.
2. Gateway request handlers did not emit request events. `AppState` only recorded lifecycle events such as gateway start/stop and startup failures.
3. Log entries had no stable monotonic id, so the UI had to guess a key from mutable fields.
4. The auto-scroll UI control had no Leptos signal or node ref.

## Fix

- Added a gateway recorder boundary:
  - normal `gateway_router` / `gateway_router_with_provider` keep a no-op recorder;
  - Tauri `AppState::start_gateway` injects a recorder that writes request stats and redacted runtime logs.
- Added in-memory request stats owned by `AppState`:
  - `total`
  - `success`
  - `failed`
  - `today`
- Recorded `/v1/models` and `/v1/messages` requests with status, code, endpoint, method, and safe route id only.
- Kept raw upstream model names out of request logs.
- Added monotonic `id` to `DiagnosticsLogEntry`.
- Changed Leptos log rows to key by backend log id, with a deterministic fallback for mocked older entries.
- Implemented the auto-scroll checkbox with a signal and `NodeRef` so the log body scrolls to bottom when enabled.

## Regression

- `cargo test -p cc-desktop-switch --lib gateway_lifecycle_records_request_stats_and_logs -- --nocapture`: pass.
- `trunk build --release`: pass.
- `cargo test --workspace`: pass, 112 passed, 2 ignored real Desktop smoke tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo fmt --all -- --check`: pass.
- `cargo tauri build`: pass.
- Playwright mocked desktop UI smoke:
  - Proxy stats: `24 / 19 / 5 / 24`.
  - Log rows: `24`.
  - Unique log text rows: `24`.
  - Auto-scroll: `atBottom = true`.
  - Console errors: `0`.

## Remaining

- Stats are process-memory stats, not persisted historical analytics.
- Streaming request success is recorded when the upstream stream response is accepted; downstream stream body errors are not counted separately yet.
- Real installed-app Windows smoke is still required to confirm WebView behavior and installer path inheritance.
