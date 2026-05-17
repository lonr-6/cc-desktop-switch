//! Provider 模型列表抓取 + autofill.

use std::collections::HashSet;
use std::time::Duration;

use axum::{extract::Path, http::StatusCode, response::IntoResponse, Json};
use codex_app_transfer_registry::MODEL_ORDER;
use serde_json::{json, Value};

use super::super::super::registry_io::{load as load_registry, with_config_write, ConfigMutation};
use super::super::common::err;
use super::test::{build_provider_test_url, provider_test_error_label, provider_test_headers};
use super::{clean_base_url, normalize_provider_api_format, provider_index, replace_path_suffix};

/// 按 HTTP status + 上游 raw 错误中的关键词识别成结构化 reason code,
/// 前端拿 code 走 i18n 翻译(2026-05-10 用户决策:中英两版按当前 locale 切换,
/// backend 不 hardcode 任何语言文案)。
///
/// 已知 pattern 来源:
/// - Gemini: UNAUTHENTICATED / API_KEY_INVALID / RESOURCE_EXHAUSTED
/// - OpenAI: invalid_api_key / insufficient_quota / model_not_found
/// - 通用: timeout / rate_limit / not_found
fn classify_upstream_error_code(status: u16, raw: Option<&str>) -> &'static str {
    let lower = raw.unwrap_or("").to_ascii_lowercase();
    match status {
        400 => {
            if lower.contains("api key not valid") || lower.contains("api_key_invalid") {
                "api_key_invalid"
            } else if lower.contains("invalid_argument") || lower.contains("malformed") {
                "invalid_argument"
            } else {
                "bad_request"
            }
        }
        401 => {
            if lower.contains("oauth")
                || lower.contains("invalid authentication")
                || lower.contains("expected access token")
            {
                "unauthenticated_oauth"
            } else if lower.contains("expired") {
                "unauthenticated_expired"
            } else {
                "unauthenticated"
            }
        }
        403 => {
            if lower.contains("permission_denied") || lower.contains("permission denied") {
                "permission_denied"
            } else if lower.contains("billing") || lower.contains("payment") {
                "billing_required"
            } else {
                "forbidden"
            }
        }
        404 => "not_found",
        405 => "method_not_allowed",
        408 | 504 => "timeout",
        429 => {
            if lower.contains("quota") || lower.contains("resource_exhausted") {
                "quota_exceeded"
            } else {
                "rate_limited"
            }
        }
        500..=599 => "server_error",
        _ => "unknown",
    }
}

fn model_endpoint_candidates(provider: &Value) -> Vec<String> {
    let base_url = clean_base_url(
        provider
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    if base_url.is_empty() {
        return Vec::new();
    }

    let api_format =
        normalize_provider_api_format(provider.get("apiFormat").and_then(|v| v.as_str()));
    let upstream = build_provider_test_url(&base_url, api_format);
    let mut candidates = Vec::new();

    if api_format == "gemini_native" {
        // Gemini native:**只**用 build_provider_test_url 拼好的 endpoint
        // (`/v1beta/models` 或 `/v1alpha/models`,跟 base_url 是否带版本相关)。
        // **不要**push 别的 candidates(如 `{base_url}/models` 或 v1/models),
        // Google 上游对 unknown path 返 404 — 多 fallback 浪费请求 + 日志噪音。
        candidates.push(upstream.clone());
    } else if api_format == "anthropic_messages" {
        candidates.push(replace_path_suffix(
            &upstream,
            &["/v1/messages", "/messages"],
            "/v1/models",
        ));
    } else if api_format == "openai_chat" {
        candidates.push(replace_path_suffix(
            &upstream,
            &["/chat/completions", "/completions"],
            "/models",
        ));
        candidates.push(format!("{base_url}/models"));
    } else {
        candidates.push(replace_path_suffix(
            &upstream,
            &["/v1/responses", "/responses"],
            "/v1/models",
        ));
        if base_url.to_ascii_lowercase().ends_with("/v1") {
            candidates.push(format!("{base_url}/models"));
        }
        candidates.push(format!("{base_url}/models"));
        if let Ok(parsed) = reqwest::Url::parse(&base_url) {
            let stripped_path = parsed.path().trim_end_matches('/');
            let lower = stripped_path.to_ascii_lowercase();
            if lower.ends_with("/anthropic") || lower.ends_with("/v1") {
                let root_path = if lower.ends_with("/anthropic") {
                    &stripped_path[..stripped_path.len().saturating_sub("/anthropic".len())]
                } else {
                    &stripped_path[..stripped_path.len().saturating_sub("/v1".len())]
                };
                let mut root = parsed.clone();
                root.set_path(root_path.trim_end_matches('/'));
                root.set_query(None);
                root.set_fragment(None);
                let root_url = root.to_string().trim_end_matches('/').to_owned();
                candidates.push(format!("{root_url}/models"));
                candidates.push(format!("{root_url}/v1/models"));
            }
        }
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|item| !item.is_empty() && seen.insert(item.clone()))
        .collect()
}

fn model_id_from_item(item: &Value) -> Option<String> {
    if let Some(s) = item.as_str() {
        return Some(s.to_owned());
    }
    let obj = item.as_object()?;
    for key in ["id", "name", "model", "model_id"] {
        if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

fn extract_model_ids(payload: &Value) -> Vec<String> {
    let mut candidates: Vec<Value> = Vec::new();
    if let Some(items) = payload.as_array() {
        candidates = items.clone();
    } else if let Some(obj) = payload.as_object() {
        for key in ["data", "models", "items", "result"] {
            if let Some(items) = obj.get(key).and_then(|v| v.as_array()) {
                candidates = items.clone();
                break;
            }
        }
        if candidates.is_empty() {
            if let Some(data) = obj.get("data").and_then(|v| v.as_object()) {
                for key in ["models", "items"] {
                    if let Some(items) = data.get(key).and_then(|v| v.as_array()) {
                        candidates = items.clone();
                        break;
                    }
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for item in candidates {
        let Some(model_id) = model_id_from_item(&item) else {
            continue;
        };
        if seen.insert(model_id.clone()) {
            ids.push(model_id);
        }
    }
    ids
}

fn usable_model_ids(model_ids: &[String]) -> Vec<String> {
    const EXCLUDE: &[&str] = &[
        "embedding",
        "rerank",
        "moderation",
        "whisper",
        "tts",
        "image",
        "vision",
        "audio",
    ];
    let usable: Vec<String> = model_ids
        .iter()
        .filter(|model_id| {
            let lower = model_id.to_ascii_lowercase();
            !EXCLUDE.iter().any(|keyword| lower.contains(keyword))
        })
        .cloned()
        .collect();
    if usable.is_empty() {
        model_ids.to_vec()
    } else {
        usable
    }
}

fn pick_model(model_ids: &[String], keywords: &[&str], fallback_index: usize) -> String {
    for keyword in keywords {
        for model_id in model_ids {
            if model_id.to_ascii_lowercase().contains(keyword) {
                return model_id.clone();
            }
        }
    }
    if model_ids.is_empty() {
        String::new()
    } else {
        model_ids[std::cmp::min(fallback_index, model_ids.len() - 1)].clone()
    }
}

fn empty_model_mappings_value() -> Value {
    let mut out = serde_json::Map::new();
    for slot in MODEL_ORDER.iter().copied() {
        out.insert(slot.to_owned(), Value::String(String::new()));
    }
    Value::Object(out)
}

fn suggest_model_mappings(model_ids: &[String]) -> Value {
    let usable = usable_model_ids(model_ids);
    let mut result = empty_model_mappings_value();
    if usable.is_empty() {
        return result;
    }
    let chosen = pick_model(
        &usable,
        &["pro", "plus", "coder", "max", "reasoner", "v4"],
        0,
    );
    if let Some(obj) = result.as_object_mut() {
        obj.insert("default".to_owned(), Value::String(chosen));
    }
    result
}

/// Antigravity 专属 model fetch — 走上游 `:fetchAvailableModels`,失败退到
/// 静态种子。共用 antigravity_oauth handler 的 shared http client + token store
/// (`~/.codex-app-transfer/antigravity-oauth.json`)。
///
/// 不传 provider 参数 — antigravity 上游 list 跟具体 provider config 无关
/// (只看 OAuth token + 可选 project_id),所以多个 user-defined antigravity_oauth
/// provider 都能 share 这条 fetch
async fn fetch_antigravity_models_impl() -> Value {
    use codex_app_transfer_gemini_oauth::{
        antigravity_static_models, ensure_valid_antigravity_token,
        fetch_antigravity_available_models, TokenStore, ANTIGRAVITY_PROVIDER,
    };

    // 静态 seed 总能拿到 — 即使 OAuth 没登录,UI 上至少能给 model 映射的选项
    let seed_ids: Vec<String> = antigravity_static_models()
        .into_iter()
        .map(|m| m.id)
        .collect();
    let seed_response = || {
        json!({
            "success": true,
            "endpoint": "(static seed: antigravity 10 models)",
            "models": seed_ids.clone(),
            "suggested": suggest_model_mappings(&seed_ids),
        })
    };

    let store = match TokenStore::for_token_filename(ANTIGRAVITY_PROVIDER.token_filename) {
        Ok(s) => s,
        Err(_) => return seed_response(),
    };

    // 共用 antigravity-oauth handler 的 shared http client(避免每个 fetch 起新
    // pool)。这里用临时 client builder 简化(get_or_init 失败也不阻塞 user)
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(_) => return seed_response(),
    };

    let access_token = match ensure_valid_antigravity_token(&client, &store).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                error_id = "ANTIGRAVITY_FORM_FETCH_TOKEN",
                error = %e,
                "antigravity model fetch: token 不可用,退到静态种子"
            );
            return seed_response();
        }
    };
    let project_id = store.load().ok().flatten().and_then(|t| t.project_id);

    match fetch_antigravity_available_models(&client, &access_token, project_id.as_deref()).await {
        Ok(models) if !models.is_empty() => {
            let ids: Vec<String> = models.into_iter().map(|m| m.id).collect();
            json!({
                "success": true,
                "endpoint": "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
                "models": ids.clone(),
                "suggested": suggest_model_mappings(&ids),
            })
        }
        _ => {
            tracing::warn!(
                error_id = "ANTIGRAVITY_FORM_FETCH_FAIL",
                "antigravity :fetchAvailableModels 失败/空,退到静态种子"
            );
            seed_response()
        }
    }
}

async fn fetch_provider_models_impl(provider: &Value) -> Value {
    // Cloud Code Assist (gemini_cli_oauth) 上游没有 listModels endpoint —
    // gemini-cli upstream 自己用 hardcoded enum (`packages/core/src/config/models.ts`)。
    // 这里返跟 gemini-cli upstream + CLIProxyAPI `internal/registry/models/models.json`
    // (provider="gemini-cli")交集对齐的固定列表(2026-05-10 同步两个 upstream)。
    //
    // **Gemini 3 系列在 OAuth 路径已可用**(实证 CLIProxyAPI 2026-05-10
    // models.json L817/845/874/904 + gemini-cli `PREVIEW_GEMINI_*` 常量
    // 路由进 Code Assist):
    //   - gemini-3-pro-preview / gemini-3.1-pro-preview
    //   - gemini-3-flash-preview / gemini-3.1-flash-lite-preview
    // 免费 tier 对 3.x 配额更紧(易触 RESOURCE_EXHAUSTED),但 OAuth 路径
    // 自动绑 cloudaicompanionProject,quota 走 GCP project 计数。
    //
    // 不含的:
    //   - `gemini-3-pro-image-preview` / `gemini-3.1-flash-image-preview`
    //     (CLIProxyAPI 仅在 gemini/aistudio API-key 路径列,gemini-cli 路径无)
    //   - `*-latest` 别名(也仅 API-key 路径)
    if provider.get("apiFormat").and_then(|v| v.as_str()) == Some("gemini_cli_oauth") {
        let model_ids = vec![
            // 稳定 2.5 系列(default 推 flash:免费 tier 配额最宽)
            "gemini-2.5-flash".to_owned(),
            "gemini-2.5-pro".to_owned(),
            "gemini-2.5-flash-lite".to_owned(),
            // Gemini 3 preview 系列(2026-05 起在 OAuth 路径可用)
            "gemini-3-pro-preview".to_owned(),
            "gemini-3.1-pro-preview".to_owned(),
            "gemini-3-flash-preview".to_owned(),
            "gemini-3.1-flash-lite-preview".to_owned(),
        ];
        return json!({
            "success": true,
            "endpoint": "(static: cloud-code-assist hardcoded list)",
            "models": model_ids.clone(),
            "suggested": suggest_model_mappings(&model_ids),
        });
    }

    // **antigravity_oauth — 真有 list endpoint**(CLIProxyAPI
    // `cmd/fetch_antigravity_models/main.go`):POST 上游
    // `:fetchAvailableModels` 拿 antigravity 专属 model list(claude-opus-4-6-thinking
    // / gemini-3-pro-low/high / gpt-oss-120b 等 10 个);失败退到 crate
    // 内嵌的静态种子(`antigravity_static_models()`,与上游 model.json 1:1)。
    // 跟 gemini_cli_oauth 不同(那个上游真没 list endpoint)
    if provider.get("apiFormat").and_then(|v| v.as_str()) == Some("antigravity_oauth") {
        return fetch_antigravity_models_impl().await;
    }

    let endpoints = model_endpoint_candidates(provider);
    if endpoints.is_empty() {
        return json!({
            "success": false,
            "message": "models.fetchFailed",
            "models": [],
            "suggested": {},
            "errors": [{"code": "invalid_base_url"}],
        });
    }

    let headers = provider_test_headers(provider, false);
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .connect_timeout(Duration::from_secs(6))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            // client builder 失败极罕见(系统资源耗尽等);返结构化 code 给前端 i18n
            let _ = error;
            return json!({
                "success": false,
                "message": "models.fetchFailed",
                "models": [],
                "suggested": {},
                "errors": [{"code": "client_init_failure"}],
            });
        }
    };

    let mut errors: Vec<Value> = Vec::new();
    for endpoint in endpoints {
        let host = reqwest::Url::parse(&endpoint)
            .ok()
            .and_then(|u| u.host_str().map(String::from))
            .unwrap_or_else(|| endpoint.clone());
        let response = match client.get(&endpoint).headers(headers.clone()).send().await {
            Ok(response) => response,
            Err(error) => {
                // 不透传英文 reqwest error label,返结构化 code 给前端 i18n 翻
                let code = match provider_test_error_label(&error) {
                    "Timeout" => "network_timeout",
                    "ConnectError" => "network_connect",
                    "RedirectError" => "network_redirect",
                    "DecodeError" => "network_decode",
                    "BodyError" => "network_body",
                    "RequestError" => "network_request",
                    _ => "network_other",
                };
                errors.push(json!({"host": host, "code": code}));
                continue;
            }
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            // 读 body 提取 error.message 仅用于 reason 关键词识别(OpenAI/Gemini/
            // Anthropic 统一 `{"error":{"message":"..."}}` shape),**不透传 raw**
            // 给 UI;返结构化 code,前端按当前 locale 翻译。
            let body = response.text().await.unwrap_or_default();
            let raw = serde_json::from_str::<Value>(&body).ok().and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(String::from)
            });
            let code = classify_upstream_error_code(status, raw.as_deref());
            errors.push(json!({"host": host, "code": code, "statusCode": status}));
            continue;
        }
        let payload = match response.json::<Value>().await {
            Ok(payload) => payload,
            Err(_) => {
                errors.push(json!({"host": host, "code": "non_json_response"}));
                continue;
            }
        };
        let model_ids = extract_model_ids(&payload);
        if !model_ids.is_empty() {
            return json!({
                "success": true,
                "endpoint": endpoint,
                "models": model_ids,
                "suggested": suggest_model_mappings(&model_ids),
            });
        }
        errors.push(json!({"host": host, "code": "models_not_found"}));
    }

    let start = errors.len().saturating_sub(5);
    // message 现在是 i18n key,前端拿 key 翻译(防 hardcode 中英任一语言)
    json!({
        "success": false,
        "message": "models.fetchFailed",
        "models": [],
        "suggested": {},
        "errors": errors[start..].to_vec(),
    })
}

pub async fn fetch_provider_models(Path(id): Path<String>) -> impl IntoResponse {
    let cfg = match load_registry() {
        Ok(c) => c,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    let provider = cfg
        .get("providers")
        .and_then(|v| v.as_array())
        .and_then(|providers| {
            providers.iter().find(|provider| {
                provider
                    .as_object()
                    .and_then(|o| o.get("id"))
                    .and_then(|v| v.as_str())
                    == Some(id.as_str())
            })
        });
    let Some(provider) = provider else {
        return err(StatusCode::NOT_FOUND, "provider not found").into_response();
    };
    let result = fetch_provider_models_impl(provider).await;
    let status = if result.get("success").and_then(|v| v.as_bool()) == Some(true) {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(result)).into_response()
}

pub async fn fetch_provider_models_payload(Json(payload): Json<Value>) -> impl IntoResponse {
    let result = fetch_provider_models_impl(&payload).await;
    let status = if result.get("success").and_then(|v| v.as_bool()) == Some(true) {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(result)).into_response()
}

pub async fn autofill_provider_models(Path(id): Path<String>) -> impl IntoResponse {
    // **不能在 with_config_write 闭包内 await**(closure 是 sync)。先 load 一份
    // provider snapshot 给 fetch 用,await long async 在锁外,然后真 mutate +
    // save 走 atomic RMW。
    let provider_snapshot = match load_registry() {
        Ok(cfg) => {
            let Some(idx) = provider_index(&cfg, &id) else {
                return err(StatusCode::NOT_FOUND, "provider not found").into_response();
            };
            cfg.get("providers")
                .and_then(|v| v.as_array())
                .and_then(|providers| providers.get(idx))
                .cloned()
                .unwrap_or_else(|| json!({}))
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    // 长 async — 在锁外执行,不阻塞其他 RMW
    let result = fetch_provider_models_impl(&provider_snapshot).await;
    if result.get("success").and_then(|v| v.as_bool()) != Some(true) {
        return (StatusCode::BAD_REQUEST, Json(result)).into_response();
    }
    let suggested = result
        .get("suggested")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // 真 mutate + save 走 atomic RMW
    let suggested_for_closure = suggested.clone();
    let write_result = with_config_write(|cfg| {
        let Some(idx) = provider_index(cfg, &id) else {
            // race:autofill 期间 provider 被并发 delete 了
            return Err("provider disappeared during autofill".into());
        };
        if let Some(providers) = cfg.get_mut("providers").and_then(|v| v.as_array_mut()) {
            if let Some(provider) = providers.get_mut(idx).and_then(|v| v.as_object_mut()) {
                provider.insert("models".into(), suggested_for_closure.clone());
                return Ok(ConfigMutation::Modified(()));
            }
        }
        Ok(ConfigMutation::Unchanged(()))
    });
    if let Err(e) = write_result {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
    }
    Json(json!({
        "success": true,
        "models": result.get("models").cloned().unwrap_or_else(|| json!([])),
        "suggested": suggested,
        "endpoint": result.get("endpoint").cloned().unwrap_or(Value::Null),
        "message": "model mappings auto-filled",
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_endpoint_candidates_anthropic_messages_use_models_endpoint() {
        assert_eq!(
            model_endpoint_candidates(&json!({
                "baseUrl": "https://api.anthropic.com/v1",
                "apiFormat": "anthropic_messages",
            })),
            vec!["https://api.anthropic.com/v1/models".to_owned()]
        );
        assert_eq!(
            model_endpoint_candidates(&json!({
                "baseUrl": "https://proxy.example/anthropic",
                "apiFormat": "claude",
            })),
            vec!["https://proxy.example/anthropic/v1/models".to_owned()]
        );
        assert_eq!(
            model_endpoint_candidates(&json!({
                "baseUrl": "https://proxy.example/anthropic/v1/messages",
                "apiFormat": "messages",
            })),
            vec!["https://proxy.example/anthropic/v1/models".to_owned()]
        );
    }

    #[test]
    fn fetch_provider_models_reads_anthropic_messages_models_with_version_header() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            use axum::{
                http::{HeaderMap as AxumHeaderMap, StatusCode as AxumStatusCode},
                routing::get,
                Router,
            };
            use tokio::net::TcpListener;

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let app = Router::new().route(
                "/v1/models",
                get(|headers: AxumHeaderMap| async move {
                    if headers
                        .get("anthropic-version")
                        .and_then(|v| v.to_str().ok())
                        == Some("2023-06-01")
                    {
                        (
                            AxumStatusCode::OK,
                            Json(json!({
                                "data": [
                                    {"id": "claude-sonnet-4-6"},
                                    {"id": "claude-opus-4-6"}
                                ]
                            })),
                        )
                    } else {
                        (
                            AxumStatusCode::BAD_REQUEST,
                            Json(json!({"error": "missing anthropic-version"})),
                        )
                    }
                }),
            );
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            let provider = json!({
                "baseUrl": format!("http://{addr}/v1"),
                "apiFormat": "anthropic_messages",
                "authScheme": "none"
            });
            let result = fetch_provider_models_impl(&provider).await;
            server.abort();

            assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
            assert_eq!(
                result.get("endpoint").and_then(|v| v.as_str()),
                Some(format!("http://{addr}/v1/models").as_str())
            );
            assert_eq!(
                result.get("models").and_then(|v| v.as_array()).cloned(),
                Some(vec![json!("claude-sonnet-4-6"), json!("claude-opus-4-6")])
            );
        });
    }

    #[test]
    fn fetch_provider_models_reads_openai_compatible_models() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            use axum::{routing::get, Router};
            use tokio::net::TcpListener;

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let app = Router::new().route(
                "/v1/models",
                get(|| async {
                    Json(json!({
                        "data": [
                            {"id": "text-embedding-3-small"},
                            {"id": "deepseek-v4-pro"},
                            {"id": "deepseek-chat"}
                        ]
                    }))
                }),
            );
            let server = tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            });

            let provider = json!({
                "baseUrl": format!("http://{addr}/v1"),
                "apiFormat": "responses",
                "authScheme": "none"
            });
            let result = fetch_provider_models_impl(&provider).await;
            server.abort();

            assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
            assert_eq!(
                result.get("endpoint").and_then(|v| v.as_str()),
                Some(format!("http://{addr}/v1/models").as_str())
            );
            assert_eq!(
                result.get("models").and_then(|v| v.as_array()).cloned(),
                Some(vec![
                    json!("text-embedding-3-small"),
                    json!("deepseek-v4-pro"),
                    json!("deepseek-chat"),
                ])
            );
            assert_eq!(result["suggested"]["default"], json!("deepseek-v4-pro"));
        });
    }
}
