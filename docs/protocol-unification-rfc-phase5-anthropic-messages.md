# Protocol Unification RFC (Phase 5 Anthropic Messages)

> Status: P6 complete on 2026-05-13; P7 documentation, full acceptance, and real Claude validation remain.
> Scope: Add a first-class `anthropic_messages` protocol adapter for Claude-family providers.

## Goal

Add a real OpenAI Responses <=> Anthropic Messages conversion path so Codex can talk to Claude-compatible upstreams through Anthropic `/v1/messages` while still exposing the local Responses contract to Codex clients.

This phase must follow the existing `core + mapper + thin adapters` architecture:

- `core` keeps provider-neutral lifecycle helpers, including route checks, previous-response recovery, tool-call cache support, and Responses SSE event emission.
- `mapper/anthropic_messages.rs` owns Anthropic Messages request and response mapping.
- `anthropic_messages/mod.rs` stays a thin adapter that delegates to the mapper and shared core helpers.
- `registry.rs` routes `anthropic_messages` and its historical aliases to the new adapter.

## Non-Goals

- Do not add a Claude preset until request mapping, response mapping, registry routing, and real-provider smoke validation are complete.
- Do not treat `anthropic`, `claude`, or `messages` as native pass-through values.
- Do not convert Anthropic server tools into Codex local function tools in MVP.
- Do not implement non-streaming normal Responses output in MVP. Normal requests should use upstream streaming and map Anthropic SSE back to Responses SSE.

## Canonical Protocol

Canonical protocol:

```text
anthropic_messages
```

Aliases that should normalize or route to it:

```text
anthropic
claude
messages
claude_messages
```

The canonical name is intentionally explicit. A generic `messages` canonical value would be easy to confuse with local `/messages` route compatibility and the existing historical alias behavior.

## MVP Scope

The MVP includes:

- Responses request body -> Anthropic Messages request body.
- Anthropic Messages SSE -> Responses SSE.
- Tool call and tool result conversion.
- Thinking/reasoning conversion.
- Upstream Anthropic error conversion.
- Unknown Anthropic SSE event tolerance.
- `/responses/compact` support for Claude-compatible providers.

The MVP does not include:

- Anthropic server tool support.
- Anthropic JSON schema guarantees beyond the fields already proven in fixtures.
- UI preset exposure before end-to-end validation.

## Compact Decision

`/responses/compact` is in MVP.

Reason: compact is a local Codex lifecycle endpoint, not an optional model feature. If Claude sessions can answer normal turns but fail once Codex asks for compaction, the adapter is not usable for long-running work.

Implementation rule:

1. Detect compact paths with the existing `core::routes` / `responses::compact` route logic.
2. Reuse the existing compact prompt injection and history-budget behavior as the source of truth for summary input construction.
3. Convert the resulting chat-shaped compact request into Anthropic Messages shape.
4. Send compact upstream as `/messages` with `stream: false`.
5. Wrap Anthropic non-streaming content back into Codex compact response shape:

```json
{
  "output": [
    {
      "type": "compaction",
      "encrypted_content": "<SUMMARY_PREFIX>\n<summary>"
    }
  ]
}
```

The existing chat compact response wrapper extracts `choices[0].message.content`. Anthropic compact handling must instead extract text from Anthropic `content[]` blocks, preserving the same `<summary>...</summary>` stripping and `SUMMARY_PREFIX` behavior.

## Request Mapping Requirements

Request mapper input is the local Responses request body. Output is an Anthropic Messages request plan.

Required mappings:

- `model` -> `model`.
- `instructions`, `developer`, and system-like messages -> top-level `system`.
- user text -> `content[]` text blocks.
- user images -> Anthropic image blocks when the source is supported.
- assistant text -> assistant text blocks.
- Responses `function_call` or chat `assistant.tool_calls` -> assistant `tool_use` blocks.
- Responses `function_call_output` or chat `tool` messages -> user `tool_result` blocks.
- Responses function tools -> Anthropic `tools[]` with `input_schema`.
- `tool_choice` -> Anthropic `auto`, `any`, `none`, or named `tool`.
- `parallel_tool_calls` -> Anthropic `disable_parallel_tool_use` with inverted meaning.
- `max_output_tokens` -> Anthropic required `max_tokens`.
- `stop` -> `stop_sequences`.
- `reasoning` / `reasoning_effort` -> Anthropic `thinking` conservatively.
- `user` or `metadata.user` -> `metadata.user_id` after Anthropic-safe filtering.

Mapper must return a diagnostic 400 when a tool result cannot be associated with a prior tool use after the existing cache repair path has run.

## Response Mapping Requirements

Anthropic streaming events must map to Responses SSE lifecycle events:

- `message_start` opens the response.
- `content_block_start` opens text, reasoning, or function-call items.
- `content_block_delta` streams text, thinking, or partial tool JSON.
- `content_block_stop` closes the active item.
- `message_delta` records stop reason and usage.
- `message_stop` emits `response.completed` or `response.incomplete`.
- `error` emits structured failure after stream start or returns an upstream error before stream start.
- `ping` is ignored.
- Unknown events are ignored with trace-level logging.

Stop reason mapping:

- `end_turn` -> completed.
- `tool_use` -> completed with function-call output.
- `max_tokens` -> incomplete with `max_output_tokens`.
- `stop_sequence` -> completed with stop metadata where possible.

## Fixture Set

P2 introduces Anthropic SSE fixtures under:

```text
crates/adapters/tests/fixtures/anthropic_messages/
```

Required fixture categories:

- text stream.
- thinking stream.
- tool-use stream.
- upstream error stream.
- unknown event stream.

P2 also introduces request/expected JSON fixtures for the request mapper. These are wired to ignored TDD tests until P3 adds the real mapper.

## Test Strategy

P2 tests:

- Parse and validate fixture shape.
- Compile ignored request-mapper tests that intentionally fail when run with `--ignored`.

P3 promoted those ignored tests by replacing the placeholder panic with real mapper calls and adding request-side coverage for path/header defaults, compact lowering, tool-name sanitization, `tool_choice`, thinking, metadata filtering, and orphan tool-result diagnostics.

P4 added response-side tests for text, thinking, tool use, sanitized tool-name reverse mapping, upstream errors, unknown events, `max_tokens`, interrupted streams, session-cache writes, and Anthropic compact responses.

P5 added adapter/registry contract tests and proxy forwarding tests so adapter default protocol headers, including `anthropic-version`, reach upstream while provider `extraHeaders` keeps override precedence.

P6 added admin/provider and UI coverage:

- backend `apiFormat` normalization now persists `anthropic_messages` for `anthropic` / `claude` / `messages` / `claude_messages` aliases;
- provider connection tests use Anthropic `/v1/messages` URL construction, Anthropic-shaped ping bodies, and `anthropic-version: 2023-06-01`;
- model-list discovery derives `/v1/models` from Anthropic Messages endpoints;
- direct mode bypass remains restricted to `responses` / `openai_responses`;
- frontend custom provider save/display/i18n now uses canonical `anthropic_messages`.

Expected commands after P2:

```bash
cargo fmt --all
cargo test -p codex-app-transfer-adapters --test anthropic_messages_request
```

Expected future TDD command during P3:

```bash
cargo test -p codex-app-transfer-adapters --test anthropic_messages_request -- --ignored
```

## Rollback Strategy

Until P3, rollback was deleting this RFC, fixture files, and the P2 integration test. No production path was affected.

After P6, rollback should be phase-scoped:

1. Remove provider UI exposure and admin normalization to stop new `anthropic_messages` provider configs from being saved.
2. Remove registry alias routing so `anthropic_messages` is no longer selected for live requests.
3. Remove `AnthropicMessagesAdapter` wiring and mapper exports.
4. Keep mapper fixtures and tests if they still describe the intended future contract.

## P7 Acceptance Gates

Before adding a Claude preset or marking this protocol stable:

- update architecture/readme/release documentation;
- run `cargo fmt --all`;
- run `cargo test -p codex-app-transfer-adapters`;
- run `cargo test -p codex-app-transfer-registry`;
- run `cargo test -p codex-app-transfer`;
- validate frontend static-resource embedding through the Tauri/Rust build path, because this repo has no root `package.json` and no standalone `npm run build`;
- run real Claude-compatible smoke tests for text, tool call/tool result, `previous_response_id`, and upstream error mapping with local secrets, without printing secret values.
