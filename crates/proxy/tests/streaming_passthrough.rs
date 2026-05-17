//! 端到端 SSE 透传测试.
//!
//! 拓扑:
//!     reqwest client ──► [Rust 代理 axum + StaticResolver] ──► [上游 mock axum]
//!
//! 数据源:Stage 0 那份 `_example_openai_chat_streaming.json` fixture
//!(以 `_` 开头,正式 `list_fixtures()` 不会收录,仅作为基建样例)。
//!
//! 验证目标:
//! 1. 上游响应的 SSE 帧能逐字节穿过 Rust 代理回到客户端
//! 2. `text/event-stream` 等响应头被保留
//! 3. fixture.expected.stream_substrings 全部命中

use std::path::PathBuf;
use std::sync::Arc;

use codex_app_transfer_proxy::{
    build_router,
    fixture::{build_upstream_mock, Fixture},
    StaticResolver,
};
use codex_app_transfer_registry::Provider;
use indexmap::IndexMap;
use tokio::net::TcpListener;

fn fixture_path(rel: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // -> crates/
    p.pop(); // -> repo root
    p.push(rel);
    p
}

async fn spawn(router: axum::Router) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    addr
}

fn provider_for(upstream_base: &str, api_format: &str) -> Provider {
    Provider {
        id: "test-upstream".into(),
        name: "Test Upstream".into(),
        base_url: upstream_base.into(),
        auth_scheme: "none".into(), // 不向 mock 上游写鉴权,简化样例
        api_format: api_format.into(),
        api_key: String::new(),
        models: IndexMap::new(),
        extra_headers: IndexMap::new(),
        model_capabilities: IndexMap::new(),
        request_options: IndexMap::new(),
        is_builtin: false,
        sort_index: 0,
        extra: IndexMap::new(),
    }
}

async fn run_passthrough_against(fixture_rel: &str) {
    run_passthrough_against_with_format(fixture_rel, "openai_chat").await;
}

async fn run_passthrough_against_with_format(fixture_rel: &str, api_format: &str) {
    let fixture = Fixture::load(fixture_path(fixture_rel))
        .unwrap_or_else(|e| panic!("load fixture {fixture_rel}: {e}"));

    // 1. 起上游 mock,在随机端口
    let upstream_addr = spawn(build_upstream_mock(&fixture)).await;
    let upstream_base = format!("http://{upstream_addr}");

    // 2. 起代理,resolver 把所有请求 fallback 到 test-upstream provider
    let resolver = Arc::new(StaticResolver::new(
        None, // smoke 测试不要求 gateway 鉴权
        vec![provider_for(&upstream_base, api_format)],
        Some("test-upstream".into()),
    ));
    let proxy_addr = spawn(build_router(resolver)).await;

    // 3. 客户端按 fixture.client_request 投递到代理
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let url = format!("http://{proxy_addr}{}", fixture.client_request.path);
    let mut req = client.request(fixture.client_request.method.parse().unwrap(), &url);
    for (k, v) in &fixture.client_request.headers {
        req = req.header(k, v);
    }
    if let Some(body) = &fixture.client_request.body_json {
        let s = serde_json::to_string(body).unwrap();
        req = req.header("content-type", "application/json").body(s);
    } else if let Some(text) = &fixture.client_request.body_text {
        req = req.body(text.clone());
    }

    let resp = req.send().await.expect("proxy client send");

    // 4. 校验 expected
    if let Some(expected_status) = fixture.expected.status {
        assert_eq!(resp.status().as_u16(), expected_status, "status mismatch");
    }
    for (k, expected_v) in &fixture.expected.headers_contain {
        let got = resp
            .headers()
            .get(k)
            .map(|v| v.to_str().unwrap_or("").to_owned())
            .unwrap_or_default();
        assert!(
            got.contains(expected_v),
            "header {k}={got:?} 不含 {expected_v:?}"
        );
    }

    let body_bytes = resp.bytes().await.expect("read proxy body");
    let body_text = String::from_utf8_lossy(&body_bytes);
    for needle in fixture
        .expected
        .body_substrings
        .iter()
        .chain(fixture.expected.stream_substrings.iter())
    {
        assert!(
            body_text.contains(needle.as_str()),
            "proxy 响应缺少子串 {needle:?};\nbody = {body_text}"
        );
    }

    // 5. 强校验:Rust 代理输出应与 fixture upstream 拼接的字节一致
    //    —— 证明 Stage 2 在 SSE 路径上是 0 转换、0 丢失的
    let upstream_concat: String = fixture
        .upstream
        .first()
        .unwrap()
        .response
        .stream
        .iter()
        .map(|f| f.data.clone())
        .collect();
    assert_eq!(
        body_text.as_ref(),
        upstream_concat.as_str(),
        "代理输出应与上游字节流逐字节一致"
    );
}

#[tokio::test]
async fn sse_passthrough_synthetic_example() {
    run_passthrough_against("tests/replay/fixtures/_example_openai_chat_streaming.json").await;
}

/// 真实 Kimi (Moonshot) SSE 响应回放 —— 4 帧含 reasoning_content / usage / [DONE].
/// 这是 Stage 3 真正会面对的形态:`delta.reasoning_content`(思考链) +
/// 末帧 `usage` + 不同 `system_fingerprint`,合成 fixture 没法覆盖.
#[tokio::test]
async fn sse_passthrough_real_kimi() {
    run_passthrough_against("tests/replay/fixtures/kimi_chat_minimal_streaming.json").await;
}

/// ResponsesPassthroughAdapter 字节级透传验证 —— 自定义第三方 provider 选
/// apiFormat=responses 时,Codex.app 入站 /v1/responses 应**字节级透传**给上游
/// (不被 ChatToResponsesConverter 重写 envelope / sequence_number / resp_id 等)。
/// 上游 mock 返 OpenAI Responses API style SSE,期望客户端拿到的字节流跟上游
/// 拼接一致 —— 等价 OpenAiChatAdapter 的 passthrough 行为,但走 /responses 路径。
#[tokio::test]
async fn responses_passthrough_byte_identical() {
    run_passthrough_against_with_format(
        "tests/replay/fixtures/_example_responses_passthrough_streaming.json",
        "responses",
    )
    .await;
}

/// H2 (silent-failure-hunter review):passthrough 上游 4xx 错误体字节级透传。
/// 防回归:`forward.rs::unwrap_or_default` 静默吞 body read 错误改成 telemetry warn,
/// 上游 400 + JSON error body 完整原样透回客户端,不丢失 error.code / error.message。
#[tokio::test]
async fn responses_passthrough_upstream_error_body_passthrough() {
    run_passthrough_against_with_format(
        "tests/replay/fixtures/_example_responses_passthrough_error.json",
        "responses",
    )
    .await;
}
