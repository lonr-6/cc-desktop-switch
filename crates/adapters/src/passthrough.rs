//! ResponsesPassthroughAdapter —— 字节级透传 OpenAI Responses API 给上游.
//!
//! 适用范围:`apiFormat == "responses"` 且入站是 `/responses` / `/responses/*`
//! / `/messages` / `/messages/*` 路径的 provider。当前仅自定义第三方 provider
//! (用户在「自定义第三方」preset 卡片显式选 Responses 协议)走此路径。
//! Builtin preset(Kimi / MiMo / DeepSeek / MiniMax / Zhipu / Bailian / Kimi Code
//! / 自定义)全部 `apiFormat=openai_chat`,行为不变。
//!
//! 区别于 `ResponsesAdapter`(本地 Responses↔Chat 协议转换适配器):此 adapter
//! **假设上游原生实现 OpenAI Responses API**,例如:
//! - OpenAI 官方 `https://api.openai.com/v1`
//! - 自建反代实现 Responses API
//! - 任何声明兼容 OpenAI Responses 协议的兼容端点
//!
//! 行为:
//! - **请求**:剥前导 `/v1`(`provider.base_url` 通常已带 `/v1`,不剥则会拼出
//!   `…/v1/v1/responses`)。body 字节级透传 ——`forward.rs` 已在 adapter
//!   之前用 `rewritten_model` 改写 model 字段,passthrough 不再处理。
//! - **响应**:trait 默认 0 转换字节级透传。SSE envelope / `sequence_number` /
//!   `chatcmpl→resp_` ID 等全部由上游产生,代理不重写。这跟 `ResponsesAdapter`
//!   的 `ChatToResponsesConverter` 状态机完全相反 —— 那个是 chat→responses
//!   协议翻译,此处是同协议直传。
//!
//! Session cache 行为:`response_session = None`、`original_responses_request =
//! None`、`is_compact = false`。透传场景上游(例如 OpenAI 服务端)自己管
//! `previous_response_id` session,代理不写 / 不读本地 cache。理由:① 非
//! OpenAI 上游不一定支持 → 写本地 cache 但读不到也无意义;② 跨 provider
//! 切换时 cache 错配反而更危险。

use bytes::Bytes;
use codex_app_transfer_registry::Provider;

use crate::registry::rewrite_local_path_for_upstream;
use crate::types::{Adapter, AdapterError, RequestPlan};

#[derive(Debug, Default, Clone, Copy)]
pub struct ResponsesPassthroughAdapter;

impl ResponsesPassthroughAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for ResponsesPassthroughAdapter {
    fn name(&self) -> &'static str {
        "responses_passthrough"
    }

    fn prepare_request(
        &self,
        client_path: &str,
        body: Bytes,
        _provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        // 用 registry::rewrite_local_path_for_upstream 完整 normalize:
        // 剥 `/openai` legacy prefix + `/claude/v1/messages` alias + `/v1` 前缀 + 保 query。
        // 不能用 `normalize_v1_prefix`(只剥 `/v1`),否则 `/openai/v1/responses` 会
        // 被透传成 `/openai/v1/responses` 拼到 baseUrl → 上游 404。
        Ok(RequestPlan {
            upstream_path: rewrite_local_path_for_upstream(client_path),
            body,
            upstream_headers: http::HeaderMap::new(),
            response_session: None,
            adapter_metadata: None,
            is_compact: false,
            original_responses_request: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn dummy_provider() -> Provider {
        Provider {
            id: "dummy".into(),
            name: "dummy".into(),
            base_url: "https://api.openai.com/v1".into(),
            auth_scheme: "bearer".into(),
            api_format: "responses".into(),
            api_key: "k".into(),
            models: IndexMap::new(),
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn name_is_stable_id() {
        assert_eq!(ResponsesPassthroughAdapter.name(), "responses_passthrough");
    }

    #[test]
    fn strips_v1_from_responses_path() {
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/responses");
    }

    #[test]
    fn preserves_query_string() {
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/v1/responses?stream=true",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/responses?stream=true");
    }

    #[test]
    fn passes_through_responses_subpath() {
        // /responses/{id}/cancel 等子路径上游(OpenAI)有定义,代理原样转
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/v1/responses/resp_abc/cancel",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/responses/resp_abc/cancel");
    }

    #[test]
    fn body_is_byte_level_passthrough() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.5","input":[],"tools":[{"type":"web_search"}],"stream":true}"#,
        );
        let plan = ResponsesPassthroughAdapter
            .prepare_request("/v1/responses", body.clone(), &dummy_provider())
            .unwrap();
        assert_eq!(plan.body, body, "body 必须字节级透传,不改写任何字段");
    }

    #[test]
    fn no_session_cache_no_envelope_replay() {
        // 透传场景上游自己管 session,代理不写 cache;
        // 也不需要 envelope replay(上游已按 Responses API 协议产生 SSE envelope)
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert!(plan.response_session.is_none());
        assert!(plan.original_responses_request.is_none());
        assert!(!plan.is_compact);
    }

    #[test]
    fn handles_path_without_v1_prefix() {
        let plan = ResponsesPassthroughAdapter
            .prepare_request("/responses", Bytes::from_static(b"{}"), &dummy_provider())
            .unwrap();
        assert_eq!(plan.upstream_path, "/responses");
    }

    #[test]
    fn strips_openai_legacy_prefix() {
        // P1 (chatgpt-codex-connector review): /openai/v1/responses 必须 normalize 成
        // /responses 给上游(provider.base_url 自带 /v1)。否则透传成
        // https://api.openai.com/v1/openai/v1/responses → 必 404。
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/openai/v1/responses",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/responses");
    }

    #[test]
    fn rewrites_claude_legacy_alias_to_messages() {
        // P1 (chatgpt-codex-connector review): /claude/v1/messages 是 Codex CLI legacy
        // alias,必须 rewrite 成 /messages 给上游。
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/claude/v1/messages",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/messages");
    }

    #[test]
    fn preserves_query_after_legacy_prefix_strip() {
        let plan = ResponsesPassthroughAdapter
            .prepare_request(
                "/openai/v1/responses?stream=true&foo=bar",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/responses?stream=true&foo=bar");
    }
}
