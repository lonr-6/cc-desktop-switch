# CC Desktop Switch Rust Mainline Plans

This file is the active execution-plan index for long or risky work.

## Purpose

Use this file when a task is too large to finish safely from chat context alone.
It keeps Codex, subagents, and human reviewers aligned on the same current plan.

`PLANS.md` is not a product roadmap. It is a working control surface:

- what is being built now
- what is explicitly out of scope
- what evidence proves it is done
- what can be rolled back
- what still needs the user's decision

## When A Plan Is Required

Create or update a plan before implementation when the task:

- touches two or more core modules
- changes Claude Desktop policy or gateway protocol behavior
- changes config migration or persisted data schema
- changes release, signing, update, or installer behavior
- requires Windows and macOS parity
- needs subagents or multi-step review

Small text edits, focused tests, and narrow bug fixes can use the task card only.

## Active Plans

| Plan | Status | Owner | Notes |
|---|---|---|---|
| Rust mainline rebuild | P83 hardens subagent review findings before the next human smoke package | Main agent | P1 skeleton/UI spike passed; P2 locked `ModelCatalog`; P3 added config migration; P4 added persisted provider commands; P5/P7 hardened Desktop planner/readback; P6/P8 added gateway core/router skeleton; P9-P17 added upstream forwarding, gateway lifecycle, Desktop writer/apply/probe, and OpenAI stream conversion; P18-P27 added Provider parity, import/export, diagnostics, file picker, smoke, model mapping, presets, and runtime logs; P28 added release manifest gate for required assets, latest.json, sha256, signatures, public key, and macOS x64; P29-P32 hardened app shell and Windows build; P33-P36 added Windows real smoke harness and export-first managed-policy cleanup runbook; P37 added non-publishing macOS arm64/x64 smoke workflow; P38 made `scripts/New-ReleaseManifest.ps1` require macOS x64 by default; P39 added staging-directory validation for `latest.json` referenced assets and sidecars; P40 rejects secret-bearing Provider templates and non-HTTP(S) URLs; P41 binds the pure Rust UI dashboard to `health`; P42 adds offline marketplace manifest validation; P43 reran latest Windows packaged app smoke with temporary config and passed single-instance/close-hide/restore; P44-P69 hardened release/readiness evidence and macOS workflow gates; P70-P75 rebuilt the pure Rust desktop UI against the current frontend/screenshots, restored stable app identity and Windows bundle behavior, wired more command-backed actions, and refreshed macOS workflow evidence. P76 fixed the remaining scroll chain and NSIS path restore. P77 fixed the backend root cause for Proxy stats/logs by recording real gateway request events in AppState, adding stable log ids, and implementing UI auto-scroll. P78 compared Rust mainline to GitHub latest HEAD `d8e89f9` and found P0 gaps. P80 implemented gateway API key auth for `/v1/models` and `/v1/messages`, generated config-scoped gateway keys, rebuilt Windows artifacts, and staged P80 on the desktop. P81 implemented runtime update check/download/verify/install. P82 implemented clear Claude Desktop config and restart Claude Desktop command-backed flows. P83 fixes review findings: update metadata verifies before asset trust and production-pins the public key fingerprint, release workflow requires existing signing secrets and validates the public key fingerprint, selected sidecars are required, download uses temp staging before replacing `updates/<version>/`, installer launch is staging-bound and revalidated, manifest-controlled filenames are plain-file-name only, OpenAI tools/tool calls convert both ways, gateway raw-model preview/SSE/successful-response leaks are normalized to safe routes including JSON string values and streaming tool deltas, authenticated gateway constructors fail fast on empty key, running gateway refreshes after provider/model/settings/port changes, managed clear is fail-closed, and settings/provider/update UI controls are removed, wired, or made state-truthful. Remaining before RC: next xhigh review closure, final human smoke runbook refresh, Windows/macOS human smoke, and P1/P2 known gaps. No publishing unless explicitly requested. |

Latest P83 update: first- through tenth-round xhigh review findings are implemented, including active-vs-selected-vs-editing UI state, Dashboard and Add Provider edit/active actions, Add Provider one-click Apply saving mappings for the edit target before activation, independent model-mapping load/save limited to an existing editing Provider instead of falling back to selected Provider, `set_active_provider Ok(false)` treated as failure with backend refresh, Apply failure readiness invalidation, Desktop readiness invalidation after config changes, backend-truth refresh after partial mutation errors while preserving the edited Provider, `_meta.json` unknown-field preservation, fail-closed clear ordering plus config restore on meta-write failure, release workflow signing-key guard, manual-only publish guard, publish-job `rc-readiness` preflight, current-commit-bound P83 macOS evidence, dual-architecture macOS real Desktop evidence collection, legacy `scripts/New-Release.ps1` hard fail, temp update staging with rollback-preserving replacement, signed metadata verification in `check_update`, successful-response raw-model normalization, CRLF-safe SSE frame parsing, selected-row toolbar removal, Desktop/Proxy truth-state styling, neutral stopped proxy status, stale verified update path clearing, strict proxy-port validation including backend rejection of `proxy_port=0`, `proxy_port` gateway refresh without dropping the old listener on occupied new ports, successful port changes stopping the old listener, redacted UI Provider export, dry-run DTO without secret-bearing `plan`, production removal of the fixture-only `apply_local_config` command, stricter imported route validation, `ccds-gw-*` diagnostics redaction, non-echoing unmapped raw route errors/logs, route replacement before upstream preview truncation, `phase: P83` readiness markers in macOS collectors and `rc-readiness`, and production-only pinned update trust anchor. `cargo xtask verify --all` passes on Windows x64 with 153 passed / 2 ignored tests and refreshed MSI/NSIS artifacts under `target/release/bundle/`; collector fixture checks pass for current commit and reject bad commit/single-arch real smoke; `git diff --check` has no whitespace errors. Ninth-round backend/joint/API and tenth-round frontend focused re-review reported no blocker/high/medium. `cargo xtask verify --stage rc-readiness` is intentionally incomplete until fresh P83 macOS arm64/x64 workflow evidence and fresh P83 macOS real Desktop evidence for the current commit are recorded. P82 Windows artifacts are stale for final human smoke; do not use them as final RC evidence. Next slice is P83 macOS workflow evidence and final human smoke before any RC packaging/publishing work.

## Resolved Decisions

- `Default` is only a form/config convenience and never a runtime fallback.
- macOS x64 is a hard gate for `v1.1.0-rc1`.
- Python stable-line PRs are handled on the stable line first; Rust mainline absorbs accepted behavior/tests separately.
- Subagent review 2026-05-08 found real gaps; high-risk apply/gateway work must first preserve strict `ModelCatalog` errors, route identity, configured proxy port, and strict readback comparison.

## Hard Parity Tracks

| Track | Current status | Next minimum task |
|---|---|---|
| Gateway/upstream | Authenticated runtime router, adapter conversion, non-stream forwarding, Anthropic SSE, OpenAI SSE semantic conversion, OpenAI tools/tool_choice/tool_result/tool_calls fixture conversion, Anthropic-style local/upstream error envelopes, raw-model preview/SSE normalization to safe routes including JSON string values, Tauri state lifecycle, local gateway smoke, real in-memory request stats/logs, and gateway API key auth exist; unauthenticated router constructors are test-only and auth constructors fail fast on empty keys | Add richer stream body error accounting and provider-specific real smoke once API keys are available |
| Desktop writers | Planner/readback health exists; local configLibrary writer, path probe, managed config block, UI Apply, opt-in Windows and macOS real smoke harnesses, Windows/macOS evidence wrappers, and export-first cleanup runbook exist; P62 Windows real local config smoke passed with backup/readback/loopback gateway/restore evidence | Run macOS real Claude Desktop smoke on both architectures through the macOS wrapper and add managed export fixtures |
| Provider CRUD/import/export | save/list/set-active/edit/delete/reorder/import/export/model-mapping/preset-import/template-import/marketplace-manifest exists with persisted roundtrip tests, conflict dry-run, skip-existing merge, legacy CC-Switch import tests, backup list/readback, Leptos UI controls, template rejection for secret-bearing fields/non-HTTP(S) URLs, offline marketplace sha256 validation, and running-gateway refresh after provider/model/settings/import/delete mutations; `proxy_port` is part of gateway fingerprint | Add signed/verified external preset marketplace source after platform smoke; do not enable network fetch without signature trust policy |
| Diagnostics/report issue | redaction core, structured diagnostics package, runtime logs, redacted config JSON, summary/package/copy/save/save-as/issue/open commands, Leptos buttons, and dashboard Report issue shortcut exist | Add richer issue attachment guidance and real app log capture if needed |
| App shell/UI | Windows GUI subsystem set; single-instance plugin registered; tray close-to-hide and explicit tray quit implemented; P74 binds tray id/tooltip/default icon, rebuilds the pure Rust desktop UI against the current screenshots, and keeps mobile out of the acceptance target; P75 fixes initial desktop scrolling and replaces more placeholder controls with Rust command-backed actions; P76 tightens the actual scroll chain and NSIS path restore; P77 makes Proxy stats/logs real and auto-scroll functional; P80 requires local gateway auth; P81 adds command-backed update check/download/verify/install; P82 adds command-backed clear/restart Desktop actions and restart reminder; P83 removes/wires visible no-op settings controls, disables installer launch until a verified download exists, splits selected Provider from active Provider, separates import preview/import actions, relabels active-provider static checks, removes hidden selected-row Provider toolbar mutations, and makes Desktop/Proxy status colors follow backend truth; remaining UI/action gaps are P1/P2: CC Switch auto import, provider model fetch/usage/smoke, confirmations/toasts/i18n; Windows real Desktop local config smoke passed before P74/P75, but installer/icon/config inheritance still needs manual in-place upgrade smoke | Build a fresh P83 Windows package for upgrade smoke, then continue P1/P2 parity |
| Release | Windows x64 local build passes; Rust release gate and PowerShell manifest smoke reject missing macOS x64, missing sha256/sig/public key, invalid `latest.json`, absent `latest.json` asset references, invalid sha256 sidecars, sha256 content mismatches, invalid signature algorithms, invalid signatures, and signature mismatches; P83 adds runtime update metadata verification before asset trust, production-pinned public-key fingerprint enforcement with test-only fixture override, required selected sidecars, temp-staging-before-replace download behavior, plain-file-name enforcement, release-directory manifest filename validation, staging-bound install revalidation, signed metadata verification in `check_update`, and release workflow guard requiring manual `workflow_dispatch`, existing signing secrets, public-key fingerprint validation, explicit `confirm_publish=PUBLISH_LATEST`, and the `release-publish` environment before Latest publishing; P75 follow-up workflow evidence is now historical only, because `rc-readiness` requires fresh P83 macOS arm64/x64 workflow and real Desktop evidence | Rerun non-publishing macOS workflow after P83, collect evidence, then human smoke; do not publish or update `latest.json` |

## Plan Rules

Each active T2/T3 plan must include:

- Goal
- Non-goals
- Current source of truth
- Work packages
- Required decisions
- Verification gates
- Release or rollback path
- Subagent scope, if any

Use `project-docs/templates/task-plan.md` for new plans.

## Codex Feature Mapping

- `AGENTS.md`: durable repo rules and short project truth.
- `.agents/skills/*`: reusable workflows Codex should trigger for repeated work.
- `PLANS.md`: active execution plans for long-running tasks.
- `project-docs/status.md`: short current truth and next step.
- `project-docs/bugs/*`: incident history and root-cause register.
- `docs/testing/eval-harness.md`: repeatable local evals and verification commands.
