//! `/api/gemini-oauth/*` admin handlers — Gemini CLI OAuth 登录 / 状态 / 注销 +
//! Cloud Code Assist project bootstrap。
//!
//! ## 路由
//!
//! - `POST /api/gemini-oauth/login`:启动 OAuth flow → bootstrap project_id →
//!   持久化 token。**长 polling** ≤ 5min(浏览器登录 callback timeout)。response
//!   含 `email + project_id + expires_at`,前端用来更新 UI。**OAuth + bootstrap
//!   + project_id sync 全成功才算 login 成功**(C2 atomicity 修):任一失败返
//!   5xx 不持久化 token,用户必须重试整流
//! - `GET /api/gemini-oauth/status`:返当前 token 状态(已登录 / 未登录 / 即将
//!   过期)。前端 dashboard 启动时调一次
//! - `DELETE /api/gemini-oauth/logout`:`TokenStore::delete()` + 清 active provider
//!   的 `extra.cloud_code_project_id` 字段(只清 active + apiFormat=gemini_cli_oauth
//!   匹配的 provider,不抹其他账号的 project_id)
//!
//! ## OAuth flow 期间 admin 行为
//!
//! `/login` 同步等待 callback 的 long-polling endpoint(单次 axum request 挂着
//! 5min)。webbrowser::open 失败时仅 tracing::warn!,**flow 继续等同一 redirect_uri
//! 的 callback** —— user 拿不到自动浏览器但可手动用任意浏览器打开 URL(URL 在
//! tracing log 里,前端从 log viewer 能看到)。

use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use codex_app_transfer_gemini_oauth::{
    bootstrap_project, persist_token, run_oauth_flow_with_cancel, FlowError, OauthFlowConfig,
    TokenStore,
};
use serde_json::{json, Value};
use tokio::sync::watch;

use super::super::registry_io::{with_config_write, ConfigMutation};
use super::super::state::AdminState;
use super::common::err;
use super::providers::active_provider;

/// **进程级**当前 in-flight OAuth login 的 cancel sender + epoch —— 任意时刻
/// 最多一个 login。**epoch 防"晚 take" race**(reviewer high #1):新 login B
/// `slot.replace((epoch_B, tx_B))` 把旧 (epoch_A, tx_A) 抢出来 send 取消 A;
/// 但 A 的 post-flow 清理路径里**只能在 slot 当前 epoch 仍是自己的 epoch_A
/// 时**才 take —— 否则 B 已经接管,A 清理把 B 的 sender 也抹掉,B 整段无法
/// 取消(直到 B 自己结束)。epoch 由 `next_epoch()` 单调原子分配。
///
/// 类型:`Mutex<Option<(u64, watch::Sender<bool>)>>`(epoch, sender)。
/// **C2 修**(silent-failure-hunter 标 critical):原 oneshot::Sender 一次性
/// 消费,只能给 OAuth flow 用,bootstrap_project 等后续阶段(5-30s)收不到
/// cancel,user 看 cancelled:false 但 token 仍 persist。watch::Sender 升级 —
/// 多 receiver clone 跨阶段共享,login_handler 把 OAuth flow + bootstrap
/// project + persist + sync 全部 wrap cancel-aware,真"贯穿 pipeline"。
fn cancel_slot() -> &'static Mutex<Option<(u64, watch::Sender<bool>)>> {
    static SLOT: OnceLock<Mutex<Option<(u64, watch::Sender<bool>)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 单调分配 cancel slot 的 epoch token —— 每个 login 持自己的 epoch,清理时
/// 用 epoch 校验防 ABA / 抢占 race。
///
/// **SAFETY (Relaxed ordering)**:本 fn 用 Relaxed 仅保证原子计数自身的单调性,
/// 不提供跟 cancel_slot 内容的 happens-before。**写 epoch 的 callsite**(`slot.
/// replace((my_epoch, ..))`)和**读 epoch 的 callsite**(`slot.as_ref()` 后比
/// `*e == my_epoch`)都在 cancel_slot() Mutex 锁内执行,Mutex 自身的
/// acquire/release 已提供 happens-before。如果未来 refactor 把 epoch 比较移到
/// 锁外,需要改 Acquire/Release(silent-failure-hunter H2 lock 防回归)
fn next_epoch() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// 锁 cancel_slot 时统一处理 poison(reviewer high #2)—— Mutex 数据轻量,
/// poison 时直接 recover 让后续路径继续 + warn log 让 operator 看到曾发生
/// panic。silent ignore 会让 cancel 整个 silent 失效。
fn lock_cancel_slot() -> std::sync::MutexGuard<'static, Option<(u64, watch::Sender<bool>)>> {
    lock_cancel_slot_with_poison_flag().0
}

/// 跟 [`lock_cancel_slot`] 一样但额外返 `was_poisoned: bool` —— H1 修让 cancel
/// response 能区分 "没 in-flight" vs "lock 过 poison recovery 之后没 in-flight"。
/// 第二种状态意味着之前有过 panic,user 当下看到 cancelled:false 不知道发生过
/// 什么 — 此 flag 让 response 携带这个信息让 UI 给 operator 提示。
fn lock_cancel_slot_with_poison_flag() -> (
    std::sync::MutexGuard<'static, Option<(u64, watch::Sender<bool>)>>,
    bool,
) {
    match cancel_slot().lock() {
        Ok(g) => (g, false),
        Err(poison) => {
            tracing::warn!(
                error_id = "OAUTH_CANCEL_SLOT_POISONED",
                "OAuth cancel slot mutex poisoned by prior panic; recovering — verify last login state"
            );
            (poison.into_inner(), true)
        }
    }
}

/// `cancel_in_flight_login` 的结构化结果(H1 修):caller(handler / app exit /
/// 新 login 抢占)能区分三种状态 + 拿到被取消的 epoch 用于 [`wait_for_login_
/// epoch_complete`] 等待**特定** task 退出(C1 chatgpt-codex P1 修):
/// - `cancelled=true` + `cancelled_epoch=Some(N)`:slot 真有 in-flight,已发
///   cancel signal,N 是被取消的 login 的 epoch
/// - `cancelled=false, slot_recovered=false`:slot 当前为空,没 in-flight
/// - `cancelled=false, slot_recovered=true`:lock 过 poison recovery,本身没
///   in-flight,但说明之前有过 panic — operator 应去看 logs
#[derive(Debug, Clone, Copy)]
pub struct CancelOutcome {
    pub cancelled: bool,
    pub slot_recovered: bool,
    pub cancelled_epoch: Option<u64>,
}

/// **进程级 last-completed-epoch 通道**(C1 chatgpt-codex P1+P2 修):
/// LoginDoneGuard::Drop 把 self.epoch 通过 watch::Sender 发出,RunEvent::Exit
/// 钩子持 Receiver clone 用 `wait_for_login_epoch_complete(target)` 等
/// `*rx.borrow() >= target` 触发返回。
///
/// 跟原 `Notify::notify_waiters()` 设计的区别:
/// - **P2 修(persist completion)**:notify_waiters 只唤醒**当前** awaiting
///   tasks,如果 cancel 在 awaiter 起来前就触发 → 信号丢失,Exit 钩子等满 2s
///   timeout。watch sticky:Sender::send 之后值持久化,Receiver `borrow()`
///   读到的就是最新值,不会因 await 时机错过
/// - **P1 修(specific epoch)**:`u64` 是**已完成的最高 epoch**。Exit 钩子
///   waits for `*rx.borrow() >= cancelled_epoch`,不会被另一个 newer login
///   完成的事件误唤醒(preemption 场景:旧 login B 抢占旧 A 后,A 完成
///   不会让 B 的 wait 提早返)
fn login_done_channel() -> &'static (watch::Sender<u64>, watch::Receiver<u64>) {
    static C: OnceLock<(watch::Sender<u64>, watch::Receiver<u64>)> = OnceLock::new();
    C.get_or_init(|| watch::channel(0))
}

/// 等待 epoch=N 的 login task 真退完。Exit 钩子用,timeout 兜底由 caller。
pub async fn wait_for_login_epoch_complete(target_epoch: u64) {
    let mut rx = login_done_channel().1.clone();
    loop {
        if *rx.borrow() >= target_epoch {
            return;
        }
        // changed() 等下次 send,Err 时 sender 已 drop(进程退出),退化 pending
        // 让上层 timeout 兜底。生产路径下 sender 是 'static,几乎不会 Err
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// RAII 守卫:Drop 时把 self.epoch 写进 [`login_done_channel`]。覆盖 login_handler
/// 任意 return / panic / future-drop 路径,无需在每个 return 前手写 send。
/// 多次 drop 同一 epoch 等价(send_if_modified 只在更高 epoch 时触发 changed),
/// 防 stale 旧 epoch 倒退覆盖 newer 完成状态。
struct LoginDoneGuard {
    epoch: u64,
}
impl Drop for LoginDoneGuard {
    fn drop(&mut self) {
        let (tx, _) = login_done_channel();
        // send_if_modified 只在 cur < self.epoch 时改值 + 触发 changed,确保
        // higher epoch 不会被 earlier 完成的旧 epoch 倒退覆盖
        let my = self.epoch;
        let _ = tx.send_if_modified(|cur| {
            if my > *cur {
                *cur = my;
                true
            } else {
                false
            }
        });
    }
}

pub fn cancel_in_flight_login() -> CancelOutcome {
    let (mut guard, slot_recovered) = lock_cancel_slot_with_poison_flag();
    let (cancelled, cancelled_epoch) = if let Some((epoch, sender)) = guard.take() {
        // watch::send(true) 把当前 value 设 true 通知所有 clone 的 receiver。
        // send 失败(所有 receiver 已 drop)等价于 flow 已结束,无 op。
        // **C2 修**:此 send 触发的 cancel 不仅 OAuth flow 收到,login_handler
        // 持的 receiver clone 也会让 bootstrap/persist/sync 阶段 select! 退出
        let _ = sender.send(true);
        (true, Some(epoch))
    } else {
        (false, None)
    };
    CancelOutcome {
        cancelled,
        slot_recovered,
        cancelled_epoch,
    }
}

/// 进程级共享的 reqwest::Client,专门给 OAuth login flow + Cloud Code Assist
/// bootstrap 用 —— **不**重复创建,跨多次 login / refresh 复用底层 TLS 连接池
/// + DNS resolver 缓存。原 login_handler 每次新建 Client 会让 reqwest 走新的
/// connector setup(rustls config / DNS / IPv6 fallback timing),login 5min
/// 内连续触发(eg user 取消重试)产生 N 个独立 connection pool 浪费 ~MB RAM
/// + 多次 Google IP 探测 = 灰色 IP rep。
///
/// **Why OnceLock**:无 lazy_static 依赖、零运行时开销、`static` 安全跨线程,
/// 第一次调用初始化。
///
/// **Init 失败处理**(chatgpt-codex P1 修):builder 失败时返 Err 让 caller 走
/// 500 错误响应。**不**用 `Client::new()` fallback —— `Client::new()` 内部也是
/// `Client::builder().build().unwrap()`,builder 失败的环境(TLS/resolver 初
/// 始化失败)`Client::new()` 100% 也 panic,把 recoverable 500 转成 runtime
/// crash。OnceLock 存 Result 让首次失败被记忆,后续调用也直接返 Err 不重试
/// (避免每次 login 都打一遍 panic-prone path)。
///
/// **Pool 配置**:`pool_idle_timeout(30s)` 跟原 login_handler 一致,避免 long-
/// idle 后端 keep-alive 超时被中断。
///
/// **Why 不复用 ProxyState.http**:ProxyState 是 chat 路径专用,有自己 timeout
/// + redirect policy。OAuth flow 想要 stricter behavior,独立 pool 边界清晰。
pub fn shared_oauth_http_client() -> Result<&'static reqwest::Client, &'static str> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    let cell = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| {
                tracing::error!(
                    error_id = "OAUTH_HTTP_CLIENT_BUILDER_FAILED",
                    error = %e,
                    "reqwest::Client::builder failed for OAuth shared client; \
                     login_handler 将返 500 — verify system TLS / resource state"
                );
                format!("reqwest::Client::builder failed: {e}")
            })
    });
    match cell {
        Ok(c) => Ok(c),
        // 静态字面值 — caller 用 .into_response() 时不需要持有 owned String
        Err(_) => Err(
            "OAuth HTTP client init failed (TLS/resolver issue); check OAUTH_HTTP_CLIENT_BUILDER_FAILED log",
        ),
    }
}

const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";

/// 拿登录用户的 Google 账号 email — OAuth scope 已含 userinfo.email,直接调
/// `USERINFO_URL`。**失败 fallback None**:email 仅展示用,缺它 status UI 显
/// fallback 但 token / chat 全程不依赖。
async fn fetch_google_user_email(http: &reqwest::Client, access_token: &str) -> Option<String> {
    fetch_google_user_email_at(http, USERINFO_URL, access_token).await
}

/// 内部版 — 接受可定制 url 让单测 mock。pub(crate) 仅给 tests 用。
pub(crate) async fn fetch_google_user_email_at(
    http: &reqwest::Client,
    url: &str,
    access_token: &str,
) -> Option<String> {
    let resp = match http.get(url).bearer_auth(access_token).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "fetch_google_user_email http failed; status UI 将显 fallback");
            return None;
        }
    };
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "fetch_google_user_email non-2xx; status UI 将显 fallback");
        return None;
    }
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "fetch_google_user_email JSON parse failed");
            return None;
        }
    };
    body.get("email")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_owned())
}

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/api/gemini-oauth/status", get(status_handler))
        .route("/api/gemini-oauth/login", post(login_handler))
        .route(
            "/api/gemini-oauth/login/cancel",
            delete(cancel_login_handler),
        )
        .route("/api/gemini-oauth/logout", delete(logout_handler))
}

/// `DELETE /api/gemini-oauth/login/cancel` — 取消当前 in-flight OAuth login。
/// 响应 `{ cancelled: bool, slotRecovered: bool }`(H1 修):
/// - `cancelled=true`:真有 in-flight 被取消
/// - `cancelled=false, slotRecovered=false`:没 in-flight (no-op)
/// - `cancelled=false, slotRecovered=true`:lock 过 poison recovery — 本次没
///   in-flight,但之前有过 panic,UI 应给 operator hint 去看 logs
async fn cancel_login_handler() -> impl IntoResponse {
    let outcome = cancel_in_flight_login();
    if outcome.cancelled {
        tracing::info!("OAuth login cancelled by user request");
    } else if outcome.slot_recovered {
        tracing::warn!(
            error_id = "OAUTH_CANCEL_NOOP_AFTER_POISON",
            "OAuth cancel requested, no in-flight login but slot had been poison-recovered \
             — earlier panic in cancel-related path,operator 应查 OAUTH_CANCEL_SLOT_POISONED log"
        );
    } else {
        // false 也 log debug 让 "cancel button does nothing" 类报告可追
        tracing::debug!("OAuth cancel requested but no in-flight login");
    }
    Json(json!({
        "cancelled": outcome.cancelled,
        "slotRecovered": outcome.slot_recovered,
    }))
    .into_response()
}

/// `GET /api/gemini-oauth/status` — 返当前 token 状态。
///
/// Response shape:
/// ```json
/// {
///   "loggedIn": true,
///   "email": "user@example.com",
///   "projectId": "auto-provisioned-1234",
///   "expiresAt": 1730000000000,  // ms-epoch
///   "shouldRefresh": false
/// }
/// ```
async fn status_handler() -> impl IntoResponse {
    let store = match TokenStore::from_home_env() {
        Ok(s) => s,
        Err(e) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("home directory unavailable: {e}"),
            )
            .into_response()
        }
    };
    match store.load() {
        Ok(None) => Json(json!({ "loggedIn": false })).into_response(),
        Ok(Some(token)) => Json(json!({
            "loggedIn": true,
            "email": token.email,
            "projectId": token.project_id,
            "expiresAt": token.expiry_date,
            "shouldRefresh": token.should_refresh(),
        }))
        .into_response(),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("token store load: {e}"),
        )
        .into_response(),
    }
}

/// `POST /api/gemini-oauth/login` — 启动 OAuth flow + bootstrap project,长 polling
/// 直到完成或 timeout。
///
/// Request body:`{}`(无参数)
/// Response:成功返 200 + 当前 status 形态;失败返 4xx/5xx + error message
async fn login_handler() -> impl IntoResponse {
    // **epoch token** 创建在最前 — 既给 cancel slot 用(抢占语义 race fix),
    // 也给 LoginDoneGuard 用(C1 chatgpt-codex P1+P2:Exit 等具体 epoch 完成
    // 而非 generic notify。preemption 场景下 newer epoch 完成不会让 wait_for_
    // login_epoch_complete(older) 提早返,避免 race teardown 切断 newer task)
    let my_epoch = next_epoch();
    // **C1 + chatgpt-codex P1+P2 修**:把 my_epoch 写进 login_done_channel →
    // RunEvent::Exit 钩子 await 到 my_epoch 才让 Tauri tear-down runtime,防
    // OAuth task 在 persist_token 中段被砍写出 partial token。watch::channel
    // sticky + specific epoch wait 让快速 cancel 不卡 2s timeout + preemption
    // 场景下 newer epoch wait 不被 older epoch 完成误唤醒
    let _done_guard = LoginDoneGuard { epoch: my_epoch };
    // 拿进程级共享 client(M1 修):跨多次 login / refresh 复用 connection
    // pool + DNS cache + TLS session,避免每次 login 重建 connector
    let http = match shared_oauth_http_client() {
        Ok(c) => c,
        Err(msg) => return err(StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    };

    // 1. 跑 OAuth flow:loopback server + browser open + 等 callback + token exchange
    //
    // on_auth_url callback 落 tracing::info!,前端 log viewer 能看到 URL 给 user
    // 手动粘贴(webbrowser::open 失败时备用路径)。完整 SSE login-stream endpoint
    // 留 followup PR (前端 UI 一并做)。
    let mut config = OauthFlowConfig::default();
    config.on_auth_url = Some(Arc::new(|url: &str| {
        tracing::info!(
            auth_url = url,
            "OAuth auth URL 已生成 — 自动打开浏览器中,失败时 user 可从 log viewer 复制粘贴"
        );
    }));

    // 注册 cancel sender 到进程级 slot:任意 cancel 路径(DELETE /login/cancel /
    // app exit / 新 login 抢占)都能 send(()) 让 flow 立即退。**抢占语义**:新
    // login 启动前 take 旧 sender 触发 send,旧 flow 收到 Cancelled 立即退出 +
    // 释放 loopback port,新 flow 接管。防 user 连点 2 次"登录"产生 2 个并行
    // OAuth flow / 2 个 loopback server / 2 个 callback URL race。
    //
    // **epoch token**(reviewer high #1 修):本 login 持 my_epoch(line ~280
    // 已 next_epoch),post-flow 清理时只在 slot 当前 epoch 跟自己匹配时才
    // take,防"已被新 login 抢占后再 take 把新 login 的 sender 误清掉"
    let (cancel_tx, mut cancel_rx) = watch::channel::<bool>(false);
    {
        let mut slot = lock_cancel_slot();
        if let Some((_, prev_sender)) = slot.replace((my_epoch, cancel_tx)) {
            tracing::info!("抢占 in-flight OAuth login(user 连点登录或并发请求)");
            let _ = prev_sender.send(true); // 旧 flow 收到 Cancelled 立即退
        }
    }

    // helper: 跑 inner future,期间 select 监听 cancel — 命中即退出整 login
    // pipeline。**C2 修核心**:wrap bootstrap_project / persist 等任意 await
    // 都能立即响应 cancel(原 oneshot 只够 OAuth flow 用,过了 OAuth 后用户
    // 按取消 silent 失效)
    async fn cancellable<F, T>(
        cancel_rx: &mut watch::Receiver<bool>,
        fut: F,
    ) -> Result<T, FlowError>
    where
        F: std::future::Future<Output = Result<T, FlowError>>,
    {
        // 入口快路径 — cancel 已 set 不浪费起 fut
        if *cancel_rx.borrow() {
            return Err(FlowError::Cancelled);
        }
        tokio::select! {
            res = fut => res,
            // changed() 等 sender send(任意值);loop 直到看到 true,防 spurious
            // 唤醒(实际 watch 不会 spurious 但留 belt-and-suspenders)
            _ = async {
                loop {
                    if cancel_rx.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                    if *cancel_rx.borrow() { return; }
                }
            } => Err(FlowError::Cancelled),
        }
    }

    let flow_result = run_oauth_flow_with_cancel(&http, &config, Some(cancel_rx.clone())).await;
    let token = match flow_result {
        Ok(t) => t,
        Err(FlowError::Cancelled) => {
            // 清理 slot — 仅在 epoch 匹配时 take(防被新 login 抢占后误清)
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
                slot.take();
            }
            tracing::info!("OAuth login cancelled — 不持久化 token");
            return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
        }
        Err(e) => {
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
                slot.take();
            }
            return Json(json!({"loggedIn": false, "error": e.to_string()})).into_response();
        }
    };

    // 2. Bootstrap Cloud Code project — 拿 project_id(**C2 修**:cancel-aware
    // wrap,5-30s bootstrap 中按 cancel 立即退,不再等 LRO timeout)
    //
    // **silent-failure-hunter C2 atomicity**:bootstrap 失败时**不 persist token**
    let project_id = match cancellable(&mut cancel_rx, async {
        bootstrap_project(&http, &token.access_token, token.project_id.clone())
            .await
            .map_err(|e| {
                // bootstrap 错误用 FlowError::TokenParse 当容器类型 — 它专门
                // 给 endpoint-side 错信息留;也可加新 variant 但本 PR scope 内
                // 复用避免 API 大改
                FlowError::TokenParse(format!("cloud_code_bootstrap: {e}"))
            })
    })
    .await
    {
        Ok(id) => id,
        Err(FlowError::Cancelled) => {
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
                slot.take();
            }
            tracing::info!("OAuth login cancelled during bootstrap_project — 不持久化 token");
            return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
        }
        Err(e) => {
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e2, _)) if *e2 == my_epoch) {
                slot.take();
            }
            tracing::error!(error = %e, "Cloud Code bootstrap 失败 — token 不 persist,login 整体失败");
            return err(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Google account authenticated but Cloud Code project provisioning failed; \
                     please retry login. Detail: {e}"
                ),
            )
            .into_response();
        }
    };

    // 3. 终态 cancel check — 在 sync 写盘前最后机会(cancel 在 persist 之后到
    // 已经晚了,token 已 in disk;此 check 让"刚刚错过 bootstrap 完成"窗口
    // 内到达的 cancel 仍能阻止 sync + persist)
    if *cancel_rx.borrow() {
        let mut slot = lock_cancel_slot();
        if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
            slot.take();
        }
        tracing::info!("OAuth login cancelled after bootstrap, before persist — 不持久化 token");
        return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
    }

    // 3.5 拿 Google 账号 email — OAuth scope 已含 userinfo.email,直接调
    // openidconnect.googleapis.com/v1/userinfo 拿。失败不阻塞 login(email
    // 是展示字段,缺了 status UI 显 fallback,token 仍可用)。
    // **silent-failure-hunter C1 修**:cancel-aware wrap,user 在 userinfo
    // HTTP 卡时(慢网络)按 cancel 也立即生效,跟 step 1/2/3 cancel 契约一致
    let email_opt = match cancellable(&mut cancel_rx, async {
        Ok::<Option<String>, FlowError>(fetch_google_user_email(http, &token.access_token).await)
    })
    .await
    {
        Ok(opt) => opt,
        Err(FlowError::Cancelled) => {
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
                slot.take();
            }
            tracing::info!("OAuth login cancelled during userinfo fetch — 不持久化 token");
            return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
        }
        // 不可能命中 — fetch_google_user_email 自身从不返 FlowError(它返 Option)
        Err(_) => None,
    };

    // 4. 把 project_id + email 写进 token + 持久化(快路径,< 1ms)
    let mut token_with_project = token;
    token_with_project.project_id = Some(project_id.clone());
    // M1 修:无条件赋值 — None 也清掉(re-login 不同账号但 userinfo fail 时
    // 不要 leak 旧账号 email)。本调用路径 token.email 始终 None,实际等价
    token_with_project.email = email_opt;
    let store = match TokenStore::from_home_env() {
        Ok(s) => s,
        Err(e) => {
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e2, _)) if *e2 == my_epoch) {
                slot.take();
            }
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("home directory unavailable: {e}"),
            )
            .into_response();
        }
    };
    if let Err(e) = persist_token(&store, &token_with_project) {
        let mut slot = lock_cancel_slot();
        if matches!(slot.as_ref(), Some((e2, _)) if *e2 == my_epoch) {
            slot.take();
        }
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("token persist failed: {e}"),
        )
        .into_response();
    }

    // 5. 把 project_id 同步到 active provider extra
    if let Err(e) = sync_project_id_to_active_provider(&project_id) {
        let mut slot = lock_cancel_slot();
        if matches!(slot.as_ref(), Some((e2, _)) if *e2 == my_epoch) {
            slot.take();
        }
        tracing::error!(error = %e, "project_id sync 失败,login 整体回滚");
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!(
                "Login succeeded but failed to sync project_id to active provider config; \
                 please retry login. Detail: {e}"
            ),
        )
        .into_response();
    }

    // 全成功,清 slot(epoch 匹配时)
    {
        let mut slot = lock_cancel_slot();
        if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
            slot.take();
        }
    }

    Json(json!({
        "loggedIn": true,
        "email": token_with_project.email,
        "projectId": project_id,
        "expiresAt": token_with_project.expiry_date,
        "shouldRefresh": false,
    }))
    .into_response()
}

/// `DELETE /api/gemini-oauth/logout` — 删 token 文件 + 清 active provider 的
/// `cloud_code_project_id`。
async fn logout_handler() -> impl IntoResponse {
    let store = match TokenStore::from_home_env() {
        Ok(s) => s,
        Err(e) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("home directory unavailable: {e}"),
            )
            .into_response();
        }
    };
    if let Err(e) = store.delete() {
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("token delete failed: {e}"),
        )
        .into_response();
    }
    // 清 active provider 的 cloud_code_project_id (best-effort,失败不阻塞 logout)
    let _ = clear_project_id_from_active_provider();
    Json(json!({ "loggedIn": false })).into_response()
}

/// 把 project_id 写进当前 active provider 的 `extra.cloud_code_project_id` 字段,
/// 让 GeminiCliAdapter 能读到。仅当 active provider 是 `apiFormat=gemini_cli_oauth`
/// 时才写,其他 provider 不动。
///
/// 走 [`with_config_write`] 闭包模式 atomic RMW,防与并发 form save / desktop
/// switch_provider 等其他 RMW 路径互相 overwrite(H1 修)。
fn sync_project_id_to_active_provider(project_id: &str) -> Result<(), String> {
    with_config_write(|cfg| {
        let Some(active) = active_provider(cfg) else {
            return Err("no active provider".into());
        };
        if active.get("apiFormat").and_then(|v| v.as_str()) != Some("gemini_cli_oauth") {
            // skip 分支 — 不动 disk(chatgpt-codex P1 修:read-only-then-write
            // 退化路径会跟未迁的 raw load+save 并发覆盖)
            return Ok(ConfigMutation::Unchanged(()));
        }
        let active_id = active
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("active provider id missing")?
            .to_owned();
        let providers = cfg
            .as_object_mut()
            .and_then(|o| o.get_mut("providers"))
            .and_then(|v| v.as_array_mut())
            .ok_or("no providers array")?;
        for p in providers.iter_mut() {
            if p.get("id").and_then(|v| v.as_str()) == Some(active_id.as_str()) {
                let obj = p.as_object_mut().ok_or("provider not object")?;
                let extra = obj
                    .entry("extra".to_owned())
                    .or_insert_with(|| Value::Object(Default::default()));
                if let Some(extra_obj) = extra.as_object_mut() {
                    extra_obj.insert(
                        "cloud_code_project_id".into(),
                        Value::String(project_id.to_owned()),
                    );
                }
                break;
            }
        }
        Ok(ConfigMutation::Modified(()))
    })
}

/// logout 时清 active provider 的 `extra.cloud_code_project_id`。**镜像 sync**
/// 的 active+apiFormat 双 gate(silent-failure-hunter C1 修):原版无脑遍历所有
/// provider,会抹掉非 active / 非 gemini_cli_oauth 的 provider 的 project_id。
/// 用户多 OAuth 账号时会让其他 provider 莫名失效。
///
/// 走 [`with_config_write`] atomic RMW,同 sync(H1 修)。
fn clear_project_id_from_active_provider() -> Result<(), String> {
    with_config_write(|cfg| {
        let Some(active) = active_provider(cfg) else {
            // skip — 不动 disk(chatgpt-codex P1 修)
            return Ok(ConfigMutation::Unchanged(()));
        };
        if active.get("apiFormat").and_then(|v| v.as_str()) != Some("gemini_cli_oauth") {
            return Ok(ConfigMutation::Unchanged(()));
        }
        let active_id = active
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("active provider id missing")?
            .to_owned();
        let providers = cfg
            .as_object_mut()
            .and_then(|o| o.get_mut("providers"))
            .and_then(|v| v.as_array_mut())
            .ok_or("no providers array")?;
        // 跟踪是否真删了字段 — 没有的 provider 也走过遍历但实际无 mutation,
        // 应回 Unchanged 让 with_config_write 跳过 save
        let mut actually_removed = false;
        for p in providers.iter_mut() {
            if p.get("id").and_then(|v| v.as_str()) != Some(active_id.as_str()) {
                continue; // 只清 active provider
            }
            if let Some(obj) = p.as_object_mut() {
                if let Some(extra) = obj.get_mut("extra").and_then(|v| v.as_object_mut()) {
                    if extra.remove("cloud_code_project_id").is_some() {
                        actually_removed = true;
                    }
                }
            }
            break;
        }
        Ok(if actually_removed {
            ConfigMutation::Modified(())
        } else {
            ConfigMutation::Unchanged(())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::admin::handlers::common::test_support::with_isolated_home;
    use crate::admin::registry_io::with_config_write;
    use serde_json::json;

    /// **C1 修核心 contract**:LoginDoneGuard::Drop 必须把 epoch 写进
    /// login_done_channel,让 wait_for_login_epoch_complete 立即返。
    #[tokio::test]
    async fn login_done_guard_writes_epoch_to_channel() {
        // 拿 unique 大 epoch 防跟其他 test 串(channel 是进程级 sticky)
        let target = next_epoch() + 100_000;
        let waiter = tokio::spawn(async move {
            wait_for_login_epoch_complete(target).await;
            "completed"
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        {
            let _guard = LoginDoneGuard { epoch: target };
        }

        let result = tokio::time::timeout(std::time::Duration::from_millis(200), waiter).await;
        match result {
            Ok(Ok("completed")) => {}
            Ok(Ok(other)) => panic!("waiter 收到非预期值: {other}"),
            Ok(Err(e)) => panic!("waiter task panicked: {e:?}"),
            Err(_) => panic!("LoginDoneGuard::drop 没把 epoch 写进 channel"),
        }
    }

    /// **chatgpt-codex P2 修**(persist completion notification):guard.drop
    /// 在 await 之前触发,wait_for_login_epoch_complete 仍能立即看到 sticky
    /// 值返,**不**等 timeout。原 Notify::notify_waiters 设计这种场景信号丢失。
    #[tokio::test]
    async fn wait_returns_immediately_when_drop_happens_before_await() {
        let target = next_epoch() + 200_000;
        // 先 drop guard(模拟 cancel signal 在 Exit 钩子 await 之前到达)
        {
            let _g = LoginDoneGuard { epoch: target };
        }
        // 然后才 await — 必须立即返(读 sticky 值)
        let started = std::time::Instant::now();
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            wait_for_login_epoch_complete(target),
        )
        .await
        .expect("await 必须在 200ms 内返,实际超时表示 sticky 值没读到 → P2 修没生效");
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "early-drop 路径必须几乎瞬时返,实际 {:?}",
            elapsed
        );
    }

    /// **chatgpt-codex P1 修**(specific epoch):preemption 场景下,wait_for_
    /// login_epoch_complete(target=B) 不会被 newer epoch C 完成事件提早返。
    /// 只有 epoch >= target 时才返。
    #[tokio::test]
    async fn wait_for_specific_epoch_not_satisfied_by_lower_epoch() {
        let target_high = next_epoch() + 300_000;
        let lower = target_high - 1;
        // 先 drop 一个 lower epoch guard — 不应满足 wait(target_high)
        {
            let _g = LoginDoneGuard { epoch: lower };
        }
        // wait 应该 timeout(因为 lower < target_high,sticky 值不到 target)
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(150),
            wait_for_login_epoch_complete(target_high),
        )
        .await;
        assert!(
            result.is_err(),
            "lower epoch 完成不该满足 higher target,但 wait 提早返了 → P1 修没生效"
        );
        // 再 drop target_high guard — 现在应立即返
        {
            let _g = LoginDoneGuard { epoch: target_high };
        }
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            wait_for_login_epoch_complete(target_high),
        )
        .await
        .expect("matching epoch drop 后 wait 应立即返");
    }

    /// **核心 preemption race**:login B 抢占 login A 的 slot 后,A 的 post-flow
    /// 清理路径**不能**清掉 B 的 sender —— epoch token 校验保证。reviewer high #1
    /// 修(原版 raw take 会让 B 整段无法取消)。
    ///
    /// 模拟:手动模拟 login A 注册 + login B 抢占 + A 走完 post-flow 清理 +
    /// 验 slot 里仍是 B 的 sender(没被 A 误清)。
    #[test]
    fn cancel_slot_epoch_prevents_a_from_clearing_b_sender() {
        // 隔离前:清空残留(如果别的 test 留下来的)— take 不带 send 不影响
        {
            let _ = lock_cancel_slot().take();
        }
        // 1. login A 注册(C2:watch::channel 替代 oneshot)
        let epoch_a = next_epoch();
        let (tx_a, _rx_a) = watch::channel::<bool>(false);
        {
            let mut slot = lock_cancel_slot();
            slot.replace((epoch_a, tx_a));
        }
        // 2. login B 抢占 — 取出旧 sender 触发 cancel + 注册自己
        let epoch_b = next_epoch();
        let (tx_b, _rx_b) = watch::channel::<bool>(false);
        let prev_sender_taken_by_b = {
            let mut slot = lock_cancel_slot();
            slot.replace((epoch_b, tx_b))
        };
        assert!(prev_sender_taken_by_b.is_some(), "B 抢占应拿到 A 的 sender");
        let (taken_epoch, _) = prev_sender_taken_by_b.unwrap();
        assert_eq!(taken_epoch, epoch_a, "B 拿到的应是 A 的 sender");

        // 3. 模拟 A 的 post-flow 清理 — 用 epoch_a 校验,slot 当前是 epoch_b 不该清
        {
            let mut slot = lock_cancel_slot();
            if matches!(slot.as_ref(), Some((e, _)) if *e == epoch_a) {
                slot.take();
            }
        }
        // 4. 验 slot 仍含 B 的 sender(epoch_b)— 没被 A 的清理误清
        {
            let slot = lock_cancel_slot();
            match slot.as_ref() {
                Some((e, _)) => assert_eq!(
                    *e, epoch_b,
                    "**preemption race**:B 的 sender 应仍在 slot,实际 epoch={e}"
                ),
                None => panic!("B 的 sender 被 A 的清理误删 — race 没修"),
            }
        }
        // cleanup:把 B 也 take 掉防泄漏到下一个 test
        {
            let _ = lock_cancel_slot().take();
        }
    }

    #[test]
    fn routes_compile_and_paths_are_unique() {
        // smoke test:确保 routes() 编译且不 panic
        let _ = routes();
    }

    /// **M1 修核心 contract**:shared_oauth_http_client() 多次调用必须返同一
    /// instance(进程级 OnceLock pooling),不是每次新建。底层 connection
    /// pool / DNS cache / TLS session 才能跨 login 复用。
    #[test]
    fn shared_oauth_http_client_returns_same_instance_across_calls() {
        let c1 = shared_oauth_http_client().expect("init OK on test env");
        let c2 = shared_oauth_http_client().expect("init OK on test env");
        let c3 = shared_oauth_http_client().expect("init OK on test env");
        // 比指针地址 — Client 没实现 PartialEq,但 OnceLock 同一 init 必返同
        // 一引用,&'static 引用比较即指针等价
        assert!(
            std::ptr::eq(c1, c2) && std::ptr::eq(c2, c3),
            "shared_oauth_http_client 必须每次返同一 OnceLock 实例,实测不同 → 没复用 connection pool"
        );
    }

    /// **H3 修**(silent-failure-hunter):preemption race test 加端到端 signal
    /// 通路验证 — 老 sync test 只验 slot epoch 逻辑,没验 watch::Sender::send(true)
    /// → Receiver::changed().await 真触发。本测试用 #[tokio::test] 把 receiver
    /// await 起来,B 抢占 send(true) 后 A 的 receiver 必须立即看到 true。
    #[tokio::test]
    async fn preemption_actually_delivers_cancel_signal_to_receiver() {
        // 清空残留
        {
            let _ = lock_cancel_slot().take();
        }
        // 1. login A 注册:持 receiver 准备 await
        let epoch_a = next_epoch();
        let (tx_a, mut rx_a) = watch::channel::<bool>(false);
        {
            let mut slot = lock_cancel_slot();
            slot.replace((epoch_a, tx_a));
        }
        assert!(!*rx_a.borrow(), "初始 cancel 状态应为 false");

        // 2. spawn 一个 task 模拟 OAuth flow 的 cancel-aware select arm —
        // 等 receiver 看到 true 立即返。如果 send 没真触发,这个 task 永远 hang
        let watcher = tokio::spawn(async move {
            loop {
                if *rx_a.borrow() {
                    return "cancelled";
                }
                if rx_a.changed().await.is_err() {
                    return "sender_dropped"; // 发生时也 OK,等价 sender drop=cancel
                }
            }
        });

        // 3. login B 抢占
        let epoch_b = next_epoch();
        let (tx_b, _rx_b) = watch::channel::<bool>(false);
        let prev_sender = {
            let mut slot = lock_cancel_slot();
            slot.replace((epoch_b, tx_b))
        };
        // 关键:B 真的对 A 的 sender 触发 send(true) — 模拟 login_handler
        // 抢占语义里的 `let _ = prev_sender.send(true)`
        if let Some((_, tx_a_taken)) = prev_sender {
            let _ = tx_a_taken.send(true);
        }

        // 4. A 的 watcher 必须在 100ms 内收到 cancel(端到端 signal delivery 验证)
        let result = tokio::time::timeout(std::time::Duration::from_millis(100), watcher).await;
        match result {
            Ok(Ok(reason)) => {
                assert!(
                    reason == "cancelled" || reason == "sender_dropped",
                    "watcher 应收到 cancel,实际 {reason}"
                );
            }
            Ok(Err(e)) => panic!("watcher task panicked: {e:?}"),
            Err(_) => panic!("watcher 100ms 内没收到 cancel — preemption signal delivery 没生效"),
        }

        // cleanup
        {
            let _ = lock_cancel_slot().take();
        }
    }

    /// **H1 修**:cancel_in_flight_login 返 CancelOutcome 区分 cancelled /
    /// no-in-flight / poison-recovery 三种状态,response 携带 slotRecovered flag。
    #[test]
    fn cancel_no_in_flight_returns_distinguishable_outcome() {
        // 清空 slot 模拟 "无 in-flight"
        {
            let _ = lock_cancel_slot().take();
        }
        let outcome = cancel_in_flight_login();
        assert!(!outcome.cancelled, "无 in-flight 时 cancelled 应 false");
        assert!(
            !outcome.slot_recovered,
            "正常 lock 路径 slot_recovered 应 false"
        );
    }

    #[test]
    fn cancel_with_in_flight_returns_cancelled_true() {
        {
            let _ = lock_cancel_slot().take();
        }
        let epoch = next_epoch();
        let (tx, _rx) = watch::channel::<bool>(false);
        {
            let mut slot = lock_cancel_slot();
            slot.replace((epoch, tx));
        }
        let outcome = cancel_in_flight_login();
        assert!(outcome.cancelled, "有 in-flight 时 cancelled 应 true");
        assert!(
            !outcome.slot_recovered,
            "正常 lock 路径 slot_recovered false"
        );
        // cleanup not needed — cancel 已 take
    }

    /// 写一个特定 providers 数组到 disk(测试 fixture)
    fn seed_config(cfg_value: Value) {
        with_config_write(|cfg| {
            *cfg = cfg_value;
            Ok(ConfigMutation::Modified(()))
        })
        .unwrap();
    }

    /// 读出当前 providers 数组用于断言
    fn read_providers() -> Vec<Value> {
        with_config_write(|cfg| {
            Ok(ConfigMutation::Unchanged(
                cfg.get("providers")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
            ))
        })
        .unwrap()
    }

    /// G2 contract 1:active=gemini_cli_oauth → sync 把 project_id 写入 active provider 的 extra
    #[test]
    fn sync_writes_project_id_to_active_oauth_provider() {
        with_isolated_home(|_home| {
            seed_config(json!({
                "activeProvider": "p-oauth",
                "providers": [
                    {"id": "p-oauth", "apiFormat": "gemini_cli_oauth", "extra": {}},
                ]
            }));
            sync_project_id_to_active_provider("proj-xyz").unwrap();
            let providers = read_providers();
            assert_eq!(
                providers[0]["extra"]["cloud_code_project_id"], "proj-xyz",
                "active=gemini_cli_oauth 必须把 project_id 写入 extra"
            );
        });
    }

    /// G2 contract 2:active 不是 gemini_cli_oauth → sync 不动任何 provider(防写错 provider)
    #[test]
    fn sync_skips_when_active_is_not_oauth() {
        with_isolated_home(|_home| {
            seed_config(json!({
                "activeProvider": "p-other",
                "providers": [
                    {"id": "p-other", "apiFormat": "openai_chat", "extra": null},
                    {"id": "p-oauth", "apiFormat": "gemini_cli_oauth", "extra": {}},
                ]
            }));
            sync_project_id_to_active_provider("proj-xyz").unwrap();
            let providers = read_providers();
            assert!(
                providers[0]["extra"].is_null(),
                "active 不是 OAuth 时 active provider extra 不该被改"
            );
            assert!(
                providers[1]["extra"]["cloud_code_project_id"].is_null(),
                "active 不是 OAuth 时其他 OAuth provider 也不该被写"
            );
        });
    }

    /// G2 contract 3:**C1 回归 gate** — clear 只清 active 的 project_id,
    /// 其他 gemini_cli_oauth provider 的 project_id 必须保留(用户多账号场景)
    #[test]
    fn clear_preserves_other_oauth_providers_project_id() {
        with_isolated_home(|_home| {
            seed_config(json!({
                "activeProvider": "p-active",
                "providers": [
                    {"id": "p-active", "apiFormat": "gemini_cli_oauth",
                     "extra": {"cloud_code_project_id": "active-proj"}},
                    {"id": "p-other", "apiFormat": "gemini_cli_oauth",
                     "extra": {"cloud_code_project_id": "other-proj"}},
                ]
            }));
            clear_project_id_from_active_provider().unwrap();
            let providers = read_providers();
            assert!(
                providers[0]["extra"]["cloud_code_project_id"].is_null()
                    || providers[0]["extra"].get("cloud_code_project_id").is_none(),
                "active provider 的 project_id 必须被清"
            );
            assert_eq!(
                providers[1]["extra"]["cloud_code_project_id"], "other-proj",
                "**C1 回归 gate**:其他 OAuth provider 的 project_id 必须保留"
            );
        });
    }

    /// G2 contract 4:无 active provider → sync 返 Err(login 时必有 active),
    /// clear 返 Ok(logout 容忍无 active,best-effort 清理)
    #[test]
    fn sync_and_clear_no_active_provider_behavior() {
        with_isolated_home(|_home| {
            seed_config(json!({
                "providers": [],
                // activeProvider 缺失
            }));
            assert!(
                sync_project_id_to_active_provider("proj").is_err(),
                "sync 无 active 必须 Err — login 流必须有 active 才走到这"
            );
            assert!(
                clear_project_id_from_active_provider().is_ok(),
                "clear 无 active 应 Ok — logout best-effort 容忍"
            );
        });
    }

    // ── fetch_google_user_email tests(silent-failure-hunter C1 修补充)──
    //
    // 起 inline axum mock server,通过 Arc<Mutex<>> 注入 (status, body)
    // 让 handler 闭包 Send+Sync。验 4 条路径。

    use axum::extract::State;

    type MockResp = std::sync::Arc<(StatusCode, String)>;

    async fn mock_handler(State(resp): State<MockResp>) -> (StatusCode, String) {
        (resp.0, resp.1.clone())
    }

    /// 起一个一次性 axum server 返指定 (status, body),返 (port, JoinHandle)。
    async fn spawn_userinfo_mock(
        status: StatusCode,
        body: &str,
    ) -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let resp: MockResp = std::sync::Arc::new((status, body.to_owned()));
        let app = axum::Router::new()
            .route("/", axum::routing::get(mock_handler))
            .with_state(resp);
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        // 给 server 一点时间真起来
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        (port, handle)
    }

    #[tokio::test]
    async fn userinfo_returns_email_on_2xx_with_email_field() {
        let (port, h) =
            spawn_userinfo_mock(StatusCode::OK, r#"{"email":"u@example.com","sub":"123"}"#).await;
        let url = format!("http://127.0.0.1:{port}/");
        let result = fetch_google_user_email_at(&reqwest::Client::new(), &url, "fake-token").await;
        h.abort();
        assert_eq!(result, Some("u@example.com".to_owned()));
    }

    #[tokio::test]
    async fn userinfo_returns_none_on_500() {
        let (port, h) = spawn_userinfo_mock(StatusCode::INTERNAL_SERVER_ERROR, "boom").await;
        let url = format!("http://127.0.0.1:{port}/");
        let result = fetch_google_user_email_at(&reqwest::Client::new(), &url, "fake").await;
        h.abort();
        assert_eq!(result, None, "non-2xx 必须 fallback None");
    }

    #[tokio::test]
    async fn userinfo_returns_none_on_invalid_json() {
        let (port, h) = spawn_userinfo_mock(StatusCode::OK, "not valid json {").await;
        let url = format!("http://127.0.0.1:{port}/");
        let result = fetch_google_user_email_at(&reqwest::Client::new(), &url, "fake").await;
        h.abort();
        assert_eq!(result, None, "JSON parse 失败 fallback None");
    }

    #[tokio::test]
    async fn userinfo_returns_none_when_email_field_missing_or_empty() {
        let (port1, h1) = spawn_userinfo_mock(StatusCode::OK, r#"{"sub":"123"}"#).await;
        let r1 = fetch_google_user_email_at(
            &reqwest::Client::new(),
            &format!("http://127.0.0.1:{port1}/"),
            "fake",
        )
        .await;
        h1.abort();
        assert_eq!(r1, None, "缺 email 字段 fallback None");

        // 空 string — `.filter(|s| !s.is_empty())` 应该 reject
        let (port2, h2) = spawn_userinfo_mock(StatusCode::OK, r#"{"email":"","sub":"123"}"#).await;
        let r2 = fetch_google_user_email_at(
            &reqwest::Client::new(),
            &format!("http://127.0.0.1:{port2}/"),
            "fake",
        )
        .await;
        h2.abort();
        assert_eq!(r2, None, "空 string email 必须 reject 返 None");
    }
}
