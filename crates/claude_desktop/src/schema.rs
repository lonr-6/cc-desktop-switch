//! Claude Desktop 端 Provider / Config schema —— 1:1 转写自
//! `lonr-6/cc-desktop-switch backend/config.py:18-185`(Apache-2 / MIT)。
//!
//! 跟 `codex-app-transfer-registry::Provider` 是**独立**两套 schema:
//! - cas 现有 Provider:Codex CLI 端,`api_format = "openai_chat"`,base url
//!   形如 `https://api.deepseek.com/v1`(OpenAI 兼容)
//! - 本 schema:Claude Desktop 端,`api_format = "anthropic"`,base url 形如
//!   `https://api.deepseek.com/anthropic`(Anthropic Messages 兼容直连)
//!
//! 两套 provider 列表并行存在 cas 配置中,各管各的 active provider。

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 单个 Provider —— 字段一对一对应 `backend/config.py` BUILTIN_PRESETS dict 字段。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    /// 第三方上游的 Anthropic-compatible endpoint(直连,无中间代理)。
    pub base_url: String,
    /// `"bearer"` 或 `"x-api-key"`(`backend/config.py:39,82,149,163`)。
    #[serde(default = "default_auth_scheme")]
    pub auth_scheme: String,
    /// 固定为 `"anthropic"`(全部 7 个内置 preset 都是这个值)。
    #[serde(default = "default_api_format")]
    pub api_format: String,
    /// 用户填入的 API key,运行时与 `extra_headers` 占位符 `{apiKey}` 一起渲染。
    #[serde(default)]
    pub api_key: String,
    /// 模型槽位映射:`sonnet` / `haiku` / `opus` / `default` → 第三方真模型 ID。
    /// 空字符串槽位等价于"fallback 到 default"(`backend/model_alias.py`)。
    #[serde(default)]
    pub models: ModelMappings,
    /// 可选 baseUrl 列表(`backend/config.py:126-135` Xiaomi MiMo Token Plan 用)。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_url_options: Vec<BaseUrlOption>,
    /// baseUrl 选择提示(`backend/config.py:136`)。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url_hint: String,
    /// 可选模型组(`backend/config.py:47-61` DeepSeek 1M / `:171-180` Qwen 1M)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub model_options: IndexMap<String, ModelOption>,
    /// 模型能力声明(`backend/config.py:181`,顶层声明,跟 modelOptions 内的能力合并)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub model_capabilities: IndexMap<String, Value>,
    /// 请求选项(`backend/config.py:62,182`)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub request_options: IndexMap<String, Value>,
    /// 可选请求选项预设(`backend/config.py:63-74` DeepSeek Max 思维)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub request_option_presets: IndexMap<String, RequestOptionPreset>,
    /// 额外 header,值可含 `{apiKey}` 占位符(`backend/config.py:75`)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub extra_headers: IndexMap<String, String>,
    #[serde(default)]
    pub is_builtin: bool,
    /// 排序(磁盘上的位置,UI 渲染顺序由它决定)。
    #[serde(default)]
    pub sort_index: i64,
    /// 透传未显式枚举的字段(防 schema 升级丢字段)。
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

/// 模型槽位映射 —— 与 `backend/model_alias.py` MODEL_SLOTS 顺序保持一致。
///
/// 用 `IndexMap` 保留磁盘顺序;键值由 `model_alias::MODEL_ORDER` 提供。
pub type ModelMappings = IndexMap<String, String>;

/// `baseUrlOptions[i]`(`backend/config.py:127-134`)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BaseUrlOption {
    pub label: String,
    pub value: String,
}

/// `modelOptions[key]`(`backend/config.py:48-60` / `:172-179`)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelOption {
    pub label: String,
    pub description: String,
    /// 启用此选项时覆盖的模型映射(可选)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub models: ModelMappings,
    /// 启用此选项时覆盖的模型能力声明(可选)。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub model_capabilities: IndexMap<String, Value>,
}

/// `requestOptionPresets[key]`(`backend/config.py:64-73`)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RequestOptionPreset {
    pub label: String,
    pub description: String,
    /// 启用此预设时合并到请求体的 options(可能含 anthropic / openai 等子 key)。
    #[serde(default)]
    pub request_options: IndexMap<String, Value>,
}

/// 顶层 Claude Desktop 配置 ——
/// 对应 `backend/config.py:18-32` DEFAULT_CONFIG。
///
/// cas 端会把这个 struct 序列化进 `~/.codex-app-transfer/config.json` 的
/// `claudeDesktop` 子段(跟 cas 现有的顶层 `providers / activeProvider` 并行),
/// 避免互相影响。具体 I/O 由 `crates/registry` 端做。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopConfig {
    pub active_provider: Option<String>,
    #[serde(default)]
    pub providers: Vec<Provider>,
    /// 用户启用的 modelOption keys(`backend/config.py` 没显式记录,但
    /// `frontend/js` 通过 active provider 的 enabledModelOptions 字段控制)。
    /// 这里独立存,避免污染 provider 自身字段。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub enabled_model_options: IndexMap<String, Vec<String>>,
    /// 用户启用的 requestOptionPreset keys。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub enabled_request_option_presets: IndexMap<String, Vec<String>>,
    /// cas 自生成的 gateway API key —— 写入 Claude Desktop plist 的
    /// `inferenceGatewayApiKey` 字段;客户端发请求时带这个,本地代理验证后
    /// 用上游真 `provider.api_key` 转发,**真 key 不暴露**给 Claude Desktop。
    /// 对照 `cc-desktop-switch/backend/config.py get_or_create_gateway_api_key`。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gateway_api_key: String,
}

impl Default for ClaudeDesktopConfig {
    fn default() -> Self {
        Self {
            active_provider: None,
            providers: Vec::new(),
            enabled_model_options: IndexMap::new(),
            enabled_request_option_presets: IndexMap::new(),
            gateway_api_key: String::new(),
        }
    }
}

fn default_auth_scheme() -> String {
    "bearer".to_owned()
}

fn default_api_format() -> String {
    "anthropic".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deepseek_preset_roundtrip() {
        // 用 backend/config.py:35-77 DeepSeek preset 验 schema 字段全覆盖
        let raw = json!({
            "id": "deepseek",
            "name": "DeepSeek",
            "baseUrl": "https://api.deepseek.com/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "deepseek-v4-pro",
                "haiku": "deepseek-v4-flash",
                "opus": "deepseek-v4-pro",
                "default": "deepseek-v4-pro"
            },
            "modelOptions": {
                "deepseek_1m": {
                    "label": "解锁 1M 上下文",
                    "description": "用于 Claude Code/长上下文场景。开启后 Sonnet、Opus 和默认模型使用 deepseek-v4-pro[1m]。",
                    "models": {
                        "sonnet": "deepseek-v4-pro[1m]",
                        "haiku": "deepseek-v4-flash",
                        "opus": "deepseek-v4-pro[1m]",
                        "default": "deepseek-v4-pro[1m]"
                    },
                    "modelCapabilities": {
                        "deepseek-v4-pro[1m]": {"supports1m": true}
                    }
                }
            },
            "requestOptionPresets": {
                "deepseek_max_effort": {
                    "label": "DeepSeek Max 思维",
                    "description": "Low：更快更省，适合简单任务。",
                    "requestOptions": {
                        "anthropic": {
                            "thinking": {"type": "enabled"},
                            "output_config": {"effort": "max"}
                        }
                    }
                }
            },
            "extraHeaders": {"x-api-key": "{apiKey}"},
            "isBuiltin": true
        });
        let provider: Provider = serde_json::from_value(raw.clone()).expect("deserialize");
        assert_eq!(provider.id, "deepseek");
        assert_eq!(provider.base_url, "https://api.deepseek.com/anthropic");
        assert_eq!(provider.auth_scheme, "bearer");
        assert_eq!(provider.api_format, "anthropic");
        assert_eq!(provider.models.get("sonnet").map(String::as_str), Some("deepseek-v4-pro"));
        assert_eq!(provider.model_options.len(), 1);
        assert_eq!(provider.request_option_presets.len(), 1);
        assert_eq!(provider.extra_headers.get("x-api-key").map(String::as_str), Some("{apiKey}"));
        // round-trip 不丢字段
        let reserialized: Value = serde_json::to_value(&provider).unwrap();
        assert_eq!(reserialized.get("id"), raw.get("id"));
        assert_eq!(reserialized.get("baseUrl"), raw.get("baseUrl"));
        assert_eq!(reserialized.get("modelOptions"), raw.get("modelOptions"));
    }

    #[test]
    fn kimi_minimal_preset_uses_defaults() {
        // backend/config.py:78-91 Kimi 没有 modelOptions / requestOptionPresets
        // / extraHeaders / baseUrlOptions,验证 skip_serializing_if 工作
        let raw = json!({
            "id": "kimi",
            "name": "Kimi (月之暗面)",
            "baseUrl": "https://api.moonshot.cn/anthropic",
            "authScheme": "bearer",
            "apiFormat": "anthropic",
            "models": {
                "sonnet": "kimi-k2.6",
                "haiku": "kimi-k2.6",
                "opus": "kimi-k2.6",
                "default": "kimi-k2.6"
            },
            "isBuiltin": true
        });
        let provider: Provider = serde_json::from_value(raw.clone()).unwrap();
        let out = serde_json::to_value(&provider).unwrap();
        assert!(out.get("modelOptions").is_none(), "空 modelOptions 不应被序列化");
        assert!(out.get("requestOptionPresets").is_none());
        assert!(out.get("extraHeaders").is_none());
        assert!(out.get("baseUrlOptions").is_none());
    }

    #[test]
    fn default_config_is_empty() {
        let cfg = ClaudeDesktopConfig::default();
        assert!(cfg.active_provider.is_none());
        assert!(cfg.providers.is_empty());
        assert!(cfg.enabled_model_options.is_empty());
    }
}
