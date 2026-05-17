# Gemini CLI OAuth + Cloud Code Assist — Wire Research

Ground truth = upstream `google-gemini/gemini-cli` (Apache-2.0). Cross-checked against `router-for-me/CLIProxyAPI` (MIT). When they diverge, gemini-cli wins; divergence flagged inline.

All `oauth2.ts:NN` cites `packages/core/src/code_assist/oauth2.ts`. `setup.ts:NN` cites `packages/core/src/code_assist/setup.ts`. `server.ts:NN` cites `packages/core/src/code_assist/server.ts`. `converter.ts:NN` cites `packages/core/src/code_assist/converter.ts`.

---

## 1. OAuth flow (10 fields, byte-exact)

| field | value |
|---|---|
| client_id | `681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com` (oauth2.ts:43-44) |
| client_secret | `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` (oauth2.ts:47-51) — same constant in CLIProxyAPI `internal/auth/gemini/gemini_auth.go:34-36` |
| auth endpoint URL | `https://accounts.google.com/o/oauth2/v2/auth` (Google default; gemini-cli uses `google-auth-library`'s `client.generateAuthUrl()` which targets this) |
| token endpoint URL | `https://oauth2.googleapis.com/token` (Google default via `google-auth-library`; CLIProxyAPI confirms at `gemini_auth.go:230`) |
| redirect_uri pattern | Web flow: `http://127.0.0.1:${port}/oauth2callback` where port is from `getAvailablePort()` / env `OAUTH_CALLBACK_PORT` / OS-assigned (oauth2.ts:339, env override `OAUTH_CALLBACK_HOST` defaults `127.0.0.1`). User-code flow: `https://codeassist.google.com/authcode` (oauth2.ts:318). **Divergence**: CLIProxyAPI hardcodes port `8085` and host `localhost` (`gemini_auth.go:36,67`) — use 127.0.0.1 + dynamic port to match upstream. |
| scopes (space-separated) | `https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile` (oauth2.ts:54-58) |
| access_type | `offline` (oauth2.ts:347 web flow, oauth2.ts:362 user-code flow) |
| prompt param | not set in either flow |
| PKCE used by gemini-cli? | **Web flow: NO** (oauth2.ts:207-213, only `redirect_uri/access_type/scope/state`). **User-code flow: YES**, `code_challenge_method: S256`, `code_challenge` from `client.generateCodeVerifierAsync()` (oauth2.ts:320-321, 362-365), and `codeVerifier` passed in `client.getToken({code, codeVerifier})` at oauth2.ts:329. We only need web flow → skip PKCE. |
| token response shape | Standard Google `Credentials` (from `google-auth-library`). Persisted JSON (oauth2.ts:660-720): `{ "access_token": "ya29...", "refresh_token": "1//...", "scope": "<3 scopes joined>", "token_type": "Bearer", "id_token": "<jwt>", "expiry_date": 1730000000000 }`. `expiry_date` is **ms epoch**, not `expires_in` seconds. |

**State param**: `crypto.randomBytes(32).toString('hex')` (oauth2.ts:200ish) — required CSRF check on callback.

---

## 2. Token refresh

- **Trigger**: handled by `google-auth-library` automatically. Library refreshes when current `access_token` is past `expiry_date` minus its internal eager-refresh window (5 min default in google-auth-library). Refreshed creds emitted via `client.on('tokens', ...)` event (oauth2.ts:133-140), which calls `cacheCredentials(tokens)` to rewrite the file. Manual validation also runs on cached load (oauth2.ts:174-177).
- **POST body** to `https://oauth2.googleapis.com/token` (form-encoded):
  ```
  client_id=681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com
  client_secret=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
  grant_type=refresh_token
  refresh_token=<stored>
  ```
- **Response**: `{ "access_token": "...", "expires_in": 3599, "scope": "...", "token_type": "Bearer", "id_token": "..." }` — Google does **not** rotate `refresh_token` for installed-app clients, so reuse the stored one. Convert `expires_in` → `expiry_date = Date.now() + expires_in*1000` to match the cached file shape.
- **File written**: `Storage.getOAuthCredsPath()` = `~/.gemini/oauth_creds.json`, mode `0o600`, 2-space JSON (oauth2.ts:660-720). For us: write `~/.codex-app-transfer/gemini-oauth.json` to avoid colliding with a real gemini-cli install.

---

## 3. loadCodeAssist + onboardUser bootstrap (setup.ts)

All HTTP via `CodeAssistServer` (server.ts) which prefixes `https://cloudcode-pa.googleapis.com/v1internal:`. Method = POST, body = JSON.

### 3.1 `:loadCodeAssist`

- **URL**: `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist`
- **Body** (setup.ts:127-135):
  ```json
  {
    "cloudaicompanionProject": "<env GOOGLE_CLOUD_PROJECT or undefined>",
    "metadata": {
      "ideType": "...", "platform": "...", "pluginType": "GEMINI", "pluginVersion": "...",
      "duetProject": "<projectId or undefined>"
    }
  }
  ```
  `coreClientMetadata` is a `ClientMetadata` (types.ts) — IDE, platform, plugin info. `pluginType: GEMINI` is the canonical string for gemini-cli.
- **Response** (types.ts `LoadCodeAssistResponse`): `{ currentTier?: GeminiUserTier, allowedTiers?: GeminiUserTier[], cloudaicompanionProject?: string, ineligibleTiers?: IneligibleTier[] }`. Each `GeminiUserTier` carries `id` (e.g. `free-tier`, `legacy-tier`, `standard-tier`), `name`, `isDefault`, `userDefinedCloudaicompanionProject` (bool — does this tier need user project?), `hasAcceptedTos`, `hasOnboardedPreviously`.

### 3.2 Decision tree (when to call `:onboardUser`)

setup.ts:157-180:
1. `tier = getOnboardTier(loadRes)` → first tier with `isDefault: true`, else `LEGACY` fallback.
2. If `tier.id === FREE` → onboard with `cloudaicompanionProject: undefined` (Google auto-creates project).
3. Else (paid/standard) → onboard with `cloudaicompanionProject: <env or returned project>`, also pass `metadata.duetProject`.
4. Always call `onboardUser` (no skip — even if `hasOnboardedPreviously: true`, the call returns immediately with the existing project).

### 3.3 `:onboardUser` (long-running operation)

- **URL**: `POST https://cloudcode-pa.googleapis.com/v1internal:onboardUser`
- **Body**:
  ```json
  { "tierId": "free-tier|legacy-tier|standard-tier",
    "cloudaicompanionProject": "<string or omitted>",
    "metadata": { ...same coreClientMetadata, "duetProject": "<projectId>" } }
  ```
- **LRO polling** (setup.ts:183-190):
  ```
  lroRes = POST :onboardUser
  while (!lroRes.done && lroRes.name) {
    sleep 5000ms
    lroRes = POST :v1internal:<operationName>  // gemini-cli calls caServer.getOperation(name)
  }
  ```
  CLIProxyAPI confirms 5s interval. Completion signal: `lroRes.done === true`.
- **Final project_id** (setup.ts:195-200): `lroRes.response.cloudaicompanionProject.id`. CLIProxyAPI same path (`internal/cmd/login.go`).

---

## 4. `:streamGenerateContent` OAuth-mode wire

### URL
`POST https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse` (server.ts:CODE_ASSIST_ENDPOINT + `:method` + `params: { alt: 'sse' }`).

### Headers
gemini-cli sets only:
- `Content-Type: application/json` (server.ts requestStreamingPost)
- `Authorization: Bearer <access_token>` — **not set explicitly**; injected by `google-auth-library`'s `AuthClient.request()`.

`User-Agent` and `X-Goog-Api-Client` are **not set** in `code_assist/server.ts`. They are set globally by gemini-cli's HTTP layer / google-auth-library defaults. CLIProxyAPI sets them explicitly for impersonation:
- `User-Agent`: `GeminiCLI/<cliVersion>/<model> (<os>; <arch>; terminal)` — e.g. `GeminiCLI/0.34.0/gemini-2.5-pro (darwin; arm64; terminal)` (`internal/misc/header_utils.go`).
- `X-Goog-Api-Client`: `google-genai-sdk/1.41.0 gl-node/v22.19.0` (constant, `header_utils.go`).
- `Accept: text/event-stream` (CLIProxyAPI `gemini_cli_executor.go`).

**Recommendation**: set all three explicitly in our Rust client to match CLIProxyAPI's impersonation (safer than relying on Google to accept missing UA from a non-Node client). Use `GeminiCLI/0.34.0/<model> (<os>; <arch>; terminal)` and the literal `google-genai-sdk/1.41.0 gl-node/v22.19.0`.

### Outer request body envelope
converter.ts:106-119 (`toGenerateContentRequest`):
```json
{
  "model": "gemini-2.5-pro",
  "project": "<projectId>",
  "user_prompt_id": "<uuid v4 per turn>",
  "request": {
    "contents": [...],
    "systemInstruction": { "role": "system", "parts": [...] },
    "cachedContent": "...",
    "tools": [...],
    "toolConfig": {...},
    "labels": {...},
    "safetySettings": [...],
    "generationConfig": { "temperature": ..., "thinkingConfig": {...}, ... },
    "session_id": "<uuid v4 per session>"
  },
  "enabled_credit_types": ["..."]
}
```
**Key**: `model` and `project` are **outer**; everything our existing GeminiNativeAdapter produces (contents/tools/systemInstruction/generationConfig) goes inside `request: { ... }`. `user_prompt_id` is **outer** (string, snake_case). `session_id` is **inner** (inside `request`, snake_case). `enabled_credit_types` is outer, optional — omit unless we have a use.

### SSE response shape
server.ts streams `data: <json>` lines, parses each chunk with `JSON.parse`. Type is `CaGenerateContentResponse` — a wrapper distinct from public Gemini API. Each event has the form:
```json
{ "response": { "candidates": [...], "usageMetadata": {...}, "modelVersion": "...", "promptFeedback": {...} } }
```
Inner `response` field matches the public `generativelanguage.googleapis.com` `:streamGenerateContent` shape. **Adapter must unwrap `event.response`** before handing to existing Gemini-native parsing. CLIProxyAPI's `TranslateStream` does this; we should too.

### Differences vs public API-key path
| public `generativelanguage.googleapis.com` | Cloud Code `cloudcode-pa.googleapis.com` |
|---|---|
| `/v1beta/models/<model>:streamGenerateContent?key=API_KEY&alt=sse` | `/v1internal:streamGenerateContent?alt=sse` |
| body = `{contents, tools, systemInstruction, generationConfig, ...}` directly | body = `{model, project, user_prompt_id, request:{...same fields..., session_id}}` |
| auth via `?key=` query | auth via `Authorization: Bearer <oauth>` |
| event = `{candidates, usageMetadata, ...}` | event = `{response: {candidates, usageMetadata, ...}}` |
| free quota: paid by API key project | free-tier quota tied to `cloudaicompanionProject` |

---

## 5. Errors + quota signals

- **401 Unauthorized**: `google.json` envelope — `{"error": {"code": 401, "message": "Request had invalid authentication credentials...", "status": "UNAUTHENTICATED"}}`. Triggers token refresh; if refresh fails, surface as `auth_error`.
- **429 Quota**: `{"error": {"code": 429, "message": "Quota exceeded ... Your quota will reset after Xs.", "status": "RESOURCE_EXHAUSTED", "details": [{"@type": "type.googleapis.com/google.rpc.RetryInfo", "retryDelay": "0.847655010s"}, {"@type": "type.googleapis.com/google.rpc.QuotaFailure", "violations": [{"quotaMetric": "...", "quotaId": "GenerateContentRequestsPerDayPerUserPerTier", "quotaDimensions": {"tier": "free"}}]}]}}`. Free-tier daily exhaustion = `quotaId` containing `PerDayPerUser` and `tier:free`. CLIProxyAPI parses `error.details[*].retryDelay` and falls back to regex `"Your quota will reset after (\d+)s"` (`gemini_cli_executor.go`).
- **403 Permission denied** with `reason: SECURITY_POLICY_VIOLATED` → VPC-SC, surface as `auth_error` (server.ts retry list excludes it).
- **499 / 5xx** → retry per server.ts `statusCodesToRetry: [[429,429],[499,499],[500,599]]`.

Map to existing structured codes:
| upstream | our code |
|---|---|
| 401 / refresh fail | `auth_error` |
| 429 free-tier daily | `quota_exceeded` |
| 429 transient (RetryInfo < 60s) | `rate_limited` |
| 403 SECURITY_POLICY_VIOLATED | `auth_error` |
| 5xx | `upstream_error` |

---

## 6. Rust impl sketch

- **Crate choice**: hand-roll with `reqwest`. The `oauth2` crate adds `RFC 8252 PKCE`/`RFC 7636` ceremony we don't need (web flow is non-PKCE) and forces builder gymnastics around our redirect-URI port. We need ~60 LOC: build authorize URL, run a one-shot `tiny_http` or `hyper` listener on `127.0.0.1:0`, exchange code via plain `reqwest::Client::post` form, then schedule refresh.
- **Token persistence**: `~/.codex-app-transfer/gemini-oauth.json` (do NOT touch `~/.gemini/oauth_creds.json` — would clobber a real gemini-cli install). Schema = subset of Google `Credentials`: `{access_token, refresh_token, scope, token_type, id_token?, expiry_date_ms}`. File mode `0o600`.
- **Concurrency**: `Arc<tokio::sync::Mutex<TokenState>>` where `TokenState = { creds, last_refresh_attempt }`. On request: lock, check `expiry_date_ms - 5min < now()`, refresh if so, drop lock, send request. Single-flight refresh — concurrent requests await the same mutex.
- **Auth scheme name**: `google_oauth_cloud_code`. Reasoning: `google_oauth` is too generic (Google OAuth covers Vertex, GCS, Calendar, …); `cloud_code` pins it to the `cloudcode-pa.googleapis.com` Code Assist surface this scheme actually targets. Future-proofs against adding plain Vertex OAuth later.
- **Bootstrap state**: cache `(projectId, userTier)` next to the token file (e.g. `~/.codex-app-transfer/gemini-cloudcode.json`) so we don't re-run `loadCodeAssist`/`onboardUser` on every cold start. Re-run on tier mismatch or 403/404 from `:streamGenerateContent`.
- **Headers**: hardcode `User-Agent: GeminiCLI/0.34.0/<model> (<os>; <arch>; terminal)` and `X-Goog-Api-Client: google-genai-sdk/1.41.0 gl-node/v22.19.0` (literal). Recompute UA per request because model is in it.
- **Adapter shape**: keep `GeminiNativeAdapter` producing the inner `request` object. New `GeminiCliAdapter` wraps: `{model, project, user_prompt_id: uuid_v4(), request: <native output with session_id injected>}`. Unwrap SSE: read `event.response`, hand to existing native parser unchanged.

---

## Verified sources
- gemini-cli: `oauth2.ts:43,47,54,133,200,207,297,318,329,339,347,362,660-720`; `setup.ts:92-200`; `server.ts` (CODE_ASSIST_ENDPOINT, requestStreamingPost, statusCodesToRetry); `converter.ts:106-131`; `types.ts` (LoadCodeAssistResponse, OnboardUserRequest, GeminiUserTier).
- CLIProxyAPI: `internal/auth/gemini/gemini_auth.go:34-43,67,230`; `internal/auth/gemini/gemini_token.go` (GeminiTokenStorage); `internal/runtime/executor/gemini_cli_executor.go` (URL, headers, 429 parsing); `internal/misc/header_utils.go` (UA + ApiClient strings); `internal/cmd/login.go` (loadCodeAssist + onboardUser bodies, project_id extraction).
