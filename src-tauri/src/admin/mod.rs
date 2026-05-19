//! Admin server —— 把 v1.4 的 `/api/*` 路由 1:1 翻成 axum + 静态 frontend/.
//!
//! 设计:
//! - 不绑端口:通过 Tauri 的自定义 URI scheme(`cas://localhost/`)将 webview
//!   请求路由进 axum router(`tower::ServiceExt::oneshot`),全程同进程,无 TCP
//!   往返
//! - **frontend/ 整目录通过 `include_dir!` 编进二进制**,首次加载零 IO
//! - `/api/*` 数据 shape **严格对齐 v1.4**(frontend/js/api.js 不需要任何改动)
//!
//! 详见 docs/refactor/migration.md Stage 6 修订日志。

pub mod handlers;
pub mod registry_io;
pub mod state;
pub mod static_files;

use axum::{
    routing::{get, post, put},
    Router,
};

pub use state::AdminState;

pub fn build_app_router(state: AdminState) -> Router {
    Router::new()
        // 单实例握手
        .route("/api/instance-info", get(handlers::common::instance_info))
        .route(
            "/api/instance-show-window",
            post(handlers::common::instance_show_window),
        )
        // 总状态
        .route("/api/status", get(handlers::common::status))
        // Providers
        .route(
            "/api/providers",
            get(handlers::providers::crud::list_providers)
                .post(handlers::providers::crud::add_provider),
        )
        .route(
            "/api/providers/{id}",
            put(handlers::providers::crud::update_provider)
                .delete(handlers::providers::crud::delete_provider),
        )
        .route(
            "/api/providers/reorder",
            put(handlers::providers::crud::reorder_providers),
        )
        .route(
            "/api/providers/{id}/default",
            put(handlers::providers::crud::set_default_provider),
        )
        .route(
            "/api/providers/{id}/activate",
            post(handlers::providers::crud::activate_provider),
        )
        .route(
            "/api/providers/{id}/secret",
            get(handlers::providers::crud::get_secret),
        )
        .route(
            "/api/providers/{id}/draft",
            post(handlers::providers::crud::save_draft),
        )
        .route(
            "/api/providers/{id}/test",
            post(handlers::providers::test::test_provider),
        )
        .route(
            "/api/providers/{id}/usage",
            post(handlers::providers::balance::query_provider_usage),
        )
        .route(
            "/api/providers/{id}/models",
            put(handlers::providers::crud::update_models),
        )
        .route(
            "/api/providers/{id}/models/available",
            get(handlers::providers::models::fetch_provider_models),
        )
        .route(
            "/api/providers/{id}/models/autofill",
            post(handlers::providers::models::autofill_provider_models),
        )
        .route(
            "/api/providers/compatibility",
            get(handlers::providers::test::provider_compatibility),
        )
        .route(
            "/api/providers/test",
            post(handlers::providers::test::test_provider_payload),
        )
        .route(
            "/api/providers/models/available",
            post(handlers::providers::models::fetch_provider_models_payload),
        )
        // Presets
        .route(
            "/api/presets",
            get(handlers::providers::presets::list_presets),
        )
        // Desktop / Codex CLI
        .route(
            "/api/desktop/status",
            get(handlers::desktop::desktop_status),
        )
        .route(
            "/api/desktop/configure",
            post(handlers::desktop::desktop_configure),
        )
        .route("/api/desktop/clear", post(handlers::desktop::desktop_clear))
        .route(
            "/api/desktop/snapshots",
            get(handlers::desktop::desktop_snapshots),
        )
        .route(
            "/api/desktop/restore",
            post(handlers::desktop::desktop_restore),
        )
        .route(
            "/api/desktop/restart-codex-app",
            post(handlers::desktop::restart_codex_app),
        )
        .route(
            "/api/desktop/snapshot-status",
            get(handlers::desktop::desktop_snapshot_status),
        )
        // Claude Desktop(独立 provider 列表;直连 Anthropic-compat 上游,不走 cas proxy)
        .route(
            "/api/claude-desktop/status",
            get(handlers::claude_desktop::status),
        )
        .route(
            "/api/claude-desktop/presets",
            get(handlers::claude_desktop::list_presets),
        )
        .route(
            "/api/claude-desktop/providers",
            get(handlers::claude_desktop::list_providers)
                .post(handlers::claude_desktop::add_provider),
        )
        .route(
            "/api/claude-desktop/providers/{id}",
            put(handlers::claude_desktop::update_provider)
                .delete(handlers::claude_desktop::delete_provider),
        )
        .route(
            "/api/claude-desktop/providers/{id}/default",
            put(handlers::claude_desktop::set_default_provider),
        )
        .route(
            "/api/claude-desktop/apply",
            post(handlers::claude_desktop::apply),
        )
        .route(
            "/api/claude-desktop/clear",
            post(handlers::claude_desktop::clear),
        )
        .route(
            "/api/claude-desktop/test-baseurl",
            post(handlers::claude_desktop::test_baseurl),
        )
        .route(
            "/api/claude-desktop/fetch-models",
            post(handlers::claude_desktop::fetch_models),
        )
        // Claude Desktop 转发服务运行时控制(端口 18099,跟 Codex 18080 分桶)
        .route(
            "/api/claude-desktop/proxy/status",
            get(handlers::claude_desktop::proxy_status),
        )
        .route(
            "/api/claude-desktop/proxy/start",
            post(handlers::claude_desktop::proxy_start),
        )
        .route(
            "/api/claude-desktop/proxy/stop",
            post(handlers::claude_desktop::proxy_stop),
        )
        .route(
            "/api/claude-desktop/proxy/logs",
            get(handlers::claude_desktop::proxy_logs),
        )
        .route(
            "/api/claude-desktop/proxy/logs/clear",
            post(handlers::claude_desktop::proxy_logs_clear),
        )
        .route(
            "/api/claude-desktop/proxy/logs/open-dir",
            post(handlers::claude_desktop::proxy_logs_open_dir),
        )
        // Codex CLI 转发服务运行时控制(端口 18080)
        // Proxy lifecycle
        .route("/api/version", get(handlers::common::version))
        .route("/api/proxy/start", post(handlers::proxy::start_proxy))
        .route("/api/proxy/stop", post(handlers::proxy::stop_proxy))
        .route("/api/proxy/status", get(handlers::proxy::proxy_status))
        .route("/api/proxy/logs", get(handlers::proxy::proxy_logs))
        .route(
            "/api/proxy/logs/clear",
            post(handlers::proxy::proxy_logs_clear),
        )
        .route(
            "/api/proxy/logs/open-dir",
            post(handlers::proxy::proxy_logs_open_dir),
        )
        .route("/api/sessions/clear", post(handlers::proxy::sessions_clear))
        // Settings
        .route(
            "/api/settings",
            get(handlers::settings::get_settings).put(handlers::settings::save_settings),
        )
        // Update
        .route("/api/update/check", get(handlers::update::update_check))
        .route(
            "/api/update/install",
            post(handlers::update::update_install),
        )
        // Config
        .route(
            "/api/config/backup",
            post(handlers::settings::create_backup),
        )
        .route("/api/config/backups", get(handlers::settings::list_backups))
        .route("/api/config/export", get(handlers::settings::export_config))
        .route(
            "/api/config/import",
            post(handlers::settings::import_config),
        )
        // Feedback
        .route("/api/feedback", post(handlers::feedback::submit_feedback))
        // Gemini CLI OAuth (login / status / logout)
        .merge(handlers::gemini_oauth::routes())
        // Plugin Unlock (CDP injection for Codex Desktop)
        .merge(handlers::plugin_unlock::routes())
        // Antigravity OAuth (login / status / logout / cancel)
        .merge(handlers::antigravity_oauth::routes())
        // 静态文件兜底
        .fallback(static_files::serve_static)
        .with_state(state)
}
