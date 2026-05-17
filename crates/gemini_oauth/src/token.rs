//! OAuth token 数据模型 + 持久化。
//!
//! Token 存到 `~/.codex-app-transfer/gemini-oauth.json`,**故意**跟 gemini-cli
//! 官方 (`~/.gemini/oauth_creds.json`) 路径区分,避免:
//! - 用户同时跑 codex-app-transfer 和 gemini-cli 时 token 互相覆盖
//! - 我们升级字段(加 project_id / email / scopes)时污染 gemini-cli 自己的状态
//!
//! ## 字段对齐 Google `Credentials` 形态
//!
//! Google google-auth-library 用 `expiry_date` (UNIX **ms**-epoch) 而非 `expires_in`
//! (秒) — refresh response 拿到的是 `expires_in: <秒>`,我们写盘前必须转换。
//! 这是 wire-level 容易踩的坑(调研 docs 第 6 条)。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::constants::REFRESH_BUFFER_SECS;

#[derive(Debug, Error)]
pub enum TokenError {
    /// 用户主目录环境变量都缺失 —— Unix 上一般是 `$HOME`,Windows 上一般是
    /// `%USERPROFILE%`(GUI 进程下 `HOME` 通常没被设过)。变体名保留 `HomeNotSet`
    /// 以维持 ABI;消息覆盖两个 env var 让 Windows 报错也能自解释。
    #[error("无法定位 token 持久化目录:HOME 与 USERPROFILE 环境变量都未设置")]
    HomeNotSet,
    #[error("token 文件 IO 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("token JSON 序列化失败: {0}")]
    Serde(#[from] serde_json::Error),
}

/// 持久化的 OAuth 凭证(完整状态,包含 refresh_token + project_id)。
///
/// 字段命名 / shape 对齐 Google `Credentials` + gemini-cli `~/.gemini/oauth_creds.json`,
/// 同时扩展我们自己需要的 `project_id` / `email`(Cloud Code bootstrap 后写入)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct OauthToken {
    /// Bearer token 字面值,用于 `Authorization: Bearer <…>` header。
    pub access_token: String,
    /// Refresh token —— 长期有效(Google 默认不过期,除非用户主动 revoke)。
    pub refresh_token: String,
    /// `Bearer` 字面值。
    pub token_type: String,
    /// Token 过期时刻,**UNIX milliseconds epoch**。Google `Credentials` 形态。
    /// `should_refresh()` 用此判断(单一过期判定;原 `is_expired()` 已删,reviewer
    /// H2 dead-code 修)。
    pub expiry_date: i64,
    /// OAuth scope(空格分隔的字符串,从 Google 响应拿到原值)。
    pub scope: String,
    /// `id_token` —— 含 email / sub claims,用于 Cloud Code 鉴别用户身份。
    /// 不是 Bearer token,不进 Authorization header。可空(刷新后才填)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    /// 用户邮箱(从 id_token 解析或 userinfo 调用拿)—— UI 展示当前登录账号。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Cloud Code Assist 的 GCP project ID —— 首次 `loadCodeAssist` /
    /// `onboardUser` 完成后填入,后续请求 outer envelope 必带。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl OauthToken {
    /// `Authorization` header 字面值。
    pub fn auth_header(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }

    /// 是否应该**主动**触发 refresh —— `expiry_date` 之前 [`REFRESH_BUFFER_SECS`]
    /// 秒就 trigger,防请求到上游时 token 刚好过期(network race)。**单一过期判断**:
    /// `service::ensure_valid_access_token` 等所有调用点都用这个,不再单独存在
    /// `is_expired()`(reviewer H2 dead-code 修;曾存在另一异步性 inclusive 边界
    /// 让未来 caller 容易选错)。
    pub fn should_refresh(&self) -> bool {
        let now_ms = unix_now_ms();
        let buffer_ms = REFRESH_BUFFER_SECS * 1000;
        now_ms >= self.expiry_date.saturating_sub(buffer_ms)
    }
}

/// 当前 UNIX 时间(ms-epoch)。SystemTime 出错(系统时钟在 1970 之前)返 0。
fn unix_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Token 持久化句柄 —— 封装 `~/.codex-app-transfer/gemini-oauth.json` 路径
/// 解析 + atomic write(temp + rename)+ secure permissions(Unix 0600)。
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    /// 用 `<home>/.codex-app-transfer/gemini-oauth.json` 默认路径(gemini-cli)。
    /// `<home>` 解析优先级:`HOME` → `USERPROFILE`(Windows GUI 进程默认只设
    /// `USERPROFILE`,无 `HOME`,所以必须 fallback)。**新代码**(支持多 OAuth
    /// provider)请用 [`Self::for_token_filename`] 显式指定文件名;本 fn 等价
    /// `for_token_filename("gemini-oauth.json")`。
    pub fn from_home_env() -> Result<Self, TokenError> {
        Self::for_token_filename("gemini-oauth.json")
    }

    /// 用 `<home>/.codex-app-transfer/<filename>` 路径,让多个 OAuth provider
    /// 共存(eg `gemini-oauth.json` vs `antigravity-oauth.json`)。filename
    /// 来自 `OauthProviderConfig::token_filename`。`<home>` 解析委派给
    /// `codex_app_transfer_registry::paths::resolve_home`,workspace 内唯一
    /// 入口,统一 `HOME` → `USERPROFILE` 回退 + 空字符串当未设,**Windows GUI
    /// 进程一律走 `USERPROFILE`**,与 `CodexPaths` 等其它路径解析一致。
    pub fn for_token_filename(filename: &str) -> Result<Self, TokenError> {
        let home =
            codex_app_transfer_registry::paths::resolve_home().ok_or(TokenError::HomeNotSet)?;
        let path = home.join(".codex-app-transfer").join(filename);
        Ok(Self { path })
    }

    /// 显式指定路径(单测用)。
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 加载 token。文件不存在返 `Ok(None)`(首次启动正常路径),其他 IO 错才报。
    pub fn load(&self) -> Result<Option<OauthToken>, TokenError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let token: OauthToken = serde_json::from_slice(&bytes)?;
                Ok(Some(token))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(TokenError::Io(e)),
        }
    }

    /// 写 token —— 先写临时文件再 rename,保证写入是 atomic(防中途崩溃留半截
    /// 文件)。Unix 平台用 [`std::os::unix::fs::OpenOptionsExt`] **创建时即设
    /// 0600 权限**(原版先 fs::write 默认 0644 后 set_permissions 0600,中间窗口
    /// token 文件世界可读;reviewer H3 race 修)。
    pub fn save(&self, token: &OauthToken) -> Result<(), TokenError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(token)?;

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            // tmp 文件可能已经存在(上次崩溃残留)— 先删。**只吞 NotFound,
            // EACCES / 别的 IO 错必须 propagate**(silent-failure-hunter M5 修;
            // 原 `let _ =` 吞所有错,例如 parent dir mode 改了无权限 unlink 也
            // silent 跳过 → 后续 create_new 直接 EEXIST 失败但 root cause 已丢)
            match std::fs::remove_file(&tmp) {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(TokenError::Io(e)),
            }
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(&json)?;
            file.sync_all()?; // fsync 保证 rename 前数据落盘(防 crash race)
        }
        #[cfg(not(unix))]
        {
            // 非 Unix(Windows)用普通 write — Windows ACL 已默认限制 user 私有,
            // POSIX 0600 概念不直接映射
            std::fs::write(&tmp, &json)?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 删除 token(用户主动 logout / revoke)。文件不存在算成功(idempotent)。
    pub fn delete(&self) -> Result<(), TokenError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(TokenError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_token(expiry_offset_secs: i64) -> OauthToken {
        OauthToken {
            access_token: "ya29.test-access".into(),
            refresh_token: "1//test-refresh".into(),
            token_type: "Bearer".into(),
            expiry_date: unix_now_ms() + expiry_offset_secs * 1000,
            scope: "https://www.googleapis.com/auth/cloud-platform".into(),
            id_token: Some("ey.test.id".into()),
            email: Some("test@example.com".into()),
            project_id: None,
        }
    }

    #[test]
    fn auth_header_uses_token_type() {
        let token = fresh_token(3600);
        assert_eq!(token.auth_header(), "Bearer ya29.test-access");
    }

    #[test]
    fn should_refresh_triggers_60s_before_expiry() {
        // expiry 在 30s 后 — 已经进了 60s buffer 内,应该 refresh
        let close_to_expiry = fresh_token(30);
        assert!(close_to_expiry.should_refresh());

        // expiry 在 120s 后 — 还早,不该 refresh
        let comfortable = fresh_token(120);
        assert!(!comfortable.should_refresh());

        // 已过期 — 当然该 refresh
        let expired = fresh_token(-100);
        assert!(expired.should_refresh());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = TokenStore::at_path(dir.path().join("token.json"));
        let token = fresh_token(3600);

        assert_eq!(store.load().unwrap(), None, "首次 load 必须返 None");

        store.save(&token).unwrap();
        let loaded = store.load().unwrap().expect("save 后必能 load");
        assert_eq!(loaded, token);

        // delete 后 load 又 None
        store.delete().unwrap();
        assert_eq!(store.load().unwrap(), None);
    }

    #[test]
    fn save_creates_parent_dir() {
        let dir = TempDir::new().unwrap();
        // 路径多层不存在的父目录,必须自动 create_dir_all
        let store = TokenStore::at_path(dir.path().join("a/b/c/token.json"));
        let token = fresh_token(3600);
        store.save(&token).unwrap();
        assert!(store.load().unwrap().is_some());
    }

    #[test]
    fn load_returns_serde_error_on_corrupt_json() {
        // **pr-test-analyzer H1 修**:token 文件存在但 JSON 损坏(disk full /
        // user 手改)走 TokenError::Serde 路径,不静默当成"未登录"
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("token.json");
        std::fs::write(&path, b"{not json").unwrap();
        let store = TokenStore::at_path(&path);
        let err = store.load().unwrap_err();
        assert!(
            matches!(err, TokenError::Serde(_)),
            "corrupt JSON 必须返 Serde 错让 caller 看到致命错,实际:{err:?}"
        );
    }

    #[test]
    fn delete_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = TokenStore::at_path(dir.path().join("token.json"));
        // 删不存在的文件不报错
        store.delete().unwrap();
        store.delete().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_unix_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let store = TokenStore::at_path(dir.path().join("token.json"));
        store.save(&fresh_token(3600)).unwrap();

        let meta = std::fs::metadata(store.path()).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "token 文件必须 0600,实际 {mode:o}");
    }

    #[test]
    fn homenotset_display_lists_both_env_vars() {
        // 公共可观察契约:`HomeNotSet` 的 `Display` 必须同时点名 HOME +
        // USERPROFILE,Windows 用户在前端 status warning 才能从字面理解错因
        // (本次 fix 的核心 UX 契约)。变体名 `HomeNotSet` 保留以维持 ABI,
        // 只 pin message 文本里两个 env var 名。
        let err = TokenError::HomeNotSet;
        let msg = err.to_string();
        assert!(
            msg.contains("HOME") && msg.contains("USERPROFILE"),
            "HomeNotSet display 必须同时提到 HOME 和 USERPROFILE,实际:{msg}"
        );
    }

    #[test]
    fn token_serde_skips_none_optional_fields() {
        let mut token = fresh_token(3600);
        token.id_token = None;
        token.email = None;
        token.project_id = None;

        let json = serde_json::to_string(&token).unwrap();
        // 三个 None 字段都不该出现在 JSON 里
        assert!(!json.contains("id_token"), "json 不应含 id_token: {json}");
        assert!(!json.contains("email"), "json 不应含 email: {json}");
        assert!(
            !json.contains("project_id"),
            "json 不应含 project_id: {json}"
        );
        // 必填字段必须有
        assert!(json.contains("access_token"));
        assert!(json.contains("refresh_token"));
        assert!(json.contains("expiry_date"));
    }
}
