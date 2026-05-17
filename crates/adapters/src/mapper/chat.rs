use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{header::HeaderValue, HeaderMap, StatusCode};

use crate::core::routes;
use crate::mapper::{RequestMapper, ResponseMapper};
use crate::responses::{
    compact, convert_chat_to_responses_stream_with_options, global_response_session_cache,
    responses_body_to_chat_body_for_provider_with_session,
};
use crate::types::{AdapterError, ByteStream, RequestPlan, ResponsePlan};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ChatResponsesMapper;

/// 哪些 provider 需要 `<think>...</think>` 兜底拆分。
/// 目前只有 MiniMax 的 OpenAI-compatible 端点在不开启 `reasoning_split` 时
/// 会把思考过程塞进 content 的 `<think>` 标签里,需要兜底解析。
pub(crate) fn provider_needs_think_tag_split(provider: &Provider) -> bool {
    let needles = [&provider.id, &provider.name, &provider.base_url];
    needles.iter().any(|value| {
        let lower = value.to_ascii_lowercase();
        lower.contains("minimax") || lower.contains("minimaxi")
    })
}

/// responses adapter 请求侧编排：
/// - `/responses/compact` 走 compact 本地包装
/// - 其他 `/responses*` 走 responses->chat 主管道转换
pub(crate) fn prepare_responses_request(
    client_path: &str,
    body: Bytes,
    provider: &Provider,
) -> Result<RequestPlan, AdapterError> {
    if compact::is_compact_path(client_path) {
        let new_body = compact::build_compact_chat_request(&body, provider)?;
        return Ok(RequestPlan {
            upstream_path: "/chat/completions".to_owned(),
            body: Bytes::from(new_body),
            upstream_headers: http::HeaderMap::new(),
            response_session: None,
            adapter_metadata: None,
            is_compact: true,
            original_responses_request: None,
        });
    }

    let upstream_path = routes::redirect_responses_to_chat(client_path);
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| AdapterError::BadRequest(format!("body 不是合法 JSON: {e}")))?;
    let original_responses_request = Some(parsed.clone());
    let conversion = responses_body_to_chat_body_for_provider_with_session(
        &parsed,
        Some(provider),
        Some(global_response_session_cache()),
    )?;
    let new_body = serde_json::to_vec(&conversion.body)
        .map_err(|e| AdapterError::Internal(format!("re-serialize: {e}")))?;
    Ok(RequestPlan {
        upstream_path,
        body: Bytes::from(new_body),
        upstream_headers: http::HeaderMap::new(),
        response_session: Some(conversion.response_session),
        adapter_metadata: None,
        is_compact: false,
        original_responses_request,
    })
}

/// responses adapter 响应侧编排：
/// - compact 走 compact response 包装
/// - 其余路径走 chat SSE -> responses SSE 转换
pub(crate) fn transform_responses_response_stream(
    upstream_status: StatusCode,
    mut upstream_headers: HeaderMap,
    upstream_stream: ByteStream,
    provider: &Provider,
    request_plan: &RequestPlan,
) -> Result<ResponsePlan, AdapterError> {
    if request_plan.is_compact {
        return compact::build_compact_response_plan(
            upstream_status,
            upstream_headers,
            upstream_stream,
        );
    }
    upstream_headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    let enable_think_tag_split = provider_needs_think_tag_split(provider);
    Ok(ResponsePlan {
        status: upstream_status,
        headers: upstream_headers,
        stream: convert_chat_to_responses_stream_with_options(
            upstream_stream,
            request_plan.response_session.clone(),
            enable_think_tag_split,
            request_plan.original_responses_request.clone(),
        ),
    })
}

impl RequestMapper for ChatResponsesMapper {
    fn map_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        prepare_responses_request(client_path, body, provider)
    }
}

impl ResponseMapper for ChatResponsesMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        transform_responses_response_stream(
            upstream_status,
            upstream_headers,
            upstream_stream,
            provider,
            request_plan,
        )
    }
}
