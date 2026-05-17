//! Claude Desktop 配置文件 I/O ——
//! `~/.codex-app-transfer/claude-desktop-config.json`
//!
//! **故意**跟 cas 现有 `~/.codex-app-transfer/config.json`(Codex CLI 端
//! provider 列表 / 设置)**完全分文件**,理由:
//! - 避免动 `crates/registry/src/schema.rs::Config` 引入对 claude_desktop
//!   crate 的反向依赖
//! - 两端配置体系独立(`apiFormat="anthropic"` 直连 vs `apiFormat="openai_chat"`
//!   走 cas proxy),user 切端不影响对端
//! - 升级/回滚时单边失败不波及对端

use std::path::{Path, PathBuf};

use crate::paths::ClaudeDesktopPaths;
use crate::schema::ClaudeDesktopConfig;
use crate::ClaudeDesktopError;

/// `~/.codex-app-transfer/claude-desktop-config.json`
pub fn config_file_path(paths: &ClaudeDesktopPaths) -> PathBuf {
    paths.app_home.join("claude-desktop-config.json")
}

/// 读 config 文件(不存在时返回 default 空 config)。
pub fn load_config(paths: &ClaudeDesktopPaths) -> Result<ClaudeDesktopConfig, ClaudeDesktopError> {
    let path = config_file_path(paths);
    if !path.exists() {
        return Ok(ClaudeDesktopConfig::default());
    }
    let text = std::fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(ClaudeDesktopConfig::default());
    }
    let cfg: ClaudeDesktopConfig = serde_json::from_str(&text).map_err(|e| {
        ClaudeDesktopError::SchemaCorrupt(format!(
            "claude-desktop-config.json 解析失败({}): {e}",
            path.display()
        ))
    })?;
    Ok(cfg)
}

/// 原子写 config 文件(tmp + rename,防写入半截 corruption)。
pub fn save_config(
    paths: &ClaudeDesktopPaths,
    cfg: &ClaudeDesktopConfig,
) -> Result<(), ClaudeDesktopError> {
    let path = config_file_path(paths);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_vec_pretty(cfg)?;
    write_atomic(&path, &serialized)?;
    Ok(())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), ClaudeDesktopError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "config.json".to_owned());
    let tmp = parent.join(format!(".{file_name}.tmp"));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Provider;
    use indexmap::IndexMap;
    use tempfile::TempDir;

    fn make_provider(id: &str) -> Provider {
        Provider {
            id: id.to_owned(),
            name: id.to_owned(),
            base_url: "https://example.com/anthropic".to_owned(),
            auth_scheme: "bearer".to_owned(),
            api_format: "anthropic".to_owned(),
            api_key: String::new(),
            models: Default::default(),
            base_url_options: Vec::new(),
            base_url_hint: String::new(),
            model_options: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            request_option_presets: IndexMap::new(),
            extra_headers: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn load_returns_default_when_missing() {
        let td = TempDir::new().unwrap();
        let paths = ClaudeDesktopPaths::from_home_dir(td.path());
        let cfg = load_config(&paths).unwrap();
        assert!(cfg.active_provider.is_none());
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let td = TempDir::new().unwrap();
        let paths = ClaudeDesktopPaths::from_home_dir(td.path());
        let mut cfg = ClaudeDesktopConfig::default();
        cfg.providers.push(make_provider("deepseek"));
        cfg.active_provider = Some("deepseek".to_owned());
        save_config(&paths, &cfg).unwrap();

        let loaded = load_config(&paths).unwrap();
        assert_eq!(loaded.active_provider.as_deref(), Some("deepseek"));
        assert_eq!(loaded.providers.len(), 1);
    }

    #[test]
    fn load_empty_file_returns_default() {
        let td = TempDir::new().unwrap();
        let paths = ClaudeDesktopPaths::from_home_dir(td.path());
        std::fs::create_dir_all(&paths.app_home).unwrap();
        std::fs::write(config_file_path(&paths), b"").unwrap();
        let cfg = load_config(&paths).unwrap();
        assert!(cfg.providers.is_empty());
    }

    #[test]
    fn load_corrupt_returns_error() {
        let td = TempDir::new().unwrap();
        let paths = ClaudeDesktopPaths::from_home_dir(td.path());
        std::fs::create_dir_all(&paths.app_home).unwrap();
        std::fs::write(config_file_path(&paths), b"not json {").unwrap();
        let err = load_config(&paths).unwrap_err();
        assert!(err.to_string().contains("claude-desktop-config.json"));
    }
}
