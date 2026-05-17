//! grok.com Web 后端 wire types(请求 payload + SSE 帧)。
//!
//! ## 协议来源
//!
//! 字段集与默认值通过以下方式 verified:
//!
//! 1. 三次真实 SuperGrok 账号请求 cURL 抓包(2026-05-11)—— 24 字段稳定
//! 2. 五次响应 SSE 抓帧(R1/R2/R3/connector_mcp/tool_call)—— 帧结构 verified
//! 3. grok.com 前端 webpack chunks 反编译(76 chunks,~11MB)—— 字段名 + 序列化函数
//! 4. chenyme/grok2api 反向工程产出借鉴(endpoint table + SSE schema)
//!
//! ## 关键事实
//!
//! - `modeId` 是后端模型 ID(替代旧 `modelName` 字段)
//! - `disabledConnectorIds: []` 是唯一的 connector 字段(黑名单 + server-side state)
//! - 客户端**不传** `connectorIds` / `connectors` / `toolOverrides`
//! - MCP 调用通过 `call_connected_tool` wrapper(`toolUsageCard.mcp.{toolName, toolArgsJson}`)
//! - 多轮通过 `parentResponseId` DAG 锚定(每条 user/model response 各有 UUID)

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// grok.com chat 请求 payload(24 字段稳定核心 + 可选高级字段)。
///
/// `POST /rest/app-chat/conversations/new` body。
///
/// **首轮**:`parent_response_id` 与 `conversation_id` 都不传(omit)。
/// **续接**:走同 endpoint,带 `parent_response_id`(上一轮 modelResponse 的 ID)。
///
/// 字段顺序对齐前端反编译看到的 default 模板,protobuf-JSON camelCase 风格。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokChatRequest {
    /// 当前轮 user input 单 string(不是 messages 数组,多轮靠 `parent_response_id`)。
    pub message: String,

    /// 后端模型 ID,实测 `grok-420-computer-use-sa`。**替代旧 `modelName` 字段**。
    pub mode_id: String,

    /// `true` = incognito 不入历史;`false` = 正常存账户对话历史。
    pub temporary: bool,

    /// 文件附件 ID 列表(由 `/rest/app-chat/upload-file` 上传后取得)。
    #[serde(default)]
    pub file_attachments: Vec<String>,

    /// 图片附件(同上)。
    #[serde(default)]
    pub image_attachments: Vec<String>,

    /// `false` = 启用 grok 内置 web/x search;`true` = 禁用。
    pub disable_search: bool,

    /// 启用图片生成(image_gen tool)。
    pub enable_image_generation: bool,

    /// 是否返回 image bytes(`false` 时只返回 URL)。
    pub return_image_bytes: bool,

    /// 内部 debug 字段,默认 false。
    pub return_raw_grok_in_xai_request: bool,

    /// 图片流式生成。
    pub enable_image_streaming: bool,

    /// 图片生成数量(实测默认 2)。
    pub image_generation_count: u32,

    /// 强制简洁回答(关闭后允许长 chain-of-thought)。
    pub force_concise: bool,

    /// 启用 side-by-side 模式(grok-4.20-heavy 的双轨思考 UI)。
    pub enable_side_by_side: bool,

    /// 强制 side-by-side(总是开)。
    pub force_side_by_side: bool,

    /// 末尾 emit `finalMetadata` 帧(含 follow-up suggestions)。
    pub send_final_metadata: bool,

    /// 禁用 follow-up 文本建议(`finalMetadata.followUpSuggestions` 仍会 emit)。
    pub disable_text_follow_ups: bool,

    /// 元数据字段,通常 `{}`。
    #[serde(default)]
    pub response_metadata: Value,

    /// `false` = 启用 grok 长期记忆;`true` = 禁用。
    pub disable_memory: bool,

    /// 是否异步 chat(非 SSE 模式)。
    pub is_async_chat: bool,

    /// 禁用自残检测短路。
    pub disable_self_harm_short_circuit: bool,

    /// Grok Files 集合 ID(类似 RAG 文件夹引用),默认 `[]`。
    #[serde(default)]
    pub collection_ids: Vec<String>,

    /// **唯一 connector 字段** —— 黑名单。默认 `[]` = 启用全部用户已注册 connector。
    ///
    /// **不要**传 `connectorIds` / `connectors` —— 那些字段在现行协议中已废弃,
    /// connector 启用状态完全由 grok.com 后端 server-side 维护。
    #[serde(default)]
    pub disabled_connector_ids: Vec<String>,

    /// 浏览器环境信息(防风控)。
    pub device_env_info: DeviceEnvInfo,

    // ── 多轮 / 高级字段(可选,首轮全 omit)──
    /// 上一轮 model response 的 `responseId`(UUID v4)。首轮 omit。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_response_id: Option<String>,

    /// 引用上一轮模型回答的某段文本(quote-reply 场景)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_quoted_text: Option<String>,

    /// 命名 system prompt(grok.com 预定义 prompt 集)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt_name: Option<String>,

    /// 长形式 system prompt(用户 customize 填的"风格")。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,

    /// 短形式 personality traits(用户 customize 填的"个性")。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_personality: Option<String>,

    /// DeepSearch 预设:`""` / `"default"` / `"deeper"`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deepsearch_preset: Option<String>,

    /// 启用 reasoning(thinking 阶段输出)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_reasoning: Option<bool>,

    /// 让 grok 浏览特定 URL 列表(类似 browse_page 预热)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webpage_urls: Option<Vec<String>>,

    /// 跳过响应 cache(强制重新生成)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_response_cache: Option<bool>,

    /// 是否来自 Grok Files 入口(`grok.com/files` UI)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_from_grok_files: Option<bool>,

    /// 启用 retry 机制。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_retries: Option<bool>,

    /// 是否为 regenerate 请求(重新生成同一 user message 的回答)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_regen_request: Option<bool>,

    /// 恢复 / 续传指定 response。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_response_id: Option<String>,

    /// 模型 override key(覆盖 modeId 后端解析)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_override_key: Option<String>,

    /// 跳过取消当前 inflight 请求。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_cancel_current_inflight_requests: Option<bool>,

    /// 对话线程 parent ID(thread 概念,与 parentResponseId 不同语义)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_parent_id: Option<String>,

    /// Companion ID(Grok 角色 / 助手 ID)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion_id: Option<String>,

    /// Imagine project ID(Imagine 创作工作流)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imagine_project_id: Option<String>,

    /// 禁用个性化推荐。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_personalization: Option<bool>,

    /// follow-up 类型 hint。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up_type: Option<String>,

    /// 任意未在 schema 列出的字段透传(向前兼容)。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// 设备环境信息(浏览器 viewport / DPR / dark mode)。
///
/// grok.com 后端用于自适应 UI 渲染 + 防风控(检测 bot 时看 viewport 是否合理)。
/// 本 adapter 用一个固定 "桌面浏览器" profile,避免每次请求随机化触发风控。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEnvInfo {
    pub dark_mode_enabled: bool,
    pub device_pixel_ratio: f32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub viewport_width: u32,
    pub viewport_height: u32,
}

impl Default for DeviceEnvInfo {
    /// 默认 profile:1470x956 屏 + 1470x371 viewport + DPR 2.0 + 亮色模式。
    ///
    /// 数值来自真实 SuperGrok 账号 macOS Safari 抓包(2026-05-11)。
    /// 保持稳定值避免触发 bot detection。
    fn default() -> Self {
        Self {
            dark_mode_enabled: false,
            device_pixel_ratio: 2.0,
            screen_width: 1470,
            screen_height: 956,
            viewport_width: 1470,
            viewport_height: 371,
        }
    }
}

/// `extra` 字段禁止透传的 key 集(review-feedback TD3):
///
/// 协议事实 — 这些字段在现行 grok.com 协议下**不被后端识别 / 已废弃 / 由
/// server-side state 接管**。如果通过 `extra` 偷渡进 payload 会让用户误以为
/// 自己开了某个 connector 但实际无效,触发 silent-degradation。
///
/// `validate()` 检测后返回 `AdapterError::BadRequest`,让 forward 主路径
/// surface 给客户端清晰错误而不是 silent ship 给上游。
pub const FORBIDDEN_EXTRA_KEYS: &[&str] = &[
    "connectorIds",  // 已废弃白名单(用 disabledConnectorIds 黑名单 + server-side state)
    "connectors",    // 同上(connectors[] 对象数组路径未启用)
    "toolOverrides", // 已废弃(grok.com toolOverrides 字段后端 silent ignore 未知工具名)
    "modelName",     // 协议已改:用 modeId 替代 modelName
    "supportedFastTools", // 后端预定义 fast tools 开关字典,不接受 schema
];

impl GrokChatRequest {
    /// 构造时一致性检查,在 `serialize_grok_request` 内调用。
    ///
    /// 检查项:
    /// 1. `message` 非空(grok.com `BadRequest` if empty)
    /// 2. `mode_id` 非空(否则 grok.com 后端会用 fallback 模型,用户失控)
    /// 3. `extra` 不含 [`FORBIDDEN_EXTRA_KEYS`] 任意 key(防偷渡已废弃字段)
    pub fn validate(&self) -> Result<(), crate::types::AdapterError> {
        use crate::types::AdapterError;
        if self.message.is_empty() {
            return Err(AdapterError::BadRequest(
                "grok_web: message field must not be empty".into(),
            ));
        }
        if self.mode_id.is_empty() {
            return Err(AdapterError::BadRequest(
                "grok_web: mode_id field must not be empty".into(),
            ));
        }
        for forbidden in FORBIDDEN_EXTRA_KEYS {
            if self.extra.contains_key(*forbidden) {
                return Err(AdapterError::BadRequest(format!(
                    "grok_web: forbidden field '{forbidden}' in extra — \
                     this field is deprecated or replaced by server-side state. \
                     See `docs/grok/04-protocol-final.md` or types.rs FORBIDDEN_EXTRA_KEYS"
                )));
            }
        }
        Ok(())
    }
}

impl Default for GrokChatRequest {
    fn default() -> Self {
        Self {
            message: String::new(),
            mode_id: "grok-420-computer-use-sa".into(),
            temporary: false,
            file_attachments: Vec::new(),
            image_attachments: Vec::new(),
            disable_search: false,
            enable_image_generation: true,
            return_image_bytes: false,
            return_raw_grok_in_xai_request: false,
            enable_image_streaming: true,
            image_generation_count: 2,
            force_concise: false,
            enable_side_by_side: true,
            force_side_by_side: false,
            send_final_metadata: true,
            disable_text_follow_ups: false,
            response_metadata: Value::Object(serde_json::Map::new()),
            disable_memory: false,
            is_async_chat: false,
            disable_self_harm_short_circuit: false,
            collection_ids: Vec::new(),
            disabled_connector_ids: Vec::new(),
            device_env_info: DeviceEnvInfo::default(),
            parent_response_id: None,
            parent_quoted_text: None,
            system_prompt_name: None,
            custom_instructions: None,
            custom_personality: None,
            deepsearch_preset: None,
            is_reasoning: None,
            webpage_urls: None,
            skip_response_cache: None,
            is_from_grok_files: None,
            enable_retries: None,
            is_regen_request: None,
            resume_response_id: None,
            model_override_key: None,
            skip_cancel_current_inflight_requests: None,
            thread_parent_id: None,
            companion_id: None,
            imagine_project_id: None,
            disable_personalization: None,
            follow_up_type: None,
            extra: serde_json::Map::new(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SSE 响应帧
// ─────────────────────────────────────────────────────────────────────────────

// 注:`GrokSseEnvelope` 类型已删除(review-feedback TD1 — type-design-analyzer
// 报告 dead code drift risk:类型 doc 声称是"两种 wrapping 形态的 envelope",
// 但实际 flatten 逻辑在 `response.rs::extract_response_frame` 里直接对
// `serde_json::Value` 操作,从未消费这个类型)。
//
// 两种 envelope 形态(`{"result":{...}}` 旧 / `{"result":{"response":{...}}}` 新)
// 直接由 `extract_response_frame` 通过 `result.get("response").unwrap_or(result)`
// 处理,三行胜过一个类型。
//
// error envelope(`{"error":{...}}`)由 `response.rs::process_buffered_lines`
// 主动检测后翻译成 `response.failed`(review-feedback H3 防护)。

/// 展平后的响应帧字段集(适用于两种 envelope wrapping)。
///
/// 字段含义:
///
/// - `token`:文本 / thinking token / XML tool card 串
/// - `is_thinking`:`true` = thinking 阶段,`false` = 正式回答
/// - `is_soft_stop`:流末标志(对应 `response.completed` 事件)
/// - `message_tag`:`header` / `summary` / `tool_usage_card` / `raw_function_result` / `final`
/// - `response_id`:本轮 model.responseId(用于下轮 `parent_response_id`)
///
/// 其余字段按需扩展,使用 `Value` 留余地。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrokResponseFrame {
    #[serde(default)]
    pub token: Option<String>,

    #[serde(default)]
    pub is_thinking: Option<bool>,

    #[serde(default)]
    pub is_soft_stop: Option<bool>,

    #[serde(default)]
    pub message_tag: Option<String>,

    #[serde(default)]
    pub message_step_id: Option<u32>,

    #[serde(default)]
    pub response_id: Option<String>,

    // ── 工具使用卡片 ──
    #[serde(default)]
    pub tool_usage_card_id: Option<String>,

    #[serde(default)]
    pub tool_usage_card: Option<Value>,

    // ── 工具结果数据帧 ──
    #[serde(default)]
    pub web_search_results: Option<Value>,

    #[serde(default)]
    pub x_search_results: Option<Value>,

    #[serde(default)]
    pub code_execution_result: Option<Value>,

    #[serde(default)]
    pub card_attachment: Option<Value>,

    // ── lifecycle 帧 ──
    /// userResponse 帧:server 收到 user message 后回灌确认。
    #[serde(default)]
    pub user_response: Option<Value>,

    /// modelResponse 帧:流末 server emit 完整 model response state。
    #[serde(default)]
    pub model_response: Option<Value>,

    /// conversation 创建帧(新会话首轮 server emit)。
    #[serde(default)]
    pub conversation: Option<Value>,

    /// UI layout hint。
    #[serde(default)]
    pub ui_layout: Option<Value>,

    /// LLM 模型 hash(可识别模型版本)。
    #[serde(default)]
    pub llm_info: Option<Value>,

    /// finalMetadata 帧:follow-up suggestions 等。
    #[serde(default)]
    pub final_metadata: Option<Value>,

    /// 其余字段透传。
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// `messageTag` 枚举(实测全集)。
///
/// 未列出的 tag 走 [`GrokMessageTag::Unknown`] 通道,默认按 final-token 处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrokMessageTag {
    /// 思考阶段子标题
    Header,
    /// 思考阶段总结(数行 bullet)
    Summary,
    /// 工具使用卡片(模型调 tool 的展示帧)
    ToolUsageCard,
    /// 工具结果(数据帧 + 空 token 收尾帧两种形态)
    RawFunctionResult,
    /// 最终回答 token 流
    Final,
    /// 未知 tag(向前兼容)
    Unknown,
}

impl GrokMessageTag {
    pub fn parse(tag: &str) -> Self {
        match tag {
            "header" => Self::Header,
            "summary" => Self::Summary,
            "tool_usage_card" => Self::ToolUsageCard,
            "raw_function_result" => Self::RawFunctionResult,
            "final" => Self::Final,
            _ => Self::Unknown,
        }
    }
}

/// 工具调用 wire 形态(从 `toolUsageCard` 字段解析)。
///
/// 两种形态:
///
/// - 内置工具:`toolUsageCard.{webSearch|browsePage|xSearch|...}.args.{query|url|...}`
/// - MCP 工具:`toolUsageCard.mcp.{toolName, toolArgsJson}`
///   其中 `toolName` 形如 `<connector_displayName>___<tool_name>`(三下划线分隔)
#[derive(Debug, Clone)]
pub enum GrokToolCall {
    /// grok.com 内置工具(web_search / browse_page / x_search / ...)
    Builtin {
        /// 工具名 camelCase → snake_case 后(`webSearch` → `web_search`)
        name: String,
        /// 工具参数(直接是 JSON object)
        args: Value,
    },
    /// 通过 grok.com `call_connected_tool` 调用的 MCP 工具
    Mcp {
        /// `<connector_displayName>___<tool_name>` 三下划线分隔
        tool_name: String,
        /// **stringified JSON**(不是 nested object)
        tool_args_json: String,
    },
}

impl GrokToolCall {
    /// 把 MCP 调用的 `tool_args_json`(stringified JSON)解析成 `Value`。
    ///
    /// 返回:
    /// - `None` —— 不是 `Mcp` 变体(`Builtin` 的 args 已是 `Value`,无需解析)
    /// - `Some(Ok(v))` —— 解析成功
    /// - `Some(Err(e))` —— stringified JSON 自身有语法错误(grok.com 上游侧 bug)
    ///
    /// review-feedback TD5:避免下游调用点重复 `serde_json::from_str`,同时保持
    /// `Mcp::tool_args_json: String` wire-faithful(grok.com 真的 double-encode)。
    pub fn mcp_args_parsed(&self) -> Option<Result<Value, serde_json::Error>> {
        match self {
            Self::Mcp { tool_args_json, .. } => Some(serde_json::from_str(tool_args_json)),
            Self::Builtin { .. } => None,
        }
    }

    /// 从 `toolUsageCard` 字段解析。
    ///
    /// 返回 `None` 表示不是已知工具调用形态(向前兼容)。
    ///
    /// **多 key 消歧**(review-feedback TD5 note):serde_json::Map 默认保留插入
    /// 顺序;一个 `toolUsageCard` 同时含 `mcp` + 内置工具 key 时,本方法优先
    /// 选 `mcp` 分支(代码显式查 `mcp` 字段在前)。多个内置工具 key 共存时
    /// 按 IndexMap 顺序取第一个非 `toolUsageCardId` key,实测中 grok.com 一帧
    /// 只 emit 单工具,该非确定性目前无现实触发面。
    pub fn parse(card: &Value) -> Option<Self> {
        let obj = card.as_object()?;
        // 优先 MCP 路径
        if let Some(mcp) = obj.get("mcp").and_then(Value::as_object) {
            let tool_name = mcp.get("toolName").and_then(Value::as_str)?.to_owned();
            let tool_args_json = mcp
                .get("toolArgsJson")
                .and_then(Value::as_str)
                .unwrap_or("{}")
                .to_owned();
            return Some(Self::Mcp {
                tool_name,
                tool_args_json,
            });
        }
        // 内置工具:第一个非 toolUsageCardId 的 key 就是工具名
        for (key, val) in obj.iter() {
            if key == "toolUsageCardId" {
                continue;
            }
            let args = val
                .as_object()
                .and_then(|v| v.get("args"))
                .cloned()
                .unwrap_or(Value::Null);
            return Some(Self::Builtin {
                name: camel_to_snake(key),
                args,
            });
        }
        None
    }
}

/// `webSearch` → `web_search` 等 camelCase 转 snake_case。
///
/// 用于 grok.com 内置工具名归一化(响应里是 camelCase,Codex tool 名习惯 snake_case)。
fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_payload_serializes_to_24_core_fields() {
        let req = GrokChatRequest {
            message: "hi".into(),
            ..Default::default()
        };
        let v: Value = serde_json::to_value(&req).unwrap();
        let obj = v.as_object().unwrap();
        // 24 字段稳定核心 + extra(空时不算)
        let required_keys = [
            "message",
            "modeId",
            "temporary",
            "fileAttachments",
            "imageAttachments",
            "disableSearch",
            "enableImageGeneration",
            "returnImageBytes",
            "returnRawGrokInXaiRequest",
            "enableImageStreaming",
            "imageGenerationCount",
            "forceConcise",
            "enableSideBySide",
            "forceSideBySide",
            "sendFinalMetadata",
            "disableTextFollowUps",
            "responseMetadata",
            "disableMemory",
            "isAsyncChat",
            "disableSelfHarmShortCircuit",
            "collectionIds",
            "disabledConnectorIds",
            "deviceEnvInfo",
        ];
        for k in required_keys {
            assert!(
                obj.contains_key(k),
                "missing required field {k} in default payload"
            );
        }
        // 可选字段必须默认 omit(parent_response_id / custom_instructions / etc.)
        assert!(!obj.contains_key("parentResponseId"));
        assert!(!obj.contains_key("customInstructions"));
        assert!(!obj.contains_key("connectorIds"));
        assert!(!obj.contains_key("connectors"));
        assert!(!obj.contains_key("toolOverrides"));
    }

    #[test]
    fn parent_response_id_serializes_when_set() {
        let req = GrokChatRequest {
            message: "hi".into(),
            parent_response_id: Some("9f82a10c-47fb-4ff0-afee-bdeb21a37b16".into()),
            ..Default::default()
        };
        let v: Value = serde_json::to_value(&req).unwrap();
        assert_eq!(
            v["parentResponseId"],
            "9f82a10c-47fb-4ff0-afee-bdeb21a37b16"
        );
    }

    #[test]
    fn camel_to_snake_basics() {
        assert_eq!(camel_to_snake("webSearch"), "web_search");
        assert_eq!(camel_to_snake("browsePage"), "browse_page");
        assert_eq!(camel_to_snake("xKeywordSearch"), "x_keyword_search");
        assert_eq!(camel_to_snake("code_execution"), "code_execution");
    }

    #[test]
    fn tool_call_parses_builtin_web_search() {
        let card = json!({
            "toolUsageCardId": "72a558af-...",
            "webSearch": {
                "args": { "query": "Model Context Protocol" }
            }
        });
        match GrokToolCall::parse(&card).unwrap() {
            GrokToolCall::Builtin { name, args } => {
                assert_eq!(name, "web_search");
                assert_eq!(args["query"], "Model Context Protocol");
            }
            _ => panic!("expected Builtin"),
        }
    }

    #[test]
    fn validate_rejects_empty_message() {
        let mut req = GrokChatRequest::default();
        req.mode_id = "grok-420-computer-use-sa".into();
        // message 默认空,validate 必须 Err
        let err = req.validate().unwrap_err();
        match err {
            crate::types::AdapterError::BadRequest(msg) => {
                assert!(msg.contains("message"), "got: {msg}");
            }
            _ => panic!("expected BadRequest"),
        }
    }

    #[test]
    fn validate_rejects_forbidden_extra_keys() {
        // review-feedback TD3:防 connectorIds 通过 extra 偷渡
        let mut req = GrokChatRequest::default();
        req.message = "hi".into();
        req.extra.insert(
            "connectorIds".into(),
            serde_json::json!(["uuid-1", "uuid-2"]),
        );
        let err = req.validate().unwrap_err();
        match err {
            crate::types::AdapterError::BadRequest(msg) => {
                assert!(msg.contains("connectorIds"), "got: {msg}");
            }
            _ => panic!("expected BadRequest"),
        }
    }

    #[test]
    fn validate_passes_for_minimum_valid_request() {
        let mut req = GrokChatRequest::default();
        req.message = "hi".into();
        assert!(req.validate().is_ok());
    }

    #[test]
    fn mcp_args_parsed_returns_some_ok_for_mcp_variant() {
        // review-feedback TD5:helper 把 stringified JSON 转回 Value
        let call = GrokToolCall::Mcp {
            tool_name: "test___ask_question".into(),
            tool_args_json: r#"{"repoName":"foo/bar","question":"why?"}"#.into(),
        };
        let parsed = call.mcp_args_parsed().unwrap().unwrap();
        assert_eq!(parsed["repoName"], "foo/bar");
        assert_eq!(parsed["question"], "why?");
    }

    #[test]
    fn mcp_args_parsed_returns_none_for_builtin() {
        let call = GrokToolCall::Builtin {
            name: "web_search".into(),
            args: serde_json::json!({"query": "x"}),
        };
        assert!(call.mcp_args_parsed().is_none());
    }

    #[test]
    fn mcp_args_parsed_handles_malformed_json() {
        let call = GrokToolCall::Mcp {
            tool_name: "x___y".into(),
            tool_args_json: "{not valid json".into(),
        };
        let result = call.mcp_args_parsed().unwrap();
        assert!(result.is_err(), "malformed JSON should return Err");
    }

    #[test]
    fn tool_call_parses_mcp_call_connected_tool() {
        let card = json!({
            "toolUsageCardId": "f743ec42-...",
            "mcp": {
                "toolName": "test___ask_question",
                "toolArgsJson": "{\"repoName\":\"modelcontextprotocol/modelcontextprotocol\"}"
            }
        });
        match GrokToolCall::parse(&card).unwrap() {
            GrokToolCall::Mcp {
                tool_name,
                tool_args_json,
            } => {
                assert_eq!(tool_name, "test___ask_question");
                assert!(tool_args_json.contains("modelcontextprotocol"));
            }
            _ => panic!("expected Mcp"),
        }
    }

    #[test]
    fn message_tag_parses_full_set() {
        assert_eq!(GrokMessageTag::parse("header"), GrokMessageTag::Header);
        assert_eq!(GrokMessageTag::parse("summary"), GrokMessageTag::Summary);
        assert_eq!(
            GrokMessageTag::parse("tool_usage_card"),
            GrokMessageTag::ToolUsageCard
        );
        assert_eq!(
            GrokMessageTag::parse("raw_function_result"),
            GrokMessageTag::RawFunctionResult
        );
        assert_eq!(GrokMessageTag::parse("final"), GrokMessageTag::Final);
        assert_eq!(
            GrokMessageTag::parse("unknown_xxx"),
            GrokMessageTag::Unknown
        );
    }
}
