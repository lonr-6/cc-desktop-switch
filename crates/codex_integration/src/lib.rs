//! Codex CLI 配置文件集成(Stage 2.5).
//!
//! 端口自 Python 端 `backend/registry.py` 中第 850-1300 行(`~/.codex/*` 相关)。
//! **不**负责 ChatGPT 桌面客户端的 plist / Windows 注册表注入(那是另一条线,
//! 留 Stage 2.5b)。
//!
//! 入口:
//! - [`apply_provider`]:把当前 active provider 写入 `~/.codex/config.toml` +
//!   `~/.codex/auth.json`(根级别 line-based 同步,保留用户其它字段)
//! - [`restore_codex_state`]:基于快照精确还原我们改过的 key,**不动**用户
//!   在我们运行期间手动加的内容
//! - [`snapshot_codex_state`]:首次 apply 前自动调一次,把原状态打包到
//!   `~/.codex-app-transfer/codex-snapshots/active/<session>/`
//!
//! 路径解析全部走 [`CodexPaths`],测试可注入临时目录。

pub mod apply;
pub mod auth;
pub mod model_catalog;
pub mod paths;
pub mod snapshot;
pub mod toml_sync;

pub use apply::{
    apply_provider, restore_codex_snapshot, restore_codex_state, ApplyConfig, ApplyResult,
};
pub use auth::{read_auth, write_auth};
pub use model_catalog::{catalog_models_for_provider, strip_model_suffix, upsert_catalog_models};
pub use paths::CodexPaths;
pub use snapshot::{
    get_snapshot_status, has_snapshot, list_snapshots, snapshot_codex_state, SnapshotInfo,
    SnapshotManifest, SnapshotStatus,
};
pub use toml_sync::sync_root_value;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CodexError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("registry io: {0}")]
    RegistryIo(#[from] codex_app_transfer_registry::IoError),
    #[error("home directory not resolved: set $HOME or pass paths explicitly")]
    NoHome,
}
