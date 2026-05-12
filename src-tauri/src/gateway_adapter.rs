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
        return Err(normalize_upstream_response_error(
            status,
            Some(&content_type),
            &body,
            true,
        ));
    }

    if !(200..300).contains(&status) {
        let body = response.text().await.unwrap_or_default();
        return Err(UpstreamError {
            status,
            code: "provider.real_smoke_failed".to_owned(),
            message: "upstream provider returned an error status".to_owned(),
            redacted_preview: preview(&body),
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
        return Err(normalize_upstream_response_error(
            status,
            Some(content_type),
            body,
            false,
        ));
    }

    let parsed = serde_json::from_str::<Value>(body)
        .map_err(|_| normalize_upstream_response_error(status, Some(content_type), body, false))?;
    if !(200..300).contains(&status) {
        return Err(UpstreamError {
            status,
            code: "provider.real_smoke_failed".to_owned(),
            message: "upstream provider returned an error status".to_owned(),
            redacted_preview: preview(body),
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
    openai_buffer: String,
    openai_message_started: bool,
    openai_content_started: bool,
    openai_message_stopped: bool,
}

impl SseStreamNormalizer {
    fn new(api_format: ApiFormat, upstream_model: String, route_id: String) -> Self {
        Self {
            api_format,
            upstream_model,
            route_id,
            openai_buffer: String::new(),
            openai_message_started: false,
            openai_content_started: false,
            openai_message_stopped: false,
        }
    }

    fn normalize(&mut self, chunk: Bytes) -> Bytes {
        match self.api_format {
            ApiFormat::Anthropic => {
                normalize_anthropic_sse_chunk(chunk, &self.upstream_model, &self.route_id)
            }
            ApiFormat::OpenAiChat => self.normalize_openai_chat_sse_chunk(chunk),
        }
    }

    fn normalize_openai_chat_sse_chunk(&mut self, chunk: Bytes) -> Bytes {
        self.openai_buffer
            .push_str(&String::from_utf8_lossy(&chunk));
        let mut output = String::new();

        while let Some(index) = self.openai_buffer.find("\n\n") {
            let frame = self.openai_buffer[..index].to_owned();
            self.openai_buffer = self.openai_buffer[index + 2..].to_owned();
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
            let message_id = value
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("ccds-openai-stream");
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
                if !self.openai_content_started {
                    self.openai_content_started = true;
                    events.push_str(&sse_event(
                        "content_block_start",
                        serde_json::json!({
                            "type": "content_block_start",
                            "index": 0,
                            "content_block": {"type": "text", "text": ""}
                        }),
                    ));
                }
                events.push_str(&sse_event(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {"type": "text_delta", "text": content}
                    }),
                ));
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
        if self.openai_content_started {
            events.push_str(&sse_event(
                "content_block_stop",
                serde_json::json!({
                    "type": "content_block_stop",
                    "index": 0
                }),
            ));
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
}

fn normalize_anthropic_sse_chunk(chunk: Bytes, upstream_model: &str, route_id: &str) -> Bytes {
    let text = String::from_utf8_lossy(&chunk);
    let normalized = text
        .replace(
            &format!(r#""model":"{upstream_model}""#),
            &format!(r#""model":"{route_id}""#),
        )
        .replace(
            &format!(r#""model": "{upstream_model}""#),
            &format!(r#""model": "{route_id}""#),
        );
    Bytes::from(normalized)
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
    let content = choice
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
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

    Ok(serde_json::json!({
        "id": object.get("id").and_then(Value::as_str).unwrap_or("ccds-openai-chat"),
        "type": "message",
        "role": "assistant",
        "model": route.route_id.clone(),
        "content": [{"type": "text", "text": content}],
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": usage.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0),
            "output_tokens": usage.get("completion_tokens").and_then(Value::as_u64).unwrap_or(0)
        }
    }))
}

pub fn normalize_upstream_response_error(
    status: u16,
    content_type: Option<&str>,
    body: &str,
    expected_stream: bool,
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
        redacted_preview: preview(body),
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
        let content = openai_content(message_object.get("content").unwrap_or(&Value::Null));
        messages.push(openai_message(role, &content));
    }

    request.insert("messages".to_owned(), Value::Array(messages));
    copy_if_present(&object, &mut request, "max_tokens");
    copy_if_present(&object, &mut request, "temperature");
    copy_if_present(&object, &mut request, "top_p");
    copy_if_present(&object, &mut request, "stream");

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
    async fn forward_invalid_html_response_returns_redacted_error() {
        async fn handler() -> (
            axum::http::StatusCode,
            [(&'static str, &'static str); 1],
            String,
        ) {
            (
                axum::http::StatusCode::OK,
                [("content-type", "text/html")],
                "<html>sk-upstream-secret</html>".to_owned(),
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
                "max_tokens": 32
            }),
        )
        .unwrap();

        let error = forward_upstream_request(&reqwest::Client::new(), upstream_request, &route())
            .await
            .unwrap_err();

        assert_eq!(error.code, "gateway.invalid_upstream_response");
        assert!(!error.redacted_preview.contains("sk-upstream-secret"));
        assert!(error.redacted_preview.contains("[REDACTED:key]"));
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
    async fn forward_stream_rejects_non_event_stream_runtime_response() {
        async fn handler() -> (StatusCode, [(&'static str, &'static str); 1], &'static str) {
            (
                StatusCode::OK,
                [("content-type", "application/json")],
                r#"{"error":"not-stream","token":"sk-stream-secret"}"#,
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

        let error = match forward_upstream_stream(
            &reqwest::Client::new(),
            upstream_request,
            &route(),
        )
        .await
        {
            Ok(_) => panic!("stream content-type mismatch should fail"),
            Err(error) => error,
        };

        assert_eq!(error.code, "gateway.invalid_stream_content_type");
        assert!(!error.redacted_preview.contains("sk-stream-secret"));
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
