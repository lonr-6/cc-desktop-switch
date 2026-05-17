use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{header::HeaderValue, HeaderMap, StatusCode};
use serde_json::Value;

use crate::mapper::{RequestMapper, ResponseMapper};
use crate::responses::compact::{
    build_compact_chat_request, build_compact_response_plan, is_compact_path,
};
use crate::responses::global_response_session_cache;
use crate::types::{AdapterError, ByteStream, RequestPlan, ResponsePlan};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct GeminiNativeMapper;

/// gemini_native 请求侧转换：
/// - `/responses/compact`：走 compact 请求包装并转 Gemini 非流式 wire
/// - 普通 `/responses`：走 responses->gemini 转换，并挂接 response_session
pub(crate) fn prepare_gemini_native_request(
    client_path: &str,
    body: Bytes,
    provider: &Provider,
) -> Result<RequestPlan, AdapterError> {
    if is_compact_path(client_path) {
        let compact_chat_body = build_compact_chat_request(&body, provider)?;
        let compact_chat_json: Value = serde_json::from_slice(&compact_chat_body)
            .map_err(|e| AdapterError::Internal(format!("compact chat body decode: {e}")))?;
        let model = compact_chat_json
            .get("model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AdapterError::BadRequest("compact body missing model".into()))?
            .to_owned();
        let gemini_request = crate::gemini_native::request::chat_normalized_to_gemini_request(
            &compact_chat_json,
            provider,
        )?;
        let gemini_body = serde_json::to_vec(&gemini_request).map_err(AdapterError::BodyDecode)?;
        let upstream_path = crate::gemini_native::request::build_gemini_upstream_path(
            &model,
            false,
            &provider.base_url,
        );
        return Ok(RequestPlan {
            upstream_path,
            body: Bytes::from(gemini_body),
            upstream_headers: http::HeaderMap::new(),
            response_session: None,
            adapter_metadata: None,
            is_compact: true,
            original_responses_request: None,
        });
    }

    let parsed: Value = serde_json::from_slice(&body)?;
    let stream = parsed
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let model = parsed
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdapterError::BadRequest("model field required".into()))?
        .to_owned();

    let conversion = crate::gemini_native::request::responses_body_to_gemini_request_with_session(
        &parsed,
        provider,
        Some(global_response_session_cache()),
    )?;
    let gemini_request = conversion.request;
    let gemini_body = serde_json::to_vec(&gemini_request).map_err(AdapterError::BodyDecode)?;
    let upstream_path = crate::gemini_native::request::build_gemini_upstream_path(
        &model,
        stream,
        &provider.base_url,
    );

    Ok(RequestPlan {
        upstream_path,
        body: Bytes::from(gemini_body),
        upstream_headers: http::HeaderMap::new(),
        response_session: Some(conversion.response_session),
        adapter_metadata: None,
        is_compact: false,
        original_responses_request: Some(parsed),
    })
}

/// gemini_native 响应流转换：
/// - compact：复用 compact 响应包装
/// - 非 compact:
///   - 非 2xx：转换为 Responses failure SSE 流
///   - 2xx：Gemini SSE -> Responses SSE
pub(crate) fn transform_gemini_native_response_stream(
    upstream_status: StatusCode,
    mut upstream_headers: HeaderMap,
    upstream_stream: ByteStream,
    request_plan: &RequestPlan,
) -> Result<ResponsePlan, AdapterError> {
    if request_plan.is_compact {
        return build_compact_response_plan(upstream_status, upstream_headers, upstream_stream);
    }

    upstream_headers.remove(http::header::CONTENT_LENGTH);
    upstream_headers.remove(http::header::CONTENT_ENCODING);
    upstream_headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    if !upstream_status.is_success() {
        let stream =
            crate::gemini_native::response::convert_gemini_error_to_responses_failure_stream(
                upstream_status,
                upstream_stream,
                request_plan.original_responses_request.clone(),
            );
        return Ok(ResponsePlan {
            status: StatusCode::OK,
            headers: upstream_headers,
            stream,
        });
    }
    let stream = crate::gemini_native::response::convert_gemini_to_responses_stream(
        upstream_stream,
        request_plan.original_responses_request.clone(),
        request_plan.response_session.clone(),
    );
    Ok(ResponsePlan {
        status: upstream_status,
        headers: upstream_headers,
        stream,
    })
}

impl RequestMapper for GeminiNativeMapper {
    fn map_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        prepare_gemini_native_request(client_path, body, provider)
    }
}

impl ResponseMapper for GeminiNativeMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        _provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        transform_gemini_native_response_stream(
            upstream_status,
            upstream_headers,
            upstream_stream,
            request_plan,
        )
    }
}
