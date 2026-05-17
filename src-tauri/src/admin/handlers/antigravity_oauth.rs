//! `/api/antigravity-oauth/*` admin handlers — Antigravity Google OAuth 登录 /
//! 状态 / 注销 + Cloud Code Assist project bootstrap。
//!
//! 跟 [`super::gemini_oauth`] **并行** —— 两套 OAuth provider 各自独立 cancel
//! slot / login done channel / token 文件,互不影响。用户可同时登录两个 provider
//! (eg gemini-cli 跑日常 + antigravity 跑实验),互不抢占。
//!
//! ## 路由
//!
//! - `POST /api/antigravity-oauth/login`
//! - `GET /api/antigravity-oauth/status`
//! - `DELETE /api/antigravity-oauth/login/cancel`
//! - `DELETE /api/antigravity-oauth/logout`

use std::sync::{Arc, Mutex, OnceLock};

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use codex_app_transfer_gemini_oauth::{
    antigravity_bootstrap_project, antigravity_static_models, ensure_valid_antigravity_token,
    fetch_antigravity_available_models, persist_token, run_antigravity_oauth_flow_with_cancel,
    FlowError, OauthFlowConfig, TokenStore, ANTIGRAVITY_PROVIDER, ANTIGRAVITY_USERINFO_ENDPOINT,
};
use serde_json::{json, Value};
use tokio::sync::watch;

use super::super::registry_io::{with_config_write, ConfigMutation};
use super::super::state::AdminState;
use super::common::err;
use super::providers::active_provider;

// ── 进程级 cancel + done channel(独立于 gemini-cli)──────────────────

fn cancel_slot() -> &'static Mutex<Option<(u64, watch::Sender<bool>)>> {
    static SLOT: OnceLock<Mutex<Option<(u64, watch::Sender<bool>)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn next_epoch() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn lock_cancel_slot_with_poison_flag() -> (
    std::sync::MutexGuard<'static, Option<(u64, watch::Sender<bool>)>>,
    bool,
) {
    match cancel_slot().lock() {
        Ok(g) => (g, false),
        Err(poison) => {
            tracing::warn!(
                error_id = "ANTIGRAVITY_CANCEL_SLOT_POISONED",
                "antigravity cancel slot mutex poisoned by prior panic; recovering"
            );
            (poison.into_inner(), true)
        }
    }
}

fn lock_cancel_slot() -> std::sync::MutexGuard<'static, Option<(u64, watch::Sender<bool>)>> {
    lock_cancel_slot_with_poison_flag().0
}

#[derive(Debug, Clone, Copy)]
pub struct CancelOutcome {
    pub cancelled: bool,
    pub slot_recovered: bool,
    pub cancelled_epoch: Option<u64>,
}

pub fn cancel_in_flight_login() -> CancelOutcome {
    let (mut guard, slot_recovered) = lock_cancel_slot_with_poison_flag();
    let (cancelled, cancelled_epoch) = if let Some((epoch, sender)) = guard.take() {
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

fn login_done_channel() -> &'static (watch::Sender<u64>, watch::Receiver<u64>) {
    static C: OnceLock<(watch::Sender<u64>, watch::Receiver<u64>)> = OnceLock::new();
    C.get_or_init(|| watch::channel(0))
}

pub async fn wait_for_login_epoch_complete(target_epoch: u64) {
    let mut rx = login_done_channel().1.clone();
    loop {
        if *rx.borrow() >= target_epoch {
            return;
        }
        if rx.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

struct LoginDoneGuard {
    epoch: u64,
}
impl Drop for LoginDoneGuard {
    fn drop(&mut self) {
        let (tx, _) = login_done_channel();
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

// ── shared HTTP client (跟 gemini-cli 独立 pool 避免 connection 跨 provider 串)──

pub fn shared_antigravity_http_client() -> Result<&'static reqwest::Client, &'static str> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    let cell = CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| {
                tracing::error!(
                    error_id = "ANTIGRAVITY_HTTP_CLIENT_BUILDER_FAILED",
                    error = %e,
                    "antigravity reqwest::Client::builder failed"
                );
                format!("reqwest::Client::builder failed: {e}")
            })
    });
    match cell {
        Ok(c) => Ok(c),
        Err(_) => Err(
            "Antigravity HTTP client init failed (TLS/resolver issue); check ANTIGRAVITY_HTTP_CLIENT_BUILDER_FAILED log",
        ),
    }
}

// ── routes ─────────────────────────────────────────────────────────

pub fn routes() -> Router<AdminState> {
    Router::new()
        .route("/api/antigravity-oauth/status", get(status_handler))
        .route("/api/antigravity-oauth/login", post(login_handler))
        .route(
            "/api/antigravity-oauth/login/cancel",
            delete(cancel_login_handler),
        )
        .route("/api/antigravity-oauth/logout", delete(logout_handler))
        .route("/api/antigravity-oauth/models", get(models_handler))
}

async fn cancel_login_handler() -> impl IntoResponse {
    let outcome = cancel_in_flight_login();
    if outcome.cancelled {
        tracing::info!("antigravity OAuth login cancelled by user request");
    } else if outcome.slot_recovered {
        tracing::warn!(
            error_id = "ANTIGRAVITY_CANCEL_NOOP_AFTER_POISON",
            "antigravity cancel requested,no in-flight login but slot had been poison-recovered"
        );
    } else {
        tracing::debug!("antigravity cancel requested but no in-flight login");
    }
    Json(json!({
        "cancelled": outcome.cancelled,
        "slotRecovered": outcome.slot_recovered,
    }))
    .into_response()
}

async fn status_handler() -> impl IntoResponse {
    let store = match TokenStore::for_token_filename(ANTIGRAVITY_PROVIDER.token_filename) {
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
            format!("antigravity token store load: {e}"),
        )
        .into_response(),
    }
}

/// `GET /api/antigravity-oauth/models` — 拉 antigravity 上游可用模型列表。
///
/// 流程(对齐 CLIProxyAPI `cmd/fetch_antigravity_models`):
/// 1. 读 antigravity-oauth.json token,refresh 如过期(共享 service-level
///    single-flight mutex)
/// 2. POST 上游 `:fetchAvailableModels`(prod → daily → sandbox host fallback)
/// 3. 失败 → 退到静态种子(crate `static_models.rs` 内嵌的 10 条)
/// 4. 响应 OpenAI `/v1/models` shape:`{object:"list", data:[{id,object,owned_by,...}]}`
///
/// 未登录时返 401(让前端引导用户先点 OAuth login)
async fn models_handler() -> impl IntoResponse {
    let store = match TokenStore::for_token_filename(ANTIGRAVITY_PROVIDER.token_filename) {
        Ok(s) => s,
        Err(e) => {
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("home directory unavailable: {e}"),
            )
            .into_response()
        }
    };

    let http = match shared_antigravity_http_client() {
        Ok(c) => c,
        Err(msg) => return err(StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    };

    // refresh-on-demand:expired token → 服务层 single-flight refresh,返新
    // access_token。project_id 从 store 上次 persist 的 token 拿(refresh 不
    // 改 project_id 字段)
    let access_token = match ensure_valid_antigravity_token(http, &store).await {
        Ok(t) => t,
        Err(e) => {
            // ServiceError::NotLoggedIn 单独 401 让前端引导 OAuth 登录
            let msg = format!("{e}");
            if msg.to_lowercase().contains("not logged in")
                || msg.to_lowercase().contains("notloggedin")
            {
                return err(
                    StatusCode::UNAUTHORIZED,
                    "antigravity 未登录,请先 OAuth 登录".to_string(),
                )
                .into_response();
            }
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("antigravity token refresh failed: {msg}"),
            )
            .into_response();
        }
    };
    let project_id_owned = store.load().ok().flatten().and_then(|t| t.project_id);
    let project_id = project_id_owned.as_deref();

    // 试上游 fetchAvailableModels — 失败时降级静态种子(不上抛 5xx,UI 至少
    // 能拿到 sane default 让用户继续配 model 映射)
    let (models_json, source) =
        match fetch_antigravity_available_models(http, &access_token, project_id).await {
            Ok(models) if !models.is_empty() => {
                let arr: Vec<Value> = models
                    .into_iter()
                    .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
                    .collect();
                (arr, "upstream")
            }
            Ok(_) => {
                tracing::warn!(
                    error_id = "ANTIGRAVITY_MODELS_EMPTY",
                    "antigravity fetchAvailableModels 返空 list,退到静态种子"
                );
                let arr: Vec<Value> = antigravity_static_models()
                    .into_iter()
                    .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
                    .collect();
                (arr, "static_seed_empty_upstream")
            }
            Err(e) => {
                tracing::warn!(
                    error_id = "ANTIGRAVITY_MODELS_FETCH_FAIL",
                    error = %e,
                    "antigravity fetchAvailableModels 失败,退到静态种子"
                );
                let arr: Vec<Value> = antigravity_static_models()
                    .into_iter()
                    .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
                    .collect();
                (arr, "static_seed")
            }
        };

    Json(json!({
        "object": "list",
        "data": models_json,
        "source": source,
    }))
    .into_response()
}

async fn login_handler() -> impl IntoResponse {
    let my_epoch = next_epoch();
    let _done_guard = LoginDoneGuard { epoch: my_epoch };

    let http = match shared_antigravity_http_client() {
        Ok(c) => c,
        Err(msg) => return err(StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    };

    let mut config = OauthFlowConfig::default();
    config.on_auth_url = Some(Arc::new(|url: &str| {
        tracing::info!(
            auth_url = url,
            "antigravity OAuth auth URL 已生成 — 自动打开浏览器中"
        );
    }));

    // 注册 cancel sender + 抢占语义(同 gemini-cli 模式)
    let (cancel_tx, mut cancel_rx) = watch::channel::<bool>(false);
    {
        let mut slot = lock_cancel_slot();
        if let Some((_, prev_sender)) = slot.replace((my_epoch, cancel_tx)) {
            tracing::info!("抢占 in-flight antigravity OAuth login");
            let _ = prev_sender.send(true);
        }
    }

    // 跑 OAuth flow (cancel-aware)
    async fn cancellable<F, T>(
        cancel_rx: &mut watch::Receiver<bool>,
        fut: F,
    ) -> Result<T, FlowError>
    where
        F: std::future::Future<Output = Result<T, FlowError>>,
    {
        if *cancel_rx.borrow() {
            return Err(FlowError::Cancelled);
        }
        tokio::select! {
            res = fut => res,
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

    let flow_result =
        run_antigravity_oauth_flow_with_cancel(http, &config, Some(cancel_rx.clone())).await;
    let token = match flow_result {
        Ok(t) => t,
        Err(FlowError::Cancelled) => {
            cleanup_slot(my_epoch);
            tracing::info!("antigravity OAuth login cancelled — token 不持久化");
            return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
        }
        Err(e) => {
            cleanup_slot(my_epoch);
            return Json(json!({"loggedIn": false, "error": e.to_string()})).into_response();
        }
    };

    // bootstrap project (cancel-aware)
    let project_id = match cancellable(&mut cancel_rx, async {
        antigravity_bootstrap_project(http, &token.access_token)
            .await
            .map_err(|e| FlowError::TokenParse(format!("antigravity_bootstrap: {e}")))
    })
    .await
    {
        Ok(id) => id,
        Err(FlowError::Cancelled) => {
            cleanup_slot(my_epoch);
            tracing::info!("antigravity login cancelled during bootstrap_project");
            return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
        }
        Err(e) => {
            cleanup_slot(my_epoch);
            tracing::error!(error = %e, "antigravity bootstrap 失败 — token 不 persist");
            return err(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Antigravity authenticated but Cloud Code project provisioning failed; \
                     please retry login. Detail: {e}"
                ),
            )
            .into_response();
        }
    };

    // 终态 cancel check
    if *cancel_rx.borrow() {
        cleanup_slot(my_epoch);
        tracing::info!("antigravity login cancelled after bootstrap, before persist");
        return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
    }

    // 拿 email(cancel-aware)— 错误必须 surface,不能 swallow 成 None
    let userinfo = match cancellable(&mut cancel_rx, async {
        Ok::<UserInfoFetch, FlowError>(
            fetch_antigravity_user_email(http, &token.access_token).await,
        )
    })
    .await
    {
        Ok(info) => info,
        Err(FlowError::Cancelled) => {
            cleanup_slot(my_epoch);
            tracing::info!("antigravity login cancelled during userinfo fetch");
            return Json(json!({"loggedIn": false, "cancelled": true})).into_response();
        }
        Err(e) => {
            // cancellable 当前实现里 inner 永远返 Ok,这条理论 unreachable;
            // 留 fallback 而不是 unwrap_err 以防未来重构悄悄引入新 Err 变体
            tracing::error!(
                error_id = "ANTIGRAVITY_USERINFO_UNEXPECTED",
                error = %e,
                "cancellable wrap 返非 Cancelled 错误 — 不应发生"
            );
            UserInfoFetch {
                email: None,
                error: Some(format!("unexpected: {e}")),
            }
        }
    };

    // 写 token + persist
    let mut token_with_project = token;
    token_with_project.project_id = Some(project_id.clone());
    token_with_project.email = userinfo.email.clone();

    let store = match TokenStore::for_token_filename(ANTIGRAVITY_PROVIDER.token_filename) {
        Ok(s) => s,
        Err(e) => {
            cleanup_slot(my_epoch);
            return err(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("home directory unavailable: {e}"),
            )
            .into_response();
        }
    };
    if let Err(e) = persist_token(&store, &token_with_project) {
        cleanup_slot(my_epoch);
        return err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("antigravity token persist failed: {e}"),
        )
        .into_response();
    }

    // sync project_id 到 active provider extra(如果 active 是 antigravity_oauth)
    // sync 失败不算 fatal —— token + project_id 已 persist 到 antigravity-oauth.json,
    // adapter 在 gemini_cli/mod.rs 有 token-file fallback,所以下次 request 仍能拿到
    // project_id。500 让用户重 login 反而更糟(白跑 OAuth flow + 已写状态混乱)。
    // 改返 200 带 syncWarning 让前端可选 surface(2026-05-11 silent-failure-hunter I6)
    let sync_warning = match sync_project_id_to_active_provider(&project_id) {
        Ok(()) => None,
        Err(e) => {
            tracing::warn!(
                error_id = "ANTIGRAVITY_PROJECT_ID_SYNC",
                error = %e,
                "antigravity project_id sync 失败;token 已 persist,adapter 走 token-file fallback"
            );
            Some(format!("project_id sync to active provider failed: {e}"))
        }
    };

    cleanup_slot(my_epoch);

    Json(json!({
        "loggedIn": true,
        "email": token_with_project.email,
        "projectId": project_id,
        "expiresAt": token_with_project.expiry_date,
        "shouldRefresh": false,
        "userinfoError": userinfo.error,
        "syncWarning": sync_warning,
    }))
    .into_response()
}

async fn logout_handler() -> impl IntoResponse {
    let store = match TokenStore::for_token_filename(ANTIGRAVITY_PROVIDER.token_filename) {
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
            format!("antigravity token delete failed: {e}"),
        )
        .into_response();
    }
    if let Err(e) = clear_project_id_from_active_provider() {
        // 不阻断 logout —— token 已删,契约满足;但留 warn 让运维能 grep
        tracing::warn!(
            error_id = "ANTIGRAVITY_CLEAR_PROJECT_ID",
            error = %e,
            "antigravity logout 后 active provider extra.cloud_code_project_id 清理失败"
        );
    }
    Json(json!({ "loggedIn": false })).into_response()
}

fn cleanup_slot(my_epoch: u64) {
    let mut slot = lock_cancel_slot();
    if matches!(slot.as_ref(), Some((e, _)) if *e == my_epoch) {
        slot.take();
    }
}

/// userinfo 抓取结果 — 区分"成功但无 email"(`error=None, email=None`,200
/// OK 没 email 字段)与"transport/auth 错误"(`error=Some, email=None`)。
/// 后者必须 surface 给前端,不能像之前那样跟前者一样默写 null —— 否则用户
/// 永远看不到 userinfo 拉失败的原因(2026-05-11 silent-failure-hunter C2)
#[derive(Debug, Clone, Default)]
struct UserInfoFetch {
    email: Option<String>,
    error: Option<String>,
}

/// 拿 antigravity 登录账号 email — 用 v2 userinfo endpoint (跟 gemini-cli
/// 用 v3 openidconnect 不同;CLIProxyAPI `auth/antigravity/constants.go:24`)。
async fn fetch_antigravity_user_email(http: &reqwest::Client, access_token: &str) -> UserInfoFetch {
    let resp = match http
        .get(ANTIGRAVITY_USERINFO_ENDPOINT)
        .bearer_auth(access_token)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = format!("userinfo http failed: {e}");
            tracing::warn!(error_id = "ANTIGRAVITY_USERINFO_HTTP", error = %e, "{msg}");
            return UserInfoFetch {
                email: None,
                error: Some(msg),
            };
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let msg = format!("userinfo non-2xx: {status}");
        tracing::warn!(error_id = "ANTIGRAVITY_USERINFO_STATUS", status = %status, "{msg}");
        return UserInfoFetch {
            email: None,
            error: Some(msg),
        };
    }
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            let msg = format!("userinfo JSON parse failed: {e}");
            tracing::warn!(error_id = "ANTIGRAVITY_USERINFO_PARSE", error = %e, "{msg}");
            return UserInfoFetch {
                email: None,
                error: Some(msg),
            };
        }
    };
    UserInfoFetch {
        email: body
            .get("email")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_owned()),
        error: None,
    }
}

/// 写 active provider 的 `extra.cloud_code_project_id`(仅当 active 是
/// antigravity_oauth)。跟 gemini-cli 镜像 — 不同 apiFormat gate。
fn sync_project_id_to_active_provider(project_id: &str) -> Result<(), String> {
    with_config_write(|cfg| {
        let Some(active) = active_provider(cfg) else {
            return Err("no active provider".into());
        };
        if active.get("apiFormat").and_then(|v| v.as_str()) != Some("antigravity_oauth") {
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

fn clear_project_id_from_active_provider() -> Result<(), String> {
    with_config_write(|cfg| {
        let Some(active) = active_provider(cfg) else {
            return Ok(ConfigMutation::Unchanged(()));
        };
        if active.get("apiFormat").and_then(|v| v.as_str()) != Some("antigravity_oauth") {
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
        let mut actually_removed = false;
        for p in providers.iter_mut() {
            if p.get("id").and_then(|v| v.as_str()) != Some(active_id.as_str()) {
                continue;
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

    #[test]
    fn routes_compile() {
        let _ = routes();
    }

    #[test]
    fn cancel_with_no_in_flight_returns_false() {
        // 清空残留(其他 test 留下的)
        let _ = lock_cancel_slot().take();
        let outcome = cancel_in_flight_login();
        assert!(!outcome.cancelled);
    }
}
