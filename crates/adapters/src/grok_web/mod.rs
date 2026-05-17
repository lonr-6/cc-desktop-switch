//! `apiFormat=grok_web` adapter —— 反代 grok.com Web 后端到 Codex APP。
//!
//! ## 协议形态
//!
//! - **Endpoint**:`POST https://grok.com/rest/app-chat/conversations/new`
//! - **请求**:JSON body(24 字段,见 [`types::GrokChatRequest`])
//! - **响应**:newline-delimited JSON SSE(每行一个 `{"result":{...}}` 帧)
//! - **鉴权**:`Cookie: sso=<JWT>; sso-rw=<JWT>; cf_clearance=<token>` + `x-statsig-id` + `x-xai-request-id`
//!
//! ## Connector 机制(协议事实)
//!
//! grok.com 的 connector(含 Bring Your Own MCP)走 **server-side state 黑名单**:
//!
//! - 用户在 `grok.com/connectors` UI 注册 + toggle on connector → 持久化到账户
//! - chat 请求 **不传** `connectorIds` / `connectors` / `toolOverrides`
//! - 只传 `disabledConnectorIds: []`(黑名单,默认空 = 启用全部)
//! - grok.com 后端读账户 state → 自动注入 enabled connector 的 tools 到模型上下文
//! - 模型 emit 的 MCP 调用:`messageTag=tool_usage_card` + `toolUsageCard.mcp.{toolName, toolArgsJson}`
//!   其中 `toolName` 形如 `<connector_displayName>___<tool_name>`(**三**下划线分隔)
//!
//! 本仓库 [`crates/adapters/src/responses/converter.rs`] 已展平 MCP namespace
//! 包装为顶级 function tools,本 adapter **不再做 connector 字段透传**,
//! 完全依赖 grok.com server-side state。
//!
//! ## 架构(Phase 4 规范)
//!
//! - `mod.rs`(本文件):`GrokWebAdapter` 薄编排层,impl `Adapter` trait
//! - `types.rs`:Grok wire types(request payload + SSE 帧)
//! - `auth.rs`:cookie / statsig header 注入
//! - `parent_response.rs`:`parentResponseId` DAG 多轮锚定 in-memory tracker
//! - `request.rs`:Codex Responses → Grok payload(被 `mapper/grok_web` 调)
//! - `response.rs`:Grok SSE → Codex Responses SSE 转换状态机
//!
//! 真正的请求 / 响应转换实现在 [`crate::mapper::grok_web::GrokWebMapper`],
//! 本 adapter 仅做 trait 接线。

use crate::mapper::{RequestMapper, ResponseMapper};
use crate::types::{Adapter, AdapterError, ByteStream, RequestPlan, ResponsePlan};
use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{HeaderMap, StatusCode};

pub mod auth;
pub mod parent_response;
pub mod request;
pub mod response;
pub mod types;

#[derive(Debug, Default, Clone, Copy)]
pub struct GrokWebAdapter;

impl GrokWebAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for GrokWebAdapter {
    fn name(&self) -> &'static str {
        "grok_web"
    }

    fn prepare_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        crate::mapper::grok_web::GrokWebMapper.map_request(client_path, body, provider)
    }

    fn transform_response_stream(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        crate::mapper::grok_web::GrokWebMapper.map_response(
            upstream_status,
            upstream_headers,
            upstream_stream,
            provider,
            request_plan,
        )
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn adapter_name_is_grok_web() {
        assert_eq!(GrokWebAdapter::new().name(), "grok_web");
    }
}
