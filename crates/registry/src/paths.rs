//! 配置目录解析 —— 等价于 `backend/config.py` 中的常量.
//!
//! [`resolve_home`] 是 workspace 内**唯一**用户主目录解析入口,被
//! `codex_integration::CodexPaths::from_home_env` 与
//! `gemini_oauth::TokenStore::for_token_filename` 共用,保证 Windows GUI
//! 进程(无 `HOME`、只有 `USERPROFILE`)行为一致(参见 PR #115)。

use std::ffi::OsString;
use std::path::PathBuf;

const CONFIG_DIR_NAME: &str = ".codex-app-transfer";
const CONFIG_FILE_NAME: &str = "config.json";
const LIBRARY_DIR_NAME: &str = "configLibrary";
const BACKUPS_DIR_NAME: &str = "backups";
const SESSIONS_DB_NAME: &str = "sessions.db";
const TOOL_ARTIFACTS_DB_NAME: &str = "tool_artifacts.db";

/// 解析当前用户的 home 目录:`$HOME` 优先,`$USERPROFILE`(Windows GUI 进程
/// 默认值)回退;空字符串视作未设。返 `None` 时调用方应映射为各自的
/// "无可定位主目录"错误(如 `CodexError::NoHome` / `TokenError::HomeNotSet`)。
///
/// 用 `std::env::var_os` 而非 `var`,保证 non-UTF-8 path(理论存在,实测罕见)
/// 也能解析,避免 `var` 返 `Err(NotUnicode)` 导致 home unavailable 假阴性。
pub fn resolve_home() -> Option<PathBuf> {
    resolve_home_from(|k| std::env::var_os(k))
}

/// [`resolve_home`] 的纯函数版本 —— 注入 env getter 给单测调用,避免在测试
/// 中跨线程改进程级环境变量(Rust 1.78+ `std::env::set_var` 在多线程下需
/// `unsafe`,workspace 测试默认并发 runner)。本 fn `pub(crate)`,仅 crate
/// 内 + 单测可达。
pub(crate) fn resolve_home_from<F>(get_env: F) -> Option<PathBuf>
where
    F: Fn(&str) -> Option<OsString>,
{
    for key in ["HOME", "USERPROFILE"] {
        if let Some(raw) = get_env(key) {
            if !raw.is_empty() {
                return Some(PathBuf::from(raw));
            }
        }
    }
    None
}

pub fn config_dir() -> Option<PathBuf> {
    resolve_home().map(|h| h.join(CONFIG_DIR_NAME))
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join(CONFIG_FILE_NAME))
}

pub fn library_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join(LIBRARY_DIR_NAME))
}

pub fn backups_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join(BACKUPS_DIR_NAME))
}

/// SQLite 持久化的 ResponseSessionCache 数据库路径,
/// `~/.codex-app-transfer/sessions.db`。
pub fn sessions_db_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join(SESSIONS_DB_NAME))
}

/// SQLite 持久化的工具原始输出 sidecar store 路径,
/// `~/.codex-app-transfer/tool_artifacts.db`。
pub fn tool_artifacts_db_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join(TOOL_ARTIFACTS_DB_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_home_prefers_home_over_userprofile() {
        let env = |k: &str| match k {
            "HOME" => Some(OsString::from("/Users/me")),
            "USERPROFILE" => Some(OsString::from(r"C:\Users\me")),
            _ => None,
        };
        assert_eq!(
            resolve_home_from(env),
            Some(PathBuf::from("/Users/me")),
            "HOME 若存在必须优先于 USERPROFILE,保证 Mac/Linux 行为不变"
        );
    }

    #[test]
    fn resolve_home_falls_back_to_userprofile_on_windows() {
        let env = |k: &str| match k {
            "USERPROFILE" => Some(OsString::from(r"C:\Users\me")),
            _ => None,
        };
        assert_eq!(
            resolve_home_from(env),
            Some(PathBuf::from(r"C:\Users\me")),
            "Windows GUI 进程没有 HOME 时必须走 USERPROFILE(PR #115 修)"
        );
    }

    #[test]
    fn resolve_home_treats_empty_strings_as_missing() {
        // CI runner / 某些 shell 可能把 HOME 设成 "" 而不是 unset;空值拼出来
        // 等价相对 cwd,行为危险 — 当未设处理,继续 fallback
        let env = |k: &str| match k {
            "HOME" => Some(OsString::new()),
            "USERPROFILE" => Some(OsString::from(r"C:\Users\me")),
            _ => None,
        };
        assert_eq!(
            resolve_home_from(env),
            Some(PathBuf::from(r"C:\Users\me")),
            "空字符串 HOME 应视作未设,继续 fallback 到 USERPROFILE"
        );
    }

    #[test]
    fn resolve_home_returns_none_when_both_missing() {
        let env = |_k: &str| None;
        assert_eq!(resolve_home_from(env), None);
    }

    #[test]
    fn tool_artifacts_db_file_lives_under_config_dir() {
        let home = PathBuf::from("/Users/me");
        let path = home.join(CONFIG_DIR_NAME).join(TOOL_ARTIFACTS_DB_NAME);
        assert_eq!(
            path,
            PathBuf::from("/Users/me/.codex-app-transfer/tool_artifacts.db")
        );
    }
}
