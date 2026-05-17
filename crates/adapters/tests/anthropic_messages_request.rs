use std::path::PathBuf;

use bytes::Bytes;
use codex_app_transfer_adapters::anthropic_messages::request::{
    anthropic_messages_default_headers, build_anthropic_messages_upstream_path,
    prepare_anthropic_messages_request, responses_body_to_anthropic_messages_request,
};
use codex_app_transfer_registry::Provider;
use indexmap::IndexMap;
use serde_json::Value;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("anthropic_messages")
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture_root().join(name)).expect("fixture should be readable")
}

fn read_json_fixture(name: &str) -> Value {
    serde_json::from_str(&read_fixture(name)).expect("fixture should be valid json")
}

fn dummy_provider() -> Provider {
    Provider {
        id: "anthropic-local".into(),
        name: "Anthropic Local".into(),
        base_url: "https://api.anthropic.com/v1".into(),
        auth_scheme: "bearer".into(),
        api_format: "anthropic_messages".into(),
        api_key: "sk-test".into(),
        models: IndexMap::new(),
        extra_headers: IndexMap::new(),
        model_capabilities: IndexMap::new(),
        request_options: IndexMap::new(),
        is_builtin: false,
        sort_index: 0,
        extra: IndexMap::new(),
    }
}

#[derive(Debug)]
struct SseFrame {
    event: String,
    data: Value,
}

fn parse_sse_fixture(name: &str) -> Vec<SseFrame> {
    read_fixture(name)
        .split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .map(|frame| {
            let mut event = None;
            let mut data = None;
            for line in frame.lines() {
                if let Some(value) = line.strip_prefix("event: ") {
                    event = Some(value.to_owned());
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data = Some(value.to_owned());
                }
            }
            SseFrame {
                event: event.expect("SSE frame should include event"),
                data: serde_json::from_str(&data.expect("SSE frame should include data"))
                    .expect("SSE data should be valid json"),
            }
        })
        .collect()
}

#[test]
fn p2_anthropic_messages_sse_fixtures_are_valid() {
    let cases = [
        (
            "text_stream.sse",
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ],
        ),
        (
            "thinking_stream.sse",
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ],
        ),
        (
            "tool_use_stream.sse",
            vec![
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ],
        ),
        ("error_stream.sse", vec!["error"]),
        (
            "unknown_event_stream.sse",
            vec![
                "message_start",
                "anthropic_future_event",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop",
            ],
        ),
    ];

    for (fixture, expected_events) in cases {
        let frames = parse_sse_fixture(fixture);
        let events: Vec<_> = frames.iter().map(|frame| frame.event.as_str()).collect();
        assert_eq!(
            events, expected_events,
            "unexpected event order in {fixture}"
        );
        for frame in frames {
            assert_eq!(
                frame.data["type"].as_str(),
                Some(frame.event.as_str()),
                "fixture {fixture} should keep event name and data.type aligned"
            );
        }
    }
}

#[test]
fn p2_request_mapper_json_fixtures_are_valid() {
    let cases = [
        ("request_text.responses.json", "request_text.anthropic.json"),
        (
            "request_tool_result.responses.json",
            "request_tool_result.anthropic.json",
        ),
    ];

    for (input_name, expected_name) in cases {
        let input = read_json_fixture(input_name);
        let expected = read_json_fixture(expected_name);
        assert!(
            input.get("input").is_some(),
            "{input_name} should model Responses input"
        );
        assert!(
            expected.get("messages").is_some(),
            "{expected_name} should model Anthropic Messages output"
        );
        assert!(
            expected.get("max_tokens").is_some(),
            "{expected_name} should include Anthropic required max_tokens"
        );
    }
}

#[test]
fn responses_text_request_lowers_to_anthropic_messages() {
    let input = read_json_fixture("request_text.responses.json");
    let expected = read_json_fixture("request_text.anthropic.json");

    let actual = responses_body_to_anthropic_messages_request(&input, &dummy_provider())
        .expect("request conversion should succeed")
        .request;

    assert_eq!(actual, expected);
}

#[test]
fn responses_tool_result_request_lowers_to_anthropic_messages() {
    let input = read_json_fixture("request_tool_result.responses.json");
    let expected = read_json_fixture("request_tool_result.anthropic.json");

    let actual = responses_body_to_anthropic_messages_request(&input, &dummy_provider())
        .expect("request conversion should succeed")
        .request;

    assert_eq!(actual, expected);
}

#[test]
fn request_mapper_sanitizes_tool_names_and_rewrites_tool_choice() {
    let input = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "input": [
            {
                "type": "function_call",
                "call_id": "call_1",
                "name": "fs.read file",
                "arguments": "{\"path\":\"Cargo.toml\"}"
            }
        ],
        "tools": [
            {
                "type": "function",
                "name": "fs.read file",
                "description": "Read",
                "parameters": {"type":"object"}
            }
        ],
        "tool_choice": {"type":"function", "function": {"name": "fs.read file"}},
        "parallel_tool_calls": false,
        "max_output_tokens": 128,
        "stream": true
    });

    let conversion = responses_body_to_anthropic_messages_request(&input, &dummy_provider())
        .expect("request conversion should succeed");

    assert_eq!(
        conversion.request["tools"][0]["name"],
        Value::String("fs_read_file".into())
    );
    assert_eq!(
        conversion.request["messages"][0]["content"][0]["name"],
        Value::String("fs_read_file".into())
    );
    assert_eq!(
        conversion.request["tool_choice"],
        serde_json::json!({
            "type": "tool",
            "name": "fs_read_file",
            "disable_parallel_tool_use": true
        })
    );
    assert_eq!(
        conversion
            .tool_name_maps
            .reverse
            .get("fs_read_file")
            .map(String::as_str),
        Some("fs.read file")
    );
}

#[test]
fn valid_underscore_tool_name_is_not_rewritten() {
    let input = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "input": "hi",
        "tools": [
            {
                "type": "function",
                "name": "_valid-tool",
                "parameters": {"type":"object"}
            }
        ],
        "max_output_tokens": 128
    });

    let conversion = responses_body_to_anthropic_messages_request(&input, &dummy_provider())
        .expect("request conversion should succeed");

    assert_eq!(
        conversion.request["tools"][0]["name"],
        Value::String("_valid-tool".into())
    );
    assert!(conversion.tool_name_maps.forward.is_empty());
    assert!(conversion.tool_name_maps.reverse.is_empty());
}

#[test]
fn request_mapper_filters_email_user_metadata_and_maps_reasoning() {
    let input = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "input": "hi",
        "user": "person@example.com",
        "reasoning": {"effort":"high"}
    });

    let request = responses_body_to_anthropic_messages_request(&input, &dummy_provider())
        .expect("request conversion should succeed")
        .request;

    assert!(request.get("metadata").is_none());
    assert_eq!(
        request["thinking"],
        serde_json::json!({"type":"enabled","budget_tokens":8192})
    );
    assert_eq!(request["max_tokens"], Value::Number(4096.into()));
}

#[test]
fn prepare_request_exposes_path_body_and_anthropic_headers() {
    let input = read_json_fixture("request_text.responses.json");
    let body = Bytes::from(serde_json::to_vec(&input).unwrap());
    let prepared = prepare_anthropic_messages_request("/v1/responses", body, &dummy_provider())
        .expect("prepare should succeed");

    assert_eq!(prepared.upstream_path, "/messages");
    assert_eq!(
        prepared
            .headers
            .get("anthropic-version")
            .and_then(|v| v.to_str().ok()),
        Some("2023-06-01")
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&prepared.body).unwrap(),
        read_json_fixture("request_text.anthropic.json")
    );
    assert!(prepared.response_session.is_some());
    assert!(!prepared.is_compact);
    assert!(prepared.original_responses_request.is_some());
}

#[test]
fn compact_prepare_uses_non_streaming_messages_request() {
    let input = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "input": "summarize this conversation"
    });
    let body = Bytes::from(serde_json::to_vec(&input).unwrap());
    let prepared =
        prepare_anthropic_messages_request("/responses/compact", body, &dummy_provider())
            .expect("compact prepare should succeed");
    let request: Value = serde_json::from_slice(&prepared.body).unwrap();

    assert_eq!(prepared.upstream_path, "/messages");
    assert!(prepared.is_compact);
    assert!(prepared.response_session.is_none());
    assert!(prepared.original_responses_request.is_none());
    assert_eq!(request["stream"], Value::Bool(false));
    assert_eq!(request["max_tokens"], Value::Number(20_000.into()));
    assert!(request["messages"]
        .as_array()
        .is_some_and(|m| !m.is_empty()));
}

#[test]
fn orphan_tool_result_returns_diagnostic_bad_request() {
    let input = serde_json::json!({
        "model": "claude-3-5-sonnet-20241022",
        "input": [
            {
                "type": "function_call_output",
                "call_id": "missing_call",
                "output": "orphan"
            }
        ],
        "max_output_tokens": 128
    });

    let err = responses_body_to_anthropic_messages_request(&input, &dummy_provider())
        .expect_err("orphan tool output should not be silently converted");

    assert!(
        err.to_string().contains("tool_call missing function.name")
            || err
                .to_string()
                .contains("tool result references unknown tool_use")
    );
}

#[test]
fn base_url_path_helper_handles_v1_prefixes() {
    assert_eq!(
        build_anthropic_messages_upstream_path("https://api.anthropic.com"),
        "/v1/messages"
    );
    assert_eq!(
        build_anthropic_messages_upstream_path("https://api.anthropic.com/v1"),
        "/messages"
    );
    assert_eq!(
        build_anthropic_messages_upstream_path("https://proxy.example/anthropic/v1/"),
        "/messages"
    );
}

#[test]
fn default_headers_include_anthropic_contract_values() {
    let headers = anthropic_messages_default_headers();
    assert_eq!(
        headers
            .get("anthropic-version")
            .and_then(|v| v.to_str().ok()),
        Some("2023-06-01")
    );
    assert_eq!(
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
}
