//! Top-level apply / restore —— 对称
//! [`codex_app_transfer_codex_integration::apply`] 设计。
//!
//! 调用顺序:
//! 1. [`apply_provider`]:首次 apply 前自动 snapshot,然后跨平台 dispatch
//!    到 [`crate::macos::apply`](macOS) / `registry::apply`(Windows stacked PR)
//! 2. [`restore_state`]:从 snapshot 还原原 plist + config.json,再走
//!    [`crate::macos::clear`] / `registry::clear` 兜底清理 marker

use crate::paths::ClaudeDesktopPaths;
use crate::schema::Provider;
use crate::snapshot::{
    current_session_dir, drop_active_snapshot, has_snapshot, restore_from_snapshot_dir,
    snapshot_state, SnapshotManifest,
};
use crate::ClaudeDesktopError;

#[derive(Debug, Clone)]
pub struct ApplyConfig<'a> {
    /// 当前激活的 Claude Desktop provider。
    pub provider: &'a Provider,
    /// 完整 provider 列表(`expose_all_models = true` 时用)。
    pub all_providers: &'a [Provider],
    /// 写入 Claude Desktop policy 的 gateway API key(用户填的 provider API key)。
    pub gateway_api_key: &'a str,
    /// 是否在 inferenceModels 字段写所有 provider 的模型(否则只本 provider)。
    pub expose_all_models: bool,
    /// 自定义 `inferenceGatewayBaseUrl`,默认 `http://127.0.0.1:18080`。
    /// 通常给空字符串走 default(对齐 cc-desktop-switch 行为)。
    pub gateway_base_url: Option<&'a str>,
    /// 应用版本(写入 snapshot manifest 便于诊断)。
    pub app_version: &'a str,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub snapshot_taken_now: bool,
    pub platform: String,
}

/// 写入 Claude Desktop 配置 —— 跨平台 dispatch。
pub fn apply_provider(
    paths: &ClaudeDesktopPaths,
    cfg: &ApplyConfig<'_>,
) -> Result<ApplyResult, ClaudeDesktopError> {
    // 1. snapshot(同 session 幂等)
    let snapshot_taken_now = !has_snapshot(paths);
    let _: SnapshotManifest = snapshot_state(paths, cfg.app_version, Some(&cfg.provider.name))?;

    // 2. 跨平台 dispatch
    let platform = apply_platform(paths, cfg)?;

    Ok(ApplyResult {
        snapshot_taken_now,
        platform,
    })
}

#[cfg(target_os = "macos")]
fn apply_platform(
    _paths: &ClaudeDesktopPaths,
    cfg: &ApplyConfig<'_>,
) -> Result<String, ClaudeDesktopError> {
    let input = crate::macos::ApplyInput {
        provider: cfg.provider,
        all_providers: cfg.all_providers,
        gateway_api_key: cfg.gateway_api_key,
        expose_all_models: cfg.expose_all_models,
        gateway_base_url: cfg.gateway_base_url,
    };
    crate::macos::apply(&input)?;
    Ok("mac".to_owned())
}

#[cfg(target_os = "windows")]
fn apply_platform(
    _paths: &ClaudeDesktopPaths,
    _cfg: &ApplyConfig<'_>,
) -> Result<String, ClaudeDesktopError> {
    // stacked PR:1:1 转写 cc-desktop-switch/backend/registry.py:124-890
    // Windows Registry HKCU + UAC 提权 + PowerShell 写入。
    Err(ClaudeDesktopError::SchemaCorrupt(
        "Windows Registry 写入待 stacked PR 实施(对应 backend/registry.py:124-890)".to_owned(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn apply_platform(
    _paths: &ClaudeDesktopPaths,
    _cfg: &ApplyConfig<'_>,
) -> Result<String, ClaudeDesktopError> {
    Err(ClaudeDesktopError::SchemaCorrupt(
        "Claude Desktop 没有 Linux GUI 版本，无需配置".to_owned(),
    ))
}

/// 还原 —— 优先从 snapshot 复制回原文件,如果没 snapshot 走 fallback clear
/// (只删 managed 字段)。
pub fn restore_state(paths: &ClaudeDesktopPaths) -> Result<bool, ClaudeDesktopError> {
    let session_dir = current_session_dir(paths);
    // 优先 active snapshot
    if session_dir.exists() && session_dir.join("manifest.json").exists() {
        restore_from_snapshot_dir(paths, &session_dir)?;
        drop_active_snapshot(paths)?;
        return Ok(true);
    }
    // fallback:无 snapshot,走平台 clear(只删 managed 字段)
    clear_platform()?;
    Ok(false)
}

#[cfg(target_os = "macos")]
fn clear_platform() -> Result<(), ClaudeDesktopError> {
    crate::macos::clear()
}

#[cfg(target_os = "windows")]
fn clear_platform() -> Result<(), ClaudeDesktopError> {
    Err(ClaudeDesktopError::SchemaCorrupt(
        "Windows Registry 清理待 stacked PR 实施".to_owned(),
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn clear_platform() -> Result<(), ClaudeDesktopError> {
    Ok(())
}
