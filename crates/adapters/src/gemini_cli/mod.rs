//! Gemini CLI OAuth adapter(`apiFormat=gemini_cli_oauth`)。
//!
//! Codex.app `/responses` → Google Cloud Code Assist `:streamGenerateContent`
//! 直转,**impersonate 官方 gemini-cli**。跟 [`crate::gemini_native`] 的关键
//! 差异:
//!
//! | 维度 | gemini_native(API key) | gemini_cli(OAuth) |
//! |---|---|---|
//! | 上游 | `generativelanguage.googleapis.com/v1{alpha,beta}/models/<m>:streamGenerateContent` | `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse` |
//! | 鉴权 | `?key=<api_key>` query | `Authorization: Bearer <oauth_access_token>` |
//! | body | inner Gemini wire 直发 | outer `{model, project, user_prompt_id, request: <inner>}` 包一层 |
//! | SSE event | `{candidates, ...}` | `{response: {candidates, ...}}` 多包一层 |
//! | 配额 | API key 关联 GCP project 计费 | free-tier per-account,绑 `cloudaicompanionProject` |
//!
//! ## 复用 gemini_native 内部转换
//!
//! 90% inner 转换逻辑(JSON Schema sanitize / web_search 兼容(对齐 cliproxy:
//! transformer 阶段统一 drop googleSearch) / 多轮 function calling round-trip /
//! contents 必须 user 起 / failure stream 等)从
//! [`crate::gemini_native::request::responses_body_to_gemini_request`] 直接 reuse,
//! 这里只做 outer wrap + SSE 外层 unwrap。
//!
//! ## project_id 来源
//!
//! 必须从 `provider.extra.cloud_code_project_id` 字段读 — 由前端 OAuth 流程
//! 完成后写入 provider config。**不在 adapter 里 fetch / refresh** —— OAuth
//! 流程在 `gemini_oauth` crate(用户 UI 触发),token 注入在 forward.rs。
//!
//! ## 致谢上游
//!
//! 借鉴 [`router-for-me/CLIProxyAPI`](https://github.com/router-for-me/CLIProxyAPI)
//! 的 `internal/runtime/executor/gemini_cli_executor.go` 拿 wire 形态。

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use http::{HeaderMap, StatusCode};

use crate::mapper::{RequestMapper, ResponseMapper};
use crate::types::{Adapter, AdapterError, ByteStream, RequestPlan, ResponsePlan};

pub mod response;

#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiCliAdapter;

impl GeminiCliAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl Adapter for GeminiCliAdapter {
    fn name(&self) -> &'static str {
        "gemini_cli_oauth"
    }

    fn prepare_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        crate::mapper::cloud_code::CloudCodeMapper.map_request(client_path, body, provider)
    }

    fn transform_response_stream(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        _provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        crate::mapper::cloud_code::CloudCodeMapper.map_response(
            upstream_status,
            upstream_headers,
            upstream_stream,
            _provider,
            request_plan,
        )
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;
    use indexmap::IndexMap;
    use serde_json::Value;

    fn dummy_provider_with_project() -> Provider {
        let mut extra = IndexMap::new();
        extra.insert(
            "cloud_code_project_id".into(),
            Value::String("test-project-12345".into()),
        );
        Provider {
            id: "gemini-cli".into(),
            name: "Gemini CLI (OAuth)".into(),
            base_url: "https://cloudcode-pa.googleapis.com".into(),
            auth_scheme: "google_oauth_cloud_code".into(),
            api_format: "gemini_cli_oauth".into(),
            api_key: "".into(), // OAuth 路径不用 api_key
            models: IndexMap::new(),
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: true,
            sort_index: 0,
            extra,
        }
    }

    #[test]
    fn name_is_stable_id() {
        assert_eq!(GeminiCliAdapter.name(), "gemini_cli_oauth");
    }

    /// 锚定 antigravity api_format 别名集合 — 必须跟 `crates/proxy/src/resolver.rs`
    /// `AuthScheme::parse` 与 `crates/adapters/src/registry.rs` adapter dispatch
    /// 一致。任一别名漏判会让用户手填的 provider config silently 读错 token 文件
    /// (gemini-oauth.json vs antigravity-oauth.json),刷新时会用错 client_id
    /// 污染对方 token —— 两个 provider 同时 brick(2026-05-11 review #1 修)
    #[test]
    fn cloud_code_api_flavor_recognizes_all_aliases() {
        // 全部 antigravity 别名(大小写无关)
        for v in [
            "antigravity_oauth",
            "antigravity",
            "google_oauth_antigravity",
            "Antigravity-OAuth", // dash 不识别(parse 在 registry/resolver 层做)
            "ANTIGRAVITY",
            "Antigravity",
        ] {
            // dash 形式不接受 —— 这里 lowercase 后是 "antigravity-oauth" 不在白名单
            // 这是有意:adapter 层只接受 underscore + 全 alias,跟 registry lookup
            // 入口的 normalize 行为对齐(registry.lookup 也 fail dash)
            let normalized = v.to_ascii_lowercase();
            let expected = matches!(
                normalized.as_str(),
                "antigravity_oauth" | "antigravity" | "google_oauth_antigravity"
            );
            assert_eq!(
                crate::mapper::cloud_code::CloudCodeApiFlavor::from_api_format(v).is_antigravity(),
                expected,
                "alias {v} 识别错"
            );
        }
        // 非 antigravity 必须返 false
        for v in [
            "gemini_cli_oauth",
            "gemini_cli",
            "google_oauth_cloud_code",
            "openai_chat",
            "",
            "antigravity_other",
        ] {
            assert!(
                !crate::mapper::cloud_code::CloudCodeApiFlavor::from_api_format(v).is_antigravity(),
                "{v} 不应判成 antigravity"
            );
        }
    }

    /// **strip helper 防御性回归**:`strip_include_server_side_tool_invocations`
    /// 必须 idempotent 地剥两种 case(camelCase / snake_case),并在 `toolConfig`
    /// 因此变空时把外层 key 一并去掉。即使 transformer 阶段(2026-05-11 对齐
    /// cliproxy 后)不再主动注入,此 helper 仍要 hold 住未来 extra-body 透传 /
    /// transformer 回归 等再注入场景。
    #[test]
    fn strip_include_server_side_tool_invocations_handles_both_casings_and_empties_toolconfig() {
        // camelCase 单独存在 → 剥 + toolConfig 变空被一并删除
        let mut obj = serde_json::json!({
            "toolConfig": {"includeServerSideToolInvocations": true}
        })
        .as_object()
        .unwrap()
        .clone();
        crate::mapper::cloud_code::strip_include_server_side_tool_invocations(&mut obj);
        assert!(
            !obj.contains_key("toolConfig"),
            "toolConfig 变空后必须整段移除,实际:{obj:?}"
        );

        // snake_case 单独存在 → 同上
        let mut obj = serde_json::json!({
            "toolConfig": {"include_server_side_tool_invocations": true}
        })
        .as_object()
        .unwrap()
        .clone();
        crate::mapper::cloud_code::strip_include_server_side_tool_invocations(&mut obj);
        assert!(!obj.contains_key("toolConfig"));

        // 两种 casing 同时存在 → 都剥
        let mut obj = serde_json::json!({
            "toolConfig": {
                "includeServerSideToolInvocations": true,
                "include_server_side_tool_invocations": true
            }
        })
        .as_object()
        .unwrap()
        .clone();
        crate::mapper::cloud_code::strip_include_server_side_tool_invocations(&mut obj);
        assert!(!obj.contains_key("toolConfig"));

        // toolConfig 含其它合法字段时仅剥目标字段,toolConfig 保留
        let mut obj = serde_json::json!({
            "toolConfig": {
                "includeServerSideToolInvocations": true,
                "functionCallingConfig": {"mode": "AUTO"}
            }
        })
        .as_object()
        .unwrap()
        .clone();
        crate::mapper::cloud_code::strip_include_server_side_tool_invocations(&mut obj);
        let tc = obj
            .get("toolConfig")
            .and_then(|v| v.as_object())
            .expect("toolConfig 含其它字段时必须保留");
        assert!(!tc.contains_key("includeServerSideToolInvocations"));
        assert!(tc.contains_key("functionCallingConfig"));

        // 无 toolConfig 时是 no-op
        let mut obj = serde_json::json!({"contents": []})
            .as_object()
            .unwrap()
            .clone();
        crate::mapper::cloud_code::strip_include_server_side_tool_invocations(&mut obj);
        assert!(obj.contains_key("contents"));
        assert_eq!(obj.len(), 1);
    }

    /// **cloud-code wire 兼容性**(2026-05-11 对齐 cliproxy):Gemini 3 + Codex tools
    /// 同 turn 出现 googleSearch + functionDeclarations 时,inner transformer
    /// (`chat_normalized_to_gemini_request`)统一 drop `googleSearch`(对齐 cliproxy
    /// 主项目"不实现 web_search"策略,避免上游 400 + 模型语义偏移)。
    /// 同时验证防御性 strip:`toolConfig.includeServerSideToolInvocations`
    /// 在 cloudcode-pa proto 不被识别(实测 2026-05-11 返 400 `Unknown name`),
    /// 即使未来 transformer 误注入也必须被剥。
    #[test]
    fn cloud_code_drops_google_search_and_strips_include_server_side_tool_invocations() {
        let body = serde_json::json!({
            "model": "gemini-3-pro-preview",
            "stream": true,
            "input": [{"type":"message","role":"user","content":"x"}],
            "tools": [
                {"type":"function","name":"exec_command","parameters":{"type":"object"}},
                {"type":"web_search"}
            ]
        });
        let plan = GeminiCliAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from(serde_json::to_vec(&body).unwrap()),
                &dummy_provider_with_project(),
            )
            .unwrap();
        let outer: Value = serde_json::from_slice(&plan.body).unwrap();
        let inner = outer.get("request").unwrap();
        let tc_field = inner
            .get("toolConfig")
            .and_then(|v| v.get("includeServerSideToolInvocations"));
        assert!(
            tc_field.is_none(),
            "includeServerSideToolInvocations 必须不存在(cloudcode-pa 不识别;transformer 也不再注入)"
        );
        let tools = inner.get("tools").and_then(|v| v.as_array()).unwrap();
        let has_gs = tools.iter().any(|t| t.get("googleSearch").is_some());
        let has_fd = tools
            .iter()
            .any(|t| t.get("functionDeclarations").is_some());
        assert!(
            !has_gs,
            "对齐 cliproxy:googleSearch 必须在 transformer 阶段被 drop,实际 tools={tools:?}"
        );
        assert!(has_fd, "functionDeclarations 必须保留(Codex 核心)");
    }

    #[test]
    fn prepare_request_outputs_outer_envelope_with_project() {
        let body = serde_json::json!({
            "model": "gemini-2.5-pro",
            "stream": true,
            "instructions": "sys",
            "input": [{"type":"message","role":"user","content":"hi"}]
        });
        let plan = GeminiCliAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from(serde_json::to_vec(&body).unwrap()),
                &dummy_provider_with_project(),
            )
            .unwrap();
        // upstream path: cloud-code internal
        assert_eq!(
            plan.upstream_path,
            "/v1internal:streamGenerateContent?alt=sse"
        );
        // body 必须有 outer envelope
        let parsed: Value = serde_json::from_slice(&plan.body).unwrap();
        assert_eq!(parsed["model"], "gemini-2.5-pro");
        assert_eq!(parsed["project"], "test-project-12345");
        assert!(parsed["user_prompt_id"].is_string());
        // request 内层应该是 Gemini wire(contents / systemInstruction)
        assert!(parsed["request"]["contents"].is_array());
        assert!(parsed["request"]["systemInstruction"].is_object());
    }

    #[test]
    fn prepare_request_non_stream_uses_generate_content() {
        let body = serde_json::json!({
            "model": "gemini-2.5-flash",
            "input": [{"type":"message","role":"user","content":"x"}]
        });
        let plan = GeminiCliAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from(serde_json::to_vec(&body).unwrap()),
                &dummy_provider_with_project(),
            )
            .unwrap();
        assert_eq!(plan.upstream_path, "/v1internal:generateContent");
    }

    #[test]
    fn missing_project_id_returns_bad_request_with_hint() {
        // 隔离 HOME 让 token store fallback 走 None 而不是命中真实磁盘
        // ~/.codex-app-transfer/gemini-oauth.json — 否则 dev 机跑 test 会因为
        // 真有 token 而把"missing project_id"路径覆盖掉。每个 test fn override
        // HOME 即可,不影响 cargo test 并发(env::set_var 进程级,但其他 test
        // 不依赖 HOME path 默认)。
        // 安全:仅 cfg(test) 路径,不进 prod
        let _guard = HomeGuard::set(tempfile::tempdir().unwrap().path());
        let mut p = dummy_provider_with_project();
        p.extra.shift_remove("cloud_code_project_id");
        let body = serde_json::json!({
            "model": "gemini-2.5-pro",
            "input": [{"type":"message","role":"user","content":"x"}]
        });
        let err = GeminiCliAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from(serde_json::to_vec(&body).unwrap()),
                &p,
            )
            .unwrap_err();
        match err {
            AdapterError::BadRequest(msg) => {
                assert!(
                    msg.contains("cloud_code_project_id"),
                    "错误必须 hint 用户跑 OAuth login,实际:{msg}"
                );
                assert!(msg.contains("OAuth login"));
            }
            other => panic!("期待 BadRequest,得到 {other:?}"),
        }
    }

    /// scoped HOME override —— Drop 时还原原值,防 test 间泄漏。
    struct HomeGuard {
        prev: Option<std::ffi::OsString>,
    }
    impl HomeGuard {
        fn set(new_home: &std::path::Path) -> Self {
            let prev = std::env::var_os("HOME");
            // SAFETY: cfg(test) 路径,test 内手动隔离 HOME 验 token-store fallback
            unsafe {
                std::env::set_var("HOME", new_home);
            }
            Self { prev }
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            // SAFETY: 同 set,Drop 时还原避免 leak
            unsafe {
                match self.prev.take() {
                    Some(v) => std::env::set_var("HOME", v),
                    None => std::env::remove_var("HOME"),
                }
            }
        }
    }

    #[test]
    fn missing_model_returns_bad_request() {
        let body = serde_json::json!({
            "input": [{"type":"message","role":"user","content":"x"}]
        });
        let err = GeminiCliAdapter
            .prepare_request(
                "/v1/responses",
                Bytes::from(serde_json::to_vec(&body).unwrap()),
                &dummy_provider_with_project(),
            )
            .unwrap_err();
        assert!(matches!(err, AdapterError::BadRequest(_)));
    }
}
