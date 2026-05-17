use std::collections::HashMap;

use serde_json::{json, Value};

/// 扫 `original_request.tools` 里 `type:"namespace"` 包装,建立
/// `function.name -> namespace.name` 反查表。
///
/// 供 Responses / GeminiNative 两条 Responses SSE 转换链路共享,避免同一套
/// 扫描规则在多个 converter 中重复维护。
pub(crate) fn build_tool_namespace_map(
    original_request: Option<&Value>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Some(req) = original_request else {
        return map;
    };
    let Some(tools) = req.get("tools").and_then(|v| v.as_array()) else {
        return map;
    };

    for tool in tools {
        let Some(obj) = tool.as_object() else {
            continue;
        };
        if obj.get("type").and_then(|v| v.as_str()) != Some("namespace") {
            continue;
        }
        let Some(ns_name) = obj.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(inner_tools) = obj.get("tools").and_then(|v| v.as_array()) else {
            continue;
        };
        for inner in inner_tools {
            let Some(inner_obj) = inner.as_object() else {
                continue;
            };
            if inner_obj.get("type").and_then(|v| v.as_str()) != Some("function") {
                continue;
            }
            if let Some(fname) = inner_obj.get("name").and_then(|v| v.as_str()) {
                // 后写覆盖前写(罕见同名跨 namespace 情况)
                map.insert(fname.to_owned(), ns_name.to_owned());
            }
        }
    }
    map
}

/// 写一帧标准 Responses SSE event:
/// `event: <name>\ndata: <json>\n\n`。
///
/// 该 helper 统一维护 `sequence_number` 注入逻辑,并在 payload 序列化失败时
/// 保留 fallback `{}` + error 日志,防止静默丢失。
pub(crate) fn emit_sse_event(
    out: &mut Vec<u8>,
    seq: &mut u64,
    event_name: &str,
    mut payload: Value,
) {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("sequence_number".into(), json!(*seq));
    }
    *seq += 1;
    let serialized = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                error = %e,
                event = event_name,
                "BUG: failed to serialize Responses SSE event payload; falling back to empty object"
            );
            "{}".into()
        }
    };
    let line = format!("event: {event_name}\ndata: {serialized}\n\n");
    out.extend_from_slice(line.as_bytes());
}
