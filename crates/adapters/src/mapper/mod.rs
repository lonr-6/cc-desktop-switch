use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{HeaderMap, StatusCode};

use crate::types::{AdapterError, ByteStream, RequestPlan, ResponsePlan};

pub(crate) mod anthropic_messages;
pub(crate) mod chat;
pub(crate) mod cloud_code;
pub(crate) mod gemini_native;
pub(crate) mod grok_web;

/// Request 侧协议映射器统一接口。
pub(crate) trait RequestMapper {
    fn map_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError>;
}

/// Response 侧协议映射器统一接口。
pub(crate) trait ResponseMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError>;
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use bytes::Bytes;
    use futures_util::stream;
    use http::header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE};
    use indexmap::IndexMap;
    use serde_json::{json, Value};

    fn responses_body_bytes() -> Bytes {
        Bytes::from(
            serde_json::to_vec(&json!({
                "model": "gemini-2.5-pro",
                "stream": true,
                "input": [{"type":"message","role":"user","content":"hi"}]
            }))
            .unwrap(),
        )
    }

    fn make_provider(id: &str, name: &str, base_url: &str, api_format: &str) -> Provider {
        Provider {
            id: id.into(),
            name: name.into(),
            base_url: base_url.into(),
            auth_scheme: "none".into(),
            api_format: api_format.into(),
            api_key: "".into(),
            models: IndexMap::new(),
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    fn assert_common_request_contract(plan: &RequestPlan) {
        assert!(plan.upstream_path.starts_with('/'));
        assert!(!plan.body.is_empty());
        assert!(!plan.is_compact);
        assert!(plan.original_responses_request.is_some());
    }

    #[test]
    fn request_mapper_contracts_hold_for_four_mappers() {
        let responses_provider = make_provider(
            "openai-chat",
            "OpenAI Chat",
            "https://api.openai.com",
            "responses",
        );
        let responses_plan = crate::mapper::chat::ChatResponsesMapper
            .map_request("/v1/responses", responses_body_bytes(), &responses_provider)
            .unwrap();
        assert_common_request_contract(&responses_plan);
        assert!(responses_plan.response_session.is_some());

        let gemini_native_provider = make_provider(
            "google-ai-studio",
            "Google AI Studio",
            "https://generativelanguage.googleapis.com",
            "gemini_native",
        );
        let gemini_native_plan = crate::mapper::gemini_native::GeminiNativeMapper
            .map_request(
                "/v1/responses",
                responses_body_bytes(),
                &gemini_native_provider,
            )
            .unwrap();
        assert_common_request_contract(&gemini_native_plan);
        assert!(gemini_native_plan.response_session.is_some());

        let mut cloud_code_provider = make_provider(
            "gemini-cli",
            "Gemini CLI",
            "https://cloudcode-pa.googleapis.com",
            "gemini_cli_oauth",
        );
        cloud_code_provider.extra.insert(
            "cloud_code_project_id".into(),
            Value::String("test-project-12345".into()),
        );
        let cloud_code_plan = crate::mapper::cloud_code::CloudCodeMapper
            .map_request(
                "/v1/responses",
                responses_body_bytes(),
                &cloud_code_provider,
            )
            .unwrap();
        assert_common_request_contract(&cloud_code_plan);
        // **task 25 修(2026-05-13)**:cloud_code 之前 hardcoded `response_session: None`,
        // 把 prod silent history loss bug 当 invariant 锁定。task 25 修复后
        // cloud_code 也走 session cache(跟 gemini_native 主路径一致),response_session
        // 必填 Some 让流末 converter 写 cache。
        assert!(
            cloud_code_plan.response_session.is_some(),
            "task 25 修后 cloud_code 必须 emit response_session 供 SSE 流末 cache write"
        );

        let anthropic_provider = make_provider(
            "claude",
            "Claude",
            "https://api.anthropic.com/v1",
            "anthropic_messages",
        );
        let anthropic_plan = crate::mapper::anthropic_messages::AnthropicMessagesMapper
            .map_request("/v1/responses", responses_body_bytes(), &anthropic_provider)
            .unwrap();
        assert_common_request_contract(&anthropic_plan);
        assert_eq!(anthropic_plan.upstream_path, "/messages");
        assert!(anthropic_plan.response_session.is_some());
        assert!(anthropic_plan.adapter_metadata.is_some());
        assert_eq!(
            anthropic_plan
                .upstream_headers
                .get("anthropic-version")
                .and_then(|v| v.to_str().ok()),
            Some("2023-06-01")
        );
    }

    #[test]
    fn response_mapper_contracts_set_sse_content_type_for_success_path() {
        let responses_provider = make_provider(
            "openai-chat",
            "OpenAI Chat",
            "https://api.openai.com",
            "responses",
        );
        let responses_plan = crate::mapper::chat::ChatResponsesMapper
            .map_request("/v1/responses", responses_body_bytes(), &responses_provider)
            .unwrap();
        let mut responses_headers = HeaderMap::new();
        responses_headers.insert(CONTENT_LENGTH, "123".parse().unwrap());
        let responses_response = crate::mapper::chat::ChatResponsesMapper
            .map_response(
                StatusCode::OK,
                responses_headers,
                Box::pin(stream::empty()),
                &responses_provider,
                &responses_plan,
            )
            .unwrap();
        assert_eq!(responses_response.status, StatusCode::OK);
        assert_eq!(
            responses_response
                .headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );

        let gemini_native_provider = make_provider(
            "google-ai-studio",
            "Google AI Studio",
            "https://generativelanguage.googleapis.com",
            "gemini_native",
        );
        let gemini_native_plan = crate::mapper::gemini_native::GeminiNativeMapper
            .map_request(
                "/v1/responses",
                responses_body_bytes(),
                &gemini_native_provider,
            )
            .unwrap();
        let mut gemini_native_headers = HeaderMap::new();
        gemini_native_headers.insert(CONTENT_LENGTH, "123".parse().unwrap());
        gemini_native_headers.insert(CONTENT_ENCODING, "gzip".parse().unwrap());
        let gemini_native_response = crate::mapper::gemini_native::GeminiNativeMapper
            .map_response(
                StatusCode::OK,
                gemini_native_headers,
                Box::pin(stream::empty()),
                &gemini_native_provider,
                &gemini_native_plan,
            )
            .unwrap();
        assert_eq!(gemini_native_response.status, StatusCode::OK);
        assert_eq!(
            gemini_native_response
                .headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );

        let mut cloud_code_provider = make_provider(
            "gemini-cli",
            "Gemini CLI",
            "https://cloudcode-pa.googleapis.com",
            "gemini_cli_oauth",
        );
        cloud_code_provider.extra.insert(
            "cloud_code_project_id".into(),
            Value::String("test-project-12345".into()),
        );
        let cloud_code_plan = crate::mapper::cloud_code::CloudCodeMapper
            .map_request(
                "/v1/responses",
                responses_body_bytes(),
                &cloud_code_provider,
            )
            .unwrap();
        let mut cloud_code_headers = HeaderMap::new();
        cloud_code_headers.insert(CONTENT_LENGTH, "123".parse().unwrap());
        cloud_code_headers.insert(CONTENT_ENCODING, "gzip".parse().unwrap());
        let cloud_code_response = crate::mapper::cloud_code::CloudCodeMapper
            .map_response(
                StatusCode::OK,
                cloud_code_headers,
                Box::pin(stream::empty()),
                &cloud_code_provider,
                &cloud_code_plan,
            )
            .unwrap();
        assert_eq!(cloud_code_response.status, StatusCode::OK);
        assert_eq!(
            cloud_code_response
                .headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );

        let anthropic_provider = make_provider(
            "claude",
            "Claude",
            "https://api.anthropic.com/v1",
            "anthropic_messages",
        );
        let anthropic_plan = crate::mapper::anthropic_messages::AnthropicMessagesMapper
            .map_request("/v1/responses", responses_body_bytes(), &anthropic_provider)
            .unwrap();
        let mut anthropic_headers = HeaderMap::new();
        anthropic_headers.insert(CONTENT_LENGTH, "123".parse().unwrap());
        anthropic_headers.insert(CONTENT_ENCODING, "gzip".parse().unwrap());
        let anthropic_response = crate::mapper::anthropic_messages::AnthropicMessagesMapper
            .map_response(
                StatusCode::OK,
                anthropic_headers,
                Box::pin(stream::empty()),
                &anthropic_provider,
                &anthropic_plan,
            )
            .unwrap();
        assert_eq!(anthropic_response.status, StatusCode::OK);
        assert_eq!(
            anthropic_response
                .headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/event-stream")
        );
    }
}
