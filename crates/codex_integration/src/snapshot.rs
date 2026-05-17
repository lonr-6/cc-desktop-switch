//! 首次 apply 前的快照机制.
//!
//! 1. apply 前调一次 [`snapshot_codex_state`]:把当前 `config.toml` 与
//!    `auth.json` 整文件复制到当前进程 session 的 active 快照目录,并写
//!    一份 `manifest.json` 记录"这两个文件原本存不存在"。
//! 2. 同一进程 session 内已经有 active 快照时,**不重复**(同会话多次 apply
//!    不会污染最初备份)。
//! 3. 发现旧 session active 快照时,先移动到 timestamp/session 命名的
//!    recovery 目录,再创建本 session active 快照,避免多版本/多进程启动覆盖
//!    最早的用户配置。
//! 4. restore 时基于快照精确还原我们改过的几个 key,**不动**用户在我们运行
//!    期间手加的内容。

use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::paths::CodexPaths;
use crate::toml_sync::write_atomic;
use crate::CodexError;

const SNAPSHOT_SCHEMA_VERSION: u32 = 2;

static CURRENT_SESSION_ID: OnceLock<String> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub snapshot_id: String,
    #[serde(default)]
    pub session_id: String,
    pub snapshot_at: String,
    pub config_existed: bool,
    pub auth_existed: bool,
    pub app_version: String,
    #[serde(default)]
    pub provider_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotStatus {
    pub has_snapshot: bool,
    pub snapshot_at: Option<String>,
    pub config_existed: bool,
    pub auth_existed: bool,
    pub app_version: Option<String>,
    pub restorable_count: usize,
    pub recovery_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: String,
    pub kind: String,
    pub snapshot_at: String,
    pub config_existed: bool,
    pub auth_existed: bool,
    pub app_version: String,
    pub provider_name: Option<String>,
    pub current_session: bool,
}

/// 是否有未还原的快照。
pub fn has_snapshot(paths: &CodexPaths) -> bool {
    current_session_snapshot_dir(paths)
        .map(|dir| manifest_path(&dir).exists())
        .unwrap_or(false)
        || paths.snapshot_manifest.exists()
}

/// 列出人工恢复可选择的所有快照(不含敏感字段)。
pub fn list_snapshots(paths: &CodexPaths) -> Vec<SnapshotInfo> {
    let mut out = Vec::new();
    if paths.snapshot_manifest.exists() {
        if let Ok(manifest) = read_manifest_from_dir(&paths.snapshot_dir) {
            out.push(info_from_manifest(
                normalize_manifest(manifest, "legacy", "legacy"),
                "legacy",
            ));
        }
    }

    for dir in snapshot_dirs_under(&paths.active_snapshots_dir) {
        if let Ok(manifest) = read_manifest_from_dir(&dir) {
            let fallback = dir_name(&dir).unwrap_or_else(|| "active".to_owned());
            out.push(info_from_manifest(
                normalize_manifest(manifest, &fallback, &fallback),
                "active",
            ));
        }
    }

    for dir in snapshot_dirs_under(&paths.recovery_snapshots_dir) {
        if let Ok(manifest) = read_manifest_from_dir(&dir) {
            let fallback = dir_name(&dir).unwrap_or_else(|| "recovery".to_owned());
            out.push(info_from_manifest(
                normalize_manifest(manifest, &fallback, &fallback),
                "recovery",
            ));
        }
    }

    out.sort_by(|a, b| b.snapshot_at.cmp(&a.snapshot_at).then(a.kind.cmp(&b.kind)));
    out
}

/// 供 UI 展示用的快照状态(不含敏感字段)。
pub fn get_snapshot_status(paths: &CodexPaths) -> SnapshotStatus {
    let snapshots = list_snapshots(paths);
    let active = current_snapshot_info(paths);
    let recovery_count = snapshots
        .iter()
        .filter(|s| s.kind == "recovery" || s.kind == "legacy")
        .count();
    if active.is_none() {
        return SnapshotStatus {
            has_snapshot: false,
            snapshot_at: None,
            config_existed: false,
            auth_existed: false,
            app_version: None,
            restorable_count: snapshots.len(),
            recovery_count,
        };
    }
    let active = active.expect("checked above");
    SnapshotStatus {
        has_snapshot: true,
        snapshot_at: Some(active.snapshot_at),
        config_existed: active.config_existed,
        auth_existed: active.auth_existed,
        app_version: Some(active.app_version),
        restorable_count: snapshots.len(),
        recovery_count,
    }
}

/// 首次 apply 前调用。已存在快照则直接返回当前 manifest。
pub fn snapshot_codex_state(
    paths: &CodexPaths,
    app_version: &str,
    provider_name: &str,
) -> Result<SnapshotManifest, CodexError> {
    move_stale_active_snapshots_to_recovery(paths)?;

    let current_dir = current_active_snapshot_dir(paths);
    if manifest_path(&current_dir).exists() {
        return read_manifest_from_dir(&current_dir);
    }
    if paths.snapshot_manifest.exists() {
        return read_manifest_from_dir(&paths.snapshot_dir);
    }
    std::fs::create_dir_all(&current_dir)?;

    let config_existed = paths.config_toml.exists();
    let auth_existed = paths.auth_json.exists();

    if config_existed {
        std::fs::copy(&paths.config_toml, config_path(&current_dir))?;
    }
    if auth_existed {
        let snapshot_auth = auth_path(&current_dir);
        std::fs::copy(&paths.auth_json, &snapshot_auth)?;
        // 快照里的 auth 也要 0600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(snapshot_auth, std::fs::Permissions::from_mode(0o600));
        }
    }

    let session_id = current_session_id().to_owned();
    let manifest = SnapshotManifest {
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        snapshot_id: session_id.clone(),
        session_id,
        snapshot_at: Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        config_existed,
        auth_existed,
        app_version: app_version.to_owned(),
        provider_name: if provider_name.is_empty() {
            None
        } else {
            Some(provider_name.to_owned())
        },
    };
    write_manifest_to_dir(&current_dir, &manifest)?;
    Ok(manifest)
}

/// 删除整个快照目录(restore 完成后的清理)。
pub fn drop_snapshot(paths: &CodexPaths) -> Result<(), CodexError> {
    if let Some(dir) = current_snapshot_dir(paths) {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// 删除指定快照。人工恢复成功后可用来清理已恢复项。
pub fn drop_snapshot_by_id(paths: &CodexPaths, snapshot_id: &str) -> Result<(), CodexError> {
    if let Some(dir) = snapshot_dir_by_id(paths, snapshot_id) {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// 删除所有 active/recovery/legacy 快照。仅用于人工选择恢复成功后的清理。
pub fn drop_all_snapshots(paths: &CodexPaths) -> Result<(), CodexError> {
    if paths.active_snapshots_dir.exists() {
        std::fs::remove_dir_all(&paths.active_snapshots_dir)?;
    }
    if paths.recovery_snapshots_dir.exists() {
        std::fs::remove_dir_all(&paths.recovery_snapshots_dir)?;
    }
    if paths.snapshot_dir.exists() {
        std::fs::remove_dir_all(&paths.snapshot_dir)?;
    }
    Ok(())
}

fn read_manifest_from_dir(dir: &Path) -> Result<SnapshotManifest, CodexError> {
    let s = std::fs::read_to_string(manifest_path(dir))?;
    Ok(serde_json::from_str(&s)?)
}

fn write_manifest_to_dir(dir: &Path, manifest: &SnapshotManifest) -> Result<(), CodexError> {
    let mut s = serde_json::to_string_pretty(manifest)?;
    s.push('\n');
    write_atomic(&manifest_path(dir), &s)?;
    Ok(())
}

/// 读取快照中的 config.toml 内容(不存在时返回空)。
pub(crate) fn read_snapshot_config(paths: &CodexPaths) -> Option<String> {
    current_snapshot_dir(paths).and_then(|dir| read_snapshot_config_from_dir(&dir))
}

pub(crate) fn read_snapshot_config_by_id(paths: &CodexPaths, snapshot_id: &str) -> Option<String> {
    snapshot_dir_by_id(paths, snapshot_id).and_then(|dir| read_snapshot_config_from_dir(&dir))
}

fn read_snapshot_config_from_dir(dir: &Path) -> Option<String> {
    std::fs::read_to_string(config_path(dir)).ok()
}

/// 读取快照中的 auth.json(不存在时返回空对象)。
pub(crate) fn read_snapshot_auth(paths: &CodexPaths) -> serde_json::Value {
    current_snapshot_dir(paths)
        .map(|dir| read_snapshot_auth_from_dir(&dir))
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()))
}

pub(crate) fn read_snapshot_auth_by_id(paths: &CodexPaths, snapshot_id: &str) -> serde_json::Value {
    snapshot_dir_by_id(paths, snapshot_id)
        .map(|dir| read_snapshot_auth_from_dir(&dir))
        .unwrap_or_else(|| serde_json::Value::Object(Default::default()))
}

fn read_snapshot_auth_from_dir(dir: &Path) -> serde_json::Value {
    let txt = std::fs::read_to_string(auth_path(dir)).ok();
    match txt {
        Some(s) if !s.trim().is_empty() => serde_json::from_str(&s)
            .unwrap_or_else(|_| serde_json::Value::Object(Default::default())),
        _ => serde_json::Value::Object(Default::default()),
    }
}

fn default_schema_version() -> u32 {
    1
}

fn current_session_id() -> &'static str {
    CURRENT_SESSION_ID
        .get_or_init(|| {
            format!(
                "{}-p{}",
                Local::now().format("%Y%m%dT%H%M%S%3f"),
                std::process::id()
            )
        })
        .as_str()
}

fn current_active_snapshot_dir(paths: &CodexPaths) -> PathBuf {
    paths.active_snapshots_dir.join(current_session_id())
}

fn current_session_snapshot_dir(paths: &CodexPaths) -> Option<PathBuf> {
    let dir = current_active_snapshot_dir(paths);
    manifest_path(&dir).exists().then_some(dir)
}

fn current_snapshot_dir(paths: &CodexPaths) -> Option<PathBuf> {
    current_session_snapshot_dir(paths).or_else(|| {
        paths
            .snapshot_manifest
            .exists()
            .then_some(paths.snapshot_dir.clone())
    })
}

fn current_snapshot_info(paths: &CodexPaths) -> Option<SnapshotInfo> {
    let dir = current_snapshot_dir(paths)?;
    let kind = if dir == paths.snapshot_dir {
        "legacy"
    } else {
        "active"
    };
    let fallback = if kind == "legacy" {
        "legacy".to_owned()
    } else {
        dir_name(&dir).unwrap_or_else(|| "active".to_owned())
    };
    let manifest = read_manifest_from_dir(&dir).ok()?;
    Some(info_from_manifest(
        normalize_manifest(manifest, &fallback, &fallback),
        kind,
    ))
}

fn snapshot_dir_by_id(paths: &CodexPaths, snapshot_id: &str) -> Option<PathBuf> {
    for dir in snapshot_dirs_under(&paths.active_snapshots_dir) {
        if snapshot_dir_matches_id(&dir, snapshot_id) {
            return Some(dir);
        }
    }
    if paths.snapshot_manifest.exists() && snapshot_id == "legacy" {
        return Some(paths.snapshot_dir.clone());
    }
    if paths.snapshot_manifest.exists() && snapshot_dir_matches_id(&paths.snapshot_dir, snapshot_id)
    {
        return Some(paths.snapshot_dir.clone());
    }
    for dir in snapshot_dirs_under(&paths.recovery_snapshots_dir) {
        if snapshot_dir_matches_id(&dir, snapshot_id) {
            return Some(dir);
        }
    }
    None
}

fn snapshot_dir_matches_id(dir: &Path, snapshot_id: &str) -> bool {
    let fallback = dir_name(dir).unwrap_or_default();
    if fallback == snapshot_id {
        return true;
    }
    read_manifest_from_dir(dir)
        .ok()
        .map(|manifest| {
            manifest.snapshot_id == snapshot_id
                || manifest.session_id == snapshot_id
                || (manifest.snapshot_id.is_empty() && snapshot_id == fallback)
        })
        .unwrap_or(false)
}

fn move_stale_active_snapshots_to_recovery(paths: &CodexPaths) -> Result<(), CodexError> {
    let current = current_session_id().to_owned();
    for dir in snapshot_dirs_under(&paths.active_snapshots_dir) {
        if dir_name(&dir).as_deref() == Some(current.as_str()) {
            continue;
        }
        move_snapshot_dir_to_recovery(paths, &dir)?;
    }
    Ok(())
}

fn move_snapshot_dir_to_recovery(paths: &CodexPaths, dir: &Path) -> Result<(), CodexError> {
    let fallback = dir_name(dir).unwrap_or_else(|| current_session_id().to_owned());
    let manifest = read_manifest_from_dir(dir)
        .map(|m| normalize_manifest(m, &fallback, &fallback))
        .unwrap_or_else(|_| SnapshotManifest {
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            snapshot_id: fallback.clone(),
            session_id: fallback.clone(),
            snapshot_at: Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
            config_existed: config_path(dir).exists(),
            auth_existed: auth_path(dir).exists(),
            app_version: String::new(),
            provider_name: None,
        });
    std::fs::create_dir_all(&paths.recovery_snapshots_dir)?;
    let target = unique_recovery_dir(paths, &manifest.snapshot_id);
    std::fs::rename(dir, &target)?;
    let mut recovery_manifest = manifest;
    recovery_manifest.schema_version = SNAPSHOT_SCHEMA_VERSION;
    if let Some(target_id) = dir_name(&target) {
        recovery_manifest.snapshot_id = target_id;
    }
    write_manifest_to_dir(&target, &recovery_manifest)?;
    Ok(())
}

fn unique_recovery_dir(paths: &CodexPaths, snapshot_id: &str) -> PathBuf {
    let safe = sanitize_path_segment(snapshot_id);
    let mut candidate = paths.recovery_snapshots_dir.join(&safe);
    let mut idx = 2;
    while candidate.exists() {
        candidate = paths.recovery_snapshots_dir.join(format!("{safe}-{idx}"));
        idx += 1;
    }
    candidate
}

fn sanitize_path_segment(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        Local::now().format("%Y%m%dT%H%M%S%3f").to_string()
    } else {
        trimmed.to_owned()
    }
}

fn snapshot_dirs_under(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && manifest_path(path).exists())
        .collect()
}

fn info_from_manifest(manifest: SnapshotManifest, kind: &str) -> SnapshotInfo {
    SnapshotInfo {
        id: manifest.snapshot_id,
        kind: kind.to_owned(),
        snapshot_at: manifest.snapshot_at,
        config_existed: manifest.config_existed,
        auth_existed: manifest.auth_existed,
        app_version: manifest.app_version,
        provider_name: manifest.provider_name,
        current_session: manifest.session_id == current_session_id(),
    }
}

fn normalize_manifest(
    mut manifest: SnapshotManifest,
    fallback_id: &str,
    fallback_session_id: &str,
) -> SnapshotManifest {
    if manifest.schema_version == 0 {
        manifest.schema_version = default_schema_version();
    }
    if manifest.snapshot_id.is_empty() {
        manifest.snapshot_id = fallback_id.to_owned();
    }
    if manifest.session_id.is_empty() {
        manifest.session_id = fallback_session_id.to_owned();
    }
    manifest
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("manifest.json")
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.toml")
}

fn auth_path(dir: &Path) -> PathBuf {
    dir.join("auth.json")
}

fn dir_name(dir: &Path) -> Option<String> {
    dir.file_name()
        .map(|name| name.to_string_lossy().to_string())
}

/// 解析快照 config.toml 中某个 root key 的原始字面量(包含引号等)。
/// 返回 `None` 表示快照里**没有**这个 key,`Some(literal)` 表示快照里此 key
/// 的字面量(可能包含两侧引号、整数无引号等);该字面量可直接喂回
/// [`crate::toml_sync::sync_root_value`]。
pub(crate) fn snapshot_toml_value_literal(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let stripped = line.trim_start();
        if !stripped.starts_with(key) {
            continue;
        }
        let after = &stripped[key.len()..];
        // 必须是 `key=...` 或 `key <空白> ...=...` 形式
        let mut rest = after.trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        rest = rest[1..].trim_start();
        // 去掉行末注释(`# ...`)
        if let Some(idx) = rest.find('#') {
            rest = rest[..idx].trim_end();
        }
        let trimmed = rest.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        return Some(trimmed.to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_with_tmp() -> (tempfile::TempDir, CodexPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = CodexPaths::from_home_dir(tmp.path());
        (tmp, paths)
    }

    #[test]
    fn snapshot_when_neither_file_exists() {
        let (_t, paths) = paths_with_tmp();
        let m = snapshot_codex_state(&paths, "v2.0.0-stage2.5", "Mock").unwrap();
        assert!(!m.config_existed);
        assert!(!m.auth_existed);
        assert!(has_snapshot(&paths));
        assert!(read_snapshot_config(&paths).is_none());
        assert_eq!(
            read_snapshot_auth(&paths),
            serde_json::Value::Object(Default::default())
        );
    }

    #[test]
    fn snapshot_copies_existing_files() {
        let (_t, paths) = paths_with_tmp();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(&paths.config_toml, "openai_base_url = \"existing\"\n").unwrap();
        std::fs::write(&paths.auth_json, "{\"OPENAI_API_KEY\":\"existing\"}\n").unwrap();
        let m = snapshot_codex_state(&paths, "v", "Mock").unwrap();
        assert!(m.config_existed);
        assert!(m.auth_existed);
        assert_eq!(
            read_snapshot_config(&paths).unwrap(),
            "openai_base_url = \"existing\"\n"
        );
    }

    #[test]
    fn snapshot_is_idempotent() {
        let (_t, paths) = paths_with_tmp();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(&paths.config_toml, "old\n").unwrap();
        snapshot_codex_state(&paths, "v", "Mock").unwrap();
        // 改了 config.toml,再 snapshot 一次 —— 不应覆盖原始备份
        std::fs::write(&paths.config_toml, "new\n").unwrap();
        snapshot_codex_state(&paths, "v", "Mock").unwrap();
        assert_eq!(
            read_snapshot_config(&paths).unwrap(),
            "old\n",
            "首次快照后再次调用必须保留原始备份"
        );
    }

    #[test]
    fn drop_snapshot_clears_dir() {
        let (_t, paths) = paths_with_tmp();
        snapshot_codex_state(&paths, "v", "Mock").unwrap();
        assert!(has_snapshot(&paths));
        drop_snapshot(&paths).unwrap();
        assert!(!has_snapshot(&paths));
    }

    #[test]
    fn stale_active_snapshot_moves_to_recovery_before_new_snapshot() {
        let (_t, paths) = paths_with_tmp();
        let stale_dir = paths.active_snapshots_dir.join("old-session");
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::write(config_path(&stale_dir), "openai_base_url = \"original\"\n").unwrap();
        std::fs::write(
            manifest_path(&stale_dir),
            serde_json::to_string(&SnapshotManifest {
                schema_version: SNAPSHOT_SCHEMA_VERSION,
                snapshot_id: "old-session".to_owned(),
                session_id: "old-session".to_owned(),
                snapshot_at: "2026-05-15T01:00:00".to_owned(),
                config_existed: true,
                auth_existed: false,
                app_version: "v-old".to_owned(),
                provider_name: Some("Old".to_owned()),
            })
            .unwrap(),
        )
        .unwrap();

        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(&paths.config_toml, "openai_base_url = \"current\"\n").unwrap();
        snapshot_codex_state(&paths, "v-new", "New").unwrap();

        assert!(!stale_dir.exists());
        let snapshots = list_snapshots(&paths);
        assert!(snapshots
            .iter()
            .any(|s| s.kind == "active" && s.app_version == "v-new"));
        assert!(snapshots
            .iter()
            .any(|s| s.kind == "recovery" && s.id == "old-session"));
    }

    #[test]
    fn recovery_snapshot_ids_follow_unique_target_dirs() {
        let (_t, paths) = paths_with_tmp();
        let first_dir = paths.active_snapshots_dir.join("first-session");
        let second_dir = paths.active_snapshots_dir.join("second-session");

        for (dir, config) in [
            (&first_dir, "openai_base_url = \"first\"\n"),
            (&second_dir, "openai_base_url = \"second\"\n"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(config_path(dir), config).unwrap();
            std::fs::write(
                manifest_path(dir),
                serde_json::to_string(&SnapshotManifest {
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                    snapshot_id: "old-session".to_owned(),
                    session_id: dir_name(dir).unwrap(),
                    snapshot_at: "2026-05-15T01:00:00".to_owned(),
                    config_existed: true,
                    auth_existed: false,
                    app_version: "v-old".to_owned(),
                    provider_name: Some("Old".to_owned()),
                })
                .unwrap(),
            )
            .unwrap();
        }

        move_snapshot_dir_to_recovery(&paths, &first_dir).unwrap();
        move_snapshot_dir_to_recovery(&paths, &second_dir).unwrap();

        let snapshots = list_snapshots(&paths);
        assert!(snapshots
            .iter()
            .any(|s| s.kind == "recovery" && s.id == "old-session"));
        assert!(snapshots
            .iter()
            .any(|s| s.kind == "recovery" && s.id == "old-session-2"));
        assert_eq!(
            read_snapshot_config_by_id(&paths, "old-session-2").unwrap(),
            "openai_base_url = \"second\"\n"
        );
    }

    #[test]
    fn snapshot_toml_value_literal_extracts() {
        let s = "# c\nopenai_base_url = \"http://x\"\nfoo = 1\n";
        assert_eq!(
            snapshot_toml_value_literal(s, "openai_base_url"),
            Some("\"http://x\"".to_owned())
        );
        assert_eq!(snapshot_toml_value_literal(s, "foo"), Some("1".to_owned()));
        assert_eq!(snapshot_toml_value_literal(s, "missing"), None);
    }

    #[test]
    fn snapshot_toml_value_literal_strips_inline_comment() {
        let s = "openai_base_url = \"http://x\" # comment\n";
        assert_eq!(
            snapshot_toml_value_literal(s, "openai_base_url"),
            Some("\"http://x\"".to_owned())
        );
    }
}
