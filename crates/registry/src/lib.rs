//! Codex App Transfer · 配置数据层(Stage 1).
//!
//! 这个 crate 负责读写 `~/.codex-app-transfer/` 下的所有 JSON 文件,与 Python
//! 版 `backend/config.py` + `backend/model_alias.py` 保持字节级互操作。
//!
//! 设计要点:
//! - 双视图:`Config` 提供类型化访问(serde 派生),`raw_io` 模块走
//!   `serde_json::Value`(带 preserve_order 特性)以保证 round-trip 字节级
//!   不变。下游业务代码用 `Config`,持久化层用 `raw_io`。
//! - 序列化器与 Python `json.dump(ensure_ascii=False, indent=2)` 等价 ——
//!   `serde_json::to_string_pretty` 默认 2 空格缩进、非 ASCII 不转义,
//!   `,` 与 `: ` 分隔符也一致。
//! - **未实现** OS 集成层(Windows 注册表 / macOS plist / Codex TOML 注入),
//!   按 docs/refactor/migration.md §4 拆分,留给 Stage 2.5 的 `crates/codex_integration`。

pub mod healing;
pub mod model_alias;
pub mod model_context_policy;
pub mod paths;
pub mod presets;
pub mod raw_io;
pub mod schema;

#[allow(deprecated)]
pub use healing::heal_builtin_extra_headers;
pub use healing::heal_builtin_provider_fields;
pub use healing::heal_legacy_update_url;
pub use model_alias::{
    empty_model_mappings, has_internal_one_m_suffix, normalize_model_mappings, openai_model_slot,
    provider_slug, strip_internal_model_suffix, MODEL_ORDER, MODEL_SLOTS,
};
pub use model_context_policy::{
    documented_context_window, model_supports_1m, ONE_M_CONTEXT_WINDOW,
};
pub use paths::{
    config_dir, config_file, library_dir, resolve_home, sessions_db_file, tool_artifacts_db_file,
};
pub use presets::builtin_presets;
pub use raw_io::{load_raw_config, save_raw_config, IoError, RawConfig};
pub use schema::{Config, ModelSlotKey, Provider, Settings, APP_VERSION, DEFAULT_UPDATE_URL};
