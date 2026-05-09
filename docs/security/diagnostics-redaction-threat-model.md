# Diagnostics Redaction Threat Model

## Goal

Diagnostics should help maintainers understand failures without exposing user secrets or conversation content.

## Assets To Protect

- Provider API keys
- Gateway keys
- Authorization headers
- Cookies
- Custom secret headers
- URL userinfo
- Query tokens
- User conversation content
- Local filesystem paths when not needed for diagnosis

## Allowed Diagnostics

- OS name and version
- CCDS version
- Claude Desktop version if detectable
- Provider display name
- API format
- Base URL host and path without credentials or sensitive query
- Explicit model mapping slots
- Desktop expected/actual base URL
- Gateway running state
- Last failure type
- Last 100 gateway log lines after redaction
- Update and installer status after redaction
- Issue fingerprint

## Redaction Rules

| Input type | Rule |
|---|---|
| API key | Replace with `[REDACTED:key]` |
| Gateway key | Replace with `[REDACTED:gateway-key]` |
| Authorization | Keep scheme only if useful, redact value |
| Cookie | Replace whole value |
| URL userinfo | Remove userinfo |
| Query token/key | Remove value |
| Custom header | Redact names matching key/token/secret/auth/cookie |
| Body preview | Never include user prompt or generated answer unless user opted in |

## Tests

Diagnostics tests must include secrets in:

- provider config
- gateway config
- extra headers
- URLs
- logs
- upstream error body

The export fails if any test secret appears in plaintext.
