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
| Update check | `update` | Parse own `latest.json`, verify asset metadata | latest.json parse, asset pick |
| Installer launch | `update`, `platform` | Hidden subprocess, clear failure errors | process launch mock |
| Single instance | `app/runtime` | Second launch focuses existing window | app lock test/manual |
| Tray/background | `app/runtime` | Closing window keeps gateway available unless user exits | manual smoke |
| Release assets | `xtask/release` | Windows/macOS assets, hashes, signatures, latest metadata | staging fixture tests |

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
