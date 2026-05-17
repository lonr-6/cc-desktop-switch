//! Stage 3.3 集成测试 —— 用真实 Kimi (Moonshot) SSE fixture 驱动
//! `convert_chat_to_responses_stream`,确认输出符合 OpenAI Responses SSE 约定。
//!
//! Kimi 这份 fixture 4 帧:
//! 1. role chunk(`delta.role=assistant`,`delta.content=""`)→ 仅 emit
//!    `response.created`,message **懒开**(没有非空内容)
//! 2. **reasoning_content delta**(`delta.reasoning_content="The"`)→ 触发
//!    reasoning 懒开:`output_item.added`(reasoning) +
//!    `reasoning_summary_part.added` + `reasoning_summary_text.delta`
//! 3. finish chunk(`delta={}`, `finish_reason="length"`,`usage` 在
//!    `choices[0].usage` 里)
//! 4. `data: [DONE]` → 关 reasoning(text/part/item.done)+ `response.completed`
//!    (`status=incomplete`,`incomplete_details.reason=max_output_tokens`,
//!    `output=[reasoning_item]`,usage 透传)

use std::path::PathBuf;
use std::pin::Pin;

use bytes::Bytes;
use codex_app_transfer_adapters::convert_chat_to_responses_stream;
use codex_app_transfer_adapters::types::ByteStream;
use futures_core::Stream;
use futures_util::stream::{self, StreamExt};
use serde_json::Value;

fn fixture_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // -> crates/
    p.pop(); // -> repo root
    p.push("tests/replay/fixtures");
    p
}

/// 把 fixture 的所有 frame 拼接成上游 SSE 字节(单个 chunk)。
fn fixture_concat_bytes(name: &str) -> Bytes {
    let path = fixture_root().join(name);
    let json: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let frames = json["upstream"][0]["response"]["stream"]
        .as_array()
        .unwrap();
    let mut buf = Vec::new();
    for f in frames {
        buf.extend_from_slice(f["data"].as_str().unwrap().as_bytes());
    }
    Bytes::from(buf)
}

fn input_stream(bytes: Bytes) -> ByteStream {
    let s: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> =
        Box::pin(stream::iter(vec![Ok(bytes)]));
    s
}

/// 多 chunk 输入,验证状态机的跨 chunk buffer 行为
fn input_stream_chunked(bytes: Bytes, chunk_size: usize) -> ByteStream {
    let mut chunks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let end = (i + chunk_size).min(bytes.len());
        chunks.push(Ok(bytes.slice(i..end)));
        i = end;
    }
    let s: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> =
        Box::pin(stream::iter(chunks));
    s
}

/// 把转换后的字节流收齐,按 `\n\n` 切回 (event_name, data_json) 列表
async fn collect_events(mut s: ByteStream) -> Vec<(String, Value)> {
    let mut buf = Vec::new();
    while let Some(item) = s.next().await {
        let chunk = item.expect("stream item");
        buf.extend_from_slice(&chunk);
    }
    let s = String::from_utf8(buf).expect("utf8");
    let mut out = Vec::new();
    for frame in s.split("\n\n") {
        if frame.trim().is_empty() {
            continue;
        }
        let mut event = String::new();
        let mut data = String::new();
        for line in frame.split('\n') {
            if let Some(v) = line.strip_prefix("event: ") {
                event = v.to_owned();
            } else if let Some(v) = line.strip_prefix("data: ") {
                data = v.to_owned();
            }
        }
        out.push((event, serde_json::from_str(&data).expect("json")));
    }
    out
}

const KIMI_REASONING_LIFECYCLE: &[&str] = &[
    "response.created",
    "response.in_progress", // OpenAI Responses 协议要求 created 后立即跟 in_progress
    "response.output_item.added", // reasoning lazy open
    "response.reasoning_summary_part.added",
    "response.reasoning_summary_text.delta", // open_reasoning 注入的 `**Thinking**\n\n` prefix
    "response.reasoning_summary_text.delta", // 上游真实 reasoning_content delta
    "response.reasoning_summary_text.done",
    "response.reasoning_summary_part.done",
    "response.output_item.done", // reasoning close
    "response.completed",
];

#[tokio::test]
async fn kimi_fixture_emits_reasoning_lifecycle_single_chunk() {
    let bytes = fixture_concat_bytes("kimi_chat_minimal_streaming.json");
    let converted = convert_chat_to_responses_stream(input_stream(bytes));
    let events = collect_events(converted).await;
    let names: Vec<_> = events.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        KIMI_REASONING_LIFECYCLE.to_vec(),
        "Kimi fixture(只有 reasoning 没有 content)应走 reasoning-only 生命周期"
    );

    // reasoning summary 文本 = 注入 prefix + 上游 `delta.reasoning_content="The"`
    let summary_done = events
        .iter()
        .find(|(n, _)| n == "response.reasoning_summary_text.done")
        .unwrap();
    assert_eq!(summary_done.1["text"], "**Thinking**\n\nThe");

    // completed: incomplete + max_output_tokens(finish_reason=length),
    // output 里只有 reasoning item,没有 message item
    let completed = &events.last().unwrap().1["response"];
    assert_eq!(completed["status"], "incomplete");
    assert_eq!(
        completed["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert_eq!(completed["model"], "kimi-k2.6");
    assert_eq!(completed["usage"]["total_tokens"], 10);
    let output = completed["output"].as_array().unwrap();
    assert_eq!(output.len(), 1);
    assert_eq!(output[0]["type"], "reasoning");
    assert_eq!(output[0]["content"], Value::Null);
    assert_eq!(output[0]["encrypted_content"], Value::Null);
    assert_eq!(output[0]["summary"][0]["type"], "summary_text");
    assert_eq!(output[0]["summary"][0]["text"], "**Thinking**\n\nThe");
}

#[tokio::test]
async fn kimi_fixture_handles_chunked_input_buffering() {
    // 把同一份 fixture 切成 11 字节小块输入,状态机应跨 chunk 正确合帧
    let bytes = fixture_concat_bytes("kimi_chat_minimal_streaming.json");
    let converted = convert_chat_to_responses_stream(input_stream_chunked(bytes, 11));
    let events = collect_events(converted).await;
    let names: Vec<_> = events.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, KIMI_REASONING_LIFECYCLE.to_vec());
}

#[tokio::test]
async fn synthetic_two_text_deltas_then_done() {
    let raw = b"data: {\"id\":\"x\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"He\"}}]}\n\n\
                data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"llo\"}}]}\n\n\
                data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
                data: [DONE]\n\n";
    let converted = convert_chat_to_responses_stream(input_stream(Bytes::from_static(raw)));
    let events = collect_events(converted).await;
    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|(n, v)| {
            (n == "response.output_text.delta").then(|| v["delta"].as_str().unwrap())
        })
        .collect();
    assert_eq!(deltas, vec!["He", "llo"]);

    let completed = events
        .iter()
        .rev()
        .find(|(n, _)| n == "response.completed")
        .unwrap();
    assert_eq!(completed.1["response"]["status"], "completed");
    assert_eq!(
        completed.1["response"]["output"][0]["content"][0]["text"],
        "Hello"
    );
}
