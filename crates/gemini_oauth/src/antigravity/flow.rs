//! Antigravity OAuth code-grant flow + token refresh。
//!
//! 跟父 crate `flow.rs` **并行**:OAuth 流程逻辑相同(loopback callback server +
//! CSRF state + token exchange + cancel-aware select),只换常量。差异:
//! - 用 ANTIGRAVITY_PROVIDER 的 client_id / client_secret / scopes
//! - **固定 callback port 51121**(gemini-cli 是动态)
//! - auth URL 加 `prompt=consent` 强制每次重授权
//!
//! 不抽公共 trait(避免过早抽象),后续第 3 个 OAuth provider 再 refactor。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{extract::Query, response::Html, routing::get, Router};
use serde::Deserialize;
use tokio::sync::oneshot;

use super::super::constants::{
    ANTIGRAVITY_CALLBACK_PORT, ANTIGRAVITY_CLIENT_ID, ANTIGRAVITY_CLIENT_SECRET,
    ANTIGRAVITY_SCOPES, AUTH_ENDPOINT, REDIRECT_PATH, TOKEN_ENDPOINT,
};
use super::super::flow::{FlowError, OauthFlowConfig};
use super::super::token::OauthToken;

/// `/token` endpoint 的 wire 响应 shape — 跟 gemini-cli 共用。
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    token_type: String,
    expires_in: i64,
    scope: String,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CallbackQuery {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

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
    Malformed,
}

/// 跑完整 Antigravity OAuth code grant 流程,带 cancel signal 支持。
/// 详见父 crate `flow::run_oauth_flow_with_cancel` 文档,本 fn 仅 antigravity
/// 常量替换。
pub async fn run_antigravity_oauth_flow_with_cancel(
    http: &reqwest::Client,
    config: &OauthFlowConfig,
    mut cancel: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<OauthToken, FlowError> {
    // 1. bind loopback — antigravity 用**固定 port** 51121
    let bind_addr: SocketAddr = format!("127.0.0.1:{ANTIGRAVITY_CALLBACK_PORT}")
        .parse()
        .expect("固定 antigravity callback port 必须可解析");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{port}/oauth-callback");
    tracing::info!(port, "antigravity OAuth loopback server bound");

    // 2. CSRF state token + auth URL
    let state = random_state_token()?;
    let auth_url = build_antigravity_auth_url(&redirect_uri, &state);

    // 3. 起 loopback server,callback 通过 oneshot 回传
    let (tx, rx) = oneshot::channel::<CallbackResult>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));
    let app = Router::new().route(
        // CLIProxyAPI antigravity 用 `/oauth-callback`(不是 `/oauth2callback`)
        "/oauth-callback",
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
    let (server_err_tx, mut server_err_rx) = oneshot::channel::<std::io::Error>();
    let server_handle = tokio::spawn(async move {
        match axum::serve(listener, app).await {
            Ok(()) => {
                tracing::warn!("axum::serve 返 Ok 异常 — listener 已关闭,后续 callback 无法到达")
            }
            Err(e) => {
                let _ = server_err_tx.send(e);
            }
        }
    });

    // 4. on_auth_url callback 让 UI 提前知道 URL(同 gemini-cli)
    if let Some(callback) = &config.on_auth_url {
        callback(&auth_url);
    }

    // 5. 尝试 open browser
    if config.auto_open_browser {
        if let Err(e) = webbrowser::open(&auth_url) {
            tracing::warn!(
                error = %e,
                "antigravity webbrowser::open 失败,等 user 手动粘贴 URL"
            );
        }
    }

    // 6. 等待 callback / timeout / server 错 / cancellation
    let cancel_fut = async {
        match cancel.as_mut() {
            Some(rx) => {
                if *rx.borrow() {
                    return;
                }
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
        Ok(server_err) = &mut server_err_rx => {
            tracing::error!(error = %server_err, "antigravity loopback HTTP server crashed mid-flow");
            return Err(FlowError::Bind(server_err));
        }
        _ = cancel_fut => {
            tracing::info!("antigravity OAuth flow cancelled by caller; aborting");
            server_handle.abort();
            return Err(FlowError::Cancelled);
        }
    };
    server_handle.abort();

    // 7. 校验 state + 提取 code
    let code = match callback {
        CallbackResult::Code {
            code,
            state: returned_state,
        } => {
            if returned_state != state {
                tracing::error!(
                    expected_len = state.len(),
                    returned_len = returned_state.len(),
                    "antigravity OAuth state mismatch"
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
                description: Some("Google callback 既没 code 也没 error,极不正常".into()),
            });
        }
    };

    // 8. POST /token 换 access_token
    exchange_antigravity_code_for_token(http, &code, &redirect_uri).await
}

/// 用 refresh_token 刷新 antigravity access_token。
pub async fn refresh_antigravity_access_token(
    http: &reqwest::Client,
    refresh_token: &str,
    existing_id_token: Option<String>,
    existing_email: Option<String>,
    existing_project_id: Option<String>,
    existing_scope: Option<String>,
) -> Result<OauthToken, FlowError> {
    refresh_antigravity_access_token_at(
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

pub(crate) async fn refresh_antigravity_access_token_at(
    http: &reqwest::Client,
    token_endpoint: &str,
    refresh_token: &str,
    existing_id_token: Option<String>,
    existing_email: Option<String>,
    existing_project_id: Option<String>,
    existing_scope: Option<String>,
) -> Result<OauthToken, FlowError> {
    let params = [
        ("client_id", ANTIGRAVITY_CLIENT_ID),
        ("client_secret", ANTIGRAVITY_CLIENT_SECRET),
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

async fn exchange_antigravity_code_for_token(
    http: &reqwest::Client,
    code: &str,
    redirect_uri: &str,
) -> Result<OauthToken, FlowError> {
    exchange_antigravity_code_for_token_at(http, TOKEN_ENDPOINT, code, redirect_uri).await
}

pub(crate) async fn exchange_antigravity_code_for_token_at(
    http: &reqwest::Client,
    token_endpoint: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<OauthToken, FlowError> {
    let params = [
        ("client_id", ANTIGRAVITY_CLIENT_ID),
        ("client_secret", ANTIGRAVITY_CLIENT_SECRET),
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
        FlowError::TokenParse("antigravity 授权码 exchange 必须返 refresh_token,但响应没有".into())
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

/// 构造 Antigravity OAuth 授权 URL。比 gemini-cli 多 `prompt=consent`(强制
/// 每次重新授权)。query 参数顺序对齐 CLIProxyAPI `auth/antigravity/auth.go:60-68`。
pub fn build_antigravity_auth_url(redirect_uri: &str, state: &str) -> String {
    let scope = ANTIGRAVITY_SCOPES.join(" ");
    let params = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("access_type", "offline")
        .append_pair("client_id", ANTIGRAVITY_CLIENT_ID)
        .append_pair("prompt", "consent")
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", &scope)
        .append_pair("state", state)
        .finish();
    format!("{AUTH_ENDPOINT}?{params}")
}

fn random_state_token() -> Result<String, FlowError> {
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf).map_err(|e| FlowError::Rng(e.to_string()))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

fn now_ms_plus_secs(secs: i64) -> i64 {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    now_ms.saturating_add(secs.saturating_mul(1000))
}

/// 用户授权完成后浏览器看到的 HTML — 跟 gemini-cli 等价。
const CALLBACK_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Codex App Transfer — Antigravity Authorized</title>
<style>
body { font-family: -apple-system, system-ui, sans-serif; max-width: 600px; margin: 60px auto; padding: 0 20px; color: #333; }
h1 { color: #4caf50; }
p { line-height: 1.6; }
</style>
</head>
<body>
<h1>✓ Antigravity authorization complete</h1>
<p>You can close this window and return to <strong>Codex App Transfer</strong>.</p>
<p>授权完成,请关闭此窗口返回 <strong>Codex App Transfer</strong>。</p>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_auth_url_includes_prompt_consent_and_5_scopes() {
        let url = build_antigravity_auth_url("http://localhost:51121/oauth-callback", "abc");
        // 必须含 prompt=consent (跟 gemini-cli 关键差异)
        assert!(
            url.contains("prompt=consent"),
            "antigravity auth URL 必须含 prompt=consent,实际:{url}"
        );
        // 5 scopes 全在
        assert!(url.contains("cloud-platform"));
        assert!(url.contains("userinfo.email"));
        assert!(url.contains("userinfo.profile"));
        assert!(url.contains("cclog"));
        assert!(url.contains("experimentsandconfigs"));
        // antigravity client_id
        assert!(url.contains("client_id=1071006060591-"));
    }

    #[test]
    fn antigravity_callback_port_is_fixed_51121() {
        assert_eq!(ANTIGRAVITY_CALLBACK_PORT, 51121);
    }
}
