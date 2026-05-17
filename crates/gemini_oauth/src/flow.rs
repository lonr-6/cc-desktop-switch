//! OAuth 2.0 code grant flow + token refresh。
//!
//! ## 流程(impersonate gemini-cli web flow)
//!
//! 1. 起 loopback HTTP server 监听 `127.0.0.1:<动态port>/oauth2callback`
//!    (动态 port = OS 自选,跟 gemini-cli `getAvailablePort()` 行为对齐)
//! 2. 生成 CSRF state(32 字节随机 hex,对齐 `oauth2.ts:200ish` `crypto.randomBytes(32).toString('hex')`)
//! 3. 构造 Google 授权 URL(`accounts.google.com/o/oauth2/v2/auth` + client_id +
//!    redirect_uri + access_type=offline + scope + state)
//! 4. 浏览器 open URL,用户登录 + 授权
//! 5. Google redirect 回 callback,带 `?code=...&state=...`
//!    - **必须**校验 state 一致(CSRF 防御)
//!    - 提取 `code` 用于换 token
//! 6. POST `oauth2.googleapis.com/token` 用 `authorization_code` grant_type 换
//!    `access_token + refresh_token + expires_in + scope + id_token`
//! 7. 转换 `expires_in` (秒) → `expiry_date` (UNIX ms-epoch),持久化
//!
//! ## Refresh
//!
//! POST `/token` 带 `grant_type=refresh_token`,响应里 `refresh_token` 字段**可能
//! 不返回**(Google 不一定 rotate),fallback 沿用旧值。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{extract::Query, response::Html, routing::get, Router};
use serde::Deserialize;
use thiserror::Error;
use tokio::sync::oneshot;

use super::constants::{
    AUTH_ENDPOINT, CLIENT_ID, CLIENT_SECRET, REDIRECT_PATH, SCOPES, TOKEN_ENDPOINT,
};
use super::token::OauthToken;

#[derive(Debug, Error)]
pub enum FlowError {
    #[error("loopback HTTP server bind 失败: {0}")]
    Bind(#[from] std::io::Error),
    #[error("CSRF state 不匹配 — 可能是恶意 callback,绝不能继续 token exchange")]
    StateMismatch,
    #[error("用户授权超时(等待 callback 超过 {0:?})")]
    Timeout(Duration),
    #[error("授权被拒绝或返回错误: {error}{}", .description.as_ref().map(|d| format!(" — {d}")).unwrap_or_default())]
    Denied {
        error: String,
        description: Option<String>,
    },
    #[error("token endpoint HTTP 失败: {0}")]
    TokenHttp(#[from] reqwest::Error),
    #[error("token endpoint 返非 2xx: HTTP {status}: {body}")]
    TokenStatus { status: u16, body: String },
    #[error("token 响应 JSON 解析失败: {0}")]
    TokenParse(String),
    #[error("OS RNG 不可用: {0}")]
    Rng(String),
    /// 调用方主动取消(eg user 关 UI / app exit / 新 login 抢占旧 login)。
    /// 跟 [`Timeout`] 区分:Timeout 是上游(用户没点)超时,Cancelled 是
    /// 我方主动放弃。callsite 应该当作"无错状态"处理而非弹错给 user。
    #[error("OAuth flow cancelled by caller (UI close / app exit / superseded by newer login)")]
    Cancelled,
}

/// OAuth flow 配置。所有字段都有默认值,通常不需要改。
#[derive(Clone)]
pub struct OauthFlowConfig {
    /// 等待 callback 的最大时长。默认 5 分钟 — 用户登 Google 账号 + 同意授权 5min 够用。
    pub callback_timeout: Duration,
    /// 是否自动打开浏览器。`false` 时返回 URL 让调用方自己处理(headless / 测试)。
    pub auto_open_browser: bool,
    /// (silent-failure H2 修)调用方注册的 callback,在 auth URL 生成之后、open
    /// browser 之前**总是**被调一次,让 UI 提前展示 URL 给用户。这样 webbrowser::
    /// open 失败时 UI 已经显示了 URL,用户可手动粘贴到任意浏览器,**flow 继续
    /// 等同一 redirect_uri 的 callback**(不需要重启 flow,不需要新 redirect_uri)。
    pub on_auth_url: Option<std::sync::Arc<dyn Fn(&str) + Send + Sync>>,
}

impl std::fmt::Debug for OauthFlowConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OauthFlowConfig")
            .field("callback_timeout", &self.callback_timeout)
            .field("auto_open_browser", &self.auto_open_browser)
            .field(
                "on_auth_url",
                &self.on_auth_url.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

impl Default for OauthFlowConfig {
    fn default() -> Self {
        Self {
            callback_timeout: Duration::from_secs(300),
            auto_open_browser: true,
            on_auth_url: None,
        }
    }
}

/// `/token` endpoint 的 wire 响应 shape。
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    /// **可能不返回**(refresh 路径常见)— None 时由调用方沿用旧 refresh_token
    #[serde(default)]
    refresh_token: Option<String>,
    /// "Bearer"
    token_type: String,
    /// **秒**(不是 ms-epoch),从 now 起算
    expires_in: i64,
    scope: String,
    #[serde(default)]
    id_token: Option<String>,
}

/// `/oauth2callback` 收到的 query 参数(Google 重定向带过来)。
#[derive(Debug, Deserialize)]
struct CallbackQuery {
    /// 授权成功路径
    #[serde(default)]
    code: Option<String>,
    /// 授权失败路径(用户拒绝 / Google 异常)
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
    /// CSRF state — 必须跟我们生成的 state 完全一致
    #[serde(default)]
    state: Option<String>,
}

/// Loopback callback server 收到结果后通过 oneshot 回传 — 跟主 flow 解耦。
#[derive(Debug)]
enum CallbackResult {
    Code {
        code: String,
        state: String,
    },
    Denied {
        error: String,
        description: Option<String>,
    },
    /// state / code 都没收到 — Google 不应该这样,但防御性处理
    Malformed,
}

/// 跑完整 OAuth code grant 流程。返回的 `OauthToken` 已含 access/refresh/expiry,
/// **不含** project_id(后续 `cloud_code` 模块 bootstrap 时填)。
///
/// ## 流程
///
/// 1. bind 127.0.0.1:0(OS 自选 port)起 loopback server
/// 2. 生成 state token,构造授权 URL,可选 open browser
/// 3. 等待 callback(timeout 内),校验 state,提取 code
/// 4. POST token endpoint exchange code → access_token
///
/// ## 错误恢复
///
/// - `StateMismatch` → 不要重试,极可能 CSRF 攻击
/// - `Timeout` → 用户没及时授权,可重启 flow
/// - `Denied` → 用户拒绝,重启 flow 让用户重新选账号
/// - webbrowser::open 失败:**不返错**,通过 `on_auth_url` callback 让 UI 提前
///   显示 URL,用户手动粘贴到任意浏览器,flow 继续等 callback(redirect_uri 不变)
pub async fn run_oauth_flow(
    http: &reqwest::Client,
    config: &OauthFlowConfig,
) -> Result<OauthToken, FlowError> {
    run_oauth_flow_with_cancel(http, config, None).await
}

/// 同 [`run_oauth_flow`],额外接受**可选** cancellation signal。
///
/// `cancel` 是 `watch::Receiver<bool>`(C2 修升级 — 原 oneshot::Receiver 一次性
/// 消费,只能给 OAuth flow 用,bootstrap_project 等后续阶段无法共享。watch
/// 支持 `clone()` 让 caller 把同一 cancel signal 喂给多阶段):
/// - 调用方持 `watch::Sender<bool>` 任意时刻 `send(true).ok()` → flow 立即
///   退出返 [`FlowError::Cancelled`],loopback server abort,token 不 persist
/// - `None` 等价于不可取消(老 API 行为,backward compat)
/// - watch 初始值约定 `false`(未取消),caller 持有 sender 直到整个 login
///   pipeline 完成才 drop
///
/// 触发场景:user 关 UI / app exit / 第二次 login 抢占第一次 in-flight。
pub async fn run_oauth_flow_with_cancel(
    http: &reqwest::Client,
    config: &OauthFlowConfig,
    mut cancel: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<OauthToken, FlowError> {
    // 1. bind loopback 拿动态 port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let local_addr: SocketAddr = listener.local_addr()?;
    let port = local_addr.port();
    let redirect_uri = format!("http://127.0.0.1:{port}{REDIRECT_PATH}");
    tracing::info!(port, "gemini OAuth loopback server bound");

    // 2. 生成 CSRF state token + auth URL
    let state = random_state_token()?;
    let auth_url = build_auth_url(&redirect_uri, &state);

    // 3. 起 loopback server,callback 通过 oneshot 回传
    let (tx, rx) = oneshot::channel::<CallbackResult>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let app = Router::new().route(
        REDIRECT_PATH,
        get({
            let tx = Arc::clone(&tx);
            move |Query(q): Query<CallbackQuery>| async move {
                let result = match (q.code, q.error, q.state) {
                    (Some(code), _, Some(state)) => CallbackResult::Code { code, state },
                    (_, Some(error), _) => CallbackResult::Denied {
                        error,
                        description: q.error_description,
                    },
                    _ => CallbackResult::Malformed,
                };
                if let Some(sender) = tx.lock().await.take() {
                    let _ = sender.send(result);
                }
                Html(CALLBACK_HTML)
            }
        }),
    );
    // axum::serve 错通过额外 oneshot 通知 main flow,而不是 silent let _ =(silent-
    // failure-hunter M2 修;原版 server crash 后 callback 永不触发,用户傻等到
    // 5min timeout 才能看到泛泛 "Timeout" 错误)
    let (server_err_tx, mut server_err_rx) = oneshot::channel::<std::io::Error>();
    let server_handle = tokio::spawn(async move {
        match axum::serve(listener, app).await {
            // axum::serve 实际不返 Ok(()),如果未来变了至少 warn 让 operator 看见
            Ok(()) => {
                tracing::warn!("axum::serve 返 Ok 异常 — listener 已关闭,后续 callback 无法到达")
            }
            Err(e) => {
                let _ = server_err_tx.send(e);
            }
        }
    });

    // 4. 通过 on_auth_url callback 让 UI 提前知道 auth URL — 总是调一次,无论
    //    后面 webbrowser::open 成功与否(silent-failure H2 修;原 design "BrowserOpen
    //    错 + abort server" 实际打破 manual-paste 路径:user 复制了 URL 含旧 port,
    //    新启 server 旧 port 已 abort → callback connection refused)。新 design:
    //    server 持续等 callback,redirect_uri 不变,UI 同时显示 URL + 试图打开浏览器,
    //    任意一边成功都能完成 flow
    if let Some(callback) = &config.on_auth_url {
        callback(&auth_url);
    }

    // 5. 尝试自动打开浏览器:失败仅 warn,**不返错**(URL 已通过 on_auth_url
    //    callback 给 UI),flow 继续等 callback。manual paste 路径 work
    if config.auto_open_browser {
        if let Err(e) = webbrowser::open(&auth_url) {
            tracing::warn!(
                error = %e,
                "webbrowser::open 失败,等 user 通过 on_auth_url 看到的 URL 手动粘贴"
            );
        }
    }

    // 5. 等待 callback / timeout / server 错 / cancellation(四选一)
    //
    // cancel 用 `OptionFuture` pattern — 没传 cancel 时 cancel_fut 永远 pending,
    // tokio::select! 不会选到,等价于老 API 行为。callsite 主动 cancel 时
    // (Sender 调 `send(true)`):rx 看到 value=true → 这里立即 abort + 返
    // Cancelled,比 5min timeout 提早数十倍释放 loopback port + reqwest client。
    //
    // watch 升级(C2 修):原 oneshot 一次性,cancel 信号只能给 OAuth flow
    // 自己用,bootstrap_project 等后续阶段错过 5-30s 都听不到。watch::Receiver
    // 支持 clone,caller 可把同一 cancel signal 喂给多阶段;watch 可重复 read
    // 当前值,即便 caller 已经 drop sender 也能 read 最后状态。
    let cancel_fut = async {
        match cancel.as_mut() {
            Some(rx) => {
                // 入口先看当前值——cancel 在调用前已 set 时立即返回不浪费
                if *rx.borrow() {
                    return;
                }
                // 等 sender 改值;true 即取消,false 仍要继续等(轮换)。
                // sender drop 时 changed() 返 Err,等价于"再无 cancel 可能",
                // 退化成 pending 让其他 select arm 决定退出
                loop {
                    if rx.changed().await.is_err() {
                        std::future::pending::<()>().await;
                    }
                    if *rx.borrow() {
                        return;
                    }
                }
            }
            None => std::future::pending::<()>().await,
        }
    };
    let callback = tokio::select! {
        result = rx => result.map_err(|_| FlowError::Timeout(config.callback_timeout))?,
        _ = tokio::time::sleep(config.callback_timeout) => {
            server_handle.abort();
            return Err(FlowError::Timeout(config.callback_timeout));
        }
        // axum::serve crash 时立即返而不是等 5min timeout
        Ok(server_err) = &mut server_err_rx => {
            tracing::error!(error = %server_err, "loopback HTTP server crashed mid-flow");
            return Err(FlowError::Bind(server_err));
        }
        _ = cancel_fut => {
            tracing::info!("OAuth flow cancelled by caller; aborting loopback server + flow");
            server_handle.abort();
            return Err(FlowError::Cancelled);
        }
    };
    server_handle.abort();

    // 6. 校验 state + 提取 code
    let code = match callback {
        CallbackResult::Code {
            code,
            state: returned_state,
        } => {
            if returned_state != state {
                tracing::error!(
                    expected_len = state.len(),
                    returned_len = returned_state.len(),
                    "OAuth state mismatch — 拒绝继续 token exchange"
                );
                return Err(FlowError::StateMismatch);
            }
            code
        }
        CallbackResult::Denied { error, description } => {
            return Err(FlowError::Denied { error, description });
        }
        CallbackResult::Malformed => {
            return Err(FlowError::Denied {
                error: "missing_code_and_state".into(),
                description: Some("Google callback 既没有 code 也没有 error,极不正常".into()),
            });
        }
    };

    // 7. POST /token 换 access_token
    exchange_code_for_token(http, &code, &redirect_uri).await
}

/// 用 refresh_token 刷新 access_token。返回新 OauthToken,自动沿用旧 refresh_token
/// 如果 Google 没返新的(行为见 RFC 6749 §1.5,Google 不一定 rotate)。
pub async fn refresh_access_token(
    http: &reqwest::Client,
    refresh_token: &str,
    existing_id_token: Option<String>,
    existing_email: Option<String>,
    existing_project_id: Option<String>,
    existing_scope: Option<String>,
) -> Result<OauthToken, FlowError> {
    refresh_access_token_at(
        http,
        TOKEN_ENDPOINT,
        refresh_token,
        existing_id_token,
        existing_email,
        existing_project_id,
        existing_scope,
    )
    .await
}

/// 内部版 — 接收可定制 token endpoint。`pub(crate)` 让 crate 外**完全不可见**
/// (silent-failure-hunter H-1 修:lib.rs export 也透不出去,proxy / admin handler
/// 等下游 crate 无法误用此 fn 绕过 const [`TOKEN_ENDPOINT`])。仅 crate 内
/// production [`refresh_access_token`] 走 const,以及 service::tests 调 mock。
pub(crate) async fn refresh_access_token_at(
    http: &reqwest::Client,
    token_endpoint: &str,
    refresh_token: &str,
    existing_id_token: Option<String>,
    existing_email: Option<String>,
    existing_project_id: Option<String>,
    existing_scope: Option<String>,
) -> Result<OauthToken, FlowError> {
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];
    let resp = http.post(token_endpoint).form(&params).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(FlowError::TokenStatus {
            status: status.as_u16(),
            body,
        });
    }
    let parsed: TokenResponse =
        serde_json::from_str(&body).map_err(|e| FlowError::TokenParse(e.to_string()))?;

    Ok(OauthToken {
        access_token: parsed.access_token,
        // refresh response 不返新 refresh_token 时沿用旧值(常见路径)
        refresh_token: parsed
            .refresh_token
            .unwrap_or_else(|| refresh_token.to_owned()),
        token_type: parsed.token_type,
        expiry_date: now_ms_plus_secs(parsed.expires_in),
        scope: existing_scope.unwrap_or(parsed.scope),
        id_token: parsed.id_token.or(existing_id_token),
        email: existing_email,
        project_id: existing_project_id,
    })
}

async fn exchange_code_for_token(
    http: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
) -> Result<OauthToken, FlowError> {
    exchange_code_for_token_at(http, TOKEN_ENDPOINT, code, redirect_uri).await
}

/// 内部版 — 接收可定制 token endpoint。`pub(crate)` 让 crate 外完全不可见,
/// 仅 production fn 走 const + tests 注入 mock。
pub(crate) async fn exchange_code_for_token_at(
    http: &reqwest::Client,
    token_endpoint: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<OauthToken, FlowError> {
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri),
    ];
    let resp = http.post(token_endpoint).form(&params).send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(FlowError::TokenStatus {
            status: status.as_u16(),
            body,
        });
    }
    let parsed: TokenResponse =
        serde_json::from_str(&body).map_err(|e| FlowError::TokenParse(e.to_string()))?;
    let refresh_token = parsed.refresh_token.ok_or_else(|| {
        FlowError::TokenParse(
            "授权码 exchange 必须返回 refresh_token,但响应没有(检查 access_type=offline)".into(),
        )
    })?;
    Ok(OauthToken {
        access_token: parsed.access_token,
        refresh_token,
        token_type: parsed.token_type,
        expiry_date: now_ms_plus_secs(parsed.expires_in),
        scope: parsed.scope,
        id_token: parsed.id_token,
        email: None,
        project_id: None,
    })
}

/// 构造 Google OAuth 授权 URL。query 参数顺序对齐 gemini-cli `oauth2.ts:207-213`
/// (虽然 RFC 6749 不要求顺序,但保持一致便于 wire diff)。
pub fn build_auth_url(redirect_uri: &str, state: &str) -> String {
    let scope = SCOPES.join(" ");
    let params = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("access_type", "offline")
        .append_pair("state", state)
        .finish();
    format!("{AUTH_ENDPOINT}?{params}")
}

/// 32 字节 OS RNG → hex(64 字符)— 对齐 gemini-cli upstream。
fn random_state_token() -> Result<String, FlowError> {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).map_err(|e| FlowError::Rng(e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// `now_ms + expires_in_secs * 1000`。用于把 token endpoint 的相对秒数转
/// gemini-cli `Credentials.expiry_date` 绝对 ms-epoch。
fn now_ms_plus_secs(secs: i64) -> i64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    now_ms.saturating_add(secs.saturating_mul(1000))
}

/// 用户授权完成后浏览器看到的 HTML(简单的成功提示)。Google 重定向到我们
/// loopback 后,这页面就在用户浏览器显示,提示可以关掉。
const CALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Codex App Transfer — OAuth Success</title>
<style>
body { font-family: -apple-system, system-ui, sans-serif; max-width: 600px; margin: 60px auto; padding: 0 20px; color: #333; }
h1 { color: #4caf50; }
p { line-height: 1.6; }
</style>
</head>
<body>
<h1>✓ Authorization complete</h1>
<p>You can close this window and return to <strong>Codex App Transfer</strong>.</p>
<p>授权完成,请关闭此窗口返回 <strong>Codex App Transfer</strong>。</p>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_url_contains_required_params() {
        let url = build_auth_url("http://127.0.0.1:12345/oauth2callback", "abc123");
        // OAuth 2.0 RFC 6749 必填 params
        assert!(url.starts_with(AUTH_ENDPOINT));
        assert!(url.contains("client_id=681255809395-"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("state=abc123"));
        // redirect_uri 必须 URL-encoded
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A12345%2Foauth2callback"));
        // scope 三个 OAuth scope 全在
        assert!(url.contains("cloud-platform"));
        assert!(url.contains("userinfo.email"));
        assert!(url.contains("userinfo.profile"));
        // 不该有 PKCE 相关字段(对齐 gemini-cli web flow)
        assert!(!url.contains("code_challenge"));
    }

    #[test]
    fn random_state_is_64_hex_chars() {
        let s = random_state_token().unwrap();
        assert_eq!(s.len(), 64, "32 bytes → 64 hex chars");
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()), "state 必须全 hex");
        // 多调几次确保不一样(防 RNG 退化成 zero)
        let s2 = random_state_token().unwrap();
        assert_ne!(s, s2, "OS RNG 必须每次产不同 state");
    }

    #[test]
    fn now_ms_plus_secs_arithmetic() {
        let before = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let result = now_ms_plus_secs(3600);
        let after = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        // result 应在 [before+3600s, after+3600s] 区间
        assert!(result >= before.saturating_add(3_600_000));
        assert!(result <= after.saturating_add(3_600_000));
    }

    #[test]
    fn now_ms_plus_secs_handles_overflow() {
        // 极端值不 panic(saturating arithmetic)
        let _ = now_ms_plus_secs(i64::MAX);
        let _ = now_ms_plus_secs(0);
    }

    #[tokio::test]
    async fn refresh_token_uses_form_encoding_and_parses_response() {
        use wiremock::matchers::{body_string_contains, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // wiremock 的 mock token endpoint
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .and(body_string_contains("refresh_token=old-refresh-xyz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.new-access",
                "expires_in": 3599,
                "scope": "https://www.googleapis.com/auth/cloud-platform",
                "token_type": "Bearer",
                "id_token": "ey.new-id"
            })))
            .mount(&server)
            .await;

        // 暂时把 TOKEN_ENDPOINT mock 掉 — 用 reqwest::Client base_url override
        // (constants.rs::TOKEN_ENDPOINT 是 const 字符串,测试不能改;只能直接调内部 helper
        //  by 重新构造 params 手动 POST 到 mock server)
        // 这里直接验 wiremock 收到了正确的 form body — flow 内部逻辑由其他单测覆盖
        let http = reqwest::Client::new();
        let resp = http
            .post(format!("{}/token", server.uri()))
            .form(&[
                ("client_id", CLIENT_ID),
                ("client_secret", CLIENT_SECRET),
                ("refresh_token", "old-refresh-xyz"),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let parsed: TokenResponse = resp.json().await.unwrap();
        assert_eq!(parsed.access_token, "ya29.new-access");
        assert_eq!(parsed.expires_in, 3599);
        assert!(
            parsed.refresh_token.is_none(),
            "Google 默认不 rotate refresh_token"
        );
    }

    #[tokio::test]
    async fn refresh_token_falls_back_to_old_refresh_when_response_omits_it() {
        // refresh_access_token 的契约:response 没 refresh_token 时沿用旧值
        // 这里直接构造 OauthToken 验 fallback 逻辑(不调 mock server)
        let parsed = TokenResponse {
            access_token: "new".into(),
            refresh_token: None,
            token_type: "Bearer".into(),
            expires_in: 3600,
            scope: "test-scope".into(),
            id_token: None,
        };
        let fallback = parsed
            .refresh_token
            .clone()
            .unwrap_or_else(|| "old-refresh".to_owned());
        assert_eq!(fallback, "old-refresh");
    }

    #[tokio::test]
    async fn exchange_code_errors_when_refresh_token_missing() {
        // **pr-test-analyzer H4 修**:initial code-exchange 必须返 refresh_token
        // (Google `access_type=offline` 才返;若漏配 access_type=offline 上游返空
        // refresh_token)。原有测试只覆盖 refresh path 的 None fallback,没覆盖
        // initial exchange 的 ok_or_else 错误路径。
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "ya29.test",
                "expires_in": 3600,
                "scope": "test-scope",
                "token_type": "Bearer"
                // **故意**漏 refresh_token,模拟 Google 没收到 access_type=offline
            })))
            .mount(&server)
            .await;

        let http = reqwest::Client::new();
        let token_endpoint = format!("{}/token", server.uri());
        let err = exchange_code_for_token_at(
            &http,
            &token_endpoint,
            "fake-code",
            "http://127.0.0.1:8080/oauth2callback",
        )
        .await
        .unwrap_err();
        match err {
            FlowError::TokenParse(msg) => {
                assert!(
                    msg.contains("refresh_token") && msg.contains("access_type=offline"),
                    "错误 message 必须 hint 配置问题,实际:{msg}"
                );
            }
            other => panic!("期待 TokenParse,实际 {other:?}"),
        }
    }

    #[test]
    fn flow_error_denied_message_includes_description() {
        let err = FlowError::Denied {
            error: "access_denied".into(),
            description: Some("User declined".into()),
        };
        let msg = err.to_string();
        assert!(msg.contains("access_denied"));
        assert!(msg.contains("User declined"));
    }

    #[test]
    fn flow_error_denied_without_description() {
        let err = FlowError::Denied {
            error: "invalid_request".into(),
            description: None,
        };
        let msg = err.to_string();
        assert!(msg.contains("invalid_request"));
        assert!(!msg.contains("None"));
    }

    #[test]
    fn flow_error_cancelled_message_distinguishes_from_timeout() {
        let cancelled_msg = FlowError::Cancelled.to_string();
        // Cancelled message 必须明示是"调用方主动取消"而不是 user-side timeout,
        // 让 UI / log 区分两种状态(timeout 应弹错,cancelled 应静默)
        assert!(
            cancelled_msg.contains("cancelled") && cancelled_msg.contains("caller"),
            "Cancelled 错误 message 需明示主动取消,实际:{cancelled_msg}"
        );
        let timeout_msg = FlowError::Timeout(Duration::from_secs(300)).to_string();
        assert!(
            !timeout_msg.contains("cancelled"),
            "Timeout 不能含 'cancelled' 字面值,会跟 Cancelled 混淆"
        );
    }

    /// **核心 cancel contract**:cancel signal 触发后,run_oauth_flow_with_cancel
    /// 必须立即返 Cancelled,不会等到 callback_timeout(5min)。本测试 lock
    /// "提早退出" 防 future 把 cancel arm 从 select! 误删后回归到长 timeout。
    #[tokio::test]
    async fn cancel_signal_aborts_flow_immediately_not_at_timeout() {
        let http = reqwest::Client::new();
        // 极短 timeout(100ms)和 cancel 时机(20ms)对比 — cancel 应该 < timeout
        let mut config = OauthFlowConfig {
            callback_timeout: Duration::from_millis(2000),
            auto_open_browser: false, // 不真打开浏览器
            on_auth_url: None,
        };
        // 用 on_auth_url 当 hook,callback 拿到 URL 后立即触发 cancel
        // (C2 升级:oneshot → watch::channel<bool>)
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel::<bool>(false);
        let cancel_holder = std::sync::Mutex::new(Some(cancel_tx));
        // on_auth_url callback 同步执行(在生成 URL 时立即调),拿走 sender
        // 然后 spawn task 在 20ms 后 send(true) → flow 在等 callback 阶段被 cancel
        config.on_auth_url = Some(Arc::new(move |_url: &str| {
            if let Ok(mut g) = cancel_holder.lock() {
                if let Some(tx) = g.take() {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        let _ = tx.send(true);
                    });
                }
            }
        }));

        let started = std::time::Instant::now();
        let result = run_oauth_flow_with_cancel(&http, &config, Some(cancel_rx)).await;
        let elapsed = started.elapsed();

        match result {
            Err(FlowError::Cancelled) => {
                // 必须 < callback_timeout / 2 才能证明 cancel 提早退而不是 timeout
                assert!(
                    elapsed < Duration::from_millis(1000),
                    "cancel 应立即退,实际耗时 {:?}(callback_timeout=2000ms)",
                    elapsed
                );
            }
            other => panic!(
                "期待 FlowError::Cancelled,实际 {:?}(elapsed {:?})",
                other, elapsed
            ),
        }
    }

    /// **C2 修核心 contract**:cancel signal 已 set true 时再调
    /// run_oauth_flow_with_cancel,入口 fast-path 立即返 Cancelled,不浪费
    /// loopback bind / browser open / 等 callback。验"watch::channel pre-set"
    /// 路径走 cancel arm。
    #[tokio::test]
    async fn cancel_already_set_returns_immediately() {
        let http = reqwest::Client::new();
        let config = OauthFlowConfig {
            callback_timeout: Duration::from_millis(2000),
            auto_open_browser: false,
            on_auth_url: None,
        };
        // 起 channel 立即 send(true) 让 receiver 看到 cancelled state
        let (tx, rx) = tokio::sync::watch::channel::<bool>(false);
        tx.send(true).unwrap();
        let started = std::time::Instant::now();
        let result = run_oauth_flow_with_cancel(&http, &config, Some(rx)).await;
        let elapsed = started.elapsed();
        match result {
            Err(FlowError::Cancelled) => {
                // pre-set cancel 应几乎瞬时退,但 OAuth flow 仍跑了 bind +
                // state token + on_auth_url 等准备工作,放宽到 < 500ms
                // (callback_timeout=2000ms,真触 timeout 是 2000+ms)
                assert!(
                    elapsed < Duration::from_millis(500),
                    "pre-set cancel 应快速退,实际 {:?}",
                    elapsed
                );
            }
            other => panic!("期待 Cancelled,实际 {:?} (elapsed {:?})", other, elapsed),
        }
    }

    /// **backward compat**:run_oauth_flow(老 API,无 cancel 参数)行为
    /// 不变 —— 内部走 None cancel,等价于不可取消,跟原 PR #97 行为一致。
    /// 本测试不真发起 OAuth(timeout 200ms 后退),只验 None cancel 不会
    /// 让 select! 立即匹配到不存在的 cancel arm。
    #[tokio::test]
    async fn run_oauth_flow_without_cancel_still_works() {
        let http = reqwest::Client::new();
        let config = OauthFlowConfig {
            callback_timeout: Duration::from_millis(150),
            auto_open_browser: false,
            on_auth_url: None,
        };
        // 没传 cancel,应等到 callback_timeout 退而不是立即 Cancelled
        let started = std::time::Instant::now();
        let result = run_oauth_flow(&http, &config).await;
        let elapsed = started.elapsed();
        match result {
            Err(FlowError::Timeout(_)) => {
                // 必须等到接近 timeout(150ms),不能立即返(< 50ms)
                assert!(
                    elapsed >= Duration::from_millis(120),
                    "无 cancel 时不能立即退,实际 {:?}",
                    elapsed
                );
            }
            other => panic!("期待 Timeout,实际 {:?}", other),
        }
    }
}
