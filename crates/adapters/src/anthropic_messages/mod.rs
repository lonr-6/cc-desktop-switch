//! Anthropic Messages adapter building blocks.
//!
//! P3 landed request-side lowering, P4 landed response-side stream conversion,
//! and P5 exposes the thin adapter that delegates to the mapper layer.

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{HeaderMap, StatusCode};

use crate::mapper::{anthropic_messages::AnthropicMessagesMapper, RequestMapper, ResponseMapper};
use crate::types::{Adapter, AdapterError, ByteStream, RequestPlan, ResponsePlan};

pub mod request;
pub mod response;

#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicMessagesAdapter;

impl AnthropicMessagesAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for AnthropicMessagesAdapter {
    fn name(&self) -> &'static str {
        "anthropic_messages"
    }

    fn prepare_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        AnthropicMessagesMapper.map_request(client_path, body, provider)
    }

    fn transform_response_stream(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        AnthropicMessagesMapper.map_response(
            upstream_status,
            upstream_headers,
            upstream_stream,
            provider,
            request_plan,
        )
    }
}
