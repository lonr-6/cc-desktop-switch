//! Antigravity Cloud Code Assist 项目 bootstrap —— `loadCodeAssist` + `onboardUser` LRO。
//!
//! 跟父 crate `cloud_code.rs` 共用 `cloudcode-pa.googleapis.com/v1internal:*`
//! 端点(底层 wire 一样),但用不同 ClientMetadata + UA。
//!
//! ## 关键差异
//!
//! - **ClientMetadata shape**(`auth/antigravity/auth.go:163-169`):
//!   - antigravity: `{ide_type:"ANTIGRAVITY", ide_name:"antigravity", ide_version:<v>}`
//!   - gemini-cli: `{ideType:"IDE_UNSPECIFIED", platform:"DARWIN_ARM64", pluginType:"GEMINI", pluginVersion:"0.34.0"}`
//! - **UA loadCodeAssist**: `antigravity/<v> darwin/arm64 google-api-nodejs-client/10.3.0`
//! - **X-Goog-Api-Client**: `gl-node/22.21.1`
//! - **CLIProxyAPI 实测**:loadCodeAssist 直接返已有 project,不需要 LRO
//!   polling(只在 `cloudaicompanionProject` 缺失时才 onboard)

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::super::cloud_code::CloudCodeError;
use super::super::constants::{
    antigravity_user_agent_loadcodeassist, ANTIGRAVITY_VERSION, ANTIGRAVITY_X_GOOG_API_CLIENT,
    CLOUD_CODE_BASE_URL,
};

/// Antigravity 客户端元数据 — 跟 gemini-cli 的 `ClientMetadata` shape 不同。
/// 字段值字面对齐 CLIProxyAPI `auth/antigravity/auth.go:163-169` + `:256-260`。
#[derive(Debug, Clone, Serialize)]
pub struct AntigravityClientMetadata {
    /// 永远 `"ANTIGRAVITY"` —— Google 上游识别字段
    pub ide_type: &'static str,
    /// 永远 `"antigravity"`
    pub ide_name: &'static str,
    /// Antigravity 版本号(`1.21.9` fallback 或动态拉的最新版)
    pub ide_version: String,
}

impl AntigravityClientMetadata {
    pub fn current() -> Self {
        Self {
            ide_type: "ANTIGRAVITY",
            ide_name: "antigravity",
            ide_version: ANTIGRAVITY_VERSION.to_owned(),
        }
    }
}

/// `loadCodeAssist` 请求 body — antigravity 比 gemini-cli 简单,只有 metadata。
#[derive(Debug, Serialize)]
pub struct LoadCodeAssistRequest {
    pub metadata: AntigravityClientMetadata,
}

#[derive(Debug, Serialize)]
pub struct OnboardUserRequest {
    #[serde(rename = "tierId")]
    pub tier_id: String,
    pub metadata: AntigravityClientMetadata,
}

/// LoadCodeAssist 响应解析 — 字段全部 optional,Google 上游可能返各种 shape。
#[derive(Debug, Deserialize, Default)]
pub struct LoadCodeAssistResponse {
    /// 已有 project_id 时直接返 string;CLIProxyAPI 还见过 nested object
    /// `{cloudaicompanionProject: {id: "..."}}`,两种 shape 都 handle
    #[serde(default, rename = "cloudaicompanionProject")]
    pub cloudaicompanion_project_value: Option<serde_json::Value>,
    #[serde(default, rename = "allowedTiers")]
    pub allowed_tiers: Vec<AllowedTier>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AllowedTier {
    pub id: String,
    #[serde(default, rename = "isDefault")]
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
pub struct LongRunningOperation {
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub response: Option<OnboardUserResponse>,
}

#[derive(Debug, Deserialize)]
pub struct OnboardUserResponse {
    #[serde(rename = "cloudaicompanionProject")]
    pub cloudaicompanion_project: Option<serde_json::Value>,
}

/// 全自动 antigravity bootstrap:loadCodeAssist 拿现有 project,缺则 onboardUser
/// 走 LRO polling。
///
/// 跟 gemini-cli `bootstrap_project` 共用 [`CloudCodeError`] 错误类型(语义等价)。
pub async fn bootstrap_project(
    http: &reqwest::Client,
    access_token: &str,
) -> Result<String, CloudCodeError> {
    bootstrap_project_at(http, CLOUD_CODE_BASE_URL, access_token).await
}

pub(crate) async fn bootstrap_project_at(
    http: &reqwest::Client,
    base_url: &str,
    access_token: &str,
) -> Result<String, CloudCodeError> {
    let metadata = AntigravityClientMetadata::current();
    let user_agent = antigravity_user_agent_loadcodeassist();

    // 1. loadCodeAssist
    let load_url = format!("{base_url}/v1internal:loadCodeAssist");
    let load_req = LoadCodeAssistRequest {
        metadata: metadata.clone(),
    };
    let resp = http
        .post(&load_url)
        .bearer_auth(access_token)
        .header("User-Agent", &user_agent)
        .header("X-Goog-Api-Client", ANTIGRAVITY_X_GOOG_API_CLIENT)
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

    // 2. 如果 loadCodeAssist 直接返 project_id(用户已 onboard),直接用
    if let Some(pid) = extract_project_id(&load_resp.cloudaicompanion_project_value) {
        return Ok(pid);
    }

    // 3. 否则走 onboardUser LRO。tier 从 allowedTiers[isDefault==true] 拿,
    // fallback `legacy-tier`(对齐 CLIProxyAPI `auth.go:222-238`)
    let tier_id = load_resp
        .allowed_tiers
        .iter()
        .find(|t| t.is_default)
        .map(|t| t.id.clone())
        .unwrap_or_else(|| "legacy-tier".to_owned());

    onboard_user_at(
        http,
        base_url,
        access_token,
        &tier_id,
        &metadata,
        &user_agent,
    )
    .await
}

/// LRO polling — 跟 gemini-cli LRO 略微不同(antigravity 用 `:onboardUser` 多次
/// 调而非 `:getOperation`,CLIProxyAPI `auth.go:251-350`)。max 5 次每次 2s 间隔。
async fn onboard_user_at(
    http: &reqwest::Client,
    base_url: &str,
    access_token: &str,
    tier_id: &str,
    metadata: &AntigravityClientMetadata,
    user_agent: &str,
) -> Result<String, CloudCodeError> {
    let onboard_url = format!("{base_url}/v1internal:onboardUser");
    let req = OnboardUserRequest {
        tier_id: tier_id.to_owned(),
        metadata: metadata.clone(),
    };

    const MAX_ATTEMPTS: u32 = 5;
    for attempt in 1..=MAX_ATTEMPTS {
        tracing::debug!(
            attempt,
            max = MAX_ATTEMPTS,
            "antigravity onboardUser polling"
        );
        let resp = http
            .post(&onboard_url)
            .bearer_auth(access_token)
            .header("User-Agent", user_agent)
            .header("X-Goog-Api-Client", ANTIGRAVITY_X_GOOG_API_CLIENT)
            .json(&req)
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
        let lro: LongRunningOperation = serde_json::from_str(&body)
            .map_err(|e| CloudCodeError::OnboardParse(format!("{e}; body={body}")))?;
        if matches!(lro.done, Some(true)) {
            if let Some(resp) = lro.response {
                if let Some(pid) = extract_project_id(&resp.cloudaicompanion_project) {
                    tracing::info!(project_id = %pid, "antigravity 拿到 project_id");
                    return Ok(pid);
                }
            }
            return Err(CloudCodeError::MissingProjectId(
                "antigravity LRO done=true 但 response.cloudaicompanionProject.id 缺失".into(),
            ));
        }
        // done != true,sleep 2s 再轮询
        if attempt < MAX_ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
    Err(CloudCodeError::LroTimeout(Duration::from_secs(
        2 * MAX_ATTEMPTS as u64,
    )))
}

/// project_id 在 cloudaicompanionProject 字段下的两种 shape 通用提取:
/// - string 直接是 ID
/// - object `{id: "..."}` 取 id 字段
fn extract_project_id(v: &Option<serde_json::Value>) -> Option<String> {
    let v = v.as_ref()?;
    if let Some(s) = v.as_str() {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }
    if let Some(obj) = v.as_object() {
        if let Some(id) = obj.get("id").and_then(|x| x.as_str()) {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_serializes_with_antigravity_fields() {
        let m = AntigravityClientMetadata::current();
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"ide_type\":\"ANTIGRAVITY\""));
        assert!(json.contains("\"ide_name\":\"antigravity\""));
        assert!(json.contains("\"ide_version\":"));
        // **不**含 gemini-cli 特有字段(防 antigravity 误用 gemini metadata)
        assert!(!json.contains("ideType"));
        assert!(!json.contains("pluginType"));
        assert!(!json.contains("platform"));
    }

    #[test]
    fn extract_project_id_handles_string_and_object_shapes() {
        let s = serde_json::json!("my-proj-123");
        assert_eq!(extract_project_id(&Some(s)), Some("my-proj-123".to_owned()));

        let obj = serde_json::json!({"id": "obj-proj-456"});
        assert_eq!(
            extract_project_id(&Some(obj)),
            Some("obj-proj-456".to_owned())
        );

        // 空字符串 / 缺字段都返 None
        assert_eq!(extract_project_id(&Some(serde_json::json!(""))), None);
        assert_eq!(
            extract_project_id(&Some(serde_json::json!({"other": "x"}))),
            None
        );
        assert_eq!(extract_project_id(&None), None);
    }
}
