//! 复用 `tests/replay/fixtures/` 的 fixture 格式;Rust 端 schema 与 Python
//! `tests/replay/fixture.py` 一一对应。
//!
//! 当前只为 Stage 2 测试服务:把 `upstream[0].response` 喂给一个临时
//! axum 服务,模拟上游;把 `client_request` 投递到我们的代理上;按
//! `expected` 校验结果。

use std::path::Path;

use axum::{body::Body, response::Response, routing::any, Router};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Fixture {
    pub name: String,
    #[serde(default)]
    pub provider: String,
    pub client_request: ClientRequest,
    #[serde(default)]
    pub upstream: Vec<UpstreamCall>,
    pub expected: Expected,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientRequest {
    #[serde(default = "default_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub body_json: Option<serde_json::Value>,
    #[serde(default)]
    pub body_text: Option<String>,
}

fn default_method() -> String {
    "POST".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamCall {
    pub url_pattern: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub response: UpstreamResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpstreamResponse {
    #[serde(default = "default_status")]
    pub status: u16,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub body_text: Option<String>,
    #[serde(default)]
    pub body_json: Option<serde_json::Value>,
    #[serde(default)]
    pub stream: Vec<Frame>,
}

fn default_status() -> u16 {
    200
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Frame {
    pub data: String,
    #[serde(default)]
    pub delay_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Expected {
    #[serde(default)]
    pub status: Option<u16>,
    #[serde(default)]
    pub headers_contain: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub body_text: Option<String>,
    #[serde(default)]
    pub body_substrings: Vec<String>,
    #[serde(default)]
    pub stream_substrings: Vec<String>,
}

impl Fixture {
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&s)?)
    }
}

fn is_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

/// 用 fixture 的第一个 upstream 调用,构造一个返回该响应的"上游模拟"axum router。
/// 路径无关:对任何请求都回放固定响应(适合测试场景中作为"上游服务"启动)。
pub fn build_upstream_mock(fixture: &Fixture) -> Router {
    let upstream = fixture
        .upstream
        .first()
        .expect("fixture must have at least one upstream call")
        .response
        .clone();
    Router::new().fallback(any(move || {
        let upstream = upstream.clone();
        async move {
            let mut builder = Response::builder().status(upstream.status);
            for (k, v) in &upstream.headers {
                // 真实抓取的 fixture 里会带 transfer-encoding/chunked /
                // connection/keep-alive 等 hop-by-hop 头;mock 自己用固定长度
                // body 回放,这些头会让 axum 输出非法 HTTP 帧 → 客户端 502。
                // 一律剔除,由 axum / hyper 重新决定如何分包。
                if is_hop_header(k) {
                    continue;
                }
                builder = builder.header(k, v);
            }
            // 三种 body 形态优先级:stream > body_json > body_text
            let body: Body = if !upstream.stream.is_empty() {
                let bytes = upstream
                    .stream
                    .iter()
                    .map(|f| f.data.clone())
                    .collect::<String>();
                Body::from(bytes)
            } else if let Some(j) = upstream.body_json {
                Body::from(j.to_string())
            } else if let Some(t) = upstream.body_text {
                Body::from(t)
            } else {
                Body::empty()
            };
            builder.body(body).unwrap()
        }
    }))
}
