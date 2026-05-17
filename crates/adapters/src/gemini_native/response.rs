//! Gemini `streamGenerateContent?alt=sse` SSE → OpenAI Responses API SSE 直转。
//!
//! 设计(2026-05-10 用户决策):**Gemini → Responses 直转**,不走 chat 中间。
//! Codex.app 入站 /responses 期待原生 Responses SSE 事件流;我们直接构造
//! `response.created` / `response.output_item.added` / `response.output_text.delta`
//! / `response.function_call_arguments.delta` / `response.output_text.annotation.added`
//! / `response.completed` 等事件,跟 ChatToResponsesConverter 同形态但消费 Gemini
//! wire 而非 chat wire。
//!
//! 依赖隔离:跟 `crates/adapters/src/responses/` 无依赖(除字段名 + envelope
//! 形态对齐 OpenAI Responses 协议),Gemini 适配器自给自足。
//!
//! Responses SSE 事件序列(Codex.app 期望):
//! ```text
//! response.created                    ← 首事件,含 envelope (status="in_progress")
//! response.in_progress                ← 立即跟在 created 后,同一份 envelope
//! [for each output item:]
//!   response.output_item.added        ← item type=message/reasoning/function_call
//!   [if message:]
//!     response.content_part.added     ← part type=output_text
//!     response.output_text.delta      ← 增量文本(可多次)
//!     response.output_text.annotation.added  ← grounding citation(可多次)
//!     response.output_text.done       ← 文本累积完毕
//!     response.content_part.done
//!   [if reasoning:]
//!     response.reasoning_summary_part.added  ← summary_index=0
//!     response.reasoning_summary_text.delta  ← 增量
//!     response.reasoning_summary_text.done
//!     response.reasoning_summary_part.done
//!   [if function_call:]
//!     response.function_call_arguments.delta  ← 一次性 emit 完整 args(Gemini 不增量)
//!     response.function_call_arguments.done
//!   response.output_item.done
//! response.completed                  ← 末事件,含完整 envelope (status="completed",
//!                                       output[],usage,finish_reason 等)
//! ```
//!
//! Gemini → Responses 字段映射:
//! - `candidates[].content.parts[].text` (thought≠true) → output_text.delta
//! - `candidates[].content.parts[].text` (thought=true) → reasoning_summary_text.delta
//! - `candidates[].content.parts[].functionCall {name, args}` → function_call output_item
//!   (args 序列化成 JSON string 灌进 function_call_arguments.delta)
//! - `candidates[].groundingMetadata` → output_text.annotation.added(在 message 内)
//! - `candidates[].finishReason` → completed envelope 的 incomplete_details(若非 STOP)
//! - `usageMetadata` → completed envelope 的 usage 字段

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};

use crate::core::events::{build_tool_namespace_map, emit_sse_event as emit_event};
use crate::responses::global_response_session_cache;
use crate::types::{ByteStream, ResponseSessionPlan};

use super::grounding::convert_grounding_metadata_to_annotations;
use super::types::{map_finish_reason, GenerateContentResponse};

// ═══════════════════════════════════════════════════════════════════════════
// 工具函数
// ═══════════════════════════════════════════════════════════════════════════

/// 24 hex char ID(对齐 OpenAI `call_<24hex>` / `resp_<24hex>` 等形态)。
fn synthesize_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{ts:016x}{n:08x}")
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 找 SSE event 边界。SSE spec 允许 `\n\n` 或 `\r\n\r\n` 分隔,Google `alt=sse`
/// 走 gRPC-gateway 经常 emit CRLF 行尾,如果只识 LF 会让整个流被 buffer 到结束
/// 才一次性 process(streaming → batch 退化)。返 (边界 byte index, 边界长度)。
fn find_event_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n");
    let crlf = if buf.len() >= 4 {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    } else {
        None
    };
    match (lf, crlf) {
        (Some(l), Some(c)) if l <= c => Some((l, 2)),
        (Some(_), Some(c)) => Some((c, 4)),
        (Some(l), None) => Some((l, 2)),
        (None, Some(c)) => Some((c, 4)),
        (None, None) => None,
    }
}

/// 上游 stream 异常终止时 synthetic finish reason — emit_completed 看到这个值
/// 会写 status="incomplete" + reason="interrupted",防止 silent truncation
/// (网络断流 / 上游 5xx 中断 / accumulated_json overflow 等)被客户端误读为
/// "completed"。**不能**跟真 Gemini finishReason 同名(INTERRUPTED 不在
/// `_GEMINI_FINISH_REASON_KEYS`)。
const FINISH_INTERRUPTED: &str = "_INTERRUPTED";

/// accumulated_json 防御上限:Gemini upstream 偶尔会发分片 JSON,但持续累积
/// 不收敛 = 上游异常 / 中间代理在乱发数据。256 KiB 足够覆盖 Gemini 单 chunk
/// 含完整 grounding metadata + 长 reasoning 的合理上限。
const MAX_ACCUMULATED_JSON_BYTES: usize = 256 * 1024;

// ═══════════════════════════════════════════════════════════════════════════
// 出 output items 的内部状态
// ═══════════════════════════════════════════════════════════════════════════

struct OpenMessage {
    item_id: String,
    output_index: u32,
    /// 累积的全文(close 时灌进 output_text.done.text + content[0].text)
    text_acc: String,
    /// 累积的 url citations(close 时灌进 message.content[0].annotations)
    annotations_acc: Vec<Value>,
}

struct OpenReasoning {
    item_id: String,
    output_index: u32,
    /// 累积的 reasoning summary 文本(close 时灌进 summary[0].text)
    text_acc: String,
}

struct ClosedFunctionCall {
    item_id: String,
    output_index: u32,
    call_id: String,
    name: String,
    arguments_json_str: String,
    /// `Some("mcp__<server>__")` 当 function.name 来自 namespace 包装。
    /// envelope output[] emit 时回灌成 item.namespace 字段。
    namespace: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════
// 主转换器
// ═══════════════════════════════════════════════════════════════════════════

pub struct GeminiToResponsesConverter {
    // ─ SSE byte 解析 buffer ─
    buffer: Vec<u8>,
    accumulated_json: String,

    // ─ envelope 字段(回灌入站 Responses request 的 tools/instructions/temperature/...)─
    response_id: String,
    model: String,
    sequence_number: u64,
    created_at: u64,
    original_request: Option<Value>,
    /// Session resume:Codex.app `previous_response_id` cache 命中时,
    /// 这里持有上一轮的响应 metadata。MVP 暂不主动读取(只透传),follow-up
    /// 可以让 envelope.id 复用 session.response_id 提升 Codex.app session
    /// 链路稳定性。
    #[allow(dead_code)]
    response_session: Option<ResponseSessionPlan>,

    // ─ 生命周期 ─
    lifecycle_opened: bool,
    completed_emitted: bool,

    // ─ output items 状态 ─
    next_output_index: u32,
    open_message: Option<OpenMessage>,
    open_reasoning: Option<OpenReasoning>,
    /// 已 close 的 function_call(完整 envelope output[] 用)
    closed_function_calls: Vec<ClosedFunctionCall>,
    /// 已 close 的 message items(H3 修复:Gemini 多轮 text→fc→text 序列会让
    /// 同一 stream 产生多个 message,旧实现 Option<Value> 会让后者覆盖前者,
    /// 导致 envelope output[] 跟客户端实际 stream 收到的事件 output_index 不匹配)。
    /// 用 (output_index, item) 元组,emit_completed 时按 output_index 排序。
    closed_messages: Vec<(u32, Value)>,
    /// 已 close 的 reasoning items(同 H3 设计)
    closed_reasonings: Vec<(u32, Value)>,
    /// P0-E:已 close 的非 message/reasoning/function_call 类 items
    /// (image_generation_call / 等扩展 type),emit_completed 也按 output_index 排序
    closed_other_items: Vec<(u32, Value)>,

    // ─ 终态 ─
    has_seen_tool_calls: bool,
    final_finish_reason: Option<String>,
    final_usage: Option<Value>,

    // ─ Gemini provider-specific metadata 累积(P0-C/D 修复:不丢上游字段)─
    /// candidate.safetyRatings 累积,envelope.metadata.gemini.safety_ratings 透出
    accumulated_safety_ratings: Vec<Value>,
    /// candidate.citationMetadata(老 Gemini 1.5 引用形式)
    accumulated_citation_metadata: Vec<Value>,
    /// candidate.urlContextMetadata(urlContext 工具响应)
    accumulated_url_context_metadata: Vec<Value>,
    /// candidate.logprobsResult
    accumulated_logprobs: Vec<Value>,
    /// candidate.tokenCount(部分 Gemini 模型逐 candidate 报)
    accumulated_token_counts: Vec<i64>,
    /// groundingMetadata.searchEntryPoint(**Google 法律 ToS 要求显示** 的
    /// "Search Suggestion" widget,UI 不显示等于违反 Google API 条款)
    accumulated_search_entry_points: Vec<Value>,
    /// groundingMetadata.webSearchQueries(透明展示哪些查询被发起)
    accumulated_web_search_queries: Vec<String>,
    /// groundingMetadata.retrievalMetadata
    accumulated_retrieval_metadata: Vec<Value>,
    /// promptFeedback.blockReason — Gemini 拒 user prompt(safety)时设置;
    /// emit_completed 看到非空 → status=incomplete + reason=content_filter +
    /// error.code=prompt_blocked
    prompt_block_reason: Option<String>,
    prompt_feedback_safety: Vec<Value>,

    /// **MCP namespace 反查表**(`function.name → namespace.name`)。Codex CLI
    /// 把每个 `[mcp_servers.<name>]` 入站打成
    /// `{type:"namespace", name:"mcp__<name>__", tools:[{type:"function",...}]}`。
    /// emit `function_call` SSE event 时(`response.output_item.added` /
    /// `.done` + envelope `output[]`),如果 function.name 命中此表,**必须**给
    /// item 加 `namespace: "mcp__<name>__"` 字段 — Codex.app 客户端按 namespace
    /// dispatch 到对应 MCP server,缺字段时所有 namespace 工具调用退化成普通
    /// function call,模型不知怎么路由就绕路 grep env / find config 自我发现。
    ///
    /// 跟 [`crate::responses::converter::ResponsesConverter::tool_namespace_map`]
    /// 同款 — 1:1 移植(2026-05-11 实测 Codex.app + Gemini 路径 MCP 调用)
    tool_namespace_map: std::collections::HashMap<String, String>,
}

impl GeminiToResponsesConverter {
    pub fn new(
        original_request: Option<Value>,
        response_session: Option<ResponseSessionPlan>,
    ) -> Self {
        let response_id = response_session
            .as_ref()
            .map(|s| s.response_id.clone())
            .unwrap_or_else(|| format!("resp_{}", synthesize_id()));
        // **build namespace map 必须先于 move**(struct field 顺序里
        // original_request 在前会先 move 走,后面 tool_namespace_map 拿不到 ref)
        let tool_namespace_map = build_tool_namespace_map(original_request.as_ref());
        Self {
            buffer: Vec::new(),
            accumulated_json: String::new(),
            response_id,
            model: String::new(),
            sequence_number: 0,
            created_at: now_unix_secs(),
            original_request,
            response_session,
            lifecycle_opened: false,
            completed_emitted: false,
            next_output_index: 0,
            open_message: None,
            open_reasoning: None,
            closed_function_calls: Vec::new(),
            closed_messages: Vec::new(),
            closed_reasonings: Vec::new(),
            closed_other_items: Vec::new(),
            has_seen_tool_calls: false,
            final_finish_reason: None,
            final_usage: None,
            accumulated_safety_ratings: Vec::new(),
            accumulated_citation_metadata: Vec::new(),
            accumulated_url_context_metadata: Vec::new(),
            accumulated_logprobs: Vec::new(),
            accumulated_token_counts: Vec::new(),
            accumulated_search_entry_points: Vec::new(),
            accumulated_web_search_queries: Vec::new(),
            accumulated_retrieval_metadata: Vec::new(),
            prompt_block_reason: None,
            prompt_feedback_safety: Vec::new(),
            tool_namespace_map,
        }
    }

    /// 查询 function.name 对应的 namespace.name —— emit function_call event 时
    /// 用来给 item 加 `namespace` 字段(Codex.app dispatch 必要)。跟
    /// [`crate::responses::converter::ResponsesConverter::lookup_namespace_for`] 同款
    fn lookup_namespace_for(&self, tool_name: &str) -> Option<&str> {
        self.tool_namespace_map.get(tool_name).map(String::as_str)
    }

    // ───── envelope 构造 ─────

    /// 从 original_request 拿字段或 fallback。Codex.app 客户端按 envelope.tools
    /// 用 `(namespace, function.name)` 复合主键反向路由 namespace 包装的 MCP tool,
    /// 必须回灌完整 tools 数组。
    fn req_field_or(&self, key: &str, fallback: Value) -> Value {
        self.original_request
            .as_ref()
            .and_then(|r| r.get(key))
            .cloned()
            .unwrap_or(fallback)
    }

    /// 把累积的 Gemini provider-specific metadata 收成 dict,塞进 envelope.metadata.gemini。
    /// 包括:safetyRatings / citationMetadata / urlContextMetadata / logprobsResult /
    /// tokenCount(每 candidate)+ groundingMetadata 完整 (searchEntryPoint / webSearchQueries
    /// / retrievalMetadata)+ promptFeedback。
    /// 客户端可按需展示(searchEntryPoint 是 Google ToS 法律要求)— 至少 wire 上不丢。
    fn build_gemini_metadata(&self) -> Option<Value> {
        let mut g = serde_json::Map::new();
        if !self.accumulated_safety_ratings.is_empty() {
            g.insert(
                "safety_ratings".into(),
                Value::Array(self.accumulated_safety_ratings.clone()),
            );
        }
        if !self.accumulated_citation_metadata.is_empty() {
            g.insert(
                "citation_metadata".into(),
                Value::Array(self.accumulated_citation_metadata.clone()),
            );
        }
        if !self.accumulated_url_context_metadata.is_empty() {
            g.insert(
                "url_context_metadata".into(),
                Value::Array(self.accumulated_url_context_metadata.clone()),
            );
        }
        if !self.accumulated_logprobs.is_empty() {
            g.insert(
                "logprobs".into(),
                Value::Array(self.accumulated_logprobs.clone()),
            );
        }
        if !self.accumulated_token_counts.is_empty() {
            g.insert(
                "candidate_token_counts".into(),
                Value::Array(
                    self.accumulated_token_counts
                        .iter()
                        .map(|n| json!(n))
                        .collect(),
                ),
            );
        }
        let mut grounding = serde_json::Map::new();
        if !self.accumulated_search_entry_points.is_empty() {
            grounding.insert(
                "search_entry_point".into(),
                Value::Array(self.accumulated_search_entry_points.clone()),
            );
        }
        if !self.accumulated_web_search_queries.is_empty() {
            grounding.insert(
                "web_search_queries".into(),
                Value::Array(
                    self.accumulated_web_search_queries
                        .iter()
                        .map(|s| Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        if !self.accumulated_retrieval_metadata.is_empty() {
            grounding.insert(
                "retrieval_metadata".into(),
                Value::Array(self.accumulated_retrieval_metadata.clone()),
            );
        }
        if !grounding.is_empty() {
            g.insert("grounding".into(), Value::Object(grounding));
        }
        if let Some(br) = &self.prompt_block_reason {
            let mut pf = serde_json::Map::new();
            pf.insert("block_reason".into(), Value::String(br.clone()));
            if !self.prompt_feedback_safety.is_empty() {
                pf.insert(
                    "safety_ratings".into(),
                    Value::Array(self.prompt_feedback_safety.clone()),
                );
            }
            g.insert("prompt_feedback".into(), Value::Object(pf));
        }
        if g.is_empty() {
            None
        } else {
            Some(Value::Object(g))
        }
    }

    fn build_envelope(&self, status: &str) -> Value {
        // H4 修复:Gemini 上游 modelVersion 字段在首 chunk 里经常缺失
        // (有时只在末 chunk 出现),旧实现 envelope.model="unknown" 会让
        // Codex.app session 的 cost 归因 / 历史 model 过滤全错。fallback 到
        // original_request["model"](客户端发出来时的 alias)做诊断价值更高。
        let model_str: String = if !self.model.is_empty() {
            self.model.clone()
        } else {
            self.original_request
                .as_ref()
                .and_then(|r| r.get("model"))
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| "unknown".into())
        };
        // P0-C/D 修复:envelope.metadata 合并用户原 metadata + gemini provider-specific
        // metadata(safetyRatings / grounding / promptFeedback / etc),不丢上游字段
        let user_metadata = self.req_field_or("metadata", Value::Null);
        let gemini_metadata = self.build_gemini_metadata();
        let metadata = match (user_metadata, gemini_metadata) {
            (Value::Object(mut user), Some(g)) => {
                user.insert("gemini".into(), g);
                Value::Object(user)
            }
            (user, Some(g)) if !user.is_null() => {
                let mut m = serde_json::Map::new();
                m.insert("user".into(), user);
                m.insert("gemini".into(), g);
                Value::Object(m)
            }
            (user, None) => user,
            (Value::Null, Some(g)) => {
                let mut m = serde_json::Map::new();
                m.insert("gemini".into(), g);
                Value::Object(m)
            }
            (other, Some(g)) => {
                let mut m = serde_json::Map::new();
                m.insert("user".into(), other);
                m.insert("gemini".into(), g);
                Value::Object(m)
            }
        };
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": model_str,
            "tools": self.req_field_or("tools", json!([])),
            "tool_choice": self.req_field_or("tool_choice", json!("auto")),
            "parallel_tool_calls": self.req_field_or("parallel_tool_calls", json!(true)),
            "reasoning": self.req_field_or("reasoning", json!({"effort": null, "summary": null})),
            "text": self.req_field_or("text", json!({"format": {"type": "text"}})),
            "metadata": metadata,
            "previous_response_id": self.req_field_or("previous_response_id", Value::Null),
            "instructions": self.req_field_or("instructions", Value::Null),
            "temperature": self.req_field_or("temperature", Value::Null),
            "top_p": self.req_field_or("top_p", Value::Null),
            "max_output_tokens": self.req_field_or("max_output_tokens", Value::Null),
            "truncation": "disabled",
        })
    }

    /// 从 `response.completed.output[]` 还原一条可缓存的 assistant message,
    /// 供 `previous_response_id` 续轮恢复历史。
    ///
    /// 跟 `responses::converter::assistant_message()` 语义对齐:
    /// - message items 的 output_text 拼成 `content`
    /// - reasoning items 的 summary_text 拼成 `reasoning_content`
    /// - function_call items 还原成 `tool_calls`
    fn build_assistant_message_for_session(output_items: &[Value]) -> Option<Value> {
        let mut text_parts: Vec<String> = Vec::new();
        let mut reasoning_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<Value> = Vec::new();

        for item in output_items {
            match item.get("type").and_then(|v| v.as_str()) {
                Some("message") => {
                    if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                        for part in content {
                            if part.get("type").and_then(|v| v.as_str()) == Some("output_text") {
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        text_parts.push(text.to_owned());
                                    }
                                }
                            }
                        }
                    }
                }
                Some("reasoning") => {
                    if let Some(summary) = item.get("summary").and_then(|v| v.as_array()) {
                        for part in summary {
                            if part.get("type").and_then(|v| v.as_str()) == Some("summary_text") {
                                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        reasoning_parts.push(text.to_owned());
                                    }
                                }
                            }
                        }
                    }
                }
                Some("function_call") => {
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    let name = item
                        .get("name")
                        .cloned()
                        .unwrap_or(Value::String(String::new()));
                    let arguments = item
                        .get("arguments")
                        .cloned()
                        .unwrap_or(Value::String("{}".into()));
                    tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": arguments,
                        }
                    }));
                }
                _ => {}
            }
        }

        if text_parts.is_empty() && reasoning_parts.is_empty() && tool_calls.is_empty() {
            return None;
        }

        let mut message = json!({
            "role": "assistant",
            "content": text_parts.join("\n"),
        });
        if !reasoning_parts.is_empty() {
            message["reasoning_content"] = Value::String(reasoning_parts.join("\n"));
        }
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }
        Some(message)
    }

    // ───── 字节 feed ─────

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        if self.completed_emitted {
            return Vec::new();
        }
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        loop {
            let Some((pos, sep_len)) = find_event_boundary(&self.buffer) else {
                break;
            };
            let event_bytes: Vec<u8> = self.buffer.drain(..pos).collect();
            // 跳过分隔符(2 字节 \n\n 或 4 字节 \r\n\r\n)
            self.buffer
                .drain(..sep_len.min(self.buffer.len()))
                .for_each(drop);
            self.process_event(&event_bytes, &mut out);
        }
        out
    }

    pub fn finish(&mut self) -> Vec<u8> {
        if self.completed_emitted {
            return Vec::new();
        }
        let mut out = Vec::new();
        // 残留 buffer 当最后一个 event 强行 process(可能是网络断流没收到 \n\n)
        if !self.buffer.is_empty() {
            let event = std::mem::take(&mut self.buffer);
            self.process_event(&event, &mut out);
        }
        // 关掉所有 open items
        if self.open_message.is_some() {
            self.close_message(&mut out);
        }
        if self.open_reasoning.is_some() {
            self.close_reasoning(&mut out);
        }
        // 若从未收到任何上游数据,补 lifecycle open(防客户端卡)
        if !self.lifecycle_opened {
            self.emit_lifecycle_open(&mut out);
        }
        // 末事件 response.completed
        self.emit_completed(&mut out);
        out
    }

    fn process_event(&mut self, event_bytes: &[u8], output: &mut Vec<u8>) {
        let Ok(event_str) = std::str::from_utf8(event_bytes) else {
            return;
        };
        let mut data_buf = String::new();
        for line in event_str.lines() {
            let Some(rest) = line.strip_prefix("data:") else {
                continue;
            };
            let trimmed = rest.trim_start();
            if !data_buf.is_empty() {
                data_buf.push('\n');
            }
            data_buf.push_str(trimmed);
        }
        if data_buf.is_empty() {
            return;
        }
        if data_buf.trim() == "[DONE]" {
            // Gemini 通常不发 [DONE],收到也无害,等 finish() 处理
            return;
        }
        // 直接解析,失败进 accumulated_json 兜底
        match serde_json::from_str::<GenerateContentResponse>(&data_buf) {
            Ok(gemini) => {
                self.accumulated_json.clear();
                self.process_gemini_chunk(gemini, output);
            }
            Err(_) => {
                if !self.accumulated_json.is_empty() {
                    self.accumulated_json.push('\n');
                }
                self.accumulated_json.push_str(&data_buf);
                // 防御无界增长(C3b):上游持续 emit 解不开的 JSON 时主动 cut 流
                // + 标 INTERRUPTED,emit_completed 会输出 incomplete + interrupted
                if self.accumulated_json.len() > MAX_ACCUMULATED_JSON_BYTES {
                    tracing::error!(
                        size = self.accumulated_json.len(),
                        cap = MAX_ACCUMULATED_JSON_BYTES,
                        "gemini SSE accumulated JSON exceeds safety cap, dropping buffer + marking interrupted"
                    );
                    self.accumulated_json.clear();
                    self.final_finish_reason = Some(FINISH_INTERRUPTED.to_owned());
                    return;
                }
                if let Ok(gemini) =
                    serde_json::from_str::<GenerateContentResponse>(&self.accumulated_json)
                {
                    self.accumulated_json.clear();
                    self.process_gemini_chunk(gemini, output);
                }
            }
        }
    }

    // ───── 处理一个 Gemini chunk ─────

    fn process_gemini_chunk(&mut self, gemini: GenerateContentResponse, out: &mut Vec<u8>) {
        // 首 chunk:补全 model + 触发 lifecycle open
        if let Some(model) = gemini.model_version {
            if !model.is_empty() {
                self.model = model;
            }
        }
        if !self.lifecycle_opened {
            self.emit_lifecycle_open(out);
        }

        // 处理候选(MVP 只关心 candidates[0],n>1 的多 candidate 留 follow-up)
        for candidate in &gemini.candidates {
            if let Some(content) = &candidate.content {
                for part in &content.parts {
                    // text part
                    if let Some(text) = &part.text {
                        if part.thought == Some(true) {
                            // reasoning text:必要时关 message + 开 reasoning
                            if self.open_message.is_some() {
                                self.close_message(out);
                            }
                            if self.open_reasoning.is_none() {
                                self.open_reasoning(out);
                            }
                            self.emit_reasoning_delta(out, text);
                        } else {
                            // 文本 text:必要时关 reasoning + 开 message
                            if self.open_reasoning.is_some() {
                                self.close_reasoning(out);
                            }
                            if self.open_message.is_none() {
                                self.open_message(out);
                            }
                            self.emit_text_delta(out, text);
                        }
                    }
                    // functionCall part
                    if let Some(fc) = &part.function_call {
                        // function_call 是独立 output item,关掉所有 message/reasoning
                        if self.open_message.is_some() {
                            self.close_message(out);
                        }
                        if self.open_reasoning.is_some() {
                            self.close_reasoning(out);
                        }
                        // P1-B:thoughtSignature 在 functionCall part 上时编码进 call_id,
                        // client roundtrip 时由请求侧 decode_tool_call_id_signature 拆出
                        // signature 写回 outgoing functionCall part(LiteLLM
                        // _encode_tool_call_id_with_signature 模式)。Gemini 3 多轮
                        // thinking 链才不会断。
                        self.emit_function_call(
                            out,
                            &fc.name,
                            &fc.args,
                            part.thought_signature.as_deref(),
                        );
                        self.has_seen_tool_calls = true;
                    }
                    // P0-E:模型多模态输出 inline_data(图/音频/视频)→ 独立 output_item
                    if let Some(inline) = &part.inline_data {
                        if self.open_message.is_some() {
                            self.close_message(out);
                        }
                        if self.open_reasoning.is_some() {
                            self.close_reasoning(out);
                        }
                        self.emit_inline_data(out, &inline.mime_type, &inline.data);
                    }
                    // P0-E:模型 file_data 输出(罕见,Gemini 偶尔通过 fileUri 引用上游存的文件)
                    if let Some(file) = &part.file_data {
                        if self.open_message.is_some() {
                            self.close_message(out);
                        }
                        if self.open_reasoning.is_some() {
                            self.close_reasoning(out);
                        }
                        self.emit_file_data(out, &file.mime_type, &file.file_uri);
                    }
                }
            }
            // groundingMetadata → annotations(挂到 active message)+ 累积完整字段
            // (P0-D 修复:searchEntryPoint 是 Google ToS 法律要求显示的 widget,
            // webSearchQueries/retrievalMetadata 是透明展示信息,全部累积透出 envelope)
            if let Some(gm) = &candidate.grounding_metadata {
                let annotations = convert_grounding_metadata_to_annotations(gm);
                if !annotations.is_empty() {
                    if self.open_message.is_none() {
                        // annotation 必须挂在 message 上;若还没开,先开
                        self.open_message(out);
                    }
                    self.emit_annotations(out, annotations);
                }
                if let Some(sep) = &gm.search_entry_point {
                    self.accumulated_search_entry_points.push(sep.clone());
                }
                if let Some(qs) = &gm.web_search_queries {
                    self.accumulated_web_search_queries
                        .extend(qs.iter().cloned());
                }
                if let Some(rm) = &gm.retrieval_metadata {
                    self.accumulated_retrieval_metadata.push(rm.clone());
                }
            }
            // P0-C:Candidate metadata 累积
            if let Some(sr) = &candidate.safety_ratings {
                self.accumulated_safety_ratings.extend(sr.iter().cloned());
            }
            if let Some(cm) = &candidate.citation_metadata {
                self.accumulated_citation_metadata.push(cm.clone());
            }
            if let Some(ucm) = &candidate.url_context_metadata {
                self.accumulated_url_context_metadata.push(ucm.clone());
            }
            if let Some(lp) = &candidate.logprobs_result {
                self.accumulated_logprobs.push(lp.clone());
            }
            if let Some(tc) = candidate.token_count {
                self.accumulated_token_counts.push(tc);
            }
            // finishReason 累积到末态(末 chunk emit completed 时用)
            // **粘性保护**(sanity check 报告):INTERRUPTED 是 C3b/C4 cap-trip /
            // upstream-Err 路径标记的"已宣告中断",不能被后续合法 chunk 的
            // finishReason="STOP" 覆盖回 completed —— 那会让"宣告 incomplete 后又
            // 静默 recover"成 silent truncation 的孪生 bug。
            if let Some(fr) = &candidate.finish_reason {
                if self.final_finish_reason.as_deref() != Some(FINISH_INTERRUPTED) {
                    self.final_finish_reason = Some(fr.clone());
                }
            }
        }
        // P0-F:promptFeedback.blockReason — Gemini 拒 user prompt(safety 拦截)→
        // 设 prompt_block_reason,emit_completed 转 status=incomplete + reason=
        // content_filter + error.code=prompt_blocked,客户端能区分"prompt 被拦"
        // 跟"模型不响应"。
        if let Some(pf) = &gemini.prompt_feedback {
            if let Some(br) = &pf.block_reason {
                if !br.is_empty() && self.prompt_block_reason.is_none() {
                    self.prompt_block_reason = Some(br.clone());
                }
            }
            if let Some(sr) = &pf.safety_ratings {
                self.prompt_feedback_safety.extend(sr.iter().cloned());
            }
        }
        // usageMetadata 累积到末态(P1-A 修复:补 tool_use_prompt + traffic_type +
        // 详细 prompt token 分类,不丢任何上游 usage 字段)
        if let Some(um) = gemini.usage_metadata {
            let mut input_details = json!({
                "cached_tokens": um.cached_content_token_count.unwrap_or(0),
            });
            if let Some(tu) = um.tool_use_prompt_token_count {
                if let Some(obj) = input_details.as_object_mut() {
                    obj.insert("tool_use_prompt_tokens".into(), json!(tu));
                }
            }
            let mut usage = json!({
                "input_tokens": um.prompt_token_count,
                "output_tokens": um.candidates_token_count,
                "total_tokens": um.total_token_count,
                "input_tokens_details": input_details,
                "output_tokens_details": {
                    "reasoning_tokens": um.thoughts_token_count.unwrap_or(0),
                },
            });
            if let Some(tt) = um.traffic_type {
                if let Some(obj) = usage.as_object_mut() {
                    obj.insert("traffic_type".into(), Value::String(tt));
                }
            }
            self.final_usage = Some(usage);
        }
    }

    // ───── lifecycle ─────

    fn emit_lifecycle_open(&mut self, out: &mut Vec<u8>) {
        self.lifecycle_opened = true;
        let mut envelope = self.build_envelope("in_progress");
        envelope["output"] = json!([]);
        envelope["usage"] = Value::Null;
        envelope["incomplete_details"] = Value::Null;
        envelope["error"] = Value::Null;
        let created = json!({"type": "response.created", "response": envelope.clone()});
        let in_progress = json!({"type": "response.in_progress", "response": envelope});
        emit_event(out, &mut self.sequence_number, "response.created", created);
        emit_event(
            out,
            &mut self.sequence_number,
            "response.in_progress",
            in_progress,
        );
    }

    /// 上游 4xx/5xx → 构造合规 Responses SSE failure event(`response.failed`)。
    /// envelope.error 用 OpenAI Responses API 的 `{code, message}` 形状,Codex.app
    /// 客户端能正确识别 + 显示。`type` 字段额外塞 upstream HTTP status 方便诊断。
    ///
    /// **WARNING — 仅 pre-stream 调用**:本方法无脑标 `completed_emitted = true`,**不会**
    /// flush 任何 pending output_item.done / content_part.done。如果上游已 emit 过部分
    /// output_*.delta 后中途调 emit_failure,客户端会看到孤立的 delta + failed,行为
    /// 未定义。当前唯一调用点是 `convert_gemini_error_to_responses_failure_stream`
    /// (fresh converter,4xx/5xx 入口),mid-stream 失败请走 `final_finish_reason =
    /// FINISH_INTERRUPTED` 让 `finish()` emit incomplete envelope。
    pub(super) fn emit_failure(&mut self, code: &str, message: &str, http_status: u16) -> Vec<u8> {
        debug_assert!(
            !self.completed_emitted,
            "emit_failure called after terminal event — would skip pending item closures"
        );
        let mut out = Vec::new();
        if !self.lifecycle_opened {
            self.emit_lifecycle_open(&mut out);
        }
        let mut envelope = self.build_envelope("failed");
        envelope["output"] = json!([]);
        envelope["usage"] = Value::Null;
        envelope["incomplete_details"] = Value::Null;
        envelope["error"] = json!({
            "code": code,
            "message": message,
            "type": format!("upstream_http_{http_status}"),
        });
        let payload = json!({"type": "response.failed", "response": envelope});
        emit_event(
            &mut out,
            &mut self.sequence_number,
            "response.failed",
            payload,
        );
        self.completed_emitted = true;
        out
    }

    fn emit_completed(&mut self, out: &mut Vec<u8>) {
        if self.completed_emitted {
            return;
        }
        // **关键防御**(C4 + C5 + P0-F):None / FINISH_INTERRUPTED / prompt_block_reason
        // 都映射成 "incomplete"。
        // - 上游断流 / 5xx mid-stream / 网络 reset → final_finish_reason 维持 None
        //   → 强制 "incomplete" 防 silent truncation
        // - prompt_block_reason 非空(Gemini 拒 user prompt safety 拦截)→ "incomplete"
        //   + reason="content_filter" + error.code="prompt_blocked"
        let prompt_blocked = self.prompt_block_reason.is_some();
        let status = if prompt_blocked {
            "incomplete"
        } else {
            match self.final_finish_reason.as_deref() {
                Some("STOP") => "completed",
                Some("MAX_TOKENS")
                | Some("SAFETY")
                | Some("RECITATION")
                | Some("BLOCKLIST")
                | Some("PROHIBITED_CONTENT")
                | Some("SPII")
                | Some("IMAGE_SAFETY")
                | Some("IMAGE_PROHIBITED_CONTENT") => "incomplete",
                Some(s) if s == FINISH_INTERRUPTED => "incomplete",
                None => "incomplete",
                _ => "completed",
            }
        };
        let mut envelope = self.build_envelope(status);

        // H3 修复:output[] 按 output_index 升序合并所有 items(message / reasoning /
        // function_call),保持跟客户端实际 stream 收到的事件顺序一致。
        // 旧实现假设 reasoning < message < function_calls 顺序固定,但 Gemini 多轮
        // text→fc→text 序列会破这条假设。
        let mut all_items: Vec<(u32, Value)> = Vec::new();
        all_items.extend(self.closed_messages.drain(..));
        all_items.extend(self.closed_reasonings.drain(..));
        all_items.extend(self.closed_other_items.drain(..));
        for fc in self.closed_function_calls.drain(..) {
            let mut item = json!({
                "type": "function_call",
                "id": fc.item_id,
                "call_id": fc.call_id,
                "name": fc.name,
                "arguments": fc.arguments_json_str,
                "status": "completed",
            });
            // namespace 字段(Codex.app dispatch MCP server 必填)
            if let Some(ns) = fc.namespace.as_ref() {
                item["namespace"] = Value::String(ns.clone());
            }
            all_items.push((fc.output_index, item));
        }
        all_items.sort_by_key(|(idx, _)| *idx);
        let output_items: Vec<Value> = all_items.into_iter().map(|(_, item)| item).collect();

        // 保存 session 历史(用于下一轮 previous_response_id 续话):
        // `prepare_request` 里已经把"上一轮历史 + 当前用户输入"放进 response_session.messages，
        // 这里补上本轮 assistant 输出后写入全局 ResponseSessionCache。
        if let Some(session) = self.response_session.take() {
            let mut messages = session.messages;
            if let Some(assistant) = Self::build_assistant_message_for_session(&output_items) {
                messages.push(assistant);
            }
            global_response_session_cache().save(&session.response_id, messages);
        }

        envelope["output"] = Value::Array(output_items);
        envelope["usage"] = self.final_usage.clone().unwrap_or(Value::Null);
        envelope["incomplete_details"] = if status == "incomplete" {
            // P0-F:prompt_blocked 优先(用户 prompt 被 safety 拦)→ content_filter
            let reason = if prompt_blocked {
                "content_filter"
            } else {
                match self.final_finish_reason.as_deref() {
                    Some("MAX_TOKENS") => "max_output_tokens",
                    Some("SAFETY")
                    | Some("RECITATION")
                    | Some("BLOCKLIST")
                    | Some("PROHIBITED_CONTENT")
                    | Some("SPII")
                    | Some("IMAGE_SAFETY")
                    | Some("IMAGE_PROHIBITED_CONTENT") => "content_filter",
                    Some(s) if s == FINISH_INTERRUPTED => "interrupted",
                    None => "interrupted",
                    _ => "interrupted",
                }
            };
            json!({"reason": reason})
        } else {
            Value::Null
        };
        // P0-F + C4 + C5:错误诊断字段
        // - prompt_blocked → error.code=prompt_blocked + 上游具体 block_reason
        // - upstream interrupted → upstream_interrupted
        envelope["error"] = if prompt_blocked {
            let br = self.prompt_block_reason.clone().unwrap_or_default();
            json!({
                "code": "prompt_blocked",
                "message": format!(
                    "Gemini upstream blocked the user prompt due to safety policy: {br}. \
                     See envelope.metadata.gemini.prompt_feedback for full safety_ratings."
                ),
                "upstream_block_reason": br,
            })
        } else if matches!(self.final_finish_reason.as_deref(), Some(s) if s == FINISH_INTERRUPTED)
            || (status == "incomplete" && self.final_finish_reason.is_none())
        {
            json!({
                "code": "upstream_interrupted",
                "message": "Gemini upstream stream ended without finishReason; treating as interrupted.",
            })
        } else {
            Value::Null
        };

        let payload = json!({"type": format!("response.{status}"), "response": envelope});
        let event_name = format!("response.{status}");
        emit_event(out, &mut self.sequence_number, &event_name, payload);
        self.completed_emitted = true;

        // 兼容 finish reason 跟 OpenAI 客户端期望(底层 _check_finish_reason 实证)—
        // 主要由 Codex.app 自己 mapping,我们 envelope 已正确,不再额外 tooling。
        let _ = map_finish_reason;
    }

    // ───── message item ─────

    fn open_message(&mut self, out: &mut Vec<u8>) {
        let item_id = format!("msg_{}", synthesize_id());
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": {
                    "type": "message",
                    "id": item_id,
                    "status": "in_progress",
                    "role": "assistant",
                    "content": [],
                },
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": 0,
                "part": { "type": "output_text", "text": "", "annotations": [] },
            }),
        );
        self.open_message = Some(OpenMessage {
            item_id,
            output_index,
            text_acc: String::new(),
            annotations_acc: Vec::new(),
        });
    }

    fn emit_text_delta(&mut self, out: &mut Vec<u8>, delta: &str) {
        let Some(msg) = self.open_message.as_mut() else {
            return;
        };
        msg.text_acc.push_str(delta);
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "item_id": msg.item_id,
                "output_index": msg.output_index,
                "content_index": 0,
                "delta": delta,
            }),
        );
    }

    fn emit_annotations(&mut self, out: &mut Vec<u8>, annotations: Vec<Value>) {
        let Some(msg) = self.open_message.as_mut() else {
            return;
        };
        for annotation in annotations {
            // emit + 累积(close 时塞进 message.content[0].annotations 整体)
            let payload = json!({
                "type": "response.output_text.annotation.added",
                "item_id": msg.item_id,
                "output_index": msg.output_index,
                "content_index": 0,
                "annotation_index": msg.annotations_acc.len(),
                "annotation": annotation.clone(),
            });
            msg.annotations_acc.push(annotation);
            emit_event(
                out,
                &mut self.sequence_number,
                "response.output_text.annotation.added",
                payload,
            );
        }
    }

    fn close_message(&mut self, out: &mut Vec<u8>) {
        let Some(msg) = self.open_message.take() else {
            return;
        };
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "item_id": msg.item_id,
                "output_index": msg.output_index,
                "content_index": 0,
                "text": msg.text_acc,
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "item_id": msg.item_id,
                "output_index": msg.output_index,
                "content_index": 0,
                "part": {
                    "type": "output_text",
                    "text": msg.text_acc,
                    "annotations": msg.annotations_acc,
                },
            }),
        );
        let item = json!({
            "type": "message",
            "id": msg.item_id,
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": msg.text_acc,
                "annotations": msg.annotations_acc,
            }],
        });
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": msg.output_index,
                "item": item.clone(),
            }),
        );
        self.closed_messages.push((msg.output_index, item));
    }

    // ───── reasoning item ─────

    fn open_reasoning(&mut self, out: &mut Vec<u8>) {
        let item_id = format!("rs_{}", synthesize_id());
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": {
                    "type": "reasoning",
                    "status": "in_progress",
                    "id": item_id,
                    "summary": [],
                    "content": null,
                    "encrypted_content": null,
                },
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.reasoning_summary_part.added",
            json!({
                "type": "response.reasoning_summary_part.added",
                "item_id": item_id,
                "output_index": output_index,
                "summary_index": 0,
                "part": { "type": "summary_text", "text": "" },
            }),
        );
        self.open_reasoning = Some(OpenReasoning {
            item_id,
            output_index,
            text_acc: String::new(),
        });
    }

    fn emit_reasoning_delta(&mut self, out: &mut Vec<u8>, delta: &str) {
        let Some(rs) = self.open_reasoning.as_mut() else {
            return;
        };
        rs.text_acc.push_str(delta);
        emit_event(
            out,
            &mut self.sequence_number,
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": rs.item_id,
                "output_index": rs.output_index,
                "summary_index": 0,
                "delta": delta,
            }),
        );
    }

    fn close_reasoning(&mut self, out: &mut Vec<u8>) {
        let Some(rs) = self.open_reasoning.take() else {
            return;
        };
        emit_event(
            out,
            &mut self.sequence_number,
            "response.reasoning_summary_text.done",
            json!({
                "type": "response.reasoning_summary_text.done",
                "item_id": rs.item_id,
                "output_index": rs.output_index,
                "summary_index": 0,
                "text": rs.text_acc,
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.reasoning_summary_part.done",
            json!({
                "type": "response.reasoning_summary_part.done",
                "item_id": rs.item_id,
                "output_index": rs.output_index,
                "summary_index": 0,
                "part": {
                    "type": "summary_text",
                    "text": rs.text_acc,
                },
            }),
        );
        let item = json!({
            "type": "reasoning",
            "status": "completed",
            "id": rs.item_id,
            "summary": [{ "type": "summary_text", "text": rs.text_acc }],
            "content": null,
            "encrypted_content": null,
        });
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": rs.output_index,
                "item": item.clone(),
            }),
        );
        self.closed_reasonings.push((rs.output_index, item));
    }

    // ───── function_call item ─────

    fn emit_function_call(
        &mut self,
        out: &mut Vec<u8>,
        name: &str,
        args: &Value,
        thought_signature: Option<&str>,
    ) {
        let item_id = format!("fc_{}", synthesize_id());
        // P1-B:thoughtSignature 编码进 call_id 让客户端 roundtrip 不丢
        // (LiteLLM _encode_tool_call_id_with_signature 模式)。
        // 用 `~~sig~~` 分隔符(JSON-safe + 跟 hex/base64 互补不冲突),
        // 请求侧 decode_tool_call_id_signature 反向拆。
        let call_id = match thought_signature {
            Some(sig) if !sig.is_empty() => {
                format!("call_{}~~sig~~{sig}", synthesize_id())
            }
            _ => format!("call_{}", synthesize_id()),
        };
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        // OpenAI function_call.arguments 是 JSON 字符串,Gemini 是结构化对象 → 序列化
        // (LOW from sanity check):跟 emit_event 的 C2 fix 一致,失败时至少 log。
        // 实际 serde_json::to_string(&Value) 几乎不可能失败(只有 NaN/Infinity 等
        // 非标准 number 才会 trip),但留 log 能帮 debug。
        let args_json_str = match serde_json::to_string(args) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    name,
                    "BUG: failed to serialize functionCall.args to JSON string; \
                     falling back to '{{}}'. This may produce a tool call with no args."
                );
                "{}".into()
            }
        };

        // **MCP namespace 字段**:如 function.name 来自 namespace 包装(从
        // `tool_namespace_map` 反查表查到),给 output_item.added / .done /
        // envelope item 全部加 `namespace: "mcp__<server>__"` —— Codex.app
        // 客户端 dispatch namespace 工具调用必填(2026-05-11 修)
        let namespace_for = self.lookup_namespace_for(name).map(str::to_owned);
        let build_item = |status: &str, arguments_str: &str| {
            let mut m = json!({
                "type": "function_call",
                "id": item_id,
                "call_id": call_id,
                "name": name,
                "arguments": arguments_str,
                "status": status,
            });
            if let Some(ns) = namespace_for.as_ref() {
                m["namespace"] = Value::String(ns.clone());
            }
            m
        };

        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": build_item("in_progress", ""),
            }),
        );
        // Gemini 一次性给完整 args(无增量),emit 单条 delta = 完整 args
        emit_event(
            out,
            &mut self.sequence_number,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": item_id,
                "output_index": output_index,
                "delta": args_json_str,
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.function_call_arguments.done",
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": item_id,
                "output_index": output_index,
                "arguments": args_json_str,
            }),
        );
        let item = build_item("completed", &args_json_str);
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item,
            }),
        );
        // P0-G:写 global ToolCallCache (call_id → name + arguments) 让下轮
        // function_call_output 即便 Codex.app 不重发 prior function_call 也能 lookup
        // (Codex.app 是 stateful client,默认依赖 server 维护 mapping)。复用项目
        // 已有 ResponsesAdapter converter.rs:665 同款模式。
        crate::responses::global_tool_call_cache().save(
            &call_id,
            crate::responses::ToolCallEntry {
                name: name.to_owned(),
                arguments: args_json_str.clone(),
            },
        );
        self.closed_function_calls.push(ClosedFunctionCall {
            item_id,
            output_index,
            call_id,
            name: name.to_owned(),
            arguments_json_str: args_json_str,
            namespace: namespace_for,
        });
    }

    // ───── inline_data / file_data 输出 (P0-E:模型多模态输出) ─────

    /// 模型在 response 里直接生成 inline_data (image/audio/video base64),
    /// 转 Responses output_item type="image_generation_call" + result 含
    /// mime_type + data。Codex.app 旧版可能不识别此 item 但 wire 上不丢。
    fn emit_inline_data(&mut self, out: &mut Vec<u8>, mime: &str, data: &str) {
        let item_id = format!("img_{}", synthesize_id());
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let item = json!({
            "type": "image_generation_call",
            "id": item_id,
            "status": "completed",
            "result": {
                "type": "inline_data",
                "mime_type": mime,
                "data": data,
            },
        });
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": item.clone(),
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item.clone(),
            }),
        );
        self.closed_other_items.push((output_index, item));
    }

    /// 模型 file_data 输出(Gemini 偶尔通过 fileUri 引用上游 Files API 存的文件)。
    fn emit_file_data(&mut self, out: &mut Vec<u8>, mime: &str, file_uri: &str) {
        let item_id = format!("file_{}", synthesize_id());
        let output_index = self.next_output_index;
        self.next_output_index += 1;
        let item = json!({
            "type": "file_search_call",
            "id": item_id,
            "status": "completed",
            "result": {
                "type": "file_data",
                "mime_type": mime,
                "file_uri": file_uri,
            },
        });
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": item.clone(),
            }),
        );
        emit_event(
            out,
            &mut self.sequence_number,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item.clone(),
            }),
        );
        self.closed_other_items.push((output_index, item));
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// ByteStream wrapper
// ═══════════════════════════════════════════════════════════════════════════

/// 上游错误 body 最大读取字节数。Gemini error 通常 <1KB;CDN HTML 错误页 / proxy
/// 异常体可能数 MB。无 cap → 失败请求并发时内存放大攻击面。截断后剩余 bytes 直接 drop
/// (上游已经表态错误,我们不需要 forward 完整 body,只需要 error message 给用户)。
const MAX_UPSTREAM_ERROR_BODY_BYTES: usize = 64 * 1024;

/// 用户可见 error message 截断阈值。Responses error envelope 无文档化硬上限,选 2000
/// 给操作者足够诊断信息(stack trace / quota detail)又不至于撑爆 SSE event。
const MAX_USER_ERROR_MESSAGE_CHARS: usize = 2000;

/// 上游 4xx/5xx 错误 → Responses SSE failure 流。
///
/// **不能直接透传 Gemini raw JSON 4xx body** — Codex.app 期待 SSE event 流,
/// 收到非 SSE raw JSON 不知道怎 parse → 卡 Thinking 永不结束(silent failure)。
/// 改成构造合规 Responses SSE:`response.created`(in_progress)→ `response.failed`
/// 含 error code + message。客户端能识别 + 显示用户级错误。
///
/// 错误分类(status code + Gemini `error.status` + message 关键词):
/// - 401 / UNAUTHENTICATED → `auth_error`
/// - 403 / PERMISSION_DENIED → `permission_denied`(API 未启用 / billing / region)
/// - 408 / 504 → `timeout`(retry 可能有效)
/// - 429 + RESOURCE_EXHAUSTED → `quota_exceeded`(retry 短期内无效)
/// - 429 其他 → `rate_limited`(指数退避 retry)
/// - 400 + SAFETY/blockReason → `content_filter`
/// - 400 其他 → `bad_request`
/// - 503 → `service_unavailable`
/// - 502 / 5xx 其他 → `server_error`
/// - 其他 → `upstream_error`
///
/// 防御性失败处理(本身**不能**埋新 silent failure):
/// - upstream ByteStream Err mid-read → 把 transport error 拼进用户 message,降级
///   `code = "upstream_transport_error"`(operator log 也会记)
/// - body 超过 [`MAX_UPSTREAM_ERROR_BODY_BYTES`] → 截断,在 message 后缀标 `…(truncated)`
/// - body 非 UTF-8 → `from_utf8_lossy` 替换,在 message 后缀标 `(non-UTF-8 body)`
/// - 任何分支都**保证** emit `response.failed`,客户端永远不会卡住
pub fn convert_gemini_error_to_responses_failure_stream(
    upstream_status: http::StatusCode,
    upstream_stream: ByteStream,
    original_request: Option<Value>,
) -> ByteStream {
    use futures_util::stream::StreamExt;
    let status_u16 = upstream_status.as_u16();
    let s: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> = Box::pin(
        stream::unfold(
            (upstream_stream, original_request, false),
            move |(mut input, orig, finished)| async move {
                if finished {
                    return None;
                }
                // 收上游 error body, cap 防 DoS, 记录 transport err 防 silent swallow
                let mut body = Vec::with_capacity(1024);
                let mut transport_err: Option<String> = None;
                let mut truncated = false;
                while let Some(chunk) = input.next().await {
                    match chunk {
                        Ok(b) => {
                            let remaining =
                                MAX_UPSTREAM_ERROR_BODY_BYTES.saturating_sub(body.len());
                            if remaining == 0 {
                                truncated = true;
                                break;
                            }
                            let take = b.len().min(remaining);
                            body.extend_from_slice(&b[..take]);
                            if take < b.len() {
                                truncated = true;
                                break;
                            }
                        }
                        Err(e) => {
                            transport_err = Some(e.to_string());
                            break;
                        }
                    }
                }
                let was_lossy = std::str::from_utf8(&body).is_err();
                let raw_text = String::from_utf8_lossy(&body).into_owned();
                let parsed: Option<Value> = serde_json::from_str(&raw_text).ok();

                // 提 Gemini error.message,支持 object {"error":{...}} 与 array [{"error":{...}}] 两种 shape
                let extract_message = |v: &Value| -> Option<String> {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(String::from)
                };
                let extract_status = |v: &Value| -> Option<String> {
                    v.get("error")
                        .and_then(|e| e.get("status"))
                        .and_then(|s| s.as_str())
                        .map(String::from)
                };
                let (upstream_message, upstream_status_str) = match parsed.as_ref() {
                    Some(v) if v.is_object() => (extract_message(v), extract_status(v)),
                    Some(v) => match v.as_array().and_then(|a| a.first()) {
                        Some(first) => (extract_message(first), extract_status(first)),
                        None => (None, None),
                    },
                    None => (None, None),
                };

                // ── 分类 ──
                let (mut code, mut kind): (&str, &str) = match status_u16 {
                    401 => ("auth_error", "Authentication failed"),
                    403 => {
                        // Gemini 403 多是 API 未启用 / billing / region — 跟 401 ("API key 错") 区分
                        if upstream_status_str.as_deref() == Some("UNAUTHENTICATED") {
                            ("auth_error", "Authentication failed")
                        } else if upstream_message
                            .as_deref()
                            .map(|m| {
                                let lower = m.to_ascii_lowercase();
                                // **Cloud Code OAuth 路径专用**:免费 tier per-account
                                // 日配额耗尽,Google 上游返 403 PERMISSION_DENIED + message
                                // 含 cloudaicompanion **AND** quota/limit 关键词(silent-failure-
                                // hunter M4 修:cloudaicompanion 单独 substring 太宽,会把
                                // "Project xxx is not authorized" / "API not enabled" 等
                                // 真 permission 错误误归 quota,误导用户等次日重试)
                                let has_companion = lower.contains("cloudaicompanion");
                                let has_quota_keyword =
                                    lower.contains("quota") || lower.contains("limit");
                                (has_companion && has_quota_keyword)
                                    || (lower.contains("quota") && lower.contains("daily"))
                                    || lower.contains("perdayperuser")
                            })
                            .unwrap_or(false)
                        {
                            (
                                "quota_exceeded",
                                "Free-tier daily quota exhausted; retry tomorrow or set up paid GCP project",
                            )
                        } else {
                            (
                                "permission_denied",
                                "Permission denied (API not enabled, billing, or region restricted)",
                            )
                        }
                    }
                    408 => ("timeout", "Upstream request timed out"),
                    400 => {
                        // INVALID_ARGUMENT 也覆盖 safety block;message 关键词探测
                        let is_safety = upstream_message
                            .as_deref()
                            .map(|m| {
                                let lower = m.to_ascii_lowercase();
                                lower.contains("safety")
                                    || lower.contains("blocked")
                                    || lower.contains("block_reason")
                            })
                            .unwrap_or(false);
                        if is_safety {
                            (
                                "content_filter",
                                "Content blocked by upstream safety filter",
                            )
                        } else {
                            ("bad_request", "Bad request to upstream")
                        }
                    }
                    429 => {
                        let is_quota = upstream_status_str.as_deref() == Some("RESOURCE_EXHAUSTED")
                            || upstream_message
                                .as_deref()
                                .map(|m| {
                                    let lower = m.to_ascii_lowercase();
                                    lower.contains("quota") || lower.contains("resource_exhausted")
                                })
                                .unwrap_or(false);
                        if is_quota {
                            ("quota_exceeded", "Quota exceeded")
                        } else {
                            ("rate_limited", "Rate limited")
                        }
                    }
                    503 => ("service_unavailable", "Upstream service unavailable"),
                    504 => ("timeout", "Upstream gateway timed out"),
                    s if (500..600).contains(&s) => ("server_error", "Upstream server error"),
                    _ => ("upstream_error", "Upstream returned an error"),
                };

                // transport error 覆盖分类:body 不完整,前面提到的 message 不可信
                if transport_err.is_some() {
                    code = "upstream_transport_error";
                    kind = "Transport error reading upstream error body";
                }

                // ── 拼用户可见 message ──
                let mut error_message = match &upstream_message {
                    Some(m) => format!("{kind} (HTTP {status_u16}): {m}"),
                    None => format!("{kind} (HTTP {status_u16})"),
                };
                if let Some(te) = &transport_err {
                    error_message.push_str(&format!(" [transport error: {te}]"));
                }
                if truncated {
                    error_message.push_str(" [body truncated]");
                }
                if was_lossy {
                    error_message.push_str(" [non-UTF-8 body]");
                }
                if error_message.chars().count() > MAX_USER_ERROR_MESSAGE_CHARS {
                    let truncated_msg: String = error_message
                        .chars()
                        .take(MAX_USER_ERROR_MESSAGE_CHARS)
                        .collect();
                    error_message = format!("{truncated_msg}…");
                }

                // operator-side log:让 5xx 显眼,4xx 走 warn
                if (500..600).contains(&status_u16) || transport_err.is_some() {
                    tracing::error!(
                        upstream_status = status_u16,
                        code,
                        truncated,
                        was_lossy,
                        transport_err = transport_err.as_deref().unwrap_or(""),
                        "gemini upstream returned error; synthesized response.failed for client"
                    );
                } else {
                    tracing::warn!(
                        upstream_status = status_u16,
                        code,
                        upstream_status_str = upstream_status_str.as_deref().unwrap_or(""),
                        message_preview = upstream_message
                            .as_deref()
                            .unwrap_or("")
                            .chars()
                            .take(200)
                            .collect::<String>(),
                        "gemini upstream returned error; synthesized response.failed for client"
                    );
                }

                let mut conv = GeminiToResponsesConverter::new(orig, None);
                let out = conv.emit_failure(code, &error_message, status_u16);
                Some((Ok(Bytes::from(out)), (input, None, true)))
            },
        ),
    );
    s
}

/// 包装 Gemini SSE byte stream → Responses SSE byte stream。
pub fn convert_gemini_to_responses_stream(
    input: ByteStream,
    original_request: Option<Value>,
    response_session: Option<ResponseSessionPlan>,
) -> ByteStream {
    struct State {
        input: ByteStream,
        conv: GeminiToResponsesConverter,
        finished: bool,
    }
    let init = State {
        input,
        conv: GeminiToResponsesConverter::new(original_request, response_session),
        finished: false,
    };
    let s: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> =
        Box::pin(stream::unfold(init, |mut s| async move {
            loop {
                if s.finished {
                    return None;
                }
                match s.input.next().await {
                    Some(Ok(chunk)) => {
                        let out = s.conv.feed(&chunk);
                        if !out.is_empty() {
                            return Some((Ok(Bytes::from(out)), s));
                        }
                    }
                    Some(Err(e)) => {
                        // **C4 修复**:上游 mid-stream Err 不能让 finish() 走默认路径
                        // 以"completed"结尾(那会让客户端把半截响应当成完整响应 — silent
                        // truncation)。先标 INTERRUPTED,finish() 看到这个值会发
                        // status=incomplete + reason=interrupted + error=upstream_interrupted。
                        tracing::warn!(error = %e, "gemini upstream stream errored mid-stream");
                        s.conv.final_finish_reason = Some(FINISH_INTERRUPTED.to_owned());
                        let final_out = s.conv.finish();
                        s.finished = true;
                        if !final_out.is_empty() {
                            return Some((Ok(Bytes::from(final_out)), s));
                        }
                        return Some((Err(e), s));
                    }
                    None => {
                        let final_out = s.conv.finish();
                        s.finished = true;
                        if !final_out.is_empty() {
                            return Some((Ok(Bytes::from(final_out)), s));
                        }
                        return None;
                    }
                }
            }
        }));
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use serde_json::json;

    use crate::responses::global_response_session_cache;

    fn drive_to_events(conv: &mut GeminiToResponsesConverter, chunks: &[&[u8]]) -> Vec<String> {
        let mut all: Vec<u8> = Vec::new();
        for c in chunks {
            all.extend_from_slice(&conv.feed(c));
        }
        all.extend_from_slice(&conv.finish());
        let s = String::from_utf8(all).unwrap();
        s.split("\n\n")
            .filter(|x| !x.is_empty())
            .map(String::from)
            .collect()
    }

    fn parse_event(event: &str) -> (String, Value) {
        let mut name = String::new();
        let mut data = String::new();
        for line in event.lines() {
            if let Some(n) = line.strip_prefix("event: ") {
                name = n.to_owned();
            }
            if let Some(d) = line.strip_prefix("data: ") {
                data = d.to_owned();
            }
        }
        (name, serde_json::from_str(&data).unwrap_or(Value::Null))
    }

    fn input_stream(bytes: &'static [u8]) -> ByteStream {
        Box::pin(stream::iter(vec![Ok(Bytes::from_static(bytes))]))
    }

    #[tokio::test]
    async fn stream_completion_saves_response_session_for_resume() {
        let response_id = format!("resp_gn_session_{}", synthesize_id());
        let session = ResponseSessionPlan {
            response_id: response_id.clone(),
            messages: vec![json!({"role":"user","content":"hello from previous turn"})],
        };
        let raw = br#"data: {"candidates":[{"content":{"parts":[{"text":"hi from gemini"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}}

"#;
        let mut converted =
            convert_gemini_to_responses_stream(input_stream(raw), None, Some(session));
        while let Some(chunk) = converted.next().await {
            let _ = chunk.expect("stream chunk should be valid");
        }

        let saved = global_response_session_cache().get(&response_id).expect(
            "gemini_native stream must persist response session for previous_response_id resume",
        );
        assert_eq!(saved.len(), 2, "保存后应为 1 条用户 + 1 条 assistant");
        assert_eq!(saved[0]["role"], "user");
        assert_eq!(saved[1]["role"], "assistant");
        assert_eq!(saved[1]["content"], "hi from gemini");
    }

    #[test]
    fn lifecycle_emits_created_in_progress_completed() {
        // 最简流程:文本 chunk + finishReason → created/in_progress/output_item.added/
        // content_part.added/output_text.delta/output_text.done/content_part.done/
        // output_item.done/completed
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1,"totalTokenCount":2}}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        // 必须包含 lifecycle 事件
        assert!(names.contains(&"response.created".into()));
        assert!(names.contains(&"response.in_progress".into()));
        assert!(names.contains(&"response.output_item.added".into()));
        assert!(names.contains(&"response.content_part.added".into()));
        assert!(names.contains(&"response.output_text.delta".into()));
        assert!(names.contains(&"response.output_text.done".into()));
        assert!(names.contains(&"response.content_part.done".into()));
        assert!(names.contains(&"response.output_item.done".into()));
        assert!(names.contains(&"response.completed".into()));
        // sequence_number 单调递增
        for (i, e) in events.iter().enumerate() {
            let (_, v) = parse_event(e);
            assert_eq!(
                v["sequence_number"], i,
                "event {i} sequence_number 必须 = i"
            );
        }
    }

    #[test]
    fn text_delta_accumulates_into_completed_envelope_output() {
        let chunk1 = br#"data: {"candidates":[{"content":{"parts":[{"text":"He"}]}}]}

"#;
        let chunk2 = br#"data: {"candidates":[{"content":{"parts":[{"text":"llo"}]}}]}

"#;
        let chunk3 = br#"data: {"candidates":[{"finishReason":"STOP"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk1, chunk2, chunk3]);
        // 找 completed envelope
        let completed = events
            .iter()
            .map(|e| parse_event(e))
            .find(|(n, _)| n == "response.completed")
            .expect("response.completed 应存在");
        let output = &completed.1["response"]["output"];
        assert_eq!(output[0]["type"], "message");
        assert_eq!(
            output[0]["content"][0]["text"], "Hello",
            "完整文本应在 envelope output 中"
        );
    }

    #[test]
    fn function_call_emits_separate_output_item() {
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"search","args":{"q":"weather"}}}]},"finishReason":"STOP"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        // 必含 function_call_arguments.delta + done
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        assert!(names.contains(&"response.function_call_arguments.delta".into()));
        assert!(names.contains(&"response.function_call_arguments.done".into()));
        // completed envelope 的 output[] 含 function_call item
        let completed = events
            .iter()
            .map(|e| parse_event(e))
            .find(|(n, _)| n == "response.completed")
            .unwrap();
        let output = &completed.1["response"]["output"];
        let fc = output
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["type"] == "function_call")
            .unwrap();
        assert_eq!(fc["name"], "search");
        // arguments 必须是 JSON 字符串(OpenAI 兼容)
        let args_str = fc["arguments"].as_str().unwrap();
        let args: Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(args["q"], "weather");
        assert!(fc["call_id"].as_str().unwrap().starts_with("call_"));
    }

    #[test]
    fn reasoning_text_emits_summary_events() {
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"text":"thinking step","thought":true},{"text":"answer"}]},"finishReason":"STOP"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        assert!(names.contains(&"response.reasoning_summary_part.added".into()));
        assert!(names.contains(&"response.reasoning_summary_text.delta".into()));
        assert!(names.contains(&"response.reasoning_summary_text.done".into()));
        // envelope output 既有 reasoning 又有 message
        let completed = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.completed")
            .unwrap();
        let output = completed.1["response"]["output"].as_array().unwrap();
        assert!(output.iter().any(|i| i["type"] == "reasoning"));
        assert!(output.iter().any(|i| i["type"] == "message"));
        // reasoning summary text 应该是 "thinking step"
        let r = output.iter().find(|i| i["type"] == "reasoning").unwrap();
        assert_eq!(r["summary"][0]["text"], "thinking step");
    }

    #[test]
    fn grounding_metadata_emits_annotation_added_events() {
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"text":"NYC weather"}]},"groundingMetadata":{"groundingChunks":[{"web":{"uri":"https://w.com","title":"W"}}],"groundingSupports":[{"segment":{"startIndex":0,"endIndex":11},"groundingChunkIndices":[0]}]},"finishReason":"STOP"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        assert!(names.contains(&"response.output_text.annotation.added".into()));
        // envelope output[].content[0].annotations 含 url_citation
        let completed = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.completed")
            .unwrap();
        let output = completed.1["response"]["output"].as_array().unwrap();
        let msg = output.iter().find(|i| i["type"] == "message").unwrap();
        let annos = msg["content"][0]["annotations"].as_array().unwrap();
        assert_eq!(annos[0]["type"], "url_citation");
        assert_eq!(annos[0]["url_citation"]["url"], "https://w.com");
    }

    #[test]
    fn split_chunks_buffered_correctly() {
        let part1 = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi";
        let part2 = b"\"}]}}]}\n\n";
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[part1, part2]);
        // 注:此 chunk 没 finishReason → emit_completed 走 "incomplete" 路径(C5 修复后行为)。
        // 但 message item 的 text 内容仍要正确累积进 envelope output[]。
        let last = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.incomplete" || n == "response.completed")
            .unwrap();
        let output = &last.1["response"]["output"];
        assert_eq!(output[0]["content"][0]["text"], "Hi");
    }

    #[test]
    fn crlf_sse_boundary_recognized() {
        // C3 修复:Google `alt=sse` 经常 emit `\r\n\r\n` 边界,只识 `\n\n` 会让
        // 整个流被 buffer 到结束才一次性 process(streaming → batch 退化)。
        let chunk =
            b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"crlf\"}]}}]}\r\n\r\n";
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        // 必须能 emit 出来(有 output_text.delta + 终态事件)
        assert!(names.contains(&"response.output_text.delta".into()));
        assert!(
            names.contains(&"response.completed".into())
                || names.contains(&"response.incomplete".into()),
            "CRLF 边界必须被识别;实际 events:{names:?}"
        );
    }

    #[test]
    fn upstream_missing_finish_reason_treated_as_interrupted() {
        // C5 修复:Gemini 上游断流没 emit finishReason → final_finish_reason=None,
        // 必须映射成 "incomplete" + reason="interrupted" + error 字段,防 silent truncation。
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"text":"half"}]}}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let last = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.incomplete")
            .expect("missing finishReason 必须 emit response.incomplete,实际未 emit");
        assert_eq!(last.1["response"]["status"], "incomplete");
        assert_eq!(
            last.1["response"]["incomplete_details"]["reason"], "interrupted",
            "missing finishReason → reason=interrupted"
        );
        assert_eq!(
            last.1["response"]["error"]["code"], "upstream_interrupted",
            "必须附 error 字段帮客户端诊断"
        );
    }

    #[test]
    fn accumulated_json_overflow_aborts_with_interrupted() {
        // C3b 修复:malformed JSON 持续累积超 256 KiB → 标 INTERRUPTED + drop buffer。
        // 构造一个永远解不开的 JSON(开括号没闭合)+ 大 payload 触发 cap。
        let bad_chunk: Vec<u8> = {
            let mut v = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"".to_vec();
            v.extend(std::iter::repeat(b'x').take(MAX_ACCUMULATED_JSON_BYTES + 1024));
            v.extend_from_slice(b"\n\n"); // 终结这个 SSE event(JSON 仍未闭合)
            v
        };
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[&bad_chunk]);
        // 必须 emit incomplete(防 OOM 后假装 completed)
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        assert!(
            names.contains(&"response.incomplete".into()),
            "accumulated JSON 超 cap 必须强制 incomplete,实际:{names:?}"
        );
    }

    #[test]
    fn envelope_includes_original_request_fields() {
        // tools / instructions / temperature 必须从 original_request 回灌
        let original = json!({
            "model":"gemini-3.1-pro-preview",
            "instructions":"You are helpful.",
            "tools":[{"type":"function","name":"test_fn","parameters":{"type":"object"}}],
            "temperature": 0.5,
            "tool_choice": "auto"
        });
        let chunk =
            br#"data: {"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(Some(original), None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let created = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.created")
            .unwrap();
        let env = &created.1["response"];
        assert_eq!(env["instructions"], "You are helpful.");
        assert_eq!(env["temperature"], 0.5);
        assert_eq!(env["tool_choice"], "auto");
        let tools = env["tools"].as_array().unwrap();
        assert_eq!(
            tools[0]["name"], "test_fn",
            "envelope.tools 必须是 Codex.app 原始 tools"
        );
    }

    #[test]
    fn max_tokens_finish_reason_emits_incomplete_status() {
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"text":"x"}]},"finishReason":"MAX_TOKENS"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let last = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.incomplete")
            .expect("MAX_TOKENS → response.incomplete");
        assert_eq!(last.1["response"]["status"], "incomplete");
        assert_eq!(
            last.1["response"]["incomplete_details"]["reason"],
            "max_output_tokens"
        );
    }

    #[test]
    fn safety_finish_reason_emits_incomplete_with_content_filter() {
        let chunk = br#"data: {"candidates":[{"finishReason":"SAFETY"}]}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let last = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.incomplete")
            .unwrap();
        assert_eq!(
            last.1["response"]["incomplete_details"]["reason"],
            "content_filter"
        );
    }

    #[test]
    fn usage_metadata_appears_in_completed_envelope() {
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"text":"hi"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":100,"candidatesTokenCount":50,"totalTokenCount":150,"thoughtsTokenCount":25,"cachedContentTokenCount":80}}

"#;
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[chunk]);
        let completed = events
            .iter()
            .map(|s| parse_event(s.as_str()))
            .find(|(n, _)| n == "response.completed")
            .unwrap();
        let usage = &completed.1["response"]["usage"];
        assert_eq!(usage["input_tokens"], 100);
        assert_eq!(usage["output_tokens"], 50);
        assert_eq!(usage["total_tokens"], 150);
        assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 25);
        assert_eq!(usage["input_tokens_details"]["cached_tokens"], 80);
    }

    #[test]
    fn no_upstream_data_still_emits_lifecycle_and_terminal() {
        // 极端情况:上游断流没发任何 chunk → 客户端不能卡死。
        // C5 修复后:无任何 chunk 必须 emit incomplete(不能假装 completed)。
        let mut conv = GeminiToResponsesConverter::new(None, None);
        let events = drive_to_events(&mut conv, &[]);
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        assert!(names.contains(&"response.created".into()));
        assert!(
            names.contains(&"response.incomplete".into()),
            "无 upstream data 必须 emit incomplete(防 silent truncation),实际:{names:?}"
        );
    }

    #[test]
    fn stream_wrapper_end_to_end() {
        let upstream_bytes = Bytes::from(
            r#"data: {"candidates":[{"content":{"parts":[{"text":"end-to-end"}]},"finishReason":"STOP"}]}

"#,
        );
        let upstream: ByteStream = Box::pin(stream::iter(vec![Ok(upstream_bytes)]));
        let mut output_stream = convert_gemini_to_responses_stream(upstream, None, None);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all_bytes = Vec::new();
        runtime.block_on(async {
            while let Some(item) = output_stream.next().await {
                all_bytes.extend_from_slice(&item.unwrap());
            }
        });
        let s = String::from_utf8(all_bytes).unwrap();
        assert!(s.contains("event: response.created"));
        assert!(s.contains("event: response.completed"));
        assert!(s.contains("end-to-end"));
    }

    fn drive_failure_stream(status: u16, body: &str) -> String {
        let upstream: ByteStream = Box::pin(stream::iter(vec![Ok(Bytes::from(body.to_owned()))]));
        let mut s = convert_gemini_error_to_responses_failure_stream(
            http::StatusCode::from_u16(status).unwrap(),
            upstream,
            None,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all = Vec::new();
        runtime.block_on(async {
            while let Some(item) = s.next().await {
                all.extend_from_slice(&item.unwrap());
            }
        });
        String::from_utf8(all).unwrap()
    }

    #[test]
    fn failure_stream_429_quota_exceeded() {
        let body = r#"{"error":{"code":429,"message":"Quota exceeded for quota metric 'Generate Content API requests per minute' and limit 'GenerateContent request limit per minute' of service 'generativelanguage.googleapis.com' for consumer 'project_number:xxx'.","status":"RESOURCE_EXHAUSTED"}}"#;
        let s = drive_failure_stream(429, body);
        assert!(s.contains("event: response.created"));
        assert!(s.contains("event: response.in_progress"));
        assert!(s.contains("event: response.failed"));
        assert!(s.contains("\"code\":\"quota_exceeded\""));
        assert!(s.contains("\"type\":\"upstream_http_429\""));
        assert!(s.contains("Quota exceeded"));
    }

    #[test]
    fn failure_stream_401_auth_error() {
        let body = r#"{"error":{"code":401,"message":"API key not valid. Please pass a valid API key.","status":"UNAUTHENTICATED"}}"#;
        let s = drive_failure_stream(401, body);
        assert!(s.contains("\"code\":\"auth_error\""));
        assert!(s.contains("API key not valid"));
        assert!(s.contains("event: response.failed"));
    }

    #[test]
    fn failure_stream_500_server_error_with_unparseable_body() {
        // 上游 5xx 经常返非 JSON HTML 错误页;不能崩,要降级到 generic message
        let s = drive_failure_stream(500, "<html>Internal Server Error</html>");
        assert!(s.contains("\"code\":\"server_error\""));
        assert!(s.contains("\"type\":\"upstream_http_500\""));
        // 没有 upstream message 时,至少有 status code
        assert!(s.contains("HTTP 500"));
    }

    #[test]
    fn failure_stream_429_rate_limited_when_no_quota_keyword() {
        // 429 但 message/status 都不含 quota 关键词 → 应分类为 rate_limited 而非 quota_exceeded
        let body = r#"{"error":{"code":429,"message":"Too many concurrent requests","status":"UNAVAILABLE"}}"#;
        let s = drive_failure_stream(429, body);
        assert!(s.contains("\"code\":\"rate_limited\""));
        assert!(!s.contains("\"code\":\"quota_exceeded\""));
        assert!(s.contains("Too many concurrent requests"));
    }

    #[test]
    fn failure_stream_403_cloudaicompanion_without_quota_keyword_stays_permission_denied() {
        // **silent-failure-hunter M4 防御**:cloudaicompanion 单独 substring 不该归 quota,
        // 必须同时含 quota/limit 关键词。`Project xxx is not authorized` / API not enabled
        // 等真 permission 错应保持 permission_denied 让用户检查 billing
        let body = r#"{"error":{"code":403,"message":"cloudaicompanionProject 'xxx-default-1234' is not authorized to access this resource.","status":"PERMISSION_DENIED"}}"#;
        let s = drive_failure_stream(403, body);
        assert!(s.contains("\"code\":\"permission_denied\""));
        assert!(!s.contains("\"code\":\"quota_exceeded\""));
    }

    #[test]
    fn failure_stream_403_cloudaicompanion_quota_classifies_as_quota_exceeded() {
        // **silent-failure-hunter H4 修**:Cloud Code OAuth 路径下 403 +
        // cloudaicompanionProject 是免费 tier per-account 日配额耗尽 — 不是
        // permission 问题,不该让用户去检查 billing。应分类 quota_exceeded
        // 让 client UI 显示"次日重置"
        let body = r#"{"error":{"code":403,"message":"Quota exceeded for cloudaicompanionProject 'xxx-default-1234'.","status":"PERMISSION_DENIED"}}"#;
        let s = drive_failure_stream(403, body);
        assert!(s.contains("\"code\":\"quota_exceeded\""));
        assert!(s.contains("Free-tier daily quota exhausted"));
        // 不应误归 permission_denied
        assert!(!s.contains("\"code\":\"permission_denied\""));
    }

    #[test]
    fn failure_stream_403_permission_denied_distinct_from_401() {
        // 403 PERMISSION_DENIED 应区分于 401 auth_error(用户不会被误导去检查 API key)
        let body = r#"{"error":{"code":403,"message":"Generative Language API has not been used in project xxx before or it is disabled.","status":"PERMISSION_DENIED"}}"#;
        let s = drive_failure_stream(403, body);
        assert!(s.contains("\"code\":\"permission_denied\""));
        assert!(s.contains("API not enabled, billing, or region restricted"));
        assert!(s.contains("\"type\":\"upstream_http_403\""));
    }

    #[test]
    fn failure_stream_403_unauthenticated_keeps_auth_error() {
        // 403 但 status=UNAUTHENTICATED(罕见但 Gemini 偶尔这么返)→ auth_error
        let body = r#"{"error":{"code":403,"message":"Invalid auth credential.","status":"UNAUTHENTICATED"}}"#;
        let s = drive_failure_stream(403, body);
        assert!(s.contains("\"code\":\"auth_error\""));
    }

    #[test]
    fn failure_stream_400_safety_block_emits_content_filter() {
        // 400 + safety 关键词 → content_filter,跟普通 schema 错区分
        let body = r#"{"error":{"code":400,"message":"Request contains content blocked by safety filter (HARM_CATEGORY_DANGEROUS).","status":"INVALID_ARGUMENT"}}"#;
        let s = drive_failure_stream(400, body);
        assert!(s.contains("\"code\":\"content_filter\""));
    }

    #[test]
    fn failure_stream_400_schema_error_stays_bad_request() {
        let body = r#"{"error":{"code":400,"message":"Invalid JSON payload received. Unknown name \"xx\".","status":"INVALID_ARGUMENT"}}"#;
        let s = drive_failure_stream(400, body);
        assert!(s.contains("\"code\":\"bad_request\""));
    }

    #[test]
    fn failure_stream_408_504_emits_timeout() {
        let s = drive_failure_stream(408, r#"{"error":{"message":"deadline"}}"#);
        assert!(s.contains("\"code\":\"timeout\""));
        let s = drive_failure_stream(504, r#"<html>504 Gateway Time-out</html>"#);
        assert!(s.contains("\"code\":\"timeout\""));
        assert!(s.contains("HTTP 504"));
    }

    #[test]
    fn failure_stream_503_service_unavailable_distinct_from_500() {
        let s = drive_failure_stream(503, r#"{"error":{"message":"overloaded"}}"#);
        assert!(s.contains("\"code\":\"service_unavailable\""));
        let s = drive_failure_stream(500, r#"{"error":{"message":"internal"}}"#);
        assert!(s.contains("\"code\":\"server_error\""));
    }

    #[test]
    fn failure_stream_array_form_error_body() {
        // Gemini 偶尔返 [{"error":{...}}] array shape,而非 object;必须能 extract message
        let body = r#"[{"error":{"code":429,"message":"array-form quota exceeded","status":"RESOURCE_EXHAUSTED"}}]"#;
        let s = drive_failure_stream(429, body);
        assert!(s.contains("array-form quota exceeded"));
        assert!(s.contains("\"code\":\"quota_exceeded\""));
    }

    #[test]
    fn failure_stream_empty_body_still_emits_failed() {
        // 空 body 也必须 emit lifecycle + failed,客户端不能卡
        let s = drive_failure_stream(500, "");
        assert!(s.contains("event: response.failed"));
        assert!(s.contains("\"code\":\"server_error\""));
        assert!(s.contains("HTTP 500"));
    }

    #[test]
    fn failure_stream_transport_error_mid_read_surfaces_in_message() {
        // 上游 ByteStream Err mid-read 不能 silent swallow — 必须出现在 user message 里 +
        // code 降级到 upstream_transport_error(client 知道 body 不可信)
        let upstream: ByteStream = Box::pin(stream::iter(vec![
            Ok(Bytes::from_static(b"{\"error\":\"partial")),
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "tcp reset by peer",
            )),
        ]));
        let mut s = convert_gemini_error_to_responses_failure_stream(
            http::StatusCode::from_u16(429).unwrap(),
            upstream,
            None,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all = Vec::new();
        runtime.block_on(async {
            while let Some(item) = s.next().await {
                all.extend_from_slice(&item.unwrap());
            }
        });
        let out = String::from_utf8(all).unwrap();
        assert!(out.contains("\"code\":\"upstream_transport_error\""));
        assert!(out.contains("transport error"));
        assert!(out.contains("tcp reset by peer"));
        assert!(out.contains("event: response.failed"));
    }

    #[test]
    fn failure_stream_oversized_body_capped_with_truncated_marker() {
        // 模拟 100KB 错误体 → body cap 64KB,JSON 解析失败(被截在中间),
        // message 标 [body truncated],客户端仍能识别错误
        let huge = format!("{{\"error\":{{\"message\":\"{}\"}}}}", "x".repeat(100_000));
        let upstream: ByteStream = Box::pin(stream::iter(vec![Ok(Bytes::from(huge))]));
        let mut s = convert_gemini_error_to_responses_failure_stream(
            http::StatusCode::from_u16(500).unwrap(),
            upstream,
            None,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all = Vec::new();
        runtime.block_on(async {
            while let Some(item) = s.next().await {
                all.extend_from_slice(&item.unwrap());
            }
        });
        let out = String::from_utf8(all).unwrap();
        assert!(out.contains("[body truncated]"));
        assert!(out.contains("event: response.failed"));
    }

    #[test]
    fn failure_stream_long_user_message_truncated_with_ellipsis() {
        // 上游返 10K-char 合法 JSON message → 用户 message 应在 2000 char 处截断 + … 标记,
        // 防 SSE event 撑爆
        let long_msg = "z".repeat(10_000);
        let body = format!(
            r#"{{"error":{{"code":429,"message":"{long_msg}","status":"RESOURCE_EXHAUSTED"}}}}"#
        );
        let s = drive_failure_stream(429, &body);
        assert!(s.contains("…"));
        assert!(s.contains("\"code\":\"quota_exceeded\""));
    }

    #[test]
    fn failure_stream_non_utf8_body_marked() {
        // 非 UTF-8 字节序列 → from_utf8_lossy 替换 + message 标 [non-UTF-8 body]
        let upstream: ByteStream = Box::pin(stream::iter(vec![Ok(Bytes::from_static(&[
            0xFF, 0xFE, 0xFD,
        ]))]));
        let mut s = convert_gemini_error_to_responses_failure_stream(
            http::StatusCode::from_u16(502).unwrap(),
            upstream,
            None,
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut all = Vec::new();
        runtime.block_on(async {
            while let Some(item) = s.next().await {
                all.extend_from_slice(&item.unwrap());
            }
        });
        let out = String::from_utf8(all).unwrap();
        assert!(out.contains("[non-UTF-8 body]"));
        assert!(out.contains("\"code\":\"server_error\""));
    }

    #[test]
    fn failure_stream_emits_complete_lifecycle() {
        // failure 流必须包含完整 created+in_progress+failed,客户端能正确进入终态
        let s = drive_failure_stream(429, r#"{"error":{"message":"rate"}}"#);
        let events: Vec<&str> = s.split("\n\n").filter(|x| !x.is_empty()).collect();
        let names: Vec<String> = events.iter().map(|e| parse_event(e).0).collect();
        assert_eq!(names[0], "response.created");
        assert_eq!(names[1], "response.in_progress");
        assert_eq!(names[2], "response.failed");
        // sequence_number 单调
        for (i, e) in events.iter().enumerate() {
            let (_, v) = parse_event(e);
            assert_eq!(v["sequence_number"], i);
        }
    }

    /// **MCP namespace dispatch** (parity test) — 对齐
    /// `responses::converter::function_call_item_includes_namespace_field_when_tool_came_from_namespace_pack`。
    /// 用 original_request.tools 含 namespace 包 build converter,模型生成 namespace
    /// 内层 function call → emit 的 output_item.added / .done + envelope output[] 全
    /// 必须含 `namespace: "mcp__notion__"` 字段(Codex.app dispatch 必要)
    #[test]
    fn function_call_item_includes_namespace_field_when_tool_came_from_namespace_pack() {
        let original_request = json!({
            "model": "gemini-3-flash",
            "tools": [
                {"type": "function", "name": "shell"},
                {"type": "namespace", "name": "mcp__notion__", "tools": [
                    {"type": "function", "name": "notion_search"},
                    {"type": "function", "name": "notion_create_pages"},
                ]},
                {"type": "namespace", "name": "mcp__figma__", "tools": [
                    {"type": "function", "name": "whoami"},
                ]}
            ]
        });
        let mut conv = GeminiToResponsesConverter::new(Some(original_request), None);
        // Gemini wire:模型回 functionCall {name: notion_search}
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"notion_search","args":{}}}]},"finishReason":"STOP"}]}

"#;
        let events_raw = drive_to_events(&mut conv, &[chunk]);
        let events: Vec<(String, Value)> = events_raw.iter().map(|e| parse_event(e)).collect();

        // output_item.added 含 namespace
        let added = events
            .iter()
            .find(|(n, p)| {
                n == "response.output_item.added"
                    && p["item"].get("type").and_then(|v| v.as_str()) == Some("function_call")
            })
            .expect("function_call output_item.added emitted");
        assert_eq!(
            added.1["item"]["namespace"], "mcp__notion__",
            "function_call.added item 必须含 namespace 字段"
        );
        assert_eq!(added.1["item"]["name"], "notion_search");

        // output_item.done 含 namespace
        let done = events
            .iter()
            .find(|(n, p)| {
                n == "response.output_item.done"
                    && p["item"].get("type").and_then(|v| v.as_str()) == Some("function_call")
            })
            .expect("function_call output_item.done emitted");
        assert_eq!(done.1["item"]["namespace"], "mcp__notion__");

        // response.completed.output[] final item 含 namespace
        let completed = events
            .iter()
            .find(|(n, _)| n == "response.completed")
            .expect("response.completed emitted");
        let final_output = completed.1["response"]["output"].as_array().unwrap();
        let final_fc = final_output
            .iter()
            .find(|item| item.get("type").and_then(|v| v.as_str()) == Some("function_call"))
            .expect("function_call in completed output");
        assert_eq!(final_fc["namespace"], "mcp__notion__");
    }

    /// 顶级 function(非 namespace 内层)的 function_call **不该**带 namespace
    /// 字段,否则 Codex.app 可能把它误当 dynamic tool 路由到不存在的 server。
    /// 对齐 `responses::converter::function_call_item_omits_namespace_when_tool_is_top_level_function`
    #[test]
    fn function_call_item_omits_namespace_when_tool_is_top_level_function() {
        let original_request = json!({
            "model": "gemini-3-flash",
            "tools": [{"type": "function", "name": "shell"}]
        });
        let mut conv = GeminiToResponsesConverter::new(Some(original_request), None);
        let chunk = br#"data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"shell","args":{}}}]},"finishReason":"STOP"}]}

"#;
        let events_raw = drive_to_events(&mut conv, &[chunk]);
        let events: Vec<(String, Value)> = events_raw.iter().map(|e| parse_event(e)).collect();
        let added = events
            .iter()
            .find(|(n, p)| {
                n == "response.output_item.added"
                    && p["item"].get("type").and_then(|v| v.as_str()) == Some("function_call")
            })
            .expect("function_call output_item.added emitted");
        assert!(
            added.1["item"].get("namespace").is_none(),
            "顶级 function 不该带 namespace 字段,实际 {:?}",
            added.1["item"].get("namespace")
        );
    }
}
