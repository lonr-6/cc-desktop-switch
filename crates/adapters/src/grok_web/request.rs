//! Codex OpenAI Responses API → grok.com chat payload 转换。
//!
//! ## 输入(Codex Responses API)
//!
//! Codex APP 走 `/v1/responses`,body 形如:
//!
//! ```json
//! {
//!   "model": "gpt-5-codex",            // 槽位名,本 adapter 映射到 grok 后端模型
//!   "input": [{ "type": "message", "role": "user", "content": "..." }, ...],
//!   "tools": [...],
//!   "stream": true,
//!   "previous_response_id": "resp_abc",  // 多轮 anchor
//!   "reasoning": { "effort": "high" }
//! }
//! ```
//!
//! ## 输出([`super::types::GrokChatRequest`])
//!
//! grok.com 后端只接受单 `message` string + `parent_response_id` DAG。
//! 多轮 message 历史**用 role prefix flatten** 到 single message string(`User: ...\n\nAssistant: ...`),
//! grok 模型从 prefix 自己理解上下文。同时 [`super::parent_response::ParentResponseTracker`]
//! 命中时传 `parent_response_id` 给 grok.com,后端 DAG 也自己拉 — **双保险**。
//!
//! ## 当前实现(2026-05-12 task 18 重构后,对齐 ARCHITECTURE_PROTOCOL_GUIDE Phase 4)
//!
//! - ✅ 模型映射:Codex 槽位 → `provider.models[slot]` → grok `modeId`
//! - ✅ **多轮上下文** + **autocompact 展开**:走 `core::input` +
//!   `responses/compact.rs` + `global_response_session_cache()`(L1 LRU + L2
//!   SQLite 持久化 `~/.codex-app-transfer/sessions.db`,30 天 TTL,**`.app`
//!   重启不丢历史**)
//! - ✅ `previous_response_id` 反查 tracker → `parent_response_id`(双保险)
//! - ✅ `reasoning.effort` → `is_reasoning` flag
//! - ✅ `disable_search` 默认 false(grok 内置 web search 开启)
//! - ⚠️ tools / web_search / MCP namespace 字段:**忽略**(server-side state 自动注入)
//!
//! 主入口:[`responses_body_to_grok_request_with_session`](生产路径,mapper 用)。
//! 兼容入口:[`responses_body_to_grok_request`] / [`responses_body_to_grok_request_with_tracker`]
//! (test fixture,**无 session cache → 不走 core → 丢历史**,prod 别用)。
//!
//! 协议事实详见本 module 各 type 的 doc comment(`super::types`、`super::parent_response`),
//! 以及 `crates/adapters/src/grok_web/mod.rs` 顶层文档。

use bytes::Bytes;
use codex_app_transfer_registry::Provider;
use serde_json::Value;

use crate::core::input::response_id_for_session;
use crate::grok_web::parent_response::{global_tracker, ParentResponseTracker};
use crate::grok_web::types::GrokChatRequest;
use crate::responses::session::ResponseSessionCache;
use crate::types::{AdapterError, ResponseSessionPlan};

/// grok.com chat endpoint path(相对 `provider.base_url`)。
pub const GROK_CHAT_PATH: &str = "/rest/app-chat/conversations/new";

/// `responses_body_to_grok_request_with_session` 的输出 — 同时返回 grok request
/// payload 跟 `ResponseSessionPlan`(供 mapper 注入 `RequestPlan.response_session`,
/// 让流末 `responses/converter` 把本轮 user+assistant messages append 进 cache,
/// 下轮 `previous_response_id` 命中时自动拉历史)。
#[derive(Debug, Clone)]
pub struct GrokRequestConversion {
    pub request: GrokChatRequest,
    pub response_session: ResponseSessionPlan,
}

/// **生产主入口**(对齐 ARCHITECTURE_PROTOCOL_GUIDE Phase 4):接 `ResponseSessionCache`
/// (sqlite 持久化 `~/.codex-app-transfer/sessions.db`),走 `core::input` 共性
/// 历史拼接 + `responses/compact.rs` 三种 compaction variant 自动展开。
///
/// **task 22 / I7 修(2026-05-12)**:`session_cache` 改**必填**(`&ResponseSessionCache`,
/// 非 `Option`)。原 `Option` 设计是 foot-gun:任何 caller(typo、误改、误删)传
/// `None` 进来 prod 仍会 silently 走 input-only 降级路径,丢历史 + compaction +
/// function_call_output。新签名让"无 cache prod 路径"在类型系统层面变成 impossible
/// state。本 fn 虽 `pub`,但当前唯一 caller 是同 crate 的
/// [`crate::mapper::grok_web::prepare_grok_web_request`](已同步改);外部 crate
/// 无引用,签名 break 实际影响为零。Test fixture 路径走
/// [`responses_body_to_grok_request`] / [`responses_body_to_grok_request_with_tracker`]
/// (`pub(crate)` 物理隔离,跨 crate 不可见)。
///
/// 双保险(grok.com 后端契约层面):
/// 1. **client 端历史拼接**(本 fn 主路径):把 merged messages flatten 成 grok
///    single message string,角色 prefix(`User: ...\n\nAssistant: ...`),grok
///    模型自己理解 context。即便 grok.com 服务端 DAG miss 也能 work。
/// 2. **grok 服务端 DAG 锚定**(`ParentResponseTracker`):tracker 命中时传
///    `parent_response_id` 给 grok.com,后端 ALSO 自己拉历史。两路任一 work
///    都行 — 双保险。
pub fn responses_body_to_grok_request_with_session(
    body: &Value,
    provider: &Provider,
    session_cache: &ResponseSessionCache,
) -> Result<GrokRequestConversion, AdapterError> {
    let codex_model = body.get("model").and_then(Value::as_str).ok_or_else(|| {
        AdapterError::BadRequest(format!(
            "missing `model` field in Codex Responses body (provider.id={})",
            provider.id
        ))
    })?;
    let mode_id = resolve_mode_id(codex_model, provider)?;

    // 走 core/responses 共性历史拼接:
    // - `responses_body_to_chat_body_for_provider_with_session` 内部调
    //   `build_messages_from_input` → 处理 previous_response_id 历史合并 +
    //   `type=compaction` 三 variant 自动展开成 user message + tool_call_repair。
    // - cache miss + input 空 → `PreviousResponseNotFound`(forward 转标准 400)
    // - cache miss + input 非空 → 降级仅本轮(已含 codex 当前输入展开)
    //
    // 走 core 路径时 instructions 已合并进 messages[0]=system,本 fn 不再
    // 单独 emit `custom_instructions`(留 None,跟原 cache 路径行为一致)。
    let conversion = crate::responses::responses_body_to_chat_body_for_provider_with_session(
        body,
        None,
        Some(session_cache),
    )?;
    let merged_messages = conversion.response_session.messages;
    let message = flatten_messages_to_grok_single_string(&merged_messages);

    let parent_response_id = resolve_parent_response_id(body, Some(global_tracker()));
    let is_reasoning = parse_reasoning_flag(body);

    Ok(GrokRequestConversion {
        request: GrokChatRequest {
            message,
            mode_id,
            parent_response_id,
            is_reasoning,
            custom_instructions: None,
            ..GrokChatRequest::default()
        },
        response_session: ResponseSessionPlan {
            response_id: response_id_for_session(),
            messages: merged_messages,
        },
    })
}

/// **测试 fixture / 兼容入口**(`pub(crate)` 物理隔离 prod 误用):Codex Responses
/// body → grok payload,**不走 core**,会丢历史 message + compaction +
/// function_call_output + reasoning 等非 `type=message` 输入项。
///
/// 此路径**只在 unit test 用**作为 cache-free 简化 harness;prod 必须走
/// [`responses_body_to_grok_request_with_session`]。
///
/// `instructions` 字段:本 fn 单独抽出来塞进 `custom_instructions` 字段(走
/// grok wire `customInstructions`)— core 路径在 messages[0]=system 里处理,
/// 本 fn 不走 core,messages 不含 system,故必须独立 surface。
pub(crate) fn responses_body_to_grok_request(
    body: &Value,
    provider: &Provider,
) -> Result<GrokChatRequest, AdapterError> {
    responses_body_to_grok_request_with_tracker(body, provider, Some(global_tracker()))
}

/// **测试 fixture**(允许注入 tracker)。`pub(crate)` 物理隔离 prod 误用。
pub(crate) fn responses_body_to_grok_request_with_tracker(
    body: &Value,
    provider: &Provider,
    tracker: Option<&ParentResponseTracker>,
) -> Result<GrokChatRequest, AdapterError> {
    let codex_model = body.get("model").and_then(Value::as_str).ok_or_else(|| {
        AdapterError::BadRequest(format!(
            "missing `model` field in Codex Responses body (provider.id={})",
            provider.id
        ))
    })?;
    let mode_id = resolve_mode_id(codex_model, provider)?;

    let messages = extract_messages_from_input_only(body)?;
    let message = flatten_messages_to_grok_single_string(&messages);
    let custom_instructions = body
        .get("instructions")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    let parent_response_id = resolve_parent_response_id(body, tracker);
    let is_reasoning = parse_reasoning_flag(body);

    Ok(GrokChatRequest {
        message,
        mode_id,
        parent_response_id,
        is_reasoning,
        custom_instructions,
        ..GrokChatRequest::default()
    })
}

fn resolve_parent_response_id(
    body: &Value,
    tracker: Option<&ParentResponseTracker>,
) -> Option<String> {
    let prev = body
        .get("previous_response_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    let Some(t) = tracker else {
        // tracker None 是 test fixture 才有的状态(prod 永远传 Some(global_tracker())),
        // 不算 silent failure,但 debug 留信号便于 grep。
        tracing::debug!(
            error_id = "GROK_WEB_PARENT_LOOKUP_NO_TRACKER",
            previous_response_id = %prev,
            "previous_response_id 存在但 tracker=None(test fixture path),grok wire 不带 parent_response_id"
        );
        return None;
    };
    let codex_id = crate::grok_web::parent_response::CodexResponseId::from(prev);
    match t.get(&codex_id) {
        Some(g) => Some(g.into_inner()),
        None => {
            // **silent-failure MED-2 修**:用户认为是续轮但 tracker miss
            // (上轮没记 mapping / .app 重启 tracker 丢 / response_id 不对),
            // grok wire 不带 parent_response_id → grok 服务端起新对话。
            // client 端 flatten 历史通常可弥补,但 cache miss + tracker miss
            // 时上下文是真断掉的。debug 让 operator 能 grep 区分"主动无 prev id"
            // 跟"prev id 给了但 tracker resolve 失败"两种 case。
            tracing::debug!(
                error_id = "GROK_WEB_PARENT_LOOKUP_MISS",
                previous_response_id = %prev,
                "previous_response_id 在 tracker 里未命中 mapping,grok wire 不带 parent_response_id"
            );
            None
        }
    }
}

/// 把 chat-shape messages 数组 flatten 成 grok 协议的 single message string。
///
/// grok.com wire 只接受 single `message: string`,不接受 messages 数组。
/// 我们把多轮 history 用角色 prefix 拼接成 single string,grok 模型自己理解上下文。
///
/// 拼接规则(参考 chenyme/grok2api 类似 pattern):
/// - `system` / `developer` → `"System: <content>"`
/// - `user` → `"User: <content>"`
/// - `assistant` → `"Assistant: <content>"` 或 `"Assistant (tool call): name({args})"`
///   (task 20 / code-reviewer H3 修:**Codex CLI 多轮 tool use 时 assistant
///   message 是 `{role:"assistant", content:"", tool_calls:[...]}`** —— 空 content
///   被原 `if content.is_empty() continue` 整段 drop,导致 grok 看不到 cause-effect
///   chain — 后续 tool_call_output 出现就 orphan。新行为:tool_calls 非空时,渲染
///   `Assistant (tool call): <name>(<args>)` 行,**每个 call 独立一段**)
/// - `tool` / `function` → `"Tool Result: <content>"`(grok 自己 server-side state
///   处理 tool,塞进文本作 context;不映射到 function_call/function_call_output schema)
/// - 各 turn 用 `\n\n` 分隔
///
/// content 兼容 string / array of {type:text|input_text|output_text, text:"..."}。
fn flatten_messages_to_grok_single_string(messages: &[Value]) -> String {
    let mut out = String::with_capacity(messages.len() * 64);
    for msg in messages {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        let content_str = extract_message_content_text(msg.get("content"));
        // task 20 / code-reviewer H3:assistant message 的 tool_calls 字段
        // (Codex CLI 多轮 tool use 期间常见的 `{role:assistant, content:"",
        // tool_calls:[...]}` 形态)在旧逻辑里被 content_str.is_empty() drop,
        // grok 看不到 tool call 事实 — 用 function_call_output 续轮时 grok 觉得
        // 是凭空冒出的 Tool Result。本段把 tool_calls 渲染成独立 prefix 段。
        //
        // **role gate**(silent-failure H3/H4 + code-reviewer Important #1 修):
        // 只对 `role=="assistant"` 渲染 tool_calls。non-assistant role 含
        // tool_calls 是协议违反,warn 后 skip,**不再用 `Assistant (tool call):`
        // 误标其他 role 的 tool call**(那是 fabricate assistant turn,worse than
        // silent drop)。
        let tool_calls_rendered = if role == "assistant" {
            render_assistant_tool_calls(msg)
        } else {
            warn_if_tool_calls_on_non_assistant_role(msg, role);
            String::new()
        };
        if content_str.is_empty() && tool_calls_rendered.is_empty() {
            continue;
        }
        let prefix = match role {
            "system" | "developer" => "System",
            "user" => "User",
            "assistant" => "Assistant",
            "tool" | "function" => "Tool Result",
            other if !other.is_empty() => other, // 防御未知角色:原样保留 prefix
            _ => continue,                       // 无 role 丢弃
        };
        if !content_str.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(prefix);
            out.push_str(": ");
            out.push_str(&content_str);
        }
        if !tool_calls_rendered.is_empty() {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&tool_calls_rendered);
        }
    }
    out
}

/// non-assistant role 含 tool_calls(协议违反)warn 一次。
fn warn_if_tool_calls_on_non_assistant_role(msg: &Value, role: &str) {
    let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) else {
        return;
    };
    if calls.is_empty() {
        return;
    }
    tracing::warn!(
        error_id = "GROK_WEB_FLATTEN_TOOL_CALLS_ON_NON_ASSISTANT_ROLE",
        role,
        count = calls.len(),
        "non-assistant message 含 tool_calls 数组(协议违反),drop 这些 tool_calls 不渲染"
    );
}

/// 渲染 assistant message 的 `tool_calls` 数组成 grok-readable prefix 段。
///
/// 输入形态(OpenAI chat completions spec):
/// ```json
/// {"role":"assistant", "content":"", "tool_calls":[
///   {"id":"call_abc","type":"function","function":{"name":"web_search","arguments":"{\"q\":\"x\"}"}}
/// ]}
/// ```
///
/// 输出形态:
/// ```text
/// Assistant (tool call): web_search({"q":"x"})
/// ```
///
/// 多个 tool_calls 各占一行(用 `\n` 分隔同一 turn 内多个 call):
/// ```text
/// Assistant (tool call): web_search({"q":"x"})
/// Assistant (tool call): read_file({"path":"a.rs"})
/// ```
///
/// 边界处理(silent-failure-hunter + code-reviewer + type-design 共识修复):
/// - `tool_calls` 缺失 / null → 返回空字符串
/// - `tool_calls` 存在但**非 array**(协议违反)→ warn + 返回空字符串
/// - `tool_calls` 空 array → 返回空字符串
/// - 单个 call 不是 object → warn + skip 该 call
/// - tool_call 缺 `function.name` → 该条记为 `<unknown>` 名字(**HIGH-1**:整批
///   nameless 时也要保留占位行,否则后续 `tool` role 续轮失去 anchor 出现 orphan
///   Tool Result),warn 一次。
/// - `function.arguments` 缺失 / null / 空字符串 → 当 empty object `{}`(标准约定)
/// - `function.arguments` **是 object / array**(非 OpenAI 标准但 chenyme/grok2api
///   类历史输入见过)→ 用 `serde_json::to_string` 序列化(**HIGH-2 修**:不再
///   silent `{}`)
/// - args 内的换行折叠为空格(防止 `Assistant (tool call): ... \n ...` 误读为
///   下一段 prefix)
/// - 调用方负责 role gate(只对 `role=="assistant"` 调本 fn);non-assistant role
///   含 tool_calls 由 caller 的 `warn_if_tool_calls_on_non_assistant_role` 处理
fn render_assistant_tool_calls(msg: &Value) -> String {
    let Some(raw_tool_calls) = msg.get("tool_calls") else {
        return String::new();
    };
    if raw_tool_calls.is_null() {
        return String::new();
    }
    let Some(calls) = raw_tool_calls.as_array() else {
        tracing::warn!(
            error_id = "GROK_WEB_FLATTEN_TOOL_CALLS_NOT_ARRAY",
            kind = json_value_kind(raw_tool_calls),
            "assistant message 的 tool_calls 字段不是 array(协议违反),drop"
        );
        return String::new();
    };
    if calls.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::with_capacity(calls.len());
    for call in calls {
        if !call.is_object() {
            tracing::warn!(
                error_id = "GROK_WEB_FLATTEN_TOOL_CALL_NOT_OBJECT",
                kind = json_value_kind(call),
                "tool_calls 数组元素不是 object(协议违反),skip 该条"
            );
            continue;
        }
        let function = call.get("function");
        let name = function
            .and_then(|f| f.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| {
                tracing::warn!(
                    error_id = "GROK_WEB_FLATTEN_TOOL_CALL_NO_NAME",
                    "tool_call 缺 function.name,用 <unknown> 占位保留 anchor 防 orphan Tool Result"
                );
                "<unknown>".to_owned()
            });
        let args_owned = render_tool_call_arguments(function.and_then(|f| f.get("arguments")));
        let args_one_line = if args_owned.contains('\n') {
            args_owned.replace('\n', " ")
        } else {
            args_owned
        };
        lines.push(format!("Assistant (tool call): {name}({args_one_line})"));
    }
    lines.join("\n")
}

/// arguments 字段渲染。
///
/// OpenAI 协议规定 `function.arguments` 是 **string**(已 JSON 序列化的 args),
/// 但实际有不少上游 / mock / 历史代码直接发 object/array。本 fn 容忍这两种形态:
/// - string 非空 → 原样返回(**不做 JSON 验证**,grok 看得懂 raw blob 即可)
/// - string 空 → `"{}"`
/// - object / array → `serde_json::to_string` 序列化(silent-failure HIGH-2 修)
/// - null / 缺失 → `"{}"`
/// - 其他标量(number / bool)→ `serde_json::to_string` 序列化(虽不合理,
///   但渲染让 user 看到比 silent `{}` 强)
fn render_tool_call_arguments(args: Option<&Value>) -> String {
    let Some(value) = args else {
        return "{}".to_owned();
    };
    if value.is_null() {
        return "{}".to_owned();
    }
    if let Some(s) = value.as_str() {
        if s.is_empty() {
            return "{}".to_owned();
        }
        return s.to_owned();
    }
    serde_json::to_string(value).unwrap_or_else(|err| {
        tracing::warn!(
            error_id = "GROK_WEB_FLATTEN_TOOL_CALL_ARGS_SERIALIZE_FAIL",
            error = %err,
            "tool_call arguments 序列化失败,fallback {{}}"
        );
        "{}".to_owned()
    })
}

fn json_value_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// 抽 chat message 的 content 字段成 text(兼容 string / array of parts)。
///
/// **silent-failure F2 修**:content 数组**有元素但无可提取 text**(image-only /
/// audio-only / tool-call-only / 未识别 part type)时,返回**占位 token**而不是
/// 静默返空字符串。让 grok 模型至少知道"这一轮有非文本内容"。
fn extract_message_content_text(content: Option<&Value>) -> String {
    let Some(c) = content else {
        return String::new();
    };
    if let Some(s) = c.as_str() {
        return s.to_owned();
    }
    let Some(parts) = c.as_array() else {
        return String::new();
    };
    if parts.is_empty() {
        return String::new();
    }
    let texts: Vec<&str> = parts
        .iter()
        .filter_map(|p| {
            // OpenAI chat schema: {type:"text",text:"..."} 或 OpenAI Responses
            // schema: {type:"input_text"|"output_text", text:"..."}
            p.get("text").and_then(Value::as_str)
        })
        .filter(|s| !s.is_empty())
        .collect();
    if !texts.is_empty() {
        return texts.join("\n");
    }
    // F2 修:有 parts 但全无 text → 收集 part types 作 placeholder
    let part_types: Vec<&str> = parts
        .iter()
        .filter_map(|p| p.get("type").and_then(Value::as_str))
        .collect();
    if part_types.is_empty() {
        return String::new();
    }
    let unique_types: std::collections::BTreeSet<&&str> = part_types.iter().collect();
    let summary = unique_types
        .iter()
        .map(|s| **s)
        .collect::<Vec<_>>()
        .join(", ");
    format!("[non-text content omitted: {summary}]")
}

/// 无 session_cache 的 fallback:从 body.input 数组按 Codex Responses spec 抽 messages,
/// **不**走 core(测试 fixture 路径)。生产路径用 with_session 入口。
fn extract_messages_from_input_only(body: &Value) -> Result<Vec<Value>, AdapterError> {
    let mut out: Vec<Value> = Vec::new();
    // 字符串形态:整段当 user
    if let Some(s) = body.get("input").and_then(Value::as_str) {
        if !s.is_empty() {
            out.push(serde_json::json!({"role": "user", "content": s}));
        }
        return Ok(out);
    }
    let arr = body
        .get("input")
        .and_then(Value::as_array)
        .ok_or_else(|| AdapterError::BadRequest("missing `input` field".into()))?;
    for item in arr {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        let content = item.get("content").cloned();
        out.push(serde_json::json!({"role": role, "content": content}));
    }
    if out.is_empty() {
        // silent-failure F4 修:错误消息更详细,帮 debug 时认出"实际是 fallback
        // 路径过滤掉 compaction / function_call_output 等非 message item"。
        let observed_types: std::collections::BTreeSet<String> = body
            .get("input")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|i| i.get("type").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let types_str = if observed_types.is_empty() {
            "[]".to_owned()
        } else {
            observed_types.into_iter().collect::<Vec<_>>().join(", ")
        };
        return Err(AdapterError::BadRequest(format!(
            "grok_web fallback: input array contains no items of type=message \
             (observed types: [{types_str}]). This path is test-only — production \
             routes should use responses_body_to_grok_request_with_session(.., session_cache)."
        )));
    }
    Ok(out)
}

/// Codex `model` 字段 → grok backend `modeId`。
///
/// 入参 `codex_model` 在到达本函数前可能已被 `crates/proxy/src/forward.rs:317`
/// 的 `rewrite_model_field` 改写成 resolver 的 `rewritten_model`(concrete
/// upstream model 名,不是 slot 名)。两种来源都要 handle。
///
/// 优先级:
/// 1. `provider.models[codex_model]`(精确槽位映射,典型场景:body.model 是 slot 名)
/// 2. `provider.models["default"]`(provider 默认 slot)
/// 3. **`codex_model` 自身 as-is** —— resolver 已 rewrite 时这就是 concrete model;
///    或用户直接传 model="grok-420-computer-use-sa" 这种已知 backend 名。
///
/// **不再 hardcoded literal fallback**(原 R3 PoC 设计 silently 路由到
/// "grok-420-computer-use-sa" — review-feedback H2 标记为破坏性,删除)。
/// **不再 Err on miss**(初版改造矫枉过正:用户在 `provider.extra` 给一个
/// 已知 backend model 名 + 不配 `provider.models` 时合法 use case,
/// PR #129 chatgpt-codex-connector P1 标记)。
///
/// Provider schema 在 [`codex_app_transfer_registry::Provider::models`] 是 IndexMap,
/// 按磁盘顺序保留,槽位 key 见 [`codex_app_transfer_registry::ModelSlotKey`]。
fn resolve_mode_id(codex_model: &str, provider: &Provider) -> Result<String, AdapterError> {
    if let Some(mode_id) = provider.models.get(codex_model).cloned() {
        return Ok(mode_id);
    }
    if let Some(default) = provider.models.get("default").cloned() {
        return Ok(default);
    }
    // P1 fallback:miss 槽位 + 无 default → 用 codex_model 自身(已 rewrite 的
    // concrete model 名 or 用户直接传的 backend 名)。**不**用 hardcoded literal,
    // 不静默路由到不同模型,保持用户/resolver 意图。
    Ok(codex_model.to_owned())
}

/// 解析 Codex `reasoning.effort` → grok `is_reasoning` flag。
///
/// 当前简化:`"high"` / `"medium"` → true,`"low"` / `"none"` / unset → false。
/// 后续可与 modeId 一起做更细 routing(`high` 用 grok-420-heavy 等)。
fn parse_reasoning_flag(body: &Value) -> Option<bool> {
    let effort = body
        .get("reasoning")
        .and_then(|r| r.get("effort"))
        .and_then(Value::as_str)?;
    Some(matches!(effort, "high" | "medium"))
}

/// 把 [`GrokChatRequest`] 序列化为 chat endpoint 请求 body。
///
/// 序列化前先调 [`GrokChatRequest::validate`] 检查 message/mode_id 非空 +
/// `extra` 不含 forbidden keys(review-feedback TD3 防 forbidden 字段偷渡)。
pub fn serialize_grok_request(req: &GrokChatRequest) -> Result<Bytes, AdapterError> {
    req.validate()?;
    let bytes = serde_json::to_vec(req).map_err(AdapterError::BodyDecode)?;
    Ok(Bytes::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_transfer_registry::Provider;
    use indexmap::IndexMap;
    use serde_json::json;

    fn make_provider() -> Provider {
        let mut models = IndexMap::new();
        models.insert("default".into(), "grok-420-computer-use-sa".into());
        models.insert("gpt_5_codex".into(), "grok-420-computer-use-sa".into());
        Provider {
            id: "grok-web".into(),
            name: "Grok Web".into(),
            base_url: "https://grok.com".into(),
            auth_scheme: "grok_cookie".into(),
            api_format: "grok_web".into(),
            api_key: String::new(),
            models,
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn single_user_message_flattened_with_user_prefix() {
        // task 18:no-cache 路径(test fixture)— input 数组按 spec 抽完 flatten
        // 成 grok single message string,**带 "User:" 前缀**(grok 模型理解角色)
        let body = json!({
            "model": "gpt_5_codex",
            "input": [
                {"type": "message", "role": "user", "content": "hello world"}
            ]
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        assert_eq!(req.message, "User: hello world");
        assert_eq!(req.mode_id, "grok-420-computer-use-sa");
    }

    #[test]
    fn content_parts_array_joined_then_prefixed() {
        let body = json!({
            "model": "gpt_5_codex",
            "input": [
                {"type": "message", "role": "user", "content": [
                    {"type": "input_text", "text": "part 1"},
                    {"type": "input_text", "text": "part 2"}
                ]}
            ]
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        // content parts join "\n",再带 "User:" prefix
        assert_eq!(req.message, "User: part 1\npart 2");
    }

    #[test]
    fn multi_turn_history_all_roles_flattened_with_prefixes() {
        // task 18 核心修复:多轮 history 全展开(不再只取最后一条 user message)。
        // grok 模型从 role-prefixed string 自己理解上下文。
        let body = json!({
            "model": "gpt_5_codex",
            "input": [
                {"type": "message", "role": "user", "content": "first user"},
                {"type": "message", "role": "assistant", "content": "model reply"},
                {"type": "message", "role": "user", "content": "second user"}
            ]
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        assert_eq!(
            req.message,
            "User: first user\n\nAssistant: model reply\n\nUser: second user"
        );
    }

    // ── task 20 / code-reviewer H3:tool_calls flatten ──────────────────

    #[test]
    fn assistant_with_only_tool_calls_renders_tool_call_segment() {
        // 核心修复:Codex CLI 多轮 tool use 期间 assistant message 形态:
        // `{role:"assistant", content:"", tool_calls:[...]}`,旧实现 content_str
        // 空 → 整段 drop → grok 看不到 cause-effect chain → 后续 tool_call_output
        // orphan。新实现 emit `Assistant (tool call): name(args)`。
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"id": "call_1", "type": "function",
                 "function": {"name": "web_search", "arguments": r#"{"q":"hello"}"#}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(out, r#"Assistant (tool call): web_search({"q":"hello"})"#);
    }

    #[test]
    fn assistant_multiple_tool_calls_each_get_own_line() {
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"name": "search", "arguments": r#"{"q":"x"}"#}},
                {"function": {"name": "read_file", "arguments": r#"{"path":"a.rs"}"#}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(
            out,
            "Assistant (tool call): search({\"q\":\"x\"})\nAssistant (tool call): read_file({\"path\":\"a.rs\"})"
        );
    }

    #[test]
    fn assistant_with_content_and_tool_calls_renders_both() {
        // assistant 同时有 text response 跟 tool_call 时,两段都 emit
        // (content 先,tool_calls 后,各自 \n\n 分隔)
        let messages = [json!({
            "role": "assistant",
            "content": "let me search",
            "tool_calls": [
                {"function": {"name": "web_search", "arguments": r#"{"q":"x"}"#}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(
            out,
            "Assistant: let me search\n\nAssistant (tool call): web_search({\"q\":\"x\"})"
        );
    }

    #[test]
    fn tool_call_then_tool_result_full_chain_flattened() {
        // 完整 cause-effect chain:user → assistant tool_call → tool result → user follow
        // MED-8 修:除了存在性,还要验**顺序**(顺序错位 grok 仍然误读)。
        let messages = [
            json!({"role": "user", "content": "search rust"}),
            json!({"role": "assistant", "content": "",
                   "tool_calls": [{"function": {"name": "web_search", "arguments": r#"{"q":"rust"}"#}}]}),
            json!({"role": "tool", "content": r#"{"results":["..."]}"#}),
            json!({"role": "assistant", "content": "Rust is..."}),
            json!({"role": "user", "content": "tell me more"}),
        ];
        let out = flatten_messages_to_grok_single_string(&messages);
        let pos_user1 = out.find("User: search rust").expect("user1 present");
        let pos_call = out
            .find(r#"Assistant (tool call): web_search({"q":"rust"})"#)
            .unwrap_or_else(|| panic!("tool_call segment missing, got: {out}"));
        let pos_result = out
            .find(r#"Tool Result: {"results":["..."]}"#)
            .expect("tool result present");
        let pos_asst2 = out.find("Assistant: Rust is...").expect("asst2 present");
        let pos_user2 = out.find("User: tell me more").expect("user2 present");
        assert!(
            pos_user1 < pos_call
                && pos_call < pos_result
                && pos_result < pos_asst2
                && pos_asst2 < pos_user2,
            "cause-effect chain 顺序错,grok 会把 Tool Result 当成无来源数据: {out}"
        );
    }

    #[test]
    fn tool_call_missing_name_uses_unknown_placeholder() {
        // **HIGH-1 修**:某 tool_call 缺 function.name 时,**emit `<unknown>` 占位**
        // 而不是 silent skip。原因:整批 nameless 时若全 skip,后续 `tool` role
        // 的 Tool Result 段失去 anchor → orphan(grok 看到 "Tool Result: ..."
        // 凭空冒出),正是 task 20 要修的整体 chain break 病根。
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"arguments": r#"{"x":1}"#}}, // 无 name
                {"function": {"name": "good_call", "arguments": "{}"}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(
            out,
            "Assistant (tool call): <unknown>({\"x\":1})\nAssistant (tool call): good_call({})",
            "nameless call 应用 <unknown> 占位保留 anchor"
        );
    }

    #[test]
    fn all_nameless_tool_calls_still_emit_anchor_for_orphan_prevention() {
        // **HIGH-1 关键 case**:整批 tool_calls 都 nameless 时,旧实现整段 drop,
        // 后续 Tool Result 变 orphan。新实现保留 `<unknown>` 占位 anchor。
        let messages = [
            json!({"role": "assistant", "content": "",
                   "tool_calls": [{"function": {"arguments": r#"{"q":"x"}"#}}]}),
            json!({"role": "tool", "content": "result data"}),
        ];
        let out = flatten_messages_to_grok_single_string(&messages);
        let pos_call = out
            .find("Assistant (tool call): <unknown>")
            .expect("placeholder anchor must be emitted");
        let pos_result = out.find("Tool Result:").expect("tool result emitted");
        assert!(
            pos_call < pos_result,
            "Tool Result 必须有前置 anchor,否则 grok 看不出来源: {out}"
        );
    }

    #[test]
    fn tool_call_missing_arguments_uses_empty_object() {
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"name": "no_args"}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(out, "Assistant (tool call): no_args({})");
    }

    #[test]
    fn empty_tool_calls_array_treated_as_no_tool_calls() {
        // tool_calls 字段存在但空 array → 跟 absent 等价,assistant 段不 emit
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": []
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert!(out.is_empty(), "空 tool_calls + 空 content 应整段 drop");
    }

    #[test]
    fn tool_call_arguments_as_object_gets_serialized() {
        // **HIGH-2 修**:OpenAI 协议 arguments 是 string,但实际 mock / 上游历史
        // 代码常常直接发 object → 旧实现 `as_str` 失败后 silent `{}` → grok 拿到
        // 假 args → tool call 行为偏差。新实现 `serde_json::to_string` 序列化。
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"name": "fn1", "arguments": {"k": "v", "n": 42}}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        // serde_json 输出 key 顺序按 insertion(serde_json 默认 BTreeMap 排序受 features 控制)
        // 用 contains 验关键字段保 robust
        assert!(
            out.starts_with("Assistant (tool call): fn1("),
            "prefix shape wrong: {out}"
        );
        assert!(
            out.contains(r#""k":"v""#),
            "k:v should be serialized: {out}"
        );
        assert!(
            out.contains(r#""n":42"#),
            "n:42 should be serialized: {out}"
        );
    }

    #[test]
    fn tool_call_arguments_empty_string_uses_empty_object() {
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"name": "fn1", "arguments": ""}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(out, "Assistant (tool call): fn1({})");
    }

    #[test]
    fn tool_call_arguments_null_uses_empty_object() {
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"name": "fn1", "arguments": null}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(out, "Assistant (tool call): fn1({})");
    }

    #[test]
    fn tool_call_arguments_with_newlines_collapsed() {
        // args 含 \n 会被 grok 误读为下一段 prefix(`Assistant (tool call): xx(\n
        // {...}\n)` 第二段 grok 看到 `{...}` 不再认 Assistant 段)。新实现折叠
        // \n → ' '。
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                {"function": {"name": "fn1", "arguments": "{\n  \"q\": \"x\"\n}"}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert!(
            !out.contains('\n') || out.matches('\n').count() == 0,
            "no \\n inside the tool_call line: {out}"
        );
    }

    #[test]
    fn non_assistant_role_with_tool_calls_dropped_not_mislabeled() {
        // **HIGH-3 / HIGH-4 修**:system / user / tool role 含 tool_calls 是协议
        // 违反。旧实现会用 "Assistant (tool call):" prefix 渲染 → fabricate
        // assistant turn(比 silent drop 更糟)。新实现 caller gate:non-assistant
        // 不调 render fn,且会 warn,不 emit。
        let messages = [
            json!({
                "role": "system",
                "content": "you are helpful",
                "tool_calls": [{"function": {"name": "ghost", "arguments": "{}"}}]
            }),
            json!({"role": "user", "content": "real user msg"}),
        ];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert!(
            !out.contains("Assistant (tool call): ghost"),
            "system role 的 tool_calls 不能误标为 Assistant: {out}"
        );
        assert!(
            out.contains("System: you are helpful"),
            "system content 保留"
        );
        assert!(out.contains("User: real user msg"), "user content 保留");
    }

    #[test]
    fn tool_calls_not_array_dropped_with_warn() {
        // tool_calls 字段是 object / string(协议违反)→ warn + drop,不 panic
        let messages = [json!({
            "role": "assistant",
            "content": "hi",
            "tool_calls": "not an array"
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(
            out, "Assistant: hi",
            "非 array tool_calls drop,content 保留"
        );
    }

    #[test]
    fn tool_calls_with_null_element_skipped() {
        // 单个 call 不是 object(null / 标量)→ skip 该条,其他正常
        let messages = [json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                null,
                {"function": {"name": "real_call", "arguments": "{}"}}
            ]
        })];
        let out = flatten_messages_to_grok_single_string(&messages);
        assert_eq!(out, "Assistant (tool call): real_call({})");
    }

    #[test]
    fn previous_response_id_resolved_via_tracker() {
        let tracker = ParentResponseTracker::default();
        tracker.record_str("resp_abc", "9f82a10c-grok-uuid");
        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "follow-up"}],
            "previous_response_id": "resp_abc"
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, Some(&tracker)).unwrap();
        assert_eq!(
            req.parent_response_id.as_deref(),
            Some("9f82a10c-grok-uuid")
        );
    }

    #[test]
    fn previous_response_id_miss_omits_parent_response_id() {
        let tracker = ParentResponseTracker::default();
        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "x"}],
            "previous_response_id": "resp_unknown"
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, Some(&tracker)).unwrap();
        assert!(req.parent_response_id.is_none());
    }

    #[test]
    fn missing_input_array_errors_with_bad_request() {
        let body = json!({ "model": "gpt_5_codex" });
        let p = make_provider();
        let err = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap_err();
        assert!(matches!(err, AdapterError::BadRequest(_)));
    }

    #[test]
    fn reasoning_high_sets_is_reasoning_true() {
        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "x"}],
            "reasoning": { "effort": "high" }
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        assert_eq!(req.is_reasoning, Some(true));
    }

    #[test]
    fn instructions_field_handled_differently_in_two_paths() {
        // task 18 + silent-failure F3:`instructions` 字段处理分两条路径:
        //   - **with_session 路径**(生产):core build_messages_from_input 把
        //     instructions 合并进 messages[0]=system,flatten 后变成 "System: ..."
        //     prefix 段,customInstructions=None(避免双重 system)。
        //   - **无 session 路径**(test fixture):不走 core,直接读 body.instructions
        //     塞 customInstructions 字段防止 instructions 完全丢失(F3 修)。
        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "x"}],
            "instructions": "You are a Rust expert."
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        // fallback 路径:instructions 走 customInstructions 字段
        assert_eq!(
            req.custom_instructions.as_deref(),
            Some("You are a Rust expert.")
        );
    }

    #[test]
    fn resolve_mode_id_falls_back_to_codex_model_when_no_slot_and_no_default() {
        // chatgpt-codex-connector PR #129 P1:miss 槽位 + 无 default 时,fallback
        // 用 codex_model 自身(已被 resolver rewrite 成 concrete upstream model,
        // 或用户直接传的已知 backend 名);**不**走 hardcoded literal。
        let p = Provider {
            id: "grok-web".into(),
            name: "Grok Web".into(),
            base_url: "https://grok.com".into(),
            auth_scheme: "grok_cookie".into(),
            api_format: "grok_web".into(),
            api_key: String::new(),
            models: IndexMap::new(), // 空
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        };
        // 模拟 resolver.rs:317 已 rewrite body.model 成 concrete grok backend
        let body = json!({
            "model": "grok-420-computer-use-sa",
            "input": [{"type": "message", "role": "user", "content": "hi"}]
        });
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        assert_eq!(
            req.mode_id, "grok-420-computer-use-sa",
            "已 rewrite 的 concrete model 应直接当 modeId,不被静默替换"
        );
    }

    #[test]
    fn serialize_grok_request_rejects_forbidden_extra_field() {
        // review-feedback TD3:serialize_grok_request 调 validate,connectorIds 偷渡被拦截
        let mut req = GrokChatRequest::default();
        req.message = "hi".into();
        req.extra.insert("connectorIds".into(), json!(["uuid-1"]));
        let err = serialize_grok_request(&req).unwrap_err();
        assert!(matches!(err, AdapterError::BadRequest(_)));
    }

    #[test]
    fn serialized_payload_excludes_connector_fields() {
        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "x"}]
        });
        let p = make_provider();
        let req = responses_body_to_grok_request_with_tracker(&body, &p, None).unwrap();
        let bytes = serialize_grok_request(&req).unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap();
        let obj = json.as_object().unwrap();
        // Connector 字段一律不传(server-side state 黑名单)
        assert!(!obj.contains_key("connectorIds"));
        assert!(!obj.contains_key("connectors"));
        assert!(!obj.contains_key("toolOverrides"));
        // 但黑名单字段存在(空)
        assert_eq!(obj["disabledConnectorIds"], json!([]));
    }

    // ── task 18:with_session 路径(对齐 ARCHITECTURE_PROTOCOL_GUIDE)─────────

    // task 22 / I7 修:删 `with_session_no_cache_falls_back_to_input_only_path`
    // 测试 — 验"None 时降级"恰好是本 PR 砍掉的 foot-gun。`session_cache: &...`
    // 必填后,`None` 在类型系统层面变 impossible state,无 test 可写。
    // Test fixture 路径已迁回 `responses_body_to_grok_request_with_tracker`,
    // 由本 mod 其他 tests 覆盖(见 `serialized_payload_excludes_connector_fields`
    // 等)。

    #[test]
    fn with_session_no_history_emits_single_message_plan() {
        // **silent-failure LOW-1 修**:补 with_session 路径下"空 cache + 仅本轮
        // 输入"的最小 case,验 ResponseSessionPlan 形状正确(messages.len()=1 +
        // response_id 已生成),防 with_session 入口 plan 字段被 future refactor
        // 误回归。
        let cache = ResponseSessionCache::new(8, std::time::Duration::from_secs(60));
        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "hi"}]
        });
        let p = make_provider();
        let conv = responses_body_to_grok_request_with_session(&body, &p, &cache).unwrap();
        assert_eq!(conv.request.message, "User: hi");
        assert!(!conv.response_session.response_id.is_empty());
        assert_eq!(
            conv.response_session.messages.len(),
            1,
            "无历史 + 单 user message 时 plan 应只含本轮 1 条消息,got: {:?}",
            conv.response_session.messages
        );
    }

    #[test]
    fn with_session_cache_hit_flattens_history_into_grok_message() {
        // task 18 核心场景:.app 重启 / 长对话场景,session_cache 拉历史。
        // 模拟 cache 已有 1 轮 user+assistant 历史,本轮 body 只带 follow-up question
        let cache = ResponseSessionCache::new(8, std::time::Duration::from_secs(60));
        cache.save(
            "resp_prev_grok",
            vec![
                json!({"role": "user", "content": "what is Rust?"}),
                json!({"role": "assistant", "content": "Rust is a systems language."}),
            ],
        );

        let body = json!({
            "model": "gpt_5_codex",
            "input": [{"type": "message", "role": "user", "content": "tell me more"}],
            "previous_response_id": "resp_prev_grok"
        });
        let p = make_provider();
        let conv = responses_body_to_grok_request_with_session(&body, &p, &cache).unwrap();
        // 验证 grok message 包含完整 history + 当前 follow-up
        assert!(
            conv.request.message.contains("User: what is Rust?"),
            "missing history user, got: {}",
            conv.request.message
        );
        assert!(
            conv.request
                .message
                .contains("Assistant: Rust is a systems language."),
            "missing history assistant, got: {}",
            conv.request.message
        );
        assert!(
            conv.request.message.contains("User: tell me more"),
            "missing follow-up user, got: {}",
            conv.request.message
        );
    }

    #[test]
    fn with_session_compaction_item_unfolded_by_core() {
        // task 18:Codex CLI autocompact 把长对话压缩成 `type=compaction` item
        // (字段 `encrypted_content` 实际是 plain summary text)。core/responses
        // `build_messages_from_input` 把它展开成 user message 注入历史。
        // grok mapper 走 with_session 路径自动享受 — 不再 drop 这条信息。
        let body = json!({
            "model": "gpt_5_codex",
            "input": [
                {"type": "compaction",
                 "encrypted_content": "[Summary of previous turns]\nUser asked about Rust, assistant explained."},
                {"type": "message", "role": "user", "content": "and lifetimes?"}
            ]
        });
        let p = make_provider();
        let cache = ResponseSessionCache::new(8, std::time::Duration::from_secs(60));
        let conv = responses_body_to_grok_request_with_session(&body, &p, &cache).unwrap();
        // compaction item 展开后作 user message 注入(具体 prefix 行为由
        // responses/request.rs::build_messages_from_input 控制),只要文本能在 grok
        // 看到 final message 里就 OK
        // 注:core merge_consecutive_user_messages 把"compaction 展开后的
        // user message"跟"后续 user message"合并成单 user 段落,所以
        // "User: and lifetimes?" 不是独立 prefix,而是跟 summary 同 user content
        // 内合并(用 \n 分隔)。grok 模型看到的是同一段 user context。
        assert!(
            conv.request.message.contains("Summary of previous turns"),
            "compaction summary missing, got: {}",
            conv.request.message
        );
        assert!(
            conv.request.message.contains("and lifetimes?"),
            "current user follow-up missing, got: {}",
            conv.request.message
        );
        // F5 修(review-feedback):不 assert `starts_with("User:")` —— 那是
        // core/responses `merge_consecutive_user_messages` 的内部实现细节,
        // grok adapter 不该耦合;弱化为"flatten 输出含至少一段 User: prefix"
        assert!(
            conv.request.message.contains("User:"),
            "flatten should include at least one `User:` prefix, got: {}",
            conv.request.message
        );
    }
}
