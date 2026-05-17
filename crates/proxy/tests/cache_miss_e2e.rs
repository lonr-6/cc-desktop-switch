//! 端到端集成:Codex CLI 用旧 `previous_response_id` 续轮但 session cache miss
//! + 当前 input 也空 → 代理在到达上游前应直接返回 OpenAI SDK-compatible 400
//! `code: "previous_response_not_found"`,**不**让请求出门,**不**触发上游 RTT。
//!
//! 拓扑:
//!     reqwest client ──► [Rust 代理 axum + StaticResolver Responses adapter]
//!
//! (无上游 mock — 测试目的就是验证请求**根本不会**到上游就被代理 short-circuit)

use std::sync::Arc;

use codex_app_transfer_proxy::{build_router, StaticResolver};
use codex_app_transfer_registry::Provider;
use indexmap::IndexMap;
use tokio::net::TcpListener;

async fn spawn_proxy_no_upstream() -> std::net::SocketAddr {
    // provider.base_url 写一个 RFC5737 测试网段 + 不可达端口,确保哪怕 router 真的
    // 试图打上游也会立即失败,反向证明 cache miss 是在 forward → adapter 转换阶段
    // 拦下的 — 而不是上游返的 400。
    let provider = Provider {
        id: "must-not-reach".into(),
        name: "Must Not Reach Upstream".into(),
        base_url: "http://192.0.2.1:1".into(),
        auth_scheme: "none".into(),
        // `openai_chat` 跟 8 条 builtin preset 真实配置一致:Codex.app 入站
        // /v1/responses 仍命中 ResponsesAdapter(通过 lookup_for_request 第一层
        // short-circuit),触发本地 cache miss → 400 short-circuit。
        // 注:`responses` 字面值现在归 ResponsesPassthroughAdapter(字节级透传
        // 上游 OpenAI Responses API),不走本地 cache,与本测试场景不符。
        api_format: "openai_chat".into(),
        api_key: String::new(),
        models: IndexMap::new(),
        extra_headers: IndexMap::new(),
        model_capabilities: IndexMap::new(),
        request_options: IndexMap::new(),
        is_builtin: false,
        sort_index: 0,
        extra: IndexMap::new(),
    };
    let resolver = Arc::new(StaticResolver::new(
        None,
        vec![provider],
        Some("must-not-reach".into()),
    ));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, build_router(resolver).into_make_service())
            .await
            .unwrap();
    });
    addr
}

#[tokio::test]
async fn cache_miss_with_empty_input_returns_openai_sdk_compatible_400() {
    let addr = spawn_proxy_no_upstream().await;
    let url = format!("http://{addr}/v1/responses");

    // 模拟 Codex CLI 续轮:带 previous_response_id 但 session cache 进程刚启动
    // 必然 miss;同时 input 也是空数组(典型场景:用户重启 Tauri / 长会话超 1h)
    let body = serde_json::json!({
        "model": "x",
        "stream": true,
        "previous_response_id": "resp_definitely_not_in_cache",
        "input": [],
        "tools": [{"type":"function","name":"shell","parameters":{"type":"object"}}]
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .unwrap();
    let body_str = serde_json::to_string(&body).unwrap();
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .body(body_str)
        .send()
        .await
        .unwrap();

    // ── 关键断言:必须是 HTTP 400 而非 502/timeout(否则说明请求出门了)
    assert_eq!(
        resp.status().as_u16(),
        400,
        "cache miss + empty input 必须在代理本地拦截,不可让请求到上游(实际响应 {})",
        resp.status()
    );
    assert!(
        resp.headers()
            .get("content-type")
            .and_then(|v: &reqwest::header::HeaderValue| v.to_str().ok())
            .unwrap_or("")
            .starts_with("application/json"),
        "content-type 必须是 application/json,实际 {:?}",
        resp.headers().get("content-type")
    );

    let body_bytes = resp.bytes().await.unwrap();
    let body_text = String::from_utf8_lossy(&body_bytes).to_string();
    let body: serde_json::Value = serde_json::from_str(&body_text)
        .unwrap_or_else(|e| panic!("body 必须是合法 JSON ({e}):{body_text}"));
    // 字段必须**字面**对齐 OpenAI Responses API 服务端真实行为
    assert_eq!(body["error"]["type"], "invalid_request_error");
    assert_eq!(body["error"]["code"], "previous_response_not_found");
    assert_eq!(body["error"]["param"], "previous_response_id");
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("resp_definitely_not_in_cache"),
        "message 必须含失效 ID 让客户端 SDK 提取,实际:{message}"
    );
    assert!(
        message.starts_with("Previous response with id"),
        "措辞必须对齐 OpenAI(LM Studio bug tracker #1188 实测格式),实际:{message}"
    );
}
