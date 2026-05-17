//! Claude Desktop 端路径解析 —— 对称 [`codex_app_transfer_codex_integration::paths::CodexPaths`]
//! 设计。
//!
//! Claude Desktop 的两个写入目标位于固定系统路径:
//! - macOS:`~/Library/Application Support/Claude/claude_desktop_config.json`
//!   + `~/Library/Preferences/com.anthropic.claudefordesktop.plist`
//! - Windows:`HKEY_CURRENT_USER\SOFTWARE\Policies\Claude`(Registry 不是文件,
//!   用 cfg 分发,不进 `ClaudeDesktopPaths`)
//!
//! cas 端 snapshot 目录跟 codex_integration 并列:
//! `~/.codex-app-transfer/claude-desktop-snapshots/active/<session>/`。

use std::path::{Path, PathBuf};

use crate::ClaudeDesktopError;

#[derive(Debug, Clone)]
pub struct ClaudeDesktopPaths {
    pub app_home: PathBuf,
    /// macOS only:`~/Library/Application Support/Claude/claude_desktop_config.json`。
    /// 其它平台 None(Windows 走 Registry 不用此字段)。
    pub config_json: Option<PathBuf>,
    /// macOS only:`~/Library/Preferences/com.anthropic.claudefordesktop.plist`。
    pub plist: Option<PathBuf>,
    /// snapshot 根目录:`~/.codex-app-transfer/claude-desktop-snapshots/`。
    pub snapshots_dir: PathBuf,
    /// 当前 session active 子目录(由 [`crate::snapshot`] 拼)。
    pub active_snapshots_dir: PathBuf,
    /// 旧 session active 快照被挪到这里,等用户手动 restore。
    pub recovery_snapshots_dir: PathBuf,
}

impl ClaudeDesktopPaths {
    pub fn from_home_env() -> Result<Self, ClaudeDesktopError> {
        let home = dirs::home_dir().ok_or_else(|| {
            ClaudeDesktopError::SchemaCorrupt("无法解析 home 目录".to_owned())
        })?;
        Ok(Self::from_home_dir(home))
    }

    pub fn from_home_dir(home: impl AsRef<Path>) -> Self {
        let home = home.as_ref();
        let app_home = home.join(".codex-app-transfer");
        let snapshots_dir = app_home.join("claude-desktop-snapshots");
        let active_snapshots_dir = snapshots_dir.join("active");
        let recovery_snapshots_dir = snapshots_dir.join("recovery");
        let (config_json, plist) = if cfg!(target_os = "macos") {
            (
                Some(
                    home.join("Library")
                        .join("Application Support")
                        .join("Claude")
                        .join("claude_desktop_config.json"),
                ),
                Some(
                    home.join("Library")
                        .join("Preferences")
                        .join("com.anthropic.claudefordesktop.plist"),
                ),
            )
        } else {
            (None, None)
        };
        Self {
            app_home,
            config_json,
            plist,
            snapshots_dir,
            active_snapshots_dir,
            recovery_snapshots_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn from_home_dir_layout() {
        let td = TempDir::new().unwrap();
        let p = ClaudeDesktopPaths::from_home_dir(td.path());
        assert!(p.snapshots_dir.ends_with("claude-desktop-snapshots"));
        assert!(p.active_snapshots_dir.ends_with("claude-desktop-snapshots/active"));
        assert!(p.recovery_snapshots_dir.ends_with("claude-desktop-snapshots/recovery"));
        if cfg!(target_os = "macos") {
            assert!(p.config_json.as_ref().unwrap().to_string_lossy().contains("Library/Application Support/Claude/"));
            assert!(p.plist.as_ref().unwrap().to_string_lossy().contains("Library/Preferences/"));
        }
    }
}
