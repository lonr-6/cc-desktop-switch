//! Claude Desktop 专属本地路由层(对照 `cc-desktop-switch/backend/proxy.py`)。
//!
//! **架构**(对照 v1.0.18+ `desktop_config_target_for_provider`):
//! - 所有 Claude Desktop 第三方 provider 都强制走本地代理 `127.0.0.1:CD_PROXY_PORT`
//! - plist 写 `inferenceGatewayBaseUrl = http://127.0.0.1:18099`(不写上游 URL)
//! - plist 写 `inferenceGatewayApiKey = <cas 自生成 gateway key>`(不暴露上游真 key)
//! - 客户端发 `body.model = claude-sonnet-4-6` 到本代理
//! - 代理用 `model_alias.rs` 把 `claude-sonnet-4-6` 翻译为 `provider.models["sonnet_4_6"]`
//!   (例如 `kimi-for-coding`)
//! - 重写 `body.model = kimi-for-coding`,转发到 `provider.baseUrl/v1/messages`
//! - 用上游真 `apiKey` 鉴权
//! - 流式 SSE 透传(Anthropic 协议端到端,只翻译 model 名)
//!
//! **端口选 18099**:
//! - 错开 cas 自身 Codex CLI proxy 默认 18080(`crates/proxy/src/server.rs`)
//! - 错开 cc-desktop-switch 默认 18082(避免用户两个工具同时跑撞车)
//! - 错开 OpenAI Codex CLI 工具的 8080 / 4180 等常见 dev 端口

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::model_alias::{
    desktop_model_entries, resolve_requested_model_slot, MODEL_SLOTS,
};
use crate::schema::Provider;

/// **专属端口**:见模块文档解释为什么选这个。
pub const CD_PROXY_PORT: u16 = 18099;

/// 默认本机绑定地址(`127.0.0.1` 只接本机,不对外暴露)。
pub const CD_PROXY_BIND: &str = "127.0.0.1";

/// 代理共享状态 —— 包含当前 active provider + 自生成的 gateway api key。
/// 通过 `apply_provider` 时更新。
#[derive(Debug, Default, Clone)]
pub struct ClaudeDesktopProxyState {
    pub inner: Arc<RwLock<InnerState>>,
}

#[derive(Debug, Default)]
pub struct InnerState {
    /// 当前 active provider(全量克隆 —— 包含上游 baseUrl / apiKey / models / extra_headers)。
    pub provider: Option<Provider>,
    /// cas 自生成的网关 token —— Claude Desktop 客户端发请求时带这个,
    /// 代理验证后改用上游真 key 转发。
    pub gateway_api_key: String,
}

impl ClaudeDesktopProxyState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(InnerState::default())),
        }
    }

    /// 更新 active provider + gateway key。`apply_provider` 时调用。
    pub async fn set_active(&self, provider: Provider, gateway_api_key: String) {
        let mut g = self.inner.write().await;
        g.provider = Some(provider);
        g.gateway_api_key = gateway_api_key;
    }

    /// 清掉 active provider(`clear()` 时调用)。
    pub async fn clear_active(&self) {
        let mut g = self.inner.write().await;
        g.provider = None;
        g.gateway_api_key.clear();
    }

    async fn snapshot(&self) -> InnerSnapshot {
        let g = self.inner.read().await;
        InnerSnapshot {
            provider: g.provider.clone(),
            gateway_api_key: g.gateway_api_key.clone(),
        }
    }
}

struct InnerSnapshot {
    provider: Option<Provider>,
    gateway_api_key: String,
}

/// 构造 axum router。挂到 `tokio::spawn(axum::serve(listener, router))` 即可起服务。
pub fn build_router(state: ClaudeDesktopProxyState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/messages", post(handle_messages))
        .route("/claude/v1/messages", post(handle_messages))
        .route("/v1/models", get(handle_models))
        .route("/claude/v1/models", get(handle_models))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok", "service": "claude-desktop-proxy"}))
}

/// `GET /v1/models` —— 给 Claude Desktop 客户端"伪装"的模型列表
/// (对照 `cc-desktop-switch/backend/proxy.py:556 /v1/models`)。返回 Anthropic
/// 风格 `{data: [{id, ...}]}`,只列出当前 active provider 的 claude-* 安全路由。
async fn handle_models(State(state): State<ClaudeDesktopProxyState>) -> impl IntoResponse {
    let snapshot = state.snapshot().await;
    let Some(provider) = snapshot.provider else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"type":"no_active_provider","message":"未配置 active provider"}})),
        )
            .into_response();
    };
    let entries = desktop_model_entries(&provider, false);
    let models: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "id": e.name,
                "type": "model",
                "display_name": e.display_name,
                "created_at": "2026-01-01T00:00:00Z",
            })
        })
        .collect();
    Json(json!({"data": models, "has_more": false})).into_response()
}

/// `POST /v1/messages` —— 接收 Claude Desktop 转发请求,翻译 body.model,转发上游。
async fn handle_messages(
    State(state): State<ClaudeDesktopProxyState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let snapshot = state.snapshot().await;

    // 1) 验证 gateway key(Claude Desktop 带的是 cas 网关 key,不是上游真 key)
    if let Err(resp) = verify_gateway_key(&headers, &snapshot.gateway_api_key) {
        return resp;
    }

    let Some(provider) = snapshot.provider else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "no_active_provider",
            "未配置 active provider,请先在 cas 中 apply",
        );
    };

    // 2) 解析 body JSON
    let mut body_json: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                &format!("body 不是合法 JSON: {e}"),
            );
        }
    };

    let original_model = body_json
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // 3) 翻译 model 名 claude-sonnet-4-6 → upstream 真实 model id
    //    对照 `cc-desktop-switch/backend/proxy.py:909-916 map_model`。
    let mapped_model = map_claude_route_to_upstream(&provider, &original_model);
    if mapped_model.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "model_not_mapped",
            &format!(
                "模型 '{original_model}' 未在 provider 中配置映射;请在 cas 编辑页填写 sonnet/haiku/opus 槽位"
            ),
        );
    }
    if let Some(obj) = body_json.as_object_mut() {
        obj.insert("model".to_owned(), Value::String(mapped_model.clone()));
    }

    // 4) reqwest 转发到 upstream
    let upstream_url = build_upstream_messages_url(&provider.base_url);
    let upstream_body = match serde_json::to_vec(&body_json) {
        Ok(b) => b,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "body_reserialize",
                &format!("重组 body 失败: {e}"),
            );
        }
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "client_build",
                &format!("reqwest client 构造失败: {e}"),
            );
        }
    };
    let mut req = client
        .post(&upstream_url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("anthropic-version", "2023-06-01");
    // 5) 注入上游真 key + auth scheme
    let upstream_key = provider.api_key.trim();
    if !upstream_key.is_empty() {
        match provider.auth_scheme.as_str() {
            "x-api-key" => {
                req = req.header("x-api-key", upstream_key);
            }
            "none" => {}
            _ => {
                req = req.header("authorization", format!("Bearer {upstream_key}"));
            }
        }
    }
    // provider.extra_headers 也透传(处理 `{apiKey}` 占位符)
    for (name, value) in &provider.extra_headers {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resolved = value.replace("{apiKey}", upstream_key);
        if let Ok(val) = HeaderValue::from_str(&resolved) {
            req = req.header(trimmed, val);
        }
    }

    // 6) 发请求 + 流式回程
    let upstream_resp = match req.body(upstream_body).send().await {
        Ok(r) => r,
        Err(e) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_unreachable",
                &format!("连不上上游 {upstream_url}: {e}"),
            );
        }
    };

    let status = upstream_resp.status();
    let content_type = upstream_resp
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let is_sse = content_type
        .to_str()
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let mut builder = Response::builder().status(status);
    if let Some(headers_mut) = builder.headers_mut() {
        headers_mut.insert("content-type", content_type.clone());
        if is_sse {
            // SSE 防 buffer / chunked
            headers_mut.insert("cache-control", HeaderValue::from_static("no-cache"));
        }
    }

    // 透传 body —— SSE chunked / 普通 JSON 都用 stream body
    let stream = upstream_resp.bytes_stream();
    let body = Body::from_stream(stream);
    builder.body(body).unwrap_or_else(|_| {
        error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "response_build",
            "构造响应失败",
        )
    })
}

fn verify_gateway_key(headers: &HeaderMap, expected: &str) -> Result<(), Response> {
    if expected.is_empty() {
        // 未配置 → 暂不强制鉴权(allow,但记 warning)
        return Ok(());
    }
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let provided = auth.strip_prefix("Bearer ").unwrap_or("").trim();
    if provided == expected {
        return Ok(());
    }
    // x-api-key fallback
    let alt = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    if alt == expected {
        return Ok(());
    }
    Err(error_response(
        StatusCode::UNAUTHORIZED,
        "gateway_auth_failed",
        "gateway key 不匹配 —— Claude Desktop 配置可能过期,请重新 apply",
    ))
}

fn error_response(status: StatusCode, error_type: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
            }
        })),
    )
        .into_response()
}

/// 翻译客户端发的 model 名(`claude-sonnet-4-6` 等白名单)→ 上游真实 model id。
///
/// 优先级:
/// 1. 命中 `MODEL_SLOTS.claude_ids` → 取 provider.models[slot.key]
/// 2. 命中 legacy 别名(`sonnet` → sonnet_4_6) → 同上
/// 3. 未识别 → fallback 用 provider.models["default"]
/// 4. 全空 → 返回空(error)
pub(crate) fn map_claude_route_to_upstream(provider: &Provider, requested: &str) -> String {
    let requested_trim = requested.trim();
    if requested_trim.is_empty() {
        return provider
            .models
            .get("default")
            .cloned()
            .unwrap_or_default()
            .trim()
            .to_owned();
    }
    // 命中 claude_ids
    let lower = requested_trim.to_ascii_lowercase();
    for slot in MODEL_SLOTS {
        for cid in slot.claude_ids {
            if *cid == lower {
                if let Some(v) = provider.models.get(slot.key) {
                    if !v.trim().is_empty() {
                        return v.trim().to_owned();
                    }
                }
                // legacy 别名兜底
                for legacy in slot.legacy {
                    if let Some(v) = provider.models.get(*legacy) {
                        if !v.trim().is_empty() {
                            return v.trim().to_owned();
                        }
                    }
                }
            }
        }
    }
    // 用 resolve_requested_model_slot 走 cc-desktop-switch 的别名解析
    if let Some(slot_key) = resolve_requested_model_slot(requested_trim) {
        if let Some(v) = provider.models.get(slot_key) {
            if !v.trim().is_empty() {
                return v.trim().to_owned();
            }
        }
    }
    // 最后 fallback default
    provider
        .models
        .get("default")
        .cloned()
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// 推导上游 `/v1/messages` 端点。对齐 `cc-desktop-switch/backend/proxy.py
/// build_upstream_url(...)`(api_format=anthropic 分支)。
fn build_upstream_messages_url(base_url: &str) -> String {
    let clean = base_url.trim().trim_end_matches('/');
    let lower = clean.to_ascii_lowercase();
    if lower.ends_with("/v1/messages") || lower.ends_with("/messages") {
        return clean.to_owned();
    }
    if lower.ends_with("/v1") {
        return format!("{clean}/messages");
    }
    format!("{clean}/v1/messages")
}

/// 生成 cas 网关 gateway key —— 类似 cc-desktop-switch
/// `cfg.get_or_create_gateway_api_key()`。前缀 `ccat_` 区分 cas-app-transfer。
pub fn generate_gateway_api_key() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mix = nanos.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(pid);
    format!("ccat_{:032x}", mix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ModelMappings;
    use indexmap::IndexMap;

    fn kimi_code_provider() -> Provider {
        let mut models = ModelMappings::new();
        models.insert("sonnet".to_owned(), "kimi-for-coding".to_owned());
        models.insert("haiku".to_owned(), "kimi-for-coding".to_owned());
        models.insert("opus".to_owned(), "kimi-for-coding".to_owned());
        models.insert("default".to_owned(), "kimi-for-coding".to_owned());
        Provider {
            id: "kimi-code".to_owned(),
            name: "Kimi Code".to_owned(),
            base_url: "https://api.kimi.com/coding".to_owned(),
            auth_scheme: "bearer".to_owned(),
            api_format: "anthropic".to_owned(),
            api_key: "sk-test".to_owned(),
            models,
            base_url_options: Vec::new(),
            base_url_hint: String::new(),
            model_options: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            request_option_presets: IndexMap::new(),
            extra_headers: IndexMap::new(),
            is_builtin: true,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn map_claude_sonnet_to_kimi_for_coding() {
        let p = kimi_code_provider();
        assert_eq!(
            map_claude_route_to_upstream(&p, "claude-sonnet-4-6"),
            "kimi-for-coding"
        );
        assert_eq!(
            map_claude_route_to_upstream(&p, "claude-haiku-4-5"),
            "kimi-for-coding"
        );
        assert_eq!(
            map_claude_route_to_upstream(&p, "claude-opus-4-7"),
            "kimi-for-coding"
        );
    }

    #[test]
    fn map_unknown_falls_back_to_default() {
        let p = kimi_code_provider();
        assert_eq!(
            map_claude_route_to_upstream(&p, "claude-unknown-99"),
            "kimi-for-coding"
        );
    }

    #[test]
    fn upstream_messages_url_strips_trailing_slash() {
        assert_eq!(
            build_upstream_messages_url("https://api.kimi.com/coding"),
            "https://api.kimi.com/coding/v1/messages"
        );
        assert_eq!(
            build_upstream_messages_url("https://api.kimi.com/coding/"),
            "https://api.kimi.com/coding/v1/messages"
        );
        assert_eq!(
            build_upstream_messages_url("https://api.example/v1/messages"),
            "https://api.example/v1/messages"
        );
    }

    #[test]
    fn gateway_key_has_prefix_and_distinct() {
        let a = generate_gateway_api_key();
        let b = generate_gateway_api_key();
        assert!(a.starts_with("ccat_"));
        assert!(b.starts_with("ccat_"));
        // 极小概率碰撞,基本不可能(纳秒级 + pid)
        assert_ne!(a, b);
    }
}
