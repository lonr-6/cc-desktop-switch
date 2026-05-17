//! `parentResponseId` DAG 多轮锚定 in-memory tracker。
//!
//! ## 协议背景
//!
//! grok.com Web 后端不接受 `messages: [...]` 数组,只接受单 `message` 字段。
//! 多轮上下文走 **DAG 锚定**:每次请求带 `parent_response_id`(上一轮 modelResponse
//! 的 UUID),后端自己拉历史。
//!
//! 实测(2026-05-11 SuperGrok)三轮 DAG 严格成立:
//!
//! ```text
//! prev_model → R1.user → R1.model → R2.user → R2.model → R3.user → R3.model
//!              b2fd...   9f82...    cbf7...   16f8...    f81c...   e501...
//! user.parentResponseId  = 上一轮 model.responseId(交替)
//! model.parentResponseId = 同轮 user.responseId
//! ```
//!
//! ## 本 tracker 职责
//!
//! Codex APP 通过 OpenAI Responses API 的 `previous_response_id` 表达多轮关系。
//! 本 tracker:
//!
//! - 接 Codex APP `previous_response_id`(我们自己发出的 Responses ID)
//! - 反查对应 grok.com modelResponse 的 `responseId`(grok 本地 UUID)
//! - 让下次请求把那个 UUID 当 `parent_response_id` 传给 grok.com
//!
//! ## 失败回退
//!
//! - tracker miss 时,**不传** `parent_response_id`(让 grok 开新会话,与首轮等价)
//! - 接受多轮信息"断片",比 hard fail 友好
//!
//! ## LRU bound(R1 PR-1,PR #129 P2 review thread 承诺项)
//!
//! 原 R3 PoC 用裸 `Mutex<HashMap>` 无 bound — 在长跑代理下每个 chat 都
//! `record` 一次,**进程内存随时间线性增长**(chatgpt-codex-connector P2 标记)。
//!
//! 本 PR 借鉴 [`crate::responses::session::ResponseSessionCache`] 的 L1 LRU 设计:
//!
//! - 容量上限 [`DEFAULT_MAX_SIZE`](默认 1000 条,跟 SessionCache 一致)
//! - `access_count` 追踪近期访问,满 + 新 key 不在 → evict 最少 `access_count` 的 entry
//! - 无 TTL(R1 阶段刻意不引入;若未来需要再加 `ttl: Option<Duration>`)
//!
//! 不持久化磁盘(grok.com cookie 本来就 session-bound,进程重启等于重新登录)。
//!
//! ## 线程安全
//!
//! 内部 `Mutex<TrackerInner>`,所有方法 `&self`,与 [`Adapter`] trait `Send + Sync`
//! 兼容。**Mutex poisoning 优雅恢复**:`PoisonError::into_inner()` 拿原值继续
//! 操作(tracker 是 cache,数据无 invariant 需保护;poison 后继续 work 比
//! silently no-op 友好,silent-failure-hunter M1)。
//!
//! [`Adapter`]: crate::types::Adapter

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// L1 LRU 默认容量上限。同 `ResponseSessionCache::DEFAULT_L1_SIZE`,1000 条
/// × ~80B per entry ≈ 80KB 上限内存占用。
pub const DEFAULT_MAX_SIZE: usize = 1000;

/// Codex APP 暴露给客户端的 Responses ID(`resp_<uuid>` 风格)。
///
/// **R1 PR-4 / TD2 type-design-analyzer 反馈**:原 `record(impl Into<String>,
/// impl Into<String>)` 两个 `String` 参数易 swap 出 bug —— Codex side ID 是
/// `resp_*` 风格,grok side 是裸 UUID v4,运行时**理论上**能从 prefix 检测但
/// 实际没做。改成 newtype 让 swap 在 compile time 就报错。
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct CodexResponseId(pub String);

impl CodexResponseId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for CodexResponseId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for CodexResponseId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// grok.com 后端的 `modelResponse.responseId`(裸 UUID v4)。
///
/// 与 [`CodexResponseId`] 分两个类型,防止 [`ParentResponseTracker::record`]
/// 参数 swap(R1 PR-4 / TD2)。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GrokResponseId(pub String);

impl GrokResponseId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<String> for GrokResponseId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for GrokResponseId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

#[derive(Debug, Clone)]
struct Entry {
    grok_response_id: String,
    /// 创建时间戳,evict_oldest 决断依据之一(secondary key)。
    ts: Instant,
    /// 累计 `get` 次数,evict_oldest 决断主键(LRU 近似:访问越少越先 evict)。
    access_count: u64,
}

#[derive(Debug)]
struct TrackerInner {
    entries: HashMap<String, Entry>,
}

/// `(Codex Responses ID) → (grok.com responseId)` 反查表,LRU-bounded。
#[derive(Debug)]
pub struct ParentResponseTracker {
    max_size: usize,
    inner: Mutex<TrackerInner>,
}

impl Default for ParentResponseTracker {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_SIZE)
    }
}

impl ParentResponseTracker {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size: max_size.max(1),
            inner: Mutex::new(TrackerInner {
                entries: HashMap::new(),
            }),
        }
    }

    /// 记录:Codex APP 暴露的 Responses ID → grok.com 后端的 modelResponse.responseId。
    ///
    /// 满 + 新 key 不在 → evict 最少 `access_count` 的旧 entry(`ts` 作 tiebreak)。
    /// Mutex poison 时用 `PoisonError::into_inner()` 恢复继续工作。
    ///
    /// **强类型化(R1 PR-4 / TD2)**:接受 [`CodexResponseId`] / [`GrokResponseId`]
    /// 而非两个 `impl Into<String>`,防止 record 时参数 swap(call sites 调试苦)。
    pub fn record(&self, codex: CodexResponseId, grok: GrokResponseId) {
        let codex_id = codex.0;
        let grok_id = grok.0;
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!(
                    error_id = "GROK_TRACKER_POISONED",
                    "parent_response_tracker mutex poisoned, recovering via PoisonError::into_inner"
                );
                poisoned.into_inner()
            }
        };
        if inner.entries.len() >= self.max_size && !inner.entries.contains_key(&codex_id) {
            evict_oldest_locked(&mut inner);
        }
        inner.entries.insert(
            codex_id,
            Entry {
                grok_response_id: grok_id,
                ts: Instant::now(),
                access_count: 0,
            },
        );
    }

    /// 查询:给定 Codex APP `previous_response_id`(包成 [`CodexResponseId`]),
    /// 返回对应 [`GrokResponseId`]。
    ///
    /// 命中后递增 `access_count`(LRU 近似)。
    /// 返回 `None` 时,请求构建方应**省略** `parent_response_id` 字段(开新会话语义)。
    pub fn get(&self, codex: &CodexResponseId) -> Option<GrokResponseId> {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!(
                    error_id = "GROK_TRACKER_POISONED",
                    "parent_response_tracker mutex poisoned, recovering via PoisonError::into_inner"
                );
                poisoned.into_inner()
            }
        };
        let entry = inner.entries.get_mut(&codex.0)?;
        entry.access_count = entry.access_count.saturating_add(1);
        Some(GrokResponseId(entry.grok_response_id.clone()))
    }

    /// 测试便捷:用 `&str` 接 `record`,内部 wrap 成 newtypes。
    /// 生产代码**必须**用 [`Self::record`](强类型 API)。
    #[cfg(test)]
    pub fn record_str(&self, codex: &str, grok: &str) {
        self.record(CodexResponseId::from(codex), GrokResponseId::from(grok));
    }

    /// 测试便捷:用 `&str` 查 + 取出 inner String。
    /// 生产代码**必须**用 [`Self::get`](强类型 API)。
    #[cfg(test)]
    pub fn get_str(&self, codex: &str) -> Option<String> {
        self.get(&CodexResponseId::from(codex))
            .map(|g| g.into_inner())
    }

    /// 容量(测试用)。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(g) => g.entries.len(),
            Err(p) => p.into_inner().entries.len(),
        }
    }

    /// 当前 max_size 上限(测试用)。
    #[cfg(test)]
    pub fn capacity(&self) -> usize {
        self.max_size
    }
}

/// LRU 近似 evict:选 `access_count` 最低的 entry,`ts` 作 tiebreak(旧的先 evict)。
fn evict_oldest_locked(inner: &mut TrackerInner) {
    let Some(victim_key) = inner
        .entries
        .iter()
        .min_by_key(|(_, e)| (e.access_count, e.ts))
        .map(|(k, _)| k.clone())
    else {
        return;
    };
    inner.entries.remove(&victim_key);
}

/// 全局单例 tracker —— 进程级,所有 Provider 共用,默认 1000 条 LRU。
pub fn global_tracker() -> &'static ParentResponseTracker {
    static TRACKER: OnceLock<ParentResponseTracker> = OnceLock::new();
    TRACKER.get_or_init(ParentResponseTracker::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_get_roundtrip() {
        let t = ParentResponseTracker::default();
        t.record_str("resp_abc", "9f82a10c-grok-uuid");
        assert_eq!(t.get_str("resp_abc").as_deref(), Some("9f82a10c-grok-uuid"));
        assert_eq!(t.get_str("resp_unknown"), None);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn record_overwrites_existing_entry() {
        let t = ParentResponseTracker::default();
        t.record_str("resp_abc", "old-uuid");
        t.record_str("resp_abc", "new-uuid");
        assert_eq!(t.get_str("resp_abc").as_deref(), Some("new-uuid"));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn global_tracker_is_singleton() {
        let a = global_tracker() as *const _;
        let b = global_tracker() as *const _;
        assert_eq!(a, b);
    }

    #[test]
    fn lru_evicts_least_used_when_full() {
        // R1 PR-1(PR #129 P2 review thread):满 + 新 key 不在 → evict 最少 access
        let t = ParentResponseTracker::new(3);
        t.record_str("a", "grok-a");
        t.record_str("b", "grok-b");
        t.record_str("c", "grok-c");
        assert_eq!(t.len(), 3);
        // b/c 各访问一次,a 保持 0 access → 最先被 evict
        let _ = t.get_str("b");
        let _ = t.get_str("c");
        // 容量满 + 新 key "d" → evict access_count 最少的 "a"
        t.record_str("d", "grok-d");
        assert_eq!(t.len(), 3);
        assert!(t.get_str("a").is_none(), "a 应被 evict");
        assert_eq!(t.get_str("b").as_deref(), Some("grok-b"));
        assert_eq!(t.get_str("c").as_deref(), Some("grok-c"));
        assert_eq!(t.get_str("d").as_deref(), Some("grok-d"));
    }

    #[test]
    fn lru_does_not_evict_when_updating_existing_key() {
        let t = ParentResponseTracker::new(2);
        t.record_str("a", "grok-a");
        t.record_str("b", "grok-b");
        assert_eq!(t.len(), 2);
        t.record_str("a", "grok-a-new"); // 已在,overwrite
        assert_eq!(t.len(), 2, "更新已存在 key 不应 evict");
        assert_eq!(t.get_str("a").as_deref(), Some("grok-a-new"));
        assert_eq!(t.get_str("b").as_deref(), Some("grok-b"));
    }

    #[test]
    fn capacity_zero_is_clamped_to_one() {
        let t = ParentResponseTracker::new(0);
        assert_eq!(t.capacity(), 1);
        t.record_str("a", "grok-a");
        assert_eq!(t.len(), 1);
        t.record_str("b", "grok-b");
        assert_eq!(t.len(), 1, "容量 1 时 b 应让 a 被 evict");
        assert!(t.get_str("a").is_none());
    }

    #[test]
    fn default_capacity_is_1000() {
        let t = ParentResponseTracker::default();
        assert_eq!(t.capacity(), DEFAULT_MAX_SIZE);
        assert_eq!(t.capacity(), 1000);
    }
}
