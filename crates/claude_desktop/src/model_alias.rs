//! 模型别名和多 provider 路由工具 —— 1:1 转写自
//! `lonr-6/cc-desktop-switch backend/model_alias.py:1-226`(Apache-2 / MIT)。

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::schema::{ModelMappings, Provider};

/// `MODEL_SLOTS[i]`(`backend/model_alias.py:10-46`)。
pub struct ModelSlot {
    pub key: &'static str,
    pub legacy: &'static [&'static str],
    pub claude_ids: &'static [&'static str],
}

/// `MODEL_SLOTS`(`backend/model_alias.py:10-46`)—— 7 个槽位顺序固定。
pub const MODEL_SLOTS: &[ModelSlot] = &[
    ModelSlot {
        key: "default",
        legacy: &["default"],
        claude_ids: &[],
    },
    ModelSlot {
        key: "opus_4_7",
        legacy: &["opus"],
        claude_ids: &["claude-opus-4-7"],
    },
    ModelSlot {
        key: "opus_4_6",
        legacy: &[],
        claude_ids: &["claude-opus-4-6"],
    },
    ModelSlot {
        key: "opus_3",
        legacy: &[],
        claude_ids: &["claude-3-opus"],
    },
    ModelSlot {
        key: "sonnet_4_6",
        legacy: &["sonnet"],
        claude_ids: &["claude-sonnet-4-6"],
    },
    ModelSlot {
        key: "sonnet_4_5",
        legacy: &[],
        claude_ids: &["claude-sonnet-4-5"],
    },
    ModelSlot {
        key: "haiku_4_5",
        legacy: &["haiku"],
        claude_ids: &["claude-haiku-4-5"],
    },
];

/// `DEFAULT_MODEL_KEY`(`backend/model_alias.py:48`)。
pub const DEFAULT_MODEL_KEY: &str = "default";

/// `LEGACY_MODEL_KEYS`(`backend/model_alias.py:49`)。
pub const LEGACY_MODEL_KEYS: &[&str] = &["default", "sonnet", "opus", "haiku"];

/// `MODEL_ORDER`(`backend/model_alias.py:47`)—— 槽位 key 顺序。
pub fn model_order() -> Vec<&'static str> {
    MODEL_SLOTS.iter().map(|s| s.key).collect()
}

/// `CLAUDE_ID_TO_SLOT`(`backend/model_alias.py:50-54`)—— Claude 模型 ID → 槽位 key。
pub fn claude_id_to_slot(claude_id: &str) -> Option<&'static str> {
    let lower = claude_id.to_ascii_lowercase();
    for slot in MODEL_SLOTS {
        for cid in slot.claude_ids {
            if *cid == lower {
                return Some(slot.key);
            }
        }
    }
    None
}

/// `empty_model_mappings()`(`backend/model_alias.py:57-58`)。
pub fn empty_model_mappings() -> ModelMappings {
    let mut out = ModelMappings::new();
    for key in model_order() {
        out.insert(key.to_owned(), String::new());
    }
    out
}

fn read_str(source: &IndexMap<String, Value>, key: &str) -> String {
    source
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// `normalize_model_mappings(models)`(`backend/model_alias.py:61-80`)——
/// 把旧四槽位 sonnet/haiku/opus/default 跟新多槽位 sonnet_4_6 / opus_4_7
/// 等统一成当前 7 槽位结构。
///
/// 接受任意 `IndexMap<String, Value>`(用户 / 旧 config 历史磁盘形态),
/// 返回 7 槽位 string-to-string 完整映射(空字符串槽位 = fallback 到 default)。
pub fn normalize_model_mappings(models: Option<&IndexMap<String, Value>>) -> ModelMappings {
    let mut normalized = empty_model_mappings();
    let Some(source) = models else {
        return normalized;
    };
    let default_value = read_str(source, "default");
    normalized.insert("default".to_owned(), default_value);

    for slot in MODEL_SLOTS {
        if slot.key == DEFAULT_MODEL_KEY {
            continue;
        }
        let mut candidates: Vec<&str> = vec![slot.key];
        candidates.extend(slot.legacy.iter().copied());
        for candidate in candidates {
            let value = read_str(source, candidate);
            if !value.is_empty() {
                normalized.insert(slot.key.to_owned(), value);
                break;
            }
        }
    }
    normalized
}

/// `model_mappings_with_legacy_aliases(models)`(`backend/model_alias.py:83-106`)——
/// 在 7 槽位之上补回旧 sonnet/opus/haiku 别名,供兼容读取(老 client 还会查
/// 这些 key)。
pub fn model_mappings_with_legacy_aliases(models: Option<&IndexMap<String, Value>>) -> ModelMappings {
    let normalized = normalize_model_mappings(models);
    let mut compat = normalized.clone();
    let get = |key: &str| normalized.get(key).cloned().unwrap_or_default();
    let default_value = get("default");

    compat.insert("default".to_owned(), default_value.clone());

    let sonnet = pick_first_nonempty(&[&get("sonnet_4_6"), &get("sonnet_4_5"), &default_value]);
    compat.insert("sonnet".to_owned(), sonnet);

    let opus = pick_first_nonempty(&[
        &get("opus_4_7"),
        &get("opus_4_6"),
        &get("opus_3"),
        &default_value,
    ]);
    compat.insert("opus".to_owned(), opus);

    let haiku = pick_first_nonempty(&[&get("haiku_4_5"), &default_value]);
    compat.insert("haiku".to_owned(), haiku);

    compat
}

fn pick_first_nonempty(candidates: &[&String]) -> String {
    for c in candidates {
        if !c.is_empty() {
            return (*c).clone();
        }
    }
    String::new()
}

/// 把 Provider.models 转成 IndexMap<String, Value> 供上面的函数消费。
fn provider_models_as_value_map(provider: &Provider) -> IndexMap<String, Value> {
    let mut out = IndexMap::new();
    for (k, v) in &provider.models {
        out.insert(k.clone(), Value::String(v.clone()));
    }
    out
}

/// `provider_model_ids(provider)`(`backend/model_alias.py:109-119`)——
/// 按稳定槽位顺序返回 provider 暴露给 Claude 的真实模型 ID(去重保序)。
pub fn provider_model_ids(provider: &Provider) -> Vec<String> {
    let models_map = provider_models_as_value_map(provider);
    let models = normalize_model_mappings(Some(&models_map));
    let mut ordered: Vec<String> = Vec::new();
    for key in model_order() {
        let model_id = models.get(key).cloned().unwrap_or_default();
        let model_id = model_id.trim().to_owned();
        if !model_id.is_empty() && !ordered.iter().any(|m| m == &model_id) {
            ordered.push(model_id);
        }
    }
    ordered
}

/// `provider_slug(provider)`(`backend/model_alias.py:122-126`)——
/// 生成稳定 provider 前缀(`re.sub(r"[^a-z0-9_-]+", "-", ...)` 等价的纯字符过滤)。
pub fn provider_slug(provider: &Provider) -> String {
    let source = if !provider.id.is_empty() {
        provider.id.clone()
    } else if !provider.name.is_empty() {
        provider.name.clone()
    } else {
        "provider".to_owned()
    };
    let lower = source.to_ascii_lowercase();
    // re.sub(r"[^a-z0-9_-]+", "-", lower)
    let mut replaced = String::with_capacity(lower.len());
    let mut prev_was_dash = false;
    for ch in lower.chars() {
        let is_keep = ch.is_ascii_alphanumeric() || ch == '_' || ch == '-';
        if is_keep {
            replaced.push(ch);
            prev_was_dash = false;
        } else if !prev_was_dash {
            replaced.push('-');
            prev_was_dash = true;
        }
    }
    // .strip("-_")
    let trimmed = replaced.trim_matches(|c: char| c == '-' || c == '_');
    let truncated: String = trimmed.chars().take(56).collect();
    if truncated.is_empty() {
        "provider".to_owned()
    } else {
        truncated
    }
}

/// `model_alias(provider, model_id)`(`backend/model_alias.py:129-131`)。
pub fn model_alias(provider: &Provider, model_id: &str) -> String {
    format!("{}/{}", provider_slug(provider), model_id)
}

/// `model_supports_1m(provider, model_id)`(`backend/model_alias.py:134-143`)——
/// `[1m]` 在 model_id 中(忽略大小写),或 `modelCapabilities[model_id].supports1m == true`。
pub fn model_supports_1m(provider: &Provider, model_id: &str) -> bool {
    if model_id.to_ascii_lowercase().contains("[1m]") {
        return true;
    }
    provider
        .model_capabilities
        .get(model_id)
        .and_then(|v| v.get("supports1m"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// `provider_model_entries(provider, use_alias)`
/// (`backend/model_alias.py:146-163`)—— 生成 `inferenceModels` / `/v1/models`
/// 共用的模型条目。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub name: String,
    pub display_name: String,
    pub source_model: String,
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub supports1m: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// `desktop_model_entries(provider, use_alias)`(对照 `backend/model_alias.py:181-223`)
/// —— 生成 Claude Desktop 暴露面的"安全路由"条目。
///
/// **关键**:`name` 字段强制用 `slot.claude_ids[0]`(Claude 白名单模型名如
/// `claude-sonnet-4-6`),让 Claude Desktop 客户端接受(1.7196+ 加了白名单
/// 检查,非 `claude-*` 名字会被拒)。`source_model` 保留上游真实模型 ID
/// (如 `kimi-for-coding`),给 proxy 模式做 alias 翻译用;Anthropic-compat
/// 直连模式则依赖上游 vendor 自己识别 Claude 模型名。
pub fn desktop_model_entries(provider: &Provider, use_alias: bool) -> Vec<ModelEntry> {
    let raw_value_map = provider_models_as_value_map(provider);
    let normalized = model_mappings_with_legacy_aliases(Some(&raw_value_map));
    let provider_name = if !provider.name.is_empty() {
        provider.name.clone()
    } else if !provider.id.is_empty() {
        provider.id.clone()
    } else {
        "Provider".to_owned()
    };
    let mut entries = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for slot in MODEL_SLOTS {
        if slot.key == DEFAULT_MODEL_KEY || slot.claude_ids.is_empty() {
            continue;
        }
        let source_model = normalized.get(slot.key).cloned().unwrap_or_default();
        if source_model.is_empty() {
            continue;
        }
        let route_id = slot.claude_ids[0];
        let name = if use_alias {
            model_alias(provider, route_id)
        } else {
            route_id.to_owned()
        };
        if seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        let display_name = if use_alias {
            format!("{provider_name} / {route_id}")
        } else {
            route_id.to_owned()
        };
        let supports_1m = model_supports_1m(provider, &source_model);
        entries.push(ModelEntry {
            name,
            display_name,
            source_model,
            provider_id: Some(provider.id.clone()),
            supports1m: supports_1m,
        });
    }
    entries
}

pub fn provider_model_entries(provider: &Provider, use_alias: bool) -> Vec<ModelEntry> {
    let provider_name = if !provider.name.is_empty() {
        provider.name.clone()
    } else if !provider.id.is_empty() {
        provider.id.clone()
    } else {
        "Provider".to_owned()
    };
    let mut entries = Vec::new();
    for model_id in provider_model_ids(provider) {
        let name = if use_alias {
            model_alias(provider, &model_id)
        } else {
            model_id.clone()
        };
        let display_name = if use_alias {
            format!("{provider_name} / {model_id}")
        } else {
            model_id.clone()
        };
        entries.push(ModelEntry {
            name,
            display_name,
            source_model: model_id.clone(),
            provider_id: Some(provider.id.clone()),
            supports1m: model_supports_1m(provider, &model_id),
        });
    }
    entries
}

/// `all_provider_model_entries(providers)`(`backend/model_alias.py:166-179`)——
/// 全 provider 去重别名模型条目。
pub fn all_provider_model_entries(providers: &[Provider]) -> Vec<ModelEntry> {
    let mut entries: Vec<ModelEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for provider in providers {
        for item in provider_model_entries(provider, true) {
            if seen.contains(&item.name) {
                continue;
            }
            seen.insert(item.name.clone());
            entries.push(item);
        }
    }
    entries
}

/// `resolve_model_alias(providers, requested_model)`
/// (`backend/model_alias.py:182-196`)。
///
/// 返回 `(matched_provider, model_id, did_resolve)`。无 `/` 时直接返
/// `(None, requested, false)`(不是别名格式)。
pub fn resolve_model_alias<'a>(
    providers: &'a [Provider],
    requested_model: &str,
) -> (Option<&'a Provider>, String, bool) {
    let requested = requested_model.to_owned();
    if !requested.contains('/') {
        return (None, requested, false);
    }
    let mut parts = requested.splitn(2, '/');
    let slug = parts.next().unwrap_or("");
    let model_id = parts.next().unwrap_or("");
    if slug.is_empty() || model_id.is_empty() {
        return (None, requested, false);
    }
    for provider in providers {
        if provider_slug(provider) == slug {
            // 即使 model_id 不在映射,也允许直通(对齐 Python `# 允许用户手动写入`)
            return (Some(provider), model_id.to_owned(), true);
        }
    }
    (None, requested, false)
}

/// `resolve_requested_model_slot(requested_model)`
/// (`backend/model_alias.py:199-225`)—— Claude 请求模型名 → 槽位 key。
pub fn resolve_requested_model_slot(requested_model: &str) -> Option<&'static str> {
    let requested = requested_model.trim().to_ascii_lowercase();
    if requested.is_empty() {
        return None;
    }
    if let Some(mapped) = claude_id_to_slot(&requested) {
        return Some(mapped);
    }
    // 兼容旧版 Claude 模型 ID
    if requested.contains("haiku") {
        return Some("haiku");
    }
    if requested.contains("sonnet") {
        if requested.contains("4-6") {
            return Some("sonnet_4_6");
        }
        if requested.contains("4-5") {
            return Some("sonnet_4_5");
        }
        return Some("sonnet");
    }
    if requested.contains("opus") {
        if requested.contains("4-7") {
            return Some("opus_4_7");
        }
        if requested.contains("4-6") {
            return Some("opus_4_6");
        }
        if requested.starts_with("claude-3") || requested.contains("-3-") || requested.ends_with("-3") {
            return Some("opus_3");
        }
        return Some("opus");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider_with(id: &str, name: &str, models: &[(&str, &str)]) -> Provider {
        let mut m = ModelMappings::new();
        for (k, v) in models {
            m.insert((*k).to_owned(), (*v).to_owned());
        }
        Provider {
            id: id.to_owned(),
            name: name.to_owned(),
            base_url: "https://example.com/anthropic".to_owned(),
            auth_scheme: "bearer".to_owned(),
            api_format: "anthropic".to_owned(),
            api_key: String::new(),
            models: m,
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
    fn model_slots_order_matches_upstream() {
        assert_eq!(
            model_order(),
            vec![
                "default",
                "opus_4_7",
                "opus_4_6",
                "opus_3",
                "sonnet_4_6",
                "sonnet_4_5",
                "haiku_4_5",
            ]
        );
    }

    #[test]
    fn claude_id_to_slot_maps_known_ids() {
        assert_eq!(claude_id_to_slot("claude-opus-4-7"), Some("opus_4_7"));
        assert_eq!(claude_id_to_slot("CLAUDE-SONNET-4-6"), Some("sonnet_4_6"));
        assert_eq!(claude_id_to_slot("claude-haiku-4-5"), Some("haiku_4_5"));
        assert_eq!(claude_id_to_slot("unknown-xyz"), None);
    }

    #[test]
    fn normalize_with_legacy_sonnet_fills_sonnet_4_6() {
        let mut m = IndexMap::new();
        m.insert("default".to_owned(), json!("kimi-k2.6"));
        m.insert("sonnet".to_owned(), json!("kimi-sonnet"));
        let normalized = normalize_model_mappings(Some(&m));
        assert_eq!(normalized.get("default").map(String::as_str), Some("kimi-k2.6"));
        assert_eq!(normalized.get("sonnet_4_6").map(String::as_str), Some("kimi-sonnet"));
        assert_eq!(normalized.get("sonnet_4_5").map(String::as_str), Some(""));
    }

    #[test]
    fn model_mappings_with_legacy_aliases_falls_back_to_default() {
        let mut m = IndexMap::new();
        m.insert("default".to_owned(), json!("kimi-k2.6"));
        let compat = model_mappings_with_legacy_aliases(Some(&m));
        assert_eq!(compat.get("sonnet").map(String::as_str), Some("kimi-k2.6"));
        assert_eq!(compat.get("opus").map(String::as_str), Some("kimi-k2.6"));
        assert_eq!(compat.get("haiku").map(String::as_str), Some("kimi-k2.6"));
    }

    #[test]
    fn provider_slug_strips_invalid_chars_and_truncates() {
        let p = provider_with("DeepSeek (官方)!", "X", &[]);
        let slug = provider_slug(&p);
        assert_eq!(slug, "deepseek");
    }

    #[test]
    fn provider_slug_falls_back_when_all_invalid() {
        let p = provider_with("@@@", "###", &[]);
        assert_eq!(provider_slug(&p), "provider");
    }

    #[test]
    fn provider_model_ids_dedups_and_orders() {
        let p = provider_with(
            "kimi",
            "Kimi",
            &[
                ("sonnet", "kimi-k2.6"),
                ("opus", "kimi-k2.6"),
                ("haiku", "kimi-k2.6"),
                ("default", "kimi-k2.6"),
            ],
        );
        let ids = provider_model_ids(&p);
        assert_eq!(ids, vec!["kimi-k2.6"], "同一模型只出现一次");
    }

    #[test]
    fn model_supports_1m_detects_bracket_marker() {
        let p = provider_with("deepseek", "DeepSeek", &[]);
        assert!(model_supports_1m(&p, "deepseek-v4-pro[1m]"));
        assert!(model_supports_1m(&p, "DEEPSEEK-V4-PRO[1M]"));
        assert!(!model_supports_1m(&p, "deepseek-v4-pro"));
    }

    #[test]
    fn model_supports_1m_reads_capability() {
        let mut p = provider_with("qwen", "Qwen", &[]);
        let mut caps = IndexMap::new();
        caps.insert("qwen3.6-plus".to_owned(), json!({"supports1m": true}));
        p.model_capabilities = caps;
        assert!(model_supports_1m(&p, "qwen3.6-plus"));
        assert!(!model_supports_1m(&p, "qwen3.6-flash"));
    }

    #[test]
    fn resolve_model_alias_returns_provider() {
        let providers = vec![provider_with(
            "deepseek",
            "DeepSeek",
            &[("default", "deepseek-v4-pro")],
        )];
        let (matched, model_id, did_resolve) =
            resolve_model_alias(&providers, "deepseek/deepseek-v4-pro");
        assert!(did_resolve);
        assert_eq!(matched.unwrap().id, "deepseek");
        assert_eq!(model_id, "deepseek-v4-pro");
    }

    #[test]
    fn resolve_requested_model_slot_known_ids() {
        assert_eq!(resolve_requested_model_slot("claude-opus-4-7"), Some("opus_4_7"));
        assert_eq!(resolve_requested_model_slot("claude-3-opus"), Some("opus_3"));
    }

    #[test]
    fn resolve_requested_model_slot_family_keywords() {
        assert_eq!(resolve_requested_model_slot("some-sonnet-thing"), Some("sonnet"));
        assert_eq!(resolve_requested_model_slot("haiku-mini"), Some("haiku"));
        assert_eq!(resolve_requested_model_slot("opus-pro"), Some("opus"));
        assert_eq!(resolve_requested_model_slot("opus-4-7"), Some("opus_4_7"));
    }

    #[test]
    fn resolve_requested_model_slot_returns_none_on_unknown() {
        assert_eq!(resolve_requested_model_slot(""), None);
        assert_eq!(resolve_requested_model_slot("gpt-5"), None);
    }
}
