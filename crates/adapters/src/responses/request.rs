//! Stage 3.2a · Responses body → Chat body 转换.
//!
//! 对应 Python 端 `backend/responses_adapter.py::convert_responses_to_chat_body`,
//! 并恢复旧版 `ResponseSessionCache` 的 `previous_response_id` 历史拼接。
//!
//! 覆盖范围:
//! - 顶层字段:`model` / `instructions` / `input` / `tools` / `tool_choice` /
//!   `max_output_tokens` → `max_tokens` / `stream` / `temperature` / `top_p` /
//!   `seed` / `stop` / `parallel_tool_calls` / `frequency_penalty` /
//!   `presence_penalty` / `user`
//! - input items:`message`(role + content)/ `function_call` /
//!   `function_call_output` / `input_image` / `input_file` / `input_audio` /
//!   `input_video`
//! - tools:`type=function` 与 `type=custom`(custom 降级为接受单字符串
//!   `input` 的 function)
//! - `text.format` → `response_format` / `reasoning` → `reasoning_effort`
//! - `store` / `metadata` / `prediction` / `service_tier` / `modalities` /
//!   `audio`
//! - 多轮 user/assistant 合并

use codex_app_transfer_registry::Provider;
use serde_json::{json, Map, Value};

use super::session::ResponseSessionCache;
use crate::core::input::{merge_messages_with_previous_response, response_id_for_session};
use crate::types::{AdapterError, ResponseSessionPlan};

#[derive(Debug, Clone)]
pub struct ResponsesBodyConversion {
    pub body: Value,
    pub response_session: ResponseSessionPlan,
}

const TOOL_OUTPUT_INLINE_MAX_CHARS: usize = 4_000;
const TOOL_OUTPUT_HEAD_CHARS: usize = 1_200;
const TOOL_OUTPUT_TAIL_CHARS: usize = 1_200;
const TOOL_OUTPUT_VISIBLE_MAX_CHARS: usize = 5_000;

/// 把 Responses API 请求体转换成 OpenAI Chat Completions 请求体.
pub fn responses_body_to_chat_body(input: &Value) -> Result<Value, AdapterError> {
    responses_body_to_chat_body_for_provider(input, None)
}

/// 把 Responses API 请求体转换成 OpenAI Chat Completions 请求体.
///
/// provider-aware 路径用于恢复 Python 版 DeepSeek/Kimi thinking 历史修复:
/// Codex 续轮工具调用时,部分上游会要求 assistant.tool_calls 历史带
/// `reasoning_content`;DeepSeek 的 thinking 还可能由 provider.requestOptions
/// 开启,而不是出现在本次请求体里。
pub fn responses_body_to_chat_body_for_provider(
    input: &Value,
    provider: Option<&Provider>,
) -> Result<Value, AdapterError> {
    Ok(responses_body_to_chat_body_for_provider_with_session(input, provider, None)?.body)
}

pub fn responses_body_to_chat_body_for_provider_with_session(
    input: &Value,
    provider: Option<&Provider>,
    session_cache: Option<&ResponseSessionCache>,
) -> Result<ResponsesBodyConversion, AdapterError> {
    let body = input
        .as_object()
        .ok_or_else(|| AdapterError::BadRequest("body 必须是 JSON 对象".into()))?;

    let mut result = serde_json::Map::new();

    // model
    if let Some(m) = body.get("model") {
        result.insert("model".into(), m.clone());
    }

    // messages: instructions(优先,作为 system 头) + input 展开;如果存在
    // previous_response_id 且 session cache 命中,先恢复历史再追加本轮 input。
    // **cache miss + input 空** → build_messages_from_input 返回
    // PreviousResponseNotFound,proxy 层 IntoResponse 会转成标准 OpenAI 400
    // (`code: "previous_response_not_found"`)让 Codex CLI fail-fast。
    let mut messages = build_messages_from_input(input, session_cache)?;
    messages = merge_consecutive_user_messages(messages);
    messages = merge_consecutive_assistant_messages(messages);
    repair_tool_call_ids(
        &mut messages,
        super::tool_call_cache::global_tool_call_cache(),
    );
    ensure_thinking_tool_call_reasoning(&mut messages, input, provider);
    convert_developer_to_system_if_needed(&mut messages, provider);

    // 视觉剥离:对已知不支持视觉的上游(deepseek-v4-* / moonshot-v1-* 非
    // vision-preview / mimo-v2-pro / mimo-v2.5-pro 等纯文本模型),把
    // messages.content 里所有 `image_url` block 替换为占位文本块。
    // **必须**做这一步:DeepSeek API 在 deserialize 阶段就对 `image_url`
    // content variant 报 400(实测 messages[8]: unknown variant `image_url`,
    // expected `text`),Codex CLI 历史里只要存在过一次图片(即使发给 vision
    // provider 后切换到 DeepSeek)就会让续轮全部失败。
    //
    // 用 body 里的实际 model(prepare_request 路径上,model 已被 forward.rs
    // 重写成 upstream 真实 model id),而不是 provider.models["default"] —
    // 因为 Codex CLI 经过 alias 映射,实际请求的 model 未必是 default。
    let body_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(codex_app_transfer_registry::strip_internal_model_suffix);
    if !provider_supports_vision(provider, body_model.as_deref()) {
        strip_image_blocks_in_place(&mut messages);
        // Stage 3 note: Additional input_image stripping from raw Responses input
        // (for Computer Use base64) can be added here once we have &mut access to body.
        // Current defense relies on strip_image_blocks_in_place after input→message conversion.
    } else {
        // 含 image_url 但无 text part 时补一个空格 text part — MiMo 多模态
        // 接口强制要求(否则 400 "Param Incorrect: text is not set"),
        // 对其他 supports_vision provider 无副作用,统一处理。
        ensure_text_part_when_image_present(&mut messages);
    }

    // 历史定位(2026-05-06 → 2026-05-08):
    // - 早期:cache miss + input 空 → 代理层主动 BadRequest 拒绝
    // - 中期:改为放行 messages:[] 给上游,期望 Codex 重试 4xx
    // - 实测:Codex CLI `codex-rs/codex-client/src/retry.rs` 对 400 fail-fast,
    //   只对 5xx 与 transport timeout 重试 → 放行后上游 19s+ 才 400,且 Codex
    //   无法重置 session,延迟分钟级
    // 现行(2026-05-08+):cache miss + input 空 → 上层 build_messages_from_input
    // 返回 `PreviousResponseNotFound`,proxy IntoResponse 转标准 OpenAI 400
    // (`code: "previous_response_not_found"`),与 OpenAI 服务端真实行为对齐。
    // 此处不再有"messages 为空"分支:进到这里 messages 必非空。
    let session_messages = messages.clone();
    result.insert("messages".into(), Value::Array(messages));

    // tools(function / custom 直接处理,namespace 递归展平,web_search /
    // web_search_preview per-provider 适配上游真支持的形态,其余 Responses
    // 专属类型 drop + warn_once)
    if let Some(Value::Array(tools)) = body.get("tools") {
        // Stage 2: Filter out computer_use_preview for non-vision models early
        let filtered_tools: Vec<&Value> =
            if !provider_supports_vision(provider, body_model.as_deref()) {
                let had_computer_use = tools.iter().any(|t| {
                    t.get("type").and_then(|v| v.as_str()) == Some("computer_use_preview")
                });

                if had_computer_use {
                    tracing::warn!(
                        target: "codex_app_transfer::computer_use",
                        provider = ?provider.map(|p| p.id.as_str()),
                        model = body_model,
                        "Codex CLI attempted to use computer_use_preview with a non-vision model. \
                         The tool has been dropped to prevent sending expensive base64 screenshots."
                    );
                }

                tools
                    .iter()
                    .filter(|t| {
                        t.get("type").and_then(|v| v.as_str()) != Some("computer_use_preview")
                    })
                    .collect()
            } else {
                tools.iter().collect()
            };

        let chat_tools: Vec<Value> = filtered_tools
            .iter()
            .flat_map(|t| convert_responses_tool_to_chat_tool(t, provider))
            .collect();
        if !chat_tools.is_empty() {
            // **Kimi `$web_search` 强制 thinking disabled**:Kimi 官方文档
            // (`platform.kimi.ai/docs/guide/use-web-search`)明确写
            // "When using `$web_search` function, you must disable the thinking
            // ability of the model"。OpenAI SDK 的 `extra_body.thinking.type=
            // "disabled"` 在 wire 上等价于 request body 顶级 `thinking:
            // {type:"disabled"}` 字段。如果 outbound tools 含 Kimi 内置
            // `$web_search`,代理在这里强制注入(用户启用 web_search 时模型
            // thinking 能力被禁用是 Kimi API 限制,UI 后续会加提示)。
            if contains_kimi_web_search_tool(&chat_tools) {
                result.insert("thinking".into(), serde_json::json!({"type": "disabled"}));
            }
            result.insert("tools".into(), Value::Array(chat_tools));
        }
    }

    // tool_choice 规范化
    if let Some(tc) = body.get("tool_choice") {
        result.insert("tool_choice".into(), normalize_tool_choice(tc));
    }

    // text.format → response_format
    // 注:对已知不支持 `json_schema` 的上游(DeepSeek 实测 2026-05-06)会自动
    // 降级为 `{"type":"json_object"}`,Codex CLI 的 system prompt 通常已写明
    // required keys,模型仍会输出符合 schema 的 JSON。Kimi / MiMo 实测都支持
    // json_schema,不在降级名单。
    if let Some(text_config) = body.get("text") {
        if let Some(response_format) = build_response_format_for_provider(text_config, provider) {
            result.insert("response_format".into(), response_format);
        }
    }

    // reasoning → reasoning_effort
    if let Some(reasoning_effort) = body.get("reasoning").and_then(build_reasoning_effort) {
        result.insert("reasoning_effort".into(), reasoning_effort);
    }

    // max_output_tokens → max_tokens
    if let Some(v) = body.get("max_output_tokens") {
        result.insert("max_tokens".into(), v.clone());
    }

    // 特殊参数处理(store / metadata / prediction / service_tier / modalities / audio)
    if let Some(v) = body.get("store").and_then(handle_store_param) {
        result.insert("store".into(), v);
    }
    if let Some(v) = body.get("metadata").and_then(handle_metadata_param) {
        result.insert("metadata".into(), v);
    }
    if let Some(v) = body.get("prediction").and_then(handle_prediction_param) {
        result.insert("prediction".into(), v);
    }
    if let Some(v) = body.get("service_tier").and_then(handle_service_tier) {
        result.insert("service_tier".into(), v);
    }
    if let Some(v) = body.get("modalities").and_then(handle_modalities) {
        result.insert("modalities".into(), v);
    }
    if let Some(v) = body.get("audio").and_then(handle_audio_param) {
        result.insert("audio".into(), v);
    }

    // 透传白名单(已被处理过的不重复)
    const PASSTHROUGH: &[&str] = &[
        "temperature",
        "top_p",
        "seed",
        "stop",
        "logit_bias",
        "parallel_tool_calls",
        "frequency_penalty",
        "presence_penalty",
        "user",
        "n",
        "logprobs",
        "top_logprobs",
        "response_format",
        "reasoning_effort",
        "max_completion_tokens",
        "safety_identifier",
        "safety_settings",
        "context",
        "truncate",
        "prompt_truncation",
        "extra_headers",
        "extra_query",
        "extra_body",
        "timeout",
    ];
    for key in PASSTHROUGH {
        if let Some(v) = body.get(*key) {
            result.entry((*key).to_owned()).or_insert_with(|| v.clone());
        }
    }

    // stream + stream_options.include_usage
    let stream = body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    result.insert("stream".into(), Value::Bool(stream));
    if stream {
        result.insert("stream_options".into(), json!({ "include_usage": true }));
    }

    sanitize_chat_body_for_provider(&mut result, provider);

    Ok(ResponsesBodyConversion {
        body: Value::Object(result),
        response_session: ResponseSessionPlan {
            response_id: response_id_for_session(),
            messages: session_messages,
        },
    })
}

fn build_messages_from_input(
    body: &Value,
    session_cache: Option<&ResponseSessionCache>,
) -> Result<Vec<Value>, AdapterError> {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(msg) = body
        .get("instructions")
        .and_then(build_instructions_message)
    {
        messages.push(msg);
    }

    let current_messages = body
        .get("input")
        .map(input_field_to_messages)
        .unwrap_or_default();
    messages.extend(current_messages);
    merge_messages_with_previous_response(messages, body, session_cache)
}

fn build_instructions_message(instructions: &Value) -> Option<Value> {
    match instructions {
        Value::Null => None,
        Value::String(s) => {
            if s.trim().is_empty() {
                None
            } else {
                Some(json!({ "role": "system", "content": s }))
            }
        }
        Value::Object(obj) => {
            if let Some(text) = obj
                .get("text")
                .or_else(|| obj.get("content"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            {
                return Some(json!({ "role": "system", "content": text }));
            }
            Some(json!({
                "role": "system",
                "content": serde_json::to_string(instructions).unwrap_or_else(|_| instructions.to_string()),
            }))
        }
        other => {
            let content = value_to_chat_string(other);
            if content.trim().is_empty() {
                None
            } else {
                Some(json!({ "role": "system", "content": content }))
            }
        }
    }
}

/// 把 `body.input` 字段(可能是 string 也可能是 array)展开成 messages 列表.
fn input_field_to_messages(input: &Value) -> Vec<Value> {
    let items = extract_input_items(input);
    let mut out = Vec::new();
    let mut pending_reasoning: Option<String> = None;

    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        if obj.get("type").and_then(|v| v.as_str()) == Some("reasoning") {
            pending_reasoning = Some(extract_reasoning_text(obj));
            continue;
        }
        let mut item_messages = input_item_to_messages(obj);
        for msg in &mut item_messages {
            if msg.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                if let Some(reasoning) = pending_reasoning.take() {
                    let has_reasoning = msg
                        .get("reasoning_content")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.trim().is_empty());
                    if !has_reasoning {
                        let repaired = if reasoning.trim().is_empty() {
                            " ".to_owned()
                        } else {
                            reasoning
                        };
                        if let Some(msg_obj) = msg.as_object_mut() {
                            msg_obj.insert("reasoning_content".into(), Value::String(repaired));
                        }
                    }
                }
            } else {
                pending_reasoning = None;
            }
        }
        out.extend(item_messages);
    }

    out
}

fn extract_input_items(input: &Value) -> Vec<Value> {
    match input {
        Value::Null => Vec::new(),
        Value::String(s) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                vec![json!({ "type": "message", "role": "user", "content": s })]
            }
        }
        Value::Object(obj) => {
            if obj.contains_key("type") {
                vec![Value::Object(obj.clone())]
            } else {
                vec![json!({
                    "type": "message",
                    "role": obj.get("role").and_then(|v| v.as_str()).unwrap_or("user"),
                    "content": obj.get("content").cloned().unwrap_or_else(|| Value::Object(obj.clone())),
                })]
            }
        }
        Value::Array(items) => items
            .iter()
            .filter_map(|item| match item {
                Value::Object(obj) if obj.contains_key("type") => Some(Value::Object(obj.clone())),
                Value::Object(obj) => Some(json!({
                    "type": "message",
                    "role": obj.get("role").and_then(|v| v.as_str()).unwrap_or("user"),
                    "content": obj.get("content").cloned().unwrap_or_else(|| Value::Object(obj.clone())),
                })),
                Value::String(s) => Some(json!({ "type": "message", "role": "user", "content": s })),
                other => Some(json!({ "type": "message", "role": "user", "content": value_to_chat_string(other) })),
            })
            .collect(),
        other => vec![json!({ "type": "message", "role": "user", "content": value_to_chat_string(other) })],
    }
}

fn extract_reasoning_text(item: &serde_json::Map<String, Value>) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(summaries) = item.get("summary").and_then(|v| v.as_array()) {
        for summary in summaries {
            if let Some(text) = summary.as_str() {
                if !text.trim().is_empty() {
                    parts.push(strip_codex_reasoning_prefix(text).to_owned());
                }
                continue;
            }
            if let Some(text) = summary.get("text").and_then(|v| v.as_str()) {
                if !text.trim().is_empty() {
                    parts.push(strip_codex_reasoning_prefix(text).to_owned());
                }
            }
        }
    }

    if parts.is_empty() {
        if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
            for block in content {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if !text.trim().is_empty() {
                        parts.push(strip_codex_reasoning_prefix(text).to_owned());
                    }
                }
            }
        }
    }

    parts.join("\n")
}

/// 续轮(`previous_response_id`)时,Codex CLI 会把 v2.0.8+ 注入的 reasoning
/// `**Thinking**\n\n` prefix 通过 reasoning summary 文本回送回来。这里在
/// 写回上游 messages 的 `reasoning_content` 之前 strip 掉,避免 prefix 累积
/// 污染上游 history、长会话里出现"前面所有轮 reasoning_content 都带人造
/// header"。新一轮 reasoning 在 `converter.rs::open_reasoning` 处会再次注入,
/// 行为对 Codex CLI UI 显示无变化。
pub(crate) const CODEX_REASONING_PREFIX: &str = "**Thinking**\n\n";

fn strip_codex_reasoning_prefix(text: &str) -> &str {
    text.strip_prefix(CODEX_REASONING_PREFIX).unwrap_or(text)
}

/// 单个 Responses input item → 一条或多条 Chat message.
fn input_item_to_messages(item: &serde_json::Map<String, Value>) -> Vec<Value> {
    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match item_type {
        "message" => {
            let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = normalize_message_content(item.get("content").unwrap_or(&Value::Null));
            vec![json!({ "role": role, "content": content })]
        }
        "function_call" => {
            let call_id = item
                .get("call_id")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("id").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_owned();
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let arguments = sanitize_tool_arguments_json_string(
                item.get("arguments").and_then(|v| v.as_str()).unwrap_or(""),
            );
            vec![json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": if call_id.is_empty() { "call_unknown".to_owned() } else { call_id },
                    "type": "function",
                    "function": { "name": name, "arguments": arguments },
                }],
            })]
        }
        "function_call_output" => {
            // call_id 字段在 Codex CLI 历史里偶尔会以 tool_call_id / id 别名出现
            let call_id = item
                .get("call_id")
                .and_then(|v| v.as_str())
                .or_else(|| item.get("tool_call_id").and_then(|v| v.as_str()))
                .or_else(|| item.get("id").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_owned();
            let output_value = item
                .get("output")
                .cloned()
                .unwrap_or(Value::String(String::new()));
            let output_str =
                normalize_tool_output_for_context(Some(call_id.as_str()), output_value);
            vec![json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": output_str,
            })]
        }
        "input_image" => {
            let image_url = item
                .get("image_url")
                .or_else(|| item.get("url"))
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));
            let detail = item
                .get("detail")
                .and_then(|v| v.as_str())
                .unwrap_or("auto");
            vec![json!({
                "role": "user",
                "content": [{
                    "type": "image_url",
                    "image_url": image_url_for_chat(image_url, detail),
                }],
            })]
        }
        "input_file" => convert_file_item_to_message(item),
        "input_audio" => {
            let data = item.get("data").cloned().unwrap_or_else(|| json!(""));
            let fmt = item.get("format").and_then(|v| v.as_str()).unwrap_or("wav");
            let mime_type = item
                .get("mime_type")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("audio/{fmt}"));
            vec![json!({
                "role": "user",
                "content": [{
                    "type": "input_audio",
                    "input_audio": {
                        "data": data,
                        "format": fmt,
                        "mime_type": mime_type,
                    },
                }],
            })]
        }
        "input_video" => {
            let video_url = item
                .get("video_url")
                .or_else(|| item.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if video_url.is_empty() {
                vec![json!({ "role": "user", "content": "[Video input]" })]
            } else {
                vec![json!({
                    "role": "user",
                    "content": [{
                        "type": "image_url",
                        "image_url": { "url": video_url, "detail": "auto" },
                    }],
                })]
            }
        }
        "file_search_call"
        | "web_search_call"
        | "computer_call"
        | "code_interpreter_call"
        | "image_generation_call" => {
            vec![json!({ "role": "user", "content": format!("[{item_type}]") })]
        }
        "compaction" | "context_compaction" | "compaction_summary" => {
            // Codex CLI 触发 auto-compact 后把 summary 作为 ResponseItem::Compaction
            // 塞进 history(`codex-rs/protocol/src/models.rs:882`),续轮 input 里
            // 会带这个 item。`encrypted_content` 字段名是历史包袱,**实际是
            // 明文** —— Codex 自家 SUMMARY_PREFIX(`codex-rs/core/src/compact.rs:262`)
            // 已写明"based on this summary..."的语义。
            //
            // 必须把它转成 user message 注入下游 chat completions(role 与 Codex
            // 自家 inline compact 一致:`build_compacted_history_with_limit`
            // 也是 push role="user"),否则上游 LLM 完全看不到 summary,等同
            // 于 compact 后失忆 —— 体感"compact 触发了但下一轮 LLM 不记得任何
            // 之前的事"。
            let summary = item
                .get("encrypted_content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_owned();
            if summary.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "role": "user", "content": summary })]
            }
        }
        _ => {
            // 兜底:若有 content 字段,作为 user message 透传;否则丢弃
            if let Some(content) = item.get("content") {
                let role = item.get("role").and_then(|v| v.as_str()).unwrap_or("user");
                vec![json!({ "role": role, "content": normalize_message_content(content) })]
            } else {
                Vec::new()
            }
        }
    }
}

pub(crate) fn normalize_tool_output_for_context(
    call_id: Option<&str>,
    output_value: Value,
) -> String {
    normalize_tool_output_for_context_with_store(
        call_id,
        output_value,
        Some(super::artifact_store::global_tool_artifact_store()),
    )
}

pub(crate) fn normalize_tool_output_for_context_with_store(
    call_id: Option<&str>,
    output_value: Value,
    artifact_store: Option<&super::artifact_store::ToolArtifactStore>,
) -> String {
    let raw = match output_value {
        Value::String(s) => s,
        other => serde_json::to_string(&other).unwrap_or_default(),
    };
    if raw.chars().count() <= TOOL_OUTPUT_INLINE_MAX_CHARS {
        return raw;
    }
    let kind = classify_tool_output(&raw);
    let artifact = artifact_store.map(|store| store.save(call_id, kind, &raw));
    build_bounded_tool_output_summary(&raw, kind, artifact.as_ref())
}

fn build_bounded_tool_output_summary(
    raw: &str,
    kind: &str,
    artifact: Option<&super::artifact_store::StoredToolArtifact>,
) -> String {
    let original_chars = raw.chars().count();
    let original_lines = raw.lines().count();
    let mut out = String::new();

    out.push_str("[Tool output stored outside model context]\n");
    out.push_str("Visible content below is a bounded evidence summary, not the full raw output.\n");
    if let Some(artifact) = artifact {
        out.push_str(&format!("Artifact ID: {}\n", artifact.artifact_id));
        if let Some(call_id) = artifact.call_id.as_deref() {
            out.push_str(&format!("Tool call ID: {call_id}\n"));
        }
    } else {
        out.push_str("Artifact ID: unavailable; raw payload could not be stored.\n");
    }
    out.push_str(&format!("Artifact kind: {kind}\n"));
    out.push_str(&format!(
        "Original size: {original_chars} chars across {original_lines} lines.\n"
    ));
    if let Some(token_count) = extract_marker_value(raw, "Original token count:") {
        out.push_str(&format!("Original token count: {token_count}\n"));
    }
    if let Some(total_lines) = extract_marker_value(raw, "Total output lines:") {
        out.push_str(&format!("Reported output lines: {total_lines}\n"));
    }

    let path_hints = extract_path_hints(raw, 12);
    if !path_hints.is_empty() {
        out.push_str("Path hints:\n");
        for path in path_hints {
            out.push_str("- ");
            out.push_str(&path);
            out.push('\n');
        }
    }

    let url_hints = extract_url_hints(raw, 12);
    if !url_hints.is_empty() {
        out.push_str("URL hints:\n");
        for url in url_hints {
            out.push_str("- ");
            out.push_str(&url);
            out.push('\n');
        }
    }

    out.push_str("\n--- Begin head excerpt ---\n");
    out.push_str(&take_first_chars(raw, TOOL_OUTPUT_HEAD_CHARS));
    out.push_str("\n--- End head excerpt ---\n");
    out.push_str("\n--- Begin tail excerpt ---\n");
    out.push_str(&take_last_chars(raw, TOOL_OUTPUT_TAIL_CHARS));
    out.push_str("\n--- End tail excerpt ---\n");
    out.push_str(&format!(
        "\n[Omitted raw tool output from model context. Original size: {original_chars} chars.]"
    ));

    if out.chars().count() > TOOL_OUTPUT_VISIBLE_MAX_CHARS {
        let mut trimmed = take_first_chars(&out, TOOL_OUTPUT_VISIBLE_MAX_CHARS);
        trimmed.push_str("\n[Tool output compression summary truncated to visible budget.]");
        return trimmed;
    }
    out
}

fn classify_tool_output(raw: &str) -> &'static str {
    let sample = raw.chars().take(20_000).collect::<String>();
    let trimmed = sample.trim_start();
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && serde_json::from_str::<Value>(trimmed).is_ok()
    {
        return "json";
    }
    if sample.contains("https://")
        || sample.contains("http://")
        || sample.contains("web_search")
        || sample.contains("Search results")
        || sample.contains("source:")
    {
        return "web_or_search";
    }
    if sample.contains("Process exited with code")
        || sample.contains("Exit code")
        || sample.contains("Wall time:")
        || sample.contains("Output:")
    {
        return "command_output";
    }
    if !extract_path_hints(&sample, 1).is_empty() {
        return "file_or_code_output";
    }
    "opaque_tool_output"
}

fn extract_marker_value(raw: &str, marker: &str) -> Option<String> {
    let start = raw.find(marker)?;
    let rest = &raw[start + marker.len()..];
    let value = rest
        .trim_start()
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn extract_url_hints(raw: &str, max: usize) -> Vec<String> {
    let mut urls = Vec::new();
    for token in raw.lines().take(200).flat_map(str::split_whitespace) {
        let candidate = token.trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
            )
        });
        if !(candidate.starts_with("http://") || candidate.starts_with("https://")) {
            continue;
        }
        if urls.iter().any(|existing| existing == candidate) {
            continue;
        }
        urls.push(candidate.to_owned());
        if urls.len() >= max {
            break;
        }
    }
    urls
}

fn extract_path_hints(raw: &str, max: usize) -> Vec<String> {
    let mut paths = Vec::new();
    for line in raw.lines().take(200) {
        for token in line.split_whitespace() {
            let candidate = token
                .trim_matches(|ch: char| {
                    matches!(
                        ch,
                        '"' | '\'' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
                    )
                })
                .split(':')
                .next()
                .unwrap_or("");
            if !(candidate.starts_with('/') || candidate.starts_with("./")) {
                continue;
            }
            if !candidate.contains('.') {
                continue;
            }
            if paths.iter().any(|existing| existing == candidate) {
                continue;
            }
            paths.push(candidate.to_owned());
            if paths.len() >= max {
                return paths;
            }
        }
    }
    paths
}

fn take_first_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn take_last_chars(value: &str, max: usize) -> String {
    let mut chars = value.chars().rev().take(max).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

fn convert_file_item_to_message(item: &serde_json::Map<String, Value>) -> Vec<Value> {
    let file_id = item
        .get("file_id")
        .and_then(|v| v.as_str())
        .or_else(|| item.get("id").and_then(|v| v.as_str()))
        .unwrap_or("");
    let file_data = item.get("file_data").and_then(|v| v.as_str());
    let filename = item
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let mime_type = item
        .get("mime_type")
        .and_then(|v| v.as_str())
        .unwrap_or("application/octet-stream");

    if let Some(data) = file_data.filter(|s| !s.is_empty()) {
        let data_uri = format!("data:{mime_type};base64,{data}");
        return vec![json!({
            "role": "user",
            "content": [{
                "type": "image_url",
                "image_url": { "url": data_uri, "detail": "auto" },
            }],
        })];
    }

    if !file_id.is_empty() && filename != "unknown" {
        return vec![
            json!({ "role": "user", "content": format!("[File: {filename} (id={file_id})]") }),
        ];
    }
    if !file_id.is_empty() {
        return vec![json!({ "role": "user", "content": format!("[File id={file_id}]") })];
    }
    if filename != "unknown" {
        return vec![json!({ "role": "user", "content": format!("[File: {filename}]") })];
    }
    vec![json!({ "role": "user", "content": "[File]" })]
}

fn image_url_for_chat(value: Value, detail: &str) -> Value {
    match value {
        Value::Object(_) => value,
        Value::String(url) => json!({ "url": url, "detail": detail }),
        other => json!({ "url": value_to_chat_string(&other), "detail": detail }),
    }
}

fn merge_consecutive_user_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "user"
            || result
                .last()
                .and_then(|prev| prev.get("role"))
                .and_then(|v| v.as_str())
                != Some("user")
        {
            result.push(msg);
            continue;
        }

        let content = msg.get("content").cloned().unwrap_or(Value::Null);
        let Some(prev_obj) = result.last_mut().and_then(|prev| prev.as_object_mut()) else {
            continue;
        };
        let prev_content = prev_obj.get("content").cloned().unwrap_or(Value::Null);
        let merged = merge_user_content(prev_content, content);
        prev_obj.insert("content".into(), merged);
    }
    result
}

fn merge_user_content(prev: Value, current: Value) -> Value {
    if prev.is_array() || current.is_array() {
        let mut arr = normalize_content_array(&prev);
        arr.extend(normalize_content_array(&current));
        Value::Array(arr)
    } else if let (Some(prev), Some(current)) = (prev.as_str(), current.as_str()) {
        Value::String(format!("{prev}\n{current}"))
    } else if !current.is_null() {
        current
    } else {
        prev
    }
}

fn merge_consecutive_assistant_messages(messages: Vec<Value>) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "assistant"
            || result
                .last()
                .and_then(|prev| prev.get("role"))
                .and_then(|v| v.as_str())
                != Some("assistant")
        {
            result.push(msg);
            continue;
        }

        let Some(prev_obj) = result.last_mut().and_then(|prev| prev.as_object_mut()) else {
            continue;
        };
        if let Some(content) = msg.get("content").filter(|v| !v.is_null()) {
            let prev_content = prev_obj.get("content").cloned().unwrap_or(Value::Null);
            let merged = merge_assistant_content(prev_content, content.clone());
            prev_obj.insert("content".into(), merged);
        }
        if let Some(new_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
            let entry = prev_obj
                .entry("tool_calls")
                .or_insert_with(|| Value::Array(Vec::new()));
            if let Some(existing) = entry.as_array_mut() {
                existing.extend(new_calls.iter().cloned());
            }
        }
        if let Some(reasoning) = msg.get("reasoning_content") {
            if let Some(prev_reasoning) = prev_obj.get("reasoning_content").and_then(|v| v.as_str())
            {
                if let Some(current) = reasoning.as_str() {
                    prev_obj.insert(
                        "reasoning_content".into(),
                        Value::String(format!("{prev_reasoning}\n{current}")),
                    );
                }
            } else {
                prev_obj.insert("reasoning_content".into(), reasoning.clone());
            }
        }
        if !prev_obj.contains_key("content") {
            prev_obj.insert("content".into(), Value::String(String::new()));
        }
    }
    result
}

fn merge_assistant_content(prev: Value, current: Value) -> Value {
    if let (Some(prev), Some(current)) = (prev.as_str(), current.as_str()) {
        if prev.is_empty() {
            Value::String(current.to_owned())
        } else if current.is_empty() {
            Value::String(prev.to_owned())
        } else {
            Value::String(format!("{prev}\n{current}"))
        }
    } else if !current.is_null() {
        current
    } else {
        prev
    }
}

fn convert_developer_to_system_if_needed(messages: &mut [Value], provider: Option<&Provider>) {
    let keep_developer = provider.is_some_and(provider_is_openai_official);
    if keep_developer {
        return;
    }
    for msg in messages {
        if msg.get("role").and_then(|v| v.as_str()) == Some("developer") {
            if let Some(obj) = msg.as_object_mut() {
                obj.insert("role".into(), Value::String("system".into()));
            }
        }
    }
}

fn provider_is_openai_official(provider: &Provider) -> bool {
    let name = provider.name.to_ascii_lowercase();
    name.contains("openai") && !name.contains("azure")
}

/// 修复 / 重建工具调用 id 关联(双向)。
///
/// **正向(tool message 找 assistant.tool_calls)**:
///   1. tool_call_id 为空 → 从前一条 assistant.tool_calls 顺序补 id
///   2. tool_call_id 非空且能在前 assistant.tool_calls 找到 → 直接 ack 通过
///   3. tool_call_id 非空但前 assistant 不含该 id(history 被压缩 / 截断 /
///      跨 session 续接)→ 查 ToolCallCache:
///        - 命中:把 tool_call 注回最近一条 assistant 的 tool_calls 列表
///        - 未命中:在前面塞一条占位 assistant
///   4. 完全没有前置 assistant + cache 也没有 → 插占位 assistant + 保留 tool
///
/// **反向(assistant.tool_calls 找 tool message)** (issue #180):
///   - assistant.tool_calls = [a, b, c] 但只跟了 tool b → DeepSeek / Kimi 严格
///     校验时 400("tool result without matching tool_use" 等)。
///   - 解决:在三个状态切换点 flush pending —— 进新 assistant message / 进
///     user/system/developer / 遍历末尾 —— 对每个未应答 id 注入占位 tool
///     消息,id 严格匹配上一条 assistant.tool_calls,content 用 litellm 同款
///     "[System: Tool execution skipped/interrupted by user. ...]"。
///
/// 与 litellm 1.84.0 `factory.py::_add_missing_tool_results` +
/// `transformation.py::_ensure_tool_results_have_corresponding_tool_calls`
/// 行为一致(只是 litellm 还做 Anthropic 合并,本仓库不需要)。
fn repair_tool_call_ids(
    messages: &mut Vec<Value>,
    tool_call_cache: &super::tool_call_cache::ToolCallCache,
) {
    let mut pending_call_ids: Vec<String> = Vec::new();
    let mut repaired: Vec<Value> = Vec::with_capacity(messages.len());
    // 跟踪最近一条 assistant 在 repaired 里的下标,用于 path B "注回前 assistant"
    let mut last_assistant_idx: Option<usize> = None;

    for mut msg in messages.drain(..) {
        let role = msg
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        if role == "assistant" {
            // 进新 assistant 前:把上一个 assistant 还没答完的 tool_calls
            // 用占位 tool 消息补齐(issue #180 反向修复点 1/3)。
            flush_pending_tool_calls_as_placeholders(
                &mut repaired,
                &mut pending_call_ids,
                tool_call_cache,
            );
            if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
                pending_call_ids = calls
                    .iter()
                    .filter_map(|call| call.get("id").and_then(|id| id.as_str()))
                    .filter(|id| !id.trim().is_empty())
                    .map(str::to_owned)
                    .collect();
            }
            last_assistant_idx = Some(repaired.len());
            repaired.push(msg);
            continue;
        }

        if role == "tool" {
            let existing = msg
                .get("tool_call_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_owned();
            if existing.is_empty() {
                // path A1:有 pending → 顺序补
                if let Some(next) = pending_call_ids.first().cloned() {
                    pending_call_ids.remove(0);
                    if let Some(obj) = msg.as_object_mut() {
                        obj.insert("tool_call_id".into(), Value::String(next));
                    }
                } else {
                    // path A2:tool_call_id 空且 pending 也空 →
                    // 没有任何 id 可以关联,作为孤儿 message 丢弃(沿用旧行为)
                    continue;
                }
            } else if let Some(pos) = pending_call_ids.iter().position(|id| id == &existing) {
                // path B1:tool_call_id 非空 + 前 assistant 含该 id → ack 通过
                pending_call_ids.remove(pos);
            } else {
                // path B2:tool_call_id 非空但前 assistant 不含该 id →
                // 查 ToolCallCache 兜底重建
                let entry = tool_call_cache.get(&existing);
                let (name, arguments) = match entry {
                    Some(e) => (e.name, sanitize_tool_arguments_json_string(&e.arguments)),
                    // path B3:cache 也未命中 → 占位 (name 空字符串),
                    // 上游能 match id 不报 400 是关键,name / args 由上游能容
                    None => (String::new(), "{}".to_owned()),
                };
                let placeholder_tool_call = json!({
                    "id": existing,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    },
                });
                match last_assistant_idx {
                    // path B-into-existing:把重建 tool_call 注回最近 assistant
                    Some(idx) => {
                        let assistant = &mut repaired[idx];
                        let obj = assistant.as_object_mut().expect("assistant must be object");
                        let calls = obj
                            .entry("tool_calls".to_owned())
                            .or_insert_with(|| Value::Array(Vec::new()));
                        if let Value::Array(arr) = calls {
                            arr.push(placeholder_tool_call);
                        }
                    }
                    // path B-orphan:连前 assistant 都没有 → 在 tool 前插占位
                    None => {
                        let placeholder_assistant = json!({
                            "role": "assistant",
                            "content": "",
                            "tool_calls": [placeholder_tool_call],
                        });
                        last_assistant_idx = Some(repaired.len());
                        repaired.push(placeholder_assistant);
                    }
                }
            }
        }

        if matches!(role.as_str(), "user" | "system" | "developer") {
            // 进新一轮对话前:flush 上一轮未答完的 tool_calls(issue #180
            // 并行工具调用部分应答场景的核心修复点 2/3)。
            flush_pending_tool_calls_as_placeholders(
                &mut repaired,
                &mut pending_call_ids,
                tool_call_cache,
            );
            last_assistant_idx = None;
        }

        repaired.push(msg);
    }

    // 末尾 flush:整段 input 末尾 assistant.tool_calls 没有任何 tool 应答
    // (Codex CLI 中断生成 / 续轮时可能出现) (issue #180 修复点 3/3)。
    flush_pending_tool_calls_as_placeholders(&mut repaired, &mut pending_call_ids, tool_call_cache);

    *messages = repaired;
}

/// 把 `pending_call_ids` 里每个未应答的 tool_call id 翻成一条占位 tool 消息,
/// 直接 append 到 `repaired` 末尾(调用点保证此时插入位置是正确的:即上一条
/// assistant.tool_calls 之后、下一条非 tool 消息之前)。
fn flush_pending_tool_calls_as_placeholders(
    repaired: &mut Vec<Value>,
    pending_call_ids: &mut Vec<String>,
    tool_call_cache: &super::tool_call_cache::ToolCallCache,
) {
    if pending_call_ids.is_empty() {
        return;
    }
    for call_id in pending_call_ids.drain(..) {
        let tool_name = tool_call_cache
            .get(&call_id)
            .map(|entry| entry.name)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "unknown_tool".to_owned());
        let content = format!(
            "[System: Tool execution skipped/interrupted by user. \
             No result provided for tool '{tool_name}'.]"
        );
        repaired.push(json!({
            "role": "tool",
            "tool_call_id": call_id,
            "content": content,
        }));
    }
}

fn ensure_thinking_tool_call_reasoning(
    messages: &mut [Value],
    body: &Value,
    provider: Option<&Provider>,
) {
    if !request_thinking_enabled(body, provider) {
        return;
    }

    let has_tool_loop = messages.iter().any(|msg| {
        msg.get("role").and_then(|v| v.as_str()) == Some("tool")
            || (msg.get("role").and_then(|v| v.as_str()) == Some("assistant")
                && msg
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .is_some_and(|calls| !calls.is_empty()))
    });
    if !has_tool_loop {
        return;
    }

    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let has_tool_calls = msg
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .is_some_and(|calls| !calls.is_empty());
        if !has_tool_calls {
            continue;
        }
        let has_reasoning = msg
            .get("reasoning_content")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty());
        if !has_reasoning {
            if let Some(obj) = msg.as_object_mut() {
                obj.insert("reasoning_content".into(), Value::String(" ".into()));
            }
        }
    }
}

fn request_thinking_enabled(body: &Value, provider: Option<&Provider>) -> bool {
    if body.get("reasoning").is_some() {
        return true;
    }
    provider
        .is_some_and(|p| provider_looks_like(p, "deepseek") && provider_chat_thinking_enabled(p))
}

fn provider_looks_like(provider: &Provider, needle: &str) -> bool {
    let needle = needle.to_ascii_lowercase();
    [&provider.id, &provider.name, &provider.base_url]
        .iter()
        .any(|value| value.to_ascii_lowercase().contains(&needle))
}

fn sanitize_chat_body_for_provider(body: &mut Map<String, Value>, provider: Option<&Provider>) {
    let Some(provider) = provider else {
        return;
    };
    if provider_looks_like(provider, "minimax") || provider_looks_like(provider, "minimaxi") {
        sanitize_minimax_chat_body(body);
    }
}

/// MiniMax M2.x 的 OpenAI-compatible chat 端点并不接受完整 OpenAI/Codex
/// 参数集。官方文档主要列出 model/messages/stream/max_tokens/
/// max_completion_tokens/temperature/top_p/tools/tool_choice/
/// mask_sensitive_info；`response_format` 仅 MiniMax-Text-01 支持。
/// Codex Responses 转 Chat 时会生成 `reasoning_effort`、`response_format`、
/// `parallel_tool_calls` 等字段，MiniMax 会统一报 400:
/// "invalid params, invalid chat setting (2013)"。
fn sanitize_minimax_chat_body(body: &mut Map<String, Value>) {
    const MINIMAX_SYSTEM_MESSAGE_MAX_CHARS: usize = 24_000;
    let response_format_allowed = body
        .get("model")
        .and_then(|v| v.as_str())
        .is_some_and(|model| model.eq_ignore_ascii_case("MiniMax-Text-01"));

    body.retain(|key, _| {
        matches!(
            key.as_str(),
            "model"
                | "messages"
                | "stream"
                | "max_tokens"
                | "max_completion_tokens"
                | "temperature"
                | "top_p"
                | "tool_choice"
                | "tools"
                | "reasoning_split"
                | "stream_options"
                | "mask_sensitive_info"
        ) || (key == "response_format" && response_format_allowed)
    });

    // MiniMax 官方建议 OpenAI-compatible M2.7 工具调用启用
    // reasoning_split,让 thinking 单独进入 reasoning_details,避免塞进
    // content 的 <think>...</think> 里。
    body.insert("reasoning_split".into(), Value::Bool(true));
    // MiniMax 的 OpenAI-compatible streaming 不稳定接受
    // `stream_options.include_usage`;缺 usage 时响应转换层会补零值 usage。
    body.remove("stream_options");
    merge_consecutive_system_messages(body);
    // **issue #139 修(2026-05-12)**:MiniMax /v1/chat/completions 不接受
    // role=system,400 invalid role。把 system 全转 user + [System]\n prefix。
    convert_minimax_system_to_user_prefix(body, MINIMAX_SYSTEM_MESSAGE_MAX_CHARS);
    sanitize_minimax_tool_call_arguments(body);
    sanitize_minimax_tools(body);

    if let Some(choice) = body.get_mut("tool_choice") {
        let allowed = choice
            .as_str()
            .is_some_and(|s| matches!(s, "auto" | "none"));
        if !allowed {
            *choice = Value::String("auto".into());
        }
    }

    remove_non_positive_number(body, "temperature");
    remove_non_positive_number(body, "top_p");
}

fn merge_consecutive_system_messages(body: &mut Map<String, Value>) {
    let Some(Value::Array(messages)) = body.get_mut("messages") else {
        return;
    };

    let mut merged: Vec<Value> = Vec::with_capacity(messages.len());
    for msg in messages.drain(..) {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let is_system = role == "system";
        let prev_is_system = merged
            .last()
            .and_then(|prev| prev.get("role"))
            .and_then(|v| v.as_str())
            == Some("system");

        if is_system && prev_is_system {
            let current = msg
                .get("content")
                .map(value_to_chat_string)
                .unwrap_or_default();
            if let Some(prev_obj) = merged.last_mut().and_then(|prev| prev.as_object_mut()) {
                let prev = prev_obj
                    .get("content")
                    .map(value_to_chat_string)
                    .unwrap_or_default();
                let combined = if prev.is_empty() {
                    current
                } else if current.is_empty() {
                    prev
                } else {
                    format!("{prev}\n\n{current}")
                };
                prev_obj.insert("content".into(), Value::String(combined));
            }
            continue;
        }

        merged.push(msg);
    }

    *messages = merged;
}

fn sanitize_minimax_tool_call_arguments(body: &mut Map<String, Value>) {
    let Some(Value::Array(messages)) = body.get_mut("messages") else {
        return;
    };
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|v| v.as_str()) != Some("assistant") {
            continue;
        }
        let Some(tool_calls) = msg.get_mut("tool_calls").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for call in tool_calls.iter_mut() {
            let Some(function) = call.get_mut("function").and_then(|v| v.as_object_mut()) else {
                continue;
            };
            let arguments = function
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            function.insert(
                "arguments".into(),
                Value::String(sanitize_tool_arguments_json_string(arguments)),
            );
        }
    }
}

fn sanitize_tool_arguments_json_string(arguments: &str) -> String {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return "{}".to_owned();
    }
    if serde_json::from_str::<Value>(trimmed).is_ok() {
        return trimmed.to_owned();
    }
    "{}".to_owned()
}

/// MiniMax `/v1/chat/completions` **不接受 `role="system"` 的 message**
/// (issue #139,2026-05-12 实证 M2.7 + Text-01 都返 `400 invalid params,
/// chat content has invalid message role: system (2013)`)。
///
/// 本 fn 把所有 system message **转成 `role="user"` + content 前置 `[System]\n`
/// 前缀**,模型从前缀 token 自己理解原 system role 语义(chenyme/grok2api 同
/// pattern 在 grok_web 多轮 flatten 也已实证 work)。
///
/// 同时保留原 `normalize_and_split_minimax_system_messages` 的副作用:
/// - `\r\n` → `\n`(MiniMax 对 Windows 换行敏感)
/// - 超过 `max_chars`(24000)的 system 内容**切片成多个独立 user message**,
///   每片用 `[System part i/N]\n` 前缀(silent-failure F4:让模型看出是同一逻辑
///   段落的连续分片,不会误判为 N 个独立 system 指令);单段不切则用 `[System]\n`
///
/// **顺序依赖**:必须在 `merge_consecutive_system_messages` **之后**调用,
/// 那一步合并相邻 system + Codex instructions 字段,**仍是 system role**;
/// 本 fn 是 MiniMax-specific 兜底,把 system role 一次性转 user。
///
/// **content 形态**(silent-failure F2 修):接受 string + array of
/// `{type:input_text|text|output_text, text:"..."}` parts(Codex CLI Responses
/// spec 数组形)。其他形态 emit `tracing::warn!` 不静默 raw JSON stringify。
fn convert_minimax_system_to_user_prefix(body: &mut Map<String, Value>, max_chars: usize) {
    debug_assert!(
        max_chars > 0,
        "max_chars=0 会让本 fn 短路前不做 role 转换,违反 'no role=system 出本 fn' invariant"
    );
    let Some(Value::Array(messages)) = body.get_mut("messages") else {
        return;
    };
    if max_chars == 0 {
        // 防御 prod 误传 0(同 debug_assert 但 release 不 panic):
        // 仍做 role 转换,不切片(整段当一个 user message,即便超长)。
        // **必须**保证 invariant "出本 fn 后没有 role=system"。
        tracing::warn!(
            error_id = "MINIMAX_SYS_CONVERT_MAX_CHARS_ZERO",
            "convert_minimax_system_to_user_prefix 收到 max_chars=0,跳过切片但仍做 role 转换"
        );
    }
    const SYSTEM_PREFIX_SINGLE: &str = "[System]\n";
    let mut rewritten: Vec<Value> = Vec::with_capacity(messages.len());
    for msg in messages.drain(..) {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if role != "system" {
            rewritten.push(msg);
            continue;
        }
        let normalized = extract_minimax_system_text(msg.get("content")).replace("\r\n", "\n");
        if normalized.is_empty() {
            tracing::debug!(
                error_id = "MINIMAX_SYS_CONVERT_EMPTY_SKIP",
                "convert_minimax_system_to_user_prefix 跳过空 system message"
            );
            continue;
        }
        if max_chars == 0
            || normalized.chars().count() + SYSTEM_PREFIX_SINGLE.chars().count() <= max_chars
        {
            // 单段 fast path:整段(含 `[System]\n` prefix)≤ max_chars,直接 emit
            rewritten.push(json!({
                "role": "user",
                "content": format!("{SYSTEM_PREFIX_SINGLE}{normalized}"),
            }));
            continue;
        }
        // 切片 path(chatgpt-codex P1 修):先**预估 prefix 长度**算进每片预算,
        // 保证最终 emit 的每条 user message **整体 chars().count() ≤ max_chars**
        // (之前直接 split(content, max_chars) 后加 prefix → 每条 ≈ max_chars + 22
        //  char 超限,MiniMax 仍 400)
        let chunks = split_system_content_for_prefix(&normalized, max_chars);
        let total = chunks.len();
        tracing::info!(
            error_id = "MINIMAX_SYS_CONVERT_SPLIT",
            original_chars = normalized.chars().count(),
            chunks = total,
            max_chars,
            "convert_minimax_system_to_user_prefix 长 system 切成多段 user prefix message"
        );
        for (idx, chunk) in chunks.into_iter().enumerate() {
            let prefix = format!("[System part {}/{total}]\n", idx + 1);
            rewritten.push(json!({
                "role": "user",
                "content": format!("{prefix}{chunk}"),
            }));
        }
    }
    *messages = rewritten;
}

/// 抽 MiniMax system message 的 content 文本(silent-failure F2 修)。
///
/// 接受形态:
/// 给 `[System part i/N]\n` prefix 留预算后切 system content。
///
/// **不变量(chatgpt-codex P1 修)**:返回的每个 chunk 加上其最终 prefix 后
/// `chars().count() ≤ max_chars`。算法两轮迭代:
/// 1. 第一轮假设 N ≤ 9(1 digit prefix `[System part 1/9]\n` = 19 char)算 budget
/// 2. 若实际切出 chunks ≥ 10 → digit 数升,用更大 prefix length 再算一次
///
/// 99 段以内单 / 双轮收敛(99 段 prefix 最长 21 char,极少触发);MAX_CHARS=24000
/// 下 system 内容 > 24000*99 ≈ 2.3MB 才可能 99+ 段,实际不可能。
///
/// **edge case**:`max_chars ≤ prefix_max` 时 budget=0,降级为 budget=1
/// (`.max(1)`)避免无限切割空 chunk。这种 misconfiguration 下 emit 单字符
/// chunks 但仍保证 invariant(prefix + 1 char ≤ max_chars 要求 max_chars ≥ 22,
/// MAX_CHARS=24000 远高于此)。
fn split_system_content_for_prefix(normalized: &str, max_chars: usize) -> Vec<String> {
    let estimate_budget = |n_digits: usize| -> usize {
        // prefix 形 "[System part {i}/{N}]\n",静态部分 "[System part /]\n" + 2*N digits
        // 保守按"i 也用 N 位数"估上限
        const STATIC_LEN: usize = "[System part /]\n".len();
        let prefix_max = STATIC_LEN + 2 * n_digits;
        max_chars.saturating_sub(prefix_max).max(1)
    };
    // 第一轮:假设 N ≤ 9(1 digit)
    let chunks = split_string_by_char_limit(normalized, estimate_budget(1));
    if chunks.len() <= 9 {
        return chunks;
    }
    // 第二轮:N ≥ 10,用更大 prefix budget 重切
    let n_digits = chunks.len().to_string().len();
    split_string_by_char_limit(normalized, estimate_budget(n_digits))
}

/// 抽 MiniMax system message 的 content 文本(silent-failure F2 修)。
///
/// 接受形态:
/// - `Value::String(s)` → 直接返回
/// - `Value::Array(parts)` → 抽 `parts[i].text` 字段 join `\n`(Codex CLI
///   Responses spec 形:`[{type:"input_text", text:"..."}, ...]`)
/// - 其他形态 → `tracing::warn!` 加 stable error_id,**不静默** raw JSON 注入
fn extract_minimax_system_text(content: Option<&Value>) -> String {
    let Some(c) = content else {
        return String::new();
    };
    match c {
        Value::String(s) => s.clone(),
        Value::Array(parts) => {
            if parts.is_empty() {
                return String::new();
            }
            let texts: Vec<&str> = parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .filter(|s| !s.is_empty())
                .collect();
            if !texts.is_empty() {
                texts.join("\n")
            } else {
                // 数组有 parts 但无可抽 text(eg image-only system,异常 schema):
                // warn + 返回空,上层 skip。比注入 raw JSON 安全。
                let part_types: Vec<&str> = parts
                    .iter()
                    .filter_map(|p| p.get("type").and_then(Value::as_str))
                    .collect();
                tracing::warn!(
                    error_id = "MINIMAX_SYS_CONTENT_NO_TEXT_PARTS",
                    part_types = ?part_types,
                    "MiniMax system message 的 content array 无 text parts,跳过"
                );
                String::new()
            }
        }
        other => {
            let shape = if other.is_null() {
                "null"
            } else if other.is_boolean() {
                "bool"
            } else if other.is_number() {
                "number"
            } else if other.is_object() {
                "object"
            } else {
                "unknown"
            };
            tracing::warn!(
                error_id = "MINIMAX_SYS_CONTENT_UNEXPECTED_SHAPE",
                shape,
                "MiniMax system message 的 content 既不是 string 也不是 array,跳过"
            );
            String::new()
        }
    }
}

fn split_string_by_char_limit(input: &str, max_chars: usize) -> Vec<String> {
    if input.is_empty() || max_chars == 0 {
        return vec![input.to_owned()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut count = 0usize;
    for ch in input.chars() {
        current.push(ch);
        count += 1;
        if count == max_chars {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn sanitize_minimax_tools(body: &mut Map<String, Value>) {
    let Some(Value::Array(tools)) = body.get_mut("tools") else {
        return;
    };
    for tool in tools.iter_mut() {
        let Some(function) = tool.get_mut("function").and_then(|v| v.as_object_mut()) else {
            continue;
        };
        // MiniMax tool examples use the classic OpenAI tool schema and do not
        // accept OpenAI strict function-calling metadata.
        function.remove("strict");
    }
}

fn remove_non_positive_number(body: &mut Map<String, Value>, key: &str) {
    let should_remove = body
        .get(key)
        .and_then(|v| v.as_f64())
        .is_some_and(|v| v <= 0.0);
    if should_remove {
        body.remove(key);
    }
}

fn provider_chat_thinking_enabled(provider: &Provider) -> bool {
    if thinking_value_enabled(provider.request_options.get("thinking"))
        || provider.request_options.get("reasoning_effort").is_some()
    {
        return true;
    }

    let Some(chat_options) = provider
        .request_options
        .get("chat")
        .and_then(|v| v.as_object())
    else {
        return false;
    };

    thinking_value_enabled(chat_options.get("thinking"))
        || chat_options.get("reasoning_effort").is_some()
}

fn thinking_value_enabled(thinking: Option<&Value>) -> bool {
    match thinking {
        Some(Value::Object(thinking)) => {
            let thinking_type = thinking
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !thinking_type.is_empty() && thinking_type != "disabled" {
                return true;
            }
        }
        Some(Value::Bool(true)) => return true,
        Some(other) if !other.is_null() => return true,
        _ => {}
    }
    false
}

/// 当前请求(provider × model 组合)是否支持 vision(messages.content
/// 里允许 `image_url` block)。
///
/// 判断顺序:
/// 1. **请求 body 的 model** 在 `provider.modelCapabilities[<model>].supports_vision`
///    显式 false/true → 直接返回(粒度细到模型,允许同 provider 不同模型差异)
/// 2. fallback 到 `provider.modelCapabilities[<default_model>].supports_vision`
///    显式声明(向后兼容旧配置)
/// 3. 模型 id 命中**模型名黑名单**(2026-05-07 实测验证的纯文本模型) → 不支持
/// 4. 其余默认支持(走 OpenAI 标准多模态)
///
/// **2026-05-07 实测覆盖**(所有 5 接入 provider 的所有公开 model):
///
/// | Model | Vision | 来源 |
/// |---|---|---|
/// | `deepseek-v4-pro` / `deepseek-v4-flash` | ❌ | 实测 400 unknown variant `image_url` |
/// | `moonshot-v1-{8k,32k,128k}` / `moonshot-v1-auto` | ❌ | 实测 400 "Image input not supported" |
/// | `kimi-k2.5` / `kimi-k2.6` | ✅ | 实测 SAW_RED + 官方 vision guide |
/// | `moonshot-v1-{8k,32k,128k}-vision-preview` | ✅ | 实测 SAW_RED |
/// | `kimi-for-coding` | ✅ | 实测 SAW_RED(虽然 base_url 像 coding-only) |
/// | `mimo-v2-omni` / `mimo-v2-flash` / `mimo-v2.5` | ✅ | 实测 SAW_RED + 官方 omni 标识 |
/// | `mimo-v2-pro` / `mimo-v2.5-pro` | ❌ | 实测响应 "I don't see any image attached" |
/// | `mimo-v2*-tts*` | n/a | 不接受 chat 接口 |
///
/// **粒度从 provider 子串改成模型名精确匹配**:旧版 `["deepseek", "xiaomi",
/// "mimo", "qwen3.6"]` 子串黑名单会把整个 MiMo provider 的 omni / flash /
/// 2.5 这三个**支持视觉的**模型一刀切掉(误杀);也会让 Moonshot 的
/// `moonshot-v1-8k` 这种纯文本模型逃过(漏杀,因为 "moonshot" 不在子串名单)。
///
/// 这条防御对应 DeepSeek `deepseek-v4-pro` 在 deserialize 阶段直接对
/// `messages[i].content[*].type == "image_url"` 报 400 unknown variant,
/// 让 Codex CLI 续轮 history 一旦含过图就全链路阻塞(2026-05-06 实测)。
fn provider_supports_vision(provider: Option<&Provider>, model: Option<&str>) -> bool {
    let Some(p) = provider else {
        return true;
    };

    // **关键**:request body 缺 model 字段时仍要保护文本-only 上游不收图。
    // codex-connector P1 review (PR #43) 指出:几条 conversion path 允许 body
    // 不带 model 进来,如果只在 `model.is_some()` 时跑黑名单,DeepSeek 这类
    // text-only provider 一旦 model 缺失就会让 image_url 透传出去,触发原本
    // 要修的 400 unknown variant 失败。
    //
    // 解决:把 body model + provider.models["default"] 合并成 effective_model,
    // 所有检查(modelCapabilities 显式 / TEXT_ONLY_MODELS 黑名单)都在它上面跑。
    let default_model = p
        .models
        .get("default")
        .map(|s| codex_app_transfer_registry::strip_internal_model_suffix(s))
        .filter(|s| !s.is_empty());
    let effective_model: Option<String> = model
        .map(|s| s.to_owned())
        .or_else(|| default_model.clone());

    // 1:effective_model 命中 provider.modelCapabilities 显式声明 → 直接采用
    if let Some(m) = effective_model.as_deref() {
        if let Some(b) = p
            .model_capabilities
            .get(m)
            .and_then(|v| v.get("supports_vision"))
            .and_then(|v| v.as_bool())
        {
            return b;
        }
    }
    // 2:body model 不在 capabilities 但 default_model 在(向后兼容旧配置:
    //    用户可能只在 modelCapabilities 标过 default model 的能力)
    if let (Some(body), Some(def)) = (model, default_model.as_deref()) {
        if body != def {
            if let Some(b) = p
                .model_capabilities
                .get(def)
                .and_then(|v| v.get("supports_vision"))
                .and_then(|v| v.as_bool())
            {
                return b;
            }
        }
    }

    // 3:effective_model 命中**硬编码模型名黑名单**(2026-05-07 实测 — 详见函数 doc)
    if let Some(m) = effective_model.as_deref() {
        let lc = m.to_ascii_lowercase();
        const TEXT_ONLY_MODELS: &[&str] = &[
            // DeepSeek v4 系列(实测 400 unknown variant `image_url`)
            "deepseek-v4-pro",
            "deepseek-v4-flash",
            // Moonshot 标准 v1 系列(无 -vision-preview 后缀,实测 400
            // "Image input not supported for model ...")
            "moonshot-v1-8k",
            "moonshot-v1-32k",
            "moonshot-v1-128k",
            "moonshot-v1-auto",
            // Xiaomi MiMo 文本-only 子集(实测响应 "I don't see any image attached")
            "mimo-v2-pro",
            "mimo-v2.5-pro",
            // 智谱 GLM 文本旗舰模型（官方明确为文本模型，视觉走 GLM-5V 等独立模型）
            "glm-5.1",
            "glm-4.7",
            // 阿里云百炼 Qwen 标准版（视觉能力走 Qwen-VL 系列）
            "qwen3.6-plus",
            "qwen3.6-flash",
            // MiniMax M2.7（文本模型，图像理解通过独立 MCP 工具实现）
            "MiniMax-M2.7",
        ];
        if TEXT_ONLY_MODELS.iter().any(|n| lc == *n) {
            return false;
        }
    }

    // 4:默认支持(覆盖未列在白名单的新模型 / OpenAI 标准 vision provider)
    true
}

/// 把 messages 中所有 `image_url` content block 替换为占位文本块,
/// 防止纯文本上游(deepseek-v4-pro / mimo-v2.5-pro 等)拒绝整 body。
/// 替换后若 content 数组只剩 text 块,会进一步合并为单 string,与
/// 普通文本消息序列化形态一致。
fn strip_image_blocks_in_place(messages: &mut [Value]) {
    tracing::debug!(
        target: "codex_app_transfer::vision",
        "Stripping vision content because current provider/model does not support vision (Computer Use protection active)"
    );

    const PLACEHOLDER: &str = "[image omitted: current provider does not support vision]";
    const COMPUTER_USE_PLACEHOLDER: &str =
        "[Computer Use screenshot omitted: current model does not support vision]";

    for msg in messages.iter_mut() {
        let Some(obj) = msg.as_object_mut() else {
            continue;
        };
        let Some(content) = obj.get_mut("content") else {
            continue;
        };
        let Value::Array(arr) = content else {
            continue;
        };
        let mut had_image = false;

        for block in arr.iter_mut() {
            let Some(block_obj) = block.as_object_mut() else {
                continue;
            };
            let block_type = block_obj
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            if block_type == "image_url" || block_type == "input_image" {
                had_image = true;

                // Simple heuristic for Computer Use screenshots
                let is_likely_computer_use = block_obj
                    .get("image_url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.len() > 20000)
                    .unwrap_or(false);

                let placeholder = if is_likely_computer_use {
                    COMPUTER_USE_PLACEHOLDER
                } else {
                    PLACEHOLDER
                };

                block_obj.clear();
                block_obj.insert("type".into(), Value::String("text".into()));
                block_obj.insert("text".into(), Value::String(placeholder.to_string()));
            }
        }
        if had_image {
            // 替换后若全是 text,合并成单 string,跟其它纯文本消息一致
            let all_text = arr
                .iter()
                .all(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"));
            if all_text {
                let combined: Vec<String> = arr
                    .iter()
                    .filter_map(|b| b.get("text").and_then(|v| v.as_str()).map(str::to_owned))
                    .collect();
                obj.insert("content".into(), Value::String(combined.join("\n")));
            }
        }
    }
}

/// 兜底:有 `image_url` block 但完全没 `text` block 的 message,在 content
/// 数组末尾追加 `{type:"text", text:" "}` —— 防止 MiMo 多模态接口因
/// "Param Incorrect: text is not set" 拒绝纯图请求(MiMo 文档要求含
/// `image_url` 时至少 1 个 text part)。其他 `supports_vision` provider
/// (Kimi / OpenAI 等)对此无副作用,统一处理避免 per-provider 分支,
/// 也省掉未来新接 vision provider 时重新评估的工作。
///
/// 对照实现:`7as0nch/mimo2codex` `reqToChat.ts:71-79` 同名兜底逻辑。
fn ensure_text_part_when_image_present(messages: &mut [Value]) {
    for msg in messages.iter_mut() {
        let Some(obj) = msg.as_object_mut() else {
            continue;
        };
        let Some(Value::Array(arr)) = obj.get_mut("content") else {
            continue;
        };
        let has_image = arr
            .iter()
            .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("image_url"));
        let has_text = arr
            .iter()
            .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"));
        if has_image && !has_text {
            arr.push(json!({"type": "text", "text": " "}));
        }
    }
}

/// Stage 3 (in progress): Stronger stripping for `input_image` (Computer Use screenshots)
/// will be added here in a follow-up refinement. For now, the main defense is
/// in strip_image_blocks_in_place + tool filtering (Stage 2).

/// Responses message.content 可能是 string 或 [{type, text/image_url}].
/// stateless 阶段:string 保留;text 块拼成 string;含 image_url 的块降级为
/// Chat 多模态格式(`[{type: "text", text}, {type: "image_url", image_url}]`).
fn normalize_message_content(content: &Value) -> Value {
    match content {
        Value::String(s) => Value::String(s.clone()),
        Value::Array(arr) => {
            // 全是 text 块:拼成单 string(Codex CLI 大多数场景)
            // 任一块是非文本:转成 Chat 多模态 array
            let mut text_only = true;
            for block in arr {
                let t = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if !matches!(t, "input_text" | "output_text" | "text") {
                    text_only = false;
                    break;
                }
            }
            if text_only {
                let mut combined = String::new();
                for block in arr {
                    if let Some(text) = block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .or_else(|| block.as_str())
                    {
                        if !combined.is_empty() {
                            combined.push('\n');
                        }
                        combined.push_str(text);
                    }
                }
                Value::String(combined)
            } else {
                let mut chat_blocks: Vec<Value> = Vec::new();
                for block in arr {
                    if let Some(b) = responses_block_to_chat_block(block) {
                        chat_blocks.push(b);
                    }
                }
                Value::Array(chat_blocks)
            }
        }
        Value::Null => Value::String(String::new()),
        other => Value::String(value_to_chat_string(other)),
    }
}

fn normalize_content_array(content: &Value) -> Vec<Value> {
    match content {
        Value::Null => Vec::new(),
        Value::Array(items) => items
            .iter()
            .filter_map(responses_block_to_chat_block)
            .collect(),
        other => responses_block_to_chat_block(other).into_iter().collect(),
    }
}

/// 单个 Responses content block → Chat content block.
fn responses_block_to_chat_block(block: &Value) -> Option<Value> {
    if let Some(s) = block.as_str() {
        return Some(json!({ "type": "text", "text": s }));
    }
    let Some(obj) = block.as_object() else {
        return Some(json!({ "type": "text", "text": value_to_chat_string(block) }));
    };
    let t = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match t {
        "input_text" | "output_text" | "text" => {
            let text = obj
                .get("text")
                .map(value_to_chat_string)
                .unwrap_or_default();
            Some(json!({ "type": "text", "text": text }))
        }
        "input_image" => {
            let detail = obj.get("detail").and_then(|v| v.as_str()).unwrap_or("auto");
            let image_url = obj
                .get("image_url")
                .or_else(|| obj.get("url"))
                .cloned()
                .unwrap_or_else(|| Value::String(String::new()));
            Some(json!({
                "type": "image_url",
                "image_url": image_url_for_chat(image_url, detail),
            }))
        }
        "image_url" => Some(block.clone()),
        "input_audio" => {
            let audio = obj.get("input_audio").cloned().unwrap_or_else(|| {
                json!({
                    "data": obj.get("data").cloned().unwrap_or_else(|| json!("")),
                    "format": obj.get("format").and_then(|v| v.as_str()).unwrap_or("wav"),
                })
            });
            Some(json!({ "type": "input_audio", "input_audio": audio }))
        }
        "refusal" => Some(json!({
            "type": "refusal",
            "refusal": obj.get("refusal").cloned().unwrap_or_else(|| json!("")),
        })),
        "input_file" => {
            let marker = obj
                .get("filename")
                .or_else(|| obj.get("file_id"))
                .map(value_to_chat_string)
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "input_file".into());
            Some(json!({ "type": "text", "text": format!("[input_file: {marker}]") }))
        }
        "input_video" => {
            let url = obj
                .get("video_url")
                .or_else(|| obj.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() {
                Some(json!({ "type": "text", "text": "[Video input]" }))
            } else {
                Some(json!({
                    "type": "image_url",
                    "image_url": { "url": url, "detail": "auto" },
                }))
            }
        }
        "" if obj.contains_key("text") => Some(json!({
            "type": "text",
            "text": obj.get("text").map(value_to_chat_string).unwrap_or_default(),
        })),
        "" if obj.contains_key("image_url") => Some({
            let mut cloned = obj.clone();
            cloned.insert("type".into(), Value::String("image_url".into()));
            Value::Object(cloned)
        }),
        "" if obj.contains_key("input_audio") => Some({
            let mut cloned = obj.clone();
            cloned.insert("type".into(), Value::String("input_audio".into()));
            Value::Object(cloned)
        }),
        _ => Some(json!({ "type": "text", "text": value_to_chat_string(block) })),
    }
}

/// 把 Responses API 的 `text.format` 翻译成 Chat Completions 的 `response_format`。对已知不支持 `json_schema` 的上游(实测 DeepSeek
/// API 在 deserialize 阶段对 `response_format.type=json_schema` 报 400
/// "This response_format type is unavailable now"),把
/// `{type:"json_schema", ...}` 降级为 `{type:"json_object"}`,让模型
/// 仍输出 JSON,schema 字段顺序由 Codex CLI 的 system prompt 中"required
/// keys"指示驱动(2026-05-06 实测各家在 system prompt 给约束时,模型
/// 输出的 JSON 都能匹配三个 key)。
///
/// 实测结果(2026-05-06,真实 API):
/// - DeepSeek v4-pro:json_schema → 400;json_object → 200 + 合法 JSON
/// - Kimi k2.6:json_schema → 200 + 合法 JSON(不降级)
/// - MiMo v2.5-pro(PAYG / Token Plan):json_schema → 200 + 合法 JSON(**不降级**)
fn build_response_format_for_provider(
    text_config: &Value,
    provider: Option<&Provider>,
) -> Option<Value> {
    let fmt = text_config.get("format")?.as_object()?;
    let fmt_type = fmt.get("type").and_then(|v| v.as_str()).unwrap_or("");

    let json_schema_value = || {
        json!({
            "type": "json_schema",
            "json_schema": {
                "name": fmt.get("name").and_then(|v| v.as_str()).unwrap_or("response_schema"),
                "schema": fmt.get("schema").cloned().unwrap_or_else(|| json!({})),
                "strict": fmt.get("strict").and_then(|v| v.as_bool()).unwrap_or(false),
            },
        })
    };

    let raw = match fmt_type {
        "json_schema" => json_schema_value(),
        "json_object" => json!({ "type": "json_object" }),
        "text" => return None,
        _ if fmt.contains_key("schema") => json_schema_value(),
        _ => return None,
    };

    // json_schema 降级:命中 provider 黑名单时,转 json_object
    if raw.get("type").and_then(|v| v.as_str()) == Some("json_schema")
        && !provider_supports_json_schema_response_format(provider)
    {
        return Some(json!({ "type": "json_object" }));
    }
    Some(raw)
}

/// 上游 provider 是否支持 `response_format = {type:"json_schema", json_schema:{...}}`。
///
/// 判断顺序:
/// 1. `provider.modelCapabilities[<default_model>].supports_json_schema_response_format`
///    显式 false → 不支持;true → 支持
/// 2. fallback 黑名单(只放经实测确认拒绝 `json_schema` 的上游):
///    - `deepseek` → 不支持(API 直接 400 unavailable)
/// 3. 其他默认支持(Kimi / MiMo 实测都支持完整 OpenAI `json_schema` 语义)
///
/// **不要把 mimo / qwen3.6 加入名单**:实测 MiMo 两家都支持
/// json_schema(2026-05-06)。误加会导致正常 schema 被无谓降级。
fn provider_supports_json_schema_response_format(provider: Option<&Provider>) -> bool {
    let Some(p) = provider else {
        return true;
    };

    // 1. 显式 modelCapabilities 优先
    let default_model = p
        .models
        .get("default")
        .map(|s| codex_app_transfer_registry::strip_internal_model_suffix(s))
        .unwrap_or_default();
    let candidates: [&str; 2] = [default_model.as_str(), default_model.trim()];
    for key in candidates {
        if key.is_empty() {
            continue;
        }
        if let Some(b) = p
            .model_capabilities
            .get(key)
            .and_then(|v| v.get("supports_json_schema_response_format"))
            .and_then(|v| v.as_bool())
        {
            return b;
        }
    }

    // 2. 实测黑名单(命中即视为不支持)。
    //    **慎重添加新条目**:必须先用真实凭据 curl 验证 json_schema 真的报错
    //    (DeepSeek 形态:400 + "This response_format type is unavailable now")。
    const KNOWN_NOT_SUPPORTED: &[&str] = &["deepseek"];
    !KNOWN_NOT_SUPPORTED
        .iter()
        .any(|needle| provider_looks_like(p, needle))
}

fn build_reasoning_effort(reasoning: &Value) -> Option<Value> {
    match reasoning {
        Value::String(s) => normalize_chat_reasoning_effort(s),
        Value::Object(obj) => {
            if let Some(effort) = obj.get("effort") {
                if let Some(effort) = effort.as_str() {
                    return normalize_chat_reasoning_effort(effort);
                }
                return Some(effort.clone());
            }
            if obj.contains_key("summary") {
                return Some(reasoning.clone());
            }
            Some(reasoning.clone())
        }
        Value::Null => None,
        other => Some(other.clone()),
    }
}

fn normalize_chat_reasoning_effort(effort: &str) -> Option<Value> {
    match effort.trim().to_ascii_lowercase().as_str() {
        "minimal" | "low" | "medium" | "high" => {
            Some(Value::String(effort.trim().to_ascii_lowercase()))
        }
        "xhigh" | "max" | "highest" => Some(Value::String("high".into())),
        "none" | "off" | "auto" | "" => None,
        _ => None,
    }
}

fn normalize_tool_choice(tool_choice: &Value) -> Value {
    let Some(obj) = tool_choice.as_object() else {
        return tool_choice.clone();
    };
    if obj
        .get("function")
        .and_then(|v| v.as_object())
        .and_then(|f| f.get("name"))
        .is_some()
    {
        return tool_choice.clone();
    }
    match obj.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "auto" => Value::String("auto".into()),
        "none" => Value::String("none".into()),
        "required" | "tool" | "any" => Value::String("required".into()),
        "function" if obj.get("function").is_none() => Value::String("required".into()),
        _ => tool_choice.clone(),
    }
}

fn handle_store_param(value: &Value) -> Option<Value> {
    value.as_bool().map(Value::Bool)
}

fn handle_metadata_param(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let mut cleaned = serde_json::Map::new();
    for (idx, (key, value)) in obj.iter().enumerate() {
        if idx >= 16 {
            break;
        }
        let key = key.chars().take(64).collect::<String>();
        let value = value_to_chat_string(value)
            .chars()
            .take(512)
            .collect::<String>();
        cleaned.insert(key, Value::String(value));
    }
    if cleaned.is_empty() {
        None
    } else {
        Some(Value::Object(cleaned))
    }
}

fn handle_prediction_param(value: &Value) -> Option<Value> {
    let obj = value.as_object()?;
    let prediction_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let content = obj.get("content")?;
    if prediction_type == "content" {
        return Some(json!({ "type": "content", "content": value_to_chat_string(content) }));
    }
    Some(json!({ "type": "content", "content": value_to_chat_string(content) }))
}

fn handle_service_tier(value: &Value) -> Option<Value> {
    value
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| Value::String(s.to_owned()))
}

fn handle_modalities(value: &Value) -> Option<Value> {
    let arr = value.as_array()?;
    let cleaned = arr
        .iter()
        .filter_map(|v| v.as_str())
        .filter(|v| matches!(*v, "text" | "audio" | "image"))
        .map(|v| Value::String(v.to_owned()))
        .collect::<Vec<_>>();
    if cleaned.is_empty() {
        None
    } else {
        Some(Value::Array(cleaned))
    }
}

fn handle_audio_param(value: &Value) -> Option<Value> {
    value.as_object().map(|_| value.clone())
}

fn value_to_chat_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

/// Responses tool 定义 → Chat tool 定义.
/// 把单个 Responses API tool 转成零或多个 Chat Completions tool。
///
/// 返回 `Vec<Value>` 而非 `Option<Value>` 是为了支持 `type:"namespace"` 展平
/// (Codex CLI 把 MCP server 工具集打成一个 namespace 包,内层 5-26 个具体
/// `type:"function"`,实测 9 个 server 共 88 个 tool 在第三方 chat provider
/// 之前必须展平为顶级 function 数组)。
///
/// 实测形态(2026-05-09 抓本机 ~/.codex/config.toml 配 12+ MCP server 时
/// Codex CLI 的入站 Responses API body):
/// - `function` × 420 / 轮(Codex 内置 + `read_mcp_resource` 等通用 meta)
/// - `namespace` × 218 / 轮(9 个 server 包装,内层 88 个具体 MCP function)
/// - `custom` × 28 / 轮(`apply_patch` 用 lark grammar)
/// - `web_search` × 28 / 轮(server-side built-in,无 name/parameters,
///   chat 端无等价,继续 drop + warn_once 提示用户)
fn convert_responses_tool_to_chat_tool(tool: &Value, provider: Option<&Provider>) -> Vec<Value> {
    let Some(obj) = tool.as_object() else {
        return vec![];
    };
    let Some(ttype) = obj.get("type").and_then(|v| v.as_str()) else {
        return vec![];
    };
    match ttype {
        "function" => {
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let description = obj
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mut parameters = obj.get("parameters").cloned().unwrap_or_else(|| json!({}));
            if let Some(po) = parameters.as_object_mut() {
                if !po.contains_key("type") {
                    po.insert("type".into(), Value::String("object".into()));
                }
            }
            let strict = obj.get("strict").and_then(|v| v.as_bool()).unwrap_or(false);
            vec![json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters,
                    "strict": strict,
                },
            })]
        }
        "custom" => {
            // Custom tool(无 JSON schema)降级为接受单字符串 input 的 function
            let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let description = obj
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            vec![json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "input": {
                                "type": "string",
                                "description": "Free-form input passed verbatim to the tool.",
                            }
                        },
                        "required": ["input"],
                    },
                    "strict": false,
                },
            })]
        }
        "namespace" => {
            // Codex CLI 用 `type:"namespace"` 包装 MCP server 工具集 — 实测
            // `~/.codex/config.toml` 配的每个 `[mcp_servers.<name>]` 在入站
            // Responses API body 里都是一个 `{type:"namespace", name:"mcp__<name>__",
            // tools:[ {type:"function", ...}, ... ]}` 包,内层 5-26 个具体 function。
            // 第三方 chat completions provider 不认 namespace type,**必须递归
            // 展平内层 functions 为顶级 tool 数组**,模型才能看到具体 MCP tools
            // 像 `notion_create_pages` / `figma_get_file_data` 等并直接调用。
            //
            // 借鉴 `7as0nch/mimo2codex` `src/translate/reqToChat.ts:232-250` 同名
            // namespace 展平逻辑(见 reqToChat 注释 "Shape we've seen in the wild")。
            //
            // 不做的:展平内层时**不**改写 tool name(实测内层 function name 已经
            // 自带前缀如 `migrate_pages_to_workers_guide`,无冲突风险);**不**保留
            // namespace 包裹元数据(模型只需看到具体 tool name + description 即可)。
            //
            // **⚠️ 跟 `gemini_native::request.rs::responses_tools_to_chat_tools`
            // 的 `"namespace"` 分支故意分歧**:那边把 `namespace.name + description`
            // 作 prefix 注入到每个内层 function.description(`[MCP server <n>: <d>]`
            // ...)。原因:Gemini 3.x 缺这层 server-level context 时倾向选"动作类"
            // 工具(误选 create 而非 search,user 实测)。Chat completions 上游
            // (OpenAI/Anthropic Messages)未观察到此 bias,故 chat 路径不注入,
            // 保持 wire 干净。如果要让两个路径行为一致,可以把 prefix 逻辑提到
            // 公共 helper — 但当前 chat 路径模型选择没问题,保持差异化最小风险。
            let Some(inner) = obj.get("tools").and_then(|v| v.as_array()) else {
                tracing::debug!(
                    namespace_name = ?obj.get("name").and_then(|v| v.as_str()),
                    "dropping namespace tool with no nested `tools` array"
                );
                return vec![];
            };
            inner
                .iter()
                .flat_map(|inner_tool| convert_responses_tool_to_chat_tool(inner_tool, provider))
                .collect()
        }
        // Codex.app 默认每轮都给 tools 数组传 `{type:"web_search",
        // external_web_access:true, search_content_types:["text","image"]}`
        // (实测 dump 确认),作为 Responses API 标准 server-side built-in。
        // 各家上游 chat completions API 用各自字段表达 web search 能力,
        // 代理层负责 per-provider 适配。本提交先实施 MiMo,Kimi /
        // DeepSeek / MiniMax / Qwen / GLM 留 TODO,逐家文档实证后跟进。
        // 实施跟踪见 `docs/web-search-implementation-tracker.md`。
        "web_search" | "web_search_preview" => convert_web_search_tool(obj, provider),
        // Responses 专属类型(local_shell / file_search / computer_use* /
        // code_interpreter / image_generation / mcp 等)Chat 端点不认,丢弃。
        // warn_once 防多轮重发刷屏(借鉴 mimo2codex `reqToChat.ts:158-172` warnOnce)。
        other => {
            crate::warn_once_drop_tool(other);
            vec![]
        }
    }
}

/// Per-provider `web_search` / `web_search_preview` 适配。Codex.app 入站默认
/// 每轮发 OpenAI Responses API 标准的 `{type:"web_search", external_web_access:true,
/// search_content_types:["text","image"]}`,本函数转成各上游 chat API 真实
/// 支持的形态。
///
/// **逐家文档实证后才能加映射**(`docs/web-search-implementation-tracker.md`)。
/// 暂未实证的 provider 走 `_ => warn_once + drop`,模型退化到用 MCP 工具(如
/// 用户配的 Node Repl + JS fetch DDG 这种自带能力)联网,**功能仍可用,只是
/// 不走最高效路径**。
///
/// ## 已实证 provider
///
/// ### Xiaomi MiMo(`platform.xiaomimimo.com`)
///
/// 1:1 复刻 `7as0nch/mimo2codex@fe79178` `src/translate/reqToChat.ts:196-209`。
/// MiMo chat 端原生支持 `type:"web_search"`(MiMo 私有扩展,**需要在 MiMo
/// 控制台开 Web Search Plugin** —— https://platform.xiaomimimo.com/#/console/plugin)。
///
/// 字段透传:`user_location` / `max_keyword` / `force_search` / `limit`(全可选)。
/// OpenAI 的 `external_web_access` / `search_content_types` / `search_context_size`
/// 在 MiMo 无等价,silent drop(对齐 mimo2codex)。
fn convert_web_search_tool(
    obj: &serde_json::Map<String, Value>,
    provider: Option<&Provider>,
) -> Vec<Value> {
    let Some(provider) = provider else {
        crate::warn_once_drop_tool("web_search:no-provider");
        return vec![];
    };

    // A 层:配置开关。`request_options.web_search_enabled` 默认 false。
    // 用户必须主动在 codex-app-transfer config 里标 true 才会启用;UI 提示
    // 文案:"web_search 需要先在 Xiaomi MiMo 控制台付费启用后才能正常使用"。
    if !provider_web_search_enabled(provider) {
        crate::warn_once_drop_tool("web_search:disabled-by-config");
        return vec![];
    }

    // B 层:运行时自动 disable cache。上游 4xx 失败一次后(forward.rs 调
    // `disable_web_search_for`),本进程后续 turn 立即 drop,避免每个 turn
    // 都触发同样错误。本次启动有效;用户去 UI 关 `web_search_enabled = false`
    // 才是持久关闭。
    if crate::is_web_search_disabled_for(&provider.id) {
        crate::warn_once_drop_tool("web_search:auto-disabled-after-failure");
        return vec![];
    }

    if provider_looks_like(provider, "xiaomimimo") || provider_looks_like(provider, "mimo") {
        // MiMo 私有 chat 端 web_search 形态(reqToChat.ts:196-209)
        let mut out = serde_json::Map::new();
        out.insert("type".into(), Value::String("web_search".into()));
        for field in ["user_location", "max_keyword", "force_search", "limit"] {
            if let Some(v) = obj.get(field) {
                out.insert(field.to_string(), v.clone());
            }
        }
        return vec![Value::Object(out)];
    }

    if provider_looks_like(provider, "kimi") || provider_looks_like(provider, "moonshot") {
        // Kimi 内置 `$web_search` builtin_function(WebFetch
        // `platform.kimi.ai/docs/guide/use-web-search` 真原文实证):
        //   {"type":"builtin_function", "function":{"name":"$web_search"}}
        // **不透传任何子字段**(Kimi 文档明确只要 type + function.name)。
        // 配套强制 `thinking:{type:"disabled"}` 顶级字段在
        // `responses_body_to_chat_body_for_provider_with_session` body 后处理
        // 注入(`contains_kimi_web_search_tool` 检测命中即写)。
        // 计费:每次搜索调用 $0.005(独立于 token),搜索结果计入 prompt_tokens。
        return vec![serde_json::json!({
            "type": "builtin_function",
            "function": {
                "name": "$web_search",
            },
        })];
    }

    // ── 文档实证不支持 web_search 的 provider ──
    // 这些 provider 的 chat completions API 明确只接受 `type:"function"`,
    // 没有 builtin web_search / native search / extra_body 顶级开关等任何
    // 形式的 server-side web 搜索能力。用户启用 web_search_enabled=true 也
    // 不会 work,只能走 P5 修通的 namespace MCP 工具(如 Node Repl + JS
    // fetch)绕路联网。warn_once 写明具体 provider 帮用户理解。

    // DeepSeek(WebFetch `api-docs.deepseek.com/api/create-chat-completion`
    // 真原文实证 2026-05-09):"Currently, only `function` is supported."
    // tools 数组只接受 type:"function",最多 128 个,无 builtin / web_search
    // / 任何 server-side 搜索能力。
    if provider_looks_like(provider, "deepseek") {
        crate::warn_once_drop_tool("web_search:not-supported-by-deepseek-api");
        return vec![];
    }

    // MiniMax(三方实证 2026-05-09:WebFetch `platform.minimaxi.com/docs/api-reference/`
    // + `platform.minimax.io/docs/api-reference/text-openai-api` + liteLLM
    // MiniMax provider 文档):MiniMax chat completions API(`api.minimaxi.com/v1`)
    // tools 仅 `type:"function"`,**无任何 builtin web_search / native search /
    // 顶级 enable_search 字段**。MiniMax 自家的 web_search 能力**仅作 Token Plan
    // MCP 工具存在**,不在 chat completions API 内。用户需联网搜索 → 走 P5
    // 修通的 namespace MCP 路径(`~/.codex/config.toml` 加 mcp_servers 条目)。
    if provider_looks_like(provider, "minimax") || provider_looks_like(provider, "minimaxi") {
        crate::warn_once_drop_tool("web_search:not-supported-by-minimax-api");
        return vec![];
    }

    // 其他 provider 尚未文档实证,走 drop + warn_once。
    // 用户实地反馈"模型不能直接用 web_search,绕路 MCP 工具/Node Repl 写
    // JS fetch HTML"是预期当前行为(P5 namespace MCP 修复后这条路是通的);
    // 后续逐家移植后会让模型直接走 chat 原生 web search,效率更高。
    crate::warn_once_drop_tool("web_search:provider-not-implemented");
    vec![]
}

/// 扫 outbound tools 数组,看是否含 Kimi 内置 `$web_search`
/// (`type:"builtin_function"` + `function.name == "$web_search"`)。
/// 命中时调用方需要在 body 顶级注入 `thinking:{type:"disabled"}` —— Kimi
/// 文档强制要求(see `docs/web-search-implementation-tracker.md` §2.1.2)。
fn contains_kimi_web_search_tool(tools: &[Value]) -> bool {
    tools.iter().any(|t| {
        t.get("type").and_then(|v| v.as_str()) == Some("builtin_function")
            && t.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                == Some("$web_search")
    })
}

/// 读 `provider.request_options.web_search_enabled`(boolean,默认 false)。
/// 用户必须显式在 codex-app-transfer 配置里标 true 才启用;**默认关闭**
/// 是因为很多 provider(如 MiMo Token Plan 套餐)没开 Web Search Plugin
/// 时,发 web_search 工具会被 400 拒绝。配套 4xx fallback 自动降级
/// (`crate::disable_web_search_for`)防止重复失败。
fn provider_web_search_enabled(provider: &Provider) -> bool {
    provider
        .request_options
        .get("web_search_enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_transfer_registry::Provider;
    use indexmap::IndexMap;

    fn convert(body: Value) -> Value {
        responses_body_to_chat_body(&body).unwrap()
    }

    fn deepseek_provider() -> Provider {
        let mut p = provider("deepseek", "DeepSeek", "https://api.deepseek.com");
        p.models.insert("default".into(), "deepseek-v4-pro".into());
        p.api_format = "openai_chat".into();
        p
    }

    fn minimax_provider() -> Provider {
        let mut p = provider("minimax", "MiniMax", "https://api.minimaxi.com/v1");
        p.models.insert("default".into(), "MiniMax-M2.7".into());
        p.api_format = "openai_chat".into();
        p
    }

    #[test]
    fn deepseek_history_strips_image_blocks_to_text_placeholder() {
        // 真实 Codex CLI history:第 9 条 user 消息含 image_url,DeepSeek 实测
        // 在 deserialize 阶段对 image_url variant 报 400(2026-05-06 实测)。
        // 转换后 image_url 必须不再存在 messages.content 任何块里。
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "input": [
                {"type":"message","role":"user","content":"hi"},
                {"type":"message","role":"user","content":[
                    {"type":"input_text","text":"看这张图"},
                    {"type":"input_image","image_url":"data:image/png;base64,AAA"}
                ]}
            ]
        });
        let p = deepseek_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let messages = out["messages"].as_array().unwrap();
        let serialized = serde_json::to_string(messages).unwrap();
        assert!(
            !serialized.contains("\"image_url\""),
            "DeepSeek 上游不接 image_url,转换后必须不含此 variant\nactual: {serialized}"
        );
        assert!(
            serialized.contains("image omitted"),
            "应当用占位文本替换,而不是直接丢弃,让模型知道历史里曾有图\nactual: {serialized}"
        );
    }

    #[test]
    fn deepseek_input_image_top_level_item_strips_to_text_placeholder() {
        // input_image 作为顶层 item(Codex CLI 当前轮直接贴图)也要被剥
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "input": [
                {"type":"input_image","image_url":"data:image/png;base64,AAA","detail":"low"}
            ]
        });
        let p = deepseek_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let serialized = serde_json::to_string(&out["messages"]).unwrap();
        assert!(!serialized.contains("\"image_url\""));
        assert!(serialized.contains("image omitted"));
    }

    // ── response_format json_schema 降级(基于实测 2026-05-06)─────────
    // - DeepSeek v4-pro:json_schema → 400;json_object → 200(必须降级)
    // - Kimi k2.6:json_schema → 200(不降级)
    // - MiMo v2.5-pro:json_schema → 200(不降级,实测两家都支持)

    fn json_schema_text_config() -> Value {
        json!({
            "format": {
                "type": "json_schema",
                "name": "risk_review",
                "strict": true,
                "schema": {
                    "type":"object",
                    "properties": {
                        "risk_level":{"type":"string","enum":["low","medium","high"]},
                        "outcome":{"type":"string","enum":["allow","deny"]}
                    },
                    "required": ["risk_level","outcome"],
                    "additionalProperties": false,
                }
            }
        })
    }

    #[test]
    fn deepseek_downgrades_json_schema_response_format_to_json_object() {
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "instructions": "Output strict JSON. Required keys: risk_level, outcome.",
            "input": [{"type":"message","role":"user","content":"Risk of ls?"}],
            "text": json_schema_text_config(),
        });
        let p = deepseek_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let rf = &out["response_format"];
        assert_eq!(
            rf["type"], "json_object",
            "DeepSeek 必须把 json_schema 降级为 json_object;实际: {rf}"
        );
        assert!(
            rf.get("json_schema").is_none(),
            "降级后不能保留 json_schema 字段:{rf}"
        );
    }

    #[test]
    fn kimi_keeps_json_schema_response_format_intact() {
        let mut kimi = provider("kimi", "Kimi", "https://api.moonshot.cn/v1");
        kimi.models.insert("default".into(), "kimi-k2.6".into());
        let req = json!({
            "model": "kimi-k2.6",
            "stream": true,
            "instructions": "x",
            "input": [{"type":"message","role":"user","content":"hi"}],
            "text": json_schema_text_config(),
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&kimi)).unwrap();
        let rf = &out["response_format"];
        assert_eq!(rf["type"], "json_schema", "Kimi 应保留 json_schema:{rf}");
        assert_eq!(rf["json_schema"]["name"], "risk_review");
        assert_eq!(rf["json_schema"]["strict"], true);
    }

    #[test]
    fn mimo_keeps_json_schema_response_format_intact() {
        // MiMo 实测两家(PAYG / Token Plan)都支持 json_schema,不能降级
        let mut mimo = provider(
            "xiaomi-mimo",
            "Xiaomi MiMo",
            "https://api.xiaomimimo.com/v1",
        );
        mimo.models.insert("default".into(), "mimo-v2.5-pro".into());
        let req = json!({
            "model": "mimo-v2.5-pro",
            "stream": true,
            "instructions": "x",
            "input": [{"type":"message","role":"user","content":"hi"}],
            "text": json_schema_text_config(),
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&mimo)).unwrap();
        let rf = &out["response_format"];
        assert_eq!(rf["type"], "json_schema", "MiMo 实测支持,不应降级:{rf}");
    }

    #[test]
    fn explicit_supports_json_schema_true_overrides_blacklist() {
        // 用户在 modelCapabilities 显式标 supports_json_schema_response_format: true
        // 即使 base_url 命中黑名单(deepseek)也保留(给未来能力升级预留)。
        let mut p = deepseek_provider();
        p.model_capabilities.insert(
            "deepseek-v4-pro".into(),
            json!({"supports_json_schema_response_format": true}),
        );
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "instructions": "x",
            "input": [{"type":"message","role":"user","content":"hi"}],
            "text": json_schema_text_config(),
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert_eq!(out["response_format"]["type"], "json_schema");
    }

    #[test]
    fn explicit_supports_json_schema_false_forces_downgrade() {
        // 即使 base_url 不在黑名单(例如 Kimi),用户显式标 false 也要降级
        let mut kimi = provider("kimi", "Kimi", "https://api.moonshot.cn/v1");
        kimi.models.insert("default".into(), "kimi-k2.6".into());
        kimi.model_capabilities.insert(
            "kimi-k2.6".into(),
            json!({"supports_json_schema_response_format": false}),
        );
        let req = json!({
            "model": "kimi-k2.6",
            "stream": true,
            "instructions": "x",
            "input": [{"type":"message","role":"user","content":"hi"}],
            "text": json_schema_text_config(),
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&kimi)).unwrap();
        assert_eq!(out["response_format"]["type"], "json_object");
    }

    #[test]
    fn minimax_m2_drops_unsupported_chat_settings() {
        // MiniMax M2.7 OpenAI-compatible chat 对 OpenAI/Codex 的扩展字段会报
        // 400 invalid chat setting (2013)。保留工具相关标准字段,剥掉
        // response_format/reasoning_effort/parallel_tool_calls 等不兼容项。
        let req = json!({
            "model": "MiniMax-M2.7",
            "stream": true,
            "reasoning": {"effort": "high"},
            "parallel_tool_calls": true,
            "store": false,
            "metadata": {"k": "v"},
            "instructions": "Output strict JSON.",
            "input": [{"type":"message","role":"user","content":"hi"}],
            "text": json_schema_text_config(),
            "tool_choice": "auto",
            "tools": [{
                "type":"function",
                "name":"shell",
                "parameters":{"type":"object"}
            }]
        });
        let p = minimax_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert!(out.get("response_format").is_none());
        assert!(out.get("reasoning_effort").is_none());
        assert!(out.get("parallel_tool_calls").is_none());
        assert!(out.get("store").is_none());
        assert!(out.get("metadata").is_none());
        assert!(out.get("tools").is_some(), "MiniMax M2 支持 tool use");
        assert_eq!(out["tool_choice"], "auto");
        assert_eq!(out["reasoning_split"], true);
        assert!(out.get("stream_options").is_none());
        assert!(out["tools"][0]["function"].get("strict").is_none());
    }

    #[test]
    fn minimax_tool_choice_required_is_downgraded_to_auto() {
        let req = json!({
            "model": "MiniMax-M2.7",
            "stream": true,
            "input": "hi",
            "tool_choice": {"type": "required"}
        });
        let p = minimax_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert_eq!(out["tool_choice"], "auto");
    }

    #[test]
    fn minimax_merges_then_converts_system_to_user_prefix() {
        // issue #139 修(2026-05-12):MiniMax /v1/chat/completions 不接受
        // role=system,400 invalid role。先 merge_consecutive_system_messages
        // 合并(instructions + system message 拼一段),再 convert_minimax_system_to_user_prefix
        // 把 role 一次性转 user + content 前 `[System]\n` prefix。
        let req = json!({
            "model": "MiniMax-M2.7",
            "stream": true,
            "instructions": "system one",
            "input": [
                {"type":"message","role":"system","content":"system two"},
                {"type":"message","role":"user","content":"hi"}
            ]
        });
        let p = minimax_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let messages = out["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        // 原 system message → 转 user role
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(
            messages[0]["content"], "[System]\nsystem one\n\nsystem two",
            "合并后的 system 段被转 user role + [System]\\n prefix"
        );
        // 原 user message 不动
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "hi");
    }

    #[test]
    fn minimax_sanitizes_invalid_tool_call_arguments_in_messages() {
        let mut body = json!({
            "model": "MiniMax-M2.7",
            "messages": [
                {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [
                        {
                            "id": "call_bad_1",
                            "type": "function",
                            "function": {"name":"f1", "arguments": ""}
                        },
                        {
                            "id": "call_bad_2",
                            "type": "function",
                            "function": {"name":"f2", "arguments": "{bad-json"}
                        },
                        {
                            "id": "call_ok",
                            "type": "function",
                            "function": {"name":"f3", "arguments": "{\"k\":1}"}
                        }
                    ]
                }
            ]
        })
        .as_object()
        .expect("json object")
        .clone();
        sanitize_minimax_chat_body(&mut body);
        let calls = body["messages"][0]["tool_calls"].as_array().unwrap();
        assert_eq!(calls[0]["function"]["arguments"], "{}");
        assert_eq!(calls[1]["function"]["arguments"], "{}");
        assert_eq!(calls[2]["function"]["arguments"], "{\"k\":1}");
    }

    #[test]
    fn minimax_long_system_split_into_multiple_user_prefix_messages() {
        // issue #139 修:超 max_chars 切片,每片独立 role=user + 标记部分编号
        // `[System part i/N]\n` prefix(silent-failure F4:让模型看出是同一逻辑
        // 段落的连续分片);单段不切则用 `[System]\n`。
        //
        // chatgpt-codex P1 修后,budget 算法减去 prefix 长度:max_chars=50,
        // prefix `[System part i/N]\n` static 16 char + 2*N digit;N=4 时 prefix
        // 长度 16+2 = 18 → budget=32。system 121 char(60+1+60)→ 切 4 段
        // (32+32+32+25)。
        let long_a = "a".repeat(60);
        let long_b = "b".repeat(60);
        let mut body = json!({
            "model": "MiniMax-M2.7",
            "messages": [
                {"role":"system","content": format!("{long_a}\r\n{long_b}")},
                {"role":"user","content":"hi"}
            ]
        })
        .as_object()
        .expect("json object")
        .clone();
        convert_minimax_system_to_user_prefix(&mut body, 50);
        let messages = body["messages"].as_array().unwrap();
        let split_count = messages.len() - 1; // 最后一条是 follow user
        assert!(
            split_count >= 3,
            "121 char 应至少切 3 段,实际 {split_count} 段"
        );
        for (i, msg) in messages.iter().enumerate() {
            assert_eq!(msg["role"], "user", "msg {i} should be user");
            // **chatgpt-codex P1 invariant**:每条 user message(含 prefix)≤ max_chars
            let len = msg["content"].as_str().unwrap().chars().count();
            assert!(
                len <= 50,
                "msg {i} len {len} > max_chars 50,违反 P1 invariant"
            );
        }
        assert_eq!(
            messages.last().unwrap()["content"],
            "hi",
            "last 应是 follow user"
        );
        // 验证 prefix marker + 重组原文(N 由实际切片数决定)
        let mut joined = String::new();
        for i in 0..split_count {
            let content = messages[i]["content"].as_str().unwrap();
            let expected_prefix = format!("[System part {}/{split_count}]\n", i + 1);
            let chunk = content.strip_prefix(&expected_prefix).unwrap_or_else(|| {
                panic!("chunk {i} missing prefix {expected_prefix:?}, got: {content}")
            });
            joined.push_str(chunk);
        }
        assert_eq!(joined, format!("{long_a}\n{long_b}"));
        assert!(!joined.contains('\r'));
    }

    #[test]
    fn minimax_system_content_as_responses_array_form_extracts_text() {
        // silent-failure F2 修:Codex CLI Responses spec content 数组形
        // `[{type:"input_text", text:"..."}]` 必须抽 text 字段,不能 raw JSON stringify
        // 塞 `[System]\n[{"type":"input_text",...}]` 给模型(那会让模型看到 JSON 噪音)
        let mut body = json!({
            "model": "MiniMax-M2.7",
            "messages": [
                {"role":"system","content": [
                    {"type":"input_text","text":"line 1"},
                    {"type":"input_text","text":"line 2"}
                ]},
                {"role":"user","content":"q"}
            ]
        })
        .as_object()
        .expect("json object")
        .clone();
        convert_minimax_system_to_user_prefix(&mut body, 1000);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(
            messages[0]["content"], "[System]\nline 1\nline 2",
            "array-form content 应抽 text join \\n,不能 raw JSON stringify"
        );
    }

    #[test]
    fn minimax_system_content_empty_array_or_no_text_parts_skipped() {
        // silent-failure F2 边界:array 有 parts 但全是 image / 无 text 字段 → skip,
        // 不注入 raw JSON 给模型
        let mut body = json!({
            "model": "MiniMax-M2.7",
            "messages": [
                {"role":"system","content": [
                    {"type":"input_image","image_url":"x"}
                ]},
                {"role":"user","content":"q"}
            ]
        })
        .as_object()
        .expect("json object")
        .clone();
        convert_minimax_system_to_user_prefix(&mut body, 1000);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "image-only system 跳过,只剩 user");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "q");
    }

    #[test]
    fn minimax_chunked_messages_total_length_within_max_chars() {
        // **chatgpt-codex P1 不变量验证**(2026-05-12):chunk + prefix 后**每条
        // emitted user message 的 chars().count() ≤ max_chars**。之前直接
        // split(content, max_chars) 后加 prefix → 每条 ≈ max_chars + 22 char
        // 超限,MiniMax 仍 400。本测试用极端长 content + 小 max_chars 验证。
        let long = "a".repeat(1_000);
        let mut body = json!({
            "model": "MiniMax-M2.7",
            "messages": [
                {"role":"system","content": long.clone()},
                {"role":"user","content":"q"}
            ]
        })
        .as_object()
        .expect("json object")
        .clone();
        const MAX: usize = 100;
        convert_minimax_system_to_user_prefix(&mut body, MAX);
        let messages = body["messages"].as_array().unwrap();
        // 全部 user role,no system
        for (i, msg) in messages.iter().enumerate() {
            assert_eq!(msg["role"], "user", "msg {i} should be user");
            let content_len = msg["content"].as_str().unwrap().chars().count();
            // **关键 invariant**:整条 user message(含 prefix)≤ MAX
            assert!(
                content_len <= MAX,
                "msg {i} content len {} > MAX {}, violates chatgpt-codex P1 invariant",
                content_len,
                MAX,
            );
        }
        // 验证 last user message 不变(role=user 但不是 split chunk)
        let last = messages.last().unwrap();
        assert_eq!(last["content"], "q");
    }

    #[test]
    fn minimax_integration_long_system_through_public_entry() {
        // silent-failure F7:整合 test 走 public entry `responses_body_to_chat_body_for_provider`
        // (含 build_messages_from_input + merge_consecutive_system + sanitize_minimax_chat_body
        // 全链路),覆盖长 system 切片场景。MAX_CHARS=24000,我们用 30000 字符 system 触发切片。
        let long = "x".repeat(30_000);
        let req = json!({
            "model": "MiniMax-M2.7",
            "stream": true,
            "instructions": long.clone(),
            "input": [{"type":"message","role":"user","content":"q"}]
        });
        let p = minimax_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let messages = out["messages"].as_array().unwrap();
        // 至少 2 段 system(30000 / 24000 = 2 切片)+ 1 user → ≥3 messages
        assert!(messages.len() >= 3, "30000 char instructions 应切 ≥2 段");
        // 全部应是 user role(没 system 残留)
        for msg in messages {
            assert_ne!(msg["role"], "system");
        }
        // 第一段含 [System part i/N] prefix
        let first_content = messages[0]["content"].as_str().unwrap();
        assert!(
            first_content.starts_with("[System part 1/"),
            "切片第一段应带 part 1/N marker,got: {}",
            &first_content[..40.min(first_content.len())]
        );
    }

    #[test]
    fn minimax_system_role_completely_eliminated_in_final_body() {
        // issue #139 防御:sanitize 后 messages 数组里**绝不应**有任何 role=system
        // (MiniMax API 直接 400 拒绝 role=system)
        let req = json!({
            "model": "MiniMax-M2.7",
            "stream": true,
            "instructions": "outer system",
            "input": [
                {"type":"message","role":"system","content":"inner system"},
                {"type":"message","role":"user","content":"q"}
            ]
        });
        let p = minimax_provider();
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let messages = out["messages"].as_array().unwrap();
        for msg in messages {
            assert_ne!(
                msg["role"], "system",
                "MiniMax sanitize 后绝不应保留 role=system,违反 MiniMax /v1/chat/completions API"
            );
        }
    }

    #[test]
    fn minimax_empty_system_content_skipped() {
        // 防御:空 system content 不发 raw `[System]\n` 空 user message
        let mut body = json!({
            "model": "MiniMax-M2.7",
            "messages": [
                {"role":"system","content":""},
                {"role":"user","content":"q"}
            ]
        })
        .as_object()
        .expect("json object")
        .clone();
        convert_minimax_system_to_user_prefix(&mut body, 1000);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "空 system 应跳过");
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "q");
    }

    #[test]
    fn minimax_text_01_keeps_response_format() {
        let mut p = provider("minimax", "MiniMax", "https://api.minimaxi.com/v1");
        p.models.insert("default".into(), "MiniMax-Text-01".into());
        let req = json!({
            "model": "MiniMax-Text-01",
            "stream": true,
            "input": "hi",
            "text": json_schema_text_config(),
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert_eq!(out["response_format"]["type"], "json_schema");
    }

    #[test]
    fn kimi_history_keeps_image_blocks_intact() {
        // Kimi(月之暗面)部分模型支持视觉,默认放行 → image_url 必须保留
        let mut kimi = provider("kimi", "Kimi", "https://api.moonshot.cn/v1");
        kimi.models.insert("default".into(), "kimi-k2.6".into());
        let req = json!({
            "model": "kimi-k2.6",
            "stream": true,
            "input": [{
                "type":"message","role":"user","content":[
                    {"type":"input_text","text":"图里是什么"},
                    {"type":"input_image","image_url":"data:image/png;base64,AAA"}
                ]
            }]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&kimi)).unwrap();
        let serialized = serde_json::to_string(&out["messages"]).unwrap();
        assert!(
            serialized.contains("\"image_url\""),
            "Kimi 应保留 image_url"
        );
    }

    // ── ensure_text_part_when_image_present 兜底:MiMo 文档强制要求图存在
    // 时 content 至少有 1 个 text part,否则 400 "Param Incorrect: text is
    // not set"。借鉴 7as0nch/mimo2codex reqToChat.ts:71-79。
    // 对其他 supports_vision provider (Kimi / OpenAI 等) 无副作用,统一处理。

    #[test]
    fn mimo_image_only_message_gets_text_part_appended() {
        // MiMo vision 模型 + 仅 image 的 user 消息(用户粘图未输入文字)→
        // 必须在 content 末尾追加 {type:"text", text:" "} 兜底
        let mut mimo = mimo_provider();
        mimo.models.insert("default".into(), "mimo-v2.5".into());
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{
                "type":"message","role":"user","content":[
                    {"type":"input_image","image_url":"data:image/png;base64,AAA"}
                ]
            }]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&mimo)).unwrap();
        let messages = out["messages"].as_array().unwrap();
        let content = messages[0]["content"].as_array().unwrap();
        assert!(
            content
                .iter()
                .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("text")),
            "兜底 text part 必须存在,否则 MiMo 400 Param Incorrect\nactual: {content:?}"
        );
        assert!(
            content
                .iter()
                .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("image_url")),
            "原 image_url 必须保留\nactual: {content:?}"
        );
    }

    #[test]
    fn mimo_image_with_existing_text_part_unchanged() {
        // 用户既贴了图也输了字 → 原 text part 已存在,不应再追加
        let mut mimo = mimo_provider();
        mimo.models.insert("default".into(), "mimo-v2.5".into());
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{
                "type":"message","role":"user","content":[
                    {"type":"input_text","text":"图里是什么"},
                    {"type":"input_image","image_url":"data:image/png;base64,AAA"}
                ]
            }]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&mimo)).unwrap();
        let messages = out["messages"].as_array().unwrap();
        let content = messages[0]["content"].as_array().unwrap();
        let text_blocks: Vec<&Value> = content
            .iter()
            .filter(|b| b.get("type").and_then(|v| v.as_str()) == Some("text"))
            .collect();
        assert_eq!(
            text_blocks.len(),
            1,
            "已有 text 时不应重复追加,只该有 1 个 text block\nactual: {content:?}"
        );
        assert_eq!(
            text_blocks[0].get("text").and_then(|v| v.as_str()),
            Some("图里是什么"),
            "原 text 内容必须保留,不能被空格 text 覆盖"
        );
    }

    #[test]
    fn kimi_image_only_message_also_gets_text_part_appended() {
        // 兜底统一对所有 supports_vision provider 应用(避免 per-provider
        // 分支),Kimi 也加。空格 text 对 Kimi 无副作用 — 验证不会影响其
        // image_url 保留。
        let mut kimi = provider("kimi", "Kimi", "https://api.moonshot.cn/v1");
        kimi.models.insert("default".into(), "kimi-k2.6".into());
        let req = json!({
            "model": "kimi-k2.6",
            "stream": true,
            "input": [{
                "type":"message","role":"user","content":[
                    {"type":"input_image","image_url":"data:image/png;base64,AAA"}
                ]
            }]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&kimi)).unwrap();
        let content = out["messages"][0]["content"].as_array().unwrap();
        assert!(
            content
                .iter()
                .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("text")),
            "Kimi 也走兜底统一处理(无副作用)"
        );
        assert!(
            content
                .iter()
                .any(|b| b.get("type").and_then(|v| v.as_str()) == Some("image_url")),
            "image_url 必须保留"
        );
    }

    #[test]
    fn text_only_provider_image_only_still_strips_to_placeholder() {
        // 非 supports_vision provider(deepseek-v4-pro)+ 仅 image →
        // 走 strip 路径,不该被 ensure_text_part 兜底干扰(strip 已自带
        // 占位文本 "image omitted")
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "input": [{
                "type":"message","role":"user","content":[
                    {"type":"input_image","image_url":"data:image/png;base64,AAA"}
                ]
            }]
        });
        let out =
            responses_body_to_chat_body_for_provider(&req, Some(&deepseek_provider())).unwrap();
        let serialized = serde_json::to_string(&out["messages"]).unwrap();
        assert!(
            !serialized.contains("\"image_url\""),
            "DeepSeek 必须 strip 掉 image_url"
        );
        assert!(
            serialized.contains("image omitted"),
            "占位文本必须存在(strip 路径,而非 ensure_text 兜底空格)"
        );
        // ensure_text_part 不应被调用(走的是 strip 分支)
        assert!(
            !serialized.contains(r#""text":" ""#),
            "走 strip 分支时,不应额外追加空格 text"
        );
    }

    // ── namespace 工具递归展平(借鉴 7as0nch/mimo2codex reqToChat.ts:232-250)
    // Codex CLI 用 type:"namespace" 包装 MCP server 工具集,内层是具体
    // type:"function"。第三方 chat completions provider 不认 namespace,必须
    // 展平为顶级 function 数组。实测每轮 218 个 namespace × 88 内层 function
    // 被旧版 `_ => None` 整个 drop,模型完全看不到 MCP 具体 tools。

    #[test]
    fn namespace_with_two_inner_functions_flattens_to_two_function_tools() {
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type": "namespace", "name": "mcp__cloudflare_docs__",
                 "description": "Tools in the mcp__cloudflare_docs__ namespace.",
                 "tools": [
                    {"type":"function","name":"migrate_pages_to_workers_guide",
                     "description":"Read this guide before migrating.",
                     "parameters":{"type":"object","properties":{},"additionalProperties":false},
                     "strict":false},
                    {"type":"function","name":"search_cloudflare_documentation",
                     "description":"Search the Cloudflare documentation.",
                     "parameters":{"type":"object","properties":{
                        "query":{"type":"string"}},"required":["query"]},
                     "strict":false}
                 ]}
            ]
        });
        let out = convert(req);
        let tools = out["tools"].as_array().expect("tools array present");
        assert_eq!(
            tools.len(),
            2,
            "namespace 内层 2 个 function 必须展平为 2 个顶级 tool"
        );
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap_or(""))
            .collect();
        assert!(names.contains(&"migrate_pages_to_workers_guide"));
        assert!(names.contains(&"search_cloudflare_documentation"));
        // namespace 包装的 name (mcp__cloudflare_docs__) 不该作为顶级工具出现
        assert!(
            !names.contains(&"mcp__cloudflare_docs__"),
            "namespace 包装名不该泄漏成 tool name"
        );
    }

    #[test]
    fn namespace_alongside_top_level_function_both_kept() {
        // 实测真实场景:tools 数组同时含顶级 function + namespace 包,展平
        // 后总数 = 顶级 function 数 + 所有 namespace 内层 function 数
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"function","name":"shell",
                 "description":"Run shell command.",
                 "parameters":{"type":"object","properties":{}}},
                {"type":"namespace","name":"mcp__notion__","tools":[
                    {"type":"function","name":"notion_search","description":"",
                     "parameters":{"type":"object","properties":{}}},
                    {"type":"function","name":"notion_create_pages","description":"",
                     "parameters":{"type":"object","properties":{}}}
                ]}
            ]
        });
        let out = convert(req);
        let names: Vec<&str> = out["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"shell"));
        assert!(names.contains(&"notion_search"));
        assert!(names.contains(&"notion_create_pages"));
    }

    #[test]
    fn namespace_with_empty_tools_array_silently_dropped() {
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"namespace","name":"mcp__empty__","tools": []}
            ]
        });
        let out = convert(req);
        // 空 namespace 不该出现在 tools 数组里;若没其他 tools,整个 tools key
        // 不应进 result(对齐"chat_tools.is_empty() 时 skip insert"逻辑)。
        assert!(out.get("tools").is_none() || out["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn namespace_missing_tools_field_silently_dropped() {
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"namespace","name":"mcp__broken__"}  // 无 tools 字段
            ]
        });
        let out = convert(req);
        assert!(out.get("tools").is_none() || out["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn nested_namespace_inside_namespace_recursively_flattens() {
        // 边界:虽然实测 Codex CLI 当前不嵌套 namespace,但实现走的是递归
        // flat_map,理应正确处理。future-safe 验证。
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"namespace","name":"outer","tools":[
                    {"type":"namespace","name":"inner","tools":[
                        {"type":"function","name":"deep_tool","description":"",
                         "parameters":{"type":"object","properties":{}}}
                    ]},
                    {"type":"function","name":"sibling","description":"",
                     "parameters":{"type":"object","properties":{}}}
                ]}
            ]
        });
        let out = convert(req);
        let names: Vec<&str> = out["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(names, vec!["deep_tool", "sibling"]);
    }

    #[test]
    fn unknown_tool_type_dropped_via_warn_once_path_does_not_panic() {
        // web_search / file_search / code_interpreter / image_generation 等
        // Responses 专属 server-side 工具在第三方 chat 端无等价,继续 drop。
        // 验证:不 panic,不出现在 outbound,与已有 type:"function" 共存。
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search","external_web_access":true,
                 "search_content_types":["text","image"]},
                {"type":"file_search","vector_store_ids":["xx"]},
                {"type":"function","name":"keep_me","description":"",
                 "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = convert(req);
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "只 keep_me 这个 function 应保留");
        assert_eq!(tools[0]["function"]["name"], "keep_me");
    }

    // ── web_search 工具 per-provider 适配 — MiMo 阶段 ─────────────────
    // Codex.app 入站默认每轮发 `{type:"web_search", external_web_access:true,
    // search_content_types:["text","image"]}`(实测 dump),代理把这个统一
    // 形态转成各上游 chat API 真实支持的形态。本批仅 MiMo 实施(1:1 复刻
    // mimo2codex `reqToChat.ts:196-209`),Kimi/DeepSeek/MiniMax 等留 follow-up
    // (逐家文档实证后跟进,见 `docs/web-search-implementation-tracker.md`)。

    /// MiMo provider 用于 web_search 测试 — 显式 enable Web Search Plugin。
    /// A 层默认 false,测试需要显式开才会触发转换。
    fn mimo_provider_with_web_search() -> Provider {
        let mut p = mimo_provider();
        p.models.insert("default".into(), "mimo-v2.5".into());
        p.request_options
            .insert("web_search_enabled".into(), json!(true));
        p
    }

    #[test]
    fn mimo_web_search_converted_to_native_schema_with_user_location() {
        let p = mimo_provider_with_web_search();
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"搜索 X 最新进展"}],
            "tools": [
                {
                    "type": "web_search",
                    "external_web_access": true,
                    "search_content_types": ["text", "image"],
                    "user_location": {
                        "type": "approximate",
                        "country": "CN",
                        "city": "Shanghai"
                    },
                    "max_keyword": 5,
                    "force_search": true,
                    "limit": 10
                }
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        let tool = &tools[0];
        assert_eq!(
            tool["type"], "web_search",
            "MiMo chat 端原生 type:web_search"
        );
        assert_eq!(tool["user_location"]["country"], "CN");
        assert_eq!(tool["user_location"]["city"], "Shanghai");
        assert_eq!(tool["max_keyword"], 5);
        assert_eq!(tool["force_search"], true);
        assert_eq!(tool["limit"], 10);
        // OpenAI 的 external_web_access / search_content_types 在 MiMo 无等价,silent drop
        assert!(
            tool.get("external_web_access").is_none(),
            "external_web_access 在 MiMo 无等价,必须 silent drop"
        );
        assert!(
            tool.get("search_content_types").is_none(),
            "search_content_types 在 MiMo 无等价,必须 silent drop"
        );
    }

    #[test]
    fn mimo_web_search_with_minimal_fields_outputs_minimal_tool() {
        // 用户没传 user_location / max_keyword 等字段时,只输出 type:"web_search"
        let p = mimo_provider_with_web_search();
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search", "external_web_access": true, "search_content_types": ["text"]}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let keys: Vec<&String> = tools[0].as_object().unwrap().keys().collect();
        assert_eq!(keys, vec![&"type".to_string()], "无可选字段时只剩 type");
        assert_eq!(tools[0]["type"], "web_search");
    }

    #[test]
    fn mimo_web_search_preview_alias_handled_same_as_web_search() {
        // Codex.app 历史上有过 web_search_preview / web_search 两种 type,
        // mimo2codex `reqToChat.ts:196` 同样处理,我们也照抄。
        let p = mimo_provider_with_web_search();
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search_preview"}]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert_eq!(out["tools"][0]["type"], "web_search");
    }

    #[test]
    fn non_mimo_provider_web_search_dropped_via_warn_once() {
        // Kimi / DeepSeek / MiniMax 等 provider 暂未文档实证,走 drop + warn_once。
        // 用户实际会看到模型走 P5 修通的 namespace MCP 工具(如 Node Repl)绕路
        // 联网搜索;后续逐家文档实证后再加映射。
        let mut kimi = provider("kimi", "Kimi", "https://api.moonshot.cn/v1");
        kimi.models.insert("default".into(), "kimi-k2.6".into());
        let req = json!({
            "model": "kimi-k2.6",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search", "external_web_access": true, "search_content_types": ["text"]},
                {"type":"function", "name":"keep_me", "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&kimi)).unwrap();
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1, "Kimi 暂未实施,web_search drop 只剩 keep_me");
        assert_eq!(tools[0]["function"]["name"], "keep_me");
    }

    #[test]
    fn web_search_with_no_provider_context_dropped() {
        // 极端情况:没有 provider 上下文(应该不发生,resolver 必填),
        // 安全 drop 不 panic
        let req = json!({
            "model": "any",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out = convert(req);
        // 没 provider 时整个 web_search drop,tools 字段不存在(empty 数组不写入)
        assert!(out.get("tools").is_none() || out["tools"].as_array().unwrap().is_empty());
    }

    // ── A 层(provider 配置开关)──
    // `request_options.web_search_enabled` 默认 false,用户必须显式标 true。
    // 默认关闭原因:很多 provider(如 MiMo Token Plan)没开 plugin 时发
    // web_search 工具会触发上游 400。

    #[test]
    fn mimo_provider_without_web_search_enabled_drops_web_search_by_default() {
        // 默认状态:mimo_provider() 没设 web_search_enabled → 视为 false → drop
        let mut p = mimo_provider();
        p.models.insert("default".into(), "mimo-v2.5".into());
        // 故意不设 web_search_enabled
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search", "external_web_access": true},
                {"type":"function", "name":"keep_me", "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            1,
            "默认 web_search_enabled=false → web_search 被 A 层 drop"
        );
        assert_eq!(tools[0]["function"]["name"], "keep_me");
    }

    #[test]
    fn mimo_provider_with_explicit_web_search_enabled_false_drops_web_search() {
        // 显式标 false 跟没设效果一致 — 都触发 A 层 drop
        let mut p = mimo_provider();
        p.models.insert("default".into(), "mimo-v2.5".into());
        p.request_options
            .insert("web_search_enabled".into(), json!(false));
        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert!(out.get("tools").is_none() || out["tools"].as_array().unwrap().is_empty());
    }

    // ── B 层(运行时自动 disable cache)──
    // `crate::disable_web_search_for(provider_id)` 后,即使配置 web_search_enabled=true,
    // 同 provider id 后续转换也立即 drop。模拟 forward.rs 4xx fallback 后的行为。

    #[test]
    fn b_layer_runtime_disable_blocks_subsequent_web_search_conversion() {
        let mut p = mimo_provider_with_web_search();
        p.id = "mimo-runtime-disable-test".into();
        // 模拟 forward.rs 4xx fallback 调用
        crate::disable_web_search_for(&p.id);

        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search"},
                {"type":"function", "name":"keep_me", "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            1,
            "运行时 disable cache 命中 → web_search 被 B 层 drop,只剩 keep_me"
        );
        assert_eq!(tools[0]["function"]["name"], "keep_me");
        assert!(crate::is_web_search_disabled_for(&p.id));
    }

    #[test]
    fn b_layer_runtime_disable_only_affects_targeted_provider_id() {
        // disable provider A 不影响 provider B(各自 cache 隔离)
        let mut a = mimo_provider_with_web_search();
        a.id = "mimo-disable-a".into();
        let mut b = mimo_provider_with_web_search();
        b.id = "mimo-untouched-b".into();
        crate::disable_web_search_for(&a.id);

        let req = json!({
            "model": "mimo-v2.5",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out_b = responses_body_to_chat_body_for_provider(&req, Some(&b)).unwrap();
        // b 的 web_search_enabled=true 且没被 disable,正常转换
        assert_eq!(out_b["tools"][0]["type"], "web_search");
    }

    // ── Kimi (Moonshot) web_search builtin_function 映射 ──
    // 来源:WebFetch `platform.kimi.ai/docs/guide/use-web-search` 真原文实证。
    // 1:1 复刻官方文档:tools 形态固定 `{type:"builtin_function", function:{name:"$web_search"}}`,
    // 强制配套 `thinking:{type:"disabled"}` 顶级字段(Kimi 文档明确强制)。

    fn kimi_provider_with_web_search() -> Provider {
        let mut p = provider(
            "kimi-for-coding",
            "Kimi For Coding",
            "https://api.kimi.com/coding/v1",
        );
        p.models.insert("default".into(), "kimi-for-coding".into());
        p.api_format = "openai_chat".into();
        p.request_options
            .insert("web_search_enabled".into(), json!(true));
        p
    }

    fn moonshot_provider_with_web_search() -> Provider {
        let mut p = provider("moonshot", "Moonshot", "https://api.moonshot.cn/v1");
        p.models.insert("default".into(), "kimi-k2.6".into());
        p.api_format = "openai_chat".into();
        p.request_options
            .insert("web_search_enabled".into(), json!(true));
        p
    }

    #[test]
    fn kimi_web_search_outputs_builtin_function_with_dollar_prefix_name() {
        let p = kimi_provider_with_web_search();
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"搜索 X"}],
            "tools": [
                {
                    "type": "web_search",
                    "external_web_access": true,
                    "search_content_types": ["text", "image"],
                    "user_location": {"country": "CN"},
                    "max_keyword": 5
                }
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().expect("tools array");
        assert_eq!(tools.len(), 1);
        // Kimi 形态:固定 builtin_function + $web_search,**不透传任何子字段**
        assert_eq!(tools[0]["type"], "builtin_function");
        assert_eq!(tools[0]["function"]["name"], "$web_search");
        // OpenAI 字段全部 silent drop(Kimi 文档明确只要 type + function.name)
        assert!(tools[0].get("user_location").is_none());
        assert!(tools[0].get("max_keyword").is_none());
        assert!(tools[0].get("external_web_access").is_none());
        assert!(tools[0].get("search_content_types").is_none());
    }

    #[test]
    fn kimi_web_search_force_injects_thinking_disabled_top_level_field() {
        let p = kimi_provider_with_web_search();
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        // Kimi 文档强制:`thinking:{type:"disabled"}` 顶级字段必填
        assert_eq!(
            out["thinking"],
            json!({"type": "disabled"}),
            "Kimi $web_search 必须配套 thinking disabled(官方文档强制)"
        );
    }

    #[test]
    fn moonshot_provider_uses_same_kimi_web_search_form() {
        // moonshot.cn / kimi.ai 同公司,provider_looks_like("moonshot") 同样命中
        let p = moonshot_provider_with_web_search();
        let req = json!({
            "model": "kimi-k2.6",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert_eq!(out["tools"][0]["type"], "builtin_function");
        assert_eq!(out["tools"][0]["function"]["name"], "$web_search");
        assert_eq!(out["thinking"], json!({"type": "disabled"}));
    }

    #[test]
    fn kimi_without_web_search_enabled_does_not_inject_thinking() {
        // 未启用 web_search 时不该强制 disable thinking(用户原 thinking 配置不变)
        let mut p = provider(
            "kimi-for-coding",
            "Kimi For Coding",
            "https://api.kimi.com/coding/v1",
        );
        p.models.insert("default".into(), "kimi-for-coding".into());
        // 故意不设 web_search_enabled
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search"},
                {"type":"function", "name":"shell", "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        // web_search 被 A 层 drop(默认关),不该注入 thinking disabled
        assert!(
            out.get("thinking").is_none(),
            "未启用 web_search 时不该注入 thinking disabled,实际: {:?}",
            out.get("thinking")
        );
        // shell function 仍然保留
        assert_eq!(out["tools"][0]["function"]["name"], "shell");
    }

    #[test]
    fn kimi_web_search_b_layer_runtime_disable_skips_thinking_injection() {
        // B 层 cache 命中(运行时已 disable)→ web_search drop → 不该注入 thinking
        let mut p = kimi_provider_with_web_search();
        p.id = "kimi-runtime-disabled".into();
        crate::disable_web_search_for(&p.id);
        let req = json!({
            "model": "kimi-for-coding",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert!(
            out.get("thinking").is_none(),
            "B 层 cache disable 后 web_search drop,thinking 不该注入"
        );
    }

    // ── DeepSeek web_search drop(文档实证不支持)──
    // 来源:WebFetch `api-docs.deepseek.com/api/create-chat-completion` 真原文
    // (2026-05-09):"Currently, only `function` is supported." DeepSeek chat
    // completions tools 数组只接受 type:"function",无任何 server-side web 搜索。

    #[test]
    fn deepseek_web_search_dropped_with_explicit_warn_key() {
        // DeepSeek 即使 web_search_enabled=true 也 drop(API 不支持)
        let mut p = deepseek_provider();
        p.request_options
            .insert("web_search_enabled".into(), json!(true));
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search"},
                {"type":"function", "name":"keep_me", "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            1,
            "DeepSeek API 不支持 web_search,只剩 keep_me function"
        );
        assert_eq!(tools[0]["function"]["name"], "keep_me");
        // DeepSeek 不应触发 Kimi thinking 注入(它跟 thinking-disabled 路径无关)
        assert!(out.get("thinking").is_none());
    }

    // ── MiniMax web_search drop(文档实证不支持)──
    // 来源:WebFetch `platform.minimaxi.com/docs/api-reference/` + liteLLM
    // MiniMax provider 文档(2026-05-09):MiniMax chat completions tools 只接受
    // type:"function",无内置 web_search;web_search 仅作 Token Plan MCP 工具存在。

    #[test]
    fn minimax_web_search_dropped_with_explicit_warn_key() {
        let mut p = minimax_provider();
        p.request_options
            .insert("web_search_enabled".into(), json!(true));
        let req = json!({
            "model": "MiniMax-M2.7",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [
                {"type":"web_search"},
                {"type":"function", "name":"keep_me", "parameters":{"type":"object","properties":{}}}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            1,
            "MiniMax chat API 不支持 web_search,只剩 keep_me function"
        );
        assert_eq!(tools[0]["function"]["name"], "keep_me");
        // MiniMax 不应触发 Kimi thinking 注入(它跟 thinking-disabled 路径无关)
        assert!(out.get("thinking").is_none());
    }

    #[test]
    fn deepseek_web_search_drop_independent_of_web_search_enabled_flag() {
        // 即使用户显式标 web_search_enabled=false / 不标,DeepSeek 都 drop
        // (其实只是 DeepSeek 不支持的硬实事,跟 A 层无关)
        let p = deepseek_provider(); // 默认未标 web_search_enabled
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"hi"}],
            "tools": [{"type":"web_search"}]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert!(
            out.get("tools").is_none() || out["tools"].as_array().unwrap().is_empty(),
            "DeepSeek 默认未启用 web_search 时,A 层先 drop(走 disabled-by-config 路径)"
        );
    }

    #[test]
    fn explicit_supports_vision_true_overrides_text_only_blacklist() {
        // 用户在 modelCapabilities 显式标 supports_vision: true → 即使模型
        // 命中黑名单(deepseek-v4-pro)也保留 image_url。给未来视觉版预留口子。
        let mut deepseek_with_vision = deepseek_provider();
        deepseek_with_vision
            .model_capabilities
            .insert("deepseek-v4-pro".into(), json!({"supports_vision": true}));
        let req = json!({
            "model": "deepseek-v4-pro",
            "stream": true,
            "input": [{
                "type":"input_image","image_url":"data:image/png;base64,AAA"
            }]
        });
        let out =
            responses_body_to_chat_body_for_provider(&req, Some(&deepseek_with_vision)).unwrap();
        let serialized = serde_json::to_string(&out["messages"]).unwrap();
        assert!(serialized.contains("\"image_url\""));
    }

    // ── vision 白名单的模型级 granularity 验证(2026-05-07 实测覆盖所有 5 接入 provider)──
    //
    // 旧版 provider-id 子串黑名单(["deepseek","xiaomi","mimo","qwen3.6"])会:
    // - 误杀:Mimo 的 mimo-v2-omni / mimo-v2-flash / mimo-v2.5(实测均支持视觉)
    // - 漏杀:Moonshot 的 moonshot-v1-{8k,32k,128k}(实测 400 "Image input not supported")
    //
    // 新版按**请求 body 的 model**精确匹配模型名黑名单。

    fn moonshot_provider() -> Provider {
        let mut p = provider("moonshot", "Moonshot", "https://api.moonshot.cn/v1");
        p.models.insert("default".into(), "kimi-k2.6".into());
        p.api_format = "openai_chat".into();
        p
    }

    fn mimo_provider() -> Provider {
        let mut p = provider(
            "xiaomi-mimo",
            "Xiaomi MiMo",
            "https://api.xiaomimimo.com/v1",
        );
        p.models.insert("default".into(), "mimo-v2.5-pro".into());
        p.api_format = "openai_chat".into();
        p
    }

    fn vision_request_for(model: &str) -> Value {
        json!({
            "model": model,
            "stream": true,
            "input": [{"type":"input_image","image_url":"data:image/png;base64,AAA"}]
        })
    }

    fn image_url_kept(req: &Value, p: &Provider) -> bool {
        let out = responses_body_to_chat_body_for_provider(req, Some(p)).unwrap();
        serde_json::to_string(&out["messages"])
            .unwrap()
            .contains("\"image_url\"")
    }

    #[test]
    fn vision_blacklist_blocks_deepseek_v4_pro() {
        let req = vision_request_for("deepseek-v4-pro");
        assert!(!image_url_kept(&req, &deepseek_provider()));
    }

    #[test]
    fn vision_blacklist_blocks_deepseek_v4_flash() {
        let req = vision_request_for("deepseek-v4-flash");
        let mut p = deepseek_provider();
        p.models
            .insert("default".into(), "deepseek-v4-flash".into());
        assert!(!image_url_kept(&req, &p));
    }

    #[test]
    fn vision_blacklist_blocks_moonshot_v1_non_preview_models() {
        // moonshot-v1-{8k,32k,128k}/auto 实测 400 "Image input not supported"
        for model in [
            "moonshot-v1-8k",
            "moonshot-v1-32k",
            "moonshot-v1-128k",
            "moonshot-v1-auto",
        ] {
            let req = vision_request_for(model);
            let mut p = moonshot_provider();
            p.models.insert("default".into(), model.into());
            assert!(
                !image_url_kept(&req, &p),
                "{model} 实测纯文本,必须 strip image_url"
            );
        }
    }

    #[test]
    fn vision_whitelist_keeps_moonshot_vision_preview_variants() {
        // moonshot-v1-{8k,32k,128k}-vision-preview 实测 SAW_RED
        for model in [
            "moonshot-v1-8k-vision-preview",
            "moonshot-v1-32k-vision-preview",
            "moonshot-v1-128k-vision-preview",
        ] {
            let req = vision_request_for(model);
            let mut p = moonshot_provider();
            p.models.insert("default".into(), model.into());
            assert!(
                image_url_kept(&req, &p),
                "{model} 实测支持视觉,必须保留 image_url"
            );
        }
    }

    #[test]
    fn vision_whitelist_keeps_kimi_k2_models() {
        // kimi-k2.5 / kimi-k2.6 实测 SAW_RED + 官方 vision guide 列出 k2.6
        for model in ["kimi-k2.5", "kimi-k2.6"] {
            let req = vision_request_for(model);
            let mut p = moonshot_provider();
            p.models.insert("default".into(), model.into());
            assert!(image_url_kept(&req, &p), "{model} 实测支持视觉");
        }
    }

    #[test]
    fn vision_whitelist_keeps_kimi_for_coding() {
        // 实测意外:kimi-for-coding 居然支持视觉(SAW_RED)
        let req = vision_request_for("kimi-for-coding");
        let mut p = provider("kimi-code", "Kimi Code", "https://api.kimi.com/coding/v1");
        p.models.insert("default".into(), "kimi-for-coding".into());
        assert!(image_url_kept(&req, &p));
    }

    #[test]
    fn vision_whitelist_keeps_mimo_omni_flash_2_5() {
        // mimo-v2-omni / mimo-v2-flash / mimo-v2.5 实测 SAW_RED
        for model in ["mimo-v2-omni", "mimo-v2-flash", "mimo-v2.5"] {
            let req = vision_request_for(model);
            let mut p = mimo_provider();
            p.models.insert("default".into(), model.into());
            assert!(
                image_url_kept(&req, &p),
                "{model} 实测支持视觉,旧版子串黑名单(\"mimo\")会误杀"
            );
        }
    }

    #[test]
    fn vision_blacklist_blocks_mimo_v2_pro_and_v2_5_pro() {
        // mimo-v2-pro / mimo-v2.5-pro 实测响应 "I don't see any image attached"
        for model in ["mimo-v2-pro", "mimo-v2.5-pro"] {
            let req = vision_request_for(model);
            let mut p = mimo_provider();
            p.models.insert("default".into(), model.into());
            assert!(!image_url_kept(&req, &p), "{model} 实测纯文本");
        }
    }

    #[test]
    fn vision_check_uses_body_model_not_provider_default() {
        // 关键:provider.default = "kimi-k2.6"(支持视觉),但 body 实际请求
        // moonshot-v1-8k(纯文本)→ 必须按 body model 判定,strip 图。
        // 旧版 provider_supports_vision(provider) 只看 default_model 会误判。
        let mut p = moonshot_provider();
        p.models.insert("default".into(), "kimi-k2.6".into());
        let req = vision_request_for("moonshot-v1-8k");
        assert!(
            !image_url_kept(&req, &p),
            "body.model=moonshot-v1-8k 必须当前请求级 strip,与 default 无关"
        );
    }

    #[test]
    fn vision_unknown_model_defaults_to_supported() {
        // 未在黑名单的模型默认放行(覆盖 OpenAI gpt-4o / 新接入 vision provider)
        let req = vision_request_for("gpt-4o");
        let mut p = provider("openai", "OpenAI", "https://api.openai.com/v1");
        p.models.insert("default".into(), "gpt-4o".into());
        assert!(image_url_kept(&req, &p));
    }

    #[test]
    fn vision_explicit_capability_overrides_blacklist_for_per_model() {
        // 用户在 modelCapabilities 显式标 supports_vision = true,即使该模型
        // 在硬编码黑名单(mimo-v2-pro)里也放行。给"我知道这是视觉版升级"留口子。
        let mut p = mimo_provider();
        p.model_capabilities
            .insert("mimo-v2-pro".into(), json!({"supports_vision": true}));
        let req = vision_request_for("mimo-v2-pro");
        assert!(image_url_kept(&req, &p));
    }

    #[test]
    fn vision_explicit_capability_false_overrides_default_pass() {
        // 反向:模型不在黑名单(默认放行),但用户标 supports_vision = false
        // → 必须 strip。给"我知道这上游临时挂了 vision"留口子。
        let mut p = provider("custom", "Custom", "https://api.custom.example/v1");
        p.models.insert("default".into(), "custom-text".into());
        p.model_capabilities
            .insert("custom-text".into(), json!({"supports_vision": false}));
        let req = vision_request_for("custom-text");
        assert!(!image_url_kept(&req, &p));
    }

    #[test]
    fn vision_falls_back_to_default_model_when_body_omits_model() {
        // codex-connector P1 review (2026-05-07 PR #43) 指出:旧改法在 body
        // 缺 model 字段时直接 return true,DeepSeek 这类 text-only provider
        // 一旦 model 缺失就让 image_url 透传 → 触发原本要修的 400 unknown
        // variant 失败。新版必须 fallback 到 provider.models["default"]。
        let p = deepseek_provider(); // default = "deepseek-v4-pro"
        let req = json!({
            // 故意不写 "model" 字段,模拟某些 conversion path 的合法形态
            "stream": true,
            "input": [
                {"type":"input_image","image_url":"data:image/png;base64,AAA"}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        let serialized = serde_json::to_string(&out["messages"]).unwrap();
        assert!(
            !serialized.contains("\"image_url\""),
            "body 缺 model + default=deepseek-v4-pro → 必须按 default 命中黑名单 strip"
        );
        assert!(serialized.contains("image omitted"), "应该用占位文本替换");
    }

    #[test]
    fn vision_falls_back_to_default_model_for_explicit_capability_too() {
        // body 缺 model,但 default 在 modelCapabilities 标了 supports_vision = false
        // → 同样要 strip,而不是默认放行。
        let mut p = provider("custom", "Custom", "https://api.custom.example/v1");
        p.models.insert("default".into(), "future-text-v1".into());
        p.model_capabilities
            .insert("future-text-v1".into(), json!({"supports_vision": false}));
        let req = json!({
            "stream": true,
            "input": [
                {"type":"input_image","image_url":"data:image/png;base64,AAA"}
            ]
        });
        let out = responses_body_to_chat_body_for_provider(&req, Some(&p)).unwrap();
        assert!(!serde_json::to_string(&out["messages"])
            .unwrap()
            .contains("\"image_url\""));
    }

    #[test]
    fn empty_input_no_session_cache_helper_returns_empty_messages() {
        // 底层 helper `responses_body_to_chat_body`(不传 session_cache)的契约:
        // 没有 session_cache 时,根本不进 cache 查询路径,纯按当前 input 拼;
        // input 空就空 — 这条路径只服务于工具/测试场景,生产代理永远传
        // `Some(global_response_session_cache())`,见生产路径测试。
        let req = json!({
            "model": "x",
            "stream": true,
            "previous_response_id": "resp_unknown_to_cache",
            "tools": [{"type":"function","name":"shell","parameters":{"type":"object"}}],
            "input": []
        });
        let out = responses_body_to_chat_body(&req).expect("无 session_cache 路径不报错");
        let msgs = out["messages"].as_array().expect("messages 字段必须存在");
        assert!(msgs.is_empty(), "无 session_cache 时纯按 input 拼");
    }

    #[test]
    fn cache_miss_with_empty_input_returns_previous_response_not_found() {
        // 关键回归(2026-05-08):生产路径(传 session_cache),Codex CLI 用旧
        // previous_response_id 续轮(代理重启 / TTL 过期 / LRU 淘汰),但当前
        // input 为空 → 没有任何上下文可发上游 → 返回 OpenAI 标准
        // PreviousResponseNotFound,proxy IntoResponse 转 HTTP 400 +
        // `code: "previous_response_not_found"`,Codex CLI fail-fast 不重试。
        //
        // 历史:2026-05-06 ~ 2026-05-08 期间代码放行 messages:[] 给上游想触发
        // Codex 重试,但实测 Codex CLI `should_retry` 对 400 直接 fail-fast
        // (`codex-rs/codex-client/src/retry.rs`),只对 5xx + transport timeout
        // 重试 → 旧策略既不能修复,又额外引入上游 RTT(实测 Kimi 19s+)。
        let cache = ResponseSessionCache::new(8, std::time::Duration::from_secs(60));
        let req = json!({
            "model": "x",
            "stream": true,
            "previous_response_id": "resp_unknown_to_cache",
            "input": []
        });
        let err = responses_body_to_chat_body_for_provider_with_session(&req, None, Some(&cache))
            .err()
            .expect("cache miss + empty input 必须报错");
        match err {
            AdapterError::PreviousResponseNotFound {
                previous_response_id,
            } => {
                assert_eq!(previous_response_id, "resp_unknown_to_cache");
            }
            other => panic!("预期 PreviousResponseNotFound,实际 {other:?}"),
        }
    }

    #[test]
    fn cache_miss_with_nonempty_input_falls_back_to_current_only() {
        // cache miss 但 input 非空 → 保留旧降级:丢 previous_response_id,只用
        // 当前 input。这条路径不报错(模型可能丢上下文,但至少能继续对话),
        // 跟 PreviousResponseNotFound 路径区分清楚。
        let cache = ResponseSessionCache::new(8, std::time::Duration::from_secs(60));
        let req = json!({
            "model": "x",
            "stream": true,
            "previous_response_id": "resp_unknown_to_cache",
            "input": [{"type":"message","role":"user","content":"hi"}]
        });
        let out = responses_body_to_chat_body_for_provider_with_session(&req, None, Some(&cache))
            .expect("cache miss 但 input 非空 → 降级,不报错");
        let msgs = out.body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn empty_input_but_with_instructions_passes_through() {
        // 只要有 instructions(system 头),messages 就非空,正常往上游发。
        let req = json!({
            "model": "x",
            "stream": true,
            "instructions": "You are Codex.",
            "input": []
        });
        let out = responses_body_to_chat_body(&req).expect("应当通过");
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "system");
    }

    fn provider(id: &str, name: &str, base_url: &str) -> Provider {
        Provider {
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            auth_scheme: "bearer".into(),
            api_format: "responses".into(),
            api_key: "sk-test".into(),
            models: IndexMap::new(),
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    fn deepseek_thinking_provider() -> Provider {
        let mut p = provider("deepseek", "DeepSeek V4 Pro", "https://api.deepseek.com/v1");
        p.request_options.insert(
            "chat".into(),
            json!({
                "thinking": {"type": "enabled"},
                "reasoning_effort": "max",
            }),
        );
        p
    }

    #[test]
    fn string_input_becomes_single_user_message() {
        let out = convert(json!({
            "model": "x",
            "input": "hello"
        }));
        assert_eq!(out["model"], "x");
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hello");
        // stream 默认 false,但 stream 字段总会被设上
        assert_eq!(out["stream"], false);
        assert!(out.get("stream_options").is_none());
    }

    #[test]
    fn instructions_prepended_as_system_message() {
        let out = convert(json!({
            "model": "x",
            "instructions": "Be concise.",
            "input": "hi"
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "Be concise.");
        assert_eq!(msgs[1]["role"], "user");
    }

    #[test]
    fn empty_instructions_is_skipped() {
        let out = convert(json!({
            "instructions": "   ",
            "input": "hi"
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn array_input_message_item_passthrough() {
        let out = convert(json!({
            "input": [
                {"type": "message", "role": "user", "content": "hello"}
            ]
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hello");
    }

    #[test]
    fn message_with_text_blocks_concatenates_to_string() {
        let out = convert(json!({
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "line1"},
                    {"type": "input_text", "text": "line2"}
                ]
            }]
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs[0]["content"], "line1\nline2");
    }

    #[test]
    fn message_with_image_block_becomes_chat_multimodal_array() {
        let out = convert(json!({
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "what is this?"},
                    {"type": "input_image", "image_url": "https://x.test/i.png", "detail": "high"}
                ]
            }]
        }));
        let content = &out["messages"][0]["content"];
        let arr = content.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["type"], "text");
        assert_eq!(arr[0]["text"], "what is this?");
        assert_eq!(arr[1]["type"], "image_url");
        assert_eq!(arr[1]["image_url"]["url"], "https://x.test/i.png");
        assert_eq!(arr[1]["image_url"]["detail"], "high");
    }

    #[test]
    fn input_image_file_audio_video_items_are_lowered_to_chat_messages() {
        let out = convert(json!({
            "input": [
                {"type": "input_image", "image_url": "https://x.test/i.png", "detail": "low"},
                {"type": "input_file", "file_id": "file_1", "filename": "notes.pdf"},
                {"type": "input_audio", "data": "YWJj", "format": "mp3"},
                {"type": "input_video", "url": "https://x.test/v.mp4"}
            ]
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1, "连续 user message 应按旧版逻辑合并");
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image_url");
        assert_eq!(content[0]["image_url"]["url"], "https://x.test/i.png");
        assert_eq!(content[1]["type"], "text");
        assert_eq!(content[1]["text"], "[File: notes.pdf (id=file_1)]");
        assert_eq!(content[2]["type"], "input_audio");
        assert_eq!(content[2]["input_audio"]["format"], "mp3");
        assert_eq!(content[2]["input_audio"]["mime_type"], "audio/mp3");
        assert_eq!(content[3]["type"], "image_url");
        assert_eq!(content[3]["image_url"]["url"], "https://x.test/v.mp4");
    }

    #[test]
    fn input_file_data_becomes_data_uri_image_url() {
        let out = convert(json!({
            "input": [{
                "type": "input_file",
                "file_data": "ZmFrZQ==",
                "mime_type": "image/png",
                "filename": "image.png"
            }]
        }));
        let content = out["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image_url");
        assert_eq!(
            content[0]["image_url"]["url"],
            "data:image/png;base64,ZmFrZQ=="
        );
    }

    #[test]
    fn compaction_item_renders_as_user_message_with_summary_text() {
        // 关键回归:Codex CLI auto-compact 后,续轮 input[] 会带
        // {"type":"compaction","encrypted_content":"<SUMMARY_PREFIX>\n<summary>"}。
        // 必须转成 user message,跟 Codex 自家 inline compact 行为对齐;否则
        // 上游 LLM 完全看不到 summary,等于 compact 后失忆。
        let out = convert(json!({
            "input": [{
                "type": "compaction",
                "encrypted_content": "Another language model started... <SUMMARY>: user wanted X, we did Y."
            }]
        }));
        let msg = &out["messages"][0];
        assert_eq!(msg["role"], "user");
        assert!(msg["content"]
            .as_str()
            .unwrap_or("")
            .contains("user wanted X, we did Y"));
    }

    #[test]
    fn context_compaction_alias_renders_same_as_compaction() {
        // ResponseItem::ContextCompaction 是 Codex protocol 里同一概念的别名
        // (`codex-rs/protocol/src/models.rs:884`),也要识别。
        let out = convert(json!({
            "input": [{
                "type": "context_compaction",
                "encrypted_content": "summary body"
            }]
        }));
        let msg = &out["messages"][0];
        assert_eq!(msg["role"], "user");
        assert_eq!(msg["content"], "summary body");
    }

    #[test]
    fn compaction_item_with_empty_encrypted_content_is_dropped() {
        // 防御:空 summary 不应往上游塞空 user message(会触发某些 provider
        // "user message must not be empty" 400)
        let out = convert(json!({
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "real user msg"}
                ]},
                {"type": "compaction", "encrypted_content": "   "}
            ]
        }));
        let messages = out["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "空 compaction 应被丢弃,只剩真实 user");
        // content 可能是 string 或 array,都接受 — 关键是没 compaction 留下来
        let content_str = serde_json::to_string(&messages[0]["content"]).unwrap();
        assert!(
            content_str.contains("real user msg"),
            "应保留真实 user message 内容,实际: {content_str}"
        );
    }

    #[test]
    fn unknown_input_item_with_content_is_normalized() {
        let out = convert(json!({
            "input": [{
                "type": "unknown_event",
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "inspect"},
                    {"type": "input_file", "filename": "a.txt"}
                ]
            }]
        }));
        let content = out["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "inspect");
        assert_eq!(content[1]["text"], "[input_file: a.txt]");
    }

    #[test]
    fn function_call_becomes_assistant_with_tool_calls() {
        let out = convert(json!({
            "input": [{
                "type": "function_call",
                "call_id": "call_abc",
                "name": "get_weather",
                "arguments": "{\"loc\":\"NYC\"}"
            }]
        }));
        let msg = &out["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["content"], "");
        assert_eq!(msg["tool_calls"][0]["id"], "call_abc");
        assert_eq!(msg["tool_calls"][0]["type"], "function");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(
            msg["tool_calls"][0]["function"]["arguments"],
            "{\"loc\":\"NYC\"}"
        );
    }

    #[test]
    fn function_call_without_arguments_defaults_to_json_object() {
        let out = convert(json!({
            "input": [{
                "type": "function_call",
                "call_id": "call_no_args",
                "name": "noop"
            }]
        }));
        let msg = &out["messages"][0];
        assert_eq!(msg["tool_calls"][0]["function"]["arguments"], "{}");
    }

    /// 给单测用的隔离 cache,避免并行测试互相污染。
    fn empty_tool_cache() -> super::super::tool_call_cache::ToolCallCache {
        super::super::tool_call_cache::ToolCallCache::new(16, std::time::Duration::from_secs(60))
    }

    #[test]
    fn function_call_output_becomes_tool_message_with_placeholder_assistant() {
        // 孤儿 function_call_output(无前置 function_call):repair 路径 B-orphan
        // 必须在它前面插占位 assistant.tool_calls,Chat 上游(Kimi/DeepSeek)
        // 严格校验时才能匹配住 tool_call_id,不会 400。
        let mut messages = vec![json!({
            "role": "tool",
            "tool_call_id": "call_abc",
            "content": "sunny",
        })];
        let cache = empty_tool_cache();
        repair_tool_call_ids(&mut messages, &cache);
        assert_eq!(messages.len(), 2, "孤儿 tool 前应插占位 assistant");
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_abc");
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "");
        assert_eq!(messages[0]["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "call_abc");
        assert_eq!(messages[1]["content"], "sunny");
    }

    #[test]
    fn function_call_output_non_string_is_json_serialized() {
        // 走完整 convert 路径(global cache 在生产里就这条路);
        // 这里只关心 content 序列化,不关心占位 assistant 行为(见上一条测试)。
        let out = convert(json!({
            "input": [{
                "type": "function_call_output",
                "call_id": "c",
                "output": {"temp": 72}
            }]
        }));
        let tool_msg = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .expect("应当有 tool 消息");
        assert_eq!(tool_msg["content"], "{\"temp\":72}");
    }

    #[test]
    fn large_function_call_output_is_bounded_before_chat_history() {
        let huge_line = "function veryLongMinifiedBundle(){return 'x';}".repeat(2_000);
        let raw_output = format!(
            "Chunk ID: 44d863\n\
             Wall time: 0.1540 seconds\n\
             Process exited with code 0\n\
             Original token count: 924828\n\
             Output:\n\
             Total output lines: 18\n\n\
             /tmp/codex-asar/webview/assets/plugins-page-selectors.js:{huge_line}"
        );

        let out = convert(json!({
            "input": [{
                "type": "function_call_output",
                "call_id": "tool_large",
                "output": raw_output
            }]
        }));
        let tool_msg = out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "tool")
            .expect("应当有 tool 消息");

        assert_eq!(tool_msg["tool_call_id"], "tool_large");
        let content = tool_msg["content"].as_str().unwrap();
        assert!(
            content.contains("[Tool output stored outside model context]"),
            "大工具结果必须显式标记为外置存储,实际: {content}"
        );
        assert!(
            content.contains("Artifact ID: tool_artifact_"),
            "大工具结果必须带可追踪 artifact id,实际: {content}"
        );
        assert!(
            content.contains("Original token count: 924828"),
            "应保留原始工具输出 token 规模线索"
        );
        assert!(
            content.len() < 20_000,
            "模型可见 tool.content 应有界,实际长度 {}",
            content.len()
        );
    }

    #[test]
    fn large_function_call_output_raw_payload_round_trips_via_artifact_store() {
        let store = super::super::artifact_store::ToolArtifactStore::new(
            8,
            std::time::Duration::from_secs(60),
        );
        let raw_output = format!(
            "Process exited with code 0\nOriginal token count: 9000\n{}",
            "raw-line\n".repeat(900)
        );
        let content = normalize_tool_output_for_context_with_store(
            Some("call_artifact"),
            Value::String(raw_output.clone()),
            Some(&store),
        );
        let artifact_id = content
            .lines()
            .find_map(|line| line.strip_prefix("Artifact ID: "))
            .expect("summary must expose artifact id");
        let stored = store.get(artifact_id).expect("raw artifact must be stored");

        assert_eq!(stored.call_id.as_deref(), Some("call_artifact"));
        assert_eq!(stored.kind, "command_output");
        assert_eq!(stored.raw_content, raw_output);
        assert!(
            content.len() < raw_output.len(),
            "model-visible summary must be smaller than raw payload"
        );
    }

    #[test]
    fn empty_tool_call_id_is_repaired_from_previous_assistant_call() {
        let out = convert(json!({
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "shell",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "output": "ok"
                }
            ]
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[1]["tool_call_id"], "call_abc");
    }

    #[test]
    fn orphan_tool_with_call_id_rebuilds_from_tool_call_cache() {
        // path B-orphan + cache 命中:占位 assistant 应当用 cache 里的 name +
        // arguments,让 Chat 上游能按真实工具名重建上下文。
        let cache = empty_tool_cache();
        cache.save(
            "call_rebuild",
            super::super::tool_call_cache::ToolCallEntry {
                name: "shell".to_owned(),
                arguments: r#"{"cmd":"ls"}"#.to_owned(),
            },
        );
        let mut messages = vec![json!({
            "role": "tool",
            "tool_call_id": "call_rebuild",
            "content": "/repo",
        })];
        repair_tool_call_ids(&mut messages, &cache);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "call_rebuild");
        assert_eq!(messages[0]["tool_calls"][0]["function"]["name"], "shell");
        assert_eq!(
            messages[0]["tool_calls"][0]["function"]["arguments"],
            r#"{"cmd":"ls"}"#
        );
        assert_eq!(messages[1]["tool_call_id"], "call_rebuild");
    }

    #[test]
    fn orphan_tool_with_call_id_inserts_tool_call_into_existing_assistant() {
        // path B-into-existing:user → assistant(无 tool_calls)→ tool
        // (call_id 不在前 assistant 的 tool_calls 里)。应当把重建的
        // tool_call 注回到那条 assistant 里,而不是再插一条占位。
        let cache = empty_tool_cache();
        cache.save(
            "call_inject",
            super::super::tool_call_cache::ToolCallEntry {
                name: "search".to_owned(),
                arguments: "{}".to_owned(),
            },
        );
        let mut messages = vec![
            json!({"role": "user", "content": "hi"}),
            json!({"role": "assistant", "content": "thinking"}),
            json!({"role": "tool", "tool_call_id": "call_inject", "content": "ok"}),
        ];
        repair_tool_call_ids(&mut messages, &cache);
        assert_eq!(
            messages.len(),
            3,
            "不应插占位 assistant,只在已有 assistant 里加 tool_calls"
        );
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["tool_calls"][0]["id"], "call_inject");
        assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "search");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_inject");
    }

    #[test]
    fn user_message_after_tool_call_resets_pending_state() {
        // path "boundary":user / system / developer 出现时清掉 pending +
        // last_assistant_idx,后续孤儿 tool 不会错把那条 assistant 当作注入
        // 目标,而是在 tool 前再插占位 assistant。
        let cache = empty_tool_cache();
        let mut messages = vec![
            json!({"role": "assistant", "content": ""}),
            json!({"role": "user", "content": "next"}),
            json!({"role": "tool", "tool_call_id": "call_after_user", "content": "x"}),
        ];
        repair_tool_call_ids(&mut messages, &cache);
        let assistant_count = messages.iter().filter(|m| m["role"] == "assistant").count();
        assert!(
            assistant_count >= 2,
            "user 边界后再来 orphan tool 必须重新插占位 assistant,实际 {assistant_count}"
        );
        let tool_msg = messages.iter().find(|m| m["role"] == "tool").unwrap();
        assert_eq!(tool_msg["tool_call_id"], "call_after_user");
    }

    /// issue #180:并行工具调用部分应答 —— assistant.tool_calls = [a, b, c],
    /// 只有 tool b 跟上,然后 user 发新消息。DeepSeek / Kimi 严格校验时 400。
    /// 修复后:user 之前应该插 a 和 c 的占位 tool 消息。
    #[test]
    fn parallel_tool_calls_partial_answer_gets_placeholder_for_missing_ids() {
        let cache = empty_tool_cache();
        let mut messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "a", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                    {"id": "b", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                    {"id": "c", "type": "function", "function": {"name": "search", "arguments": "{}"}},
                ],
            }),
            json!({"role": "tool", "tool_call_id": "b", "content": "result_b"}),
            json!({"role": "user", "content": "继续"}),
        ];
        repair_tool_call_ids(&mut messages, &cache);

        // 期望顺序: assistant → tool(b) → placeholder(a) → placeholder(c) → user
        assert_eq!(
            messages.len(),
            5,
            "应当补齐 a/c 两条占位,实际 {:#?}",
            messages
        );
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "b");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "a");
        assert!(messages[2]["content"]
            .as_str()
            .unwrap()
            .contains("Tool execution skipped/interrupted"));
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "c");
        assert_eq!(messages[4]["role"], "user");
    }

    /// 末尾 flush:整段 input 末尾 assistant.tool_calls 没有任何 tool 应答
    /// (Codex CLI 中断 / 续轮时偶发)。修复后末尾应当补占位 tool。
    #[test]
    fn trailing_unanswered_tool_calls_get_placeholder_at_end() {
        let cache = empty_tool_cache();
        let mut messages = vec![
            json!({"role": "user", "content": "go"}),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "a", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                    {"id": "b", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                ],
            }),
        ];
        repair_tool_call_ids(&mut messages, &cache);

        // 期望: user → assistant → placeholder(a) → placeholder(b)
        assert_eq!(
            messages.len(),
            4,
            "末尾 pending 必须 flush,实际 {:#?}",
            messages
        );
        assert_eq!(messages[2]["tool_call_id"], "a");
        assert_eq!(messages[3]["tool_call_id"], "b");
    }

    /// 连续 assistant message 之间没有 tool 应答:老 pending 应在新 assistant
    /// 之前 flush,而不是被静默覆盖。
    #[test]
    fn consecutive_assistants_flush_pending_before_overwrite() {
        let cache = empty_tool_cache();
        let mut messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "old", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                ],
            }),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "new", "type": "function", "function": {"name": "search", "arguments": "{}"}},
                ],
            }),
        ];
        repair_tool_call_ids(&mut messages, &cache);

        // 期望: assistant1 → placeholder(old) → assistant2 → placeholder(new)
        assert_eq!(messages.len(), 4, "实际 {:#?}", messages);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["tool_calls"][0]["id"], "old");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "old");
        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "new");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "new");
    }

    /// flush 时优先从 ToolCallCache 拿真实 tool name,占位 content 带工具名。
    #[test]
    fn flush_uses_tool_name_from_cache_when_available() {
        let cache = empty_tool_cache();
        cache.save(
            "call_a",
            super::super::tool_call_cache::ToolCallEntry {
                name: "web_search".to_owned(),
                arguments: "{}".to_owned(),
            },
        );
        let mut messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "call_a", "type": "function", "function": {"name": "web_search", "arguments": "{}"}},
                ],
            }),
            json!({"role": "user", "content": "stop"}),
        ];
        repair_tool_call_ids(&mut messages, &cache);
        let placeholder = messages
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == "call_a")
            .expect("应有占位 tool");
        let content = placeholder["content"].as_str().unwrap();
        assert!(
            content.contains("'web_search'"),
            "cache 命中时占位 content 应带真实 tool name,实际 {content}"
        );
    }

    /// cache 没命中时,占位 content 应当退化为 'unknown_tool'(还是带文案,
    /// 上游能 match id 不报 400)。
    #[test]
    fn flush_falls_back_to_unknown_tool_when_cache_misses() {
        let cache = empty_tool_cache();
        let mut messages = vec![
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "ghost", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                ],
            }),
            json!({"role": "user", "content": "stop"}),
        ];
        repair_tool_call_ids(&mut messages, &cache);
        let placeholder = messages
            .iter()
            .find(|m| m["tool_call_id"] == "ghost")
            .expect("应有占位 tool");
        let content = placeholder["content"].as_str().unwrap();
        assert!(
            content.contains("'unknown_tool'"),
            "cache miss 时占位文案应退化为 unknown_tool,实际 {content}"
        );
    }

    /// 综合回归:同一段 history 里既有正向孤儿(tool 找不到 assistant.tool_calls)
    /// 又有反向孤儿(assistant.tool_calls 找不到 tool),两侧都要修。
    #[test]
    fn forward_and_reverse_orphans_both_get_repaired() {
        // history(模拟 Codex CLI 长会话压缩 + 并行工具调用部分应答):
        //   tool a (孤儿,没有 assistant)
        //   assistant{tool_calls:[b, c]}
        //   tool b
        //   user
        let cache = empty_tool_cache();
        cache.save(
            "a",
            super::super::tool_call_cache::ToolCallEntry {
                name: "shell".to_owned(),
                arguments: "{}".to_owned(),
            },
        );
        let mut messages = vec![
            json!({"role": "tool", "tool_call_id": "a", "content": "result_a"}),
            json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [
                    {"id": "b", "type": "function", "function": {"name": "shell", "arguments": "{}"}},
                    {"id": "c", "type": "function", "function": {"name": "search", "arguments": "{}"}},
                ],
            }),
            json!({"role": "tool", "tool_call_id": "b", "content": "result_b"}),
            json!({"role": "user", "content": "继续"}),
        ];
        repair_tool_call_ids(&mut messages, &cache);

        // 正向:tool a 前应有占位 assistant
        let first = &messages[0];
        assert_eq!(
            first["role"], "assistant",
            "正向孤儿应插占位 assistant 在前"
        );
        assert_eq!(first["tool_calls"][0]["id"], "a");
        assert_eq!(messages[1]["role"], "tool");
        assert_eq!(messages[1]["tool_call_id"], "a");

        // 反向:assistant{b,c} → tool b → 应有占位 tool c → user
        let assistant_bc = messages
            .iter()
            .find(|m| {
                m["role"] == "assistant" && m["tool_calls"].as_array().map(|a| a.len()) == Some(2)
            })
            .expect("应有 [b,c] 的 assistant");
        assert_eq!(assistant_bc["tool_calls"][0]["id"], "b");
        assert_eq!(assistant_bc["tool_calls"][1]["id"], "c");
        let placeholder_c = messages
            .iter()
            .find(|m| m["role"] == "tool" && m["tool_call_id"] == "c")
            .expect("应有 c 的占位 tool");
        assert!(placeholder_c["content"]
            .as_str()
            .unwrap()
            .contains("Tool execution skipped/interrupted"));
        assert_eq!(messages.last().unwrap()["role"], "user");
    }

    /// 端到端:走完整 convert(包含 build_messages_from_input +
    /// merge_consecutive_assistant_messages + repair_tool_call_ids),复现
    /// issue #180 用户描述的 Responses input → Chat messages 转换路径。
    #[test]
    fn issue_180_responses_input_parallel_partial_answer_end_to_end() {
        let out = convert(json!({
            "input": [
                {"type": "function_call", "call_id": "a", "name": "shell", "arguments": "{}"},
                {"type": "function_call", "call_id": "b", "name": "shell", "arguments": "{}"},
                {"type": "function_call", "call_id": "c", "name": "search", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "b", "output": "result_b"},
                {"type": "message", "role": "user", "content": "继续"}
            ]
        }));
        let msgs = out["messages"].as_array().unwrap();

        // 期望: assistant{a,b,c} → tool b → placeholder a → placeholder c → user
        assert_eq!(msgs.len(), 5, "端到端 messages 长度,实际 {:#?}", msgs);
        assert_eq!(msgs[0]["role"], "assistant");
        let tool_calls = msgs[0]["tool_calls"].as_array().unwrap();
        assert_eq!(tool_calls.len(), 3);
        assert_eq!(tool_calls[0]["id"], "a");
        assert_eq!(tool_calls[1]["id"], "b");
        assert_eq!(tool_calls[2]["id"], "c");
        assert_eq!(msgs[1]["tool_call_id"], "b");
        assert_eq!(msgs[1]["content"], "result_b");
        assert_eq!(msgs[2]["tool_call_id"], "a");
        assert!(msgs[2]["content"]
            .as_str()
            .unwrap()
            .contains("skipped/interrupted"));
        assert_eq!(msgs[3]["tool_call_id"], "c");
        assert_eq!(msgs[4]["role"], "user");
    }

    #[test]
    fn orphan_tool_message_without_call_id_is_dropped() {
        let out = convert(json!({
            "input": [
                {
                    "type": "function_call_output",
                    "output": "orphan"
                },
                {
                    "type": "message",
                    "role": "user",
                    "content": "continue"
                }
            ]
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
    }

    #[test]
    fn reasoning_summary_is_attached_to_following_tool_call() {
        let out = convert(json!({
            "input": [
                {
                    "type": "reasoning",
                    "summary": [{
                        "type": "summary_text",
                        "text": "I should inspect the repo."
                    }],
                    "content": null,
                    "encrypted_content": null
                },
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "shell",
                    "arguments": "{\"cmd\":\"pwd\"}"
                }
            ]
        }));
        let msg = &out["messages"][0];
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["reasoning_content"], "I should inspect the repo.");
    }

    #[test]
    fn reasoning_summary_strips_codex_thinking_prefix_on_continuation() {
        // 续轮场景:Codex CLI 把上一轮 v2.0.8 注入的 `**Thinking**\n\n` prefix
        // 通过 reasoning summary 文本回送回来。proxy 在写回上游 messages.reasoning_content
        // 之前必须 strip,避免 prefix 累积污染上游 history。
        let out = convert(json!({
            "input": [
                {
                    "type": "reasoning",
                    "summary": [{
                        "type": "summary_text",
                        "text": "**Thinking**\n\nI should inspect the repo."
                    }],
                    "content": null,
                    "encrypted_content": null
                },
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "shell",
                    "arguments": "{\"cmd\":\"pwd\"}"
                }
            ]
        }));
        let msg = &out["messages"][0];
        assert_eq!(
            msg["reasoning_content"], "I should inspect the repo.",
            "**Thinking**\\n\\n prefix 应被 strip,只保留原始 reasoning"
        );
    }

    #[test]
    fn opaque_reasoning_item_uses_blank_placeholder_for_tool_call() {
        let out = convert(json!({
            "input": [
                {
                    "type": "reasoning",
                    "summary": [],
                    "content": null,
                    "encrypted_content": "opaque"
                },
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "shell",
                    "arguments": "{}"
                }
            ]
        }));
        assert_eq!(out["messages"][0]["reasoning_content"], " ");
    }

    #[test]
    fn request_reasoning_repairs_tool_call_assistant_reasoning() {
        let out = convert(json!({
            "reasoning": {"effort": "high"},
            "input": [
                {
                    "type": "function_call",
                    "call_id": "call_abc",
                    "name": "shell",
                    "arguments": "{}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_abc",
                    "output": "ok"
                }
            ]
        }));
        assert_eq!(out["messages"][0]["reasoning_content"], " ");
    }

    #[test]
    fn deepseek_provider_thinking_repairs_without_request_reasoning() {
        let provider = deepseek_thinking_provider();
        let out = responses_body_to_chat_body_for_provider(
            &json!({
                "input": [
                    {
                        "type": "function_call",
                        "call_id": "call_abc",
                        "name": "shell",
                        "arguments": "{}"
                    },
                    {
                        "type": "function_call_output",
                        "call_id": "call_abc",
                        "output": "ok"
                    }
                ]
            }),
            Some(&provider),
        )
        .unwrap();
        assert_eq!(out["messages"][0]["reasoning_content"], " ");
    }

    #[test]
    fn non_deepseek_provider_thinking_does_not_repair_by_config_alone() {
        let mut provider = provider("other", "Other", "https://example.test/v1");
        provider
            .request_options
            .insert("chat".into(), json!({"thinking": {"type": "enabled"}}));
        let out = responses_body_to_chat_body_for_provider(
            &json!({
                "input": [
                    {
                        "type": "function_call",
                        "call_id": "call_abc",
                        "name": "shell",
                        "arguments": "{}"
                    },
                    {
                        "type": "function_call_output",
                        "call_id": "call_abc",
                        "output": "ok"
                    }
                ]
            }),
            Some(&provider),
        )
        .unwrap();
        assert!(out["messages"][0].get("reasoning_content").is_none());
    }

    #[test]
    fn tools_function_passes_through() {
        let out = convert(json!({
            "input": "hi",
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "description": "fetch forecast",
                "parameters": {
                    "type": "object",
                    "properties": {"loc": {"type": "string"}},
                    "required": ["loc"]
                },
                "strict": true
            }]
        }));
        let tool = &out["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "get_weather");
        assert_eq!(tool["function"]["description"], "fetch forecast");
        assert_eq!(tool["function"]["strict"], true);
        assert_eq!(tool["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn tools_parameters_default_type_object() {
        let out = convert(json!({
            "input": "hi",
            "tools": [{
                "type": "function",
                "name": "f",
                "parameters": {"properties": {}}
            }]
        }));
        assert_eq!(
            out["tools"][0]["function"]["parameters"]["type"], "object",
            "缺 type 字段时应自动补 object"
        );
    }

    #[test]
    fn tools_custom_type_is_lowered_to_function_with_input() {
        let out = convert(json!({
            "input": "hi",
            "tools": [{
                "type": "custom",
                "name": "free_text_tool",
                "description": "anything"
            }]
        }));
        let tool = &out["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "free_text_tool");
        assert_eq!(
            tool["function"]["parameters"]["properties"]["input"]["type"],
            "string"
        );
        assert_eq!(tool["function"]["parameters"]["required"][0], "input");
    }

    #[test]
    fn tools_unknown_responses_only_types_dropped() {
        let out = convert(json!({
            "input": "hi",
            "tools": [
                {"type": "function", "name": "keep_me"},
                {"type": "web_search_preview"},
                {"type": "file_search"},
                {"type": "computer_use_preview"}
            ]
        }));
        let tools = out["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["function"]["name"], "keep_me");
    }

    #[test]
    fn max_output_tokens_renamed_to_max_tokens() {
        let out = convert(json!({"input": "hi", "max_output_tokens": 256}));
        assert_eq!(out["max_tokens"], 256);
        assert!(out.get("max_output_tokens").is_none());
    }

    #[test]
    fn stream_true_adds_stream_options_include_usage() {
        let out = convert(json!({"stream": true, "input": "hi"}));
        assert_eq!(out["stream"], true);
        assert_eq!(out["stream_options"]["include_usage"], true);
    }

    #[test]
    fn passthrough_fields_kept() {
        let out = convert(json!({
            "temperature": 0.7,
            "top_p": 0.95,
            "seed": 42,
            "stop": ["END"],
            "parallel_tool_calls": true,
            "frequency_penalty": 0.1,
            "presence_penalty": 0.2,
            "user": "u-1",
            "logit_bias": {"1": -1},
            "safety_identifier": "safe-1",
            "extra_body": {"provider_flag": true},
            "timeout": 30,
            "input": "hi"
        }));
        assert_eq!(out["temperature"], 0.7);
        assert_eq!(out["top_p"], 0.95);
        assert_eq!(out["seed"], 42);
        assert_eq!(out["stop"][0], "END");
        assert_eq!(out["parallel_tool_calls"], true);
        assert_eq!(out["frequency_penalty"], 0.1);
        assert_eq!(out["presence_penalty"], 0.2);
        assert_eq!(out["user"], "u-1");
        assert_eq!(out["logit_bias"]["1"], -1);
        assert_eq!(out["safety_identifier"], "safe-1");
        assert_eq!(out["extra_body"]["provider_flag"], true);
        assert_eq!(out["timeout"], 30);
    }

    #[test]
    fn text_format_reasoning_and_special_fields_follow_legacy_conversion() {
        let out = convert(json!({
            "input": "hi",
            "text": {
                "format": {
                    "type": "json_schema",
                    "name": "answer",
                    "schema": {"type": "object"},
                    "strict": true
                }
            },
            "reasoning": {"effort": "xhigh"},
            "store": true,
            "metadata": {
                "short": "value",
                "number": 123
            },
            "prediction": {"type": "diff", "content": {"patch": "same"}},
            "service_tier": "priority",
            "modalities": ["text", "audio", "bad"],
            "audio": {"voice": "alloy", "format": "mp3"},
            "tool_choice": {"type": "any"}
        }));
        assert_eq!(out["response_format"]["type"], "json_schema");
        assert_eq!(out["response_format"]["json_schema"]["name"], "answer");
        assert_eq!(out["response_format"]["json_schema"]["strict"], true);
        assert_eq!(out["reasoning_effort"], "high");
        assert_eq!(out["store"], true);
        assert_eq!(out["metadata"]["short"], "value");
        assert_eq!(out["metadata"]["number"], "123");
        assert_eq!(out["prediction"]["type"], "content");
        assert_eq!(out["prediction"]["content"], "{\"patch\":\"same\"}");
        assert_eq!(out["service_tier"], "priority");
        assert_eq!(out["modalities"].as_array().unwrap().len(), 2);
        assert_eq!(out["audio"]["voice"], "alloy");
        assert_eq!(out["tool_choice"], "required");
    }

    #[test]
    fn invalid_special_fields_are_dropped_or_sanitized() {
        let out = convert(json!({
            "input": "hi",
            "store": "yes",
            "metadata": "bad",
            "prediction": {"type": "bad"},
            "service_tier": "",
            "modalities": ["bad"],
            "audio": "loud",
            "reasoning": {"effort": "none"},
            "text": {"format": {"type": "text"}}
        }));
        assert!(out.get("store").is_none());
        assert!(out.get("metadata").is_none());
        assert!(out.get("prediction").is_none());
        assert!(out.get("service_tier").is_none());
        assert!(out.get("modalities").is_none());
        assert!(out.get("audio").is_none());
        assert!(out.get("reasoning_effort").is_none());
        assert!(out.get("response_format").is_none());
    }

    #[test]
    fn developer_role_downgrades_to_system_except_openai_official_provider() {
        let non_openai = provider("kimi", "Kimi", "https://api.moonshot.cn/v1");
        let out = responses_body_to_chat_body_for_provider(
            &json!({
                "input": [{
                    "type": "message",
                    "role": "developer",
                    "content": "rules"
                }]
            }),
            Some(&non_openai),
        )
        .unwrap();
        assert_eq!(out["messages"][0]["role"], "system");

        let openai = provider("openai", "OpenAI", "https://api.openai.com/v1");
        let out = responses_body_to_chat_body_for_provider(
            &json!({
                "input": [{
                    "type": "message",
                    "role": "developer",
                    "content": "rules"
                }]
            }),
            Some(&openai),
        )
        .unwrap();
        assert_eq!(out["messages"][0]["role"], "developer");
    }

    #[test]
    fn previous_response_id_without_session_cache_keeps_current_input_only() {
        let out = convert(json!({
            "previous_response_id": "resp_abc",
            "input": "hi"
        }));
        // 没有传入 session cache 的公开 helper 保持无状态兼容。
        assert!(out.get("previous_response_id").is_none());
        assert_eq!(out["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn previous_response_id_restores_history_before_current_input() {
        let cache = ResponseSessionCache::new(1000, std::time::Duration::from_secs(3600));
        cache.save(
            "resp_prev",
            vec![
                json!({"role": "system", "content": "old instructions"}),
                json!({"role": "user", "content": "what is the weather?"}),
                json!({
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "get_weather", "arguments": "{\"loc\":\"NYC\"}"}
                    }]
                }),
            ],
        );

        let conversion = responses_body_to_chat_body_for_provider_with_session(
            &json!({
                "instructions": "new duplicate instructions",
                "previous_response_id": "resp_prev",
                "input": [
                    {"type": "function_call_output", "call_id": "call_1", "output": "sunny"},
                    {"type": "message", "role": "user", "content": "summarize"}
                ]
            }),
            None,
            Some(&cache),
        )
        .unwrap();

        let msgs = conversion.body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "old instructions");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "call_1");
        assert_eq!(msgs[4]["content"], "summarize");
        assert_eq!(conversion.response_session.messages, msgs.clone());
    }

    #[test]
    fn full_codex_cli_loop_pattern() {
        // 真实 Codex CLI 一次工具循环的形态:instructions + 用户问题 +
        // 模型上一轮的 function_call + 用户提供的 function_call_output + 新提问
        let out = convert(json!({
            "model": "gpt-x",
            "instructions": "You are an assistant.",
            "input": [
                {"type": "message", "role": "user", "content": "what's the weather?"},
                {
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "get_weather",
                    "arguments": "{\"loc\":\"NYC\"}"
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_1",
                    "output": "{\"temp\":72,\"cond\":\"sunny\"}"
                },
                {"type": "message", "role": "user", "content": "thanks!"}
            ],
            "tools": [{
                "type": "function",
                "name": "get_weather",
                "parameters": {"type": "object", "properties": {"loc": {"type": "string"}}}
            }],
            "stream": true,
            "max_output_tokens": 1024,
            "temperature": 0.0
        }));
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 5, "system + user + assistant + tool + user");
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[1]["role"], "user");
        assert_eq!(msgs[2]["role"], "assistant");
        assert_eq!(msgs[2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(msgs[3]["role"], "tool");
        assert_eq!(msgs[3]["tool_call_id"], "call_1");
        assert_eq!(msgs[4]["role"], "user");
        assert_eq!(msgs[4]["content"], "thanks!");
        assert_eq!(out["stream"], true);
        assert_eq!(out["stream_options"]["include_usage"], true);
        assert_eq!(out["max_tokens"], 1024);
        assert_eq!(out["temperature"], 0.0);
        assert_eq!(out["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn non_object_body_rejected() {
        let err = responses_body_to_chat_body(&json!("not an object"));
        assert!(matches!(err, Err(AdapterError::BadRequest(_))));
    }
}
