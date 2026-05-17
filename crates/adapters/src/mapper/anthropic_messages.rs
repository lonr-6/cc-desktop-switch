use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{HeaderMap, StatusCode};

use crate::anthropic_messages::request::{
    into_request_plan, prepare_anthropic_messages_request, AnthropicToolNameMaps,
};
use crate::anthropic_messages::response::build_anthropic_messages_response_plan;
use crate::mapper::{RequestMapper, ResponseMapper};
use crate::types::{AdapterError, ByteStream, RequestPlan, ResponsePlan};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct AnthropicMessagesMapper;

pub(crate) fn prepare_anthropic_messages_request_plan(
    client_path: &str,
    body: Bytes,
    provider: &Provider,
) -> Result<RequestPlan, AdapterError> {
    prepare_anthropic_messages_request(client_path, body, provider).map(into_request_plan)
}

pub(crate) fn transform_anthropic_messages_response_stream(
    upstream_status: StatusCode,
    upstream_headers: HeaderMap,
    upstream_stream: ByteStream,
    request_plan: &RequestPlan,
) -> Result<ResponsePlan, AdapterError> {
    let tool_name_maps = request_plan
        .adapter_metadata
        .as_ref()
        .and_then(|value| serde_json::from_value::<AnthropicToolNameMaps>(value.clone()).ok())
        .unwrap_or_default();
    build_anthropic_messages_response_plan(
        upstream_status,
        upstream_headers,
        upstream_stream,
        request_plan.response_session.clone(),
        request_plan.original_responses_request.clone(),
        tool_name_maps,
        request_plan.is_compact,
    )
}

impl RequestMapper for AnthropicMessagesMapper {
    fn map_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        prepare_anthropic_messages_request_plan(client_path, body, provider)
    }
}

impl ResponseMapper for AnthropicMessagesMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        _provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        transform_anthropic_messages_response_stream(
            upstream_status,
            upstream_headers,
            upstream_stream,
            request_plan,
        )
    }
}
