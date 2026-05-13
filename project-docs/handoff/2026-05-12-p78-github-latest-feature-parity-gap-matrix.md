# P78 GitHub Latest Feature Parity Gap Matrix

## Scope

After P77 fixed the backend root cause for Proxy stats and request logs, four review agents compared the current Rust mainline worktree with the latest GitHub CC Desktop Switch source.

This document records the feature gap matrix only. It does not mark the gaps as implemented.

## Sources

- Rust mainline worktree: `D:\ccds-build\cc-desktop-switch-rust-mainline`
- Rust mainline HEAD at review time: `5406758658f430756696ec54da3f2a10f63380df`
- GitHub latest checkout: `target\parity-review\github-latest-20260512`
- GitHub latest HEAD at review time: `d8e89f9a860707cf411f842c103540ea205403a4`
- Review lenses:
  - Frontend / UI parity
  - Backend Provider / gateway parity
  - Platform / Desktop writer / update / installer parity
  - User workflow / documentation parity

## High-Level Result

Rust mainline now covers the core safe route model, Provider CRUD/import/export, local gateway forwarding, Desktop local config apply, diagnostics package, tray/single-instance, Windows packaging, and real Proxy stats/logs.

It is not yet full feature parity with the latest GitHub application. The highest-risk gaps are gateway API key enforcement, app update runtime, clear/restart Desktop actions, CC Switch automatic import discovery, and richer provider/model utilities.

## P0 Gaps

| Area | GitHub latest behavior | Rust mainline status | Why it matters | Next minimum implementation |
|---|---|---|---|---|
| Gateway API key auth | Local gateway validates gateway credentials before serving model/message requests | Implemented in P80. Runtime gateway now validates `Authorization` or `x-api-key` for `/v1/models` and `/v1/messages` | Local gateway is the only normal user path; without auth, any local process can call the gateway while it is running | Continue monitoring via `gateway.auth_required`; no further P0 auth work currently known |
| Update runtime | UI can check update metadata, download, verify, launch installer, and report progress/failure | Implemented in P81. Runtime commands now check `latest.json`, stage the selected platform installer with sidecars/public key, verify through release-gate logic, and launch the installer | Users expect in-app update behavior from the existing app; release gates alone do not update installed apps | Add public-URL/manual smoke evidence during RC staging; no further P0 code gap currently known |
| Clear Claude Desktop config | UI button clears CC Desktop Switch-managed Claude Desktop configuration | Implemented in P82. Clear removes only the CCDS-managed `cc-desktop-switch-local-gateway` profile from local `configLibrary`, preserves unrelated profiles, and verifies it is no longer active | User reported this as a visible missing function; fake buttons are high trust risk | Run Windows manual smoke against P82; no further P0 code gap currently known |
| Restart Claude Desktop | Existing app can guide or trigger restart after apply | Implemented in P82 as a command-backed action with UI confirmation and structured failure reporting | Apply flow is incomplete for non-technical users if restart is manual-only without explicit support | Run Windows/macOS real-machine restart smoke; no further P0 code gap currently known |

## P1 Gaps

| Area | GitHub latest behavior | Rust mainline status | Next minimum implementation |
|---|---|---|---|
| CC Switch auto import discovery | Detect existing CC Switch data and show importable candidate list | Partial. Rust supports legacy import packages/templates, but not automatic discovery/list UI | Read known CC Switch paths read-only, parse supported config/SQLite sources, expose preview candidates, then import selected entries |
| Provider model fetch / autofill | Fetch upstream model list and help build mappings | Missing. Mapping UI currently relies on static options/manual route rows | Add provider-specific model fetch command with redacted errors and per-provider fixtures |
| Per-model provider smoke | Existing UI has specific model check/speed-test style actions | Partial. Rust has static and real smoke, but not saved-provider-specific per-model checks | Add command inputs for provider id + route/model id and wire row-level UI checks |
| Provider usage / balance | Existing app exposes provider usage/balance helpers | Missing | Add optional provider capability adapters; keep unavailable providers as explicit unsupported states |
| OpenAI tool compatibility | Existing gateway/new-api compatibility handles more OpenAI fields | Partial. Rust conversion covers basic messages/streaming, but not the full tools/tool_choice/tool_result surface | Extend OpenAI conversion fixtures before exposing broader compatibility claims |
| Provider extra headers/options | Existing provider config can carry extra request metadata | Missing | Add schema fields only after deciding redaction and import/export behavior |
| Protocol auto-detect | Existing UI can infer or help choose compatible protocol | Missing. Rust supports explicit Anthropic/OpenAI Chat selection | Add detect command that probes shape safely and never sends raw model names to Desktop |
| Managed policy export | Existing platform tooling includes managed config paths/artifacts | Partial. Rust detects and blocks managed policy for normal apply, but does not export managed artifacts | Add Windows `.reg` and macOS `.mobileconfig` export fixtures as an advanced path |

## P2 Gaps

| Area | Rust mainline status | Next action |
|---|---|---|
| Modal, confirmation, toast, restart reminder UX | Many actions are command-backed, but confirmation/toast polish is incomplete | Add delete/clear/apply confirmations and persistent restart reminder after apply |
| i18n | Many UI strings are hardcoded Chinese | Centralize user-visible strings before release candidate |
| Settings controls | Theme, autostart, upstream proxy, management port, and some compatibility controls are partial/static | Wire each visible setting to command-backed state or hide it until implemented |
| Navigation behavior | Tauri shell navigation works, but old hash route/back-forward behavior is not fully copied | Decide whether desktop app needs hash parity; if yes, add route state tests |
| Public guide/tutorial parity | Usage guide exists but is not full latest documentation parity | Update guide after P0/P1 actions are real, not before |
| Legacy build scripts | Some PyInstaller/Python release scripts remain in tree as legacy artifacts | Mark as legacy or replace with Rust release scripts before RC packaging |

## Root-Cause-Oriented Next Slice

Do not fix these as isolated button patches. P80 implemented the gateway API key auth slice:

1. Add auth configuration to gateway router state.
2. Validate `Authorization: Bearer <gateway_key>` and `x-api-key: <gateway_key>`.
3. Return structured 401 errors for missing or invalid gateway key.
4. Record redacted request logs without exposing keys.
5. Add tests for `/v1/models` and `/v1/messages`.
6. Rebuild the Windows package only after the auth gate is green.

P81 then implemented the update runtime slice:

1. Add an `update` module for check/download/verify/install.
2. Reuse release-gate validation for selected runtime installer bundles.
3. Wire Tauri commands and Rust/WASM UI buttons.
4. Support Windows setup `.exe` and direct `.msi` launch paths.
5. Rebuild the Windows package after the update gate is green.

P82 then implemented the clear/restart Desktop slice:

1. Add clear logic to `desktop_writer` instead of deleting whole config directories.
2. Preserve unrelated config profiles and metadata.
3. Add command-backed clear and restart Tauri commands.
4. Add UI confirmation and restart reminder.
5. Rebuild the Windows package after the clear/restart gate is green.

The remaining work is now release-readiness and P1/P2 parity: refresh the final human smoke runbook, guard release workflow publishing, run Windows/macOS human smoke, and then continue P1 feature parity.
