# API route status

> Last audited: 2026-05-06
> Code baseline: `src-tauri/Cargo.toml` version `2.0.3`
> Primary sources: `src-tauri/src/admin/mod.rs`, `src-tauri/src/admin/handlers.rs`, `frontend/js/api.js`, `crates/proxy/src/server.rs`, `crates/proxy/src/forward.rs`, `crates/adapters/src/*`

This document is the current route contract index for the Rust/Tauri mainline. It exists to avoid the old v2.0.0 launch problem where frontend entries still existed while some backend routes returned fake success.

Status legend:

- `Implemented`: route has current Rust logic behind it.
- `Conditional`: route is implemented, but the result depends on external network, local files, platform support, or configured credentials.
- `Compatibility`: route is kept for old frontend or old local tooling shape, but current product flow has another primary path.
- `Not registered`: old v1.x route is not present in the current Rust router.
- `Stub`: registered route returns fake success or placeholder data instead of doing the action. There are no known `/api/*` stubs in this audit.

## Management API

The management UI runs inside Tauri through the custom `cas://` URI scheme. It no longer binds the v1.x debug HTTP admin port. `frontend/js/api.js` still sends `X-CAS-Request: 1` for compatibility, but the current Rust router does not use that header as its security boundary; the boundary is the in-process Tauri protocol.

| Route | Method | Status | Frontend caller | Notes |
|---|---:|---|---|---|
| `/api/instance-info` | GET | Implemented | No | Returns app id, version, and process id. Kept for legacy single-instance probe shape. |
| `/api/instance-show-window` | POST | Compatibility | No | Returns an acknowledgement. Current window focusing is handled by `tauri-plugin-single-instance` and tray events in `src-tauri/src/main.rs`, not by this route. |
| `/api/status` | GET | Implemented | Yes | Dashboard status, active provider, proxy state, Codex config health, and desktop health. |
| `/api/providers` | GET | Implemented | Yes | Lists public provider data and active provider id. Secrets are stripped. |
| `/api/providers` | POST | Implemented | Yes | Adds provider and sets it active when it is the first provider. |
| `/api/providers/{id}` | PUT | Implemented | Yes | Updates provider fields. Empty `apiKey` and empty `extraHeaders` preserve existing secrets. |
| `/api/providers/{id}` | DELETE | Implemented | Yes | Deletes provider, reindexes order, and moves active provider when needed. |
| `/api/providers/reorder` | PUT | Implemented | Yes | Reorders providers and preserves missing ids by appending existing providers. |
| `/api/providers/{id}/default` | PUT | Implemented | Yes | Sets active provider and synchronizes Codex config; proxy is restarted when needed. |
| `/api/providers/{id}/activate` | POST | Compatibility | Yes | Alias of default-provider switching, matching v1 frontend shape. |
| `/api/providers/{id}/secret` | GET | Implemented | Yes | Returns local provider secret fields only for edit forms. |
| `/api/providers/{id}/draft` | POST | Compatibility | Yes | Reuses provider update logic, matching v1 draft route semantics. |
| `/api/providers/{id}/test` | POST | Conditional | Yes | Performs real minimal provider connectivity check. Network, base URL, and API key determine result. |
| `/api/providers/{id}/usage` | POST | Conditional | Yes | Queries legacy-supported balance/usage endpoints. Unsupported providers return a clear unsupported JSON result, not fake success. |
| `/api/providers/{id}/models` | PUT | Implemented | Yes | Saves model mappings. |
| `/api/providers/{id}/models/available` | GET | Conditional | Yes | Fetches OpenAI-compatible model list from provider candidate endpoints. |
| `/api/providers/{id}/models/autofill` | POST | Conditional | Yes | Fetches provider models, saves suggested mapping, and returns `models` plus `suggested`. |
| `/api/providers/compatibility` | GET | Implemented | Yes | Returns real compatibility matrix from current provider config. |
| `/api/providers/test` | POST | Conditional | Yes | Tests unsaved provider payload before it is persisted. |
| `/api/providers/models/available` | POST | Conditional | Yes | Fetches models for unsaved provider payload. |
| `/api/presets` | GET | Implemented | Yes | Returns built-in presets. |
| `/api/desktop/status` | GET | Implemented | Yes | Reads current Codex config state and active provider model catalog. |
| `/api/desktop/configure` | POST | Conditional | Yes | Applies active provider to `~/.codex/{config.toml,auth.json}` and writes snapshot data. |
| `/api/desktop/clear` | POST | Conditional | Yes | Restores Codex config from snapshot when present. |
| `/api/desktop/restart-codex-app` | POST | Conditional | Yes | Requests a platform best-effort restart of the external Codex App after provider/model sync. |
| `/api/desktop/snapshot-status` | GET | Implemented | Yes | Reports whether this app has a Codex snapshot. |
| `/api/version` | GET | Implemented | Yes | Returns `{ "version": APP_VERSION }`; Settings/About reads this instead of hard-coding a version string. |
| `/api/proxy/start` | POST | Conditional | Yes | Starts local forwarding proxy on configured or requested port. Port binding can fail if unavailable. |
| `/api/proxy/stop` | POST | Implemented | Yes | Stops local forwarding proxy. |
| `/api/proxy/status` | GET | Implemented | Yes | Returns running state, port, and real proxy telemetry counters. |
| `/api/proxy/logs` | GET | Implemented | Yes | Returns in-memory proxy log buffer. |
| `/api/proxy/logs/clear` | POST | Implemented | Yes | Clears in-memory logs and archives disk logs to `logs/backup/`. |
| `/api/proxy/logs/open-dir` | POST | Conditional | Yes | Opens `~/.codex-app-transfer/logs/` with the platform file manager. |
| `/api/settings` | GET | Implemented | Yes | Returns current settings object. |
| `/api/settings` | PUT | Implemented | Yes | Merges and saves settings. |
| `/api/update/check` | GET | Conditional | Yes | Fetches `latest.json`, compares versions, and selects platform asset. Requires configured or passed update URL. |
| `/api/update/install` | POST | Conditional | Yes | Downloads, verifies, and launches platform installer where supported. Linux automatic install remains explicitly unsupported. |
| `/api/config/backup` | POST | Implemented | Yes | Writes a real config backup file. |
| `/api/config/backups` | GET | Implemented | Yes | Lists real backup files. |
| `/api/config/export` | GET | Implemented | Yes | Exports current config envelope. Export contains plaintext secrets. |
| `/api/config/import` | POST | Implemented | Yes | Validates and imports config, creates pre-import backup, and preserves existing local secrets when imported fields are blank. |
| `/api/feedback` | POST | Conditional | Yes | Validates feedback, applies local throttle, attaches diagnostics, and forwards to worker. Requires worker URL and network access. |

## Removed or legacy v1 management routes

| Route | v1.0.3 method | Current status | Frontend caller | Replacement |
|---|---:|---|---|---|
| `/api/providers/{id}/models` | GET | Not registered | No | Current frontend receives mappings from `GET /api/providers`; updates still use `PUT /api/providers/{id}/models`. |

## Proxy surface

The proxy is not part of `/api/*`; it is the local gateway that Codex CLI talks to after the app starts forwarding. Current Rust proxy uses an axum fallback router for HTTP requests and an adapter registry for `openai_chat` and `responses` providers.

| Client route | Method/transport | Current status | Notes |
|---|---|---|---|
| `/responses`, `/v1/responses`, `/openai/v1/responses` | HTTP POST/SSE | Implemented | `ResponsesAdapter` converts Responses request bodies to Chat Completions and converts upstream Chat SSE back to Responses SSE. |
| `/v1/chat/completions`, `/chat/completions` | HTTP POST/SSE | Implemented | `OpenAiChatAdapter` strips a leading `/v1` before forwarding to provider `baseUrl`. |
| Other HTTP paths, including `/v1/models` and `/models` | HTTP any method | Compatibility | Fallback forwards to the active provider after auth and path normalization. Unlike v1.0.3, current Rust proxy does not synthesize a local gateway model-list response here; Codex model metadata is written through `model_catalog_json` during desktop configure. |
| `/responses`, `/v1/responses`, `/openai/v1/responses` | WebSocket | Not registered | v1.0.3 had explicit FastAPI WebSocket handlers. Current Rust mainline exposes HTTP/SSE forwarding only, so current README must not claim WebSocket transport support. |
| `/v1/messages`, `/claude/v1/messages` | HTTP alias | Compatibility | v1.0.3 treated these as Responses aliases. Current fallback can receive the paths, but they are not first-class aliases in the Rust adapter. |
| `/health`, `/status` on the proxy port | HTTP GET | Compatibility | Current fallback forwards these paths upstream. Local proxy status is available through management route `/api/proxy/status`. |

## Stub audit

- Current `/api/*` registered routes: no known stub routes.
- Current `/api/*` routes that return success do so after a real local state change, a real external call, or a documented compatibility acknowledgement.
- Known non-`/api/*` transport gaps are listed in the proxy surface table above. They are not frontend management-route stubs.
