//! `/api/claude-desktop/*` handler ——
//!
//! 对称 `desktop.rs`(Codex CLI 端)设计,但**完全独立**两套 provider 列表:
//! - cas 现有 Provider(Codex CLI 端,`apiFormat="openai_chat"`,base url
//!   形如 `https://api.deepseek.com/v1`,走 cas proxy 协议转换)
//! - 本 handler 管理的 Claude Desktop Provider(`apiFormat="anthropic"`,base
//!   url 形如 `https://api.deepseek.com/anthropic`,Claude Desktop 直连**不**
//!   走 cas proxy)
//!
//! 持久化位于 `~/.codex-app-transfer/claude-desktop-config.json`(独立文件,
//! 跟 cas 顶层 `config.json` 互不影响)。

use std::sync::OnceLock;

use std::fs;

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use codex_app_transfer_claude_desktop::{
    apply::{ApplyConfig, ApplyResult},
    apply_provider, build_proxy_router, builtin_presets, claude_desktop_proxy_log_dir,
    claude_desktop_proxy_telemetry, generate_gateway_api_key, has_snapshot, list_snapshots,
    load_config, macos as cd_macos, restore_state, save_config,
    schema::{ClaudeDesktopConfig, Provider as ClaudeDesktopProvider},
    ClaudeDesktopPaths, ClaudeDesktopProxyState, CD_PROXY_BIND, CD_PROXY_PORT,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::common::{open_directory, APP_VERSION};

/// 全局 Claude Desktop proxy 状态 —— 单例,首次 apply 时初始化并启动 18099 端口
/// 的 axum server;cas 进程退出由 OS 自动清理。
static CD_PROXY: OnceLock<ClaudeDesktopProxyState> = OnceLock::new();

/// 获取(或首次初始化)Claude Desktop proxy 单例。返回的 state 可以更新
/// active provider;同时确保 axum server 已经在监听 `CD_PROXY_PORT`。
fn cd_proxy_or_init() -> ClaudeDesktopProxyState {
    if let Some(s) = CD_PROXY.get() {
        return s.clone();
    }
    let state = ClaudeDesktopProxyState::new();
    let state_for_server = state.clone();
    // spawn axum server。bind 失败(端口占用)只 log 不阻塞 apply —— 用户能看到
    // Claude Desktop 报 connection_refused 时再回头查 cas 日志。
    tokio::spawn(async move {
        let bind = format!("{CD_PROXY_BIND}:{CD_PROXY_PORT}");
        let listener = match tokio::net::TcpListener::bind(&bind).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!(
                    "[claude_desktop_proxy] bind {bind} 失败: {e} —— Claude Desktop 请求将 connection refused。请检查端口占用。"
                );
                return;
            }
        };
        eprintln!("[claude_desktop_proxy] listening on {bind}");
        let app = build_proxy_router(state_for_server);
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[claude_desktop_proxy] serve 错误: {e}");
        }
    });
    let _ = CD_PROXY.set(state.clone());
    state
}

fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"success": false, "message": msg.into()})),
    )
}

fn paths_or_err() -> Result<ClaudeDesktopPaths, (StatusCode, Json<Value>)> {
    ClaudeDesktopPaths::from_home_env()
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn load_or_err() -> Result<(ClaudeDesktopPaths, ClaudeDesktopConfig), (StatusCode, Json<Value>)> {
    let paths = paths_or_err()?;
    let cfg = load_config(&paths)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok((paths, cfg))
}

/// GET `/api/claude-desktop/status` —— Claude Desktop config 写入状态 +
/// active provider + snapshot 信息。
pub async fn status() -> impl IntoResponse {
    let paths = match paths_or_err() {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    let cfg = load_config(&paths).unwrap_or_default();

    // macOS 平台:读 plist + config.json 状态
    #[allow(unused_mut)]
    let mut desktop_status: Value = Value::Null;
    #[cfg(target_os = "macos")]
    {
        if let Ok(s) = cd_macos::get_status() {
            desktop_status = serde_json::to_value(s).unwrap_or(Value::Null);
        }
    }
    // 非 macOS:留 null;Windows 实现在 stacked PR 接力

    let snapshots = list_snapshots(&paths);
    Json(json!({
        "success": true,
        "activeProvider": cfg.active_provider,
        "providerCount": cfg.providers.len(),
        "hasSnapshot": has_snapshot(&paths),
        "snapshotCount": snapshots.len(),
        "desktopStatus": desktop_status,
        "platform": platform_label(),
    }))
    .into_response()
}

fn platform_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        "linux"
    }
}

/// GET `/api/claude-desktop/providers` —— 已添加 providers + 内置 presets。
pub async fn list_providers() -> impl IntoResponse {
    let (_paths, cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let presets = builtin_presets().unwrap_or_default();
    Json(json!({
        "success": true,
        "activeProvider": cfg.active_provider,
        "providers": cfg.providers,
        "presets": presets,
    }))
    .into_response()
}

/// GET `/api/claude-desktop/presets` —— 7 个内置 Anthropic-compat preset。
pub async fn list_presets() -> impl IntoResponse {
    match builtin_presets() {
        Ok(presets) => Json(json!({"success": true, "presets": presets})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST `/api/claude-desktop/providers` —— 添加 provider(body = 完整
/// ClaudeDesktopProvider JSON;id 由 client 生成或留空让 server 兜底)。
pub async fn add_provider(Json(mut p): Json<ClaudeDesktopProvider>) -> impl IntoResponse {
    let (paths, mut cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    if p.id.trim().is_empty() {
        p.id = generate_provider_id();
    }
    if cfg.providers.iter().any(|x| x.id == p.id) {
        return err(StatusCode::CONFLICT, format!("provider id 已存在: {}", p.id))
            .into_response();
    }
    p.sort_index = cfg.providers.len() as i64;
    cfg.providers.push(p);
    if let Err(e) = save_config(&paths, &cfg) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    Json(json!({"success": true})).into_response()
}

fn generate_provider_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("cd_{:x}", nanos)
}

/// PUT `/api/claude-desktop/providers/{id}` —— 完整覆盖更新。
pub async fn update_provider(
    Path(id): Path<String>,
    Json(p): Json<ClaudeDesktopProvider>,
) -> impl IntoResponse {
    let (paths, mut cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let Some(slot) = cfg.providers.iter_mut().find(|x| x.id == id) else {
        return err(StatusCode::NOT_FOUND, format!("provider 未找到: {id}")).into_response();
    };
    let sort_index = slot.sort_index;
    *slot = ClaudeDesktopProvider {
        id, // 路径 id 优先
        sort_index,
        ..p
    };
    if let Err(e) = save_config(&paths, &cfg) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    Json(json!({"success": true})).into_response()
}

/// DELETE `/api/claude-desktop/providers/{id}`
pub async fn delete_provider(Path(id): Path<String>) -> impl IntoResponse {
    let (paths, mut cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let before = cfg.providers.len();
    cfg.providers.retain(|x| x.id != id);
    if cfg.providers.len() == before {
        return err(StatusCode::NOT_FOUND, format!("provider 未找到: {id}")).into_response();
    }
    if cfg.active_provider.as_deref() == Some(id.as_str()) {
        cfg.active_provider = None;
    }
    if let Err(e) = save_config(&paths, &cfg) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    Json(json!({"success": true})).into_response()
}

/// PUT `/api/claude-desktop/providers/{id}/default` —— 设置 active(不自动 apply)。
pub async fn set_default_provider(Path(id): Path<String>) -> impl IntoResponse {
    let (paths, mut cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    if !cfg.providers.iter().any(|x| x.id == id) {
        return err(StatusCode::NOT_FOUND, format!("provider 未找到: {id}")).into_response();
    }
    cfg.active_provider = Some(id);
    if let Err(e) = save_config(&paths, &cfg) {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    Json(json!({"success": true})).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyRequest {
    /// 要应用的 provider id(若为 None 用 active_provider)。
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub expose_all_models: bool,
    /// 覆盖默认 base_url(`http://127.0.0.1:18080`)。绝大多数用户用 None。
    #[serde(default)]
    pub gateway_base_url: Option<String>,
}

/// POST `/api/claude-desktop/apply` —— 写入 Claude Desktop 配置。
pub async fn apply(Json(req): Json<ApplyRequest>) -> impl IntoResponse {
    let (paths, mut cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let provider_id = req
        .provider_id
        .as_ref()
        .or(cfg.active_provider.as_ref())
        .cloned();
    let Some(provider_id) = provider_id else {
        return err(StatusCode::BAD_REQUEST, "未指定 providerId 且无 active provider").into_response();
    };
    let Some(provider) = cfg.providers.iter().find(|x| x.id == provider_id) else {
        return err(
            StatusCode::NOT_FOUND,
            format!("provider 未找到: {provider_id}"),
        )
        .into_response();
    };
    // 对照 cc-desktop-switch v1.0.18+ `backend/main.py:521 desktop_config_target_for_provider`:
    // **所有第三方 provider 都强制走本地代理** —— Claude Desktop 1.7196+ 拒非白名单
    // body.model,只接受 claude-* 路由名;而第三方上游(Kimi `/coding` 只识别
    // kimi-for-coding 等)需要 vendor-specific model id。本地代理在中间做 model
    // name 翻译,两端各自走自己的协议方言。
    //
    // 落地步骤:
    // 1) 启动 cas Claude Desktop 专属 axum 代理(端口 18099,跟 cas Codex 代理 18080
    //    + cc-desktop-switch 18082 错开)。
    // 2) 取/生成 gateway_api_key 持久化到 claude-desktop-config.json(用户重启 cas
    //    后 Claude Desktop 已配置的 plist 仍能复用同一把 key)。
    // 3) 把 active provider 推送给 proxy state(每次 apply 都更新)。
    // 4) plist baseUrl 写 `http://127.0.0.1:18099`,plist apiKey 写 gateway_api_key,
    //    上游真 apiKey **不暴露**给 Claude Desktop 客户端,只 proxy 内部使用。
    let proxy_state = cd_proxy_or_init();
    if cfg.gateway_api_key.trim().is_empty() {
        cfg.gateway_api_key = generate_gateway_api_key();
        if let Err(e) = save_config(&paths, &cfg) {
            return err(StatusCode::INTERNAL_SERVER_ERROR, format!("gateway key 持久化失败: {e}"))
                .into_response();
        }
    }
    let gateway_api_key = cfg.gateway_api_key.clone();
    let provider_clone = provider.clone();
    let proxy_state_for_update = proxy_state.clone();
    let gateway_key_for_update = gateway_api_key.clone();
    tokio::spawn(async move {
        proxy_state_for_update
            .set_active(provider_clone, gateway_key_for_update)
            .await;
    });

    let effective_base = format!("http://{CD_PROXY_BIND}:{CD_PROXY_PORT}");
    let result = apply_provider(
        &paths,
        &ApplyConfig {
            provider,
            all_providers: &cfg.providers,
            gateway_api_key: &gateway_api_key,
            expose_all_models: req.expose_all_models,
            gateway_base_url: Some(&effective_base),
            app_version: APP_VERSION,
        },
    );
    match result {
        Ok(r) => {
            let ApplyResult {
                snapshot_taken_now,
                platform,
            } = r;
            Json(json!({
                "success": true,
                "snapshotTakenNow": snapshot_taken_now,
                "platform": platform,
                "providerId": provider_id,
            }))
            .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ── /api/claude-desktop/proxy/* —— Claude Desktop 转发服务运行时控制 ──
//
// 跟 `/api/proxy/*`(Codex CLI 转发,端口 18080)对称,但底层模型不同:
// - Codex 端走 [`ProxyManager`] 单实例 + 显式 start/stop(端口可断可起)。
// - Claude Desktop 端走 [`CD_PROXY`] OnceLock(端口 18099 起来后一直 listen),
//   start/stop 控制的是 **proxy state 里的 active provider**:有 active 才能
//   做 model 翻译 + 上游转发;清空 active 后请求返 503 no_active_provider。

/// GET `/api/claude-desktop/proxy/status` —— Claude Desktop 转发服务状态 + 统计
pub async fn proxy_status() -> impl IntoResponse {
    let mut active_provider_id: Option<String> = None;
    let mut gateway_key_configured = false;
    if let Some(state) = CD_PROXY.get() {
        let snap = state.inner.read().await;
        active_provider_id = snap.provider.as_ref().map(|p| p.id.clone());
        gateway_key_configured = !snap.gateway_api_key.is_empty();
    }
    let listening = CD_PROXY.get().is_some();
    let running = listening && active_provider_id.is_some();
    Json(json!({
        "running": running,
        "listening": listening,
        "port": CD_PROXY_PORT,
        "bind": CD_PROXY_BIND,
        "activeProviderId": active_provider_id,
        "gatewayKeyConfigured": gateway_key_configured,
        "stats": claude_desktop_proxy_telemetry().stats.snapshot(),
    }))
    .into_response()
}

/// POST `/api/claude-desktop/proxy/start` —— 软启动:确保 18099 init + 把 cas
/// 内部 config 里的 active provider 推给 proxy state。等价于在 Claude Desktop
/// tab 上点 apply 但**不**重写 Claude Desktop 配置文件 / plist(纯 runtime)。
pub async fn proxy_start() -> impl IntoResponse {
    let (_paths, cfg) = match load_or_err() {
        Ok(v) => v,
        Err(resp) => return resp.into_response(),
    };
    let Some(active_id) = cfg.active_provider.as_ref() else {
        return err(
            StatusCode::BAD_REQUEST,
            "无 active provider —— 请先在 Claude Desktop 页添加 provider 并设为默认",
        )
        .into_response();
    };
    let Some(provider) = cfg.providers.iter().find(|p| &p.id == active_id) else {
        return err(
            StatusCode::NOT_FOUND,
            format!("active provider {active_id} 不在 providers 列表"),
        )
        .into_response();
    };
    if cfg.gateway_api_key.trim().is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "gateway api key 未生成 —— 请先在 Claude Desktop 页点击 apply",
        )
        .into_response();
    }
    let state = cd_proxy_or_init();
    state
        .set_active(provider.clone(), cfg.gateway_api_key.clone())
        .await;
    claude_desktop_proxy_telemetry().logs.add(
        "INFO",
        format!(
            "forwarding started :{CD_PROXY_PORT} (provider={})",
            provider.id
        ),
    );
    Json(json!({
        "success": true,
        "running": true,
        "port": CD_PROXY_PORT,
        "activeProviderId": active_id,
    }))
    .into_response()
}

/// POST `/api/claude-desktop/proxy/stop` —— 软停止:清空 proxy state 里的 active
/// provider。18099 端口仍 listen(OnceLock 启动后无法 graceful shutdown),
/// 但所有请求返 503 no_active_provider。
pub async fn proxy_stop() -> impl IntoResponse {
    if let Some(state) = CD_PROXY.get() {
        state.clear_active().await;
        claude_desktop_proxy_telemetry()
            .logs
            .add("INFO", "forwarding stopped (active provider cleared)");
    }
    Json(json!({"success": true, "running": false})).into_response()
}

/// GET `/api/claude-desktop/proxy/logs`
pub async fn proxy_logs() -> impl IntoResponse {
    Json(json!({"logs": claude_desktop_proxy_telemetry().logs.get_all()})).into_response()
}

/// POST `/api/claude-desktop/proxy/logs/clear`
pub async fn proxy_logs_clear() -> impl IntoResponse {
    claude_desktop_proxy_telemetry().logs.clear();
    Json(json!({"success": true})).into_response()
}

/// POST `/api/claude-desktop/proxy/logs/open-dir`
pub async fn proxy_logs_open_dir() -> impl IntoResponse {
    let Some(path) = claude_desktop_proxy_log_dir() else {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cannot locate log directory",
        )
        .into_response();
    };
    if let Err(e) = fs::create_dir_all(&path) {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("create log directory failed: {e}"),
        )
        .into_response();
    }
    match open_directory(&path) {
        Ok(_) => Json(json!({"success": true, "path": path.to_string_lossy()})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamProbeRequest {
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_auth")]
    pub auth_scheme: String,
    #[serde(default = "default_format")]
    pub api_format: String,
}

fn default_auth() -> String {
    "bearer".to_string()
}
fn default_format() -> String {
    "anthropic".to_string()
}

/// 推导 `/v1/models` 探测 URL —— Anthropic-compat 第三方端点(`.../anthropic`)
/// 通常同时暴露 `.../v1/models`(OpenAI 兼容)。脱掉常见 ant/messages/v1 后缀
/// 后再拼 `/v1/models`,这是 cc-desktop-switch 同款 heuristic(`backend/upstream.py`)。
fn derive_models_url(base_url: &str) -> String {
    let clean = base_url.trim().trim_end_matches('/');
    let root = clean
        .strip_suffix("/v1/messages")
        .or_else(|| clean.strip_suffix("/messages"))
        .or_else(|| clean.strip_suffix("/anthropic"))
        .or_else(|| clean.strip_suffix("/v1"))
        .map(|r| r.trim_end_matches('/'))
        .unwrap_or(clean);
    format!("{}/v1/models", root)
}

fn build_auth_headers(api_key: &str, auth_scheme: &str) -> reqwest::header::HeaderMap {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT};
    let mut h = HeaderMap::new();
    h.insert(ACCEPT, HeaderValue::from_static("application/json"));
    if !api_key.is_empty() {
        match auth_scheme {
            "x-api-key" => {
                if let Ok(v) = HeaderValue::from_str(api_key) {
                    h.insert(HeaderName::from_static("x-api-key"), v);
                }
            }
            "none" => {}
            _ => {
                if let Ok(v) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
                    h.insert(reqwest::header::AUTHORIZATION, v);
                }
            }
        }
    }
    h
}

/// POST `/api/claude-desktop/test-baseurl` —— 探活上游端点。仅返 latency + status。
pub async fn test_baseurl(Json(req): Json<UpstreamProbeRequest>) -> impl IntoResponse {
    let url = derive_models_url(&req.base_url);
    let headers = build_auth_headers(&req.api_key, &req.auth_scheme);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let started = std::time::Instant::now();
    let resp = client.get(&url).headers(headers).send().await;
    let elapsed = started.elapsed().as_millis() as u64;
    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            let ok = r.status().is_success() || r.status().as_u16() == 401 || r.status().as_u16() == 403;
            Json(json!({
                "success": true,
                "ok": ok,
                "status": status,
                "latencyMs": elapsed,
                "url": url,
                "message": if ok { "可达" } else { "上游异常" },
            }))
            .into_response()
        }
        Err(e) => Json(json!({
            "success": true,
            "ok": false,
            "status": 0,
            "latencyMs": elapsed,
            "url": url,
            "message": e.to_string(),
        }))
        .into_response(),
    }
}

/// POST `/api/claude-desktop/fetch-models` —— 拉远端 model 列表。
/// 解析 OpenAI `{data: [{id}]}` 或 Anthropic `{data: [{id}]}` / `{models: [...]}` 三种格式。
pub async fn fetch_models(Json(req): Json<UpstreamProbeRequest>) -> impl IntoResponse {
    let url = derive_models_url(&req.base_url);
    let headers = build_auth_headers(&req.api_key, &req.auth_scheme);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let resp = client.get(&url).headers(headers).send().await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            return Json(json!({
                "success": false,
                "message": format!("请求失败: {e}"),
                "url": url,
            }))
            .into_response();
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Json(json!({
            "success": false,
            "status": status.as_u16(),
            "url": url,
            "message": format!("HTTP {} · {}", status.as_u16(), body.chars().take(200).collect::<String>()),
        }))
        .into_response();
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return Json(json!({
                "success": false,
                "message": format!("响应解析失败: {e}"),
                "url": url,
            }))
            .into_response();
        }
    };
    let mut models: Vec<String> = Vec::new();
    if let Some(arr) = body.get("data").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    }
    if models.is_empty() {
        if let Some(arr) = body.get("models").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(s) = item.as_str() {
                    models.push(s.to_string());
                } else if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    models.push(id.to_string());
                } else if let Some(id) = item.get("name").and_then(|v| v.as_str()) {
                    models.push(id.to_string());
                }
            }
        }
    }
    Json(json!({
        "success": true,
        "url": url,
        "models": models,
    }))
    .into_response()
}

/// POST `/api/claude-desktop/clear` —— 从 snapshot 还原 Claude Desktop 原始配置。
pub async fn clear() -> impl IntoResponse {
    let paths = match paths_or_err() {
        Ok(p) => p,
        Err(resp) => return resp.into_response(),
    };
    match restore_state(&paths) {
        Ok(restored) => Json(json!({"success": true, "restored": restored})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
