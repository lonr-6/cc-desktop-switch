//! `/api/*` 路由 handler 模块.
//!
//! 拆分自原单文件 `handlers.rs`(5229 行),按域分子模块:
//! - `common`:跨域 helper + status/version + instance
//! - `proxy`:`/api/proxy/*`
//! - `feedback`:`/api/feedback`
//! - `settings`:`/api/settings` + `/api/config/*`
//! - `update`:`/api/update/*`
//! - `desktop`:`/api/desktop/*` + Codex.app 进程管理 + apply/restore
//! - `plugin_unlock`:`/api/desktop/plugin-unlock/*` + Codex Desktop Plugins CDP 注入
//! - `providers`:`/api/providers/*` + `/api/presets`(二级再拆 crud/test/models/balance/presets)

pub mod antigravity_oauth;
pub mod claude_desktop;
pub mod common;
pub mod desktop;
pub mod feedback;
pub mod gemini_oauth;
pub mod plugin_unlock;
pub mod providers;
pub mod proxy;
pub mod settings;
pub mod update;
