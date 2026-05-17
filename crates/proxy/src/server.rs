//! axum router 构造与启动 helper.

use axum::{
    body::{to_bytes, Body},
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{HeaderMap, Method, Request},
    response::IntoResponse,
    routing::{any, get},
    Router,
};
use futures_util::StreamExt;
use serde_json::json;

use crate::forward::{forward_handler, ProxyState};
use crate::resolver::SharedResolver;

/// 把所有方法 / 所有路径都路由到 `forward_handler`(裸代理 + B1 路由 + B2 鉴权改写)。
/// Stage 3 起此 router 会再叠 adapter 中间件(provider 协议转换)。
pub fn build_router(resolver: SharedResolver) -> Router {
    let state = ProxyState::new(resolver);
    Router::new()
        .route(
            "/responses",
            get(responses_websocket_handler)
                .post(forward_handler)
                .options(forward_handler),
        )
        .route(
            "/v1/responses",
            get(responses_websocket_handler)
                .post(forward_handler)
                .options(forward_handler),
        )
        .route(
            "/openai/v1/responses",
            get(responses_websocket_handler)
                .post(forward_handler)
                .options(forward_handler),
        )
        .fallback(any(forward_handler))
        .with_state(state)
}

async fn responses_websocket_handler(
    State(state): State<ProxyState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| responses_websocket_loop(socket, state, headers))
}

async fn responses_websocket_loop(mut socket: WebSocket, state: ProxyState, headers: HeaderMap) {
    while let Some(message) = socket.next().await {
        let Ok(message) = message else {
            break;
        };
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Binary(bytes) => match String::from_utf8(bytes.to_vec()) {
                Ok(text) => text,
                Err(_) => {
                    send_ws_error(&mut socket, "Invalid UTF-8 message").await;
                    continue;
                }
            },
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(message_json) = serde_json::from_str::<serde_json::Value>(&text) else {
            send_ws_error(&mut socket, "Invalid JSON").await;
            continue;
        };
        if message_json.get("type").and_then(|v| v.as_str()) != Some("response.create") {
            continue;
        }
        let mut body = extract_response_create_body(&message_json);
        // Codex CLI ws warmup(`prewarm_websocket`,`generate: false`)与"新
        // session 首帧 input 为空 + 无 previous_response_id"这两类 frame
        // 上游(任何 chat-completions 兼容 provider)必然 400 — 因为转换后
        // messages 是空数组。**不要**转 HTTP 浪费一次 RTT,直接给 ws 客户端
        // 送 stream error 让 Codex 立即按 ws stream error 处理(进 stream retry
        // 并在 retry 耗尽后 fallback 到 HTTP `stream_responses_api`,后者发
        // 完整 history 必然成功)。
        //
        // 注意保留:`input: [] + previous_response_id != ""` 仍走转发路径,
        // 这是 ws incremental delta=0 续轮 — 走 ResponseSessionCache 查历史
        // (PR #65 sqlite 持久化覆盖)。
        if should_skip_upstream_warmup(&body) {
            // **关键**:OpenAI Responses ws 协议里 `{"type":"error",...}` 是
            // "流内错误事件"(stream-level)语义 —— Codex CLI ws 客户端收到
            // 后**不会**立即放弃 ws session,可能等 ws idle timeout(实测 ~5
            // 分钟,见反馈 fb-8f5b51fb / fb-0c121681)才 fallback HTTP。
            //
            // 应当**直接关闭 ws 连接**(发 Close frame),让 Codex CLI 立即
            // 看到"ws 通道不可用"→ 进入 try_switch_fallback_transport →
            // HTTP 路径,total wait 从 ~5 分钟降到秒级。
            //
            // 之前 v2.0.11 PR #67 用 send_ws_error 是错的协议语义,本次
            // 修正为 close。
            let _ = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: axum::extract::ws::close_code::UNSUPPORTED,
                    reason: "warmup / empty-input frame not supported; fall back to HTTP".into(),
                })))
                .await;
            break;
        }
        if body.get("stream").is_none() {
            body["stream"] = serde_json::Value::Bool(true);
        }
        let body_bytes = match serde_json::to_vec(&body) {
            Ok(bytes) => bytes,
            Err(error) => {
                send_ws_error(&mut socket, &format!("Invalid response body: {error}")).await;
                continue;
            }
        };
        let req = websocket_forward_request(&headers, body_bytes);
        let response = match forward_handler(State(state.clone()), req).await {
            Ok(response) => response,
            Err(error) => error.into_response(),
        };
        if !stream_forward_response_to_websocket(response, &mut socket).await {
            break;
        }
    }
}

fn extract_response_create_body(message: &serde_json::Value) -> serde_json::Value {
    if let Some(response) = message.get("response").filter(|v| v.is_object()) {
        return response.clone();
    }
    let mut body = serde_json::Map::new();
    if let Some(obj) = message.as_object() {
        for (key, value) in obj {
            if key != "type" {
                body.insert(key.clone(), value.clone());
            }
        }
    }
    serde_json::Value::Object(body)
}

fn websocket_forward_request(headers: &HeaderMap, body: Vec<u8>) -> axum::extract::Request {
    let mut builder = Request::builder().method(Method::POST).uri("/responses");
    for (name, value) in headers {
        if name == axum::http::header::AUTHORIZATION {
            builder = builder.header(name, value);
        }
    }
    builder
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("websocket forward request")
}

async fn stream_forward_response_to_websocket(
    response: axum::response::Response,
    socket: &mut WebSocket,
) -> bool {
    let status = response.status();
    let body = response.into_body();
    if !status.is_success() {
        let bytes = to_bytes(body, 64 * 1024).await.unwrap_or_default();
        let message = String::from_utf8_lossy(&bytes);
        send_ws_error(
            socket,
            &format!("unexpected status {}: {}", status.as_u16(), message.trim()),
        )
        .await;
        return true;
    }

    let mut stream = body.into_data_stream();
    let mut pending = String::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            send_ws_error(socket, "stream read failed").await;
            return true;
        };
        pending.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(idx) = pending.find('\n') {
            let mut line = pending[..idx].to_owned();
            pending.drain(..idx + 1);
            if line.ends_with('\r') {
                line.pop();
            }
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if socket
                    .send(Message::Text(data.to_owned().into()))
                    .await
                    .is_err()
                {
                    return false;
                }
            }
        }
    }
    true
}

async fn send_ws_error(socket: &mut WebSocket, message: &str) {
    let payload = json!({
        "type": "error",
        "error": {
            "message": message,
        },
    })
    .to_string();
    let _ = socket.send(Message::Text(payload.into())).await;
}

/// 识别**应当跳过上游转发**的 ws frame —— 这些 frame 转发到任何 chat-completions
/// 兼容 provider 必然 400(messages 空),应当直接 ws 错误响应让 Codex CLI 立即
/// 进 stream-retry / fallback-HTTP 路径,避免一次无意义的上游 RTT。
///
/// 当前匹配两类:
/// 1. **显式 warmup**:`generate: false`(Codex CLI `prewarm_websocket` /
///    `stream_responses_websocket(warmup=true)` 会显式设这个字段,见
///    `codex-rs/core/src/client.rs:1334-1343`)。语义是"预热 ws 连接,不真正
///    生成内容",上游 chat-completions API 不支持这个语义。
///
/// 2. **空 input + 无 previous_response_id**:任何来源(可能是客户端 bug /
///    探活 / 边界场景)都不可能产生合法的 chat 请求(转换后 messages 必空)。
///    保留 `previous_response_id != ""` 的空 input 帧不命中本规则 —— 那是 ws
///    incremental delta=0 续轮,走 ResponseSessionCache 查历史的合法路径。
///
/// 不识别 instructions:即使有 instructions(system message),没有真实 user
/// turn 仍然是一次纯 system 请求,部分 provider 也会 400;但 instructions
/// 路径较少出现在 ws frame 里,先不做特殊处理避免误杀。
fn should_skip_upstream_warmup(body: &serde_json::Value) -> bool {
    let generate_false = body
        .get("generate")
        .and_then(|v| v.as_bool())
        .map(|b| !b)
        .unwrap_or(false);
    if generate_false {
        return true;
    }

    let input_empty = match body.get("input") {
        None => true,
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Array(arr)) => arr.is_empty(),
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        // input 是其它形式(object 等)—— 极少见,但既然不空就别拦
        Some(_) => false,
    };
    if !input_empty {
        return false;
    }

    let prev_id = body
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    // 空 input + 无 previous_response_id = 纯空 frame,跳过上游
    prev_id.is_empty()
}

#[cfg(test)]
mod tests {
    use super::should_skip_upstream_warmup;
    use serde_json::json;

    #[test]
    fn skips_explicit_warmup_with_generate_false() {
        // Codex CLI prewarm_websocket: `ws_payload.generate = Some(false)`
        let body = json!({
            "input": [],
            "generate": false,
        });
        assert!(should_skip_upstream_warmup(&body));
    }

    #[test]
    fn skips_empty_input_without_previous_response_id() {
        // 新 session 首帧:input 空 + 没有 previous_response_id
        // (典型场景:客户端误发 / 探活 / 实测真机 13:03:06 case)
        let body = json!({"input": []});
        assert!(should_skip_upstream_warmup(&body));
        let body_no_input = json!({});
        assert!(should_skip_upstream_warmup(&body_no_input));
        let body_null_input = json!({"input": null});
        assert!(should_skip_upstream_warmup(&body_null_input));
        let body_empty_string = json!({"input": "  "});
        assert!(should_skip_upstream_warmup(&body_empty_string));
    }

    #[test]
    fn does_not_skip_incremental_turn_with_previous_response_id() {
        // ws incremental delta=0:input 空 + previous_response_id 命中 cache
        // → 走 ResponseSessionCache 查历史(PR #65 sqlite 持久化覆盖),
        // **不能跳**,要让 forward_handler 处理。
        let body = json!({
            "input": [],
            "previous_response_id": "resp_abc123",
        });
        assert!(!should_skip_upstream_warmup(&body));
    }

    #[test]
    fn does_not_skip_normal_turn_with_user_message() {
        let body = json!({
            "input": [
                {"type": "message", "role": "user", "content": "hi"}
            ]
        });
        assert!(!should_skip_upstream_warmup(&body));
    }

    #[test]
    fn does_not_skip_string_input() {
        let body = json!({"input": "non-empty user prompt"});
        assert!(!should_skip_upstream_warmup(&body));
    }

    #[test]
    fn generate_true_does_not_skip_even_with_empty_input() {
        // 边界:client 明确 generate=true 但 input 空 + 无 prev id
        // 仍然按"空 input + 无 prev id"跳过(generate=true 不抢救它)
        let body = json!({
            "input": [],
            "generate": true,
        });
        assert!(should_skip_upstream_warmup(&body));
    }
}
