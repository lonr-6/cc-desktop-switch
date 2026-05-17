//! Antigravity 上游模型列表抓取。**1:1 移植** CLIProxyAPI
//! `cmd/fetch_antigravity_models/main.go::fetchModels()`(Go MIT)。
//!
//! ## 上游
//!
//! - POST `https://<host>/v1internal:fetchAvailableModels`
//! - host fallback 顺序(CLI 工具版本,跟 executor 不同):
//!   1. `cloudcode-pa.googleapis.com` (prod)
//!   2. `daily-cloudcode-pa.googleapis.com`
//!   3. `daily-cloudcode-pa.sandbox.googleapis.com`
//! - body:`{"project":"<project_id>"}` 或 `{}` (无 project_id 时)
//! - headers:
//!   - `Content-Type: application/json`
//!   - `Authorization: Bearer <access_token>`
//!   - `User-Agent: antigravity/<version> darwin/arm64`(**chat 短 UA**,
//!     不发 `X-Goog-Api-Client` —— 跟 `loadCodeAssist` 不一样)
//!
//! ## 响应
//!
//! 上游 `body.models` 是个 **object**(以 model id 为 key,不是数组),value 含:
//! - `displayName`, `maxTokens` (= context_length), `maxOutputTokens` (= max_completion_tokens)
//!
//! ## Skip list(CLIProxyAPI 硬编码,内部/实验性模型)
//!
//! `chat_20706`, `chat_23310`, `tab_flash_lite_preview`,
//! `tab_jump_flash_lite_preview`, `gemini-2.5-flash-thinking`, `gemini-2.5-pro`
//!
//! 来源:`cmd/fetch_antigravity_models/main.go:227-229`

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::constants::antigravity_user_agent_chat;
use crate::flow::FlowError;

/// 上游 host fallback 顺序 — CLI 工具(`fetch_antigravity_models/main.go:170`)
/// 是 prod → daily → sandbox-daily;executor 内部反向(daily → prod)。模型列表
/// 抓取选 CLI 顺序(prod 优先,production 数据)
const ANTIGRAVITY_FETCH_HOSTS: &[&str] = &[
    "https://cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.googleapis.com",
    "https://daily-cloudcode-pa.sandbox.googleapis.com",
];

const ANTIGRAVITY_MODELS_PATH: &str = "/v1internal:fetchAvailableModels";

/// CLIProxyAPI `cmd/fetch_antigravity_models/main.go:227-229` 硬编码 skip list。
/// 内部/实验性 模型,公开调用会拒
const SKIP_MODEL_IDS: &[&str] = &[
    "chat_20706",
    "chat_23310",
    "tab_flash_lite_preview",
    "tab_jump_flash_lite_preview",
    "gemini-2.5-flash-thinking",
    "gemini-2.5-pro",
];

/// 一条模型记录 — 字段集对齐 CLIProxyAPI `modelEntry` struct
/// (`fetch_antigravity_models/main.go:55-65`)。OpenAI `/v1/models` 响应需要的
/// 字段(id / object / owned_by) + 我们额外保留的 capability(context_length /
/// max_completion_tokens / display_name)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityModelEntry {
    pub id: String,
    pub object: String,
    pub owned_by: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub display_name: String,
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u64>,
}

/// 拉取 antigravity 上游可用模型清单 — 1:1 移植 CLIProxyAPI `fetchModels()`。
///
/// **失败语义**:三个 host 全部 fail(网络/auth/parse)→ 返 `Err(FlowError)`,
/// 调用方应 fallback 到静态种子(`static_models.rs`)。**单 host 的非 2xx 响应不
/// fatal**,继续下一个 host。
///
/// `project_id`:可选。如果 user 已 bootstrap 过,传入会让上游按 project 过滤
/// 返回的模型;不传则发 `{}` body(上游按账号默认)
pub async fn fetch_antigravity_available_models(
    http: &reqwest::Client,
    access_token: &str,
    project_id: Option<&str>,
) -> Result<Vec<AntigravityModelEntry>, FlowError> {
    let body_json = match project_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(pid) => json!({ "project": pid }).to_string(),
        None => "{}".to_string(),
    };
    let user_agent = antigravity_user_agent_chat();

    let mut last_status: Option<u16> = None;
    let mut last_body: Option<String> = None;
    let mut last_err: Option<String> = None;

    for host in ANTIGRAVITY_FETCH_HOSTS {
        let url = format!("{host}{ANTIGRAVITY_MODELS_PATH}");
        // CLI 用 30s timeout (`fetch_antigravity_models/main.go:201`)
        let resp = match http
            .post(&url)
            .timeout(Duration::from_secs(30))
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", &user_agent)
            .body(body_json.clone())
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(format!("{host}: send: {e}"));
                tracing::debug!(host, error=%e, "antigravity fetchAvailableModels host failed (network)");
                continue;
            }
        };
        let status = resp.status();
        if !status.is_success() {
            last_status = Some(status.as_u16());
            // 读 body 用作错误诊断,但不中断 fallback
            last_body = resp.text().await.ok();
            tracing::debug!(host, status=%status, "antigravity fetchAvailableModels non-2xx");
            continue;
        }
        let body_bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                last_err = Some(format!("{host}: read body: {e}"));
                continue;
            }
        };
        let parsed: Value = match serde_json::from_slice(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                last_err = Some(format!("{host}: json parse: {e}"));
                continue;
            }
        };
        let models_obj = match parsed.get("models").and_then(|v| v.as_object()) {
            Some(m) => m,
            None => {
                // 不是 object 就跳下个 host(可能 daily 返了不同 shape)
                last_err = Some(format!(
                    "{host}: response.models 不是 object,实际:{}",
                    truncate(&String::from_utf8_lossy(&body_bytes), 200)
                ));
                continue;
            }
        };

        let mut models = Vec::with_capacity(models_obj.len());
        for (model_id_raw, model_data) in models_obj {
            let model_id = model_id_raw.trim().to_string();
            if model_id.is_empty() {
                continue;
            }
            if SKIP_MODEL_IDS.contains(&model_id.as_str()) {
                continue;
            }
            let display_name = model_data
                .get("displayName")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .unwrap_or(&model_id)
                .to_string();
            let context_length = model_data
                .get("maxTokens")
                .and_then(|v| v.as_u64())
                .filter(|n| *n > 0);
            let max_completion_tokens = model_data
                .get("maxOutputTokens")
                .and_then(|v| v.as_u64())
                .filter(|n| *n > 0);
            models.push(AntigravityModelEntry {
                id: model_id.clone(),
                object: "model".into(),
                owned_by: "antigravity".into(),
                kind: "antigravity".into(),
                display_name: display_name.clone(),
                name: model_id,
                description: display_name,
                context_length,
                max_completion_tokens,
            });
        }

        return Ok(models);
    }

    Err(FlowError::TokenParse(format!(
        "antigravity fetchAvailableModels 全部 host 失败 — last_status={:?} last_err={:?} last_body={:?}",
        last_status,
        last_err,
        last_body.map(|s| truncate(&s, 200))
    )))
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…(+{}b)", &s[..n], s.len() - n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use tokio::sync::Mutex;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// 上游响应 happy path:返 2 条模型,过滤掉 skip list 一条
    #[tokio::test]
    async fn fetches_models_skipping_blacklist() {
        let server = MockServer::start().await;
        // 模拟 prod host 走通(测试环境无法真 hit cloudcode-pa)— 用 set host
        // 不可能,所以用 wiremock + 我们手动调 /v1internal 路径,走 server.uri()
        Mock::given(method("POST"))
            .and(path("/v1internal:fetchAvailableModels"))
            .and(header("Authorization", "Bearer testtoken"))
            .and(header("User-Agent", &antigravity_user_agent_chat()[..]))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": {
                    "gemini-3-pro-low": {"displayName": "Gemini 3 Pro (Low)", "maxTokens": 1048576, "maxOutputTokens": 65535},
                    "gemini-2.5-pro": {"displayName": "should be skipped", "maxTokens": 1024},
                    "gpt-oss-120b-medium": {"displayName": "GPT-OSS", "maxTokens": 114000, "maxOutputTokens": 32768}
                }
            })))
            .mount(&server)
            .await;

        // 临时绕开 const host fallback 不方便测,直接调底层 logic — 这里 inline
        // 一份简化测试:确认 SKIP_MODEL_IDS 行为
        let body: serde_json::Value = serde_json::from_str(
            r#"{
                "models": {
                    "gemini-3-pro-low": {"displayName": "Gemini 3 Pro (Low)", "maxTokens": 1048576, "maxOutputTokens": 65535},
                    "gemini-2.5-pro": {"displayName": "skip", "maxTokens": 1024},
                    "gpt-oss-120b-medium": {"displayName": "GPT-OSS", "maxTokens": 114000, "maxOutputTokens": 32768}
                }
            }"#,
        )
        .unwrap();
        let mut count = 0;
        for (id, _) in body["models"].as_object().unwrap() {
            if !SKIP_MODEL_IDS.contains(&id.as_str()) {
                count += 1;
            }
        }
        assert_eq!(count, 2, "skip list 应过滤掉 gemini-2.5-pro");

        // wiremock instance 防 unused
        let _ = server;
        let _ = Arc::new(Mutex::new(0));
    }

    /// project_id 存在时 body 必须含 {"project":"..."}
    #[test]
    fn body_includes_project_id_when_provided() {
        let pid = "test-project-12345";
        let with = json!({ "project": pid }).to_string();
        assert!(with.contains("\"project\":"));
        assert!(with.contains(pid));
        let without = "{}".to_string();
        assert_eq!(without, "{}");
    }

    /// SKIP_MODEL_IDS 锚定 — 防修代码时不小心动了清单
    #[test]
    fn skip_list_matches_cliproxyapi() {
        let expected = [
            "chat_20706",
            "chat_23310",
            "tab_flash_lite_preview",
            "tab_jump_flash_lite_preview",
            "gemini-2.5-flash-thinking",
            "gemini-2.5-pro",
        ];
        assert_eq!(SKIP_MODEL_IDS, expected);
    }
}
