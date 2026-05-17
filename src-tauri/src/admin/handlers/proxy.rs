//! `/api/proxy/*` —— 代理生命周期 + 网关密钥 + 端口.

use std::fs;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use codex_app_transfer_proxy::{proxy_log_dir, proxy_telemetry};
use codex_app_transfer_registry::RawConfig;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::proxy_runner::ProxyManager;

use super::super::registry_io::load as load_registry;
use super::super::state::AdminState;
use super::common::{err, generate_gateway_key_value, open_directory};

pub(super) fn read_proxy_port(cfg: &RawConfig) -> u16 {
    cfg.get("settings")
        .and_then(|s| s.get("proxyPort"))
        .and_then(|v| v.as_u64())
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(18080)
}

pub(super) fn read_gateway_key(cfg: &RawConfig) -> String {
    cfg.get("gatewayApiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

pub(super) fn ensure_gateway_key(cfg: &mut RawConfig) -> String {
    let existing = read_gateway_key(cfg);
    if !existing.is_empty() {
        return existing;
    }
    let gateway_key = generate_gateway_key_value();
    cfg.as_object_mut()
        .unwrap()
        .insert("gatewayApiKey".into(), Value::String(gateway_key.clone()));
    gateway_key
}

pub(super) async fn start_proxy_if_needed(
    manager: &ProxyManager,
    port: u16,
) -> Result<bool, String> {
    if manager.status().running {
        manager.stop_silent();
    }
    manager.start(port).await.map(|_| true)
}

// ── /api/proxy/* ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct StartProxyInput {
    pub port: Option<u16>,
}

pub async fn start_proxy(
    State(state): State<AdminState>,
    body: Option<Json<StartProxyInput>>,
) -> impl IntoResponse {
    let port = body
        .and_then(|b| b.0.port)
        .or_else(|| load_registry().ok().map(|cfg| read_proxy_port(&cfg)))
        .unwrap_or(18080);
    match state.proxy_manager.start(port).await {
        Ok(s) => {
            let actual_port = s
                .addr
                .as_ref()
                .and_then(|a| a.split(':').last().and_then(|p| p.parse::<u16>().ok()))
                .unwrap_or(port);
            proxy_telemetry()
                .logs
                .add("INFO", format!("forwarding started :{actual_port}"));
            Json(json!({
                "success": true,
                "running": s.running,
                "port": actual_port,
            }))
            .into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn stop_proxy(State(state): State<AdminState>) -> impl IntoResponse {
    state.proxy_manager.stop_silent();
    proxy_telemetry()
        .logs
        .add("INFO", "forwarding stopped".to_owned());
    Json(json!({"success": true, "running": false})).into_response()
}

pub async fn proxy_status(State(state): State<AdminState>) -> impl IntoResponse {
    let s = state.proxy_manager.status();
    let cfg = load_registry().unwrap_or_else(|_| json!({}));
    let port = s
        .addr
        .as_ref()
        .and_then(|a| a.split(':').last().and_then(|p| p.parse::<u16>().ok()))
        .unwrap_or_else(|| read_proxy_port(&cfg));
    Json(json!({
        "running": s.running,
        "port": port,
        "stats": proxy_telemetry().stats.snapshot(),
    }))
    .into_response()
}

pub async fn proxy_logs() -> impl IntoResponse {
    Json(json!({"logs": proxy_telemetry().logs.get_all()})).into_response()
}

pub async fn proxy_logs_clear() -> impl IntoResponse {
    proxy_telemetry().logs.clear();
    Json(json!({"success": true})).into_response()
}

/// `POST /api/sessions/clear` —— 清除 Responses session cache 持久化历史
/// (`~/.codex-app-transfer/sessions.db` 全表 + 内存 hot cache)。
///
/// 隐私控制点:用户主动清除"应用记得的对话历史"。生产场景:换设备前 / 用户
/// 切账号 / debug 数据问题。返回清掉的 L2 行数。**清除后正在进行的 Codex CLI
/// 会话续轮会触发 cache miss → PR 1 标准 OpenAI 400(`previous_response_not_found`)
/// → Codex CLI fail-fast,用户重发对话即可。**
pub async fn sessions_clear() -> impl IntoResponse {
    let cache = codex_app_transfer_adapters::responses::session::global_response_session_cache();
    match cache.clear_all_persisted() {
        Ok(rows) => {
            proxy_telemetry().logs.add(
                "INFO",
                format!("sessions.db cleared by admin: {rows} rows removed"),
            );
            Json(json!({"success": true, "rowsRemoved": rows})).into_response()
        }
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("clear sessions.db failed: {e}"),
        )
        .into_response(),
    }
}

pub async fn proxy_logs_open_dir() -> impl IntoResponse {
    let Some(path) = proxy_log_dir() else {
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
