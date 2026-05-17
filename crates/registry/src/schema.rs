//! 类型化 schema —— 与 backend/config.py 中 DEFAULT_CONFIG 一一对应.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APP_VERSION: &str = "1.0.4";
pub const DEFAULT_UPDATE_URL: &str =
    "https://github.com/Cmochance/codex-app-transfer/releases/latest/download/latest.json";

pub const DEFAULT_THEME: &str = "default";
pub const DEFAULT_LANGUAGE: &str = "zh";
pub const DEFAULT_PROXY_PORT: u16 = 18080;
pub const DEFAULT_ADMIN_PORT: u16 = 18081;

/// Provider 缺省 `authScheme`：旧版 / 手编 config 不写时按主流 OpenAI 兼容
/// 上游回退为 `bearer`，避免反序列化失败。
fn default_auth_scheme() -> String {
    "bearer".to_owned()
}

/// Provider 缺省 `apiFormat`：与现存 5 家用户 + 7 家内置预设保持一致。
fn default_api_format() -> String {
    "openai_chat".to_owned()
}

/// 顶层配置文件结构(对应 `~/.codex-app-transfer/config.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub version: String,
    pub active_provider: Option<String>,
    pub gateway_api_key: Option<String>,
    #[serde(default)]
    pub providers: Vec<Provider>,
    pub settings: Settings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: APP_VERSION.to_owned(),
            active_provider: None,
            gateway_api_key: None,
            providers: Vec::new(),
            settings: Settings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub language: String,
    pub proxy_port: u16,
    pub admin_port: u16,
    pub auto_start: bool,
    pub auto_apply_on_start: bool,
    pub expose_all_provider_models: bool,
    pub restore_codex_on_exit: bool,
    pub update_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_owned(),
            language: DEFAULT_LANGUAGE.to_owned(),
            proxy_port: DEFAULT_PROXY_PORT,
            admin_port: DEFAULT_ADMIN_PORT,
            auto_start: false,
            auto_apply_on_start: true,
            expose_all_provider_models: false,
            restore_codex_on_exit: true,
            update_url: DEFAULT_UPDATE_URL.to_owned(),
        }
    }
}

/// Provider 记录 —— 字段集是已知必备 + 可选,未知字段挂在 `extra` 里
/// 透传(典型如内置预设的 `notices` / `baseUrlOptions` / `requestOptionPresets`
/// / `baseUrlHint` 等只在部分 provider 出现的字段).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_auth_scheme")]
    pub auth_scheme: String,
    #[serde(default = "default_api_format")]
    pub api_format: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub models: ModelMappings,
    #[serde(default)]
    pub extra_headers: IndexMap<String, String>,
    #[serde(default)]
    pub model_capabilities: IndexMap<String, Value>,
    #[serde(default)]
    pub request_options: IndexMap<String, Value>,
    #[serde(default)]
    pub is_builtin: bool,
    #[serde(default)]
    pub sort_index: i64,
    /// 透传任何此结构未显式枚举的字段(notices / baseUrlOptions /
    /// requestOptionPresets / baseUrlHint / docsUrl / ...).
    #[serde(flatten)]
    pub extra: IndexMap<String, Value>,
}

/// 模型槽位映射 —— 与 `backend/model_alias.py` MODEL_SLOTS 顺序保持一致.
///
/// 用 `IndexMap` 保留磁盘顺序;键值由 `model_alias::MODEL_ORDER` 提供.
pub type ModelMappings = IndexMap<String, String>;

/// 用枚举形式记录槽位 key,便于业务代码以编译期保证引用.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelSlotKey {
    Default,
    Gpt55,
    Gpt54,
    Gpt54Mini,
    Gpt53Codex,
    Gpt52,
}

impl ModelSlotKey {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelSlotKey::Default => "default",
            ModelSlotKey::Gpt55 => "gpt_5_5",
            ModelSlotKey::Gpt54 => "gpt_5_4",
            ModelSlotKey::Gpt54Mini => "gpt_5_4_mini",
            ModelSlotKey::Gpt53Codex => "gpt_5_3_codex",
            ModelSlotKey::Gpt52 => "gpt_5_2",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_tolerates_missing_auth_scheme_and_api_format() {
        let json = r#"{
            "id": "mock-provider",
            "name": "Mock",
            "baseUrl": "http://127.0.0.1:29090",
            "apiKey": "mock-key",
            "models": { "default": "mock-model" },
            "sortIndex": 0
        }"#;
        let p: Provider = serde_json::from_str(json).expect("旧版 / 手编 config 应能加载");
        assert_eq!(p.auth_scheme, "bearer");
        assert_eq!(p.api_format, "openai_chat");
    }

    #[test]
    fn provider_tolerates_missing_models() {
        let json = r#"{
            "id": "p",
            "name": "P",
            "baseUrl": "http://x",
            "authScheme": "bearer",
            "apiFormat": "openai_chat"
        }"#;
        let p: Provider = serde_json::from_str(json).unwrap();
        assert!(p.models.is_empty());
    }
}
