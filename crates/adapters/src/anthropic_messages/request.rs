use std::collections::{BTreeMap, BTreeSet};

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::header::{HeaderName, HeaderValue, CONTENT_TYPE};
use http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::responses::{
    compact, global_response_session_cache, responses_body_to_chat_body_for_provider_with_session,
    ResponseSessionCache,
};
use crate::types::{AdapterError, RequestPlan, ResponseSessionPlan};

const DEFAULT_MAX_TOKENS: u64 = 4096;
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnthropicToolNameMaps {
    pub forward: BTreeMap<String, String>,
    pub reverse: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AnthropicMessagesRequestConversion {
    pub request: Value,
    pub response_session: ResponseSessionPlan,
    pub tool_name_maps: AnthropicToolNameMaps,
}

#[derive(Debug, Clone)]
pub struct AnthropicMessagesPreparedRequest {
    pub upstream_path: String,
    pub body: Bytes,
    pub headers: HeaderMap,
    pub response_session: Option<ResponseSessionPlan>,
    pub is_compact: bool,
    pub original_responses_request: Option<Value>,
    pub tool_name_maps: AnthropicToolNameMaps,
}

/// Anthropic Messages requires a version header even when the request body is
/// otherwise OpenAI-compatible at the local gateway boundary. P5 adapter wiring
/// will merge these defaults without overriding user-configured provider
/// `extraHeaders`.
pub fn anthropic_messages_default_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("anthropic-version"),
        HeaderValue::from_static(DEFAULT_ANTHROPIC_VERSION),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers
}

/// Choose the relative path that `crates/proxy` will append to provider.base_url.
///
/// If the configured base URL already ends in `/v1`, appending `/messages`
/// avoids `.../v1/v1/messages`. Otherwise we append `/v1/messages`, matching
/// LiteLLM's Anthropic Messages URL completion behavior.
pub fn build_anthropic_messages_upstream_path(base_url: &str) -> String {
    let without_query = base_url.split('?').next().unwrap_or(base_url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let trimmed = without_fragment.trim_end_matches('/');
    let path = trimmed
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/').map(|(_, path)| path))
        .unwrap_or("");
    if path.trim_end_matches('/').ends_with("v1") {
        "/messages".to_owned()
    } else {
        "/v1/messages".to_owned()
    }
}

pub fn prepare_anthropic_messages_request(
    client_path: &str,
    body: Bytes,
    provider: &Provider,
) -> Result<AnthropicMessagesPreparedRequest, AdapterError> {
    if compact::is_compact_path(client_path) {
        let compact_chat_body = compact::build_compact_chat_request(&body, provider)?;
        let compact_chat_json: Value = serde_json::from_slice(&compact_chat_body)
            .map_err(|e| AdapterError::Internal(format!("compact chat body decode: {e}")))?;
        let wire = chat_body_to_anthropic_messages_request(&compact_chat_json, false)?;
        let body = serde_json::to_vec(&wire.request).map_err(AdapterError::BodyDecode)?;
        return Ok(AnthropicMessagesPreparedRequest {
            upstream_path: build_anthropic_messages_upstream_path(&provider.base_url),
            body: Bytes::from(body),
            headers: anthropic_messages_default_headers(),
            response_session: None,
            is_compact: true,
            original_responses_request: None,
            tool_name_maps: wire.tool_name_maps,
        });
    }

    let parsed: Value = serde_json::from_slice(&body)?;
    let conversion = responses_body_to_anthropic_messages_request_with_session(
        &parsed,
        provider,
        Some(global_response_session_cache()),
    )?;
    let body = serde_json::to_vec(&conversion.request).map_err(AdapterError::BodyDecode)?;
    Ok(AnthropicMessagesPreparedRequest {
        upstream_path: build_anthropic_messages_upstream_path(&provider.base_url),
        body: Bytes::from(body),
        headers: anthropic_messages_default_headers(),
        response_session: Some(conversion.response_session),
        is_compact: false,
        original_responses_request: Some(parsed),
        tool_name_maps: conversion.tool_name_maps,
    })
}

pub fn into_request_plan(prepared: AnthropicMessagesPreparedRequest) -> RequestPlan {
    let adapter_metadata = serde_json::to_value(&prepared.tool_name_maps).ok();
    RequestPlan {
        upstream_path: prepared.upstream_path,
        body: prepared.body,
        upstream_headers: prepared.headers,
        response_session: prepared.response_session,
        adapter_metadata,
        is_compact: prepared.is_compact,
        original_responses_request: prepared.original_responses_request,
    }
}

pub fn responses_body_to_anthropic_messages_request(
    input: &Value,
    provider: &Provider,
) -> Result<AnthropicMessagesRequestConversion, AdapterError> {
    responses_body_to_anthropic_messages_request_with_session(input, provider, None)
}

pub fn responses_body_to_anthropic_messages_request_with_session(
    input: &Value,
    provider: &Provider,
    session_cache: Option<&ResponseSessionCache>,
) -> Result<AnthropicMessagesRequestConversion, AdapterError> {
    let conversion = responses_body_to_chat_body_for_provider_with_session(
        input,
        Some(provider),
        session_cache,
    )?;
    let wire = chat_body_to_anthropic_messages_request(&conversion.body, true)?;
    Ok(AnthropicMessagesRequestConversion {
        request: wire.request,
        response_session: conversion.response_session,
        tool_name_maps: wire.tool_name_maps,
    })
}

#[derive(Debug, Clone)]
pub struct AnthropicMessagesWireConversion {
    pub request: Value,
    pub tool_name_maps: AnthropicToolNameMaps,
}

pub fn chat_body_to_anthropic_messages_request(
    chat_body: &Value,
    stream: bool,
) -> Result<AnthropicMessagesWireConversion, AdapterError> {
    let body = chat_body
        .as_object()
        .ok_or_else(|| AdapterError::BadRequest("chat body must be a JSON object".into()))?;
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| AdapterError::BadRequest("model field required".into()))?;

    let tools = body
        .get("tools")
        .and_then(|v| v.as_array())
        .map(|tools| convert_chat_tools(tools))
        .transpose()?;
    let tool_name_maps = tools
        .as_ref()
        .map(|converted| converted.name_maps.clone())
        .unwrap_or_default();

    let mut out = Map::new();
    out.insert("model".into(), Value::String(model.to_owned()));
    out.insert(
        "messages".into(),
        Value::Array(convert_chat_messages(
            body.get("messages").and_then(|v| v.as_array()),
            &tool_name_maps,
        )?),
    );
    out.insert("max_tokens".into(), max_tokens_for_anthropic(body));
    out.insert("stream".into(), Value::Bool(stream));

    if let Some(system) = collect_system_text(body.get("messages").and_then(|v| v.as_array())) {
        out.insert("system".into(), Value::String(system));
    }
    if let Some(converted) = tools {
        if !converted.tools.is_empty() {
            out.insert("tools".into(), Value::Array(converted.tools));
        }
    }
    if let Some(tool_choice) = convert_tool_choice(
        body.get("tool_choice"),
        body.get("parallel_tool_calls").and_then(|v| v.as_bool()),
        &tool_name_maps,
    ) {
        out.insert("tool_choice".into(), tool_choice);
    }
    if let Some(stop_sequences) = convert_stop_sequences(body.get("stop")) {
        out.insert("stop_sequences".into(), stop_sequences);
    }
    copy_if_present(body, &mut out, "temperature");
    copy_if_present(body, &mut out, "top_p");
    copy_if_present(body, &mut out, "top_k");
    if let Some(thinking) = convert_thinking(body) {
        out.insert("thinking".into(), thinking);
    }
    if let Some(metadata) = convert_metadata(body) {
        out.insert("metadata".into(), metadata);
    }

    Ok(AnthropicMessagesWireConversion {
        request: Value::Object(out),
        tool_name_maps,
    })
}

fn copy_if_present(body: &Map<String, Value>, out: &mut Map<String, Value>, key: &str) {
    if let Some(value) = body.get(key) {
        out.insert(key.to_owned(), value.clone());
    }
}

fn max_tokens_for_anthropic(body: &Map<String, Value>) -> Value {
    for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        if let Some(n) = value_to_positive_u64(body.get(key)) {
            return Value::Number(n.into());
        }
    }
    Value::Number(DEFAULT_MAX_TOKENS.into())
}

fn value_to_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.round().max(1.0) as u64)),
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None,
    }
    .map(|n| n.max(1))
}

fn collect_system_text(messages: Option<&Vec<Value>>) -> Option<String> {
    let mut parts = Vec::new();
    for msg in messages.into_iter().flatten() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        if matches!(role, "system" | "developer") {
            let text = content_to_text(msg.get("content").unwrap_or(&Value::Null));
            if !text.trim().is_empty() {
                parts.push(text);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

fn convert_chat_messages(
    messages: Option<&Vec<Value>>,
    tool_names: &AnthropicToolNameMaps,
) -> Result<Vec<Value>, AdapterError> {
    let mut out = Vec::new();
    let mut known_tool_use_ids: BTreeSet<String> = BTreeSet::new();
    for msg in messages.into_iter().flatten() {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        match role {
            "system" | "developer" => {}
            "user" => {
                let content = user_content_blocks(msg.get("content").unwrap_or(&Value::Null));
                if !content.is_empty() {
                    out.push(json!({ "role": "user", "content": content }));
                }
            }
            "assistant" => {
                let content = assistant_content_blocks(msg, tool_names, &mut known_tool_use_ids)?;
                if !content.is_empty() {
                    out.push(json!({ "role": "assistant", "content": content }));
                }
            }
            "tool" => {
                let tool_use_id = msg
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        AdapterError::BadRequest(
                            "tool result missing tool_call_id for Anthropic Messages".into(),
                        )
                    })?;
                if !known_tool_use_ids.contains(tool_use_id) {
                    return Err(AdapterError::BadRequest(format!(
                        "tool result references unknown tool_use id {tool_use_id}"
                    )));
                }
                let content = content_to_text(msg.get("content").unwrap_or(&Value::Null));
                out.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    }],
                }));
            }
            _ => {}
        }
    }
    Ok(out)
}

fn user_content_blocks(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) => text_block_vec(s),
        Value::Array(items) => {
            let mut blocks = Vec::new();
            for item in items {
                let Some(obj) = item.as_object() else {
                    let text = value_to_string(item);
                    if !text.trim().is_empty() {
                        blocks.push(json!({ "type": "text", "text": text }));
                    }
                    continue;
                };
                match obj.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "text" | "input_text" | "output_text" => {
                        if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                            if !text.trim().is_empty() {
                                blocks.push(json!({ "type": "text", "text": text }));
                            }
                        }
                    }
                    "image_url" => {
                        if let Some(block) = image_url_to_anthropic_block(obj.get("image_url")) {
                            blocks.push(block);
                        }
                    }
                    _ => {
                        let text = content_block_to_text(item);
                        if !text.trim().is_empty() {
                            blocks.push(json!({ "type": "text", "text": text }));
                        }
                    }
                }
            }
            blocks
        }
        Value::Null => Vec::new(),
        other => text_block_vec(&value_to_string(other)),
    }
}

fn assistant_content_blocks(
    msg: &Value,
    tool_names: &AnthropicToolNameMaps,
    known_tool_use_ids: &mut BTreeSet<String>,
) -> Result<Vec<Value>, AdapterError> {
    let mut blocks = Vec::new();
    if let Some(reasoning) = msg
        .get("reasoning_content")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        blocks.push(json!({ "type": "thinking", "thinking": reasoning }));
    }

    let text = content_to_text(msg.get("content").unwrap_or(&Value::Null));
    if !text.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": text }));
    }

    if let Some(tool_calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for call in tool_calls {
            let id = call
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("call_unknown");
            let original_name = call
                .pointer("/function/name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    AdapterError::BadRequest(
                        "assistant tool_call missing function.name for Anthropic Messages".into(),
                    )
                })?;
            let name = tool_names
                .forward
                .get(original_name)
                .map(String::as_str)
                .unwrap_or(original_name);
            let arguments = call
                .pointer("/function/arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let input = parse_tool_arguments(arguments)?;
            known_tool_use_ids.insert(id.to_owned());
            blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": input,
            }));
        }
    }
    Ok(blocks)
}

fn text_block_vec(text: &str) -> Vec<Value> {
    if text.trim().is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": "text", "text": text })]
    }
}

fn content_to_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(content_block_to_text)
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => value_to_string(other),
    }
}

fn content_block_to_text(block: &Value) -> String {
    if let Some(obj) = block.as_object() {
        for key in ["text", "content"] {
            if let Some(text) = obj.get(key).and_then(|v| v.as_str()) {
                return text.to_owned();
            }
        }
    }
    value_to_string(block)
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

fn image_url_to_anthropic_block(image_url: Option<&Value>) -> Option<Value> {
    let url = match image_url? {
        Value::String(s) => s.as_str(),
        Value::Object(obj) => obj.get("url").and_then(|v| v.as_str()).unwrap_or(""),
        _ => "",
    };
    if url.trim().is_empty() {
        return None;
    }
    if let Some(rest) = url.strip_prefix("data:") {
        let (media_type, data) = rest.split_once(";base64,")?;
        return Some(json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": media_type,
                "data": data,
            }
        }));
    }
    Some(json!({
        "type": "image",
        "source": {
            "type": "url",
            "url": url,
        }
    }))
}

fn parse_tool_arguments(arguments: &str) -> Result<Value, AdapterError> {
    let parsed: Value = serde_json::from_str(arguments).map_err(|e| {
        AdapterError::BadRequest(format!("tool_call arguments are not valid JSON: {e}"))
    })?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Ok(json!({ "input": parsed }))
    }
}

struct ConvertedTools {
    tools: Vec<Value>,
    name_maps: AnthropicToolNameMaps,
}

fn convert_chat_tools(tools: &[Value]) -> Result<ConvertedTools, AdapterError> {
    let original_names = tools
        .iter()
        .filter_map(chat_tool_original_name)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let name_maps = build_tool_name_maps(&original_names);
    let mut converted = Vec::new();
    for tool in tools {
        let Some(name) = chat_tool_original_name(tool) else {
            continue;
        };
        let sanitized_name = name_maps
            .forward
            .get(name)
            .map(String::as_str)
            .unwrap_or(name);
        let function = tool.get("function").and_then(|v| v.as_object());
        let description = function
            .and_then(|f| f.get("description"))
            .or_else(|| tool.get("description"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let input_schema = function
            .and_then(|f| f.get("parameters"))
            .or_else(|| tool.get("parameters"))
            .cloned()
            .unwrap_or_else(|| json!({ "type": "object" }));
        let mut obj = Map::new();
        obj.insert("name".into(), Value::String(sanitized_name.to_owned()));
        if !description.trim().is_empty() {
            obj.insert("description".into(), Value::String(description.to_owned()));
        }
        obj.insert("input_schema".into(), input_schema);
        converted.push(Value::Object(obj));
    }
    Ok(ConvertedTools {
        tools: converted,
        name_maps,
    })
}

fn chat_tool_original_name(tool: &Value) -> Option<&str> {
    tool.pointer("/function/name")
        .and_then(|v| v.as_str())
        .or_else(|| tool.get("name").and_then(|v| v.as_str()))
        .filter(|s| !s.trim().is_empty())
}

fn build_tool_name_maps(original_names: &[String]) -> AnthropicToolNameMaps {
    let mut used = BTreeSet::new();
    let mut forward = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    for original in original_names {
        let base = sanitize_tool_name_base(original);
        let mut candidate = base.clone();
        let mut suffix = 2u32;
        while used.contains(&candidate) {
            let suffix_text = format!("_{suffix}");
            let head_len = 128usize.saturating_sub(suffix_text.len());
            candidate = format!("{}{}", truncate_chars(&base, head_len), suffix_text);
            suffix += 1;
        }
        used.insert(candidate.clone());
        if &candidate != original {
            forward.insert(original.clone(), candidate.clone());
            reverse.insert(candidate, original.clone());
        }
    }
    AnthropicToolNameMaps { forward, reverse }
}

fn sanitize_tool_name_base(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
        if out.len() >= 128 {
            break;
        }
    }
    if out.is_empty() {
        "tool".to_owned()
    } else {
        truncate_chars(&out, 128)
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn convert_tool_choice(
    tool_choice: Option<&Value>,
    parallel_tool_calls: Option<bool>,
    tool_names: &AnthropicToolNameMaps,
) -> Option<Value> {
    let mut mapped = match tool_choice {
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Some(json!({ "type": "auto" })),
            "required" | "any" => Some(json!({ "type": "any" })),
            "none" => Some(json!({ "type": "none" })),
            _ => None,
        },
        Some(Value::Object(obj)) => {
            if let Some(name) = obj
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
            {
                Some(json!({
                    "type": "tool",
                    "name": tool_names.forward.get(name).map(String::as_str).unwrap_or(name),
                }))
            } else {
                match obj.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "auto" => Some(json!({ "type": "auto" })),
                    "required" | "any" => Some(json!({ "type": "any" })),
                    "none" => Some(json!({ "type": "none" })),
                    "tool" => obj.get("name").and_then(|v| v.as_str()).map(|name| {
                        json!({
                            "type": "tool",
                            "name": tool_names.forward.get(name).map(String::as_str).unwrap_or(name),
                        })
                    }),
                    _ => None,
                }
            }
        }
        _ => None,
    };
    if let (Some(Value::Object(obj)), Some(parallel)) = (&mut mapped, parallel_tool_calls) {
        if obj.get("type").and_then(|v| v.as_str()) != Some("none") {
            obj.insert("disable_parallel_tool_use".into(), Value::Bool(!parallel));
        }
    } else if let Some(parallel) = parallel_tool_calls {
        mapped = Some(json!({
            "type": "auto",
            "disable_parallel_tool_use": !parallel,
        }));
    }
    mapped
}

fn convert_stop_sequences(stop: Option<&Value>) -> Option<Value> {
    match stop? {
        Value::String(s) if !s.is_empty() => Some(Value::Array(vec![Value::String(s.clone())])),
        Value::Array(items) => {
            let values = items
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_owned()))
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some(Value::Array(values))
        }
        _ => None,
    }
}

fn convert_thinking(body: &Map<String, Value>) -> Option<Value> {
    if let Some(thinking) = body.get("thinking").filter(|v| v.is_object()) {
        return Some(thinking.clone());
    }
    let effort = body
        .get("reasoning_effort")
        .and_then(|v| v.as_str())
        .map(str::to_ascii_lowercase)?;
    match effort.as_str() {
        "none" | "off" => None,
        "minimal" | "low" => Some(json!({ "type": "enabled", "budget_tokens": 1024 })),
        "medium" => Some(json!({ "type": "enabled", "budget_tokens": 4096 })),
        "high" => Some(json!({ "type": "enabled", "budget_tokens": 8192 })),
        "xhigh" | "max" => Some(json!({ "type": "enabled", "budget_tokens": 16384 })),
        _ => None,
    }
}

fn convert_metadata(body: &Map<String, Value>) -> Option<Value> {
    let user = body
        .get("user")
        .and_then(|v| v.as_str())
        .or_else(|| {
            body.get("metadata")
                .and_then(|v| v.get("user"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            body.get("metadata")
                .and_then(|v| v.get("user_id"))
                .and_then(|v| v.as_str())
        })?;
    is_valid_anthropic_user_id(user).then(|| json!({ "user_id": user }))
}

fn is_valid_anthropic_user_id(user_id: &str) -> bool {
    let trimmed = user_id.trim();
    if trimmed.is_empty() {
        return false;
    }
    let looks_like_email = trimmed.contains('@')
        && trimmed.rsplit_once('.').is_some()
        && !trimmed.contains(char::is_whitespace);
    if looks_like_email {
        return false;
    }
    let digit_count = trimmed.chars().filter(|ch| ch.is_ascii_digit()).count();
    let phone_chars_only = trimmed
        .chars()
        .all(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | '(' | ')' | ' '));
    !(phone_chars_only && digit_count >= 7)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_path_avoids_double_v1() {
        assert_eq!(
            build_anthropic_messages_upstream_path("https://api.anthropic.com/v1"),
            "/messages"
        );
        assert_eq!(
            build_anthropic_messages_upstream_path("https://api.anthropic.com"),
            "/v1/messages"
        );
        assert_eq!(
            build_anthropic_messages_upstream_path("https://proxy.example/anthropic/v1/"),
            "/messages"
        );
    }

    #[test]
    fn default_headers_include_anthropic_version() {
        let headers = anthropic_messages_default_headers();
        assert_eq!(
            headers
                .get("anthropic-version")
                .and_then(|v| v.to_str().ok()),
            Some(DEFAULT_ANTHROPIC_VERSION)
        );
        assert_eq!(
            headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    #[test]
    fn invalid_user_ids_are_filtered() {
        assert!(!is_valid_anthropic_user_id("a@example.com"));
        assert!(!is_valid_anthropic_user_id("+1 (555) 123-4567"));
        assert!(is_valid_anthropic_user_id("local-user-123"));
    }
}
