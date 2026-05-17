//! gemini-cli OAuth 流程的硬编码常量。
//!
//! 这些值**故意公开** —— Google 设计 installed-app OAuth 凭证为客户端嵌入,见
//! [Installed app flow](https://developers.google.com/identity/protocols/oauth2/native-app)。
//! 跟 gemini-cli 官方 (`packages/core/src/code_assist/oauth2.ts:43-51`) 保持一致。

/// gemini-cli 客户端 ID(installed-app 类型)。
pub const CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";

/// gemini-cli 客户端 secret(installed-app 设计为公开)。
pub const CLIENT_SECRET: &str = "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";

/// Google OAuth 2.0 授权端点(用户浏览器跳转目标)。
pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Google OAuth 2.0 token 端点(code → access_token + refresh_token)。
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// Cloud Code Assist 内部 API base URL —— OAuth 路径专用,**跟 API key 路径**
/// (`generativelanguage.googleapis.com`)不同。
pub const CLOUD_CODE_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";

/// OAuth scope(空格分隔)。三个 scope 缺一不可:
/// - `cloud-platform`:Cloud Code Assist API 调用权限
/// - `userinfo.email`:展示用户当前登录邮箱
/// - `userinfo.profile`:展示用户名(诊断用)
pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
];

/// 出站 User-Agent —— impersonate gemini-cli。Google 上游会按这个字段做客户端
/// 识别,跟 `X-Goog-Api-Client` 一起出现在所有 cloudcode-pa 请求里。值跟
/// CLIProxyAPI `header_utils.go::DetectUserAgent` 一致(format
/// `GeminiCLI/0.34.0 (<platform>; <arch>; terminal)`,platform/arch 跟
/// `process.platform` / `process.arch` 一致 — `darwin`/`linux`/`win32`,
/// `arm64`/`x64`/`ia32`)。
///
/// **不能 hardcode**:Linux 用户上传 `darwin; arm64` UA 会让 Google 上游 telemetry
/// 把 Linux 流量当 macOS 统计 + 部分 quota / abuse 检测可能 trip。
pub fn detect_user_agent() -> String {
    let platform = match std::env::consts::OS {
        "macos" => "darwin",
        "linux" => "linux",
        "windows" => "win32",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    };
    format!("GeminiCLI/0.34.0 ({platform}; {arch}; terminal)")
}

/// 兼容老调用方 —— 跟 `detect_user_agent()` 同一身份格式;preset extraHeaders
/// 不能放运行时值,需要静态字符串时用此 const(macOS Apple Silicon 字面)。
/// **新代码请用 `detect_user_agent()`**。
#[deprecated(note = "use detect_user_agent() — preset extraHeaders 走 forward.rs runtime 注入")]
pub const USER_AGENT: &str = "GeminiCLI/0.34.0 (darwin; arm64; terminal)";

/// 出站 X-Goog-Api-Client header —— Google 内部 telemetry,缺这个字段
/// cloudcode-pa 端点会按"非官方客户端"分支响应。值字面对齐 CLIProxyAPI。
pub const X_GOOG_API_CLIENT: &str = "google-genai-sdk/1.41.0 gl-node/v22.19.0";

/// loopback redirect URI 路径 —— 每次启动随机 port,完整 URI 在 flow 模块
/// 动态构造:`http://127.0.0.1:<port>/oauth2callback`。
pub const REDIRECT_PATH: &str = "/oauth2callback";

/// Token expired 前多少秒自动触发 refresh —— 60s buffer 防 race(请求到上游时
/// token 刚好过期)。
pub const REFRESH_BUFFER_SECS: i64 = 60;

// ── Antigravity provider 常量 ────────────────────────────────────────
//
// Antigravity 是 Google 另一个 OAuth-based 客户端,跟 gemini-cli **共用**
// `cloudcode-pa.googleapis.com/v1internal:*` 上游端点(chat / bootstrap 同样),
// 但使用不同的 OAuth 身份 + UA + ClientMetadata。CLIProxyAPI antigravity 实现
// 见 `internal/auth/antigravity/{auth.go, constants.go}` + `internal/misc/
// antigravity_version.go`。

/// Antigravity 客户端 ID(installed-app 类型)。CLIProxyAPI `auth/antigravity/
/// constants.go:6`。
pub const ANTIGRAVITY_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";

/// Antigravity 客户端 secret。CLIProxyAPI `:7`。
pub const ANTIGRAVITY_CLIENT_SECRET: &str = "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";

/// Antigravity 固定 callback port(不像 gemini-cli 用动态 port)。CLIProxyAPI `:8`。
pub const ANTIGRAVITY_CALLBACK_PORT: u16 = 51121;

/// Antigravity OAuth scopes —— 比 gemini-cli 多 2 个(`cclog` + `experimentsandconfigs`)。
/// CLIProxyAPI `:12-18`。
pub const ANTIGRAVITY_SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

/// Antigravity 出站 X-Goog-Api-Client header。CLIProxyAPI `antigravity_version.go:23`。
pub const ANTIGRAVITY_X_GOOG_API_CLIENT: &str = "gl-node/22.21.1";

/// Antigravity 版本号(hardcode fallback,跟 CLIProxyAPI `antigravity_version.go:19`
/// `antigravityFallbackVersion` 一致)。CLIProxyAPI 有 background goroutine 拉
/// `https://antigravity-auto-updater-974169037036.us-central1.run.app/releases`
/// 拿最新版,我们暂时只用 fallback。
///
/// **2026-05-11**:1.21.9 已被 Google upstream 拒(返 "This version of Antigravity
/// is no longer supported")。auto-updater 当前 stable = 1.23.2,bump 跟上。
/// followup PR 应实现跟 CLIProxyAPI 一致的 6h-cached HTTP poll updater
pub const ANTIGRAVITY_VERSION: &str = "1.23.2";

/// Antigravity chat / generate request UA(短形式,无 nodejs-client 后缀)。
/// CLIProxyAPI `antigravity_version.go:132`。
pub fn antigravity_user_agent_chat() -> String {
    format!("antigravity/{ANTIGRAVITY_VERSION} darwin/arm64")
}

/// Antigravity loadCodeAssist / onboardUser control-plane UA(长形式,带
/// nodejs-client 后缀)。CLIProxyAPI `antigravity_version.go:138-151`。
pub fn antigravity_user_agent_loadcodeassist() -> String {
    format!("antigravity/{ANTIGRAVITY_VERSION} darwin/arm64 google-api-nodejs-client/10.3.0")
}

/// Antigravity 用户信息端点 — 用 v2 (gemini-cli 用 v3 openidconnect),
/// CLIProxyAPI `auth/antigravity/constants.go:24`。
pub const ANTIGRAVITY_USERINFO_ENDPOINT: &str =
    "https://www.googleapis.com/oauth2/v2/userinfo?alt=json";

/// OAuth provider 通用配置 — gemini-cli 和 antigravity 共用一套 OAuth flow
/// 实现,差异通过此结构注入。
///
/// **callback_port = None** 表示动态选 port(gemini-cli 风格);Some(N) 强制
/// 用固定 port(antigravity 风格,N=51121)。
/// **prompt_consent = true** 在 auth URL 加 `prompt=consent` 强制每次重授权
/// (antigravity 风格)。
#[derive(Debug, Clone, Copy)]
pub struct OauthProviderConfig {
    pub provider_id: &'static str,
    pub client_id: &'static str,
    pub client_secret: &'static str,
    pub scopes: &'static [&'static str],
    pub callback_port: Option<u16>,
    pub prompt_consent: bool,
    /// Token 持久化文件名(`~/.codex-app-transfer/<token_filename>`),不同
    /// provider 必须不同 token 文件,防覆盖。
    pub token_filename: &'static str,
    pub x_goog_api_client: &'static str,
}

/// gemini-cli provider 配置(等价于现有 hardcoded 常量,用于新代码统一接口)。
pub const GEMINI_CLI_PROVIDER: OauthProviderConfig = OauthProviderConfig {
    provider_id: "gemini_cli",
    client_id: CLIENT_ID,
    client_secret: CLIENT_SECRET,
    scopes: SCOPES,
    callback_port: None,
    prompt_consent: false,
    token_filename: "gemini-oauth.json",
    x_goog_api_client: X_GOOG_API_CLIENT,
};

/// Antigravity provider 配置。
pub const ANTIGRAVITY_PROVIDER: OauthProviderConfig = OauthProviderConfig {
    provider_id: "antigravity",
    client_id: ANTIGRAVITY_CLIENT_ID,
    client_secret: ANTIGRAVITY_CLIENT_SECRET,
    scopes: ANTIGRAVITY_SCOPES,
    callback_port: Some(ANTIGRAVITY_CALLBACK_PORT),
    prompt_consent: true,
    token_filename: "antigravity-oauth.json",
    x_goog_api_client: ANTIGRAVITY_X_GOOG_API_CLIENT,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_id_matches_gemini_cli_upstream() {
        // Pin 防回归 — gemini-cli 历史上 rotate 过一次 client_id。如果 Google
        // 再 rotate 让我们 401,这条断言会被改,同时记录 rotate 时间。
        assert!(CLIENT_ID.starts_with("681255809395-"));
        assert!(CLIENT_ID.ends_with(".apps.googleusercontent.com"));
    }

    #[test]
    fn scopes_include_cloud_platform_and_userinfo() {
        let joined = SCOPES.join(" ");
        assert!(joined.contains("cloud-platform"));
        assert!(joined.contains("userinfo.email"));
        assert!(joined.contains("userinfo.profile"));
    }

    #[test]
    fn cloud_code_base_url_is_internal_endpoint() {
        // 不能误用 generativelanguage.googleapis.com — 那是 API-key 路径。
        assert_eq!(CLOUD_CODE_BASE_URL, "https://cloudcode-pa.googleapis.com");
    }
}
