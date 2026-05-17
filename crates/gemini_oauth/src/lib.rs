//! Gemini CLI OAuth 2.0 + Cloud Code Assist 集成。
//!
//! 实现 impersonate `google-gemini/gemini-cli` 官方 OAuth 流程,让用户通过浏览器
//! 登录 Google 账号一次,后续所有请求复用本地持久化的 access_token。**不需要**
//! 用户手动配 API key,且对接 `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent`
//! Cloud Code Assist 端点(免费 tier per-account 配额,跟 Gemini API key 路径不同
//! 的 baseUrl + 不同的 outer envelope)。
//!
//! 调研完整 wire-level 决策见 `docs/gemini-cli-oauth-research.md`。
//!
//! ## 模块结构
//!
//! - [`token`] — `OauthToken` 数据模型 + `~/.codex-app-transfer/gemini-oauth.json`
//!   持久化(读 / 写 / 删,含 expiry / project_id)
//! - [`flow`](TODO) — code grant 流程:loopback callback server + browser open +
//!   `oauth2.googleapis.com/token` exchange + persist
//! - [`refresh`](TODO) — token expired 前 60s 自动 refresh
//! - [`cloud_code`](TODO) — `loadCodeAssist` + `onboardUser` LRO bootstrap
//!
//! ## 关键 OAuth 常量(对齐 gemini-cli upstream `oauth2.ts:43-51`)
//!
//! - `client_id` / `client_secret` — Google 设计为公开嵌入的 installed-app 凭证
//! - `auth_endpoint` / `token_endpoint` — 标准 Google OAuth 2.0
//! - `scopes` — `cloud-platform` + `userinfo.email` + `userinfo.profile`
//! - PKCE — gemini-cli web flow **不用**(对齐上游;user-code flow 才用)
//! - redirect_uri — `http://127.0.0.1:<动态port>/oauth2callback`(每次启动随机 port)
//!
//! ## 致谢上游
//!
//! Wire-level 实现参考 [`router-for-me/CLIProxyAPI`](https://github.com/router-for-me/CLIProxyAPI)
//! (Go, MIT) 的 `internal/auth/gemini/` 与 `internal/runtime/executor/gemini_cli_executor.go`。

pub mod antigravity;
pub mod cloud_code;
pub mod constants;
pub mod flow;
pub mod service;
pub mod token;

pub use cloud_code::{bootstrap_project, ClientMetadata, CloudCodeError};
pub use constants::{
    antigravity_user_agent_chat, antigravity_user_agent_loadcodeassist, detect_user_agent,
    OauthProviderConfig, ANTIGRAVITY_PROVIDER, ANTIGRAVITY_USERINFO_ENDPOINT, ANTIGRAVITY_VERSION,
    ANTIGRAVITY_X_GOOG_API_CLIENT, GEMINI_CLI_PROVIDER, X_GOOG_API_CLIENT,
};
pub use flow::{
    build_auth_url, refresh_access_token, run_oauth_flow, run_oauth_flow_with_cancel, FlowError,
    OauthFlowConfig,
};
pub use service::{
    ensure_valid_access_token, ensure_valid_antigravity_token, persist_token, ServiceError,
};
pub use token::{OauthToken, TokenError, TokenStore};

// Antigravity provider re-exports(parallel module,跟 gemini-cli 共用 token / FlowError 等)
pub use antigravity::{
    antigravity_bootstrap_project, antigravity_static_models, fetch_antigravity_available_models,
    refresh_antigravity_access_token, run_antigravity_oauth_flow_with_cancel,
    AntigravityClientMetadata, AntigravityModelEntry,
};
