//! `/api/desktop/plugin-unlock/*` — Codex Desktop Plugins 解锁 HTTP API.
//!
//! 前端通过这组 API 控制解锁服务:
//! - GET  /api/desktop/plugin-unlock/status   → 查询当前状态
//! - POST /api/desktop/plugin-unlock/start    → 启动守护循环
//! - POST /api/desktop/plugin-unlock/stop     → 停止守护循环
//! - POST /api/desktop/plugin-unlock/reinject → 手动触发重新注入

use std::sync::Arc;

use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};

use crate::codex_plugin_unlocker::{PluginUnlockService, UnlockStatus};

use super::super::state::AdminState;
use super::common::err;

/// 解锁服务实例（全局单例，通过 OnceCell/Lazy 初始化）
use tokio::sync::OnceCell;

static UNLOCK_SERVICE: OnceCell<Arc<PluginUnlockService>> = OnceCell::const_new();

async fn get_service() -> Arc<PluginUnlockService> {
    UNLOCK_SERVICE
        .get_or_init(|| async { Arc::new(PluginUnlockService::default_new()) })
        .await
        .clone()
}

// ── HTTP Handlers ──

/// GET /api/desktop/plugin-unlock/status
pub async fn status_handler() -> impl IntoResponse {
    let service = get_service().await;
    let status = service.status().await;

    let (code, message) = match &status {
        UnlockStatus::Disconnected => ("disconnected", "Codex Desktop 未运行或无调试端口"),
        UnlockStatus::Connecting => ("connecting", "正在连接 CDP..."),
        UnlockStatus::Connected => ("connected", "已连接，等待注入"),
        UnlockStatus::Injected => ("injected", "✅ Plugins 已解锁"),
        UnlockStatus::Failed { error } => ("failed", error.as_str()),
    };

    Json(json!({
        "success": true,
        "status": code,
        "message": message,
        "detail": status
    }))
}

/// POST /api/desktop/plugin-unlock/start
pub async fn start_handler() -> impl IntoResponse {
    let service = get_service().await;

    // 检查是否已经在运行（通过状态判断）
    match service.status().await {
        UnlockStatus::Injected | UnlockStatus::Connected | UnlockStatus::Connecting => {
            return Json(json!({
                "success": true,
                "message": "解锁服务已在运行"
            }));
        }
        _ => {}
    }

    service.start();

    Json(json!({
        "success": true,
        "message": "解锁服务已启动"
    }))
}

/// POST /api/desktop/plugin-unlock/stop
pub async fn stop_handler() -> impl IntoResponse {
    let service = get_service().await;
    service.stop().await;

    Json(json!({
        "success": true,
        "message": "解锁服务已停止"
    }))
}

/// POST /api/desktop/plugin-unlock/reinject
pub async fn reinject_handler() -> impl IntoResponse {
    let service = get_service().await;
    service.reinject().await;

    Json(json!({
        "success": true,
        "message": "已请求重新注入"
    }))
}

/// 组装路由
pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/api/desktop/plugin-unlock/status", get(status_handler))
        .route("/api/desktop/plugin-unlock/start", post(start_handler))
        .route("/api/desktop/plugin-unlock/stop", post(stop_handler))
        .route(
            "/api/desktop/plugin-unlock/reinject",
            post(reinject_handler),
        )
}
