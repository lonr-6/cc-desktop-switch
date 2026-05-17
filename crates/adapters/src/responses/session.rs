//! Responses API conversation session cache —— **双层**(2026-05-08+):
//!
//! - **L1 in-memory hot cache**(`HashMap` + LRU + TTL):热路径零 IO,跟 v1.x
//!   行为一致(1000 条 × 60 分钟 LRU,可 `with_capacity` 调整)。
//! - **L2 sqlite write-through**(默认 `~/.codex-app-transfer/sessions.db`,30
//!   天 TTL):**进程重启不丢历史**。Codex CLI 用旧 `previous_response_id` 续轮
//!   时,L1 miss → 从 L2 查回 → 升温到 L1。
//!
//! 写入双写;读取先 L1 后 L2。L2 任何 IO 错误 → log warning 后退到纯 L1,代理
//! 仍能对外服务(只是丢"重启不丢历史"这个新增能力)。
//!
//! ## 数据治理
//!
//! - **Schema version**:启动时检查 `sessions_meta` 表里的 `schema_version`,
//!   不匹配 / 表不存在 → 备份现 db 为 `sessions.db.bak.<unix-ts>` 然后重建空
//!   db(用户丢历史一次,但能正常运行;重启场景跟 PR 1 修的 cache miss 相同 →
//!   返回 OpenAI SDK 兼容 400)。**当前 schema_version = 1**。
//! - **TTL** 30 天:启动时 `DELETE WHERE last_access_unix < now - 30d` 清过期。
//! - **隐私**:db 文件包含完整对话历史(messages JSON),用户可:(a) admin
//!   endpoint `POST /api/sessions/clear` 全清;(b) 直接删 db 文件。README 在
//!   隐私小节里写明。
//! - **并发**:rusqlite `Connection` 不是 Sync,用 `Mutex` 包。WAL + synchronous
//!   NORMAL 提升单写多读性能;sqlite 自身保证原子性,不需要额外 transaction。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// 当前 sqlite schema 版本。改 schema 时把这个值 +1,旧 db 会被自动备份重建。
const SCHEMA_VERSION: i64 = 1;

/// 默认 L2(sqlite)TTL — 30 天,跟 OpenAI Responses API 服务端 30 天 retention
/// 对齐(`docs/guides/conversation-state`)。
const DEFAULT_PERSISTED_TTL: Duration = Duration::from_secs(30 * 24 * 3600);

/// L1 默认尺寸 / TTL,保留 v1.x 行为(1000 条 × 60 分钟 LRU)。
const DEFAULT_L1_SIZE: usize = 1000;
const DEFAULT_L1_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone)]
struct SessionEntry {
    messages: Vec<Value>,
    ts: Instant,
    access_count: u64,
}

#[derive(Debug)]
struct SessionCacheInner {
    entries: HashMap<String, SessionEntry>,
}

#[derive(Debug)]
pub struct ResponseSessionCache {
    /// L1 内存层 LRU 容量上限。
    max_size: usize,
    /// L1 内存层 TTL。
    ttl: Duration,
    /// L2 sqlite TTL(从 last_access_unix 起算)。仅在 db 启用时生效。
    persisted_ttl: Duration,
    inner: Mutex<SessionCacheInner>,
    /// L2 sqlite 持久化层。`Some` = 启用 write-through;`None` = 纯内存
    /// (单元测试 / db 启动失败 fallback)。
    db: Mutex<Option<Connection>>,
}

impl ResponseSessionCache {
    /// 纯内存 cache(测试 / fallback 用),不持久化。
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            max_size: max_size.max(1),
            ttl,
            persisted_ttl: DEFAULT_PERSISTED_TTL,
            inner: Mutex::new(SessionCacheInner {
                entries: HashMap::new(),
            }),
            db: Mutex::new(None),
        }
    }

    /// 内存 + sqlite 双层。`db_path` 通常是 `~/.codex-app-transfer/sessions.db`。
    /// 任何 sqlite 初始化错误(权限 / IO / schema 不匹配)→ 回退到纯内存,**不**
    /// 让代理启动失败;并把错误 log 到调用方传入的 `on_error`。
    ///
    /// `persisted_ttl` 控制 L2 过期清理(默认 30 天)。
    pub fn with_db_path(
        max_size: usize,
        ttl: Duration,
        persisted_ttl: Duration,
        db_path: &Path,
    ) -> (Self, Option<String>) {
        let cache = Self {
            max_size: max_size.max(1),
            ttl,
            persisted_ttl,
            inner: Mutex::new(SessionCacheInner {
                entries: HashMap::new(),
            }),
            db: Mutex::new(None),
        };
        let warn = match init_db(db_path, persisted_ttl) {
            Ok(conn) => {
                *cache.db.lock().expect("session cache db mutex poisoned") = Some(conn);
                None
            }
            Err(e) => Some(format!(
                "sessions.db init failed at {}: {e} — falling back to in-memory only",
                db_path.display()
            )),
        };
        (cache, warn)
    }

    pub fn save(&self, response_id: &str, messages: Vec<Value>) {
        if response_id.trim().is_empty() {
            return;
        }

        // L1 写入(同步,纳秒级)
        {
            let mut inner = self.inner.lock().expect("session cache mutex poisoned");
            self.evict_expired_locked(&mut inner);
            if inner.entries.len() >= self.max_size && !inner.entries.contains_key(response_id) {
                self.evict_oldest_locked(&mut inner);
            }
            inner.entries.insert(
                response_id.to_owned(),
                SessionEntry {
                    messages: messages.clone(),
                    ts: Instant::now(),
                    access_count: 0,
                },
            );
        }

        // L2 sqlite write-through(毫秒级)。失败仅 log,不影响 L1。
        if let Err(e) = self.persist_save(response_id, &messages) {
            log_db_warning(
                "SESSIONS_DB_SAVE_FAILED",
                format!("save response_id={response_id} failed: {e}"),
            );
        }
    }

    pub fn get(&self, response_id: &str) -> Option<Vec<Value>> {
        if response_id.trim().is_empty() {
            return None;
        }

        // L1 先查
        {
            let mut inner = self.inner.lock().expect("session cache mutex poisoned");
            let expired = inner
                .entries
                .get(response_id)
                .map(|entry| entry.ts.elapsed() > self.ttl)
                .unwrap_or(false);
            if expired {
                inner.entries.remove(response_id);
            } else if let Some(entry) = inner.entries.get_mut(response_id) {
                entry.access_count += 1;
                return Some(entry.messages.clone());
            }
        }

        // L1 miss → L2 sqlite 查;命中则升温到 L1
        match self.persist_load(response_id) {
            Ok(Some(messages)) => {
                let mut inner = self.inner.lock().expect("session cache mutex poisoned");
                if inner.entries.len() >= self.max_size {
                    self.evict_oldest_locked(&mut inner);
                }
                inner.entries.insert(
                    response_id.to_owned(),
                    SessionEntry {
                        messages: messages.clone(),
                        ts: Instant::now(),
                        access_count: 1,
                    },
                );
                Some(messages)
            }
            Ok(None) => None,
            Err(e) => {
                log_db_warning(
                    "SESSIONS_DB_LOAD_FAILED",
                    format!("load response_id={response_id} failed: {e}"),
                );
                None
            }
        }
    }

    pub fn build_messages_with_history(
        &self,
        previous_response_id: &str,
        current_messages: &[Value],
    ) -> Vec<Value> {
        let mut out = Vec::new();
        if let Some(history) = self.get(previous_response_id) {
            out.extend(history);
        }
        out.extend(current_messages.iter().cloned());
        out
    }

    /// 仅清 L1 内存(老语义,保留供已有调用)。
    pub fn clear(&self) {
        self.inner
            .lock()
            .expect("session cache mutex poisoned")
            .entries
            .clear();
    }

    /// **彻底清除**:L1 内存 + L2 sqlite 表。给 admin endpoint
    /// `POST /api/sessions/clear` 用。返回清掉的 L2 行数(L1 总是清空)。
    pub fn clear_all_persisted(&self) -> Result<usize, String> {
        self.clear();
        let mut guard = self.db.lock().expect("session cache db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(0);
        };
        conn.execute("DELETE FROM response_sessions", [])
            .map_err(|e| {
                let detail = format!("clear failed: {e}");
                log_db_warning("SESSIONS_DB_CLEAR_FAILED", detail.clone());
                detail
            })
    }

    /// 启动时清 L2 过期 entry(`last_access_unix < now - persisted_ttl`)。
    pub fn evict_expired_persisted(&self) -> Result<usize, String> {
        let mut guard = self.db.lock().expect("session cache db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(0);
        };
        let cutoff = unix_now().saturating_sub(self.persisted_ttl.as_secs() as i64);
        conn.execute(
            "DELETE FROM response_sessions WHERE last_access_unix <= ?1",
            params![cutoff],
        )
        .map_err(|e| format!("sessions.db evict expired failed: {e}"))
    }

    fn persist_save(&self, response_id: &str, messages: &[Value]) -> rusqlite::Result<()> {
        let mut guard = self.db.lock().expect("session cache db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(());
        };
        // **silent-failure H1 修**:旧实现 `unwrap_or_else(|_| "[]")` 让编码失败
        // 静默覆盖原本有效的 row(`ON CONFLICT DO UPDATE`)→ 后续 get 返空历史 →
        // 用户视角是 silent context loss。新实现编码失败 warn + **跳过本次写入**
        // 让 L2 原 row 保留(L1 内存层已 save,本轮请求仍正常)。
        let json = match serde_json::to_string(messages) {
            Ok(s) => s,
            Err(e) => {
                log_db_warning(
                    "SESSIONS_DB_SAVE_ENCODE_FAILED",
                    format!(
                        "json encode failed for response_id={response_id}, \
                         skip L2 write to preserve any prior row: {e}"
                    ),
                );
                return Ok(());
            }
        };
        let now = unix_now();
        conn.execute(
            "INSERT INTO response_sessions \
             (response_id, messages_json, created_unix, last_access_unix, access_count) \
             VALUES (?1, ?2, ?3, ?3, 0) \
             ON CONFLICT(response_id) DO UPDATE SET \
                 messages_json = excluded.messages_json, \
                 last_access_unix = excluded.last_access_unix",
            params![response_id, json, now],
        )?;
        Ok(())
    }

    fn persist_load(&self, response_id: &str) -> rusqlite::Result<Option<Vec<Value>>> {
        let mut guard = self.db.lock().expect("session cache db mutex poisoned");
        let Some(conn) = guard.as_mut() else {
            return Ok(None);
        };
        let cutoff = unix_now().saturating_sub(self.persisted_ttl.as_secs() as i64);
        // 同时检查 last_access_unix 防越过 TTL 的脏读
        let row: Option<(String, i64)> = conn
            .query_row(
                "SELECT messages_json, last_access_unix FROM response_sessions \
                 WHERE response_id = ?1 AND last_access_unix > ?2",
                params![response_id, cutoff],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?;
        let Some((json, _last)) = row else {
            return Ok(None);
        };
        // 命中后更新 last_access_unix(顺手 +access_count 便于将来观测)
        // **silent-failure H2 修**:旧 `let _ = ...` 静默 UPDATE 失败 → row 的
        // `last_access_unix` 永远不前进 → 下次 `evict_expired_persisted` 把活会话
        // 当过期删 → 用户视角 history 凭空消失。新实现 warn 不 panic,let read
        // 路径仍 return 数据。
        let now = unix_now();
        if let Err(e) = conn.execute(
            "UPDATE response_sessions SET last_access_unix = ?1, access_count = access_count + 1 \
             WHERE response_id = ?2",
            params![now, response_id],
        ) {
            log_db_warning(
                "SESSIONS_DB_TOUCH_FAILED",
                format!(
                    "last_access_unix UPDATE failed for response_id={response_id} \
                     (read served from L2,but TTL refresh skipped — row 可能被 evict): {e}"
                ),
            );
        }
        match serde_json::from_str::<Vec<Value>>(&json) {
            Ok(messages) => Ok(Some(messages)),
            Err(parse_err) => {
                // 历史格式损坏 → 删除该行,当 miss 处理。
                // **F6 follow-up**(code-reviewer IMPORTANT #2 修):旧实现 silent
                // delete,operator 无从观测数据损坏率。新实现 emit warn 让磁盘
                // corruption / schema drift / 手工编辑 db 等异常路径有信号。
                let delete_result = conn.execute(
                    "DELETE FROM response_sessions WHERE response_id = ?1",
                    params![response_id],
                );
                log_db_warning(
                    "SESSIONS_DB_ROW_CORRUPT",
                    format!(
                        "messages_json parse failed for response_id={response_id} \
                         (delete_ok={}): {parse_err}",
                        delete_result.is_ok()
                    ),
                );
                Ok(None)
            }
        }
    }

    fn evict_expired_locked(&self, inner: &mut SessionCacheInner) {
        let ttl = self.ttl;
        inner.entries.retain(|_, entry| entry.ts.elapsed() <= ttl);
    }

    fn evict_oldest_locked(&self, inner: &mut SessionCacheInner) {
        let Some(oldest_key) = inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| (entry.access_count, entry.ts))
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        inner.entries.remove(&oldest_key);
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_else(|e| {
            // **silent-failure M4 修**:SystemTime pre-epoch(系统时钟漂移到
            // 1970 前)极罕见但会让 TTL evict 全 no-op + 新 row created_unix=0,
            // 默默丢失观测;此处 warn 一次保留信号。
            log_db_warning(
                "SESSIONS_DB_CLOCK_PRE_EPOCH",
                format!("SystemTime before UNIX_EPOCH: {e} — falling back to 0"),
            );
            0
        })
}

/// 初始化 sqlite db。schema 检测 + 不匹配时备份 + 重建。
fn init_db(db_path: &Path, _persisted_ttl: Duration) -> rusqlite::Result<Connection> {
    if let Some(parent) = db_path.parent() {
        // **silent-failure M1 修**:旧 `let _ = create_dir_all` 让 fs 错误被
        // 后续 `Connection::open` 的 generic sqlite error 掩盖。warn 让 operator
        // 知道根因是 fs perm / parent-is-file 还是 sqlite 真坏。
        if let Err(e) = std::fs::create_dir_all(parent) {
            log_db_warning(
                "SESSIONS_DB_PARENT_DIR_FAILED",
                format!(
                    "create_dir_all({}) failed: {e} — Connection::open 可能也会失败,\
                     真根因是 fs 不是 sqlite",
                    parent.display()
                ),
            );
        }
    }
    let conn = open_db_with_pragmas(db_path)?;
    if needs_rebuild(&conn)? {
        drop(conn);
        backup_corrupt_db(db_path);
        let conn = open_db_with_pragmas(db_path)?;
        create_schema(&conn)?;
        return Ok(conn);
    }
    create_schema_if_missing(&conn)?;
    Ok(conn)
}

fn open_db_with_pragmas(db_path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path)?;
    // WAL = 单写多读不互锁,显著提升并发读性能(代理常见用法)
    // synchronous=NORMAL = 放松 fsync 频率,在系统崩溃时可能丢最后几条但
    // 不会损坏 db。session cache 重建即可,可接受。
    // **silent-failure M2 修**:pragma 失败(read-only FS / 网络 mount)会让
    // doc-comment 的"WAL 单写多读"承诺破裂 + 并发降级,旧 `let _ =` 静默 → 没
    // 人看得到。emit warn 不 fail open(degrade 仍能跑,只是慢)。
    if let Err(e) = conn.pragma_update(None, "journal_mode", "WAL") {
        log_db_warning(
            "SESSIONS_DB_PRAGMA_FAILED",
            format!("pragma journal_mode=WAL failed: {e} — concurrent reads will block writes"),
        );
    }
    if let Err(e) = conn.pragma_update(None, "synchronous", "NORMAL") {
        log_db_warning(
            "SESSIONS_DB_PRAGMA_FAILED",
            format!("pragma synchronous=NORMAL failed: {e} — fsync frequency unchanged"),
        );
    }
    Ok(conn)
}

fn needs_rebuild(conn: &Connection) -> rusqlite::Result<bool> {
    // 表不存在 → 不算 corrupt,走 create_schema_if_missing
    let table_exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='sessions_meta'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if table_exists.is_none() {
        return Ok(false);
    }
    // 表存在 → 检查版本是否匹配
    let version: Option<i64> = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM sessions_meta WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    Ok(version != Some(SCHEMA_VERSION))
}

fn create_schema_if_missing(conn: &Connection) -> rusqlite::Result<()> {
    let table_exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type='table' AND name='response_sessions'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    if table_exists.is_none() {
        create_schema(conn)?;
    }
    Ok(())
}

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS response_sessions (
            response_id TEXT PRIMARY KEY,
            messages_json TEXT NOT NULL,
            created_unix INTEGER NOT NULL,
            last_access_unix INTEGER NOT NULL,
            access_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_last_access ON response_sessions(last_access_unix);
        CREATE TABLE IF NOT EXISTS sessions_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    conn.execute(
        "INSERT OR REPLACE INTO sessions_meta (key, value) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

fn backup_corrupt_db(db_path: &Path) {
    let bak = PathBuf::from(format!("{}.bak.{}", db_path.display(), unix_now()));
    // **silent-failure M3 修**:旧 `let _ = rename(...)` 静默 → rename 失败时
    // 后续会基于**原 corrupt db** 重建 schema(可能成功也可能挂);上层 warn
    // 文案仍宣称"backed up + rebuilding",撒谎给 operator。新实现区分两条
    // 路径,rename 失败时换 error_id 让 operator 看到真实状态。
    match std::fs::rename(db_path, &bak) {
        Ok(()) => log_db_warning(
            "SESSIONS_DB_SCHEMA_MISMATCH",
            format!(
                "schema mismatch at {} — backed up to {} and rebuilding empty db",
                db_path.display(),
                bak.display()
            ),
        ),
        Err(e) => log_db_warning(
            "SESSIONS_DB_BACKUP_FAILED",
            format!(
                "schema mismatch at {} but backup rename to {} failed: {e} — \
                 重建会基于 corrupt db,可能继续异常",
                db_path.display(),
                bak.display()
            ),
        ),
    }
}

/// L2 sqlite 失败 surface(F6 修):
///
/// 既走 `tracing::warn!`(带 **stable `error_id`** 字段)让 telemetry pipeline /
/// log search / 任何 tracing subscriber 都能定位 + 聚合,又**保留 `eprintln!`
/// 兜底** — proxy 启动时把 stderr 重定向到 `~/.codex-app-transfer/logs/proxy-
/// <date>.log`,即便 tracing subscriber 未配 / panic,这条日志仍能落盘。
///
/// `error_id` 是稳定 token,grep / metrics rollup 用,**不要随版本改**:
///
/// 上层失败:
/// - `SESSIONS_DB_INIT_FAILED` — sqlite 打不开 / schema 建不出来,fallback 纯内存
/// - `SESSIONS_DB_SAVE_FAILED` — write-through INSERT/UPDATE 失败(L1 未受影响)
/// - `SESSIONS_DB_LOAD_FAILED` — SELECT 失败,本次 get 返 None
/// - `SESSIONS_DB_SCHEMA_MISMATCH` — schema_version 不匹配,db 已备份重建
/// - `SESSIONS_DB_CLEAR_FAILED` — admin DELETE 失败(也通过 Result 返调用方)
/// - `SESSIONS_DB_EVICT_FAILED` — 过期清理失败(non-fatal,启动时 best-effort)
///
/// 子路径失败(silent-failure-hunter task 21 追加):
/// - `SESSIONS_DB_SAVE_ENCODE_FAILED` — `serde_json::to_string(messages)` 失败,
///   **跳过本次 L2 写入**保留原 row(防 silent context loss,H1)
/// - `SESSIONS_DB_TOUCH_FAILED` — read 命中后 `last_access_unix` UPDATE 失败,
///   row 可能被错 evict(H2)
/// - `SESSIONS_DB_ROW_CORRUPT` — `messages_json` parse 失败,该 row 已删(code-reviewer #2)
/// - `SESSIONS_DB_PARENT_DIR_FAILED` — `create_dir_all` 失败,sqlite 错误根因是 fs(M1)
/// - `SESSIONS_DB_PRAGMA_FAILED` — `journal_mode=WAL` / `synchronous=NORMAL` 失败,
///   并发 / fsync 行为退化(M2)
/// - `SESSIONS_DB_BACKUP_FAILED` — schema-mismatch 时 rename 失败,后续重建基于
///   corrupt db,可能继续异常(M3)
/// - `SESSIONS_DB_CLOCK_PRE_EPOCH` — `SystemTime::now() < UNIX_EPOCH`,TTL evict
///   退化为 no-op(M4)
///
/// `error_id` 必须用 `&'static str`(literal)以保跨版本稳定;`detail` 给人类
/// 阅读的上下文(path / error message),不进 metric label。
///
/// **deferred suggestion**(type-design-analyzer):升级为 `enum SessionDbErrorId`
/// 获取 compile-time enforcement。本 PR 暂用 literal 跟 codebase 其他
/// `tracing::warn!(error_id=...)` 用法一致(`tool_call_cache.rs` / `request.rs`
/// 同模式),若未来 error_id 数量翻倍或出现拼写漂移再升级。
fn log_db_warning(error_id: &'static str, detail: String) {
    tracing::warn!(error_id, detail = %detail, "sessions.db");
    // 兼容老路径:Tauri proxy log file 收 stderr,保留 eprintln 兜底防 tracing
    // subscriber 未初始化(早期启动期 / unit test)时丢日志。
    eprintln!("warning: [{error_id}] {detail}");
}

/// 全局单例,生产代理路径用。
///
/// 启动时尝试在 `~/.codex-app-transfer/sessions.db` 打开 sqlite;成功则双层模式,
/// 失败 fallback 纯内存(纯内存模式 ≈ v2.0.10 行为,跟 PR 1 配合表现仍正常 —
/// 只是丢"重启不丢历史"这个新增能力)。
pub fn global_response_session_cache() -> &'static ResponseSessionCache {
    static CACHE: OnceLock<ResponseSessionCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let db_path = codex_app_transfer_registry::sessions_db_file();
        match db_path {
            Some(path) => {
                let (cache, warn) = ResponseSessionCache::with_db_path(
                    DEFAULT_L1_SIZE,
                    DEFAULT_L1_TTL,
                    DEFAULT_PERSISTED_TTL,
                    &path,
                );
                if let Some(msg) = warn {
                    log_db_warning("SESSIONS_DB_INIT_FAILED", msg);
                }
                // 启动时清一遍 L2 过期 row;失败仅 log
                if let Err(e) = cache.evict_expired_persisted() {
                    log_db_warning("SESSIONS_DB_EVICT_FAILED", e);
                }
                cache
            }
            None => ResponseSessionCache::new(DEFAULT_L1_SIZE, DEFAULT_L1_TTL),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cache_restores_history_before_current_messages() {
        let cache = ResponseSessionCache::new(2, Duration::from_secs(60));
        cache.save(
            "resp_1",
            vec![
                json!({"role": "user", "content": "first"}),
                json!({"role": "assistant", "content": "answer"}),
            ],
        );

        let merged = cache
            .build_messages_with_history("resp_1", &[json!({"role": "user", "content": "next"})]);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0]["content"], "first");
        assert_eq!(merged[2]["content"], "next");
    }

    #[test]
    fn cache_evicts_least_used_oldest_entry() {
        let cache = ResponseSessionCache::new(2, Duration::from_secs(60));
        cache.save("resp_1", vec![json!({"role": "user", "content": "one"})]);
        cache.save("resp_2", vec![json!({"role": "user", "content": "two"})]);
        assert!(cache.get("resp_2").is_some());
        cache.save("resp_3", vec![json!({"role": "user", "content": "three"})]);

        // 纯内存模式下,L1 LRU 淘汰 resp_1
        assert!(cache.get("resp_1").is_none());
        assert!(cache.get("resp_2").is_some());
        assert!(cache.get("resp_3").is_some());
    }

    // ── L2 sqlite 持久化测试 ──────────────────────────────────────────

    fn fresh_db_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.db");
        (dir, path)
    }

    #[test]
    fn l2_sqlite_persists_across_cache_instances() {
        // 关键回归(2026-05-08):sqlite write-through 让 cache 实例销毁后,
        // 新建实例(模拟 Tauri 重启)仍能读回历史。修复 PR 1 cache miss 根因。
        let (_dir, path) = fresh_db_path();

        let (cache_a, warn_a) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        assert!(warn_a.is_none(), "首次开 db 应当成功,实际 warn: {warn_a:?}");
        cache_a.save(
            "resp_persisted",
            vec![
                json!({"role": "user", "content": "across-restart"}),
                json!({"role": "assistant", "content": "yes I remember"}),
            ],
        );
        // 模拟"进程退出":只 drop cache_a,db 文件留在磁盘
        drop(cache_a);

        let (cache_b, warn_b) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        assert!(warn_b.is_none());
        let restored = cache_b.get("resp_persisted").expect("L2 应当恢复历史");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0]["content"], "across-restart");
        assert_eq!(restored[1]["content"], "yes I remember");
    }

    #[test]
    fn l1_miss_warms_from_l2_then_serves_l1() {
        // L1 LRU 满淘汰后,L2 仍能命中并升温回 L1。
        let (_dir, path) = fresh_db_path();
        let (cache, _) = ResponseSessionCache::with_db_path(
            1, // L1 容量 1,任何新条目都会挤掉旧的
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        cache.save("resp_a", vec![json!({"role": "user", "content": "A"})]);
        cache.save("resp_b", vec![json!({"role": "user", "content": "B"})]);
        // resp_a 在 L1 已被淘汰(容量 1),但 L2 还在
        let restored = cache.get("resp_a").expect("L2 应能命中并升温到 L1");
        assert_eq!(restored[0]["content"], "A");
    }

    #[test]
    fn clear_all_persisted_wipes_both_layers() {
        let (_dir, path) = fresh_db_path();
        let (cache, _) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        cache.save("resp_x", vec![json!({"role": "user", "content": "x"})]);
        let cleared = cache.clear_all_persisted().unwrap();
        assert!(cleared >= 1, "至少清掉 1 行,实际 {cleared}");
        assert!(cache.get("resp_x").is_none());

        // 重新开实例确认 L2 也确实清了
        drop(cache);
        let (cache2, _) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        assert!(cache2.get("resp_x").is_none());
    }

    #[test]
    fn schema_mismatch_backs_up_and_rebuilds() {
        // 用 schema_version=999 的"假未来 db"模拟 schema 不匹配场景:
        // - init_db 应当备份它然后重建空 db
        // - 老数据丢,但新 cache 能正常工作
        let (_dir, path) = fresh_db_path();
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions_meta(key TEXT PRIMARY KEY, value TEXT NOT NULL); \
                 INSERT INTO sessions_meta VALUES('schema_version', '999');",
            )
            .unwrap();
        }

        let (cache, warn) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &path,
        );
        assert!(warn.is_none(), "重建路径应成功,不报 init 失败");
        // 备份文件应存在
        let parent = path.parent().unwrap();
        let bak_count = std::fs::read_dir(parent)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("sessions.db.bak.")
            })
            .count();
        assert_eq!(bak_count, 1, "应当生成一个备份文件");
        // 新 db 能正常写读
        cache.save(
            "resp_after_rebuild",
            vec![json!({"role": "user", "content": "ok"})],
        );
        let restored = cache.get("resp_after_rebuild").unwrap();
        assert_eq!(restored[0]["content"], "ok");
    }

    #[test]
    fn ttl_expired_l2_row_does_not_leak() {
        // L2 TTL 过期的 row 不能被 get 命中(即使 L1 没缓存它)
        let (_dir, path) = fresh_db_path();
        // persisted_ttl = 1s,save 后 sleep > 1s 应当 miss
        let (cache, _) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            Duration::from_secs(1),
            &path,
        );
        cache.save("resp_old", vec![json!({"role": "user", "content": "old"})]);
        std::thread::sleep(Duration::from_millis(2100));
        // L1 还可能没过期(L1 ttl 是 60s),先 clear L1 强制走 L2
        cache.clear();
        assert!(cache.get("resp_old").is_none(), "L2 TTL 过期应 miss");
    }

    #[test]
    fn evict_expired_persisted_removes_old_rows() {
        let (_dir, path) = fresh_db_path();
        let (cache, _) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            Duration::from_secs(1),
            &path,
        );
        cache.save("resp_old", vec![json!({"role": "user", "content": "old"})]);
        cache.save(
            "resp_old2",
            vec![json!({"role": "user", "content": "old2"})],
        );
        std::thread::sleep(Duration::from_millis(2100));
        let removed = cache.evict_expired_persisted().unwrap();
        assert!(removed >= 2, "至少清两条过期,实际 {removed}");
    }

    #[test]
    fn fallback_to_in_memory_when_db_path_invalid() {
        // 故意给一个不可写路径(目录不存在 + 父级也无法创建,平台无关的方式
        // 是用一个已存在文件作为目录前缀)。退一步用 /dev/null/sub 这种:
        // *nix 上 /dev/null 是字符设备不能当目录,会失败。Windows 上换成
        // 不可达盘符。我们这里用更稳的方式:故意把 path 设成一个**已存在
        // 的文件**当 parent dir,create_dir_all 会失败。
        let dir = tempfile::tempdir().unwrap();
        let blocker_file = dir.path().join("blocker");
        std::fs::write(&blocker_file, b"x").unwrap();
        // 现在 blocker_file 是文件;尝试把 sessions.db 放在它"下面"
        let bad_path = blocker_file.join("sessions.db");

        let (cache, warn) = ResponseSessionCache::with_db_path(
            8,
            Duration::from_secs(60),
            DEFAULT_PERSISTED_TTL,
            &bad_path,
        );
        // db 初始化必失败 → warn 应该有
        assert!(warn.is_some(), "不可达 db_path 必须 warn,得到 {warn:?}");
        // L1 仍工作
        cache.save("resp_x", vec![json!({"role": "user", "content": "x"})]);
        assert!(cache.get("resp_x").is_some());
    }
}
