//! `/api/feedback` —— 反馈提交 + 节流 + 附件打包.

use std::fs;
use std::path::Path as FsPath;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use codex_app_transfer_codex_integration::CodexPaths;
use codex_app_transfer_proxy::{proxy_log_dir, recent_feedback_bundles};
use reqwest::{header::CONTENT_TYPE, multipart};
use serde_json::{json, Value};

use super::super::registry_io::load as load_registry;
use super::common::{active_provider_name, current_epoch_secs, err, APP_VERSION};

pub(super) const FEEDBACK_WORKER_URL: &str =
    "https://codex-app-transfer-feedback.alysechencn.workers.dev";

pub(super) struct FeedbackThrottleState {
    pub(super) last_success: Option<Instant>,
    pub(super) failure_ts: Vec<Instant>,
    pub(super) failure_cooldown_until: Option<Instant>,
}

pub(super) struct FeedbackThrottle {
    pub(super) inner: Mutex<FeedbackThrottleState>,
}

impl FeedbackThrottle {
    pub(super) const SUCCESS_COOLDOWN: Duration = Duration::from_secs(60);
    pub(super) const FAILURE_WINDOW: Duration = Duration::from_secs(300);
    pub(super) const FAILURE_LIMIT: usize = 5;
    pub(super) const FAILURE_COOLDOWN: Duration = Duration::from_secs(60);

    pub(super) fn new() -> Self {
        Self {
            inner: Mutex::new(FeedbackThrottleState {
                last_success: None,
                failure_ts: Vec::new(),
                failure_cooldown_until: None,
            }),
        }
    }

    pub(super) fn acquire(&self) -> Result<(), String> {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();

        if let Some(last_success) = inner.last_success {
            let elapsed = now.saturating_duration_since(last_success);
            if elapsed < Self::SUCCESS_COOLDOWN {
                let wait = Self::SUCCESS_COOLDOWN.saturating_sub(elapsed).as_secs();
                return Err(format!(
                    "just submitted; wait {wait}s before sending another feedback"
                ));
            }
        }

        if let Some(until) = inner.failure_cooldown_until {
            if now < until {
                let wait = until.saturating_duration_since(now).as_secs();
                return Err(format!(
                    "too many consecutive failures; wait {wait}s before retrying"
                ));
            }
        }

        inner
            .failure_ts
            .retain(|ts| now.saturating_duration_since(*ts) < Self::FAILURE_WINDOW);
        Ok(())
    }

    pub(super) fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.last_success = Some(Instant::now());
        inner.failure_ts.clear();
        inner.failure_cooldown_until = None;
    }

    pub(super) fn record_failure(&self) {
        let now = Instant::now();
        let mut inner = self.inner.lock().unwrap();
        inner
            .failure_ts
            .retain(|ts| now.saturating_duration_since(*ts) < Self::FAILURE_WINDOW);
        inner.failure_ts.push(now);
        if inner.failure_ts.len() >= Self::FAILURE_LIMIT {
            inner.failure_cooldown_until = Some(now + Self::FAILURE_COOLDOWN);
        }
    }
}

static FEEDBACK_THROTTLE: OnceLock<FeedbackThrottle> = OnceLock::new();

pub(super) fn feedback_throttle() -> &'static FeedbackThrottle {
    FEEDBACK_THROTTLE.get_or_init(FeedbackThrottle::new)
}

pub(super) fn feedback_worker_url(raw: &str) -> Result<&str, String> {
    let url = raw.trim();
    if url.is_empty() {
        Err("feedback service is not configured".to_owned())
    } else {
        Ok(url)
    }
}

pub(super) fn multipart_text_part(text: String, mime: &str) -> multipart::Part {
    multipart::Part::text(text.clone())
        .mime_str(mime)
        .unwrap_or_else(|_| multipart::Part::text(text))
}

pub(super) fn feedback_proxy_tail_content(path: &FsPath) -> Option<String> {
    let content = fs::read(path).ok()?;
    let content = String::from_utf8_lossy(&content);
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(200);
    let tail = lines[start..].join("\n");
    if tail.trim().is_empty() {
        return None;
    }
    Some(tail)
}

pub(super) fn feedback_proxy_tail_part() -> Option<multipart::Part> {
    let log_dir = proxy_log_dir()?;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = log_dir.join(format!("proxy-{today}.log"));
    let tail = feedback_proxy_tail_content(&path)?;
    let part =
        multipart::Part::bytes(tail.into_bytes()).file_name(format!("proxy-tail-{today}.log"));
    Some(
        part.mime_str("text/plain")
            .unwrap_or_else(|_| multipart::Part::text("")),
    )
}

fn redacted_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                let key_lower = k.to_ascii_lowercase();
                let is_sensitive_key = key_lower.contains("apikey")
                    || key_lower.contains("api_key")
                    || key_lower.contains("authorization")
                    || key_lower.contains("token")
                    || key_lower.contains("secret")
                    || key_lower.contains("password");
                if is_sensitive_key {
                    out.insert(k.clone(), Value::String("<REDACTED>".to_owned()));
                } else {
                    out.insert(k.clone(), redacted_json(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redacted_json).collect()),
        Value::String(s) => {
            let lower = s.to_ascii_lowercase();
            if lower.contains("bearer ")
                || lower.contains("sk-")
                || lower.contains("api_key")
                || lower.contains("apikey")
            {
                Value::String("<REDACTED>".to_owned())
            } else {
                Value::String(s.clone())
            }
        }
        other => other.clone(),
    }
}

fn sanitize_codex_toml(raw: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') || !trimmed.contains('=') {
            out.push(line.to_owned());
            continue;
        }
        let key = trimmed
            .split('=')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let normalized = key.replace(['-', '_'], "");
        let sensitive = key.contains("api_key")
            || key.contains("api-key")
            || key.contains("apikey")
            || normalized.contains("apikey")
            || key.contains("token")
            || key.contains("secret")
            || key.contains("password")
            || key.contains("authorization");
        if sensitive {
            let prefix_len = line.find('=').unwrap_or(line.len());
            let prefix = &line[..prefix_len + 1];
            out.push(format!("{prefix} \"<REDACTED>\""));
        } else {
            out.push(line.to_owned());
        }
    }
    out.join("\n")
}

fn diagnostic_attachments(include_diag: bool) -> Vec<FeedbackAttachment> {
    if !include_diag {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut log_idx = 10usize;

    if let Ok(cfg) = load_registry() {
        let cfg_redacted = redacted_json(&cfg);
        if let Ok(raw) = serde_json::to_vec_pretty(&cfg_redacted) {
            parts.push(FeedbackAttachment {
                field: format!("log{log_idx}"),
                name: "proxy-config.redacted.json".to_owned(),
                content_type: "application/json".to_owned(),
                raw,
            });
            log_idx += 1;
        }
    }

    if let Ok(paths) = CodexPaths::from_home_env() {
        if let Ok(raw_toml) = fs::read_to_string(&paths.config_toml) {
            let sanitized = sanitize_codex_toml(&raw_toml);
            if !sanitized.trim().is_empty() {
                parts.push(FeedbackAttachment {
                    field: format!("log{log_idx}"),
                    name: "codex-config.redacted.toml".to_owned(),
                    content_type: "text/plain".to_owned(),
                    raw: sanitized.into_bytes(),
                });
                log_idx += 1;
            }
        }
    }

    for bundle_path in recent_feedback_bundles(3) {
        let Ok(raw) = fs::read(&bundle_path) else {
            continue;
        };
        let name = bundle_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("feedback-bundle.json")
            .to_owned();
        parts.push(FeedbackAttachment {
            field: format!("log{log_idx}"),
            name,
            content_type: "application/json".to_owned(),
            raw,
        });
        log_idx += 1;
    }

    let versions = format!(
        "codex-app-transfer={}\nproxy_runtime={}\nclient_type=codex-desktop\nos={} {}\n",
        APP_VERSION,
        APP_VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    parts.push(FeedbackAttachment {
        field: format!("log{log_idx}"),
        name: "versions.txt".to_owned(),
        content_type: "text/plain".to_owned(),
        raw: versions.into_bytes(),
    });

    parts
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct FeedbackAttachment {
    pub(super) field: String,
    pub(super) name: String,
    pub(super) content_type: String,
    pub(super) raw: Vec<u8>,
}

pub(super) fn feedback_attachments(input: &Value, timestamp_secs: u64) -> Vec<FeedbackAttachment> {
    let mut shot_idx = 0usize;
    let mut log_idx = 0usize;
    let mut parts = Vec::new();

    if let Some(attachments) = input.get("attachments").and_then(|v| v.as_array()) {
        for attachment in attachments {
            let Some(obj) = attachment.as_object() else {
                continue;
            };
            let content_b64 = obj
                .get("content_b64")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let Ok(raw) = STANDARD.decode(content_b64.as_bytes()) else {
                continue;
            };
            if raw.is_empty() || raw.len() > 5 * 1024 * 1024 {
                continue;
            }
            let kind = obj.get("kind").and_then(|v| v.as_str()).unwrap_or("log");
            let fallback_name = format!("{kind}-{timestamp_secs}.bin");
            let name = obj
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .unwrap_or(&fallback_name)
                .to_owned();
            let content_type = obj
                .get("content_type")
                .and_then(|v| v.as_str())
                .filter(|v| v.contains('/'))
                .unwrap_or("application/octet-stream")
                .to_owned();
            let field = if kind == "screenshot" {
                let field = format!("screenshot{shot_idx}");
                shot_idx += 1;
                field
            } else {
                let field = format!("log{log_idx}");
                log_idx += 1;
                field
            };
            parts.push(FeedbackAttachment {
                field,
                name,
                content_type,
                raw,
            });
        }
    }

    parts
}

// ── /api/feedback ────────────────────────────────────────────────────

pub async fn submit_feedback(body: Bytes) -> Response {
    submit_feedback_with_body(body, FEEDBACK_WORKER_URL, feedback_throttle()).await
}

pub(super) async fn submit_feedback_with_body(
    body: Bytes,
    worker_url: &str,
    throttle: &FeedbackThrottle,
) -> Response {
    if let Err(reason) = throttle.acquire() {
        return err(StatusCode::TOO_MANY_REQUESTS, reason).into_response();
    }

    let input = match serde_json::from_slice::<Value>(&body) {
        Ok(input) => input,
        Err(_) => return err(StatusCode::BAD_REQUEST, "request body is not JSON").into_response(),
    };

    let title = input
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    let contact_email = input
        .get("contact_email")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    let body_text = input
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    let include_diag = input
        .get("include_diagnostics")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if body_text.is_empty() {
        return err(StatusCode::BAD_REQUEST, "description is required").into_response();
    }

    let worker_url = match feedback_worker_url(worker_url) {
        Ok(url) => url,
        Err(e) => {
            throttle.record_failure();
            return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
        }
    };

    let mut meta = json!({"app_version": APP_VERSION});
    if include_diag {
        let active_name = load_registry()
            .ok()
            .map(|cfg| active_provider_name(&cfg))
            .unwrap_or_default();
        if let Some(obj) = meta.as_object_mut() {
            obj.insert(
                "os".to_owned(),
                Value::String(std::env::consts::OS.to_owned()),
            );
            obj.insert(
                "arch".to_owned(),
                Value::String(std::env::consts::ARCH.to_owned()),
            );
            obj.insert(
                "active_provider_name".to_owned(),
                Value::String(active_name),
            );
            obj.insert("include_diagnostics".to_owned(), Value::Bool(true));
        }
    }

    let mut form = multipart::Form::new()
        .part(
            "meta",
            multipart_text_part(meta.to_string(), "application/json"),
        )
        .part("title", multipart_text_part(title, "text/plain"))
        .part(
            "contact_email",
            multipart_text_part(contact_email, "text/plain"),
        )
        .part("body", multipart_text_part(body_text, "text/plain"));

    for attachment in feedback_attachments(&input, current_epoch_secs()) {
        let FeedbackAttachment {
            field,
            name,
            content_type,
            raw,
        } = attachment;
        let part = multipart::Part::bytes(raw.clone()).file_name(name.clone());
        let part = part
            .mime_str(&content_type)
            .unwrap_or_else(|_| multipart::Part::bytes(raw).file_name(name));
        form = form.part(field, part);
    }

    for attachment in diagnostic_attachments(include_diag) {
        let FeedbackAttachment {
            field,
            name,
            content_type,
            raw,
        } = attachment;
        let part = multipart::Part::bytes(raw.clone()).file_name(name.clone());
        let part = part
            .mime_str(&content_type)
            .unwrap_or_else(|_| multipart::Part::bytes(raw).file_name(name));
        form = form.part(field, part);
    }

    if include_diag {
        if let Some(part) = feedback_proxy_tail_part() {
            form = form.part("log_proxy_tail", part);
        }
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            throttle.record_failure();
            return err(
                StatusCode::BAD_GATEWAY,
                format!("feedback service unavailable: {e}"),
            )
            .into_response();
        }
    };

    let response = match client.post(worker_url).multipart(form).send().await {
        Ok(response) => response,
        Err(e) => {
            throttle.record_failure();
            return err(
                StatusCode::BAD_GATEWAY,
                format!("feedback service unavailable: {e}"),
            )
            .into_response();
        }
    };
    let status = response.status();
    let is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.starts_with("application/json"))
        .unwrap_or(false);
    let data = if is_json {
        response.json::<Value>().await.unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    if !status.is_success() || data.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        throttle.record_failure();
        let status_code = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let status_code = if status_code.is_client_error() || status_code.is_server_error() {
            status_code
        } else {
            StatusCode::BAD_GATEWAY
        };
        let message = data
            .get("error")
            .or_else(|| data.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("upstream error");
        return err(status_code, message).into_response();
    }

    throttle.record_success();
    let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("");
    Json(json!({
        "success": true,
        "id": id,
        "message": format!("feedback received (ID: {id})"),
        "email_sent": data.get("email_sent").and_then(|v| v.as_bool()).unwrap_or(false),
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use super::super::common::random_hex;

    #[test]
    fn feedback_throttle_matches_legacy_success_and_failure_cooldowns() {
        let throttle = FeedbackThrottle::new();
        assert!(throttle.acquire().is_ok());
        throttle.record_success();
        assert!(throttle.acquire().unwrap_err().contains("just submitted"));

        let throttle = FeedbackThrottle::new();
        for _ in 0..FeedbackThrottle::FAILURE_LIMIT {
            throttle.record_failure();
        }
        assert!(throttle
            .acquire()
            .unwrap_err()
            .contains("too many consecutive failures"));
    }

    #[test]
    fn feedback_attachments_match_legacy_limits_and_fields() {
        let oversized = STANDARD.encode(vec![b'x'; 5 * 1024 * 1024 + 1]);
        let input = json!({
            "attachments": [
                {"kind": "screenshot", "name": "shot.png", "content_type": "image/png", "content_b64": STANDARD.encode(b"image-bytes")},
                {"kind": "log", "content_type": "not-a-mime", "content_b64": STANDARD.encode(b"log-bytes")},
                {"kind": "log", "name": "too-large.log", "content_b64": oversized},
                {"kind": "log", "name": "bad.log", "content_b64": "%%%"}
            ]
        });

        let attachments = feedback_attachments(&input, 1234);
        assert_eq!(attachments.len(), 2);
        assert_eq!(attachments[0].field, "screenshot0");
        assert_eq!(attachments[0].name, "shot.png");
        assert_eq!(attachments[0].content_type, "image/png");
        assert_eq!(attachments[0].raw, b"image-bytes");
        assert_eq!(attachments[1].field, "log0");
        assert_eq!(attachments[1].name, "log-1234.bin");
        assert_eq!(attachments[1].content_type, "application/octet-stream");
        assert_eq!(attachments[1].raw, b"log-bytes");
    }

    #[test]
    fn feedback_proxy_tail_reads_last_200_lines_lossily() {
        let root = std::env::temp_dir().join(format!(
            "cas-feedback-tail-{}-{}",
            std::process::id(),
            random_hex(6)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("proxy.log");

        let mut content = Vec::new();
        for i in 0..205 {
            content.extend_from_slice(format!("line-{i}\n").as_bytes());
        }
        content.extend_from_slice(b"bad-\xff\n");
        fs::write(&path, content).unwrap();

        let tail = feedback_proxy_tail_content(&path).unwrap();
        assert!(!tail.contains("line-0"));
        assert!(tail.contains("line-6"));
        assert!(tail.contains("line-204"));
        assert!(tail.contains("bad-\u{fffd}"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn feedback_submit_posts_json_payload_as_multipart_to_worker() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            use axum::{
                body::Bytes as AxumBytes,
                http::{header::CONTENT_TYPE as AXUM_CONTENT_TYPE, HeaderMap as AxumHeaderMap},
                routing::post,
                Router,
            };
            use tokio::net::TcpListener;

            let seen_body = Arc::new(Mutex::new(Vec::<u8>::new()));
            let seen_content_type = Arc::new(Mutex::new(String::new()));
            let app = Router::new().route(
                "/feedback",
                post({
                    let seen_body = Arc::clone(&seen_body);
                    let seen_content_type = Arc::clone(&seen_content_type);
                    move |headers: AxumHeaderMap, body: AxumBytes| {
                        let seen_body = Arc::clone(&seen_body);
                        let seen_content_type = Arc::clone(&seen_content_type);
                        async move {
                            *seen_content_type.lock().unwrap() = headers
                                .get(AXUM_CONTENT_TYPE)
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("")
                                .to_owned();
                            *seen_body.lock().unwrap() = body.to_vec();
                            Json(json!({"ok": true, "id": "fb-test", "email_sent": true}))
                        }
                    }
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            let payload = json!({
                "title": "short title",
                "contact_email": "user@example.com",
                "body": "feedback body",
                "include_diagnostics": false,
                "attachments": [
                    {"kind": "screenshot", "name": "shot.png", "content_type": "image/png", "content_b64": STANDARD.encode(b"png-bytes")},
                    {"kind": "log", "content_b64": STANDARD.encode(b"log-bytes")}
                ]
            });
            let throttle = FeedbackThrottle::new();
            let response = submit_feedback_with_body(
                Bytes::from(payload.to_string()),
                &format!("http://{addr}/feedback"),
                &throttle,
            )
            .await;
            server.abort();

            assert_eq!(response.status(), StatusCode::OK);
            let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let data: Value = serde_json::from_slice(&response_body).unwrap();
            assert_eq!(data["success"], json!(true));
            assert_eq!(data["id"], json!("fb-test"));
            assert_eq!(data["email_sent"], json!(true));

            assert!(seen_content_type
                .lock()
                .unwrap()
                .starts_with("multipart/form-data"));
            let seen = seen_body.lock().unwrap().clone();
            let multipart = String::from_utf8_lossy(&seen);
            assert!(multipart.contains("name=\"meta\""));
            assert!(multipart.contains("name=\"title\""));
            assert!(multipart.contains("short title"));
            assert!(multipart.contains("name=\"contact_email\""));
            assert!(multipart.contains("user@example.com"));
            assert!(multipart.contains("name=\"body\""));
            assert!(multipart.contains("feedback body"));
            assert!(multipart.contains("name=\"screenshot0\"; filename=\"shot.png\""));
            assert!(multipart.contains("name=\"log0\"; filename=\"log-"));
        });
    }

    #[test]
    fn feedback_submit_preserves_legacy_validation_and_upstream_errors() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            use axum::{routing::post, Router};
            use tokio::net::TcpListener;

            let throttle = FeedbackThrottle::new();
            let response =
                submit_feedback_with_body(Bytes::from("not-json"), "http://127.0.0.1", &throttle)
                    .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let data: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(data["message"], json!("request body is not JSON"));

            let throttle = FeedbackThrottle::new();
            let response = submit_feedback_with_body(
                Bytes::from(json!({"body": ""}).to_string()),
                "http://127.0.0.1",
                &throttle,
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let data: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(data["message"], json!("description is required"));

            let throttle = FeedbackThrottle::new();
            let response = submit_feedback_with_body(
                Bytes::from(json!({"body": "configured"}).to_string()),
                "",
                &throttle,
            )
            .await;
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let data: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(data["message"], json!("feedback service is not configured"));

            let app = Router::new().route(
                "/feedback",
                post(|| async { Json(json!({"ok": false, "error": "worker failed"})) }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            let throttle = FeedbackThrottle::new();
            let response = submit_feedback_with_body(
                Bytes::from(json!({"body": "goes upstream"}).to_string()),
                &format!("http://{addr}/feedback"),
                &throttle,
            )
            .await;
            server.abort();
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let data: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(data["message"], json!("worker failed"));
        });
    }
}
