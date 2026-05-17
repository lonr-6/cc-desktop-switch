//! 端到端验证 B1(多 provider 路由)+ B2(鉴权改写)。
//!
//! 拓扑:
//!     reqwest client ──► [Rust 代理 + StaticResolver]
//!                         ├─► upstream-a(echo mock,所有请求回 JSON 反射)
//!                         └─► upstream-b(echo mock,所有请求回 JSON 反射)
//!
//! upstream mock 把收到的 method / path / headers / body 原样反射为 JSON,
//! 测试拿到代理的响应后即可判定:
//! - 是否打到了正确的上游(用 mock 自身的 marker 头区分)
//! - Authorization / X-Api-Key / extra-headers 是否被代理重写正确
//! - body 中的 model 字段是否被剥掉 `<slug>/` 前缀

use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{body::Body, extract::Request, response::Response, routing::any, Router};
use codex_app_transfer_proxy::{build_router, proxy_telemetry, StaticResolver};
use codex_app_transfer_registry::Provider;
use futures_util::{SinkExt, StreamExt};
use indexmap::IndexMap;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, http::HeaderValue, Message as WsMessage},
};

/// echo-back 上游:把收到的请求镜像成 JSON 返回。`marker` 用来在响应头里
/// 标记是哪个 mock,以便测试断言代理选对了上游。
///
/// `headers` 字段:每个 header name 只保留**最后一个**值(serde Map insert
/// 语义),用于已有断言。
/// `headers_all` 字段:每个 header name → 所有值的列表,用于检测同名
/// header 是否被重复发送(extras override 单测要看这里)。
fn echo_mock(marker: &'static str) -> Router {
    Router::new().fallback(any(move |req: Request| async move {
        let (parts, body) = req.into_parts();
        let bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .unwrap_or_default();
        let mut headers_map = serde_json::Map::new();
        let mut headers_all: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        for (k, v) in parts.headers.iter() {
            let name = k.as_str().to_owned();
            let value = v.to_str().unwrap_or("").to_owned();
            headers_map.insert(name.clone(), serde_json::Value::String(value.clone()));
            headers_all
                .entry(name)
                .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                .as_array_mut()
                .unwrap()
                .push(serde_json::Value::String(value));
        }
        let payload = json!({
            "marker": marker,
            "method": parts.method.as_str(),
            "path": parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/"),
            "headers": headers_map,
            "headers_all": headers_all,
            "body": String::from_utf8_lossy(&bytes),
        });
        Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .header("x-mock-marker", marker)
            .body(Body::from(payload.to_string()))
            .unwrap()
    }))
}

fn chat_sse_capture_mock(calls: Arc<Mutex<Vec<serde_json::Value>>>) -> Router {
    Router::new().fallback(any(move |req: Request| {
        let calls = calls.clone();
        async move {
            let (parts, body) = req.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX)
                .await
                .unwrap_or_default();
            calls.lock().unwrap().push(json!({
                "method": parts.method.as_str(),
                "path": parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/"),
                "body": String::from_utf8_lossy(&bytes),
            }));
            let payload = concat!(
                "data: {\"id\":\"chatcmpl_test\",\"model\":\"mock-chat\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl_test\",\"model\":\"mock-chat\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            );
            Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .body(Body::from(payload))
                .unwrap()
        }
    }))
}

fn anthropic_sse_capture_mock(calls: Arc<Mutex<Vec<serde_json::Value>>>) -> Router {
    Router::new().fallback(any(move |req: Request| {
        let calls = calls.clone();
        async move {
            let (parts, body) = req.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX)
                .await
                .unwrap_or_default();
            let mut headers_map = serde_json::Map::new();
            let mut headers_all: serde_json::Map<String, serde_json::Value> =
                serde_json::Map::new();
            for (k, v) in parts.headers.iter() {
                let name = k.as_str().to_owned();
                let value = v.to_str().unwrap_or("").to_owned();
                headers_map.insert(name.clone(), serde_json::Value::String(value.clone()));
                headers_all
                    .entry(name)
                    .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::String(value));
            }
            calls.lock().unwrap().push(json!({
                "method": parts.method.as_str(),
                "path": parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/"),
                "headers": headers_map,
                "headers_all": headers_all,
                "body": String::from_utf8_lossy(&bytes),
            }));
            let payload = concat!(
                "event: message_start\n",
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_test\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-test\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\n",
                "event: content_block_start\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
                "event: content_block_delta\n",
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\n",
                "event: content_block_stop\n",
                "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "event: message_delta\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":1}}\n\n",
                "event: message_stop\n",
                "data: {\"type\":\"message_stop\"}\n\n",
            );
            Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .body(Body::from(payload))
                .unwrap()
        }
    }))
}

async fn spawn(router: Router) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    addr
}

fn provider(
    id: &str,
    base: &str,
    api_key: &str,
    auth_scheme: &str,
    extras: &[(&str, &str)],
) -> Provider {
    let mut h = IndexMap::new();
    for (k, v) in extras {
        h.insert((*k).to_owned(), (*v).to_owned());
    }
    Provider {
        id: id.into(),
        name: id.into(),
        base_url: base.into(),
        auth_scheme: auth_scheme.into(),
        api_format: "openai_chat".into(),
        api_key: api_key.into(),
        models: IndexMap::new(),
        extra_headers: h,
        model_capabilities: IndexMap::new(),
        request_options: IndexMap::new(),
        is_builtin: false,
        sort_index: 0,
        extra: IndexMap::new(),
    }
}

struct Stack {
    proxy: std::net::SocketAddr,
    upstream_a: std::net::SocketAddr,
    upstream_b: std::net::SocketAddr,
}

async fn build_stack() -> Stack {
    let upstream_a = spawn(echo_mock("upstream-a")).await;
    let upstream_b = spawn(echo_mock("upstream-b")).await;
    let resolver = Arc::new(StaticResolver::new(
        Some("cas_test_gw".into()),
        vec![
            provider(
                "provider-a",
                &format!("http://{upstream_a}"),
                "sk-a-bearer",
                "bearer",
                &[],
            ),
            provider(
                "provider-b",
                &format!("http://{upstream_b}"),
                "sk-b-key",
                "x-api-key",
                &[("User-Agent", "TestAgent/1.0")],
            ),
        ],
        Some("provider-a".into()),
    ));
    let proxy = spawn(build_router(resolver)).await;
    Stack {
        proxy,
        upstream_a,
        upstream_b,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn body_json(resp: reqwest::Response) -> serde_json::Value {
    let bytes = resp.bytes().await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: serde_json::Value) {
    let payload = json!({
        "sessionId": "bf3f9f",
        "runId": "pre-fix",
        "hypothesisId": hypothesis_id,
        "location": location,
        "message": message,
        "data": data,
        "timestamp": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/Users/alysechen/alysechen/github/codex-app-transfer/.cursor/debug-bf3f9f.log")
    {
        let _ = writeln!(f, "{}", payload);
    }
}

#[tokio::test]
async fn successful_forward_updates_proxy_telemetry() {
    let before = proxy_telemetry().stats.snapshot();
    let s = build_stack().await;

    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .header("content-type", "application/json")
        .body(r#"{"model":"provider-a/gpt-x"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 200);

    let after = proxy_telemetry().stats.snapshot();
    assert!(after.total >= before.total + 1);
    assert!(after.success >= before.success + 1);

    let logs = proxy_telemetry().logs.get_all();
    assert!(logs
        .iter()
        .any(|entry| entry.level == "INFO" && entry.message.contains("request: POST")));
    assert!(logs
        .iter()
        .any(|entry| entry.level == "SUCCESS" && entry.message == "upstream status 200"));
}

#[tokio::test]
async fn anthropic_messages_forward_injects_adapter_protocol_headers() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let upstream = spawn(anthropic_sse_capture_mock(calls.clone())).await;
    let mut claude = provider(
        "claude-provider",
        &format!("http://{upstream}/v1"),
        "sk-claude",
        "bearer",
        &[],
    );
    claude.api_format = "anthropic".into();
    let resolver = Arc::new(StaticResolver::new(
        Some("cas_test_gw".into()),
        vec![claude],
        Some("claude-provider".into()),
    ));
    let proxy = spawn(build_router(resolver)).await;

    let resp = client()
        .post(format!("http://{proxy}/v1/responses"))
        .header("authorization", "Bearer cas_test_gw")
        .header("anthropic-version", "stale-client-value")
        .header("content-type", "text/plain")
        .body(
            json!({
                "model": "claude-provider/claude-3-5-sonnet-latest",
                "stream": true,
                "input": [{"type":"message","role":"user","content":"hi"}]
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let _ = resp.bytes().await.unwrap();

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call["path"], "/v1/messages");
    assert_eq!(call["headers"]["anthropic-version"], "2023-06-01");
    assert_eq!(call["headers"]["content-type"], "application/json");
    assert_eq!(
        call["headers_all"]["anthropic-version"]
            .as_array()
            .expect("anthropic-version header list")
            .len(),
        1
    );
    let body: serde_json::Value =
        serde_json::from_str(call["body"].as_str().expect("captured body")).unwrap();
    assert_eq!(body["model"], "claude-3-5-sonnet-latest");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["role"], "user");
}

#[tokio::test]
async fn gateway_auth_failure_updates_proxy_telemetry() {
    let before = proxy_telemetry().stats.snapshot();
    let s = build_stack().await;

    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .body(r#"{"model":"any"}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status().as_u16(), 401);

    let after = proxy_telemetry().stats.snapshot();
    assert!(after.total >= before.total + 1);
    assert!(after.failed >= before.failed + 1);

    let logs = proxy_telemetry().logs.get_all();
    assert!(logs
        .iter()
        .any(|entry| entry.level == "ERROR" && entry.message.contains("proxy request failed")));
}

#[tokio::test]
async fn unauthorized_without_gateway_key() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .body(r#"{"model":"any"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn unauthorized_with_wrong_gateway_key() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer wrong")
        .body(r#"{"model":"any"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn slug_routes_to_provider_a_with_bearer_and_strips_slug() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .header("content-type", "application/json")
        .body(r#"{"model":"provider-a/gpt-x"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get("x-mock-marker").unwrap(),
        "upstream-a",
        "应该路由到 upstream-a"
    );
    let v = body_json(resp).await;
    let auth = v["headers"]["authorization"].as_str().unwrap();
    assert_eq!(
        auth, "Bearer sk-a-bearer",
        "B2: authorization 必须重写为 provider-a 的 key"
    );
    let body = v["body"].as_str().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(parsed["model"], "gpt-x", "应该剥掉 `provider-a/` 前缀");
    // 上游不应收到 user-agent(provider-a 没配 extras)
    assert!(
        v["headers"]
            .get("user-agent")
            .map(|x| x.as_str().unwrap_or(""))
            .unwrap_or("")
            != "TestAgent/1.0",
        "provider-a 没配 extras,不应注入 TestAgent"
    );
    assert_eq!(s.upstream_a.port() > 0 && s.upstream_b.port() > 0, true);
}

#[tokio::test]
async fn slug_routes_to_provider_b_with_x_api_key_and_extras() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .header("content-type", "application/json")
        .body(r#"{"model":"provider-b/coding"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get("x-mock-marker").unwrap(),
        "upstream-b",
        "应该路由到 upstream-b"
    );
    let v = body_json(resp).await;
    // B2: X-Api-Key 路径
    assert_eq!(
        v["headers"]["x-api-key"].as_str().unwrap(),
        "sk-b-key",
        "B2: X-Api-Key 必须等于 provider-b 的 key"
    );
    // B2: extra header 注入
    assert_eq!(
        v["headers"]["user-agent"].as_str().unwrap(),
        "TestAgent/1.0",
        "B2: provider-b.extraHeaders 必须注入 User-Agent"
    );
    // 入站 gateway Authorization 不应泄漏到上游
    assert!(
        v["headers"]
            .get("authorization")
            .map(|x| x.as_str().unwrap_or(""))
            .unwrap_or("")
            != "Bearer cas_test_gw",
        "incoming gateway Authorization 不应原样转发"
    );
    let body = v["body"].as_str().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(parsed["model"], "coding");
}

/// 同名 header 必须**只**带 `provider.extraHeaders` 的值上线,**不能**和
/// 客户端原始 header 一起以多值形式打到上游。reqwest `RequestBuilder::header`
/// 是 append 语义,如果不在复制客户端 header 时过滤掉 extras 已覆盖的名字,
/// kimi-code 这类靠 `User-Agent: KimiCLI/1.40.0` 伪装身份的 provider 会被
/// 上游"首条 UA"一票否决(2026-05-07 Windows v2.0.8 Kimi 403 现场)。
#[tokio::test]
async fn extras_header_overrides_client_value_no_duplicate() {
    let s = build_stack().await;
    // 客户端显式带 User-Agent 模拟 Codex CLI 自加身份头
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .header("user-agent", "client-codex-cli/0.x.x")
        .header("content-type", "application/json")
        .body(r#"{"model":"provider-b/coding"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let v = body_json(resp).await;

    let ua_values = v["headers_all"]["user-agent"]
        .as_array()
        .expect("upstream 应当至少收到一条 user-agent");
    assert_eq!(
        ua_values.len(),
        1,
        "extras 必须 override:上游应只见到 1 条 User-Agent,实际: {:?}",
        ua_values
    );
    assert_eq!(
        ua_values[0].as_str().unwrap(),
        "TestAgent/1.0",
        "唯一一条 User-Agent 必须是 extras 的值,而不是客户端 codex CLI 的值"
    );
}

/// 关键回归(2026-05-08 Kimi Windows 403):extras 没 User-Agent 时,客户端的
/// codex_cli_rs/... UA 必须被 strip,上游收到的是中性 default UA
/// (Codex-App-Transfer/<v>),绝不能透传客户端 codex UA。
///
/// provider-a 在 build_stack 里 extras 不含 User-Agent(只有 `x-mock-marker`),
/// 所以走 default UA 路径。
#[tokio::test]
async fn client_user_agent_stripped_when_provider_extras_lacks_ua() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .header("user-agent", "codex_cli_rs/0.128.0 (Windows 10.0; x86_64)")
        .header("content-type", "application/json")
        .body(r#"{"model":"plain-model-name"}"#) // 走 fallback default = provider-a
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let v = body_json(resp).await;
    let ua_values = v["headers_all"]["user-agent"]
        .as_array()
        .expect("upstream 应有 user-agent");
    assert_eq!(ua_values.len(), 1, "应只有 1 条 UA,实际: {:?}", ua_values);
    let ua = ua_values[0].as_str().unwrap();
    assert!(
        !ua.contains("codex_cli_rs"),
        "客户端 codex UA 必须被 strip,实际泄漏: {ua}"
    );
    assert!(
        ua.starts_with("Codex-App-Transfer/"),
        "无 extras UA 时上游应收到中性 default UA,实际: {ua}"
    );
}

/// 关键回归(2026-05-08):Codex CLI 内置注入 originator / x-codex-installation-id /
/// x-codex-window-id / x-openai-* / chatgpt-account-id 等身份头(`codex-rs/login/
/// src/auth/default_client.rs::default_headers` + `codex-rs/core/src/client.rs:481-605`)。
/// 这些头对第三方 OpenAI-compatible provider 永远没用,但 Kimi For Coding 等 provider
/// 的反爬规则会按这些头判定"非白名单 client"返回 403 access_terminated_error。
/// 出站时**必须**整片剔除,不能透传到上游。
#[tokio::test]
async fn codex_identity_headers_are_stripped_on_forward() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        // Codex CLI 自家身份头(精确名 + 前缀两类全覆盖)
        .header("originator", "codex_cli_rs")
        .header("x-codex-installation-id", "test-installation-uuid")
        .header("x-codex-window-id", "test-window-uuid")
        .header("x-openai-subagent", "memgen")
        .header("x-openai-memgen-request", "1")
        .header("chatgpt-account-id", "test-acct")
        .header("session_id", "test-session")
        .header("thread_id", "test-thread")
        // 普通 header 应正常透传
        .header("content-type", "application/json")
        .body(r#"{"model":"provider-b/coding"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let v = body_json(resp).await;
    let headers_all = v["headers_all"].as_object().expect("headers_all");

    // 精确名黑名单
    for forbidden in [
        "originator",
        "chatgpt-account-id",
        "session_id",
        "thread_id",
    ] {
        assert!(
            !headers_all.contains_key(forbidden),
            "上游绝不应收到 codex 身份头 {forbidden},实际 headers: {:?}",
            headers_all.keys().collect::<Vec<_>>()
        );
    }

    // 前缀黑名单(防御未来 Codex CLI 加新头)
    for (k, _) in headers_all.iter() {
        let lower = k.to_ascii_lowercase();
        assert!(
            !lower.starts_with("x-codex-"),
            "上游不应收到任何 x-codex-* 头,但有: {k}"
        );
        assert!(
            !lower.starts_with("x-openai-"),
            "上游不应收到任何 x-openai-* 头,但有: {k}"
        );
        assert!(
            !lower.starts_with("x-chatgpt-"),
            "上游不应收到任何 x-chatgpt-* 头,但有: {k}"
        );
    }

    // 普通 header 仍然透传
    assert!(
        headers_all.contains_key("content-type"),
        "正常 content-type 头仍应透传"
    );
}

#[tokio::test]
async fn fallback_to_default_when_no_slug() {
    let s = build_stack().await;
    let resp = client()
        .post(format!("http://{}/v1/chat/completions", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .header("content-type", "application/json")
        .body(r#"{"model":"plain-model-name"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get("x-mock-marker").unwrap(),
        "upstream-a",
        "无 slug 时应 fallback 到 default(provider-a)"
    );
    let v = body_json(resp).await;
    let body = v["body"].as_str().unwrap();
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        parsed["model"], "plain-model-name",
        "无 slug 时不重写 model"
    );
}

/// Stage 3.1:adapter 负责路径规范化,`/v1/foo` 走 openai_chat 适配器后
/// 上游收到 `/foo`(因为 baseUrl 已含 `/v1`,这样合起来恰好一份)。
#[tokio::test]
async fn adapter_normalizes_v1_prefix_and_keeps_query() {
    let s = build_stack().await;
    let resp = client()
        .get(format!("http://{}/v1/models?deep=1&order=asc", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let v = body_json(resp).await;
    assert_eq!(
        v["path"].as_str().unwrap(),
        "/models?deep=1&order=asc",
        "openai_chat adapter 应把入站 /v1/foo 规范化为 /foo,query 保留"
    );
    assert_eq!(v["method"].as_str().unwrap(), "GET");
}

/// 入站不带 /v1 前缀的路径不应被改写。
#[tokio::test]
async fn adapter_passes_through_paths_without_v1_prefix() {
    let s = build_stack().await;
    let resp = client()
        .get(format!("http://{}/models", s.proxy))
        .header("authorization", "Bearer cas_test_gw")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let v = body_json(resp).await;
    assert_eq!(v["path"].as_str().unwrap(), "/models");
}

#[tokio::test]
async fn openai_chat_provider_handles_responses_route_like_legacy_proxy() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let upstream = spawn(chat_sse_capture_mock(calls.clone())).await;
    let mut active = provider(
        "kimi-code",
        &format!("http://{upstream}/v1"),
        "sk-kimi",
        "bearer",
        &[("User-Agent", "KimiCLI/1.40.0")],
    );
    active
        .models
        .insert("default".into(), "kimi-for-coding".into());
    let resolver = Arc::new(StaticResolver::new(
        Some("cas_test_gw".into()),
        vec![active],
        Some("kimi-code".into()),
    ));
    let proxy = spawn(build_router(resolver)).await;

    let resp = client()
        .post(format!("http://{proxy}/responses"))
        .header("authorization", "Bearer cas_test_gw")
        .header("content-type", "application/json")
        .body(
            json!({
                "model": "gpt-5.5",
                "input": "hello",
                "stream": true
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let _ = resp.text().await.unwrap();

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["method"], "POST");
    assert_eq!(
        calls[0]["path"], "/v1/chat/completions",
        "Codex /responses must be handled locally and converted to upstream Chat Completions for openai_chat providers"
    );
    let body: serde_json::Value = serde_json::from_str(calls[0]["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["model"], "kimi-for-coding");
    assert_eq!(body["stream"], true);
    assert!(body["messages"].is_array());
}

#[tokio::test]
async fn websocket_responses_route_uses_legacy_responses_conversion() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let upstream = spawn(chat_sse_capture_mock(calls.clone())).await;
    let mut active = provider(
        "kimi-code",
        &format!("http://{upstream}/v1"),
        "sk-kimi",
        "bearer",
        &[("User-Agent", "KimiCLI/1.40.0")],
    );
    active
        .models
        .insert("default".into(), "kimi-for-coding".into());
    let resolver = Arc::new(StaticResolver::new(
        Some("cas_test_gw".into()),
        vec![active],
        Some("kimi-code".into()),
    ));
    let proxy = spawn(build_router(resolver)).await;

    let mut request = format!("ws://{proxy}/responses")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_static("Bearer cas_test_gw"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();
    socket
        .send(WsMessage::Text(
            json!({
                "type": "response.create",
                "response": {
                    "model": "gpt-5.5",
                    "input": "hello"
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap();

    let first_message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("websocket response timed out")
        .expect("websocket closed")
        .expect("websocket message");
    let WsMessage::Text(text) = first_message else {
        panic!("expected text websocket response");
    };
    let payload: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_ne!(
        payload["type"], "error",
        "websocket should not return error"
    );

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["method"], "POST");
    assert_eq!(calls[0]["path"], "/v1/chat/completions");
    let body: serde_json::Value = serde_json::from_str(calls[0]["body"].as_str().unwrap()).unwrap();
    assert_eq!(body["model"], "kimi-for-coding");
    assert_eq!(body["stream"], true);
    assert!(body["messages"].is_array());
}

#[tokio::test]
async fn qwen_openai_chat_provider_responses_route_rewrites_and_auths() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let upstream = spawn(chat_sse_capture_mock(calls.clone())).await;
    let mut qwen = provider(
        "bailian",
        &format!("http://{upstream}/v1"),
        "sk-qwen-upstream",
        "bearer",
        &[],
    );
    qwen.models.insert("default".into(), "qwen3.6-plus".into());
    let resolver = Arc::new(StaticResolver::new(
        Some("cas_test_qwen_gateway".into()),
        vec![qwen],
        Some("bailian".into()),
    ));
    let proxy = spawn(build_router(resolver)).await;

    // #region agent log
    agent_debug_log(
        "H3",
        "crates/proxy/tests/auth_and_routing.rs:qwen_openai_chat_provider_responses_route_rewrites_and_auths:before_request",
        "sending qwen responses request through proxy",
        json!({
            "proxyAddr": proxy.to_string(),
            "incomingGatewayAuth": "Bearer cas_test_qwen_gateway",
            "requestModel": "gpt-5.5",
        }),
    );
    // #endregion

    let resp = client()
        .post(format!("http://{proxy}/v1/responses"))
        .header("authorization", "Bearer cas_test_qwen_gateway")
        .header("content-type", "application/json")
        .body(
            json!({
                "model": "gpt-5.5",
                "input": "hello qwen",
                "stream": true
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let _ = resp.text().await.unwrap();

    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    let method = calls[0]["method"].as_str().unwrap_or_default();
    let path = calls[0]["path"].as_str().unwrap_or_default();
    let body: serde_json::Value = serde_json::from_str(calls[0]["body"].as_str().unwrap()).unwrap();

    // #region agent log
    agent_debug_log(
        "H4",
        "crates/proxy/tests/auth_and_routing.rs:qwen_openai_chat_provider_responses_route_rewrites_and_auths:upstream_capture",
        "captured upstream request for qwen",
        json!({
            "method": method,
            "path": path,
            "rewrittenModel": body["model"],
            "stream": body["stream"],
            "hasMessagesArray": body["messages"].is_array(),
        }),
    );
    // #endregion

    assert_eq!(method, "POST");
    assert_eq!(path, "/v1/chat/completions");
    assert_eq!(body["model"], "qwen3.6-plus");
    assert_eq!(body["stream"], true);
    assert!(body["messages"].is_array());
}
