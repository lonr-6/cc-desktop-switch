//! Adapter trait 与协议数据结构.

use std::pin::Pin;

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use futures_core::Stream;
use http::{HeaderMap, StatusCode};
use serde_json::Value;
use thiserror::Error;

/// Adapter 内部 / 上下游间使用的字节流.
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static>>;

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("body decode: {0}")]
    BodyDecode(#[from] serde_json::Error),
    #[error("internal: {0}")]
    Internal(String),
    /// `previous_response_id` 在 session cache 未命中,且当前 input 不足以
    /// 独立构成完整对话(messages 为空)。对齐 OpenAI Responses API 服务端
    /// 真实行为(LM Studio bug tracker #1188、Microsoft semantic-kernel #13128
    /// 等多源验证):返回 HTTP 400 + `code: "previous_response_not_found"`。
    ///
    /// proxy 层 IntoResponse 会把它转成 SDK 兼容的标准 OpenAI 错误体,Codex
    /// CLI 看到 400 立即 fail-fast(不会进 5xx / transport timeout 重试路径),
    /// 把延迟从分钟级降到秒级。
    #[error("previous_response_not_found: {previous_response_id}")]
    PreviousResponseNotFound { previous_response_id: String },
}

/// 单次请求经过 adapter 后,proxy 拿到的"出站计划".
#[derive(Debug, Clone)]
pub struct RequestPlan {
    /// 相对 `provider.base_url` 的上游路径(以 `/` 开头),
    /// 例如 `/chat/completions`。proxy 会用 `{base}{this}` 拼最终 URL。
    pub upstream_path: String,
    /// 改写后的请求体。openai_chat 路径下与入参等同。
    pub body: Bytes,
    /// Adapter-provided outbound headers that belong to the upstream protocol.
    /// proxy applies these after provider `extra_headers`, without overriding
    /// user-configured provider headers.
    pub upstream_headers: HeaderMap,
    /// Responses adapter uses this to save the Chat messages that produced the
    /// outbound response, so future `previous_response_id` requests can restore
    /// history.
    pub response_session: Option<ResponseSessionPlan>,
    /// Adapter-specific request state that response mappers may need later.
    /// This stays internal to the adapter/proxy boundary and must not be exposed
    /// as part of the user-facing protocol.
    pub adapter_metadata: Option<Value>,
    /// 是否是 `/responses/compact` 请求(由 Responses adapter 转换成 chat
    /// completions 模拟 compact 端点)。`transform_response_stream` 据此选择
    /// 直接 SSE 转换还是包装成 `{"output":[{"type":"compaction",...}]}` 响应。
    pub is_compact: bool,
    /// 原入站 Responses API request 的**完整 body**(未经任何展平 / 协议转换)。
    /// chat→responses SSE 状态机用它构造合规 envelope:`response.created` /
    /// `response.in_progress` / `response.completed` 三个生命周期事件的
    /// `response` 对象需要回灌 `tools` / `parallel_tool_calls` / `tool_choice`
    /// / `reasoning` / `text` / `metadata` / `previous_response_id` /
    /// `instructions` / `temperature` / `top_p` / `max_output_tokens` /
    /// `truncation` 等十多个字段(借鉴 mimo2codex `streamToSse.ts:75-105`
    /// `buildResponseSnapshot`)。
    ///
    /// 关键作用是 `tools` 字段:Codex CLI 客户端用 envelope.tools + `(namespace,
    /// function.name)` 复合主键路由模型生成的 function_call 到正确的 MCP server。
    /// 缺这个字段时,namespace 包装的 MCP 工具调用会 "unsupported call"。
    ///
    /// openai_chat 路径不涉及此字段(入站本来就是 chat 格式,无 namespace 包装),留 None。
    pub original_responses_request: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct ResponseSessionPlan {
    pub response_id: String,
    pub messages: Vec<Value>,
}

/// adapter 处理完上游响应后,交给 proxy 的"回灌计划".
pub struct ResponsePlan {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub stream: ByteStream,
}

/// 适配器接口。**所有方法须保持无内部状态**(状态走方法参数),
/// 让单个 adapter 实例可在多个并发请求间安全共享。
pub trait Adapter: Send + Sync {
    /// 唯一标识,用于日志与 registry 匹配,与 `apiFormat` 字符串对齐。
    fn name(&self) -> &'static str;

    /// 接收入站请求(已剥掉 gateway auth),输出上游路径与改写后的 body。
    ///
    /// `client_path` 是入站请求的路径(可能含 query),例如
    /// `/v1/chat/completions?stream=true`。
    fn prepare_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError>;

    /// 接收上游响应的状态 + 头 + 流,产出回灌给客户端的状态 + 头 + 流。
    ///
    /// 默认实现是 0 转换的透传(用于 openai_chat)。Stage 3.2 起的
    /// SSE 状态机适配器会重写这一方法。
    fn transform_response_stream(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        _provider: &Provider,
        _request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        Ok(ResponsePlan {
            status: upstream_status,
            headers: upstream_headers,
            stream: upstream_stream,
        })
    }
}
