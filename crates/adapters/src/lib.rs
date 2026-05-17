//! Codex App Transfer · Provider 协议适配层(Stage 3).
//!
//! 设计目标:
//! - 让 `crates/proxy` 在转发前/后,把入站协议与上游 provider 协议互转
//! - 每种 `apiFormat`(`openai_chat` / `responses` / 未来更多)对应一个
//!   `Adapter` 实现,通过 `AdapterRegistry::lookup` 按 provider 配置选用
//! - **本轮(Stage 3.1)**只交付 `OpenAiChatAdapter`(覆盖现有 5 家用户
//!   provider 的 100%),Responses API ↔ Chat 互转留 Stage 3.2/3.3
//!
//! 流式语义:`transform_response_stream` 接收上游字节流,返回客户端字节流。
//! 对于 passthrough 适配器(本轮的 openai_chat),返回值就是入参,实现
//! 为 0 复制 / 0 缓冲。Stage 3.2 起的 SSE 状态机适配器会重写这条流。

pub mod anthropic_messages;
pub(crate) mod core;
pub mod gemini_cli;
pub mod gemini_native;
pub mod grok_web;
pub(crate) mod mapper;
pub mod openai_chat;
pub mod passthrough;
pub mod registry;
pub mod responses;
pub mod types;

pub use anthropic_messages::AnthropicMessagesAdapter;
pub use gemini_cli::GeminiCliAdapter;
pub use gemini_native::GeminiNativeAdapter;
pub use grok_web::GrokWebAdapter;
pub use openai_chat::OpenAiChatAdapter;
pub use passthrough::ResponsesPassthroughAdapter;
pub use registry::AdapterRegistry;
pub use responses::{
    convert_chat_to_responses_stream, responses_body_to_chat_body,
    responses_body_to_chat_body_for_provider, ChatToResponsesConverter, ResponsesAdapter,
};
pub use types::{Adapter, AdapterError, ByteStream, RequestPlan, ResponsePlan};

/// 把"丢弃某个未知 Responses 工具 type"的告警在整个进程内**每个 type 只 warn 一次**。
///
/// Codex CLI 多轮对话每轮都会重发完整 tools 列表,普通 `tracing::warn!` 会让相同
/// 警告每轮触发一次,30 分钟攒几十行重复 warn,真有问题时埋没在噪音里。借鉴
/// `7as0nch/mimo2codex` `reqToChat.ts:158-172` 的 `warnOnce` 思路:全局
/// `HashSet` 记录已 warn 过的 type,后续静默 drop。
///
/// 进程重启后重置(也就是想要的行为 — 重启可能跟版本升级 / 新 Codex CLI 行为
/// 相关,值得再看一次)。
pub fn warn_once_drop_tool(tool_type: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut guard) = seen.lock() else {
        // poisoned mutex 在生产 unlikely;直接 warn 不重 dedup,避免 panic
        tracing::warn!(tool_type = %tool_type, "dropping unsupported responses tool type");
        return;
    };
    if guard.insert(tool_type.to_owned()) {
        tracing::warn!(tool_type = %tool_type, "dropping unsupported responses tool type");
    }
}

/// 本进程已自动禁用 web_search 的 provider id 集合 — 4xx fallback 路径调用
/// `disable_web_search_for(provider_id)` 加入,`convert_web_search_tool`
/// 调用 `is_web_search_disabled_for` 命中即 drop。
///
/// **设计语义**(对齐用户决策"A+B 双层"):
/// - **A**:Provider 配置 `request_options.web_search_enabled` 默认 false,
///   只有用户显式标 true 才会发 web_search 工具上去
/// - **B**:上游真的拒了(MiMo plugin 没开 / token plan 套餐不支持 / 其他)
///   后,proxy 自动加入此 cache,本进程后续 turn 立即 drop。下次启动
///   cache 重置(用户去 UI 关 web_search_enabled = false 才是持久关闭)。
///
/// **本提交不做**:① transparent retry without web_search(用户视角第一次
/// 请求失败,需要重新提问下一个 turn 才 work);② 写回 config.json 持久化
/// (应用重启后用户配置仍是 enabled=true,需要再失败一次)。这两项留 follow-up。
fn web_search_disabled_set() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn disable_web_search_for(provider_id: &str) {
    if let Ok(mut guard) = web_search_disabled_set().lock() {
        if guard.insert(provider_id.to_owned()) {
            tracing::warn!(
                provider_id = %provider_id,
                "auto-disabling web_search after upstream rejection (likely Web Search Plugin not activated upstream)"
            );
        }
    }
}

pub fn is_web_search_disabled_for(provider_id: &str) -> bool {
    web_search_disabled_set()
        .lock()
        .map(|s| s.contains(provider_id))
        .unwrap_or(false)
}

/// 把入站 `/v1/foo?bar` 规范化为 `/foo?bar`;若开头不是 `/v1/` 则原样返回。
///
/// 用于把 Codex CLI 入站 Responses/Chat 路径的 `/v1` 前缀剥离 —
/// `provider.base_url` 通常已带 `/v1`(如 `https://api.openai.com/v1`),不剥
/// 则会拼出 `…/v1/v1/...`(Stage 3.1 实测 OpenAI 兼容上游 404 / 405)。
///
/// `OpenAiChatAdapter` 与 `ResponsesPassthroughAdapter` 共用此规则。
pub(crate) fn normalize_v1_prefix(path: &str) -> String {
    let path = if path.is_empty() { "/" } else { path };
    if let Some(stripped) = path.strip_prefix("/v1/") {
        format!("/{stripped}")
    } else if path == "/v1" {
        "/".to_owned()
    } else {
        path.to_owned()
    }
}

#[cfg(test)]
mod normalize_v1_prefix_tests {
    use super::normalize_v1_prefix;

    #[test]
    fn strips_v1_chat() {
        assert_eq!(
            normalize_v1_prefix("/v1/chat/completions"),
            "/chat/completions"
        );
    }

    #[test]
    fn strips_v1_responses() {
        assert_eq!(normalize_v1_prefix("/v1/responses"), "/responses");
    }

    #[test]
    fn preserves_query() {
        assert_eq!(
            normalize_v1_prefix("/v1/responses?stream=true"),
            "/responses?stream=true"
        );
    }

    #[test]
    fn passthrough_when_no_v1() {
        assert_eq!(normalize_v1_prefix("/responses"), "/responses");
    }

    #[test]
    fn lone_v1_becomes_root() {
        assert_eq!(normalize_v1_prefix("/v1"), "/");
    }

    #[test]
    fn empty_becomes_root() {
        assert_eq!(normalize_v1_prefix(""), "/");
    }
}
