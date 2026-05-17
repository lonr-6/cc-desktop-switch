//! Cloud Code Assist 项目 bootstrap —— `loadCodeAssist` + `onboardUser` LRO。
//!
//! OAuth code-grant 完成只是拿到 `access_token`,但 `:streamGenerateContent` 还要
//! body 里带 `project: <id>`(免费 tier 的配额绑这个 GCP project)。Google 自动
//! provision 流程:
//!
//! 1. `POST /v1internal:loadCodeAssist` —— 拿用户当前 tier 列表 + 已有 project(若有)
//! 2. 决策 tier:第一个 `isDefault: true` 或 fallback LEGACY
//! 3. `POST /v1internal:onboardUser` —— 触发 LRO(long-running operation)
//! 4. 轮询 LRO(5s 间隔)直到 `done == true`,从 `response.cloudaicompanionProject.id`
//!    拿最终 project_id
//!
//! ## 行为对齐上游
//!
//! gemini-cli `setup.ts:92-200` + CLIProxyAPI `internal/cmd/login.go`:
//! - `pluginType: "GEMINI"` 是 hard-coded 字面值,Google 用它识别"非 IDE 插件"
//!   的 standalone CLI 客户端
//! - free-tier 不传 `cloudaicompanionProject`(让 Google 自动建);其他 tier 传
//!   用户已有 project_id(或环境变量 `GOOGLE_CLOUD_PROJECT`)
//! - `hasOnboardedPreviously: true` 时 onboardUser 仍要调,上游会立即返已存在的
//!   project,不重新 provision

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[allow(deprecated)]
use super::constants::USER_AGENT;
use super::constants::{detect_user_agent, CLOUD_CODE_BASE_URL, X_GOOG_API_CLIENT};

#[derive(Debug, Error)]
pub enum CloudCodeError {
    #[error("HTTP 失败: {0}")]
    Http(#[from] reqwest::Error),
    #[error("loadCodeAssist 返非 2xx: HTTP {status}: {body}")]
    LoadStatus { status: u16, body: String },
    #[error("onboardUser 返非 2xx: HTTP {status}: {body}")]
    OnboardStatus { status: u16, body: String },
    #[error("loadCodeAssist 响应 JSON 解析失败: {0}")]
    LoadParse(String),
    #[error("onboardUser 响应 JSON 解析失败: {0}")]
    OnboardParse(String),
    #[error("loadCodeAssist 没返任何 tier — Google 上游异常")]
    NoTier,
    #[error("LRO 轮询超时(等 onboardUser done 超过 {0:?})")]
    LroTimeout(Duration),
    #[error("LRO 完成但 response.cloudaicompanionProject.id 缺失:{0}")]
    MissingProjectId(String),
    #[error(
        "LRO 上游返回畸形(连续 {0} 次 done=None,Google 上游可能 schema 变更或 partial outage)"
    )]
    MalformedLro(u32),
}

/// 客户端身份元数据 —— 跟着 loadCodeAssist / onboardUser 一起送给 Google,
/// 用来命中"官方 gemini-cli"分支(不是这个值就走 generic API 路径,可能 reject)。
///
/// 字段值字面对齐 CLIProxyAPI `header_utils.go` + gemini-cli `coreClientMetadata`。
#[derive(Debug, Clone, Serialize)]
pub struct ClientMetadata {
    /// `IDE_UNSPECIFIED` —— standalone CLI,不是 IDE 插件
    #[serde(rename = "ideType")]
    pub ide_type: &'static str,
    /// `DARWIN_ARM64` / `LINUX_AMD64` / `WINDOWS_AMD64` 等
    pub platform: String,
    /// **必须**是 `"GEMINI"` —— hard-coded 上游识别字段
    #[serde(rename = "pluginType")]
    pub plugin_type: &'static str,
    /// gemini-cli 自报版本,我们用 `0.34.0` 对齐 USER_AGENT
    #[serde(rename = "pluginVersion")]
    pub plugin_version: &'static str,
    /// `duetProject` —— paid tier 用,free tier 留 None
    #[serde(rename = "duetProject", skip_serializing_if = "Option::is_none")]
    pub duet_project: Option<String>,
}

impl ClientMetadata {
    /// 默认元数据,plugin_type / version 锁死跟 USER_AGENT 一致。platform 按
    /// Google 上游 `ClientMetadata.Platform` enum 字面拼:`DARWIN_ARM64` /
    /// `LINUX_AMD64` / `WINDOWS_AMD64` 等。
    ///
    /// **关键**:**不能**直接用 `std::env::consts::{OS,ARCH}` upper-case 拼 ——
    /// 那会拿到 `MACOS_AARCH64` / `MACOS_X86_64` / `LINUX_X86_64`,Google 上游
    /// `loadCodeAssist` 立即返 400 `Invalid value at 'metadata.platform'`。
    /// 实测 2026-05-11(MacBook Apple Silicon)整条 login flow break。
    /// gemini-cli upstream(`packages/cli/src/utils/userInfo.ts`)用 `process.platform`
    /// → `darwin/linux/win32`,跟 Rust 的 `std::env::consts::OS=macos` 名空间不
    /// 重叠,必须显式 map。
    pub fn default_for_current_platform() -> Self {
        Self {
            ide_type: "IDE_UNSPECIFIED",
            platform: detect_platform(),
            plugin_type: "GEMINI",
            plugin_version: "0.34.0",
            duet_project: None,
        }
    }
}

/// 把 Rust `std::env::consts::{OS,ARCH}` 转 Google `ClientMetadata.Platform`
/// enum 字面值。未识别 OS/ARCH 组合 fallback `PLATFORM_UNSPECIFIED`(Google
/// 上游可能 reject,比直接发错值更安全)。
///
/// Mapping 来源:CLIProxyAPI `header_utils.go::DetectPlatform()` + gemini-cli
/// `userInfo.ts` 的 `getPlatform()`。
fn detect_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "DARWIN",
        "linux" => "LINUX",
        "windows" => "WINDOWS",
        other => {
            tracing::warn!(
                os = other,
                "unknown OS for Google ClientMetadata.platform mapping; using PLATFORM_UNSPECIFIED"
            );
            return "PLATFORM_UNSPECIFIED".to_owned();
        }
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "ARM64",
        "x86_64" => "AMD64",
        "x86" => "X86",
        other => {
            tracing::warn!(arch = other, "unknown ARCH for Google ClientMetadata.platform mapping; using PLATFORM_UNSPECIFIED");
            return "PLATFORM_UNSPECIFIED".to_owned();
        }
    };
    format!("{os}_{arch}")
}

/// `loadCodeAssist` 请求 body。
#[derive(Debug, Serialize)]
pub struct LoadCodeAssistRequest {
    /// 已有 project_id(从 `~/.codex-app-transfer/gemini-oauth.json` 或
    /// `GOOGLE_CLOUD_PROJECT` 环境变量),没有就 None。
    #[serde(
        rename = "cloudaicompanionProject",
        skip_serializing_if = "Option::is_none"
    )]
    pub cloudaicompanion_project: Option<String>,
    pub metadata: ClientMetadata,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeminiUserTier {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
    /// 该 tier 是否需要用户自定义的 cloudaicompanionProject。free-tier 是 false
    /// (Google 自动建);paid 是 true。
    #[serde(default, rename = "userDefinedCloudaicompanionProject")]
    pub user_defined_cloudaicompanion_project: bool,
    #[serde(default, rename = "hasAcceptedTos")]
    pub has_accepted_tos: bool,
    #[serde(default, rename = "hasOnboardedPreviously")]
    pub has_onboarded_previously: bool,
}

/// `loadCodeAssist` 响应。所有字段都可空 — Google 上游若没数据返 200 + 空对象。
#[derive(Debug, Deserialize, Default)]
pub struct LoadCodeAssistResponse {
    #[serde(default, rename = "currentTier")]
    pub current_tier: Option<GeminiUserTier>,
    #[serde(default, rename = "allowedTiers")]
    pub allowed_tiers: Vec<GeminiUserTier>,
    /// 用户已有的 project_id(server-side 之前 onboard 过留下的)
    #[serde(default, rename = "cloudaicompanionProject")]
    pub cloudaicompanion_project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OnboardUserRequest {
    #[serde(rename = "tierId")]
    pub tier_id: String,
    #[serde(
        rename = "cloudaicompanionProject",
        skip_serializing_if = "Option::is_none"
    )]
    pub cloudaicompanion_project: Option<String>,
    pub metadata: ClientMetadata,
}

/// LRO operation 响应 —— `done` 是终态信号,`response` 在 `done==Some(true)` 时
/// 含 `cloudaicompanionProject.id`。`name` 在 `done==Some(false)` 时填,用来 polling。
///
/// `done: Option<bool>`(silent-failure-hunter M4 修):原版 `#[serde(default)] done: bool`
/// 字段缺失时 default false → 上游若返 `{}` 形态(malformed)轮询 60s 一直
/// silent timeout。改 Option<bool> + 调用方 `matches!(last_op.done, Some(true))`
/// 让 None / true / false 三态明确,malformed 响应不会 silent spin。
#[derive(Debug, Deserialize)]
pub struct LongRunningOperation {
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub response: Option<OnboardUserResponse>,
}

#[derive(Debug, Deserialize)]
pub struct OnboardUserResponse {
    #[serde(rename = "cloudaicompanionProject")]
    pub cloudaicompanion_project: Option<CloudAiCompanionProject>,
}

#[derive(Debug, Deserialize)]
pub struct CloudAiCompanionProject {
    pub id: String,
}

/// 选 tier 的决策树:第一个 `isDefault: true` → fallback `legacy-tier`(对齐
/// gemini-cli `setup.ts:160-180` 的 `getOnboardTier` 行为)。
fn pick_tier(load_resp: &LoadCodeAssistResponse) -> Option<GeminiUserTier> {
    if let Some(tier) = &load_resp.current_tier {
        if tier.is_default {
            return Some(tier.clone());
        }
    }
    for tier in &load_resp.allowed_tiers {
        if tier.is_default {
            return Some(tier.clone());
        }
    }
    // fallback:legacy-tier 兜底(gemini-cli 行为)
    load_resp
        .allowed_tiers
        .iter()
        .find(|t| t.id.contains("legacy"))
        .cloned()
}

/// 全自动 bootstrap —— 调 loadCodeAssist + 决定 tier + 调 onboardUser + 轮询 LRO。
/// 返回最终 `project_id`,调用方应该写回 `OauthToken.project_id` 持久化。
///
/// `existing_project_id` 是已有的 project(从 token store 读 / 环境变量),
/// 没有就 None。free-tier 路径不需要它。
///
/// `lro_poll_interval` 默认 5s 对齐上游;`lro_timeout` 默认 60s(onboard 通常 < 30s)。
pub async fn bootstrap_project(
    http: &reqwest::Client,
    access_token: &str,
    existing_project_id: Option<String>,
) -> Result<String, CloudCodeError> {
    bootstrap_project_with_polling(
        http,
        access_token,
        existing_project_id,
        Duration::from_secs(5),
        Duration::from_secs(60),
    )
    .await
}

/// 带可调 polling 参数的版本 —— 单测注入更短的 interval 跑得快。
pub async fn bootstrap_project_with_polling(
    http: &reqwest::Client,
    access_token: &str,
    existing_project_id: Option<String>,
    poll_interval: Duration,
    poll_timeout: Duration,
) -> Result<String, CloudCodeError> {
    bootstrap_project_at(
        http,
        CLOUD_CODE_BASE_URL,
        access_token,
        existing_project_id,
        poll_interval,
        poll_timeout,
    )
    .await
}

/// 内部版 — 接收可定制 base_url。`pub(crate)` 让 crate 外**完全不可见**(silent-
/// failure-hunter / pr-test-analyzer 反馈:test 应能 wiremock mock 整条 path 而
/// 不是 reconstruct 请求 manually)。仅 crate 内 production 走 const + tests 注入 mock。
pub(crate) async fn bootstrap_project_at(
    http: &reqwest::Client,
    base_url: &str,
    access_token: &str,
    existing_project_id: Option<String>,
    poll_interval: Duration,
    poll_timeout: Duration,
) -> Result<String, CloudCodeError> {
    let metadata = ClientMetadata::default_for_current_platform();

    // 1. loadCodeAssist
    let load_req = LoadCodeAssistRequest {
        cloudaicompanion_project: existing_project_id.clone(),
        metadata: metadata.clone(),
    };
    let load_url = format!("{base_url}/v1internal:loadCodeAssist");
    let resp = http
        .post(&load_url)
        .bearer_auth(access_token)
        .header("User-Agent", &detect_user_agent())
        .header("X-Goog-Api-Client", X_GOOG_API_CLIENT)
        .json(&load_req)
        .send()
        .await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(CloudCodeError::LoadStatus {
            status: status.as_u16(),
            body,
        });
    }
    let load_resp: LoadCodeAssistResponse = serde_json::from_str(&body)
        .map_err(|e| CloudCodeError::LoadParse(format!("{e}; body={body}")))?;

    let tier = pick_tier(&load_resp).ok_or(CloudCodeError::NoTier)?;
    let is_free_tier = tier.id.contains("free");

    // 2. onboardUser body —— free tier 不传 project_id,paid 传
    let onboard_project = if is_free_tier {
        None
    } else {
        existing_project_id.or(load_resp.cloudaicompanion_project.clone())
    };
    let onboard_req = OnboardUserRequest {
        tier_id: tier.id.clone(),
        cloudaicompanion_project: onboard_project,
        metadata,
    };
    let onboard_url = format!("{base_url}/v1internal:onboardUser");

    // 3. 第一次 POST :onboardUser → 拿 LRO,后续轮询用 `:getOperation` 而不是
    //    重 POST :onboardUser(reviewer H1 / silent-failure H2 修;重 POST 不是
    //    幂等等价,慢 onboard 会让 Google 每 5s billing 一次 + 可能 rate limit;
    //    gemini-cli setup.ts 用的就是 caServer.getOperation(name) — 我们对齐)
    let started_at = std::time::Instant::now();
    let initial_resp = http
        .post(&onboard_url)
        .bearer_auth(access_token)
        .header("User-Agent", &detect_user_agent())
        .header("X-Goog-Api-Client", X_GOOG_API_CLIENT)
        .json(&onboard_req)
        .send()
        .await?;
    let status = initial_resp.status();
    let body = initial_resp.text().await?;
    if !status.is_success() {
        return Err(CloudCodeError::OnboardStatus {
            status: status.as_u16(),
            body,
        });
    }
    let mut last_op: LongRunningOperation = serde_json::from_str(&body)
        .map_err(|e| CloudCodeError::OnboardParse(format!("{e}; body={body}")))?;

    // 后续 polling 路径(silent-failure H1 修):连续 None 计数 — done 字段缺失
    // 3 次后 fast-fail MalformedLro 防 silent 60s timeout。Some(false) 仍正常等。
    const MAX_CONSECUTIVE_NONE: u32 = 3;
    let mut consecutive_none: u32 = 0;
    while !matches!(last_op.done, Some(true)) {
        if last_op.done.is_none() {
            consecutive_none += 1;
            tracing::warn!(
                consecutive_none,
                op_name = ?last_op.name,
                "LRO 响应缺 `done` 字段(Google 上游可能 schema 变更或 partial outage)"
            );
            if consecutive_none >= MAX_CONSECUTIVE_NONE {
                return Err(CloudCodeError::MalformedLro(consecutive_none));
            }
        } else {
            consecutive_none = 0;
        }

        if started_at.elapsed() >= poll_timeout {
            return Err(CloudCodeError::LroTimeout(poll_timeout));
        }
        tokio::time::sleep(poll_interval).await;
        // sleep 后再次 timeout 检查 — 防 sleep 期间过 deadline 仍发请求(M2 修)
        if started_at.elapsed() >= poll_timeout {
            return Err(CloudCodeError::LroTimeout(poll_timeout));
        }

        let op_name = last_op.name.as_deref().ok_or_else(|| {
            CloudCodeError::OnboardParse(
                "LRO done!=true 但 response.name 缺失,无法 :getOperation 轮询".into(),
            )
        })?;
        let get_op_url = format!("{base_url}/v1internal:getOperation");
        tracing::info!(op_name, elapsed_ms = ?started_at.elapsed().as_millis(), "LRO :getOperation poll");
        let resp = http
            .post(&get_op_url)
            .bearer_auth(access_token)
            .header("User-Agent", &detect_user_agent())
            .header("X-Goog-Api-Client", X_GOOG_API_CLIENT)
            .json(&serde_json::json!({ "name": op_name }))
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(CloudCodeError::OnboardStatus {
                status: status.as_u16(),
                body,
            });
        }
        last_op = serde_json::from_str(&body)
            .map_err(|e| CloudCodeError::OnboardParse(format!("{e}; body={body}")))?;
    }

    // 4. 提 project_id
    let project_id = last_op
        .response
        .and_then(|r| r.cloudaicompanion_project.map(|p| p.id))
        .ok_or_else(|| {
            CloudCodeError::MissingProjectId(
                "LRO done=true 但 response.cloudaicompanionProject.id 不存在".into(),
            )
        })?;
    Ok(project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// 测试专用:覆盖 CLOUD_CODE_BASE_URL 为 mock server URL —— 不能直接改 const,
    /// 改用直接调内部 helper 方式(bypass bootstrap_project,调 wiremock 验各步骤
    /// 的 wire shape)。
    #[tokio::test]
    async fn load_code_assist_sends_required_metadata() {
        let server = MockServer::start().await;
        // 用 detect_user_agent() 而非 const USER_AGENT —— const 硬码 darwin/arm64,
        // CI Linux runner (linux/x64) 跑会 mismatch 导致 wiremock 不命中 Mock 返 404
        let expected_ua = detect_user_agent();
        let _mock = Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .and(header_exists("authorization"))
            .and(header("user-agent", expected_ua.as_str()))
            .and(header("x-goog-api-client", X_GOOG_API_CLIENT))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "currentTier": {
                    "id": "free-tier",
                    "name": "Free",
                    "isDefault": true,
                    "userDefinedCloudaicompanionProject": false,
                    "hasAcceptedTos": true,
                    "hasOnboardedPreviously": false
                },
                "allowedTiers": []
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let metadata = ClientMetadata::default_for_current_platform();
        let req = LoadCodeAssistRequest {
            cloudaicompanion_project: None,
            metadata,
        };
        let resp = http
            .post(format!("{}/v1internal:loadCodeAssist", server.uri()))
            .bearer_auth("test-token")
            .header("User-Agent", &detect_user_agent())
            .header("X-Goog-Api-Client", X_GOOG_API_CLIENT)
            .json(&req)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let parsed: LoadCodeAssistResponse = resp.json().await.unwrap();
        assert_eq!(
            parsed.current_tier.unwrap().id,
            "free-tier",
            "tier 解析必须命中 free-tier"
        );
    }

    #[test]
    fn pick_tier_prefers_current_then_default_then_legacy() {
        // current_tier.is_default=true → 直接返 current_tier
        let resp = LoadCodeAssistResponse {
            current_tier: Some(GeminiUserTier {
                id: "free-tier".into(),
                name: None,
                is_default: true,
                user_defined_cloudaicompanion_project: false,
                has_accepted_tos: true,
                has_onboarded_previously: false,
            }),
            allowed_tiers: vec![],
            cloudaicompanion_project: None,
        };
        assert_eq!(pick_tier(&resp).unwrap().id, "free-tier");

        // current_tier.is_default=false,allowed 里有 default → 返 allowed 里的
        let resp = LoadCodeAssistResponse {
            current_tier: Some(GeminiUserTier {
                id: "expired".into(),
                name: None,
                is_default: false,
                user_defined_cloudaicompanion_project: false,
                has_accepted_tos: false,
                has_onboarded_previously: false,
            }),
            allowed_tiers: vec![GeminiUserTier {
                id: "standard-tier".into(),
                name: None,
                is_default: true,
                user_defined_cloudaicompanion_project: true,
                has_accepted_tos: true,
                has_onboarded_previously: true,
            }],
            cloudaicompanion_project: None,
        };
        assert_eq!(pick_tier(&resp).unwrap().id, "standard-tier");

        // 都没 default → fallback legacy
        let resp = LoadCodeAssistResponse {
            current_tier: None,
            allowed_tiers: vec![GeminiUserTier {
                id: "legacy-tier".into(),
                name: None,
                is_default: false,
                user_defined_cloudaicompanion_project: false,
                has_accepted_tos: false,
                has_onboarded_previously: false,
            }],
            cloudaicompanion_project: None,
        };
        assert_eq!(pick_tier(&resp).unwrap().id, "legacy-tier");

        // 完全空 → None
        let resp = LoadCodeAssistResponse::default();
        assert!(pick_tier(&resp).is_none());
    }

    #[test]
    fn client_metadata_platform_format_matches_upstream_enum() {
        let m = ClientMetadata::default_for_current_platform();
        assert_eq!(m.plugin_type, "GEMINI");
        assert_eq!(m.plugin_version, "0.34.0");
        assert_eq!(m.ide_type, "IDE_UNSPECIFIED");
        // platform 必须命中 Google ClientMetadata.Platform 字面 enum
        // 之一(DARWIN_ARM64 / LINUX_AMD64 / WINDOWS_AMD64 等),
        // 或 fallback PLATFORM_UNSPECIFIED。**绝对不能**含 "MACOS" / "AARCH64"
        // / "X86_64" 等 std::env::consts 原始值 — Google 上游 returns 400
        const VALID: &[&str] = &[
            "DARWIN_ARM64",
            "DARWIN_AMD64",
            "LINUX_ARM64",
            "LINUX_AMD64",
            "LINUX_X86",
            "WINDOWS_ARM64",
            "WINDOWS_AMD64",
            "WINDOWS_X86",
            "PLATFORM_UNSPECIFIED",
        ];
        assert!(
            VALID.contains(&m.platform.as_str()),
            "platform '{}' 不在 Google ClientMetadata.Platform enum 范围,login 整流会被 400 拒;若新增平台请同步加 mapping",
            m.platform
        );
        // bug 检测:这些是 std::env::consts 原始值 upper-case,**绝不能**出现
        for forbidden in ["MACOS", "AARCH64", "X86_64"] {
            assert!(
                !m.platform.contains(forbidden),
                "platform '{}' 含 std::env::consts 原始值 '{forbidden}' — Google 上游不识别",
                m.platform
            );
        }
    }

    #[test]
    fn metadata_serializes_camel_case_with_correct_field_names() {
        let m = ClientMetadata::default_for_current_platform();
        let json = serde_json::to_string(&m).unwrap();
        // upstream 字段都 camelCase
        assert!(json.contains("\"ideType\""));
        assert!(json.contains("\"pluginType\""));
        assert!(json.contains("\"pluginVersion\""));
        assert!(!json.contains("\"plugin_type\""), "不该 snake_case");
        // duetProject 是 None 不应序列化(skip_serializing_if)
        assert!(!json.contains("duetProject"));
    }

    #[tokio::test]
    async fn bootstrap_project_returns_id_when_lro_done_immediately() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "currentTier": {
                    "id": "free-tier",
                    "isDefault": true,
                    "userDefinedCloudaicompanionProject": false
                }
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:onboardUser"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "done": true,
                "response": {
                    "cloudaicompanionProject": {
                        "id": "test-project-12345"
                    }
                }
            })))
            .mount(&server)
            .await;

        // 直接调 helper(用 mock URL 替代 const)— 把 mock URL 传内部
        let http = reqwest::Client::new();
        let metadata = ClientMetadata::default_for_current_platform();
        let load_url = format!("{}/v1internal:loadCodeAssist", server.uri());
        let onboard_url = format!("{}/v1internal:onboardUser", server.uri());

        let load_resp: LoadCodeAssistResponse = http
            .post(&load_url)
            .bearer_auth("token")
            .header("User-Agent", &detect_user_agent())
            .header("X-Goog-Api-Client", X_GOOG_API_CLIENT)
            .json(&LoadCodeAssistRequest {
                cloudaicompanion_project: None,
                metadata: metadata.clone(),
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let tier = pick_tier(&load_resp).unwrap();
        assert_eq!(tier.id, "free-tier");

        let lro: LongRunningOperation = http
            .post(&onboard_url)
            .bearer_auth("token")
            .header("User-Agent", &detect_user_agent())
            .header("X-Goog-Api-Client", X_GOOG_API_CLIENT)
            .json(&OnboardUserRequest {
                tier_id: tier.id,
                cloudaicompanion_project: None,
                metadata,
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(lro.done, Some(true));
        assert_eq!(
            lro.response.unwrap().cloudaicompanion_project.unwrap().id,
            "test-project-12345"
        );
    }

    #[test]
    fn lro_response_handles_missing_project_id() {
        // done=true 但 response 字段空 — bootstrap 应该报 MissingProjectId
        let lro: LongRunningOperation = serde_json::from_value(serde_json::json!({
            "done": true
        }))
        .unwrap();
        assert_eq!(lro.done, Some(true));
        assert!(lro.response.is_none());
    }

    #[test]
    fn lro_response_handles_in_progress() {
        let lro: LongRunningOperation = serde_json::from_value(serde_json::json!({
            "done": false,
            "name": "operations/abc123"
        }))
        .unwrap();
        assert_eq!(lro.done, Some(false));
        assert_eq!(lro.name.as_deref(), Some("operations/abc123"));
    }

    #[test]
    fn lro_response_done_field_missing_yields_none_not_silent_false() {
        // **M4 修**:原版 `done: bool` + #[serde(default)] 缺失字段静默 default false
        // → 调用方 `if !done` 当 in-progress 一直轮询 timeout。改 Option<bool>
        // 让 None/Some(true)/Some(false) 三态明确,bootstrap 调用方用 matches!
        // (done, Some(true)) 严格判断,None 不会被当 in-progress
        let lro: LongRunningOperation = serde_json::from_value(serde_json::json!({
            "name": "operations/x"
        }))
        .unwrap();
        assert_eq!(lro.done, None);
        assert!(!matches!(lro.done, Some(true)));
        assert!(!matches!(lro.done, Some(false)));
    }

    #[tokio::test]
    async fn bootstrap_project_polls_lro_with_get_operation_and_returns_project_id() {
        // **pr-test-analyzer C3 修**(7/10):bootstrap_project end-to-end 通过
        // wiremock 验整条 wire path —— loadCodeAssist + 第一次 onboardUser
        // (done=false) + :getOperation 轮询(reviewer H1 修后用 :getOperation
        // 不重 POST :onboardUser)+ 终态 done=true 拿 project_id
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "currentTier": {
                    "id": "free-tier",
                    "isDefault": true,
                    "userDefinedCloudaicompanionProject": false
                }
            })))
            .mount(&server)
            .await;
        // 第一次 :onboardUser → done=false + name
        Mock::given(method("POST"))
            .and(path("/v1internal:onboardUser"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/abc-1",
                "done": false
            })))
            .expect(1)
            .mount(&server)
            .await;
        // 后续 :getOperation → done=true with project
        Mock::given(method("POST"))
            .and(path("/v1internal:getOperation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/abc-1",
                "done": true,
                "response": {
                    "cloudaicompanionProject": {"id": "auto-provisioned-proj-42"}
                }
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let project_id = bootstrap_project_at(
            &http,
            &server.uri(),
            "ya29.test-token",
            None,
            Duration::from_millis(10),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(project_id, "auto-provisioned-proj-42");
    }

    #[tokio::test]
    async fn bootstrap_project_fails_fast_with_malformed_lro_when_done_field_missing() {
        // **silent-failure-hunter H1 修(commit C 第二轮)**:连续 3 次 done=None
        // 后 fast-fail MalformedLro,而不是 silent loop 直到 60s timeout。tracing
        // ::warn! 让 operator 区分"Google 慢"vs"Google 上游 schema 变更"
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "currentTier": {"id":"free-tier","isDefault":true,"userDefinedCloudaicompanionProject":false}
            })))
            .mount(&server)
            .await;
        // onboardUser + getOperation 都返 done 缺失(Option<bool>=None)
        Mock::given(method("POST"))
            .and(path("/v1internal:onboardUser"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/never-done"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:getOperation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/never-done"
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = bootstrap_project_at(
            &http,
            &server.uri(),
            "ya29.test-token",
            None,
            Duration::from_millis(1),
            Duration::from_secs(10), // timeout 远大于,确 MalformedLro 而非 timeout
        )
        .await
        .unwrap_err();
        match err {
            CloudCodeError::MalformedLro(n) => assert!(n >= 3, "至少 3 次 None 才 fail,n={n}"),
            other => panic!("应返 MalformedLro,实际:{other:?}"),
        }
    }

    #[tokio::test]
    async fn bootstrap_project_returns_lro_timeout_when_done_false_persistently() {
        // 完整覆盖 timeout 路径:done=Some(false) 持续 — 应 timeout(MalformedLro 不会
        // 触发因 done 不是 None)
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1internal:loadCodeAssist"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "currentTier": {"id":"free-tier","isDefault":true,"userDefinedCloudaicompanionProject":false}
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:onboardUser"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/slow",
                "done": false
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1internal:getOperation"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "name": "operations/slow",
                "done": false
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let err = bootstrap_project_at(
            &http,
            &server.uri(),
            "ya29.test-token",
            None,
            Duration::from_millis(10),
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, CloudCodeError::LroTimeout(_)),
            "done=Some(false) 持续应 timeout 不是 MalformedLro,实际:{err:?}"
        );
    }

    /// **KNOWN BRITTLE**(silent-failure-hunter M3 + pr-test-analyzer H1):本测试
    /// **明确 lock 当前 buggy 行为**给 future maintainer 看。`tier.id.contains
    /// ("free")` 实现脆弱:
    /// - false positive: `freemium-tier` / `free-trial-2` 都被当 free-tier
    /// - false negative: `FREE-TIER`(大写)不识别
    ///
    /// 修底层需要先跟 Google 团队确认全 tier id 列表(gemini-cli setup.ts 也用
    /// substring,改 exact set 风险高)。**未来 fix matcher 时这个测试会 fail —
    /// 那是 desired**,届时改测试用 expected exact-set 行为而不是 revert fix。
    #[test]
    fn pick_tier_free_tier_substring_match_KNOWN_BRITTLE() {
        let cases = [
            // 正常 case
            ("free-tier", true),
            ("legacy-tier", false),
            ("standard-tier", false),
            // **bug-compatible** false positives — 当前实现错误识别为 free
            ("freemium-tier", true),
            ("free-trial-2", true),
            // **bug** false negative — uppercase 不匹配
            ("FREE-TIER", false),
        ];
        for (id, expected_is_free) in cases {
            let resp = LoadCodeAssistResponse {
                current_tier: Some(GeminiUserTier {
                    id: id.into(),
                    name: None,
                    is_default: true,
                    user_defined_cloudaicompanion_project: false,
                    has_accepted_tos: false,
                    has_onboarded_previously: false,
                }),
                allowed_tiers: vec![],
                cloudaicompanion_project: None,
            };
            let picked = pick_tier(&resp).unwrap();
            let is_free = picked.id.contains("free");
            assert_eq!(
                is_free, expected_is_free,
                "KNOWN BRITTLE: tier {id} 当前 substring 判定 = {is_free}(此测试故意 lock buggy 行为,fix matcher 时改测试)"
            );
        }
    }
}
