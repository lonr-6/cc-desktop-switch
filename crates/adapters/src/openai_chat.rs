//! OpenAI Chat Completions 适配器.
//!
//! 适用范围:任何 `apiFormat == "openai_chat"` 的 provider。本仓库当前 5 家
//! 用户配置 + 7 家内置预设全部走这条路径(详见
//! `backend/config.py::BUILTIN_PRESETS`)。
//!
//! 行为:
//! - **请求**:把入站 `/v1/chat/completions` 等 OpenAI 风格路径剥掉前导
//!   `/v1`,得到相对 provider.base_url 的上游路径。body 不动。
//! - **响应**:passthrough(继承 trait 默认实现)。
//!
//! Bug 修复(Stage 3.1):此前 proxy 直接把 `/v1/chat/completions` 拼到
//! `baseUrl=https://api.moonshot.cn/v1` 后面会得到 `…/v1/v1/chat/completions`
//! → 真请求 404 / 405。adapter 在这里负责剥前导 `/v1`,base_url 中的 `/v1`
//! 由 provider 配置自带,合起来恰好一份。

use bytes::Bytes;
use codex_app_transfer_registry::Provider;

use crate::normalize_v1_prefix;
use crate::types::{Adapter, AdapterError, RequestPlan};

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAiChatAdapter;

impl OpenAiChatAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for OpenAiChatAdapter {
    fn name(&self) -> &'static str {
        "openai_chat"
    }

    fn prepare_request(
        &self,
        client_path: &str,
        body: Bytes,
        _provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        Ok(RequestPlan {
            upstream_path: normalize_v1_prefix(client_path),
            body,
            upstream_headers: http::HeaderMap::new(),
            response_session: None,
            adapter_metadata: None,
            is_compact: false,
            // openai_chat 路径直接 passthrough,入站本来就是 chat 格式,
            // 无 namespace 包装也无 Responses API envelope 字段需求,留 None。
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
            base_url: "https://up.example/v1".into(),
            auth_scheme: "bearer".into(),
            api_format: "openai_chat".into(),
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
        assert_eq!(OpenAiChatAdapter.name(), "openai_chat");
    }

    #[test]
    fn strips_leading_v1_from_chat_path() {
        let a = OpenAiChatAdapter;
        let plan = a
            .prepare_request(
                "/v1/chat/completions",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/chat/completions");
    }

    #[test]
    fn preserves_query_string() {
        let a = OpenAiChatAdapter;
        let plan = a
            .prepare_request(
                "/v1/chat/completions?stream=true",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/chat/completions?stream=true");
    }

    #[test]
    fn passes_through_path_without_v1_prefix() {
        let a = OpenAiChatAdapter;
        let plan = a
            .prepare_request(
                "/chat/completions",
                Bytes::from_static(b"{}"),
                &dummy_provider(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/chat/completions");
    }

    #[test]
    fn lone_v1_becomes_root() {
        let a = OpenAiChatAdapter;
        let plan = a
            .prepare_request("/v1", Bytes::from_static(b""), &dummy_provider())
            .unwrap();
        assert_eq!(plan.upstream_path, "/");
    }

    #[test]
    fn body_is_passthrough() {
        let a = OpenAiChatAdapter;
        let body = Bytes::from_static(br#"{"model":"x","stream":true}"#);
        let plan = a
            .prepare_request("/v1/chat/completions", body.clone(), &dummy_provider())
            .unwrap();
        assert_eq!(plan.body, body);
    }
}
