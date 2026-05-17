//! 本地实现 OpenAI Responses 私有 `/responses/compact` 端点。
//!
//! Codex CLI 在累计 token 超过 `model_auto_compact_token_limit` 时会调
//! `POST /responses/compact`,期望后端做"上下文压缩"——把整段对话历史摘要成
//! 一段简短的纯文本 summary,用 `{"output":[{"type":"compaction",
//! "encrypted_content":"<SUMMARY_PREFIX>\n<text>"}]}` 形态回写。
//!
//! 这是 OpenAI 官方 Responses API 的私有扩展,**所有第三方 OpenAI-compatible
//! provider(MiMo / Kimi / DeepSeek / MiniMax / 智谱 / 百炼)都不支持**——
//! 透传必 404,litellm 也只对 openai provider 实现透传。
//!
//! 本模块在我们代理层本地实现:把 `CompactionInput` 重组成普通
//! `/chat/completions` 请求(注入抄自 codex 自家的 SUMMARIZATION_PROMPT 作
//! 为 system message),拿到上游 chat completion 响应后,提取
//! `choices[0].message.content` 作为 summary,包装成 Codex CLI 期待的
//! compact 响应。
//!
//! ## 协议来源
//!
//! 通过 `openai/codex` 公开源码反查(Apache-2 license,标注引用):
//! - 请求结构 `CompactionInput`:`codex-rs/codex-api/src/common.rs`
//! - 响应结构 `CompactHistoryResponse { output: Vec<ResponseItem> }` +
//!   `ResponseItem::Compaction { encrypted_content: String }`:
//!   `codex-rs/codex-api/src/endpoint/compact.rs` + `codex-rs/protocol/src/models.rs:882`
//! - SUMMARY_PREFIX / SUMMARIZATION_PROMPT 文本:
//!   `codex-rs/core/templates/compact/summary_prefix.md`、
//!   `codex-rs/core/templates/compact/prompt.md`
//! - `encrypted_content` 字段名是历史包袱,**实际是明文** `format!("{PREFIX}\n{summary}")`
//!   (`codex-rs/core/src/compact.rs:262`)。

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use futures_util::stream::StreamExt;
use http::{HeaderMap, HeaderValue, StatusCode};
use serde_json::{json, Value};

use crate::core::routes;
use crate::types::{AdapterError, ByteStream, ResponsePlan};

use super::request::responses_body_to_chat_body_for_provider;

/// **v2.0.12 prompt rewrite**:从原 Codex CLI 的 86 字符 prompt 改为 Claude Code
/// 风格的 9-section 结构化 prompt(精简移植自 Piebald-AI/claude-code-system-prompts
/// 反编译公开版本 `agent-prompt-conversation-summarization.md`)。
///
/// ## 为什么改
///
/// 原 Codex prompt(86 字符)对 GPT-5 系列指令遵循够用,但接到第三方 provider
/// (Kimi/MiMo/DeepSeek)指令遵循能力相对弱,实测 summary 只记最近 1-2 个动作,
/// 丢任务目标 / 文件路径 / 历次 user 主诉 → 用户体感"compact 后断片"。
/// (用户反馈截图:"在 curl 网页"不知道 curl 什么 / 为什么。)
///
/// Claude Code "几乎感觉不到断点" 不是模型更强,是 **prompt 把"必须保留什么"
/// 枚举死了**:9 个固定 section、chronological 强制要求、ALL user messages
/// 逐字列、Next Step 强制 verbatim quote 最近用户诉求。
///
/// ## 关键设计点
///
/// 1. **`<analysis>` + `<summary>` 二段输出**:让模型先做时序 chain-of-thought
///    再生 summary;`collect_and_wrap_compact_body` 抽 `<summary>` 段落注入
///    下一轮(避免 analysis 部分污染 history)。
/// 2. **9 section 强 schema**:每条 section 用 markdown header,模型只能填空。
/// 3. **All User Messages 必须逐字列**(section 6):防丢历次主诉 —
///    用户中途换需求 / 给反馈是最常被压缩掉的信息。
/// 4. **Next Step 强制 verbatim quote**(section 9):最近用户原话引用,防 drift。
/// 5. **Files and Code Sections 含具体文件路径 + 完整 snippets**(section 3):
///    防丢实现细节(用户截图里 "curl 网页"不知道 curl 什么 = 这条 section 没填)。
/// 6. **末尾 few-shot example**:第三方 provider 没 example 时输出格式飘,
///    给一段示范让模型对齐结构(花费几百 token 换稳定输出)。
const COMPACT_SUMMARIZATION_PROMPT: &str = "Your task is to create a detailed CONTEXT CHECKPOINT summary of the conversation so far, paying close attention to the user's explicit requests and your previous actions. This summary should be thorough in capturing technical details, code patterns, and architectural decisions that would be essential for continuing the work without losing context.

Before providing your final summary, wrap your analysis in <analysis> tags to organize your thoughts. In your analysis:

1. Chronologically walk through every message in the conversation. For each message identify:
   - The user's explicit requests and intents
   - Your approach to addressing those requests
   - Key decisions, technical concepts, and code patterns
   - Specific details: file paths, full code snippets, function signatures, command-line invocations, URLs, configuration values
   - Errors encountered and how they were fixed
   - Any user feedback that asked you to do something differently — capture this verbatim

2. Double-check that your analysis covers every request the user made and every concrete artifact (file, command, URL, error message) referenced.

After the analysis, provide your summary inside <summary> tags using EXACTLY these nine sections in this order:

1. **Primary Request and Intent**: All of the user's explicit requests captured in detail. Include the original phrasing where possible.
2. **Key Technical Concepts**: Technologies, frameworks, libraries, protocols, and tools that came up.
3. **Files and Code Sections**: Enumerate every file you examined / modified / created. Use absolute paths when known. Include the most important code snippets verbatim.
4. **Errors and Fixes**: Every error you ran into and exactly how it was resolved. Note any user correction verbatim.
5. **Problem Solving**: Problems solved and ongoing troubleshooting threads.
6. **All User Messages**: List ALL user messages (excluding tool results) verbatim or near-verbatim, in chronological order. This is critical — it preserves intent shifts that get lost otherwise.
7. **Pending Tasks**: Tasks the user explicitly asked for that are not yet completed.
8. **Current Work**: Precisely what was being worked on right before this checkpoint. Include relevant file names and code snippets.
9. **Next Step**: The immediate next action, DIRECTLY in line with the user's most recent explicit request. Include a verbatim direct quote from the most recent user message showing exactly where you left off — this prevents task drift.

Be thorough and structured. Do NOT compress at the cost of losing file paths, command-line invocations, URLs, error messages, or the user's literal words. The next LLM should be able to seamlessly continue the work without asking the user to re-explain anything.

<example>
<analysis>
The user started by asking to review the auth module for race conditions. I read src/auth/login.rs:120-180 and found a TOCTOU on session_token validation. The user then corrected me, saying \"actually the bug is in refresh, not login\". I switched to src/auth/refresh.rs:45-90 and found the actual race in cache invalidation. Final user message before this checkpoint: \"add a regression test for the refresh race\".
</analysis>

<summary>
1. **Primary Request and Intent**: Review the auth module for race conditions; the user clarified the bug was in the refresh path, not login.
2. **Key Technical Concepts**: TOCTOU race, session token validation, cache invalidation, tokio Mutex.
3. **Files and Code Sections**:
   - `src/auth/login.rs:120-180`: original suspicion (false positive — no actual race here).
   - `src/auth/refresh.rs:45-90`: actual race in cache invalidation between `lookup` and `replace`.
4. **Errors and Fixes**: Initially misidentified the buggy file. User correction (verbatim): \"actually the bug is in refresh, not login\".
5. **Problem Solving**: Identified the race, designed fix using `tokio::sync::Mutex` around the lookup-replace section.
6. **All User Messages**:
   - \"review the auth module for race conditions\"
   - \"actually the bug is in refresh, not login\"
   - \"add a regression test for the refresh race\"
7. **Pending Tasks**:
   - Add regression test for the refresh race.
8. **Current Work**: Just identified the race in `src/auth/refresh.rs:45-90`; designed the Mutex fix but have not yet written code.
9. **Next Step**: Add a regression test for the refresh race, per user's most recent message: \"add a regression test for the refresh race\".
</summary>
</example>";

/// 抄自 `openai/codex` 仓库 `codex-rs/core/templates/compact/summary_prefix.md` (Apache-2).
/// Codex CLI 反序列化 compact 响应后,通过 `is_summary_message`(`startswith(PREFIX)`)
/// 识别这段文本是 compaction summary 并接管历史回放。**前缀必须保持字面一致**。
pub(crate) const COMPACT_SUMMARY_PREFIX: &str = "Another language model started to solve this problem and produced a summary of its thinking process. You also have access to the state of the tools that were used by that language model. Use this to build on the work that has already been done and avoid duplicating work. Here is the summary produced by the other language model, use the information in this summary to assist with your own analysis:";

/// `COMPACT_USER_MESSAGE_MAX_TOKENS` from `codex-rs/core/src/compact.rs:48`.
const COMPACT_MAX_OUTPUT_TOKENS: u32 = 20_000;

/// Compact must reserve room for the summarization prompt and the generated
/// summary. This is a byte budget over the final Chat `messages` array, applied
/// after Responses-to-Chat conversion because that is the real upstream shape.
const COMPACT_CHAT_MESSAGES_MAX_BYTES: usize = 120 * 1024;
const COMPACT_OMISSION_NOTICE_MAX_CHARS: usize = 8_000;
const COMPACT_SINGLE_MESSAGE_MAX_CHARS: usize = 8_000;
const COMPACT_TOOL_ARGUMENTS_MAX_CHARS: usize = 3_000;
const COMPACT_EXCERPT_HEAD_CHARS: usize = 1_800;
const COMPACT_EXCERPT_TAIL_CHARS: usize = 1_000;

/// 收上游 chat completions 响应的最大字节数,防止异常 provider 把我们打挂内存。
/// 32 MB 远超合理 chat completion 响应大小(typical 几十 KB)。
const MAX_UPSTREAM_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// 判断入站 path 是否是 `/responses/compact`(含可选 `/v1/`、`/openai/v1/` 前缀)。
pub(crate) fn is_compact_path(path: &str) -> bool {
    routes::is_exact_responses_compact_path(path)
}

/// 把 Codex CLI 的 `CompactionInput` JSON 改写成上游 `/chat/completions` 请求体。
///
/// 策略(v2.0.12 调整):
/// - **注入 `COMPACT_SUMMARIZATION_PROMPT` 作为最后一条 user message**(append
///   到 input 数组末尾),而不是 instructions/system。原因:
///   * 第三方 provider 对 user 服从度普遍 > system,structured prompt 更被尊重
///   * 避免 system prompt cache 截断 / 去重(部分 provider 把超长 system 截短)
///   * 对齐 Codex CLI 自家做法(`compact.rs::build_compact_request` 把 prompt
///     当 `UserInput::Text` 注入)
/// - 保留 `input` 数组(原对话历史),交给现有 `responses_body_to_chat_body_for_provider`
///   做 ResponseItem → ChatMessage 转换、merge consecutive、tool call repair、vision 剥离等
/// - `stream = false`(上游回完整 chat completion JSON,不是 SSE)
/// - 丢弃 `instructions`(摘要任务不应受原任务 system prompt 影响)
/// - 保留 `tools`(`ensure_thinking_tool_call_reasoning` 的 `has_tool_loop`
///   检测需要,且第三方 provider 看到 tools 字段不会 400)
pub(crate) fn build_compact_chat_request(
    body_bytes: &[u8],
    provider: &Provider,
) -> Result<Vec<u8>, AdapterError> {
    let parsed: Value = serde_json::from_slice(body_bytes)
        .map_err(|e| AdapterError::BadRequest(format!("compact body 不是合法 JSON: {e}")))?;
    let model = parsed.get("model").cloned().unwrap_or(Value::Null);
    let raw_input = parsed.get("input").cloned();

    // A2:把 SUMMARIZATION_PROMPT 作为最后一条 user message append 到 input。
    // 必须**先 normalize input 为 array**才能可靠 append —— `extract_input_items`
    // (`responses/request.rs:376`)接受 Null / String / Object / Array 多种形式,
    // 实际客户端 body 也可能是 string/object(非典型但合法)。如果只 match
    // array 路径,non-array input 时会**完全丢失 prompt**,上游收到无 summary
    // 指令的请求,返回任意 chat 内容而不是 summary —— PR #71 codex review 报
    // 的 P2 隐患(2026-05-08)。
    let mut input_array: Vec<Value> = match raw_input {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(arr)) => arr,
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![json!({
                    "type": "message",
                    "role": "user",
                    "content": s,
                })]
            }
        }
        Some(obj @ Value::Object(_)) => {
            // 已是 single item object(可能是带 type 的 input item,也可能是
            // {role,content} 形式),直接当 array[0]
            vec![obj]
        }
        Some(other) => {
            // bool / number 等非典型形式,toString 包成 user message 兜底
            vec![json!({
                "type": "message",
                "role": "user",
                "content": other.to_string(),
            })]
        }
    };
    input_array.push(json!({
        "type": "message",
        "role": "user",
        "content": COMPACT_SUMMARIZATION_PROMPT,
    }));
    let input = Value::Array(input_array);

    let mut synthetic_responses_body = json!({
        "model": model,
        "input": input,
        "stream": false,
        "max_output_tokens": COMPACT_MAX_OUTPUT_TOKENS,
    });

    // 透传原 CompactionInput 里的 thinking-相关字段。
    // 关键:`responses_body_to_chat_body_for_provider` 内部的
    // `ensure_thinking_tool_call_reasoning` 通过 `body.get("reasoning")` 判断
    // 是否启用 thinking,只在 reasoning 字段存在时才给 history 里的
    // assistant tool_call message 补 reasoning_content。如果不透传,Kimi /
    // DeepSeek 等 thinking 默认开的上游会 400 报
    // "thinking is enabled but reasoning_content is missing in assistant
    // tool call message"。
    if let Some(reasoning) = parsed.get("reasoning") {
        synthetic_responses_body["reasoning"] = reasoning.clone();
    }
    if let Some(tools) = parsed.get("tools") {
        // 工具定义需要透传(含 ensure_thinking_tool_call_reasoning 路径
        // 的 has_tool_loop 检测,以及万一上游借 tool 信息提取上下文)。
        synthetic_responses_body["tools"] = tools.clone();
    }

    let chat_body =
        responses_body_to_chat_body_for_provider(&synthetic_responses_body, Some(provider))?;
    let chat_body = enforce_compact_chat_message_budget(chat_body);
    serde_json::to_vec(&chat_body)
        .map_err(|e| AdapterError::Internal(format!("re-serialize compact body: {e}")))
}

fn enforce_compact_chat_message_budget(mut chat_body: Value) -> Value {
    let Some(messages) = chat_body.get_mut("messages").and_then(|v| v.as_array_mut()) else {
        return chat_body;
    };
    let original_bytes = serialized_messages_len(messages);
    if original_bytes <= COMPACT_CHAT_MESSAGES_MAX_BYTES {
        return chat_body;
    }
    let Some(prompt_message) = messages.pop() else {
        return chat_body;
    };
    let original_message_count = messages.len() + 1;
    let groups = group_chat_messages(std::mem::take(messages));
    let prompt_bytes = serialized_messages_len(&[prompt_message.clone()]);
    let history_budget = COMPACT_CHAT_MESSAGES_MAX_BYTES
        .saturating_sub(prompt_bytes)
        .saturating_sub(COMPACT_OMISSION_NOTICE_MAX_CHARS + 512);

    let mut retained_rev: Vec<Vec<Value>> = Vec::new();
    let mut retained_bytes = 0usize;
    let mut split_at = groups.len();

    for idx in (0..groups.len()).rev() {
        let compacted = compact_group_for_budget(groups[idx].clone());
        let group_bytes = serialized_messages_len(&compacted);
        if retained_bytes + group_bytes > history_budget && !retained_rev.is_empty() {
            split_at = idx + 1;
            break;
        }
        retained_bytes = retained_bytes.saturating_add(group_bytes);
        retained_rev.push(compacted);
        split_at = idx;
    }

    retained_rev.reverse();
    let mut retained_groups = retained_rev;
    let mut new_messages: Vec<Value> = Vec::new();
    if original_bytes > COMPACT_CHAT_MESSAGES_MAX_BYTES {
        new_messages.push(build_compact_omission_notice(
            &groups[..split_at],
            original_message_count,
            original_bytes,
        ));
    }
    for group in &retained_groups {
        new_messages.extend(group.iter().cloned());
    }
    new_messages.push(prompt_message.clone());

    while serialized_messages_len(&new_messages) > COMPACT_CHAT_MESSAGES_MAX_BYTES
        && !retained_groups.is_empty()
    {
        retained_groups.remove(0);
        let omitted_count = groups.len().saturating_sub(retained_groups.len());
        new_messages.clear();
        new_messages.push(build_compact_omission_notice(
            &groups[..omitted_count],
            original_message_count,
            original_bytes,
        ));
        for group in &retained_groups {
            new_messages.extend(group.iter().cloned());
        }
        new_messages.push(prompt_message.clone());
    }

    if serialized_messages_len(&new_messages) > COMPACT_CHAT_MESSAGES_MAX_BYTES {
        new_messages.clear();
        new_messages.push(build_compact_omission_notice(
            &groups,
            original_message_count,
            original_bytes,
        ));
        new_messages.push(prompt_message);
    }

    *messages = new_messages;
    chat_body
}

fn serialized_messages_len(messages: &[Value]) -> usize {
    serde_json::to_vec(messages)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
}

fn group_chat_messages(messages: Vec<Value>) -> Vec<Vec<Value>> {
    let mut groups = Vec::new();
    let mut idx = 0usize;
    while idx < messages.len() {
        let mut group = vec![messages[idx].clone()];
        let is_assistant_tool_call = messages[idx].get("role").and_then(|v| v.as_str())
            == Some("assistant")
            && messages[idx]
                .get("tool_calls")
                .and_then(|v| v.as_array())
                .is_some_and(|calls| !calls.is_empty());
        idx += 1;
        if is_assistant_tool_call {
            while idx < messages.len()
                && messages[idx].get("role").and_then(|v| v.as_str()) == Some("tool")
            {
                group.push(messages[idx].clone());
                idx += 1;
            }
        }
        groups.push(group);
    }
    groups
}

fn compact_group_for_budget(group: Vec<Value>) -> Vec<Value> {
    group.into_iter().map(compact_message_for_budget).collect()
}

fn compact_message_for_budget(mut message: Value) -> Value {
    if serialized_messages_len(&[message.clone()]) <= COMPACT_SINGLE_MESSAGE_MAX_CHARS {
        return message;
    }

    if let Some(calls) = message.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
        for call in calls {
            if let Some(args) = call
                .pointer_mut("/function/arguments")
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
            {
                if args.chars().count() > COMPACT_TOOL_ARGUMENTS_MAX_CHARS {
                    call["function"]["arguments"] = Value::String(shortened_text(
                        "Tool call arguments shortened for compact input",
                        &args,
                        COMPACT_TOOL_ARGUMENTS_MAX_CHARS,
                    ));
                }
            }
        }
    }

    if serialized_messages_len(&[message.clone()]) <= COMPACT_SINGLE_MESSAGE_MAX_CHARS {
        return message;
    }

    let role = message
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("message")
        .to_owned();
    let text = message_text(&message);
    if let Some(obj) = message.as_object_mut() {
        obj.insert(
            "content".to_owned(),
            Value::String(shortened_text(
                &format!("{role} message shortened for compact input"),
                &text,
                COMPACT_SINGLE_MESSAGE_MAX_CHARS,
            )),
        );
    }
    message
}

fn build_compact_omission_notice(
    omitted_groups: &[Vec<Value>],
    original_message_count: usize,
    original_bytes: usize,
) -> Value {
    let omitted_messages: usize = omitted_groups.iter().map(Vec::len).sum();
    let omitted_bytes: usize = omitted_groups
        .iter()
        .map(|group| serialized_messages_len(group))
        .sum();
    let mut notice = String::new();
    notice.push_str("[Compact input budget applied]\n");
    notice.push_str(
        "Older conversation blocks were omitted or shortened from this compact request so the compact request itself stays below the upstream context limit. Newest blocks and the summarization instructions were preserved.\n",
    );
    notice.push_str(&format!(
        "Original messages: {original_message_count}. Omitted messages: {omitted_messages}. Original chat messages JSON bytes: {original_bytes}. Omitted JSON bytes: {omitted_bytes}.\n"
    ));

    let user_excerpts = omitted_user_excerpts(omitted_groups, 12);
    if !user_excerpts.is_empty() {
        notice.push_str("Omitted user-message excerpts:\n");
        for excerpt in user_excerpts {
            notice.push_str("- ");
            notice.push_str(&excerpt);
            notice.push('\n');
        }
    }

    if notice.chars().count() > COMPACT_OMISSION_NOTICE_MAX_CHARS {
        notice = take_first_chars(&notice, COMPACT_OMISSION_NOTICE_MAX_CHARS);
        notice.push_str("\n[Omission notice truncated to compact budget.]");
    }

    json!({
        "role": "user",
        "content": notice,
    })
}

fn omitted_user_excerpts(groups: &[Vec<Value>], max: usize) -> Vec<String> {
    let mut excerpts = Vec::new();
    for message in groups.iter().flatten() {
        if message.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let text = message_text(message);
        if text.trim().is_empty() {
            continue;
        }
        excerpts.push(short_excerpt(&text, 500));
        if excerpts.len() >= max {
            break;
        }
    }
    excerpts
}

fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(text);
                }
            }
            if out.is_empty() {
                serde_json::to_string(parts).unwrap_or_default()
            } else {
                out
            }
        }
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
        None => serde_json::to_string(message).unwrap_or_default(),
    }
}

fn shortened_text(label: &str, text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let head = take_first_chars(text, COMPACT_EXCERPT_HEAD_CHARS.min(max_chars / 2));
    let tail = take_last_chars(text, COMPACT_EXCERPT_TAIL_CHARS.min(max_chars / 3));
    format!(
        "[{label}]\nOriginal size: {} chars.\n--- Begin head excerpt ---\n{}\n--- End head excerpt ---\n--- Begin tail excerpt ---\n{}\n--- End tail excerpt ---\n[Omitted middle content from compact request.]",
        text.chars().count(),
        head,
        tail
    )
}

fn short_excerpt(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        let mut excerpt = take_first_chars(&normalized, max_chars);
        excerpt.push_str("...");
        excerpt
    }
}

fn take_first_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn take_last_chars(value: &str, max: usize) -> String {
    let mut chars = value.chars().rev().take(max).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

/// 把上游 `/chat/completions` 的非流式 JSON 响应包装成 Codex CLI 期待的
/// compact response。
///
/// 当上游返回非 2xx 时,把它的 status + body 透传给客户端(让 Codex CLI
/// 拿到上游真实错误而不是被我们包成"假成功")。
pub(crate) fn build_compact_response_plan(
    upstream_status: StatusCode,
    mut upstream_headers: HeaderMap,
    upstream_stream: ByteStream,
) -> Result<ResponsePlan, AdapterError> {
    upstream_headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    upstream_headers.remove(http::header::CONTENT_LENGTH);
    upstream_headers.remove(http::header::TRANSFER_ENCODING);

    let stream_with_logic = Box::pin(futures_util::stream::once(async move {
        let body = collect_and_wrap_compact_body(upstream_status, upstream_stream)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        Ok::<Bytes, std::io::Error>(Bytes::from(body))
    }));

    Ok(ResponsePlan {
        status: if upstream_status.is_success() {
            StatusCode::OK
        } else {
            upstream_status
        },
        headers: upstream_headers,
        stream: stream_with_logic,
    })
}

async fn collect_and_wrap_compact_body(
    upstream_status: StatusCode,
    mut upstream_stream: ByteStream,
) -> Result<Vec<u8>, AdapterError> {
    let mut buf = Vec::new();
    while let Some(chunk) = upstream_stream.next().await {
        let bytes = chunk.map_err(|e| AdapterError::Internal(format!("upstream io: {e}")))?;
        if buf.len() + bytes.len() > MAX_UPSTREAM_RESPONSE_BYTES {
            return Err(AdapterError::Internal(format!(
                "compact upstream response > {MAX_UPSTREAM_RESPONSE_BYTES} bytes"
            )));
        }
        buf.extend_from_slice(&bytes);
    }

    if !upstream_status.is_success() {
        // 上游错误:body 可能是 HTML/JSON/纯文本,无脑透传给客户端
        // (Codex CLI 收到非 2xx 会显示原始 body)。
        return Ok(buf);
    }

    let parsed: Value = serde_json::from_slice(&buf).map_err(|e| {
        let preview: String = String::from_utf8_lossy(&buf).chars().take(500).collect();
        AdapterError::Internal(format!(
            "compact upstream non-JSON response: {e}; first 500 chars: {preview}"
        ))
    })?;
    let raw = parsed
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AdapterError::Internal("compact upstream missing choices[0].message.content".to_owned())
        })?;

    compact_response_body_from_summary_text(raw)
}

pub(crate) fn compact_response_body_from_summary_text(raw: &str) -> Result<Vec<u8>, AdapterError> {
    // B1:抽 `<summary>...</summary>` tag 内容(配合 v2.0.12 prompt 强制
    // `<analysis>` + `<summary>` 二段输出),无 tag 时容错回退原文。
    // 不抽 analysis 部分,避免污染下一轮 history(模型 chain-of-thought
    // 的 meta-discussion 进 history 后会让续轮模型被带偏)。
    let summary = extract_summary_section(raw).trim().to_owned();

    let encrypted_content = format!("{COMPACT_SUMMARY_PREFIX}\n{summary}");
    let compact_response = json!({
        "output": [{
            "type": "compaction",
            "encrypted_content": encrypted_content,
        }]
    });
    serde_json::to_vec(&compact_response)
        .map_err(|e| AdapterError::Internal(format!("serialize compact response: {e}")))
}

/// 从模型输出中抽 `<summary>...</summary>` 段落 — 配合 v2.0.12 prompt 强制
/// `<analysis>` + `<summary>` 二段输出。容错策略:
///
/// - 找到第一个 `<summary>` 和最后一个 `</summary>`,返回中间内容
/// - 无 `<summary>` tag → 返回 raw(可能模型没遵守格式,先用着,日志会
///   反映 summary 质量)
/// - 有 `<summary>` 无 `</summary>`(模型截断) → 返回 `<summary>` 之后所有文本
fn extract_summary_section(raw: &str) -> &str {
    let Some(start) = raw.find("<summary>") else {
        return raw;
    };
    let after = &raw[start + "<summary>".len()..];
    if let Some(end) = after.rfind("</summary>") {
        &after[..end]
    } else {
        after
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use codex_app_transfer_registry::Provider;
    use futures_util::stream;
    use serde_json::json;

    fn make_provider() -> Provider {
        let mut p = Provider {
            id: "mimo".into(),
            name: "MiMo".into(),
            base_url: "https://example.com/v1".into(),
            auth_scheme: "bearer".into(),
            api_format: "responses".into(),
            api_key: String::new(),
            models: Default::default(),
            extra_headers: Default::default(),
            model_capabilities: Default::default(),
            request_options: Default::default(),
            is_builtin: false,
            sort_index: 0,
            extra: Default::default(),
        };
        p.models.insert("default".into(), "mimo-v2.5".into());
        p
    }

    #[test]
    fn is_compact_path_recognizes_v1_and_bare_forms() {
        assert!(is_compact_path("/responses/compact"));
        assert!(is_compact_path("/v1/responses/compact"));
        assert!(is_compact_path("/openai/v1/responses/compact"));
        assert!(is_compact_path("/responses/compact?foo=bar"));
        assert!(is_compact_path("/responses/compact/"));
        // 负向
        assert!(!is_compact_path("/responses"));
        assert!(!is_compact_path("/responses/compact/extra"));
        assert!(!is_compact_path("/chat/completions"));
    }

    #[test]
    fn build_compact_chat_request_passes_through_reasoning_field_for_thinking_repair() {
        // Kimi/DeepSeek 等 thinking 模式 provider 要求历史里的 assistant
        // tool_call message 必带 reasoning_content。`ensure_thinking_tool_call_reasoning`
        // 通过 body.reasoning 字段判断是否启用 thinking。compact 路径合成的
        // synthetic body **必须**透传原 reasoning,否则 thinking 模式上游
        // 会 400 "thinking is enabled but reasoning_content is missing"。
        let p = make_provider();
        let body = json!({
            "model": "kimi-for-coding",
            "input": [
                {"type": "function_call", "call_id": "c1", "name": "shell", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "ok"},
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "next"}
                ]}
            ],
            "reasoning": {"effort": "high"},
            "tools": [{"type": "function", "name": "shell"}]
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        // 找到 function_call 转出来的 assistant message,必须带 reasoning_content
        let assistant_with_tool_calls = messages
            .iter()
            .find(|m| {
                m["role"] == "assistant" && m.get("tool_calls").and_then(|v| v.as_array()).is_some()
            })
            .expect("应有一条 assistant + tool_calls(从 function_call 转换而来)");
        // ensure_thinking_tool_call_reasoning 在缺真实 reasoning 时塞 " "(单空格占位)
        // 这就是 Kimi/DeepSeek 上游接受的兜底值,字段存在即可,不做非空断言。
        assert!(
            assistant_with_tool_calls
                .get("reasoning_content")
                .and_then(|v| v.as_str())
                .is_some(),
            "thinking 启用时 assistant tool_call 必须带 reasoning_content 字段(可以是单空格占位)"
        );
    }

    #[test]
    fn build_compact_chat_request_bounds_large_tool_output_before_prompt() {
        let p = make_provider();
        let huge_line = "const minified='x';".repeat(3_000);
        let raw_output = format!(
            "Chunk ID: 44d863\n\
             Wall time: 0.1540 seconds\n\
             Process exited with code 0\n\
             Original token count: 924828\n\
             Output:\n\
             Total output lines: 18\n\n\
             /tmp/codex-asar/webview/assets/plugins-page-selectors.js:{huge_line}"
        );
        let body = json!({
            "model": "kimi-for-coding",
            "input": [
                {"type": "function_call", "call_id": "tool_large", "name": "exec_command", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "tool_large", "output": raw_output}
            ]
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        let tool_msg = messages
            .iter()
            .find(|m| m["role"] == "tool")
            .expect("compact 请求中应保留 bounded tool message");
        let content = tool_msg["content"].as_str().unwrap();

        assert_eq!(tool_msg["tool_call_id"], "tool_large");
        assert!(content.contains("[Tool output stored outside model context]"));
        assert!(content.contains("Artifact ID: tool_artifact_"));
        assert!(content.contains("Original token count: 924828"));
        assert!(
            content.len() < 20_000,
            "compact 前 tool.content 应被有界化,实际长度 {}",
            content.len()
        );
        assert!(
            messages
                .last()
                .and_then(|m| m.get("content"))
                .and_then(|v| v.as_str())
                .is_some_and(|text| text
                    .contains("Your task is to create a detailed CONTEXT CHECKPOINT summary")),
            "compact summary prompt 仍应作为最后一条 user message 注入"
        );
    }

    #[test]
    fn build_compact_chat_request_prunes_chat_messages_to_compact_budget() {
        let p = make_provider();
        let old_huge = "old research detail ".repeat(10_000);
        let recent = "recent user instruction that must remain visible";
        let body = json!({
            "model": "kimi-for-coding",
            "input": [
                {"type": "message", "role": "user", "content": old_huge},
                {"type": "message", "role": "assistant", "content": "ack"},
                {"type": "message", "role": "user", "content": recent}
            ]
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        let messages_bytes = serde_json::to_vec(messages).unwrap().len();

        assert!(
            messages_bytes <= COMPACT_CHAT_MESSAGES_MAX_BYTES,
            "compact messages must be budgeted before upstream request; actual={messages_bytes}"
        );
        assert!(
            messages.iter().any(|m| {
                m["role"] == "user"
                    && m["content"]
                        .as_str()
                        .unwrap_or("")
                        .contains("[Compact input budget applied]")
            }),
            "budget pruning must be explicit, not silent"
        );
        assert!(
            messages.iter().any(|m| {
                m["role"] == "user" && m["content"].as_str().unwrap_or("").contains(recent)
            }),
            "recent user message should be retained"
        );
        assert!(
            messages
                .last()
                .and_then(|m| m.get("content"))
                .and_then(|v| v.as_str())
                .is_some_and(|text| text.contains("CONTEXT CHECKPOINT")),
            "summarization prompt must remain the last message"
        );
    }

    #[test]
    fn build_compact_chat_request_keeps_tail_tool_chain_together_after_pruning() {
        let p = make_provider();
        let old_huge = "old context ".repeat(10_000);
        let body = json!({
            "model": "kimi-for-coding",
            "input": [
                {"type": "message", "role": "user", "content": old_huge},
                {"type": "function_call", "call_id": "tail_tool", "name": "shell", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "tail_tool", "output": "short result"},
                {"type": "message", "role": "user", "content": "continue from the tool result"}
            ],
            "tools": [{"type": "function", "name": "shell"}]
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();

        let assistant_idx = messages
            .iter()
            .position(|m| {
                m["role"] == "assistant"
                    && m.get("tool_calls")
                        .and_then(|v| v.as_array())
                        .is_some_and(|calls| calls.iter().any(|call| call["id"] == "tail_tool"))
            })
            .expect("tail assistant tool call should be retained");
        let tool_msg = messages
            .get(assistant_idx + 1)
            .expect("tool response should immediately follow assistant tool call");
        assert_eq!(tool_msg["role"], "tool");
        assert_eq!(tool_msg["tool_call_id"], "tail_tool");
    }

    #[test]
    fn build_compact_chat_request_injects_prompt_as_last_user_message() {
        // v2.0.12 调整:SUMMARIZATION_PROMPT 注入成**最后一条 user message**
        // (不是 system),对齐 Codex CLI 自家做法,提升第三方 provider 服从度。
        let p = make_provider();
        let body = json!({
            "model": "mimo-v2.5",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "hello"}
                ]},
                {"type": "message", "role": "assistant", "content": [
                    {"type": "output_text", "text": "world"}
                ]},
            ],
            "instructions": "ORIGINAL_PROJECT_INSTRUCTIONS",
            "tools": [{"type": "function", "name": "shell"}],
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();

        // 最后一条 message 必须是 user + 包含 SUMMARIZATION_PROMPT 关键字
        let last = messages.last().expect("non-empty messages");
        assert_eq!(last["role"], "user", "prompt 必须注入成 user message");
        let last_content = last["content"].as_str().unwrap_or_else(|| {
            // content 也可能是 array(取决于 provider 转换路径)
            last["content"]
                .as_array()
                .and_then(|a| {
                    a.iter()
                        .find_map(|b| b.get("text").and_then(|v| v.as_str()))
                })
                .unwrap_or_default()
        });
        assert!(
            last_content.contains("CONTEXT CHECKPOINT"),
            "last user message 必须含 SUMMARIZATION_PROMPT 关键字 'CONTEXT CHECKPOINT',实际:{last_content}"
        );
        assert!(
            last_content.contains("All User Messages"),
            "9-section schema 必须含 'All User Messages' 段名"
        );
        assert!(
            last_content.contains("<analysis>") && last_content.contains("<summary>"),
            "二段输出格式必须出现在 prompt 里"
        );

        // 原 instructions **不应**进 system/任何 message(摘要任务不受原任务 system 影响)
        assert!(
            !messages.iter().any(|m| m["content"]
                .as_str()
                .unwrap_or("")
                .contains("ORIGINAL_PROJECT_INSTRUCTIONS")),
            "原 instructions 应被丢掉,不应进 messages"
        );
        // 没有 system message(prompt 改 user message 后)
        assert!(
            !messages.iter().any(|m| m["role"] == "system"),
            "compact 请求不应再产生 system message,实际 messages 角色:{:?}",
            messages
                .iter()
                .map(|m| m["role"].clone())
                .collect::<Vec<_>>()
        );
        // 历史 user / assistant 保留
        assert!(messages
            .iter()
            .any(|m| m["role"] == "user" && m["content"].as_str().unwrap_or("").contains("hello")));
        assert!(messages
            .iter()
            .any(|m| m["role"] == "assistant"
                && m["content"].as_str().unwrap_or("").contains("world")));
        // stream 字段不带(false 在 chat body 转换里会被丢)
        assert!(parsed.get("stream").is_none() || parsed["stream"] == false);
    }

    #[test]
    fn build_compact_chat_request_injects_prompt_when_input_is_string() {
        // 关键回归(2026-05-08 codex review P2):input 不一定是 array,
        // 也可能是 string / object / null / 缺失。**所有形式都必须确保 prompt
        // 被注入**,否则上游收到无 summary 指令的请求,返回任意 chat 内容。
        let p = make_provider();
        let body = json!({
            "model": "mimo-v2.5",
            "input": "raw user prompt as plain string",
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        let last = messages.last().expect("messages 非空");
        let last_text = last["content"].as_str().unwrap_or_default();
        assert!(
            last_text.contains("CONTEXT CHECKPOINT"),
            "string input 路径下 prompt 必须仍被注入,实际 last:{last:?}"
        );
        // 原 string input 也应保留为前一条 user message
        assert!(messages.iter().any(|m| {
            m["role"] == "user"
                && m["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("raw user prompt as plain string")
        }));
    }

    #[test]
    fn build_compact_chat_request_injects_prompt_when_input_is_object() {
        // input 是单个 object item(非典型但合法),prompt 必须注入
        let p = make_provider();
        let body = json!({
            "model": "mimo-v2.5",
            "input": {"type": "message", "role": "user", "content": "single obj"},
        });
        let bytes = serde_json::to_vec(&body).unwrap();
        let chat = build_compact_chat_request(&bytes, &p).unwrap();
        let parsed: Value = serde_json::from_slice(&chat).unwrap();
        let messages = parsed["messages"].as_array().unwrap();
        let last = messages.last().unwrap();
        assert!(
            last["content"]
                .as_str()
                .unwrap_or("")
                .contains("CONTEXT CHECKPOINT"),
            "object input 路径下 prompt 必须仍被注入"
        );
    }

    #[test]
    fn build_compact_chat_request_injects_prompt_when_input_is_null_or_missing() {
        let p = make_provider();
        for body in [
            json!({"model": "mimo-v2.5"}),
            json!({"model": "mimo-v2.5", "input": null}),
            json!({"model": "mimo-v2.5", "input": []}),
            json!({"model": "mimo-v2.5", "input": ""}),
        ] {
            let bytes = serde_json::to_vec(&body).unwrap();
            let chat = build_compact_chat_request(&bytes, &p).unwrap();
            let parsed: Value = serde_json::from_slice(&chat).unwrap();
            let messages = parsed["messages"].as_array().unwrap();
            let last = messages.last().expect("messages 必非空(prompt 至少一条)");
            assert!(
                last["content"]
                    .as_str()
                    .unwrap_or("")
                    .contains("CONTEXT CHECKPOINT"),
                "null/empty input 时 prompt 也必须注入,实际 body={body:?},last={last:?}"
            );
        }
    }

    // ── extract_summary_section ──────────────────────────────────────

    #[test]
    fn extract_summary_section_strips_analysis_and_keeps_summary() {
        let raw = "<analysis>\nblah blah meta\n</analysis>\n<summary>\nactual summary content\n</summary>";
        assert_eq!(
            extract_summary_section(raw).trim(),
            "actual summary content"
        );
    }

    #[test]
    fn extract_summary_section_handles_summary_only_no_analysis() {
        let raw = "<summary>\njust a summary\n</summary>";
        assert_eq!(extract_summary_section(raw).trim(), "just a summary");
    }

    #[test]
    fn extract_summary_section_returns_raw_when_no_tag() {
        // 模型没遵守格式 → 整段保留(总比丢好,日志会反映质量)
        let raw = "this is plain text without any tags";
        assert_eq!(extract_summary_section(raw), raw);
    }

    #[test]
    fn extract_summary_section_handles_truncated_close_tag() {
        // 模型输出超 max_tokens 被截断,只有 <summary> 没 </summary>
        let raw = "<analysis>meta</analysis><summary>\npartial summary content cut off here";
        assert_eq!(
            extract_summary_section(raw).trim(),
            "partial summary content cut off here"
        );
    }

    #[test]
    fn extract_summary_section_picks_outermost_when_nested_or_repeated() {
        // 防御:few-shot example 里也有 `<summary>` 被模型直接 echo 出来时
        // 用 first-open + last-close,取最外层。
        let raw = "<summary>real <summary>nested example</summary> content</summary>";
        assert_eq!(
            extract_summary_section(raw).trim(),
            "real <summary>nested example</summary> content"
        );
    }

    fn one_chunk_stream(bytes: Vec<u8>) -> ByteStream {
        Box::pin(stream::once(async move {
            Ok::<Bytes, std::io::Error>(Bytes::from(bytes))
        }))
    }

    #[tokio::test]
    async fn collect_and_wrap_extracts_summary_into_compaction_item() {
        let upstream_body = serde_json::to_vec(&json!({
            "id": "chatcmpl_x",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "summary text body"},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        }))
        .unwrap();

        let body = collect_and_wrap_compact_body(StatusCode::OK, one_chunk_stream(upstream_body))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        let output = parsed["output"].as_array().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["type"], "compaction");
        let enc = output[0]["encrypted_content"].as_str().unwrap();
        assert!(
            enc.starts_with(COMPACT_SUMMARY_PREFIX),
            "encrypted_content 必须以 SUMMARY_PREFIX 开头(Codex CLI 用它识别 summary)"
        );
        assert!(enc.ends_with("summary text body"));
    }

    #[tokio::test]
    async fn collect_and_wrap_strips_analysis_keeps_only_summary_in_encrypted_content() {
        // v2.0.12 关键回归:上游模型按 prompt 输出 `<analysis>` + `<summary>`,
        // 我们必须只把 `<summary>` 段塞进 encrypted_content,不能把 analysis
        // chain-of-thought 也塞进下一轮 history(会污染续轮模型注意力)。
        let model_output = "<analysis>\n\
            User asked X, I did Y, then user corrected to Z.\n\
            </analysis>\n\
            <summary>\n\
            1. Primary Request: do Z\n\
            2. Files: /abs/foo.rs\n\
            6. All User Messages:\n\
            - \"do X\"\n\
            - \"actually do Z\"\n\
            </summary>";
        let upstream_body = serde_json::to_vec(&json!({
            "choices": [{
                "message": {"role": "assistant", "content": model_output},
                "finish_reason": "stop"
            }]
        }))
        .unwrap();

        let body = collect_and_wrap_compact_body(StatusCode::OK, one_chunk_stream(upstream_body))
            .await
            .unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        let enc = parsed["output"][0]["encrypted_content"].as_str().unwrap();
        assert!(enc.starts_with(COMPACT_SUMMARY_PREFIX));
        // 只保留 summary 部分
        assert!(
            !enc.contains("<analysis>") && !enc.contains("</analysis>"),
            "analysis tag 不应进 encrypted_content"
        );
        assert!(
            !enc.contains("User asked X, I did Y"),
            "analysis chain-of-thought 内容不应被保留"
        );
        // summary 内容保留
        assert!(enc.contains("Primary Request: do Z"));
        assert!(enc.contains("All User Messages"));
        assert!(enc.contains("\"actually do Z\""));
    }

    #[tokio::test]
    async fn collect_and_wrap_chunked_upstream_response() {
        // 上游分多 chunk 来,我们应该正确拼接后解析
        let upstream_body = serde_json::to_vec(&json!({
            "choices": [{"message": {"content": "chunked summary"}, "finish_reason": "stop"}]
        }))
        .unwrap();
        let mid = upstream_body.len() / 2;
        let part1 = upstream_body[..mid].to_vec();
        let part2 = upstream_body[mid..].to_vec();
        let s: ByteStream = Box::pin(stream::iter(vec![
            Ok(Bytes::from(part1)),
            Ok(Bytes::from(part2)),
        ]));
        let body = collect_and_wrap_compact_body(StatusCode::OK, s)
            .await
            .unwrap();
        let parsed: Value = serde_json::from_slice(&body).unwrap();
        assert!(parsed["output"][0]["encrypted_content"]
            .as_str()
            .unwrap()
            .ends_with("chunked summary"));
    }

    #[tokio::test]
    async fn collect_and_wrap_passes_through_upstream_error_body() {
        // 上游 4xx/5xx 时直接透传 body,让 Codex CLI 看到真实错误
        let body = collect_and_wrap_compact_body(
            StatusCode::BAD_REQUEST,
            one_chunk_stream(b"<html>upstream rate limit</html>".to_vec()),
        )
        .await
        .unwrap();
        assert_eq!(body, b"<html>upstream rate limit</html>");
    }

    #[tokio::test]
    async fn collect_and_wrap_rejects_oversized_response() {
        let huge: Vec<u8> = vec![0; MAX_UPSTREAM_RESPONSE_BYTES + 1];
        let err = collect_and_wrap_compact_body(StatusCode::OK, one_chunk_stream(huge))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("> "));
    }

    #[tokio::test]
    async fn collect_and_wrap_errors_on_missing_message_content() {
        let upstream_body =
            serde_json::to_vec(&json!({"choices": [{"finish_reason": "stop"}]})).unwrap();
        let err = collect_and_wrap_compact_body(StatusCode::OK, one_chunk_stream(upstream_body))
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("missing choices[0].message.content"));
    }
}
