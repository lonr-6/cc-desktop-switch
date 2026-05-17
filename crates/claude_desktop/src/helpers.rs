//! 跨平台 helper —— 1:1 转写自
//! `lonr-6/cc-desktop-switch backend/registry.py:13-122`(Apache-2 / MIT)。
//!
//! 这些函数 macOS / Windows 共用(serialize headers / models 等纯数据变换),
//! 平台特化(Windows Registry / macOS plist 实际写盘)在
//! [`crate::registry`] / [`crate::macos`] 各自实现。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[allow(unused_imports)]
use crate::model_alias::{all_provider_model_entries, provider_model_entries};
use crate::schema::Provider;

/// `CCDS_MARKER`(`backend/registry.py:14`)——
/// Claude Desktop policy 字段标记位,表示该 policy table 由本工具管理,
/// restore 时一并清除。
pub const CCDS_MARKER: &str = "ccds_managed";

/// `DESKTOP_CONFIG`(`backend/registry.py:17-25`)—— 7 个 Claude Desktop
/// gateway policy 字段,**顺序 + 值类型必须**与上游一致(Windows Registry
/// REG_SZ vs REG_DWORD;macOS plist string vs integer)。
///
/// Rust 端用 [`DesktopConfigField`] 列表表达,运行时根据 `kind` 选择 plist 类型。
#[derive(Debug, Clone, Copy)]
pub struct DesktopConfigField {
    pub name: &'static str,
    pub default_value: DesktopConfigValue,
}

#[derive(Debug, Clone, Copy)]
pub enum DesktopConfigValue {
    Str(&'static str),
    Int(i64),
}

/// 7 字段顺序对齐 `backend/registry.py:17-25`。
pub const DESKTOP_CONFIG: &[DesktopConfigField] = &[
    DesktopConfigField {
        name: "inferenceProvider",
        default_value: DesktopConfigValue::Str("gateway"),
    },
    DesktopConfigField {
        name: "inferenceGatewayApiKey",
        default_value: DesktopConfigValue::Str(""),
    },
    DesktopConfigField {
        name: "inferenceGatewayAuthScheme",
        default_value: DesktopConfigValue::Str("bearer"),
    },
    DesktopConfigField {
        name: "inferenceGatewayHeaders",
        default_value: DesktopConfigValue::Str("[]"),
    },
    DesktopConfigField {
        name: "inferenceModels",
        default_value: DesktopConfigValue::Str(r#"["sonnet","haiku","opus"]"#),
    },
    DesktopConfigField {
        name: "inferenceGatewayBaseUrl",
        default_value: DesktopConfigValue::Str("http://127.0.0.1:18080"),
    },
    DesktopConfigField {
        name: "isClaudeCodeForDesktopEnabled",
        default_value: DesktopConfigValue::Int(1),
    },
];

/// `_managed_policy_names(names)`(`backend/registry.py:29-32`)——
/// 给定一组现有 policy 字段名,返回**本工具管理**(7 字段 + ccds_managed)
/// 的那些,用于 clear / restore 时只清自己写的字段。
pub fn managed_policy_names<'a>(names: &'a [String]) -> Vec<&'a str> {
    let mut managed: HashSet<&str> = DESKTOP_CONFIG.iter().map(|f| f.name).collect();
    managed.insert(CCDS_MARKER);
    names
        .iter()
        .filter(|n| managed.contains(n.as_str()))
        .map(|s| s.as_str())
        .collect()
}

/// `_safe_config_value(name, value)`(`backend/registry.py:51-58`)——
/// 把可能含密钥的字段 mask 成 `******`,供前端 / 日志展示。
pub fn safe_config_value(name: &str, value: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("headers") && (value.is_empty() || value == "[]") {
        return String::new();
    }
    let secret_tokens = ["key", "token", "secret", "authorization", "headers"];
    if secret_tokens.iter().any(|t| lower.contains(t)) {
        return if value.is_empty() {
            String::new()
        } else {
            "******".to_owned()
        };
    }
    value.to_owned()
}

/// `serialize_gateway_headers(extra_headers, api_key)`
/// (`backend/registry.py:61-74`)—— 把 provider extraHeaders + api_key 转
/// 成 Claude Desktop policy `inferenceGatewayHeaders` 字段的 JSON 字符串
/// (形如 `["x-api-key: sk-...","x-extra: v"]`)。空 → `""`。
pub fn serialize_gateway_headers<'a, I>(extra_headers: I, api_key: &str) -> String
where
    I: IntoIterator<Item = (&'a String, &'a String)>,
{
    let mut headers: Vec<String> = Vec::new();
    for (name, value) in extra_headers {
        let header_name = name.trim();
        if header_name.is_empty() {
            continue;
        }
        let header_value = value.replace("{apiKey}", api_key);
        headers.push(format!("{header_name}: {header_value}"));
    }
    if headers.is_empty() {
        return String::new();
    }
    // ensure_ascii=False, separators=(",", ":") 对齐上游
    serde_json::to_string(&headers).unwrap_or_default()
}

/// `_desktop_model_items(items)`(`backend/registry.py:35-49`)——
/// 只保留 Claude Desktop policy 支持的模型字段
/// (`name` / `displayName` / 可选 `supports1m`)。
fn desktop_model_items(items: &[crate::model_alias::ModelEntry]) -> Vec<Value> {
    let mut cleaned: Vec<Value> = Vec::with_capacity(items.len());
    for item in items {
        let mut obj = serde_json::Map::new();
        if !item.name.is_empty() {
            obj.insert("name".to_owned(), Value::String(item.name.clone()));
        }
        if !item.display_name.is_empty() {
            obj.insert(
                "displayName".to_owned(),
                Value::String(item.display_name.clone()),
            );
        }
        // ❗`sourceModel` —— Claude Desktop 客户端真正发请求时用的 body.model 值。
        // 没有它客户端会 fallback 把 `name`(`claude-sonnet-4-6` 等白名单)直接当
        // body.model 发给上游 → Kimi Code `/coding` 只识别 `kimi-for-coding`,
        // 立刻 404 model_not_found。对照 `cc-desktop-switch/backend/model_alias.py:204`
        // `item["sourceModel"] = source_model`。
        if !item.source_model.is_empty() {
            obj.insert(
                "sourceModel".to_owned(),
                Value::String(item.source_model.clone()),
            );
        }
        // providerId —— Claude Desktop 用来在 cowork / multi-provider 场景路由
        // 到正确的 gateway entry(`backend/model_alias.py:205`)。
        if let Some(pid) = &item.provider_id {
            if !pid.is_empty() {
                obj.insert("providerId".to_owned(), Value::String(pid.clone()));
            }
        }
        if item.supports1m {
            obj.insert("supports1m".to_owned(), Value::Bool(true));
        }
        cleaned.push(Value::Object(obj));
    }
    cleaned
}

const FALLBACK_MODELS: &[&str] = &["sonnet", "haiku", "opus"];

/// `provider_inference_models(provider)`
/// (`backend/registry.py:90-101`)。
pub fn provider_inference_models(provider: Option<&Provider>) -> Vec<Value> {
    let Some(p) = provider else {
        return FALLBACK_MODELS
            .iter()
            .map(|s| Value::String((*s).to_owned()))
            .collect();
    };
    // ❗用 `desktop_model_entries` 而非 `provider_model_entries`:
    // - desktop_model_entries(`backend/model_alias.py:181-223`):name=`slot.claude_ids[0]`
    //   (Claude 白名单如 `claude-sonnet-4-6`),source_model=上游真实 ID。
    // - provider_model_entries(`backend/model_alias.py:225-243`):name=真实 ID
    //   (`kimi-for-coding` 等),给 proxy / 调试用,不该暴露给 Claude Desktop。
    //
    // Claude Desktop 1.7196 加了白名单校验,非 `claude-*` 的 model name 会被客户端拒绝,
    // model picker 退化到默认 Sonnet 4.6 单选。这是用户报"模型映射失败"的根因。
    let entries = crate::model_alias::desktop_model_entries(p, false);
    let cleaned = desktop_model_items(&entries);
    if cleaned.is_empty() {
        FALLBACK_MODELS
            .iter()
            .map(|s| Value::String((*s).to_owned()))
            .collect()
    } else {
        cleaned
    }
}

/// `all_provider_inference_models(providers)`
/// (对照 `backend/model_alias.py:245-258 all_provider_model_entries`)——
/// expose_all 模式跨多 provider 去重生成所有 claude-* 安全路由(用 alias
/// 后缀如 `claude-sonnet-4-6@kimi-code`)。
pub fn all_provider_inference_models(providers: &[Provider]) -> Vec<Value> {
    // 同上:走 desktop_model_entries(use_alias=true),不要走 provider_model_entries。
    let mut entries: Vec<crate::model_alias::ModelEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in providers {
        for item in crate::model_alias::desktop_model_entries(p, true) {
            if seen.contains(&item.name) {
                continue;
            }
            seen.insert(item.name.clone());
            entries.push(item);
        }
    }
    let cleaned = desktop_model_items(&entries);
    if cleaned.is_empty() {
        FALLBACK_MODELS
            .iter()
            .map(|s| Value::String((*s).to_owned()))
            .collect()
    } else {
        cleaned
    }
}

/// `serialize_inference_models(provider, providers, expose_all)`
/// (`backend/registry.py:110-121`)—— 返回 `inferenceModels` 字段的 JSON
/// 字符串(供 Windows Registry REG_SZ 写入;macOS plist 也用同样格式)。
pub fn serialize_inference_models(
    provider: Option<&Provider>,
    providers: &[Provider],
    expose_all: bool,
) -> String {
    let models = if expose_all {
        all_provider_inference_models(providers)
    } else {
        provider_inference_models(provider)
    };
    serde_json::to_string(&Value::Array(models)).unwrap_or_else(|_| "[]".to_owned())
}

/// 平台 OS 名称(`backend/registry.py:76-82` `_os_name()`)。
pub fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "win"
    } else if cfg!(target_os = "macos") {
        "mac"
    } else {
        "linux"
    }
}

/// 非 Windows 且非 macOS 时的提示
/// (`backend/registry.py:85-87` `_not_supported()`)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotSupported {
    pub success: bool,
    pub message: String,
}

pub fn not_supported() -> NotSupported {
    NotSupported {
        success: false,
        message: "Claude Desktop 没有 Linux GUI 版本，无需配置".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_alias::ModelEntry;
    use crate::schema::ModelMappings;
    use indexmap::IndexMap;

    fn provider(id: &str, name: &str, api_key: &str) -> Provider {
        Provider {
            id: id.to_owned(),
            name: name.to_owned(),
            base_url: "https://example.com/anthropic".to_owned(),
            auth_scheme: "bearer".to_owned(),
            api_format: "anthropic".to_owned(),
            api_key: api_key.to_owned(),
            models: ModelMappings::new(),
            base_url_options: Vec::new(),
            base_url_hint: String::new(),
            model_options: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            request_option_presets: IndexMap::new(),
            extra_headers: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn desktop_config_has_seven_fields_in_upstream_order() {
        assert_eq!(DESKTOP_CONFIG.len(), 7);
        let names: Vec<&str> = DESKTOP_CONFIG.iter().map(|f| f.name).collect();
        assert_eq!(
            names,
            vec![
                "inferenceProvider",
                "inferenceGatewayApiKey",
                "inferenceGatewayAuthScheme",
                "inferenceGatewayHeaders",
                "inferenceModels",
                "inferenceGatewayBaseUrl",
                "isClaudeCodeForDesktopEnabled",
            ]
        );
    }

    #[test]
    fn managed_policy_names_filters_to_self() {
        let names = vec![
            "inferenceProvider".to_owned(),
            "ccds_managed".to_owned(),
            "userField".to_owned(),
            "inferenceGatewayApiKey".to_owned(),
        ];
        let managed = managed_policy_names(&names);
        assert_eq!(managed.len(), 3, "ccds_managed + 2 个 inference 字段");
        assert!(!managed.contains(&"userField"), "用户自加字段不动");
    }

    #[test]
    fn safe_config_value_masks_secrets() {
        assert_eq!(safe_config_value("inferenceGatewayApiKey", "sk-abc"), "******");
        assert_eq!(safe_config_value("inferenceGatewayApiKey", ""), "");
        assert_eq!(safe_config_value("inferenceProvider", "gateway"), "gateway");
        assert_eq!(safe_config_value("inferenceGatewayHeaders", "[]"), "");
        assert_eq!(
            safe_config_value("inferenceGatewayHeaders", r#"[{"x-api-key":"sk"}]"#),
            "******",
            "headers 字段任何非空都 mask(可能含 key)"
        );
    }

    #[test]
    fn serialize_gateway_headers_substitutes_apikey() {
        let mut h = IndexMap::new();
        h.insert("x-api-key".to_owned(), "{apiKey}".to_owned());
        h.insert("x-extra".to_owned(), "static-value".to_owned());
        let serialized = serialize_gateway_headers(h.iter(), "sk-secret");
        // ensure_ascii=False, separators=(",", ":") 对齐 Python json.dumps
        assert_eq!(
            serialized,
            r#"["x-api-key: sk-secret","x-extra: static-value"]"#
        );
    }

    #[test]
    fn serialize_gateway_headers_empty_returns_empty_string() {
        let h: IndexMap<String, String> = IndexMap::new();
        assert_eq!(serialize_gateway_headers(h.iter(), "sk-x"), "");
    }

    #[test]
    fn provider_inference_models_fallback_when_no_provider() {
        let result = provider_inference_models(None);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0], Value::String("sonnet".to_owned()));
        assert_eq!(result[1], Value::String("haiku".to_owned()));
        assert_eq!(result[2], Value::String("opus".to_owned()));
    }

    #[test]
    fn serialize_inference_models_for_provider() {
        let mut p = provider("deepseek", "DeepSeek", "");
        // sonnet legacy key 会被 normalize 到 sonnet_4_6 slot
        p.models.insert("sonnet".to_owned(), "deepseek-v4-pro".to_owned());
        let serialized = serialize_inference_models(Some(&p), &[], false);
        // name 必须是 Claude 白名单(claude-sonnet-4-6),不能是上游真实 ID
        assert!(serialized.contains(r#""name":"claude-sonnet-4-6""#));
        assert!(serialized.contains(r#""displayName":"claude-sonnet-4-6""#));
        // 上游真实 ID 不会出现在 inferenceModels(Claude 白名单只接受 claude-*)
        assert!(!serialized.contains(r#""name":"deepseek-v4-pro""#));
    }

    #[test]
    fn os_name_is_one_of_three() {
        let n = os_name();
        assert!(matches!(n, "win" | "mac" | "linux"));
    }

    // 强制使用 ModelEntry import,避免未使用 warning
    #[test]
    fn _model_entry_use_check() {
        let _e = ModelEntry {
            name: "x".to_owned(),
            display_name: "y".to_owned(),
            source_model: "z".to_owned(),
            provider_id: None,
            supports1m: false,
        };
    }
}
