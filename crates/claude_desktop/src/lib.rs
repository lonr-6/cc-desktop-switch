//! Claude Desktop 配置写入 + 内置 provider 预设。
//!
//! **借鉴自** `lonr-6/cc-desktop-switch`(`backend/config.py:18-185` 的 schema
//! 与 BUILTIN_PRESETS,Apache-2 / MIT)。Rust 端按"1:1 转写"原则:不在 cas
//! 端做架构创新,字段、preset 数据、模型槽位语义都跟上游对齐。
//!
//! 跟 `codex-app-transfer-codex-integration` 是**两个独立 crate**:
//! - `codex_integration`:Codex CLI 端,写 `~/.codex/config.toml` 的
//!   `openai_base_url` / `model_catalog_json` 等 key,服务 OpenAI 兼容协议
//! - `claude_desktop`(本 crate):Claude Desktop 桌面客户端端,写
//!   `~/Library/Application Support/Claude/claude_desktop_config.json` 的
//!   `enterpriseConfig.inferenceGateway*` 字段(macOS)+ Windows Registry
//!   `HKCU\SOFTWARE\Policies\Claude` 字段集,服务 Anthropic Messages 直连。
//!
//! ## 范围(PR-1)
//!
//! 当前 crate 提供:
//! - [`schema`]:Provider / ClaudeDesktopConfig 数据结构(1:1 转写
//!   `backend/config.py:18-185`)
//! - [`presets`]:7 个 Anthropic-compatible 内置 provider 预设
//!   (DeepSeek / Kimi / Kimi Code / Xiaomi MiMo PayG / Xiaomi MiMo Token Plan
//!   / 智谱 GLM / 阿里云百炼)
//!
//! 后续 PR 接力:
//! - `registry.rs`:Windows Registry HKCU `SOFTWARE\Policies\Claude` 写入
//!   (1:1 转写 `cc-desktop-switch/backend/registry.py:1-890`)
//! - `macos.rs`:macOS plist `~/Library/Preferences/com.anthropic.claudefordesktop.plist`
//!   + Application Support `claude_desktop_config.json` 写入(cc-desktop-switch
//!   macOS 端无 upstream 实现,cas 端按实测字段集设计)
//! - `apply.rs` / `restore.rs` / `snapshot.rs`:对称 codex_integration 模式

pub mod apply;
pub mod helpers;
pub mod io;
#[cfg(any(target_os = "macos", test))]
pub mod macos;
pub mod model_alias;
pub mod paths;
pub mod presets;
pub mod proxy;
pub mod schema;
pub mod snapshot;

pub use proxy::{
    build_router as build_proxy_router, generate_gateway_api_key, ClaudeDesktopProxyState,
    CD_PROXY_BIND, CD_PROXY_PORT,
};

pub use apply::{apply_provider, restore_state, ApplyConfig, ApplyResult};
pub use io::{config_file_path, load_config, save_config};
pub use paths::ClaudeDesktopPaths;
pub use snapshot::{
    current_session_dir, drop_active_snapshot, has_snapshot, list_snapshots,
    restore_from_snapshot_dir, snapshot_state, SnapshotInfo, SnapshotManifest,
};

pub use model_alias::{
    all_provider_model_entries, claude_id_to_slot, empty_model_mappings, model_alias,
    model_mappings_with_legacy_aliases, model_order, model_supports_1m, normalize_model_mappings,
    provider_model_entries, provider_model_ids, provider_slug, resolve_model_alias,
    resolve_requested_model_slot, ModelEntry, ModelSlot, DEFAULT_MODEL_KEY, LEGACY_MODEL_KEYS,
    MODEL_SLOTS,
};
pub use presets::builtin_presets;
pub use schema::{
    BaseUrlOption, ClaudeDesktopConfig, ModelMappings, ModelOption, Provider, RequestOptionPreset,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClaudeDesktopError {
    #[error("schema 损坏: {0}")]
    SchemaCorrupt(String),
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 序列化失败: {0}")]
    Json(#[from] serde_json::Error),
}
