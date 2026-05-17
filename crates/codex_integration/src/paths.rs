//! 路径解析:`~/.codex/{config.toml,auth.json}` + 本应用 Codex 快照目录.

use std::path::{Path, PathBuf};

use crate::CodexError;

#[derive(Debug, Clone)]
pub struct CodexPaths {
    pub codex_home: PathBuf,
    pub app_home: PathBuf,
    pub config_toml: PathBuf,
    pub auth_json: PathBuf,
    pub model_catalog_json: PathBuf,
    /// Legacy single-snapshot path kept for upgrade compatibility.
    pub snapshot_dir: PathBuf,
    pub snapshot_config: PathBuf,
    pub snapshot_auth: PathBuf,
    pub snapshot_manifest: PathBuf,
    pub snapshots_dir: PathBuf,
    pub active_snapshots_dir: PathBuf,
    pub recovery_snapshots_dir: PathBuf,
}

impl CodexPaths {
    /// 用真实用户 home 目录构造。Home 解析委派给
    /// [`codex_app_transfer_registry::paths::resolve_home`],它是 workspace
    /// 内唯一入口,统一 `HOME` → `USERPROFILE` 回退 + 空字符串视作未设(避免
    /// 此前 3 处独立实现 drift,PR #115 后续清理)。
    pub fn from_home_env() -> Result<Self, CodexError> {
        let home = codex_app_transfer_registry::paths::resolve_home().ok_or(CodexError::NoHome)?;
        Ok(Self::from_home_dir(home))
    }

    /// 显式给一个 home 目录(测试常用 tmp dir)。
    pub fn from_home_dir(home: impl AsRef<Path>) -> Self {
        let home = home.as_ref();
        let codex_home = home.join(".codex");
        let app_home = home.join(".codex-app-transfer");
        let snapshot_dir = app_home.join("codex-snapshot");
        let snapshots_dir = app_home.join("codex-snapshots");
        let active_snapshots_dir = snapshots_dir.join("active");
        let recovery_snapshots_dir = snapshots_dir.join("recovery");
        Self {
            config_toml: codex_home.join("config.toml"),
            auth_json: codex_home.join("auth.json"),
            model_catalog_json: app_home.join("config.json"),
            snapshot_config: snapshot_dir.join("config.toml"),
            snapshot_auth: snapshot_dir.join("auth.json"),
            snapshot_manifest: snapshot_dir.join("manifest.json"),
            snapshot_dir,
            snapshots_dir,
            active_snapshots_dir,
            recovery_snapshots_dir,
            codex_home,
            app_home,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_home_dir_layout() {
        let p = CodexPaths::from_home_dir("/x");
        assert_eq!(p.codex_home, PathBuf::from("/x/.codex"));
        assert_eq!(p.app_home, PathBuf::from("/x/.codex-app-transfer"));
        assert_eq!(p.config_toml, PathBuf::from("/x/.codex/config.toml"));
        assert_eq!(p.auth_json, PathBuf::from("/x/.codex/auth.json"));
        assert_eq!(
            p.model_catalog_json,
            PathBuf::from("/x/.codex-app-transfer/config.json")
        );
        assert_eq!(
            p.snapshot_dir,
            PathBuf::from("/x/.codex-app-transfer/codex-snapshot")
        );
        assert_eq!(
            p.snapshot_manifest,
            PathBuf::from("/x/.codex-app-transfer/codex-snapshot/manifest.json")
        );
        assert_eq!(
            p.snapshots_dir,
            PathBuf::from("/x/.codex-app-transfer/codex-snapshots")
        );
        assert_eq!(
            p.active_snapshots_dir,
            PathBuf::from("/x/.codex-app-transfer/codex-snapshots/active")
        );
        assert_eq!(
            p.recovery_snapshots_dir,
            PathBuf::from("/x/.codex-app-transfer/codex-snapshots/recovery")
        );
    }
}
