//! Claude Desktop 配置 snapshot —— 简化版(对比
//! [`codex_app_transfer_codex_integration::snapshot`])。
//!
//! 设计原则:
//! - **单 active snapshot**:首次 apply 前自动 snapshot 一份,记 plist +
//!   config.json 的"我们改动之前"原始内容
//! - **同 session 不重复**:同进程多次 apply 不会覆盖最初备份
//! - **跨 session 隔离**:发现旧 session active snapshot 时挪到 recovery
//!   目录(对齐 codex_integration 设计)
//! - macOS 用,Windows Registry 端用 export `.reg` 文件做对应 snapshot
//!   (Windows 实现在 stacked PR 接力)
//!
//! manifest 字段对齐 codex_integration `SnapshotManifest`,便于复用前端 UI。

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::paths::ClaudeDesktopPaths;
use crate::ClaudeDesktopError;

pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

static CURRENT_SESSION_ID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub snapshot_id: String,
    #[serde(default)]
    pub session_id: String,
    pub snapshot_at: String,
    pub plist_existed: bool,
    pub config_json_existed: bool,
    pub app_version: String,
    #[serde(default)]
    pub provider_name: Option<String>,
}

fn default_schema_version() -> u32 {
    SNAPSHOT_SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: String,
    pub kind: String,
    pub snapshot_at: String,
    pub plist_existed: bool,
    pub config_json_existed: bool,
    pub app_version: String,
    pub provider_name: Option<String>,
    pub current_session: bool,
}

fn current_session_id() -> String {
    CURRENT_SESSION_ID
        .get_or_init(|| {
            let timestamp = Local::now().format("%Y%m%dT%H%M%S%3f").to_string();
            let pid = std::process::id();
            format!("{timestamp}-p{pid}")
        })
        .clone()
}

pub fn current_session_dir(paths: &ClaudeDesktopPaths) -> PathBuf {
    paths.active_snapshots_dir.join(current_session_id())
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.json")
}

fn plist_backup(dir: &Path) -> PathBuf {
    dir.join("com.anthropic.claudefordesktop.plist")
}

fn config_json_backup(dir: &Path) -> PathBuf {
    dir.join("claude_desktop_config.json")
}

/// 当前 session 是否有 active snapshot。
pub fn has_snapshot(paths: &ClaudeDesktopPaths) -> bool {
    manifest_path(&current_session_dir(paths)).exists()
}

/// 把旧 session 的 active snapshot 挪到 recovery,本 session 起新 snapshot。
fn rotate_stale_active_snapshots(paths: &ClaudeDesktopPaths) -> Result<(), ClaudeDesktopError> {
    if !paths.active_snapshots_dir.exists() {
        return Ok(());
    }
    let current = current_session_id();
    let entries = match std::fs::read_dir(&paths.active_snapshots_dir) {
        Ok(it) => it,
        Err(_) => return Ok(()),
    };
    std::fs::create_dir_all(&paths.recovery_snapshots_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if name_str == current {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let target = paths.recovery_snapshots_dir.join(&name_str);
        let _ = std::fs::rename(&path, &target);
    }
    Ok(())
}

/// 在 apply 之前调一次。同 session 已有 snapshot 时 noop;旧 session
/// active snapshot 自动挪到 recovery。
pub fn snapshot_state(
    paths: &ClaudeDesktopPaths,
    app_version: &str,
    provider_name: Option<&str>,
) -> Result<SnapshotManifest, ClaudeDesktopError> {
    let dir = current_session_dir(paths);
    let manifest_file = manifest_path(&dir);
    if manifest_file.exists() {
        let text = std::fs::read_to_string(&manifest_file)?;
        let manifest: SnapshotManifest = serde_json::from_str(&text)?;
        return Ok(manifest);
    }
    rotate_stale_active_snapshots(paths)?;
    std::fs::create_dir_all(&dir)?;

    let plist_existed = paths
        .plist
        .as_ref()
        .map(|p| p.exists())
        .unwrap_or(false);
    let config_json_existed = paths
        .config_json
        .as_ref()
        .map(|p| p.exists())
        .unwrap_or(false);

    if let (Some(src), true) = (paths.plist.as_ref(), plist_existed) {
        std::fs::copy(src, plist_backup(&dir))?;
    }
    if let (Some(src), true) = (paths.config_json.as_ref(), config_json_existed) {
        std::fs::copy(src, config_json_backup(&dir))?;
    }

    let session_id = current_session_id();
    let manifest = SnapshotManifest {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_id: session_id.clone(),
        session_id,
        snapshot_at: Local::now().to_rfc3339(),
        plist_existed,
        config_json_existed,
        app_version: app_version.to_owned(),
        provider_name: provider_name.map(str::to_owned),
    };
    std::fs::write(&manifest_file, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(manifest)
}

/// 列出所有可恢复 snapshot(active + recovery)。
pub fn list_snapshots(paths: &ClaudeDesktopPaths) -> Vec<SnapshotInfo> {
    let mut out = Vec::new();
    let current = current_session_id();
    for (root, kind) in [
        (&paths.active_snapshots_dir, "active"),
        (&paths.recovery_snapshots_dir, "recovery"),
    ] {
        if !root.exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            let manifest_file = manifest_path(&dir);
            let Ok(text) = std::fs::read_to_string(&manifest_file) else {
                continue;
            };
            let Ok(manifest): Result<SnapshotManifest, _> = serde_json::from_str(&text) else {
                continue;
            };
            let id = manifest.snapshot_id.clone();
            out.push(SnapshotInfo {
                current_session: id == current && kind == "active",
                id,
                kind: kind.to_owned(),
                snapshot_at: manifest.snapshot_at,
                plist_existed: manifest.plist_existed,
                config_json_existed: manifest.config_json_existed,
                app_version: manifest.app_version,
                provider_name: manifest.provider_name,
            });
        }
    }
    out
}

/// 从 snapshot dir 恢复 plist + config.json(用 manifest 决定是否覆盖 /
/// 删除现有文件)。如果 manifest 里 plist_existed=false,则 restore 时
/// 删除现有 plist 而不是复制(回到"原本不存在"状态)。
pub fn restore_from_snapshot_dir(
    paths: &ClaudeDesktopPaths,
    snapshot_dir: &Path,
) -> Result<(), ClaudeDesktopError> {
    let manifest_file = manifest_path(snapshot_dir);
    let text = std::fs::read_to_string(&manifest_file)?;
    let manifest: SnapshotManifest = serde_json::from_str(&text)?;

    if let Some(target) = paths.plist.as_ref() {
        if manifest.plist_existed {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(plist_backup(snapshot_dir), target)?;
        } else if target.exists() {
            let _ = std::fs::remove_file(target);
        }
    }
    if let Some(target) = paths.config_json.as_ref() {
        if manifest.config_json_existed {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(config_json_backup(snapshot_dir), target)?;
        } else if target.exists() {
            let _ = std::fs::remove_file(target);
        }
    }
    Ok(())
}

/// 删除当前 session active snapshot(restore 成功后清理)。
pub fn drop_active_snapshot(paths: &ClaudeDesktopPaths) -> Result<(), ClaudeDesktopError> {
    let dir = current_session_dir(paths);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_dummy_macos_files(home: &Path) -> (PathBuf, PathBuf) {
        let plist = home
            .join("Library")
            .join("Preferences")
            .join("com.anthropic.claudefordesktop.plist");
        let cfg = home
            .join("Library")
            .join("Application Support")
            .join("Claude")
            .join("claude_desktop_config.json");
        std::fs::create_dir_all(plist.parent().unwrap()).unwrap();
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&plist, b"<plist>original</plist>").unwrap();
        std::fs::write(&cfg, br#"{"preferences": {"original": true}}"#).unwrap();
        (plist, cfg)
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn snapshot_copies_existing_files_and_records_manifest() {
        let td = TempDir::new().unwrap();
        write_dummy_macos_files(td.path());
        let paths = ClaudeDesktopPaths::from_home_dir(td.path());

        let manifest = snapshot_state(&paths, "0.0-test", Some("DeepSeek")).unwrap();
        assert!(manifest.plist_existed);
        assert!(manifest.config_json_existed);
        assert_eq!(manifest.app_version, "0.0-test");
        assert_eq!(manifest.provider_name.as_deref(), Some("DeepSeek"));

        // 同 session 二次 snapshot:noop,manifest 不变(snapshot_at 不变)
        let again = snapshot_state(&paths, "0.0-other", Some("Kimi")).unwrap();
        assert_eq!(again.snapshot_at, manifest.snapshot_at);
        assert_eq!(again.provider_name.as_deref(), Some("DeepSeek"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn restore_uses_manifest_to_decide_overwrite_vs_delete() {
        let td = TempDir::new().unwrap();
        let (plist, cfg) = write_dummy_macos_files(td.path());
        let paths = ClaudeDesktopPaths::from_home_dir(td.path());

        snapshot_state(&paths, "v", Some("X")).unwrap();
        // 模拟 apply 改写了 plist + config.json
        std::fs::write(&plist, b"<plist>modified</plist>").unwrap();
        std::fs::write(&cfg, br#"{"enterpriseConfig":"modified"}"#).unwrap();

        let dir = current_session_dir(&paths);
        restore_from_snapshot_dir(&paths, &dir).unwrap();

        assert_eq!(std::fs::read(&plist).unwrap(), b"<plist>original</plist>");
        assert_eq!(
            std::fs::read(&cfg).unwrap(),
            br#"{"preferences": {"original": true}}"#
        );
    }
}
