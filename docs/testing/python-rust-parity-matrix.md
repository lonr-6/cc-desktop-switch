# Python Stable Line To Rust Mainline Parity Matrix

This matrix prevents the Rust rewrite from accidentally dropping stable-line behavior.

## Rule

Rust does not need to copy Python internals, but it must preserve intentional user-visible behavior unless a decision document explicitly changes it.

## Matrix

| Stable-line capability | Rust owner module | Expected Rust behavior | Required tests |
|---|---|---|---|
| Provider create/edit/delete/reorder | `core/provider` | Preserve provider data and ordering | config roundtrip, reorder persistence |
| Provider preset import | `core/provider` | Import known presets without leaking keys | preset parse, redaction |
| Model mapping | `core/model_catalog` | Only explicit mapped routes enter Desktop menu | route generation, no raw model names |
| Default model | `core/model_catalog` | Form/config convenience only; no runtime fallback; not Desktop-visible | no Default in `inferenceModels`, unmapped route returns 400 |
| 1M support | `core/model_catalog` | Attach `supports1m` only to explicit supported route | supports1m route test |
| Max support | `core/model_catalog`, `gateway` | Capability-aware mapping; clear error if unsupported | max unsupported fingerprint |
| Claude Desktop local user config | `platform/desktop/local_config` | Write local 3P configLibrary, read back, preserve unrelated settings | configLibrary fixture tests, readback mismatch |
| Claude Desktop managed policy export | `platform/desktop/managed_export` | Generate Windows registry / macOS mobileconfig artifacts without changing local Apply semantics | registry/mobileconfig fixture tests |
| Local gateway | `gateway` | Only normal path from Claude Desktop to provider | request route tests |
| OpenAI/new-api upstream | `gateway/upstream` | OpenAI Chat conversion and structured non-JSON errors | non-stream/stream invalid response tests |
| Anthropic upstream | `gateway/upstream` | Anthropic-compatible forwarding with safe route mapping | messages, streaming tests |
| Diagnostics export | `diagnostics` | Fixed schema, redacted secrets, issue fingerprint | redaction tests |
| Import/export backup | `core/config` | Full backup import/export remains possible | config schema migration tests |
| CC-Switch import | `core/import` | Import supported config format into providers | import fixture tests |
| Update check | `update` | Parse own `latest.json`, verify signed metadata before asset trust, pin release public-key fingerprint | latest.json parse, asset pick, signed metadata rejection |
| Installer launch | `update`, `platform` | Hidden subprocess, clear failure errors | process launch mock |
| Single instance | `app/runtime` | Second launch focuses existing window | app lock test/manual |
| Tray/background | `app/runtime` | Closing window keeps gateway available unless user exits | manual smoke |
| Release assets | `xtask/release` | Windows/macOS assets, hashes, signatures, latest metadata | staging fixture tests |

## Latest GitHub Parity Review 2026-05-12

This snapshot compares the current Rust mainline against latest GitHub CC Desktop Switch HEAD `d8e89f9a860707cf411f842c103540ea205403a4`.

| Priority | Capability | Rust status | Required next test |
|---|---|---|---|
| P0 | Gateway API key auth for `/v1/models` and `/v1/messages` | Implemented in P80 | Missing key, invalid key, valid `Authorization`, valid `x-api-key`, redacted logs |
| P0 | Runtime update check/download/verify/install | Implemented in P81; fail-closed hardened in P83 | Manifest parse, asset selection, latest metadata verification before asset trust, release workflow signing-key and public-key fingerprint guard, pinned public-key fingerprint check, required latest/selected-asset sidecars, temp staging before replacing version staging, plain-file-name rejection, selected bundle sha/signature verification, staging-bound installer launch failure |
| P0 | Clear Claude Desktop CCDS-managed config | Implemented in P82; managed side-effect blocked in P83 | Clear only managed entries, readback verifies removed entries, unrelated config preserved, managed config blocks before local clear |
| P0 | Restart Claude Desktop action | Implemented in P82 | Platform command with user confirmation, structured failure surface, real-machine smoke |
| P1 | CC Switch automatic discovery/import list | Partial manual import only | Read-only discovery fixtures, candidate preview, selected import |
| P1 | Provider model fetch/autofill | Missing | Provider-specific mocked fetch, redacted upstream failure |
| P1 | Saved-provider per-model smoke/speed test | Partial generic smoke only | Provider id + model/route specific smoke fixtures |
| P1 | Provider usage/balance | Missing | Unsupported provider result plus one mocked supported provider |
| P1 | OpenAI tools/tool_choice/tool_result compatibility | Fixture implemented in P83 | OpenAI request conversion fixtures for tools/tool_choice/tool_result, response `tool_calls` -> Anthropic `tool_use`, streaming `tool_calls` delta conversion, route-aware upstream error/SSE/success-response normalization |
| P1 | Provider extra headers/request options | Missing | Config roundtrip, redaction, import/export |
| P1 | Protocol auto-detect | Missing | Safe probe fixtures for Anthropic/OpenAI-compatible endpoints |
| P1 | Managed policy export artifacts | Partial detect/block only | Windows `.reg` and macOS `.mobileconfig` fixture export |
| P2 | Confirmations, toasts, restart reminders, full i18n, settings wiring | Partial; visible no-op settings controls removed/wired in P83 | UI smoke with command-backed actions and no visible no-op controls |

## Known Intentional Changes

| Area | Stable behavior | Rust target |
|---|---|---|
| Direct mode | Some old paths allowed direct provider writes | Hidden advanced/debug only |
| Show all provider models | Could expose too many models | Removed from normal UI |
| Raw model names | Older versions exposed provider model names | Claude-safe route aliases only |
| Browser Admin API | Python UI used local HTTP API | Tauri UI uses commands; gateway API remains separate |
| Default fallback | Stable-line history allowed some fallback behavior | Rust rejects unmapped routes; `Default` is not runtime behavior |

## Stable-Line PR Rule

Stable-line community PRs are handled in the Python stable worktree first. Rust mainline absorbs the final accepted behavior and adds parity/eval coverage, but does not directly import Python patch structure.
