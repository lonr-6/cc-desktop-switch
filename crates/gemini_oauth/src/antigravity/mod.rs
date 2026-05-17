//! Antigravity OAuth provider — Google 另一个 OAuth-based 客户端,跟 gemini-cli
//! **共用** `cloudcode-pa.googleapis.com/v1internal:*` 上游端点(chat / bootstrap
//! 同样)但用不同 OAuth 身份 + UA + ClientMetadata + 独立 token 文件。
//!
//! 跟父 crate 的 [`super::flow`] / [`super::cloud_code`] 是**并行实现**:架构等价
//! 但常量 / 元数据 / 头部全用 antigravity 系列。这种重复换取**零侵入** gemini-cli
//! 路径(已端到端验证 + 大量单测覆盖,改动风险大)。后续如果出现第 3 个 OAuth
//! provider,再考虑抽公共 trait。
//!
//! ## 跟 gemini-cli 差异(file:line 引自 CLIProxyAPI)
//!
//! | 维度 | gemini-cli | antigravity | 来源 |
//! |---|---|---|---|
//! | OAuth client | `681255809395-...` | `1071006060591-...` | `auth/antigravity/constants.go:6` |
//! | callback port | dynamic | 固定 51121 | `:8` |
//! | scopes | 3 | +2 (`cclog`, `experimentsandconfigs`) | `:12-18` |
//! | auth URL | (default) | 加 `prompt=consent` | `auth.go:61-68` |
//! | UA loadCodeAssist | `GeminiCLI/0.34.0 (...)` | `antigravity/<v> darwin/arm64 google-api-nodejs-client/10.3.0` | `antigravity_version.go:138-151` |
//! | UA chat | 同上 | `antigravity/<v> darwin/arm64`(短形式) | `:132` |
//! | X-Goog-Api-Client | `google-genai-sdk/1.41.0 gl-node/v22.19.0` | `gl-node/22.21.1` | `antigravity_version.go:23` |
//! | ClientMetadata | `{ideType, platform, pluginType, pluginVersion}` | `{ide_type:ANTIGRAVITY, ide_name, ide_version}` | `auth.go:163-169` |
//! | 上游 endpoint | `cloudcode-pa.googleapis.com/v1internal:*` | **同样** | `executor:46-48` |
//! | token 文件 | `~/.codex-app-transfer/gemini-oauth.json` | `~/.codex-app-transfer/antigravity-oauth.json` | (本实现) |

pub mod cloud_code;
pub mod flow;
pub mod models;
pub mod static_models;

pub use cloud_code::{
    bootstrap_project as antigravity_bootstrap_project, AntigravityClientMetadata,
};
pub use flow::{refresh_antigravity_access_token, run_antigravity_oauth_flow_with_cancel};
pub use models::{fetch_antigravity_available_models, AntigravityModelEntry};
pub use static_models::antigravity_static_models;
