use std::collections::BTreeMap;
use std::pin::Pin;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::diagnostics::redact_diagnostics_text;
use crate::model_catalog::RouteResolution;
use crate::provider::{ApiFormat, AuthScheme, Provider};

const MAX_ERROR_PREVIEW_CHARS: usize = 512;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamHeader {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<UpstreamHeader>,
    pub body: Value,
    pub api_format: ApiFormat,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProviderResponse {
    pub status: u16,
    pub body: Value,
    pub content_type: String,
}

pub type GatewayProviderByteStream =
    Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

pub struct GatewayProviderStream {
    pub status: u16,
    pub content_type: String,
    pub body: GatewayProviderByteStream,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub redacted_preview: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicErrorEnvelope {
    #[serde(rename = "type")]
    pub envelope_type: String,
    pub error: AnthropicErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AnthropicErrorBody {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
    pub redacted_preview: String,
}

pub fn build_messages_upstream_request(
    provider: &Provider,
    route: &RouteResolution,
    body: Value,
) -> Result<UpstreamRequest, UpstreamError> {
    match provider.api_format {
        ApiFormat::Anthropic => build_anthropic_passthrough(provider, route, body),
        ApiFormat::OpenAiChat => build_openai_chat_request(provider, route, body),
    }
}

pub async fn forward_upstream_request(
    client: &reqwest::Client,
    request: UpstreamRequest,
    route: &RouteResolution,
) -> Result<GatewayProviderResponse, UpstreamError> {
    if request
        .body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(UpstreamError {
            status: 501,
            code: "gateway.streaming_not_implemented".to_owned(),
            message: "SSE streaming is not implemented yet".to_owned(),
            redacted_preview: String::new(),
        });
    }

    let mut builder = client.post(&request.url).json(&request.body);
    for header in &request.headers {
        builder = builder.header(header.name.as_str(), header.value.as_str());
    }

    let response = builder.send().await.map_err(|error| UpstreamError {
        status: 502,
        code: "provider.real_smoke_failed".to_owned(),
        message: "failed to reach upstream provider".to_owned(),
        redacted_preview: preview(&error.to_string()),
    })?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let body = response.text().await.map_err(|error| UpstreamError {
        status: 502,
        code: "provider.real_smoke_failed".to_owned(),
        message: "failed to read upstream response body".to_owned(),
        redacted_preview: preview(&error.to_string()),
    })?;

    normalize_upstream_response(
        status,
        Some(&content_type),
        &body,
        request.api_format,
        route,
    )
}

pub async fn forward_upstream_stream(
    client: &reqwest::Client,
    request: UpstreamRequest,
    route: &RouteResolution,
) -> Result<GatewayProviderStream, UpstreamError> {
    if !request_wants_stream(&request) {
        return Err(UpstreamError {
            status: 400,
            code: "provider.api_format_mismatch".to_owned(),
            message: "stream forwarding requires stream=true".to_owned(),
            redacted_preview: String::new(),
        });
    }

    let mut builder = client.post(&request.url).json(&request.body);
    for header in &request.headers {
        builder = builder.header(header.name.as_str(), header.value.as_str());
    }

    let response = builder.send().await.map_err(|error| UpstreamError {
        status: 502,
        code: "provider.real_smoke_failed".to_owned(),
        message: "failed to reach upstream provider".to_owned(),
        redacted_preview: preview(&error.to_string()),
    })?;
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();

    if !content_type
        .to_ascii_lowercase()
        .contains("text/event-stream")
    {
        let body = response.text().await.unwrap_or_default();
        return Err(normalize_upstream_response_error_for_route(
            status,
            Some(&content_type),
            &body,
            true,
            route,
        ));
    }

    if !(200..300).contains(&status) {
        let body = response.text().await.unwrap_or_default();
        return Err(UpstreamError {
            status,
            code: "provider.real_smoke_failed".to_owned(),
            message: "upstream provider returned an error status".to_owned(),
            redacted_preview: preview_for_route(&body, route),
        });
    }

    let mut normalizer = SseStreamNormalizer::new(
        request.api_format.clone(),
        route.upstream_model.clone(),
        route.route_id.clone(),
    );
    let body = response.bytes_stream().map(move |chunk| {
        chunk
            .map(|bytes| normalizer.normalize(bytes))
            .map_err(std::io::Error::other)
    });

    Ok(GatewayProviderStream {
        status,
        content_type: "text/event-stream".to_owned(),
        body: Box::pin(body),
    })
}

pub fn request_wants_stream(request: &UpstreamRequest) -> bool {
    request
        .body
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn normalize_upstream_response(
    status: u16,
    content_type: Option<&str>,
    body: &str,
    api_format: ApiFormat,
    route: &RouteResolution,
) -> Result<GatewayProviderResponse, UpstreamError> {
    let content_type = content_type.unwrap_or("");
    if !content_type
        .to_ascii_lowercase()
        .contains("application/json")
    {
        return Err(normalize_upstream_response_error_for_route(
            status,
            Some(content_type),
            body,
            false,
            route,
        ));
    }

    let parsed = serde_json::from_str::<Value>(body).map_err(|_| {
        normalize_upstream_response_error_for_route(status, Some(content_type), body, false, route)
    })?;
    if !(200..300).contains(&status) {
        return Err(UpstreamError {
            status,
            code: "provider.real_smoke_failed".to_owned(),
            message: "upstream provider returned an error status".to_owned(),
            redacted_preview: preview_for_route(body, route),
        });
    }

    let body = match api_format {
        ApiFormat::Anthropic => normalize_anthropic_response(parsed, route),
        ApiFormat::OpenAiChat => openai_chat_to_anthropic_response(parsed, route)?,
    };

    Ok(GatewayProviderResponse {
        status,
        body,
        content_type: "application/json".to_owned(),
    })
}

struct SseStreamNormalizer {
    api_format: ApiFormat,
    upstream_model: String,
    route_id: String,
    anthropic_buffer: String,
    openai_buffer: String,
    openai_message_started: bool,
    openai_content_block_index: Option<usize>,
    openai_next_block_index: usize,
    openai_tool_blocks: BTreeMap<u64, OpenAiStreamToolBlock>,
    openai_message_stopped: bool,
}

#[derive(Clone, Debug)]
struct OpenAiStreamToolBlock {
    index: usize,
    id: Option<String>,
    name: Option<String>,
    started: bool,
}

impl SseStreamNormalizer {
    fn new(api_format: ApiFormat, upstream_model: String, route_id: String) -> Self {
        Self {
            api_format,
            upstream_model,
            route_id,
            anthropic_buffer: String::new(),
            openai_buffer: String::new(),
            openai_message_started: false,
            openai_content_block_index: None,
            openai_next_block_index: 0,
            openai_tool_blocks: BTreeMap::new(),
            openai_message_stopped: false,
        }
    }

    fn normalize(&mut self, chunk: Bytes) -> Bytes {
        match self.api_format {
            ApiFormat::Anthropic => self.normalize_anthropic_sse_chunk(chunk),
            ApiFormat::OpenAiChat => self.normalize_openai_chat_sse_chunk(chunk),
        }
    }

    fn normalize_anthropic_sse_chunk(&mut self, chunk: Bytes) -> Bytes {
        self.anthropic_buffer
            .push_str(&String::from_utf8_lossy(&chunk));
        let mut output = String::new();

        while let Some((index, separator_len)) = next_sse_frame_boundary(&self.anthropic_buffer) {
            let frame = self.anthropic_buffer[..index].to_owned();
            self.anthropic_buffer = self.anthropic_buffer[index + separator_len..].to_owned();
            output.push_str(&normalize_anthropic_sse_frame(
                &frame,
                &self.upstream_model,
                &self.route_id,
            ));
        }

        Bytes::from(output)
    }

    fn normalize_openai_chat_sse_chunk(&mut self, chunk: Bytes) -> Bytes {
        self.openai_buffer
            .push_str(&String::from_utf8_lossy(&chunk));
        let mut output = String::new();

        while let Some((index, separator_len)) = next_sse_frame_boundary(&self.openai_buffer) {
            let frame = self.openai_buffer[..index].to_owned();
            self.openai_buffer = self.openai_buffer[index + separator_len..].to_owned();
            output.push_str(&self.normalize_openai_chat_sse_frame(&frame));
        }

        Bytes::from(output)
    }

    fn normalize_openai_chat_sse_frame(&mut self, frame: &str) -> String {
        let Some(data) = sse_data_payload(frame) else {
            return String::new();
        };
        if data == "[DONE]" {
            return self.openai_stop_events("end_turn");
        }

        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return String::new();
        };
        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
        else {
            return String::new();
        };

        let mut events = String::new();
        if !self.openai_message_started {
            self.openai_message_started = true;
            let message_id = self.sanitize_text(
                value
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("ccds-openai-stream"),
            );
            events.push_str(&sse_event(
                "message_start",
                serde_json::json!({
                    "type": "message_start",
                    "message": {
                        "id": message_id,
                        "type": "message",
                        "role": "assistant",
                        "model": self.route_id.clone(),
                        "content": [],
                        "stop_reason": null,
                        "stop_sequence": null,
                        "usage": {"input_tokens": 0, "output_tokens": 0}
                    }
                }),
            ));
        }

        if let Some(content) = choice
            .get("delta")
            .and_then(Value::as_object)
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
        {
            if !content.is_empty() {
                let index = self.ensure_openai_text_block(&mut events);
                let content = self.sanitize_text(content);
                events.push_str(&sse_event(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "text_delta", "text": content}
                    }),
                ));
            }
        }

        if let Some(tool_calls) = choice
            .get("delta")
            .and_then(Value::as_object)
            .and_then(|delta| delta.get("tool_calls"))
            .and_then(Value::as_array)
        {
            for tool_call in tool_calls {
                if let Some(tool_call) = tool_call.as_object() {
                    self.handle_openai_tool_call_delta(tool_call, &mut events);
                }
            }
        }

        if let Some(finish_reason) = choice.get("finish_reason").and_then(Value::as_str) {
            if !finish_reason.is_empty() {
                events.push_str(&self.openai_stop_events(openai_stop_reason(finish_reason)));
            }
        }

        events
    }

    fn openai_stop_events(&mut self, stop_reason: &str) -> String {
        if self.openai_message_stopped {
            return String::new();
        }
        self.openai_message_stopped = true;

        let mut events = String::new();
        if let Some(index) = self.openai_content_block_index {
            events.push_str(&sse_event(
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop",
                    "index": index
                }),
            ));
        }
        for block in self.openai_tool_blocks.values() {
            if block.started {
                events.push_str(&sse_event(
                    "content_block_stop",
                    serde_json::json!({
                        "type": "content_block_stop",
                        "index": block.index
                    }),
                ));
            }
        }
        events.push_str(&sse_event(
            "message_delta",
            serde_json::json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": stop_reason,
                    "stop_sequence": null
                },
                "usage": {"output_tokens": 0}
            }),
        ));
        events.push_str(&sse_event(
            "message_stop",
            serde_json::json!({
                "type": "message_stop"
            }),
        ));
        events
    }

    fn ensure_openai_text_block(&mut self, events: &mut String) -> usize {
        if let Some(index) = self.openai_content_block_index {
            return index;
        }
        let index = self.openai_next_block_index;
        self.openai_next_block_index += 1;
        self.openai_content_block_index = Some(index);
        events.push_str(&sse_event(
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {"type": "text", "text": ""}
            }),
        ));
        index
    }

    fn handle_openai_tool_call_delta(
        &mut self,
        tool_call: &Map<String, Value>,
        events: &mut String,
    ) {
        let upstream_index = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or(self.openai_tool_blocks.len() as u64);
        if !self.openai_tool_blocks.contains_key(&upstream_index) {
            let index = self.openai_next_block_index;
            self.openai_next_block_index += 1;
            self.openai_tool_blocks.insert(
                upstream_index,
                OpenAiStreamToolBlock {
                    index,
                    id: None,
                    name: None,
                    started: false,
                },
            );
        }

        let block = self
            .openai_tool_blocks
            .get_mut(&upstream_index)
            .expect("tool block must exist");
        if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
            if !id.trim().is_empty() {
                block.id = Some(replace_upstream_model_text(
                    id,
                    &self.upstream_model,
                    &self.route_id,
                ));
            }
        }
        if let Some(name) = tool_call
            .get("function")
            .and_then(Value::as_object)
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
        {
            if !name.trim().is_empty() {
                block.name = Some(replace_upstream_model_text(
                    name,
                    &self.upstream_model,
                    &self.route_id,
                ));
            }
        }
        if !block.started {
            block.started = true;
            events.push_str(&sse_event(
                "content_block_start",
                serde_json::json!({
                    "type": "content_block_start",
                    "index": block.index,
                    "content_block": {
                        "type": "tool_use",
                        "id": block.id.clone().unwrap_or_else(|| format!("toolu_{upstream_index}")),
                        "name": block.name.clone().unwrap_or_else(|| "tool_call".to_owned()),
                        "input": {}
                    }
                }),
            ));
        }
        if let Some(arguments) = tool_call
            .get("function")
            .and_then(Value::as_object)
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
        {
            if !arguments.is_empty() {
                let arguments =
                    replace_upstream_model_text(arguments, &self.upstream_model, &self.route_id);
                events.push_str(&sse_event(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": block.index,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": arguments
                        }
                    }),
                ));
            }
        }
    }

    fn sanitize_text(&self, text: &str) -> String {
        replace_upstream_model_text(text, &self.upstream_model, &self.route_id)
    }
}

fn normalize_anthropic_sse_frame(frame: &str, upstream_model: &str, route_id: &str) -> String {
    let mut lines = Vec::new();
    for line in frame.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                lines.push("data: [DONE]".to_owned());
                continue;
            }
            if let Ok(mut value) = serde_json::from_str::<Value>(data) {
                replace_model_values(&mut value, upstream_model, route_id);
                lines.push(format!(
                    "data: {}",
                    serde_json::to_string(&value).unwrap_or_else(|_| {
                        replace_upstream_model_text(data, upstream_model, route_id)
                    })
                ));
            } else {
                lines.push(format!(
                    "data: {}",
                    replace_upstream_model_text(data, upstream_model, route_id)
                ));
            }
        } else {
            lines.push(replace_upstream_model_text(line, upstream_model, route_id));
        }
    }
    format!("{}\n\n", lines.join("\n"))
}

fn replace_model_values(value: &mut Value, upstream_model: &str, route_id: &str) {
    match value {
        Value::String(text)
            if !upstream_model.is_empty() && text != route_id && text.contains(upstream_model) =>
        {
            *text = text.replace(upstream_model, route_id);
        }
        Value::String(_) => {}
        Value::Array(items) => {
            for item in items {
                replace_model_values(item, upstream_model, route_id);
            }
        }
        Value::Object(object) => {
            for item in object.values_mut() {
                replace_model_values(item, upstream_model, route_id);
            }
        }
        _ => {}
    }
}

fn replace_upstream_model_text(text: &str, upstream_model: &str, route_id: &str) -> String {
    if upstream_model.is_empty() {
        text.to_owned()
    } else {
        text.replace(upstream_model, route_id)
    }
}

fn next_sse_frame_boundary(buffer: &str) -> Option<(usize, usize)> {
    let lf = buffer.find("\n\n").map(|index| (index, 2));
    let crlf = buffer.find("\r\n\r\n").map(|index| (index, 4));
    match (lf, crlf) {
        (Some(lf), Some(crlf)) => Some(if lf.0 <= crlf.0 { lf } else { crlf }),
        (Some(lf), None) => Some(lf),
        (None, Some(crlf)) => Some(crlf),
        (None, None) => None,
    }
}

fn sse_data_payload(frame: &str) -> Option<&str> {
    frame
        .lines()
        .find_map(|line| line.strip_prefix("data:").map(str::trim))
}

fn sse_event(event: &str, data: Value) -> String {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(&data).unwrap()
    )
}

fn openai_stop_reason(reason: &str) -> &str {
    match reason {
        "length" => "max_tokens",
        "content_filter" => "stop_sequence",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    }
}

pub fn anthropic_error_payload(error: &UpstreamError) -> AnthropicErrorEnvelope {
    AnthropicErrorEnvelope {
        envelope_type: "error".to_owned(),
        error: AnthropicErrorBody {
            error_type: error.code.clone(),
            message: error.message.clone(),
            redacted_preview: error.redacted_preview.clone(),
        },
    }
}

fn normalize_anthropic_response(mut body: Value, route: &RouteResolution) -> Value {
    if let Some(object) = body.as_object_mut() {
        object.insert("model".to_owned(), Value::String(route.route_id.clone()));
    }
    replace_model_values(&mut body, &route.upstream_model, &route.route_id);
    body
}

fn openai_chat_to_anthropic_response(
    body: Value,
    route: &RouteResolution,
) -> Result<Value, UpstreamError> {
    let object = body.as_object().ok_or_else(|| {
        adapter_error(
            "provider.api_format_mismatch",
            "OpenAI Chat response must be a JSON object",
        )
    })?;
    let choice = object
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(Value::as_object)
        .ok_or_else(|| {
            adapter_error(
                "provider.api_format_mismatch",
                "OpenAI Chat response must contain choices",
            )
        })?;
    let message = choice
        .get("message")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            adapter_error(
                "provider.api_format_mismatch",
                "OpenAI Chat response choice must contain a message",
            )
        })?;
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("end_turn");
    let usage = object.get("usage").cloned().unwrap_or_else(|| {
        serde_json::json!({
            "prompt_tokens": 0,
            "completion_tokens": 0
        })
    });

    let mut response = serde_json::json!({
        "id": object.get("id").and_then(Value::as_str).unwrap_or("ccds-openai-chat"),
        "type": "message",
        "role": "assistant",
        "model": route.route_id.clone(),
        "content": openai_message_to_anthropic_content(message),
        "stop_reason": openai_stop_reason(stop_reason),
        "stop_sequence": null,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
            "output_tokens": usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0)
        }
    });
    replace_model_values(&mut response, &route.upstream_model, &route.route_id);
    Ok(response)
}

pub fn normalize_upstream_response_error(
    status: u16,
    content_type: Option<&str>,
    body: &str,
    expected_stream: bool,
) -> UpstreamError {
    normalize_upstream_response_error_inner(status, content_type, body, expected_stream, None)
}

fn normalize_upstream_response_error_for_route(
    status: u16,
    content_type: Option<&str>,
    body: &str,
    expected_stream: bool,
    route: &RouteResolution,
) -> UpstreamError {
    normalize_upstream_response_error_inner(
        status,
        content_type,
        body,
        expected_stream,
        Some(route),
    )
}

fn normalize_upstream_response_error_inner(
    status: u16,
    content_type: Option<&str>,
    body: &str,
    expected_stream: bool,
    route: Option<&RouteResolution>,
) -> UpstreamError {
    let content_type = content_type.unwrap_or("").to_ascii_lowercase();
    let is_json = content_type.contains("application/json");
    let is_event_stream = content_type.contains("text/event-stream");
    let code = if expected_stream && !is_event_stream {
        "gateway.invalid_stream_content_type"
    } else if !is_json && !is_event_stream {
        "gateway.invalid_upstream_response"
    } else {
        "provider.real_smoke_failed"
    };

    UpstreamError {
        status,
        code: code.to_owned(),
        message: "upstream provider returned an unsupported response".to_owned(),
        redacted_preview: preview_with_optional_route(body, route),
    }
}

fn build_anthropic_passthrough(
    provider: &Provider,
    route: &RouteResolution,
    body: Value,
) -> Result<UpstreamRequest, UpstreamError> {
    let mut object = object_body(body)?;
    object.insert(
        "model".to_owned(),
        Value::String(route.upstream_model.clone()),
    );

    Ok(UpstreamRequest {
        method: "POST".to_owned(),
        url: messages_url(provider),
        headers: auth_headers(provider),
        body: Value::Object(object),
        api_format: ApiFormat::Anthropic,
    })
}

fn build_openai_chat_request(
    provider: &Provider,
    route: &RouteResolution,
    body: Value,
) -> Result<UpstreamRequest, UpstreamError> {
    let object = object_body(body)?;
    let mut request = Map::new();
    request.insert(
        "model".to_owned(),
        Value::String(route.upstream_model.clone()),
    );

    let mut messages = Vec::new();
    if let Some(system) = object.get("system").and_then(Value::as_str) {
        if !system.trim().is_empty() {
            messages.push(openai_message("system", system.trim()));
        }
    }

    let anthropic_messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            adapter_error("provider.api_format_mismatch", "messages must be an array")
        })?;
    for message in anthropic_messages {
        let Some(message_object) = message.as_object() else {
            return Err(adapter_error(
                "provider.api_format_mismatch",
                "message entries must be objects",
            ));
        };
        let role = message_object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        messages.extend(openai_messages_from_anthropic_message(
            role,
            message_object,
        )?);
    }

    request.insert("messages".to_owned(), Value::Array(messages));
    copy_if_present(&object, &mut request, "max_tokens");
    copy_if_present(&object, &mut request, "temperature");
    copy_if_present(&object, &mut request, "top_p");
    copy_if_present(&object, &mut request, "stream");
    if let Some(tools) = object.get("tools") {
        request.insert("tools".to_owned(), openai_tools_from_anthropic(tools)?);
    }
    if let Some(tool_choice) = object.get("tool_choice") {
        request.insert(
            "tool_choice".to_owned(),
            openai_tool_choice_from_anthropic(tool_choice),
        );
    }

    Ok(UpstreamRequest {
        method: "POST".to_owned(),
        url: chat_completions_url(provider),
        headers: auth_headers(provider),
        body: Value::Object(request),
        api_format: ApiFormat::OpenAiChat,
    })
}

fn object_body(body: Value) -> Result<Map<String, Value>, UpstreamError> {
    body.as_object().cloned().ok_or_else(|| {
        adapter_error(
            "provider.api_format_mismatch",
            "request body must be a JSON object",
        )
    })
}

fn auth_headers(provider: &Provider) -> Vec<UpstreamHeader> {
    let mut headers = Vec::new();
    match provider.auth_scheme {
        AuthScheme::Bearer => headers.push(UpstreamHeader {
            name: "authorization".to_owned(),
            value: format!("Bearer {}", provider.api_key),
        }),
        AuthScheme::XApiKey => headers.push(UpstreamHeader {
            name: "x-api-key".to_owned(),
            value: provider.api_key.clone(),
        }),
        AuthScheme::None => {}
    }
    headers.push(UpstreamHeader {
        name: "content-type".to_owned(),
        value: "application/json".to_owned(),
    });
    headers
}

fn messages_url(provider: &Provider) -> String {
    format!("{}/v1/messages", provider.base_url.trim_end_matches('/'))
}

fn chat_completions_url(provider: &Provider) -> String {
    format!(
        "{}/v1/chat/completions",
        provider.base_url.trim_end_matches('/')
    )
}

fn openai_message(role: &str, content: &str) -> Value {
    serde_json::json!({
        "role": role,
        "content": content,
    })
}

fn openai_messages_from_anthropic_message(
    role: &str,
    message_object: &Map<String, Value>,
) -> Result<Vec<Value>, UpstreamError> {
    let content = message_object.get("content").unwrap_or(&Value::Null);
    if role == "assistant" {
        let (text, tool_calls) = openai_assistant_content(content)?;
        let mut message = Map::new();
        message.insert("role".to_owned(), Value::String("assistant".to_owned()));
        message.insert(
            "content".to_owned(),
            if text.trim().is_empty() {
                Value::Null
            } else {
                Value::String(text)
            },
        );
        if !tool_calls.is_empty() {
            message.insert("tool_calls".to_owned(), Value::Array(tool_calls));
        }
        return Ok(vec![Value::Object(message)]);
    }

    if role == "user" {
        return Ok(openai_user_messages(content));
    }

    Ok(vec![openai_message(role, &openai_content(content))])
}

fn openai_assistant_content(value: &Value) -> Result<(String, Vec<Value>), UpstreamError> {
    let mut text = Vec::new();
    let mut tool_calls = Vec::new();
    match value {
        Value::Array(parts) => {
            for part in parts {
                let Some(part) = part.as_object() else {
                    if let Some(part_text) = part.as_str() {
                        text.push(part_text.to_owned());
                    }
                    continue;
                };
                match part.get("type").and_then(Value::as_str) {
                    Some("tool_use") => {
                        let id = part
                            .get("id")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or("toolu_ccds");
                        let name = part
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|value| !value.trim().is_empty())
                            .ok_or_else(|| {
                                adapter_error(
                                    "provider.api_format_mismatch",
                                    "tool_use content requires a tool name",
                                )
                            })?;
                        let input = part
                            .get("input")
                            .cloned()
                            .unwrap_or_else(|| Value::Object(Map::new()));
                        tool_calls.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_owned())
                            }
                        }));
                    }
                    _ => {
                        if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                            text.push(part_text.to_owned());
                        }
                    }
                }
            }
        }
        _ => text.push(openai_content(value)),
    }
    Ok((text.join("\n"), tool_calls))
}

fn openai_user_messages(value: &Value) -> Vec<Value> {
    let mut messages = Vec::new();
    match value {
        Value::Array(parts) => {
            let mut text = Vec::new();
            for part in parts {
                let Some(part_object) = part.as_object() else {
                    if let Some(part_text) = part.as_str() {
                        text.push(part_text.to_owned());
                    }
                    continue;
                };
                if part_object.get("type").and_then(Value::as_str) == Some("tool_result") {
                    if !text.is_empty() {
                        messages.push(openai_message("user", &text.join("\n")));
                        text.clear();
                    }
                    let tool_call_id = part_object
                        .get("tool_use_id")
                        .or_else(|| part_object.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("toolu_ccds");
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": openai_content(part_object.get("content").unwrap_or(&Value::Null))
                    }));
                } else if let Some(part_text) = part_object.get("text").and_then(Value::as_str) {
                    text.push(part_text.to_owned());
                }
            }
            if !text.is_empty() || messages.is_empty() {
                messages.push(openai_message("user", &text.join("\n")));
            }
        }
        _ => messages.push(openai_message("user", &openai_content(value))),
    }
    messages
}

fn openai_tools_from_anthropic(value: &Value) -> Result<Value, UpstreamError> {
    let tools = value
        .as_array()
        .ok_or_else(|| adapter_error("provider.api_format_mismatch", "tools must be an array"))?;
    Ok(Value::Array(
        tools
            .iter()
            .filter_map(|tool| {
                let object = tool.as_object()?;
                if object.get("type").and_then(Value::as_str) == Some("function") {
                    return Some(Value::Object(object.clone()));
                }
                let name = object.get("name")?.as_str()?;
                let mut function = Map::new();
                function.insert("name".to_owned(), Value::String(name.to_owned()));
                if let Some(description) = object.get("description").and_then(Value::as_str) {
                    function.insert(
                        "description".to_owned(),
                        Value::String(description.to_owned()),
                    );
                }
                function.insert(
                    "parameters".to_owned(),
                    object
                        .get("input_schema")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!({"type": "object"})),
                );
                Some(serde_json::json!({
                    "type": "function",
                    "function": Value::Object(function)
                }))
            })
            .collect(),
    ))
}

fn openai_tool_choice_from_anthropic(value: &Value) -> Value {
    if let Some(choice) = value.as_str() {
        return Value::String(choice.to_owned());
    }
    let Some(object) = value.as_object() else {
        return value.clone();
    };
    match object.get("type").and_then(Value::as_str).unwrap_or("auto") {
        "tool" => object
            .get("name")
            .and_then(Value::as_str)
            .map(|name| {
                serde_json::json!({
                    "type": "function",
                    "function": {"name": name}
                })
            })
            .unwrap_or_else(|| Value::String("auto".to_owned())),
        "any" => Value::String("required".to_owned()),
        "none" => Value::String("none".to_owned()),
        _ => Value::String("auto".to_owned()),
    }
}

fn openai_message_to_anthropic_content(message: &Map<String, Value>) -> Vec<Value> {
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content.push(serde_json::json!({"type": "text", "text": text}));
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let Some(tool_call) = tool_call.as_object() else {
                continue;
            };
            let function = tool_call
                .get("function")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let arguments = function
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input = serde_json::from_str::<Value>(arguments)
                .unwrap_or_else(|_| serde_json::json!({"arguments": arguments}));
            content.push(serde_json::json!({
                "type": "tool_use",
                "id": tool_call.get("id").and_then(Value::as_str).unwrap_or("toolu_ccds"),
                "name": function.get("name").and_then(Value::as_str).unwrap_or("tool_call"),
                "input": input
            }));
        }
    }
    content
}

fn openai_content(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                if let Some(text) = part
                    .as_object()
                    .and_then(|part| part.get("text"))
                    .and_then(Value::as_str)
                {
                    return Some(text.to_owned());
                }
                part.as_str().map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn copy_if_present(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

fn adapter_error(code: &str, message: &str) -> UpstreamError {
    UpstreamError {
        status: 400,
        code: code.to_owned(),
        message: message.to_owned(),
        redacted_preview: String::new(),
    }
}

fn preview(body: &str) -> String {
    redact_diagnostics_text(body)
        .chars()
        .take(MAX_ERROR_PREVIEW_CHARS)
        .collect()
}

fn preview_for_route(body: &str, route: &RouteResolution) -> String {
    preview_with_optional_route(body, Some(route))
}

fn preview_with_optional_route(body: &str, route: Option<&RouteResolution>) -> String {
    let mut preview_source = redact_diagnostics_text(body);
    if let Some(route) = route {
        preview_source =
            replace_upstream_model_text(&preview_source, &route.upstream_model, &route.route_id);
    }
    preview_source
        .chars()
        .take(MAX_ERROR_PREVIEW_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Json, Router};
    use futures_util::StreamExt as _;
    use tokio::net::TcpListener;

    use crate::model_catalog::RouteResolution;

    use super::*;

    fn provider(api_format: ApiFormat) -> Provider {
        provider_with_base_url(api_format, "https://api.example.test".to_owned())
    }

    fn provider_with_base_url(api_format: ApiFormat, base_url: String) -> Provider {
        Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url,
            auth_scheme: AuthScheme::Bearer,
            api_format,
            api_key: "sk-provider-secret".to_owned(),
        }
    }

    fn route() -> RouteResolution {
        RouteResolution {
            route_id: "claude-deepseek-v4-pro".to_owned(),
            provider_id: "provider-deepseek".to_owned(),
            upstream_model: "deepseek-v4-pro".to_owned(),
            supports_1m: true,
            supports_max: false,
        }
    }

    fn disjoint_route() -> RouteResolution {
        RouteResolution {
            route_id: "claude-safe-route".to_owned(),
            provider_id: "provider-deepseek".to_owned(),
            upstream_model: "raw-upstream-model".to_owned(),
            supports_1m: true,
            supports_max: false,
        }
    }

    async fn spawn_json_upstream(path: &'static str, response: Value) -> String {
        async fn handler(
            State(response): State<Arc<Value>>,
            Json(_body): Json<Value>,
        ) -> Json<Value> {
            Json((*response).clone())
        }

        let app = Router::new()
            .route(path, post(handler))
            .with_state(Arc::new(response));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[test]
    fn anthropic_passthrough_replaces_safe_route_with_upstream_model() {
        let request = build_messages_upstream_request(
            &provider(ApiFormat::Anthropic),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 128
            }),
        )
        .unwrap();

        assert_eq!(request.api_format, ApiFormat::Anthropic);
        assert_eq!(request.url, "https://api.example.test/v1/messages");
        assert_eq!(request.body["model"], "deepseek-v4-pro");
        assert_eq!(request.body["messages"][0]["content"], "hello");
        assert!(request
            .headers
            .iter()
            .any(|header| header.name == "authorization"));
    }

    #[test]
    fn auth_headers_follow_provider_auth_scheme() {
        let mut provider = provider(ApiFormat::Anthropic);
        provider.auth_scheme = AuthScheme::XApiKey;
        let headers = auth_headers(&provider);
        assert!(headers
            .iter()
            .any(|header| header.name == "x-api-key" && header.value == "sk-provider-secret"));
        assert!(!headers.iter().any(|header| header.name == "authorization"));

        provider.auth_scheme = AuthScheme::None;
        let headers = auth_headers(&provider);
        assert!(!headers.iter().any(|header| header.name == "x-api-key"));
        assert!(!headers.iter().any(|header| header.name == "authorization"));
        assert!(headers.iter().any(|header| header.name == "content-type"));
    }

    #[test]
    fn openai_chat_conversion_maps_messages_and_sampling_fields() {
        let request = build_messages_upstream_request(
            &provider(ApiFormat::OpenAiChat),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "system": "You are concise.",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "text", "text": "hello"}, {"type": "text", "text": "world"}]
                }],
                "max_tokens": 256,
                "temperature": 0.2,
                "stream": true
            }),
        )
        .unwrap();

        assert_eq!(request.api_format, ApiFormat::OpenAiChat);
        assert_eq!(request.url, "https://api.example.test/v1/chat/completions");
        assert_eq!(request.body["model"], "deepseek-v4-pro");
        assert_eq!(request.body["messages"][0]["role"], "system");
        assert_eq!(request.body["messages"][0]["content"], "You are concise.");
        assert_eq!(request.body["messages"][1]["role"], "user");
        assert_eq!(request.body["messages"][1]["content"], "hello\nworld");
        assert_eq!(request.body["max_tokens"], 256);
        assert_eq!(request.body["stream"], true);
    }

    #[test]
    fn openai_chat_conversion_preserves_tools_and_tool_results() {
        let request = build_messages_upstream_request(
            &provider(ApiFormat::OpenAiChat),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "tools": [{
                    "name": "search_docs",
                    "description": "Search documents",
                    "input_schema": {
                        "type": "object",
                        "properties": {"query": {"type": "string"}}
                    }
                }],
                "tool_choice": {"type": "tool", "name": "search_docs"},
                "messages": [
                    {
                        "role": "assistant",
                        "content": [
                            {"type": "text", "text": "I will search."},
                            {"type": "tool_use", "id": "toolu_1", "name": "search_docs", "input": {"query": "ccds"}}
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "found"}]},
                            {"type": "text", "text": "summarize"}
                        ]
                    }
                ],
                "max_tokens": 256
            }),
        )
        .unwrap();

        assert_eq!(request.body["tools"][0]["type"], "function");
        assert_eq!(
            request.body["tools"][0]["function"]["parameters"]["properties"]["query"]["type"],
            "string"
        );
        assert_eq!(
            request.body["tool_choice"]["function"]["name"],
            "search_docs"
        );
        assert_eq!(request.body["messages"][0]["role"], "assistant");
        assert_eq!(
            request.body["messages"][0]["tool_calls"][0]["function"]["name"],
            "search_docs"
        );
        assert_eq!(request.body["messages"][1]["role"], "tool");
        assert_eq!(request.body["messages"][1]["tool_call_id"], "toolu_1");
        assert_eq!(request.body["messages"][2]["content"], "summarize");
    }

    #[test]
    fn openai_chat_conversion_rejects_invalid_message_shape() {
        let error = build_messages_upstream_request(
            &provider(ApiFormat::OpenAiChat),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "messages": "not-an-array"
            }),
        )
        .unwrap_err();

        assert_eq!(error.status, 400);
        assert_eq!(error.code, "provider.api_format_mismatch");
    }

    #[tokio::test]
    async fn forward_anthropic_passthrough_normalizes_response_model_to_safe_route() {
        let base_url = spawn_json_upstream(
            "/v1/messages",
            serde_json::json!({
                "id": "msg-test",
                "type": "message",
                "role": "assistant",
                "model": "deepseek-v4-pro",
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }),
        )
        .await;
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::Anthropic, base_url),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32
            }),
        )
        .unwrap();

        let response =
            forward_upstream_request(&reqwest::Client::new(), upstream_request, &route())
                .await
                .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body["model"], "claude-deepseek-v4-pro");
        assert_eq!(response.body["content"][0]["text"], "ok");
        assert!(!serde_json::to_string(&response)
            .unwrap()
            .contains(r#""model":"deepseek-v4-pro""#));
    }

    #[tokio::test]
    async fn forward_anthropic_passthrough_replaces_raw_model_in_success_payload_values() {
        let route = disjoint_route();
        let base_url = spawn_json_upstream(
            "/v1/messages",
            serde_json::json!({
                "id": "msg_raw-upstream-model",
                "type": "message",
                "role": "assistant",
                "model": "raw-upstream-model",
                "content": [{"type": "text", "text": "answer from raw-upstream-model"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }),
        )
        .await;
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::Anthropic, base_url),
            &route,
            serde_json::json!({
                "model": "claude-safe-route",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32
            }),
        )
        .unwrap();

        let response = forward_upstream_request(&reqwest::Client::new(), upstream_request, &route)
            .await
            .unwrap();
        let serialized = serde_json::to_string(&response.body).unwrap();

        assert_eq!(response.status, 200);
        assert!(!serialized.contains("raw-upstream-model"));
        assert!(serialized.contains("claude-safe-route"));
    }

    #[test]
    fn openai_chat_response_tool_calls_convert_to_anthropic_tool_use() {
        let response = openai_chat_to_anthropic_response(
            serde_json::json!({
                "id": "chatcmpl-tools",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {
                                "name": "search_docs",
                                "arguments": "{\"query\":\"ccds\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 5, "completion_tokens": 2}
            }),
            &route(),
        )
        .unwrap();

        assert_eq!(response["model"], "claude-deepseek-v4-pro");
        assert_eq!(response["stop_reason"], "tool_use");
        assert_eq!(response["content"][0]["type"], "tool_use");
        assert_eq!(response["content"][0]["name"], "search_docs");
        assert_eq!(response["content"][0]["input"]["query"], "ccds");
        assert!(!serde_json::to_string(&response)
            .unwrap()
            .contains(r#""model":"deepseek-v4-pro""#));
    }

    #[test]
    fn openai_chat_response_replaces_raw_model_in_success_payload_values() {
        let route = disjoint_route();
        let response = openai_chat_to_anthropic_response(
            serde_json::json!({
                "id": "chatcmpl_raw-upstream-model",
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": "answer from raw-upstream-model",
                        "tool_calls": [{
                            "id": "call_raw-upstream-model",
                            "type": "function",
                            "function": {
                                "name": "lookup_raw-upstream-model",
                                "arguments": "{\"query\":\"raw-upstream-model\"}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 5, "completion_tokens": 2}
            }),
            &route,
        )
        .unwrap();
        let serialized = serde_json::to_string(&response).unwrap();

        assert!(!serialized.contains("raw-upstream-model"));
        assert!(serialized.contains("claude-safe-route"));
        assert_eq!(response["model"], "claude-safe-route");
    }

    #[tokio::test]
    async fn forward_invalid_html_response_returns_redacted_error() {
        async fn handler() -> (
            axum::http::StatusCode,
            [(&'static str, &'static str); 1],
            String,
        ) {
            (
                axum::http::StatusCode::OK,
                [("content-type", "text/html")],
                "<html>sk-upstream-secret raw-upstream-model</html>".to_owned(),
            )
        }

        let app = Router::new().route("/v1/messages", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::Anthropic, format!("http://{addr}")),
            &disjoint_route(),
            serde_json::json!({
                "model": "claude-safe-route",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32
            }),
        )
        .unwrap();

        let error =
            forward_upstream_request(&reqwest::Client::new(), upstream_request, &disjoint_route())
                .await
                .unwrap_err();

        assert_eq!(error.code, "gateway.invalid_upstream_response");
        assert!(!error.redacted_preview.contains("sk-upstream-secret"));
        assert!(!error.redacted_preview.contains("raw-upstream-model"));
        assert!(error.redacted_preview.contains("claude-safe-route"));
        assert!(error.redacted_preview.contains("[REDACTED:key]"));
    }

    #[test]
    fn anthropic_sse_normalizer_handles_split_frames_and_model_whitespace() {
        let mut normalizer = SseStreamNormalizer::new(
            ApiFormat::Anthropic,
            "raw-upstream-model".to_owned(),
            "claude-safe-route".to_owned(),
        );

        let first = normalizer.normalize(Bytes::from_static(
            br#"event: message_start
data: {"type":"message_start","message":{"model" : "raw-up"#,
        ));
        let second = normalizer.normalize(Bytes::from_static(
            br#"stream-model","nested":["raw-upstream-model"]}}

"#,
        ));
        let third = normalizer.normalize(Bytes::from_static(
            br#"event: error
data: {"type":"error","error":{"message":"model raw-upstream-model overloaded"}}

"#,
        ));
        let body = format!(
            "{}{}{}",
            String::from_utf8(first.to_vec()).unwrap(),
            String::from_utf8(second.to_vec()).unwrap(),
            String::from_utf8(third.to_vec()).unwrap()
        );

        assert!(body.contains("claude-safe-route"));
        assert!(!body.contains("raw-upstream-model"));
    }

    #[test]
    fn sse_normalizer_accepts_crlf_frame_boundaries() {
        let mut anthropic = SseStreamNormalizer::new(
            ApiFormat::Anthropic,
            "raw-upstream-model".to_owned(),
            "claude-safe-route".to_owned(),
        );
        let anthropic_body = String::from_utf8(
            anthropic
                .normalize(Bytes::from_static(
                    b"event: message_start\r\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"raw-upstream-model\"}}\r\n\r\n",
                ))
                .to_vec(),
        )
        .unwrap();
        assert!(anthropic_body.contains("claude-safe-route"));
        assert!(!anthropic_body.contains("raw-upstream-model"));

        let mut openai = SseStreamNormalizer::new(
            ApiFormat::OpenAiChat,
            "raw-upstream-model".to_owned(),
            "claude-safe-route".to_owned(),
        );
        let openai_body = String::from_utf8(
            openai
                .normalize(Bytes::from_static(
                    b"data: {\"id\":\"chatcmpl_raw-upstream-model\",\"choices\":[{\"delta\":{\"content\":\"raw-upstream-model\"},\"finish_reason\":null}]}\r\n\r\n",
                ))
                .to_vec(),
        )
        .unwrap();
        assert!(openai_body.contains("claude-safe-route"));
        assert!(!openai_body.contains("raw-upstream-model"));
    }

    #[tokio::test]
    async fn forward_anthropic_stream_preserves_event_stream_and_safe_route() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "text/event-stream")],
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"deepseek-v4-pro\"}}\n\n",
            )
        }

        let app = Router::new().route("/v1/messages", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::Anthropic, format!("http://{addr}")),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32,
                "stream": true
            }),
        )
        .unwrap();

        let mut response =
            forward_upstream_stream(&reqwest::Client::new(), upstream_request, &route())
                .await
                .unwrap();
        let mut body = Vec::new();
        while let Some(chunk) = response.body.next().await {
            body.extend_from_slice(&chunk.unwrap());
        }
        let body = String::from_utf8(body).unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, "text/event-stream");
        assert!(body.contains(r#""model":"claude-deepseek-v4-pro""#));
        assert!(!body.contains(r#""model":"deepseek-v4-pro""#));
    }

    #[tokio::test]
    async fn forward_openai_stream_converts_chat_chunks_to_anthropic_events() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "text/event-stream")],
                "data: {\"id\":\"chatcmpl-test\",\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
        }

        let app = Router::new().route("/v1/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::OpenAiChat, format!("http://{addr}")),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32,
                "stream": true
            }),
        )
        .unwrap();

        let mut response =
            forward_upstream_stream(&reqwest::Client::new(), upstream_request, &route())
                .await
                .unwrap();
        let mut body = Vec::new();
        while let Some(chunk) = response.body.next().await {
            body.extend_from_slice(&chunk.unwrap());
        }
        let body = String::from_utf8(body).unwrap();

        assert_eq!(response.status, 200);
        assert!(body.contains("event: message_start"));
        assert!(body.contains("event: content_block_start"));
        assert!(body.contains("event: content_block_delta"));
        assert!(body.contains("event: message_stop"));
        assert!(body.contains(r#""model":"claude-deepseek-v4-pro""#));
        assert!(body.contains(r#""text":"hello""#));
        assert!(body.contains(r#""text":" world""#));
        assert!(!body.contains("chat.completion.chunk"));
        assert!(!body.contains(r#""model":"deepseek-v4-pro""#));
    }

    #[tokio::test]
    async fn forward_openai_stream_converts_tool_call_deltas() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "text/event-stream")],
                "data: {\"id\":\"chatcmpl-tools\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"search_docs\",\"arguments\":\"{\\\"query\\\":\"}}]},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ccds\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
            )
        }

        let app = Router::new().route("/v1/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::OpenAiChat, format!("http://{addr}")),
            &route(),
            serde_json::json!({
                "model": "claude-deepseek-v4-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32,
                "stream": true
            }),
        )
        .unwrap();

        let mut response =
            forward_upstream_stream(&reqwest::Client::new(), upstream_request, &route())
                .await
                .unwrap();
        let mut body = Vec::new();
        while let Some(chunk) = response.body.next().await {
            body.extend_from_slice(&chunk.unwrap());
        }
        let body = String::from_utf8(body).unwrap();

        assert!(body.contains(r#""type":"tool_use""#));
        assert!(body.contains(r#""name":"search_docs""#));
        assert!(body.contains(r#""type":"input_json_delta""#));
        assert!(body.contains(r#""stop_reason":"tool_use""#));
        assert!(!body.contains(r#""model":"deepseek-v4-pro""#));
    }

    #[tokio::test]
    async fn forward_openai_stream_replaces_raw_model_in_success_payload_values() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "text/event-stream")],
                "data: {\"id\":\"chatcmpl_raw-upstream-model\",\"choices\":[{\"delta\":{\"content\":\"answer from raw-upstream-model\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_raw-upstream-model\",\"type\":\"function\",\"function\":{\"name\":\"lookup_raw-upstream-model\",\"arguments\":\"{\\\"query\\\":\\\"raw-upstream-model\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
            )
        }

        let route = disjoint_route();
        let app = Router::new().route("/v1/chat/completions", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::OpenAiChat, format!("http://{addr}")),
            &route,
            serde_json::json!({
                "model": "claude-safe-route",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32,
                "stream": true
            }),
        )
        .unwrap();

        let mut response =
            forward_upstream_stream(&reqwest::Client::new(), upstream_request, &route)
                .await
                .unwrap();
        let mut body = Vec::new();
        while let Some(chunk) = response.body.next().await {
            body.extend_from_slice(&chunk.unwrap());
        }
        let body = String::from_utf8(body).unwrap();

        assert!(!body.contains("raw-upstream-model"));
        assert!(body.contains("claude-safe-route"));
        assert!(body.contains(r#""type":"tool_use""#));
    }

    #[tokio::test]
    async fn forward_stream_rejects_non_event_stream_runtime_response() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "application/json")],
                r#"{"error":"not-stream","model":"raw-upstream-model","token":"sk-stream-secret"}"#,
            )
        }

        let app = Router::new().route("/v1/messages", post(handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let upstream_request = build_messages_upstream_request(
            &provider_with_base_url(ApiFormat::Anthropic, format!("http://{addr}")),
            &disjoint_route(),
            serde_json::json!({
                "model": "claude-safe-route",
                "messages": [{"role": "user", "content": "hello"}],
                "max_tokens": 32,
                "stream": true
            }),
        )
        .unwrap();

        let error = match forward_upstream_stream(
            &reqwest::Client::new(),
            upstream_request,
            &disjoint_route(),
        )
        .await
        {
            Ok(_) => panic!("stream content-type mismatch should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "gateway.invalid_stream_content_type");
        assert!(!error.redacted_preview.contains("sk-stream-secret"));
        assert!(!error.redacted_preview.contains("raw-upstream-model"));
        assert!(error.redacted_preview.contains("claude-safe-route"));
        assert!(error.redacted_preview.contains("[REDACTED:key]"));
    }

    #[test]
    fn invalid_upstream_response_redacts_preview() {
        let error = normalize_upstream_response_error(
            200,
            Some("text/html"),
            r#"<html>login sk-upstream-secret Authorization: Bearer sk-auth-secret</html>"#,
            false,
        );

        assert_eq!(error.code, "gateway.invalid_upstream_response");
        assert_eq!(error.status, 200);
        assert!(!error.redacted_preview.contains("sk-upstream-secret"));
        assert!(!error.redacted_preview.contains("sk-auth-secret"));
        assert!(error.redacted_preview.contains("[REDACTED:key]"));
        let payload = anthropic_error_payload(&error);
        assert_eq!(payload.envelope_type, "error");
        assert_eq!(
            payload.error.error_type,
            "gateway.invalid_upstream_response"
        );
        assert!(payload.error.redacted_preview.contains("[REDACTED:key]"));
    }

    #[test]
    fn route_preview_replaces_raw_model_before_truncation() {
        let body = format!(
            "{}{}",
            "x".repeat(MAX_ERROR_PREVIEW_CHARS - "claude-safe-route".len()),
            "raw-upstream-model"
        );
        let preview = preview_for_route(&body, &disjoint_route());

        assert_eq!(preview.chars().count(), MAX_ERROR_PREVIEW_CHARS);
        assert!(!preview.contains("raw-upstream"));
        assert!(preview.ends_with("claude-safe-route"));
    }

    #[test]
    fn invalid_stream_content_type_gets_stream_fingerprint() {
        let error = normalize_upstream_response_error(
            200,
            Some("application/json"),
            r#"{"error":"not a stream"}"#,
            true,
        );

        assert_eq!(error.code, "gateway.invalid_stream_content_type");
    }
}
