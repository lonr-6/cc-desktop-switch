use std::path::PathBuf;
use std::pin::Pin;

use bytes::Bytes;
use codex_app_transfer_adapters::anthropic_messages::request::AnthropicToolNameMaps;
use codex_app_transfer_adapters::anthropic_messages::response::{
    build_anthropic_compact_response_plan, convert_anthropic_messages_to_responses_stream,
};
use codex_app_transfer_adapters::responses::{
    global_response_session_cache, global_tool_call_cache,
};
use codex_app_transfer_adapters::types::{ByteStream, ResponseSessionPlan};
use futures_core::Stream;
use futures_util::stream::{self, StreamExt};
use http::{HeaderMap, StatusCode};
use serde_json::{json, Value};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("anthropic_messages")
}

fn read_fixture(name: &str) -> Bytes {
    Bytes::from(std::fs::read(fixture_root().join(name)).expect("fixture should be readable"))
}

fn input_stream(bytes: Bytes) -> ByteStream {
    let s: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> =
        Box::pin(stream::iter(vec![Ok(bytes)]));
    s
}

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

fn convert_fixture(name: &str) -> ByteStream {
    convert_anthropic_messages_to_responses_stream(
        input_stream(read_fixture(name)),
        None,
        None,
        AnthropicToolNameMaps::default(),
    )
}

#[tokio::test]
async fn text_stream_maps_to_responses_lifecycle() {
    let events = collect_events(convert_fixture("text_stream.sse")).await;
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "response.created",
            "response.in_progress",
            "response.output_item.added",
            "response.content_part.added",
            "response.output_text.delta",
            "response.output_text.delta",
            "response.output_text.done",
            "response.content_part.done",
            "response.output_item.done",
            "response.completed",
        ]
    );

    let deltas: Vec<&str> = events
        .iter()
        .filter_map(|(name, value)| {
            (name == "response.output_text.delta").then(|| value["delta"].as_str().unwrap())
        })
        .collect();
    assert_eq!(deltas, vec!["Hel", "lo"]);

    let completed = &events.last().unwrap().1["response"];
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["model"], "claude-3-5-sonnet-20241022");
    assert_eq!(completed["output"][0]["content"][0]["text"], "Hello");
    assert_eq!(completed["usage"]["input_tokens"], 12);
    assert_eq!(completed["usage"]["output_tokens"], 2);
    assert_eq!(completed["usage"]["total_tokens"], 14);
}

#[tokio::test]
async fn thinking_stream_maps_to_reasoning_summary() {
    let events = collect_events(convert_fixture("thinking_stream.sse")).await;
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"response.reasoning_summary_part.added"));
    assert!(names.contains(&"response.reasoning_summary_text.delta"));
    assert!(names.contains(&"response.reasoning_summary_text.done"));

    let done = events
        .iter()
        .find(|(name, _)| name == "response.reasoning_summary_text.done")
        .unwrap();
    assert_eq!(done.1["text"], "I need to inspect the request shape.");

    let completed = &events.last().unwrap().1["response"];
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["output"][0]["type"], "reasoning");
    assert_eq!(
        completed["output"][0]["summary"][0]["text"],
        "I need to inspect the request shape."
    );
}

#[tokio::test]
async fn tool_use_stream_maps_function_call_and_saves_cache() {
    let events = collect_events(convert_fixture("tool_use_stream.sse")).await;
    let completed = &events.last().unwrap().1["response"];
    let item = &completed["output"][0];
    assert_eq!(item["type"], "function_call");
    assert_eq!(item["call_id"], "toolu_01");
    assert_eq!(item["name"], "read_file");
    assert_eq!(item["arguments"], "{\"path\":\"Cargo.toml\"}");

    let cached = global_tool_call_cache()
        .get("toolu_01")
        .expect("tool call should be cached for next-turn tool_result repair");
    assert_eq!(cached.name, "read_file");
    assert_eq!(cached.arguments, "{\"path\":\"Cargo.toml\"}");
}

#[tokio::test]
async fn tool_use_stream_restores_sanitized_tool_name() {
    let maps = AnthropicToolNameMaps {
        forward: [("fs.read file".to_owned(), "fs_read_file".to_owned())].into(),
        reverse: [("fs_read_file".to_owned(), "fs.read file".to_owned())].into(),
    };
    let raw = Bytes::from_static(
        br#"event: message_start
data: {"type":"message_start","message":{"model":"claude-test","usage":{"input_tokens":1,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_sanitized","name":"fs_read_file","input":{}}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":2}}

event: message_stop
data: {"type":"message_stop"}

"#,
    );
    let events = collect_events(convert_anthropic_messages_to_responses_stream(
        input_stream(raw),
        None,
        None,
        maps,
    ))
    .await;
    let completed = &events.last().unwrap().1["response"];
    assert_eq!(completed["output"][0]["name"], "fs.read file");
}

#[tokio::test]
async fn error_event_maps_to_response_failed() {
    let events = collect_events(convert_fixture("error_stream.sse")).await;
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "response.created",
            "response.in_progress",
            "response.failed"
        ]
    );
    let failed = &events.last().unwrap().1["response"];
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["error"]["code"], "overloaded_error");
    assert_eq!(failed["error"]["message"], "Overloaded");
}

#[tokio::test]
async fn unknown_events_are_ignored() {
    let events = collect_events(convert_fixture("unknown_event_stream.sse")).await;
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert!(!names.contains(&"anthropic_future_event"));
    assert_eq!(events.last().unwrap().0, "response.completed");
    assert_eq!(
        events.last().unwrap().1["response"]["output"][0]["content"][0]["text"],
        "ok"
    );
}

#[tokio::test]
async fn max_tokens_stop_reason_emits_incomplete() {
    let raw = Bytes::from_static(
        br#"event: message_start
data: {"type":"message_start","message":{"model":"claude-test","usage":{"input_tokens":1,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: message_delta
data: {"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":8}}

event: message_stop
data: {"type":"message_stop"}

"#,
    );
    let events = collect_events(convert_anthropic_messages_to_responses_stream(
        input_stream(raw),
        None,
        None,
        AnthropicToolNameMaps::default(),
    ))
    .await;
    assert_eq!(events.last().unwrap().0, "response.incomplete");
    assert_eq!(
        events.last().unwrap().1["response"]["incomplete_details"]["reason"],
        "max_output_tokens"
    );
}

#[tokio::test]
async fn stream_interruption_emits_incomplete_not_completed() {
    let raw = Bytes::from_static(
        br#"event: message_start
data: {"type":"message_start","message":{"model":"claude-test","usage":{"input_tokens":1,"output_tokens":1}}}

event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}

"#,
    );
    let events = collect_events(convert_anthropic_messages_to_responses_stream(
        input_stream_chunked(raw, 7),
        None,
        None,
        AnthropicToolNameMaps::default(),
    ))
    .await;
    let names: Vec<_> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"response.incomplete"));
    assert!(!names.contains(&"response.completed"));
    assert_eq!(
        events.last().unwrap().1["response"]["incomplete_details"]["reason"],
        "interrupted"
    );
}

#[tokio::test]
async fn stream_completion_saves_response_session() {
    global_response_session_cache().clear();
    let session = ResponseSessionPlan {
        response_id: "resp_anthropic_session_test".to_owned(),
        messages: vec![json!({"role": "user", "content": "hello"})],
    };
    let converted = convert_anthropic_messages_to_responses_stream(
        input_stream(read_fixture("text_stream.sse")),
        Some(session),
        Some(json!({"model": "claude-test"})),
        AnthropicToolNameMaps::default(),
    );
    let _ = collect_events(converted).await;

    let saved = global_response_session_cache()
        .get("resp_anthropic_session_test")
        .expect("response session should be saved");
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0]["role"], "user");
    assert_eq!(saved[1]["role"], "assistant");
    assert_eq!(saved[1]["content"], "Hello");
}

#[tokio::test]
async fn compact_response_extracts_anthropic_content_text() {
    let upstream = json!({
        "id": "msg_compact",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "text",
            "text": "<analysis>hidden</analysis><summary>Keep this context.</summary>",
        }],
    });
    let plan = build_anthropic_compact_response_plan(
        StatusCode::OK,
        HeaderMap::new(),
        input_stream(Bytes::from(serde_json::to_vec(&upstream).unwrap())),
    )
    .unwrap();
    let mut body = Vec::new();
    let mut stream = plan.stream;
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk.unwrap());
    }
    let parsed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(parsed["output"][0]["type"], "compaction");
    let encrypted = parsed["output"][0]["encrypted_content"].as_str().unwrap();
    assert!(encrypted.ends_with("Keep this context."));
    assert!(!encrypted.contains("hidden"));
}
