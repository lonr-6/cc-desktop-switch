//! 跨域 helper + status/version + instance handler 合并.
//!
//! 包含被多个子模块引用的工具函数(`err` / `open_directory` / `current_epoch_secs`
//! 等)、顶层 status / version / instance handler,以及 lib internal typecheck shim.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use codex_app_transfer_codex_integration::{has_snapshot, CodexPaths};
use codex_app_transfer_registry::RawConfig;
use serde_json::{json, Value};

use super::super::registry_io::{load as load_registry, public_provider};
use super::super::state::AdminState;

pub(super) const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(super) fn err(status: StatusCode, msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"success": false, "message": msg.into()})),
    )
}

pub(super) fn open_directory(path: &PathBuf) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(path);
        command
    } else if cfg!(target_os = "windows") {
        let mut command = Command::new("explorer");
        command.arg(path);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open log directory: {e}"))
}

pub(super) fn active_provider_name(config: &Value) -> String {
    let active_id = config.get("activeProvider").and_then(|v| v.as_str());
    config
        .get("providers")
        .and_then(|v| v.as_array())
        .and_then(|providers| {
            if let Some(active_id) = active_id {
                providers
                    .iter()
                    .find(|provider| provider.get("id").and_then(|v| v.as_str()) == Some(active_id))
            } else {
                providers.first()
            }
        })
        .and_then(|provider| provider.get("name").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_owned()
}

pub(super) fn current_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn read_setting_bool(cfg: &RawConfig, key: &str, default: bool) -> bool {
    cfg.get("settings")
        .and_then(|settings| settings.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

pub(super) fn generate_gateway_key_value() -> String {
    let mut buf = [0u8; 32];
    let _ = getrandom::getrandom(&mut buf);
    format!("cas_{}", URL_SAFE_NO_PAD.encode(buf))
}

pub(super) fn random_hex(bytes_len: usize) -> String {
    let mut buf = vec![0u8; bytes_len];
    let _ = getrandom::getrandom(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

// ── /api/instance-info & /api/instance-show-window ───────────────────

pub async fn instance_info() -> Json<Value> {
    Json(json!({
        "app": "codex-app-transfer",
        "version": APP_VERSION,
        "pid": std::process::id(),
    }))
}

pub async fn instance_show_window() -> Json<Value> {
    // 由 main.rs 通过 channel/event 拉前主窗口;这里至少回 ack
    Json(json!({"success": true}))
}

// ── /api/status ──────────────────────────────────────────────────────

pub async fn status(State(state): State<AdminState>) -> impl IntoResponse {
    let cfg = match load_registry() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let providers_count = cfg
        .get("providers")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let active = super::providers::active_provider(&cfg).map(|p| public_provider(&p));
    let active_id = cfg
        .get("activeProvider")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let proxy_port = super::proxy::read_proxy_port(&cfg);
    let proxy_status = state.proxy_manager.status();
    let codex_paths = CodexPaths::from_home_env().ok();
    let codex_configured = codex_paths.as_ref().map(has_snapshot).unwrap_or(false);
    let actual_base_url = codex_paths
        .as_ref()
        .and_then(|paths| super::desktop::read_codex_toml_root_string(paths, "openai_base_url"));
    let actual_api_key_present = codex_paths
        .as_ref()
        .map(super::desktop::codex_openai_api_key_present)
        .unwrap_or(false);
    let desktop_target = super::desktop::desktop_target_for_active_provider(&cfg);
    let desktop_health = super::desktop::desktop_health(
        codex_paths.as_ref(),
        codex_configured,
        actual_base_url.as_deref(),
        actual_api_key_present,
        desktop_target.as_ref(),
    );

    Json(json!({
        "desktopConfigured": codex_configured,
        "proxyRunning": proxy_status.running,
        "proxyPort": proxy_port,
        "desktopMode": desktop_target.as_ref().map(|target| target.mode).unwrap_or("unconfigured"),
        "desktopRequiresProxy": desktop_target
            .as_ref()
            .map(|target| target.requires_proxy)
            .unwrap_or(false),
        "activeProvider": active,
        "activeProviderId": active_id,
        "providerCount": providers_count,
        "desktopHealth": desktop_health,
        "exposeAllProviderModels": false,
    }))
    .into_response()
}

// ── /api/version ─────────────────────────────────────────────────────

pub async fn version() -> Json<Value> {
    Json(json!({"version": APP_VERSION}))
}

#[allow(dead_code)]
pub fn _state_typecheck(_s: Arc<AdminState>) -> bool {
    true
}

/// 测试用 helper:把 HOME / USERPROFILE 切到 isolated tempdir,跑完函数后
/// 还原 + 清理.全局共享同一把 `Mutex`,确保多个 test 不会并发改 env。
/// 跨子模块的测试都通过 `super::common::test_support::with_isolated_home`
/// 复用同一份实现 + 同一把锁(原 `_legacy.rs` 单 mod 时只有一份)。
#[cfg(test)]
pub(in crate::admin) mod test_support {
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    use super::random_hex;

    pub(in crate::admin) fn with_isolated_home<T>(f: impl FnOnce(&Path) -> T) -> T {
        static HOME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        // 清掉之前 panic 留下的 poison —— 我们的 EnvGuard 已经把 env 还原干净
        let mutex = HOME_LOCK.get_or_init(|| Mutex::new(()));
        let _guard = match mutex.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };

        struct EnvGuard {
            home: Option<OsString>,
            userprofile: Option<OsString>,
            root: PathBuf,
        }

        impl Drop for EnvGuard {
            fn drop(&mut self) {
                match &self.home {
                    Some(value) => std::env::set_var("HOME", value),
                    None => std::env::remove_var("HOME"),
                }
                match &self.userprofile {
                    Some(value) => std::env::set_var("USERPROFILE", value),
                    None => std::env::remove_var("USERPROFILE"),
                }
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        let root = std::env::temp_dir().join(format!(
            "cas-admin-test-{}-{}",
            std::process::id(),
            random_hex(6)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let env_guard = EnvGuard {
            home: std::env::var_os("HOME"),
            userprofile: std::env::var_os("USERPROFILE"),
            root: root.clone(),
        };
        std::env::set_var("HOME", &root);
        std::env::remove_var("USERPROFILE");

        let result = f(&root);
        drop(env_guard);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_endpoint_matches_legacy_shape() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let Json(payload) = version().await;
            assert_eq!(payload, json!({"version": APP_VERSION}));
        });
    }
}
