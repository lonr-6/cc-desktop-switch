# Web Search and Tool Result Context Task

## Goal

Resolve the feedback case in `反馈/0512-1/` by first understanding how long web search results and long tool results enter model context, then choosing an implementation plan. This task is about long web/search/tool result payloads entering context, not generic long-context support.

## Current Status

- P1 Option B has been implemented locally and validated. The user confirmed the direction:
  - Option B is the P1 mainline.
  - Option D is the long-term enhancement.
  - Option C is deferred to P2.
- Option D has now also been implemented locally and validated as a sidecar artifact store.
- P1.1 vendor-practice research is complete.
- P1.2 local code-path investigation is complete.
- P1.4/P1.5 implementation validation is complete for the shared `function_call_output` ingress path and Gemini native duplicate path.
- Claude project memories have been imported into Codex memory extension notes:
  `/Users/alysechen/.codex/memories/extensions/ad_hoc/notes/2026-05-13T01-24-19-codex-app-transfer-claude-memory-import.md`.
- P2 has been implemented locally and validated.
- Do not treat "lower the auto-compact threshold" as the solution. The feedback points to tool/search payload budgeting and compact input construction.

## Task Tree

- [x] P1. Why one large research task creates so much context
  - [x] P1.1 Collect current official vendor practices for long web/search/tool results entering context.
  - [x] P1.2 Map this project's current web search and tool-output paths against those practices.
  - [x] P1.3 Review implementation方案 with the user and choose one.
  - [x] P1.4 After confirmation, write tests for the chosen方案.
  - [x] P1.5 After confirmation, implement and validate the chosen方案.
  - [x] P1.6 User review before starting P2.
- [x] P2. Why compact grows after threshold trigger
  - [x] P2.1 Start only after P1方案 is confirmed.
  - [x] P2.2 Add compact request input-budget regression tests.
  - [x] P2.3 Implement compact-only input budgeting and explicit omission metadata.
  - [x] P2.4 Validate compact, adapters, and registry tests.
- [ ] P3. Why Kimi displays about 244k/245k instead of 256k
  - [ ] P3.1 Start only after P1/P2 sequencing is confirmed.
- [x] P4. Option D artifact/source store
  - [x] P4.1 Add sidecar store for raw tool/search/page payloads.
  - [x] P4.2 Put artifact IDs and bounded evidence in model-visible tool content.
  - [x] P4.3 Validate normal Responses, compact, and Gemini native paths.

## P1.1 Vendor Practice Findings

These notes are based on current official documentation.

### OpenAI

- OpenAI Responses exposes web search as a hosted `web_search` tool.
- Responses include a `web_search_call` item and URL citation annotations rather than requiring clients to paste raw page bodies into ordinary history.
- OpenAI exposes consulted `sources` separately from inline citations.
- OpenAI's web search context path is capped at 128k even when the selected model has a larger context window.
- File search exposes `max_num_results`, and raw search results are not returned by default unless requested with `include`.

Sources:
- https://developers.openai.com/api/docs/guides/tools-web-search
- https://developers.openai.com/api/docs/guides/tools-file-search

### Anthropic

- Claude's web search is a server tool.
- Anthropic documents that basic web search can be token-intensive because it can fetch full HTML.
- The newer dynamic filtering path filters retrieved search content before loading it into context.
- `max_uses` limits search count.
- Custom `search_result` blocks advise returning only the most relevant results, preserving source/title/content metadata, and splitting long content into logical blocks.

Sources:
- https://platform.claude.com/docs/en/agents-and-tools/tool-use/web-search-tool
- https://platform.claude.com/docs/en/build-with-claude/search-results

### Google Gemini / Vertex AI

- Google frames web search as grounding.
- `GroundingMetadata` separates search queries, grounding chunks, grounding supports, and search entry points.
- Web chunks carry URI/title/domain evidence instead of being treated as undifferentiated chat text.

Sources:
- https://docs.cloud.google.com/vertex-ai/generative-ai/docs/grounding/grounding-with-google-search
- https://docs.cloud.google.com/vertex-ai/generative-ai/docs/reference/rest/v1/GroundingMetadata

### xAI / Grok

- xAI's current documented web search tool is on the Responses API path.
- It exposes citations/sources and source controls such as `allowed_domains` and `excluded_domains`.
- The documented model is server-side search plus citations, not client-side replay of every fetched page.

Source:
- https://docs.x.ai/developers/tools/web-search

### Perplexity

- Perplexity Search API is a concrete example of explicit result budgeting.
- It exposes `max_results`, `max_tokens_per_page`, and total `max_tokens`.
- It distinguishes raw ranked search results from LLM-generated summaries.

Source:
- https://docs.perplexity.ai/docs/search/quickstart

### Moonshot / Kimi

- Moonshot base chat does not access web/databases/code unless tool workflows are used.
- Kimi `$web_search` is implemented through builtin tool-call compatibility.
- Kimi documents that search results count into `prompt_tokens` and exposes usage/search-token information.
- Kimi K2.5 has tool-use and thinking compatibility constraints.

Sources:
- https://platform.moonshot.cn/docs/introduction
- https://platform.kimi.com/docs/guide/use-web-search
- https://platform.kimi.com/docs/guide/kimi-k2-5-quickstart

## P1.1 Conclusion

Across these vendors, the common pattern is not "put every web page and every tool result into normal conversation history."

The common pattern is:

1. Search/browse runs as a tool or grounding layer.
2. Results carry source metadata and citations.
3. Model-visible content is bounded by result count, domain filters, dynamic filtering, chunks, excerpts, or explicit token budgets.
4. Raw trace/source lists are kept separate from normal conversation text.
5. If retrieved content counts toward prompt tokens, the API exposes enough usage metadata to budget it.

## P1.2 Local Code Findings

### `反馈/0512-1/` evidence now checked

Files in `反馈/0512-1/`:

- `image.png`: screenshot shows the first-turn research workflow auto-compacted, then failed with `Invalid request: Your request exceeded model token limit: 262144 (requested: 336979)`. The same screenshot shows the UI context meter at `244k / 245k tokens used`.
- `bundle-20260512-232148-61422-1778599308510.json`: the relevant failing upstream bundle. It is `POST /responses/compact` to `https://api.kimi.com/coding/v1/chat/completions`, with a transformed chat body of `854476` bytes. The stored diagnostic body is truncated to `251194` characters and reports `592332` omitted bytes. The upstream response body is the Kimi token-limit error above.
- The stored diagnostic body contains many prior tool messages and includes one shell/search tool result with `Original token count: 924828`. That result is a local grep over extracted Codex app assets, including minified JavaScript, and it is being replayed as model-visible `role:"tool"` content.
- `bundle-20260512-014743-42908-1778521663471.json` and `bundle-20260512-014743-42908-1778521663481.json`: unrelated Grok Web 401 bundles, useful only as folder inventory.
- `codex-config.redacted.toml` and `proxy-config.redacted.json`: redacted config snapshots. The Kimi provider path matters because native `$web_search` is not the root failure path here.
- `[反馈] (无标题) · fb-da4d0cc8.pdf`: `pdftotext` found no extractable text; the actionable feedback text is represented by the screenshot.

Conclusion from the feedback bundle:

> The compact request exceeded Kimi's real context because raw tool outputs had already entered the conversation history before compact. The observed failure is not solved by lowering the auto-compact trigger. The first priority is bounding and structuring long tool/search outputs before they become replayable chat history.

### Provider-native web search path

- `docs/web-search-implementation-tracker.md` records that Codex.app sends `{type:"web_search"}` in the inbound tools array by default, and this proxy converts it per provider.
- In `crates/adapters/src/responses/request.rs`, provider-native web search is only forwarded for supported providers and only when enabled by provider options. MiMo and Kimi/Moonshot have special handling; DeepSeek and MiniMax are dropped because their documented Chat APIs do not support native web search tools.
- In `crates/adapters/src/gemini_native/request.rs`, `web_search` can become Gemini `googleSearch`, but it is later dropped when normal function declarations are also present because Gemini rejects built-in search plus function calling in the same request.
- In `crates/adapters/src/grok_web/response.rs`, Grok server-side `webSearchResults` are converted into short reasoning summaries and URL citations. This path is closer to the vendor pattern.

This path is not the direct root cause when it is truly active and bounded. The important caveat is that it is not always active.

### Generic tool-result path

The riskier path is generic `function_call_output`.

- `responses_body_to_chat_body_for_provider_with_session` builds Chat messages from Responses `input`, then clones those messages into session state.
- `input_item_to_messages` maps `function_call_output.output` into Chat `role:"tool"` with full `content`.
- There is no current model-visible budget, source metadata split, URL/title/snippet normalization, or raw-payload separation at that point.
- `compact.rs` reuses this same conversion path, so compact inherits whatever tool content is present in the input.

This is the direct mismatch with vendor practice: ordinary tool output is treated as replayable chat content.

### Mapping to `反馈/0512-1/`

- The active Kimi Code config in `反馈/0512-1/proxy-config.redacted.json` has empty `requestOptions`, so this project's Kimi/Moonshot native `$web_search` path was not enabled.
- The screenshot shows a first-turn large research workflow with browser/search/source-code exploration actions.
- The saved compact diagnostics show many tool messages and large tool content before compaction.

Current P1 hypothesis:

> The failing first-turn research case is mainly caused by local/browser/MCP research outputs entering the next upstream request as full Chat `tool.content`, not by Kimi-native `$web_search` itself.

## Implementation Options For P1

User decision:

- P1 mainline: Option B.
- Long-term enhancement: Option D.
- P2 safety layer: Option C.
- Option A remains a compatibility improvement, but it is not sufficient for this feedback case.

### Option A: Provider-native web search first

Make native `web_search` the preferred path when the active provider supports it, and avoid encouraging fallback to local/browser search tools.

What it changes:
- enable or improve provider-native mapping for supported providers;
- surface unsupported-provider behavior clearly;
- keep citations/source metadata from provider responses.

Pros:
- closest to OpenAI/Anthropic/Google/xAI documented model;
- less local payload management;
- provider can do its own result filtering and citation handling.

Cons:
- provider support is inconsistent;
- Gemini cannot combine `googleSearch` with function calling in the current path;
- Kimi `$web_search` still counts search content toward prompt tokens;
- does not solve arbitrary MCP/browser/local tool outputs.

Best use:
- as a compatibility improvement, not sufficient alone for the feedback failure.

### Option B: Shared bounded `function_call_output` ingress

Add a shared normalization/budget layer before `function_call_output` becomes Chat `tool.content`.

What it changes:
- classify tool outputs into search results, web pages, command/logs, code/file output, opaque JSON, and small output;
- keep tool call IDs intact;
- store raw output separately when possible;
- put only bounded, structured evidence into model-visible `tool.content`;
- enforce per-result and aggregate budgets before session cache and compact.

Pros:
- directly addresses the feedback failure path;
- applies to normal turns and compact because both use the shared converter;
- preserves provider-neutral behavior;
- aligns with vendor practice for bounded retrieved content.

Cons:
- needs careful tests to avoid breaking tool-call repair;
- risk of losing useful information if classification is too crude;
- raw artifact/storage design must be decided if we want more than simple truncation.

Best use:
- likely core P1 solution, but implementation details need confirmation before coding.

### P1 Approved Mainline: Option B concrete design

Scope:

- Add a shared bounded tool-output normalization layer before `function_call_output.output` is written into Chat `tool.content`.
- Primary hook: `crates/adapters/src/responses/request.rs::input_item_to_messages`, because normal `/responses` and `/responses/compact` both pass through this conversion path.
- The compact path currently builds a synthetic Responses body and reuses the same converter in `crates/adapters/src/responses/compact.rs::build_compact_chat_request`, so fixing the shared ingress covers both normal turns and compact input.
- Gemini native has related `function_call_output` handling in `crates/adapters/src/gemini_native/request.rs`; P1 should verify whether it reuses the shared path for the affected route or needs the same normalizer called explicitly.

Behavior:

- Preserve `call_id` / `tool_call_id` exactly so tool-call repair and provider ordering rules do not regress.
- Leave small tool outputs unchanged.
- For large command/log outputs, replace raw content with a bounded evidence summary: command metadata when available, exit code, original byte/character count, first useful lines, last useful lines, matched file paths or headings, and an explicit omission marker.
- For search/web-like results, keep bounded structured evidence: title, URL/domain, snippet/excerpt, result count, and omitted-size metadata. Do not replay full HTML or minified assets as ordinary chat text.
- For large JSON/object output, keep a compact shape/key summary plus bounded excerpts instead of serializing the entire object.
- Do not introduce destructive silent drop. The model-visible message must say content was compressed and what was retained.
- Do not depend on the long-term artifact store in P1. Raw payload storage with source IDs belongs to Option D.

Tests to write before implementation:

- A regression fixture based on `反馈/0512-1/`: a `function_call_output` containing `Original token count: 924828` and minified JavaScript must convert to bounded `tool.content`, while preserving `tool_call_id`.
- `/responses/compact` must use the same bounded output path and must not re-embed the raw tool result before appending the compact prompt.
- Small `function_call_output` strings remain byte-for-byte visible.
- Non-string JSON output remains valid and bounded.
- Tool-call repair tests around orphan `function_call_output` continue to pass.
- Compaction summary items remain unchanged; this fix targets tool outputs, not existing `compaction` items.

Validation target:

- For the feedback-shaped fixture, transformed chat body size should be bounded by policy constants rather than proportional to raw tool output size.
- The transformed request must include explicit omission metadata so the model understands that full raw content was not replayed.
- Existing provider web-search tests for Kimi/MiMo/DeepSeek/MiniMax should remain unchanged because P1 does not change provider-native web-search mapping.

### Option C: Compact-only input pruning

Leave normal tool output conversion unchanged, but add pruning inside `/responses/compact`.

What it changes:
- compact would prune old/large tool outputs before appending the summary prompt;
- compact would possibly drop irrelevant tools and reduce output budget.

Pros:
- smaller code surface;
- directly reduces the observed compact failure.

Cons:
- too late: normal requests can still become huge before compact;
- does not fix first-turn research context growth itself;
- can still leave session history full of raw tool content.

Best use:
- a P2 safety layer after P1, not a standalone P1 solution.

### P2 Active Design: compact request input budget

Root cause:

- `crates/adapters/src/responses/compact.rs::build_compact_chat_request` copies original compact `input` into a synthetic Responses body.
- It then appends `COMPACT_SUMMARIZATION_PROMPT` as an additional user message.
- The synthetic body is converted to Chat messages and sent upstream with `max_output_tokens = 20_000`.
- Before P2 there is no total budget for compact input, so a request can trigger compact near the visible context threshold and then grow further because summary instructions and output allowance are added on top.

Implementation target:

- Apply a compact-only budget after Responses-to-Chat conversion, because that is the actual upstream payload shape.
- Always preserve the final summarization prompt.
- Preserve the newest conversation blocks first.
- Keep assistant tool-call groups together with their following tool outputs so provider tool-order validation does not regress.
- If older content is pruned, insert a model-visible omission notice with counts and user-message excerpts, rather than silently dropping context.
- Keep this separate from P1/P4. P1/P4 prevents raw tool/search payload replay; P2 prevents compact itself from becoming oversized even when history is still large.

Implemented behavior:

- `build_compact_chat_request` now applies a compact-only budget after Responses-to-Chat conversion.
- The final Chat `messages` array is bounded by `COMPACT_CHAT_MESSAGES_MAX_BYTES`.
- The final summarization prompt is always preserved as the last user message.
- The newest conversation blocks are preserved first.
- Assistant messages with `tool_calls` are grouped with their following `tool` messages so compact pruning does not create provider-invalid orphan tool messages.
- Oversized retained messages and tool-call arguments are shortened with explicit markers.
- When the budget layer is applied, a user-visible compact notice is inserted with original message count, omitted message count, original JSON byte size, omitted byte size, and omitted user-message excerpts.

### Option D: Artifact/source store with IDs

Move raw tool/search/page payloads into a sidecar store and pass only source IDs plus selected excerpts to the model.

What it changes:
- raw search/page/tool payloads are saved outside prompt context;
- model-visible content contains source IDs, URL/title/snippet/excerpt, byte counts, and omission metadata;
- follow-up tool calls can request more detail by source ID.

Pros:
- closest to mature retrieval architecture;
- best long-term answer for large research sessions;
- avoids irreversible information loss from simple truncation.

Cons:
- larger product/design surface;
- needs storage lifecycle, privacy, cleanup, and replay fixture decisions;
- may need UI/diagnostic changes.

### Option D Implemented Design

Implemented scope:

- Added `crates/adapters/src/responses/artifact_store.rs`.
- Added a new registry path for `~/.codex-app-transfer/tool_artifacts.db`.
- Large `function_call_output` payloads are written to `ToolArtifactStore` before model-visible chat history is built.
- Model-visible `tool.content` now contains:
  - `Artifact ID`;
  - preserved `Tool call ID`;
  - artifact kind such as `command_output`, `web_or_search`, `file_or_code_output`, `json`, or `opaque_tool_output`;
  - original character and line counts;
  - original token count markers when present;
  - path hints, URL hints, and bounded head/tail excerpts;
  - explicit omission metadata.
- Small tool outputs remain inline unchanged.
- The OpenAI-compatible Responses path, `/responses/compact`, and Gemini native normalized-chat path share the same long-output normalization behavior.

Storage lifecycle:

- Default persistent store is `~/.codex-app-transfer/tool_artifacts.db`.
- Store uses SQLite with a 30-day persisted TTL, matching the existing session-cache retention model.
- If SQLite initialization or writes fail, the store falls back to bounded in-memory storage and emits stable warning IDs.
- Unit tests use in-memory global artifact storage to avoid writing to the developer's real home directory.

Boundary:

- This solves raw payload replay into model context and session history.
- It does not add a new model-callable retrieval tool, because upstream-generated tool calls must correspond to tools known by the Codex client. Adding a hidden provider-only retrieval tool would create unsupported calls on the client side unless the proxy also owns that execution loop.

Best use:
- long-term robust version of Option B.

## Suggested Discussion Order

1. P1: implement Option B with tests first.
2. P2: add compact-only safety pruning if P1 still leaves edge cases.
3. Long-term: design Option D artifact/source store with source IDs and retrieval.

## Execution Record

- 2026-05-13: Created this task document after the user clarified that P1 is about long web/search/tool results entering context.
- 2026-05-13: Completed vendor-practice research.
- 2026-05-13: Completed local code-path investigation for provider-native web search and generic `function_call_output`.
- 2026-05-13: Corrected the document after an implementation was started without user confirmation. The code changes were withdrawn; this document now returns P1 to方案 review state.
- 2026-05-13: Imported this project's Claude memory index into Codex memory extension notes.
- 2026-05-13: User confirmed Option B as P1, Option D as long-term, and Option C as P2.
- 2026-05-13: Rechecked all files under `反馈/0512-1/` and tied the P1 root cause to raw tool output entering `/responses/compact`.
- 2026-05-13: Implemented Option B by bounding large `function_call_output` content before chat-history construction and compact prompt construction.
- 2026-05-13: Added regression coverage for the OpenAI-compatible Responses conversion path, compact request construction, and Gemini native normalized-chat path.
- 2026-05-13: User asked to continue Option D and fully solve the long tool/search payload context issue before P2.
- 2026-05-13: Implemented Option D sidecar artifact storage and changed large tool output summaries from simple compression markers to artifact IDs plus bounded evidence.
- 2026-05-13: User asked to continue P2.
- 2026-05-13: Implemented compact-only input budgeting after Responses-to-Chat conversion, preserving the final prompt and newest valid message blocks while making budget pruning explicit.

## Validation

- P1.1 validation is documentation-level: conclusions are tied to vendor docs listed above.
- P1.2 validation is code-reading level: findings are tied to local files listed in the code-path sections.
- P1.2 feedback validation is tied to `反馈/0512-1/bundle-20260512-232148-61422-1778599308510.json` and `反馈/0512-1/image.png`.
- P1.4/P1.5 validation commands:
  - `cargo fmt --check`
  - `cargo test -p codex-app-transfer-adapters large_tool_output`
  - `cargo test -p codex-app-transfer-adapters large_function_call_output_is_bounded`
  - `cargo test -p codex-app-transfer-adapters`
- P4 validation commands:
  - `cargo fmt --check`
  - `cargo test -p codex-app-transfer-adapters artifact`
  - `cargo test -p codex-app-transfer-adapters large_function_call_output`
  - `cargo test -p codex-app-transfer-adapters large_tool_output`
  - `cargo test -p codex-app-transfer-adapters`
  - `cargo test -p codex-app-transfer-registry`
- P2 validation commands:
  - `cargo fmt --check`
  - `cargo test -p codex-app-transfer-adapters build_compact_chat_request_`
  - `cargo test -p codex-app-transfer-adapters large_tool_output`
  - `cargo test -p codex-app-transfer-adapters large_function_call_output`
  - `cargo test -p codex-app-transfer-adapters artifact`
  - `cargo test -p codex-app-transfer-adapters`
  - `cargo test -p codex-app-transfer-registry`

## Next Step

Review the P1/P2/P4 implementation result with the user. P3 remains next if the user wants to continue.
