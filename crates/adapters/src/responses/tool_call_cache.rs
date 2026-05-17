//! Tool call definition cache —— call_id → (name, arguments)。
//!
//! Codex CLI 多轮工具调用时把 `function_call_output` 用 `call_id` 关联回上一
//! 轮的 `function_call`,但当 history 被压缩 / 用户截断时,前一条 assistant
//! 可能已经不在 messages 里。Chat Completions 上游(Kimi / DeepSeek 实测)对
//! "孤儿 tool message"零容忍,会直接 400。
//!
//! 本缓存对应改造前 Python `session_cache.py::ToolCallCache`(内嵌在
//! ResponseSessionCache 模块里)+ 改造前 `responses_adapter.py::_repair_tool_call_ids`
//! 的 path B(查 cache → 重建 tool_call → 注回前 assistant / 插占位
//! assistant),也对齐 litellm 1.84.0 `transformation.py::
//! _ensure_tool_results_have_corresponding_tool_calls`。
//!
//! 写时机:Chat → Responses 流的 `converter.rs::close_tool_call`,工具调用
//! 闭合(收齐 name + arguments)时把 `(call_id, name, args)` 写入。
//! gemini_native::response.rs::emit_function_call 同样路径写入(共享缓存)。
//!
//! 读时机:Responses → Chat 请求侧 `request.rs::repair_tool_call_ids`,以及
//! gemini_native::request.rs::function_call_output 处理。
//!
//! ## 持久化(2026-05-11 加)
//!
//! 默认 `$HOME/.codex-app-transfer/tool_call_cache.json` 持久化,**跨进程重启
//! 保留** call_id → (name, args) 映射。这样:
//! - 用户重启 app(eg dev rebuild),Codex.app 用 previous_response_id 续话时
//!   不会因 cache 丢失报"no matching prior function_call"
//! - **切换 provider(eg gemini-cli OAuth → antigravity OAuth)** 时同一个
//!   conversation 的 call_id 仍能反查到 name,会话不丢失(user 反馈核心需求)
//!
//! 同步写策略:每次 `save()` 后**同步原子写**(temp + rename)。Cache size
//! 上限 1000 entries,每条 ~200 bytes,JSON 总 ~200KB,sync write 延迟 ms 级
//! 可接受(tool call 频率本来就不高)。Best-effort:写盘失败只 warn,
//! in-memory cache 不受影响。
//!
//! ## ⚠️ Single-writer semantics
//!
//! 当前实现**只保证 process 内的 in-memory cache 一致**,disk 文件采用
//! "**last-writer-wins**" 语义。如果用户同时跑**多个 .app 实例**(eg dev
//! rebuild 时旧的还没退、用户故意开 2 个),它们 share 同一份
//! `~/.codex-app-transfer/tool_call_cache.json`,**新写的 entry 可能被另一
//! 实例的 stale snapshot 覆盖**。In-memory 仍正常,只是 disk 不可靠。
//!
//! 不建议跨进程并发跑。要彻底防 race 需要 `fs2::FileExt::lock_exclusive`
//! 包裹 read-merge-write,scope 比较大,follow-up PR 实现。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEntry {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    tool_call: ToolCallEntry,
    /// UNIX epoch millis(改 SystemTime 而非 Instant 以支持序列化 + 跨重启)。
    /// 系统时钟跳变(eg NTP 校准)可能导致 entry 提前 / 推迟过期 ±
    /// (jump amount),但 cache 本来 best-effort,可接受
    inserted_at_ms: u64,
    #[serde(default)]
    access_count: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PersistedCache {
    /// schema 版本,break change 时 bump 用 — 当前 v1
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    entries: HashMap<String, CacheEntry>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug)]
struct ToolCallCacheInner {
    entries: HashMap<String, CacheEntry>,
}

#[derive(Debug)]
pub struct ToolCallCache {
    max_size: usize,
    ttl: Duration,
    inner: Mutex<ToolCallCacheInner>,
    /// 持久化 JSON 文件路径。`None` = 纯内存模式(单测 / 显式 disable)
    persist_path: Option<PathBuf>,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ToolCallCache {
    /// 纯内存 cache(单测 / 不需要持久化场景用)。
    pub fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            max_size: max_size.max(1),
            ttl,
            inner: Mutex::new(ToolCallCacheInner {
                entries: HashMap::new(),
            }),
            persist_path: None,
        }
    }

    /// 带持久化的 cache。启动时尝试 load 已有 entries,后续 `save()` 同步落盘。
    /// load 失败 / 解析失败时空 cache 启动 + tracing::warn,不 fail 进程。
    pub fn with_persistence(max_size: usize, ttl: Duration, path: PathBuf) -> Self {
        let entries = match Self::load_from_disk(&path, ttl) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    error_id = "TOOL_CALL_CACHE_LOAD_FAIL",
                    path = %path.display(),
                    error = %e,
                    "tool call cache: 加载持久化文件失败 — 空 cache 启动 \
                     (会话恢复路径可能拿不到 call_id 反查)"
                );
                HashMap::new()
            }
        };
        Self {
            max_size: max_size.max(1),
            ttl,
            inner: Mutex::new(ToolCallCacheInner { entries }),
            persist_path: Some(path),
        }
    }

    pub fn save(&self, call_id: &str, tool_call: ToolCallEntry) {
        if call_id.trim().is_empty() {
            return;
        }
        let snapshot = {
            let mut inner = self.inner.lock().expect("tool call cache mutex poisoned");
            self.evict_expired_locked(&mut inner);
            if inner.entries.len() >= self.max_size && !inner.entries.contains_key(call_id) {
                self.evict_oldest_locked(&mut inner);
            }
            inner.entries.insert(
                call_id.to_owned(),
                CacheEntry {
                    tool_call,
                    inserted_at_ms: now_unix_ms(),
                    access_count: 0,
                },
            );
            // 拿 entries 的 snapshot(clone)给 disk write 用,持锁时间最短
            inner.entries.clone()
        };
        // 锁外做 disk IO,不阻塞其他 save / get
        if let Some(path) = self.persist_path.as_ref() {
            if let Err(e) = Self::write_to_disk(path, &snapshot) {
                tracing::warn!(
                    error_id = "TOOL_CALL_CACHE_SAVE_FAIL",
                    path = %path.display(),
                    error = %e,
                    "tool call cache: 持久化写入失败 — in-memory 仍 OK \
                     (但下次重启此 entry 会丢)"
                );
            }
        }
    }

    pub fn get(&self, call_id: &str) -> Option<ToolCallEntry> {
        if call_id.trim().is_empty() {
            return None;
        }
        let mut inner = self.inner.lock().expect("tool call cache mutex poisoned");
        let now = now_unix_ms();
        let ttl_ms = self.ttl.as_millis() as u64;
        let expired = inner
            .entries
            .get(call_id)
            .map(|entry| now.saturating_sub(entry.inserted_at_ms) > ttl_ms)
            .unwrap_or(false);
        if expired {
            inner.entries.remove(call_id);
            return None;
        }
        let entry = inner.entries.get_mut(call_id)?;
        entry.access_count += 1;
        Some(entry.tool_call.clone())
    }

    pub fn clear(&self) {
        self.inner
            .lock()
            .expect("tool call cache mutex poisoned")
            .entries
            .clear();
        // 同步删 disk 文件(idempotent)
        if let Some(path) = self.persist_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }

    fn evict_expired_locked(&self, inner: &mut ToolCallCacheInner) {
        let now = now_unix_ms();
        let ttl_ms = self.ttl.as_millis() as u64;
        inner
            .entries
            .retain(|_, entry| now.saturating_sub(entry.inserted_at_ms) <= ttl_ms);
    }

    fn evict_oldest_locked(&self, inner: &mut ToolCallCacheInner) {
        let Some(oldest_key) = inner
            .entries
            .iter()
            .min_by_key(|(_, entry)| (entry.access_count, entry.inserted_at_ms))
            .map(|(key, _)| key.clone())
        else {
            return;
        };
        inner.entries.remove(&oldest_key);
    }

    fn load_from_disk(
        path: &Path,
        ttl: Duration,
    ) -> Result<HashMap<String, CacheEntry>, std::io::Error> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // 首次启动 / 用户清过缓存 — 正常路径,空 cache
                return Ok(HashMap::new());
            }
            Err(e) => return Err(e),
        };
        let parsed: PersistedCache = serde_json::from_slice(&bytes).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("tool_call_cache.json schema parse: {e}"),
            )
        })?;
        // **schema version forward-compat 检查**(code-reviewer M1 修):
        // 当前只支持 v1。未来 v2 可能加新字段(eg `args` 改 schema)。如果
        // load 不匹配的 version,**主动 fail**(走 corrupt-fallback)而不是
        // silent 当 v1 处理 — 否则:
        //   - downgrade (v2 file → v1 binary):新字段被 v1 load 时丢,save 时
        //     全文件覆盖 → 信息永久丢
        //   - 旧 binary 看不懂新 entry 但 happy-path 跑 → 用户看不出来
        const SUPPORTED_VERSION: u32 = 1;
        if parsed.version != SUPPORTED_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "tool_call_cache.json schema version mismatch: 文件 v{} \
                     当前 binary 只支持 v{SUPPORTED_VERSION}。建议:(a) 升级 app \
                     binary;(b) 删该文件让 cache 重建(会话恢复历史丢)",
                    parsed.version
                ),
            ));
        }
        // 顺手 evict 已过期 entry(避免给内存填充无用条目)
        let now = now_unix_ms();
        let ttl_ms = ttl.as_millis() as u64;
        let entries: HashMap<String, CacheEntry> = parsed
            .entries
            .into_iter()
            .filter(|(_, e)| now.saturating_sub(e.inserted_at_ms) <= ttl_ms)
            .collect();
        Ok(entries)
    }

    /// 原子写:写到 `<path>.tmp` 再 rename 到 `<path>`(防写中崩溃留半截文件)。
    /// 复用 `TokenStore::save` 同款 pattern
    fn write_to_disk(
        path: &Path,
        entries: &HashMap<String, CacheEntry>,
    ) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let payload = PersistedCache {
            version: 1,
            entries: entries.clone(),
        };
        let json = serde_json::to_vec(&payload).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("serialize tool_call_cache: {e}"),
            )
        })?;
        let tmp = path.with_extension("json.tmp");
        // tmp 残留则删(只吞 NotFound,其他 IO 错抛)
        match std::fs::remove_file(&tmp) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

/// 默认持久化路径:`$HOME/.codex-app-transfer/tool_call_cache.json`。
/// HOME 不可用(eg sandboxed test runner)→ 回退纯内存
pub fn global_tool_call_cache() -> &'static ToolCallCache {
    static CACHE: OnceLock<ToolCallCache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let cap = 1000;
        let ttl = Duration::from_secs(3600);
        match std::env::var_os("HOME") {
            Some(home) => {
                let path = PathBuf::from(home)
                    .join(".codex-app-transfer")
                    .join("tool_call_cache.json");
                ToolCallCache::with_persistence(cap, ttl, path)
            }
            None => {
                tracing::warn!(
                    error_id = "TOOL_CALL_CACHE_NO_HOME",
                    "HOME 未设置,tool call cache 退到纯内存模式 \
                     (跨重启会话恢复不可用)"
                );
                ToolCallCache::new(cap, ttl)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_and_get_round_trip() {
        let cache = ToolCallCache::new(8, Duration::from_secs(60));
        cache.save(
            "call_a",
            ToolCallEntry {
                name: "search".into(),
                arguments: r#"{"q":"foo"}"#.into(),
            },
        );
        let entry = cache.get("call_a").expect("cache should hit");
        assert_eq!(entry.name, "search");
        assert_eq!(entry.arguments, r#"{"q":"foo"}"#);
    }

    #[test]
    fn empty_call_id_is_ignored_on_save_and_get() {
        let cache = ToolCallCache::new(8, Duration::from_secs(60));
        cache.save(
            "",
            ToolCallEntry {
                name: "noop".into(),
                arguments: String::new(),
            },
        );
        assert!(cache.get("").is_none());
    }

    #[test]
    fn lru_eviction_drops_least_used_oldest() {
        let cache = ToolCallCache::new(2, Duration::from_secs(60));
        cache.save(
            "call_1",
            ToolCallEntry {
                name: "a".into(),
                arguments: "{}".into(),
            },
        );
        cache.save(
            "call_2",
            ToolCallEntry {
                name: "b".into(),
                arguments: "{}".into(),
            },
        );
        // 给 call_2 提访问计数
        let _ = cache.get("call_2");
        // 插第三条触发淘汰,call_1(0 访问)被踢
        cache.save(
            "call_3",
            ToolCallEntry {
                name: "c".into(),
                arguments: "{}".into(),
            },
        );
        assert!(cache.get("call_1").is_none());
        assert!(cache.get("call_2").is_some());
        assert!(cache.get("call_3").is_some());
    }

    #[test]
    fn ttl_expired_entry_is_purged_on_get() {
        let cache = ToolCallCache::new(8, Duration::from_millis(1));
        cache.save(
            "call_x",
            ToolCallEntry {
                name: "search".into(),
                arguments: "{}".into(),
            },
        );
        std::thread::sleep(Duration::from_millis(10));
        assert!(cache.get("call_x").is_none());
    }

    /// **持久化核心 test**:save 后 disk 有 JSON;新 cache 实例从同 path load
    /// 能拿回原 entry — 模拟 app 重启 / 切 provider 后会话恢复
    #[test]
    fn persistence_round_trip_via_disk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.json");
        let cache_a = ToolCallCache::with_persistence(8, Duration::from_secs(60), path.clone());
        cache_a.save(
            "call_resume",
            ToolCallEntry {
                name: "notion_search".into(),
                arguments: r#"{"q":"开发"}"#.into(),
            },
        );
        // 此时 disk 应该有文件
        assert!(path.exists(), "save 后必须 atomic write 到 disk");

        // 模拟新进程启动:新 cache 实例从同 path load
        let cache_b = ToolCallCache::with_persistence(8, Duration::from_secs(60), path.clone());
        let entry = cache_b
            .get("call_resume")
            .expect("重启后必须能从 disk 拿回 entry");
        assert_eq!(entry.name, "notion_search");
        assert_eq!(entry.arguments, r#"{"q":"开发"}"#);
    }

    /// 持久化 cache load 时已过期 entry 自动 evict(防内存填无用)
    #[test]
    fn load_drops_expired_entries() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.json");
        // 写一个已过期的 entry(inserted_at_ms = 0,远古)
        let mut entries = HashMap::new();
        entries.insert(
            "old".to_owned(),
            CacheEntry {
                tool_call: ToolCallEntry {
                    name: "x".into(),
                    arguments: "{}".into(),
                },
                inserted_at_ms: 0,
                access_count: 0,
            },
        );
        ToolCallCache::write_to_disk(&path, &entries).unwrap();

        let cache = ToolCallCache::with_persistence(8, Duration::from_secs(60), path);
        assert!(
            cache.get("old").is_none(),
            "过期 entry 应被 load 时 filter 掉"
        );
    }

    /// 损坏的 JSON 文件不让进程崩,启动空 cache + warn
    #[test]
    fn corrupt_json_file_falls_back_to_empty_cache() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.json");
        std::fs::write(&path, b"{not valid json").unwrap();
        let cache = ToolCallCache::with_persistence(8, Duration::from_secs(60), path);
        // 空 cache + 不 panic
        assert!(cache.get("anything").is_none());
        // 之后还能正常 save / get
        cache.save(
            "ok",
            ToolCallEntry {
                name: "n".into(),
                arguments: "{}".into(),
            },
        );
        assert!(cache.get("ok").is_some());
    }

    /// version 不匹配的 JSON 文件视作 corrupt(走 InvalidData fallback)
    /// 防 forward/backward 不兼容时 silently 当 v1 处理丢字段
    #[test]
    fn schema_version_mismatch_treated_as_corrupt() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.json");
        // 写一个 version=99 的 JSON(未来 v2 file 被 v1 binary 读到)
        std::fs::write(
            &path,
            br#"{"version": 99, "entries": {"call_x": {"tool_call": {"name": "n", "arguments": "{}"}, "inserted_at_ms": 1000, "access_count": 0}}}"#,
        )
        .unwrap();
        // load 应失败 → with_persistence fallback 到空 cache(warn + 不 panic)
        let cache = ToolCallCache::with_persistence(8, Duration::from_secs(60), path);
        assert!(
            cache.get("call_x").is_none(),
            "version 不匹配应触发 corrupt fallback,而非 silent load 当 v1 处理"
        );
    }

    /// `clear()` 同步删 disk 文件(idempotent — 文件不存在不报错)
    #[test]
    fn clear_removes_disk_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cache.json");
        let cache = ToolCallCache::with_persistence(8, Duration::from_secs(60), path.clone());
        cache.save(
            "x",
            ToolCallEntry {
                name: "n".into(),
                arguments: "{}".into(),
            },
        );
        assert!(path.exists());
        cache.clear();
        assert!(!path.exists(), "clear 必须删 disk 文件");
    }
}
