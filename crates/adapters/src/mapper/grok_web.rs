//! Grok Web `RequestMapper` + `ResponseMapper` 实现。
//!
//! 按 [`docs/protocol-unification-rfc-phase4.md`] 的 Phase 4 规范,本文件是 grok_web
//! adapter 的**核心逻辑落地点**(adapter 自身仅 thin wrapper)。
//!
//! - [`prepare_grok_web_request`]:Codex Responses body → grok payload + 鉴权头
//! - [`transform_grok_web_response_stream`]:grok SSE → Codex Responses SSE

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, StatusCode};
use serde_json::Value;

use crate::grok_web::{
    request::{
        responses_body_to_grok_request_with_session, serialize_grok_request, GROK_CHAT_PATH,
    },
    response::{convert_grok_error_to_responses_failure_stream, convert_grok_sse_to_responses_sse},
};
use crate::mapper::{RequestMapper, ResponseMapper};
use crate::responses::global_response_session_cache;
use crate::types::{AdapterError, ByteStream, RequestPlan, ResponsePlan};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct GrokWebMapper;

impl RequestMapper for GrokWebMapper {
    fn map_request(
        &self,
        _client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        prepare_grok_web_request(body, provider)
    }
}

impl ResponseMapper for GrokWebMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        transform_grok_web_response_stream(
            upstream_status,
            upstream_headers,
            upstream_stream,
            provider,
            request_plan,
        )
    }
}

/// grok_web 请求侧:Codex Responses body → grok chat payload + headers。
///
/// **多轮上下文 + autocompact**(2026-05-12 task 18,对齐 ARCHITECTURE_PROTOCOL_GUIDE):
/// 接 `global_response_session_cache()`(L1 LRU + L2 SQLite 持久化
/// `~/.codex-app-transfer/sessions.db`,30 天 TTL),走 `core::input` 共性
/// 历史拼接 + `responses/compact.rs` 三种 compaction variant 自动展开。
/// 双保险:client 端历史 flatten 进 grok message + grok 服务端 DAG 用
/// `parent_response_id`(`ParentResponseTracker` 命中时传)。
///
/// `/responses/compact` 端点 grok.com 后端不暴露(R3 PoC 注释),目前 mapper
/// 也是 fallback 当普通 chat 请求处理,后续可加 compact 短路。
pub(crate) fn prepare_grok_web_request(
    body: Bytes,
    provider: &Provider,
) -> Result<RequestPlan, AdapterError> {
    let parsed: Value = serde_json::from_slice(&body)?;
    let conversion = responses_body_to_grok_request_with_session(
        &parsed,
        provider,
        global_response_session_cache(),
    )?;
    let grok_body = serialize_grok_request(&conversion.request)?;

    Ok(RequestPlan {
        upstream_path: GROK_CHAT_PATH.to_owned(),
        body: grok_body,
        upstream_headers: http::HeaderMap::new(),
        response_session: Some(conversion.response_session),
        adapter_metadata: None,
        is_compact: false,
        original_responses_request: Some(parsed),
    })
}

/// grok_web 响应侧:grok newline-delimited JSON SSE → Codex Responses SSE。
///
/// **错误处理**(review-feedback A1):上游 4xx/5xx 时,**不直接透传 raw grok JSON
/// 但伪装 SSE Content-Type**(那会让 Codex APP 卡 "Thinking" — gemini_native
/// 已踩过同一个坑,见 `gemini_native/response.rs:1474`)。改成合规 Responses 失败流
/// `response.created` + `response.failed`,classify status code 给结构化 error.code,
/// 内附 grok body 摘录(cap 防 DoS)。
///
/// **返回 status 永远 200**(因为 body 是合规 SSE event stream,客户端按 SSE 解析),
/// 真正的 error 信息走 `response.failed` event.error.{code,message} 字段。
pub(crate) fn transform_grok_web_response_stream(
    upstream_status: StatusCode,
    _upstream_headers: HeaderMap,
    upstream_stream: ByteStream,
    _provider: &Provider,
    request_plan: &RequestPlan,
) -> Result<ResponsePlan, AdapterError> {
    // **task 18 code-reviewer C2 修**:SSE response_id 必须 **复用 RequestPlan
    // 里 response_session.response_id**(由 `responses_body_to_grok_request_with_session`
    // 一次性铸造)。否则:
    //   - response_session.response_id 作 cache key 写盘
    //   - SSE event 用另一个 `resp_grok_<uuid>` 给客户端
    //   - 下轮客户端拿 SSE id 查 cache → key mismatch → 永远 miss
    // 没 response_session(eg fallback test fixture)时,fall back 到 `resp_grok_<uuid>`。
    let response_id = request_plan
        .response_session
        .as_ref()
        .map(|s| s.response_id.clone())
        .unwrap_or_else(|| format!("resp_grok_{}", crate::grok_web::auth::generate_uuid_v4()));

    if !upstream_status.is_success() {
        // 翻译成合规 Responses failure SSE 流(review-feedback A1):
        // grok body 由 `convert_grok_error_to_responses_failure_stream` cap+UTF-8 lossy 处理,
        // 输出永远是 `response.created` + `response.failed` 两个事件。
        let downstream = convert_grok_error_to_responses_failure_stream(
            upstream_status,
            upstream_stream,
            response_id,
        );
        return Ok(ResponsePlan {
            status: StatusCode::OK,
            headers: build_sse_headers(),
            stream: downstream,
        });
    }

    // **C1 修**:把 RequestPlan.response_session clone 进 ConvState,流末 save
    // 累积的 assistant text 到 global cache,下轮 previous_response_id 可命中。
    let response_session = request_plan.response_session.clone();
    let downstream =
        convert_grok_sse_to_responses_sse(upstream_stream, response_id, response_session);
    Ok(ResponsePlan {
        status: StatusCode::OK,
        headers: build_sse_headers(),
        stream: downstream,
    })
}

fn build_sse_headers() -> HeaderMap {
    let mut h = HeaderMap::with_capacity(2);
    h.insert(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    h.insert("cache-control", HeaderValue::from_static("no-store"));
    h
}

// 注:鉴权头注入(cookie / statsig / xai-request-id)**不**经过 mapper 层 wrapper,
// `forward.rs` 直接调用 `crate::grok_web::auth::apply_grok_headers`。
// 这是 grok_web 与其他 adapter(走 inject_auth 的 Bearer/x-api-key)的差异点,
// 因为 grok.com 需要一组复合 headers(7~10 个),用单一 fn 接口最清晰。

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_transfer_registry::Provider;
    use indexmap::IndexMap;
    use serde_json::json;

    fn make_provider() -> Provider {
        let mut models = IndexMap::new();
        models.insert("default".into(), "grok-420-computer-use-sa".into());
        let mut extra = IndexMap::new();
        extra.insert(
            "grokWeb".into(),
            json!({
                "cookies": {
                    "sso": "j1",
                    "sso-rw": "j2",
                    "cf_clearance": "c"
                },
                "statsigId": "stat-id"
            }),
        );
        Provider {
            id: "grok-web".into(),
            name: "Grok Web".into(),
            base_url: "https://grok.com".into(),
            auth_scheme: "grok_cookie".into(),
            api_format: "grok_web".into(),
            api_key: String::new(),
            models,
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra,
        }
    }

    #[test]
    fn prepare_request_emits_grok_chat_path() {
        let body = Bytes::from(
            serde_json::to_vec(&json!({
                "model": "default",
                "input": [{"type": "message", "role": "user", "content": "hi"}]
            }))
            .unwrap(),
        );
        let plan = prepare_grok_web_request(body, &make_provider()).unwrap();
        assert_eq!(plan.upstream_path, GROK_CHAT_PATH);
        assert!(plan.original_responses_request.is_some());
        // payload 必须含 disabledConnectorIds 黑名单,无 connectorIds 白名单
        let payload: Value = serde_json::from_slice(&plan.body).unwrap();
        assert_eq!(payload["disabledConnectorIds"], json!([]));
        assert!(!payload.as_object().unwrap().contains_key("connectorIds"));
    }
}
