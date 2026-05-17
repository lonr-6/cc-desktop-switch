//! Gemini native API wire types.
//!
//! 1:1 镜像 LiteLLM `litellm/types/llms/vertex_ai.py` 的 TypedDict 定义,
//! 用 Rust serde struct + camelCase 字段。
//!
//! 主要 schema 来源:
//! - <https://ai.google.dev/api/generate-content> — RequestBody / Candidate / GenerateContentResponse
//! - <https://ai.google.dev/api/caching#Tool> — Tool / FunctionDeclaration / ToolConfig
//! - <https://ai.google.dev/gemini-api/docs/google-search> — GroundingMetadata / GroundingChunk

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ─────────── Request side ───────────

/// 一个 Part = text / inlineData / functionCall / functionResponse / fileData
/// 之中**恰好一个**(Gemini 严格,序列化时空字段必须省略)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "inlineData", skip_serializing_if = "Option::is_none")]
    pub inline_data: Option<InlineData>,
    #[serde(rename = "fileData", skip_serializing_if = "Option::is_none")]
    pub file_data: Option<FileData>,
    #[serde(rename = "functionCall", skip_serializing_if = "Option::is_none")]
    pub function_call: Option<FunctionCall>,
    #[serde(rename = "functionResponse", skip_serializing_if = "Option::is_none")]
    pub function_response: Option<FunctionResponse>,
    /// 标记本 part 是 thought / reasoning 内容(Gemini 3.x thinking)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
    /// thinking part 的 signature(Codex.app 不直接展示,作为 provider_specific
    /// state 透传给后续 turn,跟 LiteLLM `transformation.py:469` 一致)。
    #[serde(rename = "thoughtSignature", skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    /// base64 编码后的二进制数据。
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileData {
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    #[serde(rename = "fileUri")]
    pub file_uri: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON object,Gemini 的 args 是结构化对象不是 OpenAI 那样的字符串。
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionResponse {
    pub name: String,
    /// 上游期望 `{"response": {...}}` wrap;LiteLLM
    /// `prompt_templates/factory.py:convert_to_gemini_tool_call_result` 也这样组。
    pub response: Value,
}

/// `contents[].role` 只接受 `"user"` 或 `"model"`(无 system / tool 角色 —
/// system 走顶层 systemInstruction,tool response 用 user role 包 functionResponse)。
///
/// **反序列化时 role 是 optional**:Gemini streaming chunks 内部的 `content`
/// 经常省略 role(隐含 "model"),用 `#[serde(default)]` 兜底成 "model"。
/// 序列化时(我们出站请求)总是显式 emit。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Content {
    #[serde(default = "default_content_role")]
    pub role: String,
    #[serde(default)]
    pub parts: Vec<Part>,
}

fn default_content_role() -> String {
    "model".to_owned()
}

/// `system_instruction` 顶层字段(Gemini 不支持 messages 里独立 system role)。
/// 跟 Content 共享 schema 但 role 通常是 `"user"` 或干脆省略 — Google 文档
/// 多个 example 显示无 role 字段也 work。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInstruction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub parts: Vec<Part>,
}

/// 一个 Tool 内部**只能有一种**:functionDeclarations / googleSearch /
/// googleSearchRetrieval / codeExecution / urlContext / googleMaps / computerUse。
/// LiteLLM `vertex_and_google_ai_studio_gemini.py:688` 注释:
/// "A Tool object should contain exactly one type of Tool"。
///
/// MVP 只实现 functionDeclarations + googleSearch;其他类型保留 `extra` 给
/// 用户透传(`extra_body.tools` 路径)。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tool {
    #[serde(
        rename = "functionDeclarations",
        skip_serializing_if = "Option::is_none"
    )]
    pub function_declarations: Option<Vec<FunctionDeclaration>>,
    /// `googleSearch: {}` — Gemini grounding via Google Search(Gemini 2+/3+ 才支持)。
    #[serde(rename = "googleSearch", skip_serializing_if = "Option::is_none")]
    pub google_search: Option<Value>,
    /// `googleSearchRetrieval: {dynamicRetrievalConfig: {...}}` — Gemini 1.5 老版本
    /// 的 grounding 接口(2.0 起被 googleSearch 替代,但旧模型仍接受)。
    #[serde(
        rename = "googleSearchRetrieval",
        skip_serializing_if = "Option::is_none"
    )]
    pub google_search_retrieval: Option<Value>,
    /// `urlContext: {}` — URL 上下文工具(Gemini 2.5+)。
    #[serde(rename = "urlContext", skip_serializing_if = "Option::is_none")]
    pub url_context: Option<Value>,
    /// `codeExecution: {}` — 代码执行工具。
    #[serde(rename = "codeExecution", skip_serializing_if = "Option::is_none")]
    pub code_execution: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDeclaration {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema(OpenAPI 3.0 子集)— Gemini 接受跟 OpenAI 函数 parameters
    /// 几乎同形态的 schema,只是不接 `additionalProperties` / `strict` /
    /// 部分高级 keyword(LiteLLM `common_utils.py:_build_vertex_schema` 清洗)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
    /// Gemini 2+ 才支持响应 schema(部分 fn 返结构化 JSON 时用)。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    #[serde(
        rename = "functionCallingConfig",
        skip_serializing_if = "Option::is_none"
    )]
    pub function_calling_config: Option<FunctionCallingConfig>,
    #[serde(
        rename = "includeServerSideToolInvocations",
        skip_serializing_if = "Option::is_none"
    )]
    pub include_server_side_tool_invocations: Option<bool>,
    #[serde(rename = "retrievalConfig", skip_serializing_if = "Option::is_none")]
    pub retrieval_config: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallingConfig {
    /// `"AUTO"` / `"NONE"` / `"ANY"`(对应 OpenAI auto / none / required)。
    pub mode: String,
    #[serde(
        rename = "allowedFunctionNames",
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_function_names: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(rename = "topP", skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(rename = "topK", skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i64>,
    #[serde(rename = "maxOutputTokens", skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    #[serde(rename = "stopSequences", skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(rename = "candidateCount", skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<i64>,
    #[serde(rename = "responseMimeType", skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    #[serde(rename = "responseSchema", skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<Value>,
    /// Gemini 2+ thinking budget(对齐 OpenAI `reasoning_effort`):
    /// none → -1(disabled), low → 1024, medium → 8192, high → 16384(LiteLLM
    /// `vertex_and_google_ai_studio_gemini.py:822` 默认映射;Gemini 3+ 用 level 而不是 budget)。
    #[serde(rename = "thinkingConfig", skip_serializing_if = "Option::is_none")]
    pub thinking_config: Option<ThinkingConfig>,
    #[serde(rename = "responseModalities", skip_serializing_if = "Option::is_none")]
    pub response_modalities: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThinkingConfig {
    #[serde(rename = "thinkingBudget", skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<i64>,
    #[serde(rename = "includeThoughts", skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
    /// Gemini 3+ 用 level("low"/"medium"/"high")替代 budget 数值。
    #[serde(rename = "thinkingLevel", skip_serializing_if = "Option::is_none")]
    pub thinking_level: Option<String>,
}

/// 顶层 generateContent 请求体。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestBody {
    pub contents: Vec<Content>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<SystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    pub tool_config: Option<ToolConfig>,
    #[serde(rename = "safetySettings", skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<Value>>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
    #[serde(rename = "cachedContent", skip_serializing_if = "Option::is_none")]
    pub cached_content: Option<String>,
}

// ─────────── Response side ───────────

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GenerateContentResponse {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<Candidate>,
    #[serde(rename = "promptFeedback", skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<PromptFeedback>,
    #[serde(rename = "usageMetadata", skip_serializing_if = "Option::is_none")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "responseId", skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(rename = "modelVersion", skip_serializing_if = "Option::is_none")]
    pub model_version: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Candidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(rename = "finishReason", skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    #[serde(rename = "safetyRatings", skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<Value>>,
    #[serde(rename = "citationMetadata", skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<Value>,
    #[serde(rename = "groundingMetadata", skip_serializing_if = "Option::is_none")]
    pub grounding_metadata: Option<GroundingMetadata>,
    #[serde(rename = "urlContextMetadata", skip_serializing_if = "Option::is_none")]
    pub url_context_metadata: Option<Value>,
    #[serde(rename = "tokenCount", skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i64>,
    #[serde(rename = "logprobsResult", skip_serializing_if = "Option::is_none")]
    pub logprobs_result: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GroundingMetadata {
    #[serde(rename = "webSearchQueries", skip_serializing_if = "Option::is_none")]
    pub web_search_queries: Option<Vec<String>>,
    #[serde(rename = "searchEntryPoint", skip_serializing_if = "Option::is_none")]
    pub search_entry_point: Option<Value>,
    #[serde(rename = "groundingChunks", skip_serializing_if = "Option::is_none")]
    pub grounding_chunks: Option<Vec<GroundingChunk>>,
    #[serde(rename = "groundingSupports", skip_serializing_if = "Option::is_none")]
    pub grounding_supports: Option<Vec<GroundingSupport>>,
    #[serde(rename = "retrievalMetadata", skip_serializing_if = "Option::is_none")]
    pub retrieval_metadata: Option<Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GroundingChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<GroundingChunkWeb>,
    /// 其他 chunk 类型(retrieved_context 等)留 raw,MVP 不映射。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GroundingChunkWeb {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GroundingSupport {
    pub segment: GroundingSegment,
    #[serde(
        rename = "groundingChunkIndices",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub grounding_chunk_indices: Vec<usize>,
    #[serde(
        rename = "confidenceScores",
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub confidence_scores: Vec<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct GroundingSegment {
    #[serde(rename = "startIndex", default)]
    pub start_index: usize,
    #[serde(rename = "endIndex", default)]
    pub end_index: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(rename = "partIndex", default, skip_serializing_if = "Option::is_none")]
    pub part_index: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct UsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    pub prompt_token_count: i64,
    #[serde(rename = "candidatesTokenCount", default)]
    pub candidates_token_count: i64,
    #[serde(rename = "totalTokenCount", default)]
    pub total_token_count: i64,
    #[serde(
        rename = "cachedContentTokenCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cached_content_token_count: Option<i64>,
    #[serde(
        rename = "thoughtsTokenCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub thoughts_token_count: Option<i64>,
    #[serde(
        rename = "toolUsePromptTokenCount",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_use_prompt_token_count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traffic_type: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct PromptFeedback {
    #[serde(rename = "blockReason", skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(rename = "safetyRatings", skip_serializing_if = "Option::is_none")]
    pub safety_ratings: Option<Vec<Value>>,
}

// ─────────── Helpers ───────────

/// Gemini finishReason → OpenAI finish_reason 映射(LiteLLM `vertex_and_google_ai_studio_gemini.py:1311
/// `_GEMINI_FINISH_REASON_KEYS`)。
pub fn map_finish_reason(gemini: &str) -> &'static str {
    match gemini {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT" => "content_filter",
        "TOO_MANY_TOOL_CALLS" | "MALFORMED_FUNCTION_CALL" | "MALFORMED_RESPONSE" => "tool_calls",
        // FINISH_REASON_UNSPECIFIED / LANGUAGE / OTHER 等
        _ => "stop",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_serializes_only_set_fields() {
        // 防回归:Part 只带 text 时 wire 不能出现 inlineData/functionCall/...
        let p = Part {
            text: Some("hi".into()),
            ..Default::default()
        };
        let s = serde_json::to_string(&p).unwrap();
        assert_eq!(s, r#"{"text":"hi"}"#);
    }

    #[test]
    fn function_call_part_omits_text() {
        let p = Part {
            function_call: Some(FunctionCall {
                name: "search".into(),
                args: serde_json::json!({"q": "weather"}),
            }),
            ..Default::default()
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert!(v.get("text").is_none(), "functionCall part 必须不含 text");
        assert_eq!(v["functionCall"]["name"], "search");
        assert_eq!(v["functionCall"]["args"]["q"], "weather");
    }

    #[test]
    fn request_body_camel_case_wire() {
        let body = RequestBody {
            contents: vec![Content {
                role: "user".into(),
                parts: vec![Part {
                    text: Some("hi".into()),
                    ..Default::default()
                }],
            }],
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                max_output_tokens: Some(1024),
                ..Default::default()
            }),
            ..Default::default()
        };
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&body).unwrap()).unwrap();
        // generation_config → generationConfig
        assert!(v.get("generationConfig").is_some());
        assert_eq!(v["generationConfig"]["maxOutputTokens"], 1024);
        // 未设置字段不出现
        assert!(v.get("systemInstruction").is_none());
        assert!(v.get("tools").is_none());
    }

    #[test]
    fn finish_reason_mapping() {
        assert_eq!(map_finish_reason("STOP"), "stop");
        assert_eq!(map_finish_reason("MAX_TOKENS"), "length");
        assert_eq!(map_finish_reason("SAFETY"), "content_filter");
        assert_eq!(map_finish_reason("PROHIBITED_CONTENT"), "content_filter");
        assert_eq!(map_finish_reason("MALFORMED_FUNCTION_CALL"), "tool_calls");
        assert_eq!(map_finish_reason("UNKNOWN_FUTURE_VALUE"), "stop");
    }

    #[test]
    fn grounding_metadata_parses_real_wire_shape() {
        // 来自 ai.google.dev/api/generate-content GroundingMetadata 文档示例
        let raw = serde_json::json!({
            "webSearchQueries": ["纽约今天天气"],
            "groundingChunks": [
                {"web": {"uri": "https://weather.com/nyc", "title": "Weather.com - NYC"}},
                {"web": {"uri": "https://accuweather.com/nyc", "title": "AccuWeather"}}
            ],
            "groundingSupports": [
                {
                    "segment": {"startIndex": 0, "endIndex": 25, "text": "纽约今天 25°C 晴"},
                    "groundingChunkIndices": [0, 1],
                    "confidenceScores": [0.95, 0.88]
                }
            ]
        });
        let gm: GroundingMetadata = serde_json::from_value(raw).unwrap();
        assert_eq!(gm.web_search_queries.as_ref().unwrap().len(), 1);
        let chunks = gm.grounding_chunks.unwrap();
        assert_eq!(
            chunks[0].web.as_ref().unwrap().uri,
            "https://weather.com/nyc"
        );
        let supports = gm.grounding_supports.unwrap();
        assert_eq!(supports[0].segment.start_index, 0);
        assert_eq!(supports[0].segment.end_index, 25);
        assert_eq!(supports[0].grounding_chunk_indices, vec![0, 1]);
    }
}
