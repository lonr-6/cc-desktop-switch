//! 用户注册表 (~/.codex-app-transfer/config.json) 读写助手.

use codex_app_transfer_registry::{
    config_file, heal_builtin_provider_fields, heal_legacy_update_url, load_raw_config,
    save_raw_config, RawConfig, DEFAULT_UPDATE_URL,
};
use serde_json::{json, Value};

pub fn load() -> Result<RawConfig, String> {
    let path = config_file().ok_or_else(|| "cannot locate user config directory".to_owned())?;
    if !path.exists() {
        return Ok(json!({
            "version": "1.0.4",
            "activeProvider": null,
            "gatewayApiKey": null,
            "providers": [],
            "settings": {
                "theme": "default",
                "language": "zh",
                "proxyPort": 18080,
                "adminPort": 18081,
                "autoStart": false,
                "autoApplyOnStart": true,
                "exposeAllProviderModels": false,
                "restoreCodexOnExit": true,
                "updateUrl": DEFAULT_UPDATE_URL
            }
        }));
    }
    let mut cfg = load_raw_config(&path).map_err(|e| format!("read config.json failed: {e}"))?;
    // 强制覆盖 builtin provider 的"非用户配置"字段(apiFormat / authScheme /
    // extraHeaders) — 详见 codex_app_transfer_registry::healing 模块说明。
    // 老版本(v1.x)写入或用户手改可能让这些字段不对(空字符串 / "responses" /
    // 缺失等),触发 MiMo 404 / Kimi 403 等绕过代理的功能性 bug。
    //
    // 策略:有改动 → **写回磁盘**(2026-05-08 用户确认:这类内部协议路由信号
    // 不支持用户自定义,直接覆盖磁盘旧配置,以后不再因残留而出错)。
    let mut healed = heal_builtin_provider_fields(&mut cfg);
    // 迁移老 fork 的 updateUrl（lonr-6 等）→ Cmochance（任务 2+3）。
    // 与 provider healing 相同策略：有变更就尽量写回磁盘，避免用户下次启动
    // 仍被残留的旧仓库 URL 带偏“检查更新”。
    if heal_legacy_update_url(&mut cfg) {
        healed = true;
    }
    if healed {
        // 写回失败不致命:内存里 heal 过的版本仍可用,下次启动再尝试同步盘
        if let Err(e) = save_raw_config(&path, &cfg) {
            eprintln!("warning: write back config.json after heal failed (in-memory healed version still in effect for this session): {e}");
        }
    }
    Ok(cfg)
}

/// **module-private**(架构性 enforcement,2026-05-11):RMW 调用方**只能**走
/// [`with_config_write`],不能直接调 `save`。把 save 限制到 registry_io 模块
/// 内部让 prod 代码物理上无法 import raw save —— 防新增 callsite 误用 raw
/// load+save 而 reintroduce lost-update race。
///
/// 测试代码通过 [`save_for_test`](cfg(test) 暴露)显式 opt-in。
fn save(cfg: &RawConfig) -> Result<(), String> {
    let path = config_file().ok_or_else(|| "cannot locate user config directory".to_owned())?;
    save_raw_config(&path, cfg).map_err(|e| format!("write config.json failed: {e}"))
}

/// **测试专用**:绕过 `with_config_write` 直接 save。prod 代码访问不到(cfg(test)
/// 编译时排除)。test 路径仍可任意写 fixture / 直接验 disk state。
#[cfg(test)]
pub fn save_for_test(cfg: &RawConfig) -> Result<(), String> {
    save(cfg)
}

/// 序列化 config.json 的 RMW 操作 — 单进程内多 admin handler 并发时
/// (eg user 一边 form save provider,一边 OAuth callback sync project_id),
/// 防止 `load → mutate → save` 序列被另一**同样走 with_config_write**的并发
/// RMW 切入导致最终 save 覆盖中间结果("write skew")。
///
/// **迁移状态**(2026-05-11 完成全栈):所有 prod RMW callsite 走此 API:
/// - `gemini_oauth.rs`:sync_project_id_to_active_provider /
///   clear_project_id_from_active_provider
/// - `desktop.rs`:sync_desktop_for_active_provider / switch_provider_and_sync /
///   desktop_configure
/// - `providers/crud.rs`:add_provider / update_provider / delete_provider /
///   reorder_providers / update_models(save_draft 复用 update_provider)
/// - `providers/models.rs`:autofill_provider_models(read 锁外 await + 写锁内)
/// - `settings.rs`:save_settings / import_config / create_config_backup
///
/// 测试代码 + read-only handlers(get_*)仍可用 raw [`load`] / [`save`],它们
/// 不构成 RMW 序列。任何**新加** RMW 路径**必须**走 [`with_config_write`],
/// 而不是 raw load+save(自动 lint 待 followup)。
///
/// **不可重入**:`std::sync::Mutex` 同线程 re-lock 直接 deadlock。closure
/// 内部**严禁**再调 `with_config_write` / `load` / `save` —— 只能纯 mutate
/// 传入的 `&mut RawConfig`。
///
/// 实现:进程内全局 std Mutex,锁 lifetime 覆盖整个 closure。`config.json`
/// 文件 ~5KB,save IO ~ms 级,Mutex 阻塞 admin async runtime 不可感知;
/// 不引入 tokio::sync::Mutex 是为了保持 sync API 兼容现有同步 handler 调用方
/// (admin tower 已经把每个 request 跑独立 task,真并发时锁等待会让 task
/// 排队但不阻塞整个 runtime)。
///
/// **panic 安全**:Mutex poisoning 时直接重置(`into_inner` 获 inner data
/// drop 后新建)— config 锁本身不携带数据,poison 风险最低,直接 ignore
/// 让后续 RMW 继续(用 `lock().unwrap_or_else(|e| e.into_inner())` 模式)。
static CONFIG_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 闭包返回值告诉 [`with_config_write`] 是否真改了 config —— `Modified` 触发
/// `save()`,`Unchanged` 跳过 save 全程 read-only。
///
/// **存在原因**(chatgpt-codex P1 修,2026-05-11):skip 分支(eg active provider
/// 不是 gemini_cli_oauth)早期实现返 `Ok(())`,with_config_write 无条件 save
/// 让 read-only 路径退化成 read-write,跟仍未迁的 raw load+save callsite 并发
/// 时仍 lost-update。换成 enum 后 caller **必须显式声明**有没有改。
pub enum ConfigMutation<T> {
    /// 闭包改了 config,with_config_write 必须 save。
    Modified(T),
    /// 闭包没改 config(纯 read 决策 / skip 分支)— save 跳过,不 touch disk。
    Unchanged(T),
}

/// thread-local 守卫:进入 [`with_config_write`] 闭包时设 true,退出时还原。
/// 用于检测 closure 内套调 with_config_write 的 reentrant deadlock —— std
/// Mutex 同线程 re-lock 永久 hang(无 panic 无 timeout,UI 静默冻结)。本守卫
/// 让 reentrant 立刻 panic 报告位置,silent hang → loud panic。
thread_local! {
    static IN_WITH_CONFIG_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 拿 cross-process 文件锁(`<config_dir>/config.json.lock`),`fs2::FileExt::
/// lock_exclusive` 走 OS 级 fcntl/LockFileEx,任意 OS 进程同时 with_config_write
/// 时排队执行。配合进程内 `CONFIG_FILE_LOCK` Mutex(单 OS 进程内 thread 之间
/// 串行),实现"两层锁"全栈 race-free。
///
/// **lock 文件**(non-config 文件):**不**直接锁 config.json — 那样 file open /
/// rename 顺序复杂(配合 atomic rename 保存)。专门 sentinel 文件 `.lock`,只
/// 用作锁 token,内容空,跨进程协调用。
///
/// **失败 fallback**:lock_exclusive 失败(eg lock 文件路径不可用)tracing::warn
/// 后**继续走仅进程内 Mutex 路径** — 即便没 cross-process 锁,单进程内仍 race-
/// free,跟未升级前等价。错误仅在 user 真同时跑 2 个 .app 实例时才暴露(罕见)。
fn lock_config_file_cross_process() -> Option<std::fs::File> {
    let path = config_file()?.with_extension("json.lock");
    // **fresh-install fix**(chatgpt-codex P2):~/.codex-app-transfer/ 在首次启
    // 动时不存在,直接 OpenOptions::open 会返 ENOENT → fallback 到无锁路径,
    // 正好在 bootstrap 多 .app 实例并发场景失去保护。先 create_dir_all 父目录,
    // 跟 save_raw_config 内部走的 mkdir 路径对齐
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                error_id = "CONFIG_LOCK_PARENT_DIR_CREATE_FAILED",
                error = %e,
                parent = ?parent,
                "create_dir_all for cross-process lock parent failed; falling back"
            );
            return None;
        }
    }
    // 创建 / 打开 lock 文件 — OpenOptions read+write+create 让其他进程也能 open
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(
                error_id = "CONFIG_LOCK_FILE_OPEN_FAILED",
                error = %e,
                path = ?path,
                "cross-process config lock file open failed; falling back to in-process Mutex only"
            );
            return None;
        }
    };
    use fs2::FileExt;
    if let Err(e) = file.lock_exclusive() {
        tracing::warn!(
            error_id = "CONFIG_LOCK_EXCLUSIVE_FAILED",
            error = %e,
            "fs2::lock_exclusive failed; falling back to in-process Mutex only"
        );
        return None;
    }
    Some(file)
}

pub fn with_config_write<F, T>(f: F) -> Result<T, String>
where
    F: FnOnce(&mut RawConfig) -> Result<ConfigMutation<T>, String>,
{
    // **reentrant detection**(防 silent deadlock):同线程已经持锁却又进 with_
    // config_write → std Mutex 直接 deadlock 永远 hang。本检查让它 panic 报告
    // 哪个 callsite 套调了。常见误用:closure 内调用了某个 helper,helper 又
    // (直接或间接)再调 with_config_write/load/save。
    IN_WITH_CONFIG_WRITE.with(|flag| {
        if flag.get() {
            panic!(
                "with_config_write reentered from within its own closure — std::sync::Mutex \
                 would deadlock silently. Check call stack for nested with_config_write / load / save."
            );
        }
        flag.set(true);
    });
    // RAII 守卫保证 panic 路径也复位 flag(否则下次 with_config_write 永远 panic)
    struct ReentryGuard;
    impl Drop for ReentryGuard {
        fn drop(&mut self) {
            IN_WITH_CONFIG_WRITE.with(|flag| flag.set(false));
        }
    }
    let _reentry_guard = ReentryGuard;

    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|poison| {
        // poison = 之前一次 closure 内 panic 留下的状态。锁本身不带数据,
        // recover 安全;但要 log 让 operator 看到有 panic 发生过 —— 否则
        // 完全 silent(silent-failure-hunter H2 修)
        tracing::error!(
            error_id = "REGISTRY_LOCK_POISONED",
            "CONFIG_FILE_LOCK poisoned by prior panic in with_config_write closure; \
             recovering — check logs for the original panic + verify config.json integrity"
        );
        poison.into_inner()
    });
    // **cross-process** 锁:第二层兜 OS 级别。File drop 时 OS 自动 release lock
    // (不需要显式 unlock)。fallback None 时 silent — 单进程内 Mutex 仍 race-free
    let _file_lock = lock_config_file_cross_process();

    let mut cfg = load()?;
    // closure 返 Err → 直接冒泡,save 不被调,内存版本 cfg drop
    match f(&mut cfg)? {
        ConfigMutation::Modified(v) => {
            save(&cfg)?;
            Ok(v)
        }
        ConfigMutation::Unchanged(v) => {
            // **跳过 save**(chatgpt-codex P1 修):skip 分支不 touch disk,
            // 不会跟仍未迁的 raw load+save callsite 并发覆盖。等价于 read-only。
            Ok(v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::handlers::common::test_support::with_isolated_home;
    use std::sync::Arc;
    use std::sync::Barrier;

    /// Sanity:闭包错误时不应破坏 disk 状态 —— closure 返 Err 时 with_config_write
    /// 直接冒泡 error,save 不被调,disk 文件**字节级不变**。**不能仅 assert
    /// result.is_err()** —— 那只测 `?` 操作符,没 lock contract(future 把 `f(&mut
    /// cfg)?; save(&cfg)?` 改 `let r = f(&mut cfg); save(&cfg)?; r` 测照过但 disk
    /// 已脏)— pr-test-analyzer G1 修
    #[test]
    fn closure_error_does_not_touch_disk() {
        use codex_app_transfer_registry::config_file;

        with_isolated_home(|_home| {
            // 1. 先成功写一次拿到 baseline disk 状态
            with_config_write(|cfg| {
                cfg.as_object_mut()
                    .unwrap()
                    .insert("counter".into(), serde_json::json!(42));
                Ok(ConfigMutation::Modified(()))
            })
            .unwrap();
            let path = config_file().unwrap();
            let before = std::fs::read(&path).unwrap();

            // 2. closure 故意 mutate cfg 然后返 Err — 内存改了,disk 必须不动
            let result: Result<(), String> = with_config_write(|cfg| {
                cfg.as_object_mut()
                    .unwrap()
                    .insert("counter".into(), serde_json::json!(999));
                Err("intentional fail".into())
            });
            assert!(result.is_err(), "closure 返 Err 必须冒泡");

            // 3. disk 字节比对 — 任何 partial save 都会让 hash 变
            let after = std::fs::read(&path).unwrap();
            assert_eq!(
                before, after,
                "closure Err 时 disk 必须字节级不变(防 future 把 save 提到 ? 之前)"
            );

            // 4. 重新 load 验内存版本也是 42 而不是 999
            let n = with_config_write(|cfg| {
                Ok(ConfigMutation::Unchanged(
                    cfg.as_object()
                        .and_then(|o| o.get("counter"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(-1),
                ))
            })
            .unwrap();
            assert_eq!(n, 42, "Err closure 不该 leak 内存改动到 disk");
        });
    }

    /// **chatgpt-codex P1 修**回归 gate:closure 返 Unchanged 时 disk 字节级
    /// 不变 —— skip 分支(active≠OAuth)不能退化成 read-only-then-write。原版
    /// `Ok(())` 无条件 save,跟未迁的 raw load+save callsite 并发会 lost-update。
    #[test]
    fn closure_unchanged_does_not_touch_disk() {
        use codex_app_transfer_registry::config_file;

        with_isolated_home(|_home| {
            // 先 seed
            with_config_write(|cfg| {
                cfg.as_object_mut()
                    .unwrap()
                    .insert("counter".into(), serde_json::json!(7));
                Ok(ConfigMutation::Modified(()))
            })
            .unwrap();
            let path = config_file().unwrap();
            let before = std::fs::read(&path).unwrap();
            let before_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

            // 等一会防 mtime 同 second
            std::thread::sleep(std::time::Duration::from_millis(50));

            // closure 故意 mutate 内存 cfg 但返 Unchanged — disk 必须不变
            // (这测试 contract:Unchanged 即便内存改了也跳过 save)
            with_config_write(|cfg| {
                cfg.as_object_mut()
                    .unwrap()
                    .insert("counter".into(), serde_json::json!(999));
                Ok(ConfigMutation::Unchanged(()))
            })
            .unwrap();

            let after = std::fs::read(&path).unwrap();
            let after_mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
            assert_eq!(before, after, "Unchanged 时 disk content 必须字节级不变");
            assert_eq!(before_mtime, after_mtime, "Unchanged 时 mtime 不该被 touch");

            // 重新 load 应该是原 7 不是 999
            let n = with_config_write(|cfg| {
                Ok(ConfigMutation::Unchanged(
                    cfg.as_object()
                        .and_then(|o| o.get("counter"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(-1),
                ))
            })
            .unwrap();
            assert_eq!(n, 7, "Unchanged closure 不该 leak 内存改动到 disk");
        });
    }

    /// **panic recovery 路径**:closure 内 panic 让 Mutex poisoned。下次
    /// with_config_write 调用应:
    /// - 不 deadlock / 不 double-panic
    /// - poison 通过 unwrap_or_else 恢复
    /// - tracing::error!("REGISTRY_LOCK_POISONED") 已 log(silent-failure H2 修)
    /// - 后续操作能正常 read/write disk
    #[test]
    fn poison_recovers_for_next_caller() {
        with_isolated_home(|_home| {
            // 第一次调用,closure 内 panic 让 mutex poisoned
            let panic_result = std::panic::catch_unwind(|| {
                let _: Result<(), String> = with_config_write(|_cfg| {
                    panic!("intentional panic to poison the mutex");
                });
            });
            assert!(panic_result.is_err(), "panic 必须真发生");

            // 第二次调用应该能正常 work,不 deadlock 不 panic
            let result = with_config_write(|cfg| {
                cfg.as_object_mut()
                    .unwrap()
                    .insert("after_poison".into(), serde_json::json!(true));
                Ok(ConfigMutation::Modified(()))
            });
            assert!(
                result.is_ok(),
                "poison recovery 失败,后续 with_config_write 应能继续: {:?}",
                result
            );
        });
    }

    /// **reentrant detection contract**:closure 内套调 with_config_write 必须
    /// panic 而非 silent deadlock。`#[should_panic]` 验 panic message 含
    /// "reentered"。RAII guard 保证 panic 路径也复位 thread-local flag。
    #[test]
    #[should_panic(expected = "reentered")]
    fn reentrant_call_panics_not_deadlocks() {
        with_isolated_home(|_home| {
            let _ = with_config_write(|_cfg| {
                // 套调 with_config_write — 必须 panic 报告 reentrant
                let _ = with_config_write(|_inner| Ok(ConfigMutation::Unchanged(())));
                Ok(ConfigMutation::Unchanged(()))
            });
        });
    }

    /// 验 RAII guard:reentrant panic 后,下一次调用 with_config_write 应能
    /// 正常 work(thread-local flag 已复位,不会永久 false-positive panic)。
    #[test]
    fn after_reentrant_panic_next_call_works() {
        with_isolated_home(|_home| {
            // 触发 reentrant panic
            let panic_result = std::panic::catch_unwind(|| {
                let _ = with_config_write(|_cfg| {
                    let _ = with_config_write(|_| Ok(ConfigMutation::Unchanged(())));
                    Ok(ConfigMutation::Unchanged(()))
                });
            });
            assert!(panic_result.is_err(), "套调必须触发 panic");
            // 下一次 with_config_write 应能正常返(RAII guard 复位 flag)
            let result = with_config_write(|_cfg| Ok::<_, String>(ConfigMutation::Unchanged(42)));
            assert_eq!(
                result.unwrap(),
                42,
                "RAII guard 失效:reentrant panic 后 thread-local flag 未复位"
            );
        });
    }

    /// **核心 atomicity 验**:多线程并发 RMW 时,每条 closure 串行执行,
    /// 最终累计结果跟 sequential 等价(不丢任何一条 mutation)。原版 raw
    /// load+mutate+save 序列在并发下会 lost update —— 本测试 lock 防回归。
    /// `with_isolated_home` 已 serialize 整个 admin test 集 HOME 用,所以
    /// 这里跑完外面 HOME 自动还原,跨 test 不污染。
    #[test]
    fn concurrent_rmw_no_lost_update() {
        with_isolated_home(|_home| {
            // 初始化 config
            let _ = with_config_write(|cfg| {
                cfg.as_object_mut()
                    .unwrap()
                    .insert("counter".into(), serde_json::json!(0));
                Ok(ConfigMutation::Modified(()))
            });

            // 8 线程各自 +1 共 800 次,Barrier 同步起跑增并发竞争
            const THREADS: usize = 8;
            const ITERS_PER_THREAD: usize = 100;
            let barrier = Arc::new(Barrier::new(THREADS));
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let b = Arc::clone(&barrier);
                    std::thread::spawn(move || {
                        b.wait();
                        for _ in 0..ITERS_PER_THREAD {
                            let _ = with_config_write(|cfg| {
                                let n = cfg
                                    .as_object()
                                    .and_then(|o| o.get("counter"))
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                cfg.as_object_mut()
                                    .unwrap()
                                    .insert("counter".into(), serde_json::json!(n + 1));
                                Ok(ConfigMutation::Modified(()))
                            });
                        }
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            let final_n = with_config_write(|cfg| {
                Ok(ConfigMutation::Unchanged(
                    cfg.as_object()
                        .and_then(|o| o.get("counter"))
                        .and_then(|v| v.as_i64())
                        .unwrap_or(-1),
                ))
            })
            .unwrap();

            assert_eq!(
                final_n,
                (THREADS * ITERS_PER_THREAD) as i64,
                "atomicity 失败:并发 +1 共 {} 次,实际 counter={}",
                THREADS * ITERS_PER_THREAD,
                final_n
            );
        });
    }
}

/// Mask provider 给前端展示:apiKey 字段去除,extraHeaders 清空(可能含敏感
/// 头),其它字段透传 + 加 `hasApiKey` 标记。
pub fn public_provider(p: &Value) -> Value {
    let Some(obj) = p.as_object() else {
        return p.clone();
    };
    let mut out = obj.clone();
    let has_key = out
        .get("apiKey")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    out.remove("apiKey");
    out.remove("extraHeaders");
    out.insert("hasApiKey".into(), Value::Bool(has_key));
    // grokWeb 含 sso / sso-rw / cf_clearance cookie + statsigId,全是敏感凭证。
    // 跟 apiKey 一样 mask 出去,只暴露 `hasGrokWeb` 给前端,让 UI 决定 placeholder。
    // 前端编辑保存若不重填 → 不传 grokWeb 字段 → 后端 update_provider 保留现值。
    let has_grok_web = out
        .get("grokWeb")
        .and_then(|v| v.as_object())
        .map(|o| {
            let has_cookies = o
                .get("cookies")
                .and_then(|c| c.as_object())
                .map(|c| {
                    c.values()
                        .any(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(false))
                })
                .unwrap_or(false);
            let has_statsig = o
                .get("statsigId")
                .and_then(|v| v.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            has_cookies || has_statsig
        })
        .unwrap_or(false);
    out.remove("grokWeb");
    out.insert("hasGrokWeb".into(), Value::Bool(has_grok_web));
    Value::Object(out)
}
