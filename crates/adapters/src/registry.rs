//! AdapterRegistry —— 按 `provider.api_format` 字符串查找 adapter 实例.
//!
//! 当前内置:
//! - `openai_chat` → `OpenAiChatAdapter`
//! - `responses` → `ResponsesAdapter`
//! - `anthropic_messages` → `AnthropicMessagesAdapter`
//! - `gemini_native` / `gemini_cli_oauth` / `grok_web` 等 provider-specific adapter

use std::sync::Arc;

use crate::anthropic_messages::AnthropicMessagesAdapter;
use crate::core::routes;
use crate::gemini_cli::GeminiCliAdapter;
use crate::gemini_native::GeminiNativeAdapter;
use crate::grok_web::GrokWebAdapter;
use crate::openai_chat::OpenAiChatAdapter;
use crate::passthrough::ResponsesPassthroughAdapter;
use crate::responses::ResponsesAdapter;
use crate::types::Adapter;

#[derive(Clone)]
pub struct AdapterRegistry {
    openai_chat: Arc<dyn Adapter>,
    responses: Arc<dyn Adapter>,
    responses_passthrough: Arc<dyn Adapter>,
    anthropic_messages: Arc<dyn Adapter>,
    gemini_native: Arc<dyn Adapter>,
    gemini_cli: Arc<dyn Adapter>,
    grok_web: Arc<dyn Adapter>,
}

impl AdapterRegistry {
    pub fn with_builtins() -> Self {
        Self {
            openai_chat: Arc::new(OpenAiChatAdapter),
            responses: Arc::new(ResponsesAdapter),
            responses_passthrough: Arc::new(ResponsesPassthroughAdapter),
            anthropic_messages: Arc::new(AnthropicMessagesAdapter),
            gemini_native: Arc::new(GeminiNativeAdapter),
            gemini_cli: Arc::new(GeminiCliAdapter),
            grok_web: Arc::new(GrokWebAdapter),
        }
    }

    /// 按 `apiFormat` 字符串(已小写化)查 adapter。
    ///
    /// - `openai` / `openai_chat` / `chat_completions` → `openai_chat` adapter
    /// - `responses` / `openai_responses` → `responses` adapter(协议转换层)
    /// - `anthropic_messages` / `anthropic` / `claude` / `messages` /
    ///   `claude_messages` → `anthropic_messages` adapter
    /// - **空 / 未知值 fallback 到 `openai_chat`**(跟 `Provider::api_format`
    ///   schema serde default 一致):本项目核心是 chat ↔ responses 协议转换
    ///   器,默认走 chat completions 转发更安全;客户端发 `/responses` 路径时
    ///   `lookup_for_request` 仍会自动选 ResponsesAdapter 做转换。
    ///
    /// 注:Python 早期 backend 把空值 fallback 到 `responses`,造成 v1.x 的
    /// 配置升级 bug(2026-05-08 实测 MiMo 直连上游 404)— 本方法已纠正。
    pub fn lookup(&self, api_format: &str) -> Arc<dyn Adapter> {
        let normalized = api_format.trim().to_ascii_lowercase().replace('-', "_");
        match normalized.as_str() {
            "openai" | "openai_chat" | "chat_completions" => self.openai_chat.clone(),
            "responses" | "openai_responses" => self.responses.clone(),
            "anthropic_messages" | "anthropic" | "claude" | "messages" | "claude_messages" => {
                self.anthropic_messages.clone()
            }
            "gemini_native" | "google_ai_studio" | "gemini" => self.gemini_native.clone(),
            "gemini_cli_oauth" | "gemini_cli" | "google_oauth_cloud_code" => {
                self.gemini_cli.clone()
            }
            // Antigravity OAuth — 跟 gemini-cli 共用上游 wire (cloudcode-pa/v1internal:*),
            // adapter 层完全等价(outer envelope + SSE unwrap),只 OAuth 身份 / UA 不同。
            // 复用 GeminiCliAdapter 即可;forward.rs / project_id 来源由 auth_scheme 区分
            "antigravity_oauth" | "antigravity" | "google_oauth_antigravity" => {
                self.gemini_cli.clone()
            }
            // grok.com Web 后端反代(SuperGrok / X Premium+ cookie 鉴权)。
            // 协议事实(详见 `crates/adapters/src/grok_web/mod.rs` + `types.rs` doc comments):
            // - Endpoint: POST grok.com/rest/app-chat/conversations/new
            // - Connector 走 server-side state + 黑名单(disabledConnectorIds: [])
            // - MCP 通过 call_connected_tool wrapper,namespace 用 `___` 三下划线
            "grok_web" | "grok" | "grok_com" => self.grok_web.clone(),
            // 空字符串 / 未知值 → 跟 schema default 一致,fallback openai_chat
            _ => self.openai_chat.clone(),
        }
    }

    /// Selects the adapter for a local Codex request.
    ///
    /// The provider's `apiFormat` describes the upstream protocol, while Codex
    /// still enters this proxy through local Responses routes. v1.x handled
    /// `/responses` locally first, then converted to the provider protocol. Keep
    /// that routing rule here so OpenAI Chat providers do not receive
    /// `/responses` directly.
    pub fn lookup_for_request(&self, api_format: &str, client_path: &str) -> Arc<dyn Adapter> {
        let normalized = api_format.trim().to_ascii_lowercase().replace('-', "_");
        // 用户显式选 responses 透传 + 入站是 /responses 路径 → 字节级透传给上游。
        // 上游需原生实现 OpenAI Responses API(如 OpenAI 官方 / 自建反代);
        // anthropic/claude/messages 是 Python 历史兼容值,现在走
        // AnthropicMessagesAdapter 做 Responses ↔ Anthropic Messages 转换,
        // 不在 responses passthrough 分支。
        //
        // **关键例外**:`/responses/compact` 是本仓库私有扩展(不是 OpenAI 官方
        // 端点),OpenAI 上游不实现 → 必须走 ResponsesAdapter 在本地包装成 chat
        // completions 模拟实现。passthrough 这条路径会让上游必 404。
        if matches!(normalized.as_str(), "responses" | "openai_responses")
            && is_local_responses_route(client_path)
            && !is_responses_compact_subpath(client_path)
        {
            return self.responses_passthrough.clone();
        }
        if matches!(
            normalized.as_str(),
            "openai" | "openai_chat" | "chat_completions"
        ) && is_local_responses_route(client_path)
        {
            return self.responses.clone();
        }
        self.lookup(api_format)
    }
}

/// 给 passthrough adapter 用:把 client_path(可能含 query)normalize 成上游
/// 标准 path,处理:
/// - `/openai/v1/responses` → `/responses`(剥 legacy `/openai` prefix)
/// - `/claude/v1/messages` → `/messages`(legacy alias)
/// - `/v1/responses` → `/responses`(剥 `/v1`,因 provider.base_url 已带 `/v1`)
/// - 保留 query string
pub fn rewrite_local_path_for_upstream(client_path: &str) -> String {
    routes::rewrite_local_path_for_upstream(client_path)
}

/// 是否是 `/responses/compact*` 子路径(本仓库私有扩展,OpenAI 上游不实现)。
/// passthrough adapter 必须排除这条路径,留给 ResponsesAdapter 在本地包装实现。
pub fn is_responses_compact_subpath(client_path: &str) -> bool {
    routes::is_responses_compact_subpath(client_path)
}

pub fn is_local_responses_route(client_path: &str) -> bool {
    // `/responses` / `/messages` 是 OpenAI Codex CLI 本地入站路径;
    // `/responses/compact` 等 `/responses/*` 子路径(以及 `/messages/*`)
    // 是 OpenAI Responses API 的私有扩展,第三方 provider 都不实现,**必须**
    // 走 ResponsesAdapter 在本地处理而不是透传到 openai_chat 上游(否则
    // OpenaiChatAdapter 会把 `/responses/compact` 直接转给 chat-completions
    // 上游 base_url,触发 404)。
    routes::is_local_responses_route(client_path)
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_openai_chat_aliases() {
        let r = AdapterRegistry::with_builtins();
        for v in ["openai", "openai_chat", "Chat-Completions", "OPENAI_CHAT"] {
            assert_eq!(
                r.lookup(v).name(),
                "openai_chat",
                "alias {v} 应解析到 openai_chat"
            );
        }
    }

    #[test]
    fn lookup_antigravity_aliases_route_to_gemini_cli_adapter() {
        // 3 个 antigravity 别名(全部接受,大小写无关)— 必须全部解析到 gemini_cli
        // adapter 复用同一个 wire(cloudcode-pa),但运行时按 auth_scheme 区分 token
        // 文件 + UA(forward.rs)。漏一个别名会让用户手填的 provider config 跑错
        // adapter,典型现象 = 401 / project_id 串号(2026-05-11 加,锚定 alias 集合)
        let r = AdapterRegistry::with_builtins();
        for v in [
            "antigravity_oauth",
            "antigravity",
            "google_oauth_antigravity",
            "Antigravity-OAuth",
            "ANTIGRAVITY",
        ] {
            assert_eq!(
                r.lookup(v).name(),
                "gemini_cli_oauth",
                "antigravity 别名 {v} 必须复用 gemini_cli adapter wire"
            );
        }
    }

    #[test]
    fn lookup_responses_aliases() {
        let r = AdapterRegistry::with_builtins();
        for v in ["responses", "openai_responses", "Openai-Responses"] {
            assert_eq!(r.lookup(v).name(), "responses", "{v} 应解析到 responses");
        }
    }

    #[test]
    fn lookup_anthropic_messages_aliases() {
        let r = AdapterRegistry::with_builtins();
        for v in [
            "anthropic_messages",
            "anthropic",
            "claude",
            "messages",
            "claude_messages",
            "Anthropic-Messages",
            "CLAUDE",
        ] {
            assert_eq!(
                r.lookup(v).name(),
                "anthropic_messages",
                "{v} 应解析到 anthropic_messages"
            );
        }
    }

    #[test]
    fn lookup_grok_web_aliases() {
        // grok_web 接 grok.com Web 后端反代(SuperGrok cookie),协议事实
        // 见 docs/grok/04-protocol-final.md。aliases 全集小,但 lookup 必须 stable —
        // 漏一个会让用户手填 apiFormat="grok"(无下划线)的 provider 错路到
        // openai_chat → 直连 grok.com 必 401(没注入 cookie/statsig)。
        let r = AdapterRegistry::with_builtins();
        for v in ["grok_web", "grok", "grok_com", "Grok-Web", "GROK_WEB"] {
            assert_eq!(
                r.lookup(v).name(),
                "grok_web",
                "alias {v} 应解析到 grok_web"
            );
        }
    }

    #[test]
    fn lookup_empty_or_unknown_falls_back_to_openai_chat() {
        // 关键回归(2026-05-08):空 / 未知值 fallback 到 openai_chat,跟
        // Provider::api_format schema serde default 一致。早期 v1.x backend
        // 把空值 fallback 到 responses,造成 Kimi/MiMo 配置升级时绕过代理 →
        // 直连上游 → 404。修法见 docs/refactor/admin-handlers.md。
        let r = AdapterRegistry::with_builtins();
        assert_eq!(r.lookup("").name(), "openai_chat");
        assert_eq!(r.lookup("unknown_format").name(), "openai_chat");
        // 显式 responses 仍走 responses adapter;Anthropic 历史别名走新 adapter。
        assert_eq!(r.lookup("responses").name(), "responses");
        assert_eq!(r.lookup("anthropic").name(), "anthropic_messages");
    }

    #[test]
    fn openai_chat_local_responses_routes_use_responses_adapter() {
        let r = AdapterRegistry::with_builtins();
        for path in [
            "/responses",
            "/responses?stream=1",
            "/v1/responses",
            "/openai/v1/responses",
            "/v1/messages",
            "/claude/v1/messages",
        ] {
            assert_eq!(
                r.lookup_for_request("openai_chat", path).name(),
                "responses",
                "{path} must be treated as a local Codex Responses route"
            );
        }
        assert_eq!(
            r.lookup_for_request("openai_chat", "/v1/chat/completions")
                .name(),
            "openai_chat"
        );
    }

    #[test]
    fn responses_compact_subpath_routes_to_responses_adapter() {
        // 关键回归 (2026-05-07):/responses/compact 必须命中 ResponsesAdapter,
        // 让它在本地实现 compact 端点,而不是被 OpenaiChatAdapter 直接透传到
        // 上游 chat-completions provider(那一定 404,因为这是 OpenAI 私有
        // 扩展,第三方都没实现)。
        let r = AdapterRegistry::with_builtins();
        for path in [
            "/responses/compact",
            "/responses/compact?foo=1",
            "/v1/responses/compact",
            "/openai/v1/responses/compact",
        ] {
            assert_eq!(
                r.lookup_for_request("openai_chat", path).name(),
                "responses",
                "{path} 必须走 ResponsesAdapter 本地处理(不能透传成 OpenaiChat)"
            );
        }
    }

    #[test]
    fn responses_routes_match_does_not_trigger_on_unrelated_prefixes() {
        // 防回归:`/responses_alt`、`/responsesfake` 不应被误判成 local 路由
        let r = AdapterRegistry::with_builtins();
        for path in ["/responses_alt", "/v1/responsesfake", "/v1/messagessuffix"] {
            assert_eq!(
                r.lookup_for_request("openai_chat", path).name(),
                "openai_chat",
                "{path} 不应误判为 Codex 本地 Responses/Messages 路由"
            );
        }
    }

    #[test]
    fn responses_passthrough_active_for_responses_format_on_local_routes() {
        // apiFormat=responses + 入站 /responses 路径 → ResponsesPassthroughAdapter
        // (字节级透传给上游 OpenAI Responses API,不做协议转换)
        let r = AdapterRegistry::with_builtins();
        for path in [
            "/responses",
            "/responses?stream=1",
            "/v1/responses",
            "/openai/v1/responses",
            "/v1/responses/resp_abc/cancel",
            "/v1/messages",
        ] {
            assert_eq!(
                r.lookup_for_request("responses", path).name(),
                "responses_passthrough",
                "{path} apiFormat=responses 必须走 passthrough adapter"
            );
        }
        // openai_responses 别名同样命中
        assert_eq!(
            r.lookup_for_request("openai_responses", "/v1/responses")
                .name(),
            "responses_passthrough"
        );
        // 大小写 / 连字符变体
        assert_eq!(
            r.lookup_for_request("Openai-Responses", "/v1/responses")
                .name(),
            "responses_passthrough"
        );
    }

    #[test]
    fn anthropic_aliases_use_anthropic_messages_adapter_not_passthrough() {
        // anthropic/claude/messages 是 Python 历史兼容值 → 走
        // AnthropicMessagesAdapter 本地协议转换,不进 responses passthrough 分支。
        let r = AdapterRegistry::with_builtins();
        for api_format in ["anthropic", "claude", "messages", "claude_messages"] {
            assert_eq!(
                r.lookup_for_request(api_format, "/v1/responses").name(),
                "anthropic_messages",
                "{api_format} 必须走 AnthropicMessagesAdapter 本地转换,不走 passthrough"
            );
        }
    }

    #[test]
    fn responses_format_with_chat_path_falls_back_to_lookup() {
        // 入站非 /responses 路径(理论上 Codex.app 不会发,但防御性)
        // → fallback 到 lookup,apiFormat=responses 仍归 ResponsesAdapter
        let r = AdapterRegistry::with_builtins();
        assert_eq!(
            r.lookup_for_request("responses", "/v1/chat/completions")
                .name(),
            "responses"
        );
    }

    #[test]
    fn responses_compact_subpath_never_uses_passthrough_even_for_responses_format() {
        // P1 (chatgpt-codex-connector review): /responses/compact 是本仓库私有扩展
        // (不是 OpenAI 官方端点),OpenAI 上游不实现 → 必须走 ResponsesAdapter 本地
        // 包装成 chat completions 模拟实现。即便 apiFormat=responses 也不能进 passthrough
        // (passthrough 上去必 404)。
        let r = AdapterRegistry::with_builtins();
        for path in [
            "/responses/compact",
            "/responses/compact?foo=1",
            "/v1/responses/compact",
            "/openai/v1/responses/compact",
        ] {
            assert_eq!(
                r.lookup_for_request("responses", path).name(),
                "responses",
                "{path} 即使 apiFormat=responses 也必须走 ResponsesAdapter,不能 passthrough"
            );
            assert_eq!(
                r.lookup_for_request("openai_responses", path).name(),
                "responses",
                "{path} openai_responses 别名也必须走 ResponsesAdapter"
            );
        }
        // 防回归:`/responses/compact_alt` 不属于 compact 私有扩展,仍走 passthrough
        // (语义上是普通子路径,passthrough 让上游决定是否实现)
        assert_eq!(
            r.lookup_for_request("responses", "/v1/responses/compact_alt")
                .name(),
            "responses_passthrough"
        );
    }

    #[test]
    fn rewrite_local_path_for_upstream_strips_legacy_prefixes() {
        // P1 (chatgpt-codex-connector review): passthrough adapter 必须 normalize
        // 所有 legacy prefix(/openai / /claude/v1/messages),否则透传到上游必 404。
        assert_eq!(
            rewrite_local_path_for_upstream("/openai/v1/responses"),
            "/responses"
        );
        assert_eq!(
            rewrite_local_path_for_upstream("/claude/v1/messages"),
            "/messages"
        );
        assert_eq!(
            rewrite_local_path_for_upstream("/v1/responses?stream=true"),
            "/responses?stream=true"
        );
        assert_eq!(
            rewrite_local_path_for_upstream("/openai/v1/responses?model=gpt-5"),
            "/responses?model=gpt-5"
        );
        assert_eq!(rewrite_local_path_for_upstream("/responses"), "/responses");
        assert_eq!(rewrite_local_path_for_upstream("/v1"), "/");
    }

    #[test]
    fn is_responses_compact_subpath_matches_only_compact_extension() {
        assert!(is_responses_compact_subpath("/responses/compact"));
        assert!(is_responses_compact_subpath("/v1/responses/compact"));
        assert!(is_responses_compact_subpath("/openai/v1/responses/compact"));
        assert!(is_responses_compact_subpath("/v1/responses/compact?foo=1"));
        assert!(is_responses_compact_subpath("/responses/compact/sub"));
        // 防回归:其他 /responses/* 子路径不算 compact
        assert!(!is_responses_compact_subpath("/responses"));
        assert!(!is_responses_compact_subpath("/v1/responses"));
        assert!(!is_responses_compact_subpath("/v1/responses/resp_abc"));
        assert!(!is_responses_compact_subpath("/responses/compact_alt"));
    }
}
