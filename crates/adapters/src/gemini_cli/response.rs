//! Cloud Code SSE outer envelope unwrap。
//!
//! gemini_native 的 SSE → Responses 状态机([`crate::gemini_native::response::
//! convert_gemini_to_responses_stream`])期待 wire 是 `data: {candidates,...}\n\n`。
//! Cloud Code Assist 给每个 event 多包一层 `{response: {...}}`:
//!
//! ```text
//! data: {"response":{"candidates":[...],"usageMetadata":{...}}}\n\n
//! ```
//!
//! 本模块在 byte stream 入口插一层 transformer,把每个 SSE event 的 JSON 取出
//! `.response` 字段,重新序列化回 `data: <inner>\n\n`,然后传给 native 的状态机
//! 复用所有现成转换逻辑(reasoning / function_call / annotations / failure 等)。
//!
//! 实现策略:行级 buffer + 按 `\n\n` 切完整 event,每 event 独立处理。
//! - 多个 event 一次到达 → 切开逐个处理
//! - event 跨 chunk 到达 → buffer 累积直到 `\n\n` 边界
//! - data 字段不是有效 JSON / 没有 `.response` 字段 → 透传原 event(防御性,Cloud
//!   Code 偶尔返非 wrap 形态比如 keepalive pings)

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use serde_json::Value;

use crate::types::ByteStream;

/// 把 Cloud Code SSE byte stream → unwrapped Gemini SSE byte stream。
///
/// 每个 event 是 SSE 标准格式:
/// ```text
/// event: <name>\n
/// data: <json>\n
/// \n
/// ```
///
/// 我们解析 `data:` 后 JSON,如果含 `.response` 字段就 emit `data: <response>\n\n`,
/// 否则原样透传(防御 keepalive / 非 wrap 事件)。`event:` 行如果存在也保留。
pub fn unwrap_cloud_code_sse_envelope(input: ByteStream) -> ByteStream {
    Box::pin(UnwrapStream {
        inner: input,
        buffer: Vec::new(),
        finished: false,
    })
}

struct UnwrapStream {
    inner: ByteStream,
    buffer: Vec<u8>,
    finished: bool,
}

impl Stream for UnwrapStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            // 1. 先看 buffer 里有没有完整 event(以 \n\n 结尾)
            if let Some((event, rest)) = take_complete_event(&this.buffer) {
                this.buffer = rest;
                let processed = process_event(&event);
                if !processed.is_empty() {
                    return Poll::Ready(Some(Ok(Bytes::from(processed))));
                }
                // 如果该 event 处理后输出空(比如非 wrap 格式 + 解析失败),
                // 跳过继续看 buffer 下个 event
                continue;
            }

            // 2. buffer 没完整 event,从 inner stream 拉新 chunk
            if this.finished {
                return Poll::Ready(None);
            }
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    this.buffer.extend_from_slice(&chunk);
                    // continue loop:回到第 1 步看是否能切出 event
                }
                Poll::Ready(Some(Err(e))) => {
                    this.finished = true;
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    this.finished = true;
                    // EOF 时 buffer 里可能有"没以 \n\n 结尾"的残留 — 当成最后一个
                    // event 处理(SSE 协议建议 EOF 也算一个 event 边界)
                    if !this.buffer.is_empty() {
                        let last = std::mem::take(&mut this.buffer);
                        let processed = process_event(&last);
                        if !processed.is_empty() {
                            return Poll::Ready(Some(Ok(Bytes::from(processed))));
                        }
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// 在 buffer 里找第一个完整 SSE event(以 `\n\n` 结尾)。返回 (event_bytes, remaining)。
/// 如果找不到完整 event 返 None。
fn take_complete_event(buffer: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    // SSE event 边界是 `\n\n`(两个连续 LF)。CRLF 客户端 `\r\n\r\n` 也兼容。
    let lf2 = find_subseq(buffer, b"\n\n");
    let crlf2 = find_subseq(buffer, b"\r\n\r\n");
    let (boundary_end, sep_len) = match (lf2, crlf2) {
        (Some(a), Some(b)) if a < b => (a + 2, 2),
        (Some(_), Some(b)) => (b + 4, 4),
        (Some(a), None) => (a + 2, 2),
        (None, Some(b)) => (b + 4, 4),
        (None, None) => return None,
    };
    let _ = sep_len; // 仅用于解释
    let event = buffer[..boundary_end].to_vec();
    let rest = buffer[boundary_end..].to_vec();
    Some((event, rest))
}

/// 朴素 byte slice substring 查找。SSE event 通常很小(<10KB),朴素 O(n*m) 够用。
fn find_subseq(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 处理单个 SSE event,把 `data:` 里的 JSON 解 `.response` 重新序列化。返回新的
/// event bytes(含 trailing `\n\n`)。失败时返原 event 透传(防御 keepalive / 非
/// wrap 形态)。
fn process_event(event: &[u8]) -> Vec<u8> {
    // 找 `data:` 行
    let text = match std::str::from_utf8(event) {
        Ok(s) => s,
        Err(_) => return event.to_vec(),
    };
    // SSE 一个 event 可能有多行(event:/id:/data:),我们只动 data: 行,其他原样保留
    let mut new_lines: Vec<String> = Vec::new();
    let mut data_payload: Option<String> = None;
    for line in text.split('\n') {
        if let Some(rest) = line.strip_prefix("data: ") {
            // 多行 data 在 SSE 协议里 concat,但 Cloud Code 实测一行 = 一个 JSON,
            // 这里直接当单行处理(简单 + 跟 gemini_native::response 状态机一致)
            data_payload = Some(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("data:") {
            // SSE 允许 `data:<no space>` 形态(罕见)
            data_payload = Some(rest.to_owned());
        } else {
            new_lines.push(line.to_owned());
        }
    }

    let new_data = match data_payload {
        Some(payload) => {
            // 解析 JSON,提 .response 字段
            match serde_json::from_str::<Value>(&payload) {
                Ok(Value::Object(mut obj)) => {
                    if let Some(inner) = obj.remove("response") {
                        match serde_json::to_string(&inner) {
                            Ok(s) => s,
                            Err(_) => payload, // 序列化失败极不正常,原样透传
                        }
                    } else if let Some(error) = obj.get("error") {
                        // **Critical** silent-failure 修(2026-05-11 完整版):Cloud Code 上游
                        // 在 HTTP 200 SSE 流中间偶发 `data: {"error":{...}}` 事件(quota 中途
                        // 耗尽 / 项目权限丢 / 内部异常),没 `.response` wrap 字段。原版当
                        // keepalive silent 透传 → native 看不懂忽略 → 用户 Codex.app 收到
                        // 无 candidates 的空流 silent 完成。
                        //
                        // 完整修法:转成 Gemini wire 形式的合法事件 — emit 一条带 ⚠️ 前缀的
                        // assistant text 把 upstream error.message 直接放给 user 看,加
                        // finishReason=OTHER 让 native 状态机走完整 lifecycle(text delta +
                        // text done + completed),client 收到的是 response.completed 含
                        // "⚠️ Cloud Code error: <message>" 的 output_text 而不是空流。
                        let error_message = error
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("(no message)");
                        let error_code = error.get("code").and_then(|v| v.as_i64());
                        let error_status = error.get("status").and_then(|v| v.as_str());
                        tracing::error!(
                            body = %payload,
                            error_code = ?error_code,
                            error_status = ?error_status,
                            message = %error_message,
                            "Cloud Code 200 流中 inline error event;转成 candidates[].text 让 user 看到 error.message"
                        );
                        let display_text = format!("⚠️ Cloud Code error: {error_message}");
                        // **关键**:finishReason 必须让 native 状态机产 status="incomplete"
                        // 而不是 "completed"。`OTHER` 在 map_finish_reason 落到默认 "stop"
                        // → emit_completed 走 completed 分支 → client 误以为成功(即便有 ⚠️
                        // 文字 client 的 retry/telemetry 仍当 success)。改用 `SAFETY`:
                        // map_finish_reason → "content_filter" + emit_completed 产
                        // status="incomplete" + incomplete_details.reason="content_filter"。
                        // 语义略偏(实际不是 safety block 是 upstream 内部错),但 ⚠️ 文字
                        // 已说明真实原因,且 client 能正确识别"非 normal completion"
                        let synthetic = serde_json::json!({
                            "candidates": [{
                                "content": {
                                    "role": "model",
                                    "parts": [{"text": display_text}]
                                },
                                "finishReason": "SAFETY"
                            }]
                        });
                        synthetic.to_string()
                    } else {
                        // 真正的 keepalive(纯 comment / 心跳)— 原样透传
                        payload
                    }
                }
                Ok(_) | Err(_) => payload, // JSON parse 失败原样透传(防御)
            }
        }
        None => return event.to_vec(), // 没 data: 行(纯 comment 等)原样透传
    };

    // 重新组装 event
    let mut out = String::new();
    for line in new_lines {
        if !line.is_empty() {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("data: ");
    out.push_str(&new_data);
    out.push_str("\n\n");
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream::{self, StreamExt};

    fn run_unwrap(chunks: Vec<&[u8]>) -> String {
        let bytes_chunks: Vec<Result<Bytes, std::io::Error>> = chunks
            .into_iter()
            .map(|c| Ok(Bytes::from(c.to_vec())))
            .collect();
        let input: ByteStream = Box::pin(stream::iter(bytes_chunks));
        let mut out = unwrap_cloud_code_sse_envelope(input);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all = Vec::new();
        runtime.block_on(async {
            while let Some(item) = out.next().await {
                all.extend_from_slice(&item.unwrap());
            }
        });
        String::from_utf8(all).unwrap()
    }

    #[test]
    fn single_event_unwraps_response_field() {
        let input = b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}}\n\n";
        let out = run_unwrap(vec![input]);
        assert!(out.starts_with("data: "));
        assert!(out.contains("candidates"));
        // 不该再有 outer "response" 字段
        assert!(!out.contains("\"response\":"));
        // 必须以 \n\n 结尾
        assert!(out.ends_with("\n\n"));
    }

    #[test]
    fn multiple_events_in_one_chunk_all_unwrapped() {
        let input = b"data: {\"response\":{\"candidates\":[{\"x\":1}]}}\n\ndata: {\"response\":{\"candidates\":[{\"x\":2}]}}\n\n";
        let out = run_unwrap(vec![input]);
        let events: Vec<&str> = out.split("\n\n").filter(|s| !s.is_empty()).collect();
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("\"x\":1"));
        assert!(events[1].contains("\"x\":2"));
        for e in &events {
            assert!(!e.contains("\"response\":"));
        }
    }

    #[test]
    fn event_split_across_chunks_buffered_correctly() {
        // 第一 chunk 半个 JSON,第二 chunk 后半 + 边界
        let chunk1 = b"data: {\"response\":{\"cand";
        let chunk2 = b"idates\":[{\"y\":42}]}}\n\n";
        let out = run_unwrap(vec![chunk1, chunk2]);
        assert!(out.contains("\"y\":42"));
        assert!(!out.contains("\"response\":"));
    }

    #[test]
    fn inline_error_event_emits_synthetic_candidates_with_warning_text() {
        // **Critical** silent-failure 修(完整版,2026-05-11 commit B):Cloud Code 200
        // 流中 `data: {"error":...}` event 转成合法 Gemini wire 形式
        // `{"candidates":[{"content":{"parts":[{"text":"⚠️ Cloud Code error: <msg>"}]}, finishReason:"OTHER"}]}`
        // 让 native 状态机走完整 lifecycle (text delta + text done + completed) — user
        // Codex.app 看到带 ⚠️ 前缀的 assistant 消息含 upstream error.message,而不是
        // 当 keepalive silent 透传 → 空流终止
        let input = b"data: {\"error\":{\"code\":429,\"message\":\"quota exceeded\"}}\n\n";
        let out = run_unwrap(vec![input]);
        // 不再透传原 error JSON
        assert!(
            !out.contains("\"code\":429"),
            "原 error code 不该泄漏到 output(应转换成 user-friendly 形式)"
        );
        // 转成的合法 candidates wire 应包含 error.message 给 user 看到
        assert!(out.contains("\"candidates\""));
        assert!(out.contains("⚠️ Cloud Code error: quota exceeded"));
        assert!(
            out.contains("\"finishReason\":\"SAFETY\""),
            "finishReason=SAFETY 让 native 状态机产 status=incomplete + reason=content_filter,\
             不是 OTHER (会被映射到 stop=completed 误导 client 以为正常结束)"
        );
    }

    /// **Integration test** — pipe `unwrap_cloud_code_sse_envelope` 输出喂给
    /// `gemini_native::response::convert_gemini_to_responses_stream`,验整条链路
    /// (Cloud Code SSE → unwrap → native 状态机 → Responses SSE)。
    ///
    /// 这是 silent-failure-hunter 标的 critical test gap:adapter 层每个 unit
    /// 各自测了,但没测 unwrap + native pipeline 端到端 user-facing event sequence。
    #[test]
    fn integration_inline_error_produces_user_visible_warning_event_sequence() {
        use crate::gemini_native::response::convert_gemini_to_responses_stream;
        use futures_util::StreamExt;

        let cloud_code_sse =
            b"data: {\"error\":{\"code\":429,\"message\":\"quota exceeded mid-stream\"}}\n\n";
        let chunks: Vec<Result<Bytes, std::io::Error>> =
            vec![Ok(Bytes::from(cloud_code_sse.to_vec()))];
        let cloud_code_input: ByteStream = Box::pin(stream::iter(chunks));

        // unwrap layer 转 inline error → 合法 Gemini wire form
        let unwrapped = unwrap_cloud_code_sse_envelope(cloud_code_input);
        // native 状态机消费 unwrapped 流
        let mut responses = convert_gemini_to_responses_stream(unwrapped, None, None);

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all = Vec::new();
        runtime.block_on(async {
            while let Some(item) = responses.next().await {
                all.extend_from_slice(&item.unwrap());
            }
        });
        let out = String::from_utf8(all).unwrap();

        // user 看到的 SSE event 序列 — 必须有 lifecycle + text delta 含 error.message
        assert!(
            out.contains("event: response.created"),
            "lifecycle 必须 emit response.created,实际:\n{out}"
        );
        assert!(
            out.contains("event: response.in_progress"),
            "lifecycle 必须 emit response.in_progress"
        );
        assert!(
            out.contains("event: response.output_text.delta"),
            "user 必须看到 text delta 含 error.message"
        );
        // error.message 必须出现在 user-visible 的 stream 里
        assert!(
            out.contains("quota exceeded mid-stream"),
            "upstream error.message 必须传到 user 端 SSE,实际:\n{out}"
        );
        assert!(
            out.contains("⚠️"),
            "⚠️ 前缀必须出现让 user 知道这是 error 而非 normal output"
        );
        // **必须** terminal 是 incomplete 不是 completed,client 才能区分错误 vs 正常成功
        // (silent-failure-hunter C-1 critical 修:OTHER → completed 让 client retry/telemetry
        // 误判 success;改用 SAFETY → content_filter incomplete)
        assert!(
            out.contains("event: response.incomplete"),
            "stream 必须 terminal=incomplete(不是 completed)让 client 识别 error 终态,实际:\n{out}"
        );
        assert!(
            !out.contains("event: response.completed"),
            "禁止 emit response.completed —— 那会让 client 误判 success,实际:\n{out}"
        );
    }

    #[test]
    fn mixed_response_and_error_prefers_response_branch() {
        // 防御性 lock:同 event 既有 .response 又有 .error 时,我们的实现先 remove .response
        // → 走正常 unwrap 路径(error 分支不触发)。这是 Cloud Code 实测不会出现的形态,
        // 但 lock 当前行为防 future 重构改 branch 顺序导致 error 优先吃掉 response。
        let input =
            b"data: {\"response\":{\"candidates\":[{\"x\":1}]},\"error\":{\"code\":429}}\n\n";
        let out = run_unwrap(vec![input]);
        // .response unwrap 后:输出 {"candidates":[{"x":1}]} — 不含 outer "response" key
        // 也不含 "error"(它是 outer object 字段,跟 response 同级,被 remove("response")
        // 后没保留)
        assert!(out.contains("\"x\":1"));
        assert!(!out.contains("\"response\":"));
    }

    #[test]
    fn pure_keepalive_event_passed_through_unchanged() {
        // 纯 keepalive / 心跳(无 .response 也无 .error)还是原样透传
        let input = b"data: {\"_keepalive\":true,\"timestamp\":1234}\n\n";
        let out = run_unwrap(vec![input]);
        assert!(out.contains("_keepalive"));
        assert!(out.contains("1234"));
    }

    #[test]
    fn malformed_json_data_passed_through() {
        let input = b"data: not-valid-json\n\n";
        let out = run_unwrap(vec![input]);
        assert!(out.contains("not-valid-json"));
    }

    #[test]
    fn crlf_line_endings_recognized() {
        let input = b"data: {\"response\":{\"x\":1}}\r\n\r\n";
        let out = run_unwrap(vec![input]);
        assert!(out.contains("\"x\":1"));
        assert!(!out.contains("\"response\":"));
    }

    #[test]
    fn event_without_trailing_double_newline_at_eof_still_processed() {
        // EOF 边界:最后一个 event 没 \n\n 结尾 — 仍要 process(SSE 实测如此)
        let input = b"data: {\"response\":{\"x\":99}}";
        let out = run_unwrap(vec![input]);
        assert!(out.contains("\"x\":99"));
    }

    #[test]
    fn data_line_without_space_prefix_still_unwrapped() {
        // **pr-test-analyzer H3 修**:SSE 协议允许 `data:<no space>` 形态(罕见但
        // legal)。process_event 已经处理这一支(strip_prefix("data:")),但之前
        // 没测覆盖。本测试 lock 行为防 future regression
        let input = b"data:{\"response\":{\"candidates\":[{\"x\":99}]}}\n\n";
        let out = run_unwrap(vec![input]);
        assert!(out.contains("\"x\":99"));
        assert!(!out.contains("\"response\":"));
    }

    #[test]
    fn empty_stream_produces_empty_output() {
        let out = run_unwrap(vec![]);
        assert!(out.is_empty());
    }

    #[test]
    fn multi_line_event_keeps_event_id_lines() {
        // SSE event 可以有 event:/id: 行,unwrap 不该丢
        let input = b"event: message\ndata: {\"response\":{\"x\":1}}\n\n";
        let out = run_unwrap(vec![input]);
        assert!(out.contains("event: message"));
        assert!(out.contains("\"x\":1"));
    }
}
