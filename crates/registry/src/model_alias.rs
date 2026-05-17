//! 模型别名 / 多 provider 路由(对应 `backend/model_alias.py`).

use indexmap::IndexMap;
use once_cell::sync::Lazy;

use crate::schema::ModelMappings;

pub struct ModelSlot {
    pub key: &'static str,
    pub legacy: &'static [&'static str],
    pub openai_id: Option<&'static str>,
}

pub static MODEL_SLOTS: &[ModelSlot] = &[
    ModelSlot {
        key: "default",
        legacy: &["default"],
        openai_id: None,
    },
    ModelSlot {
        key: "gpt_5_5",
        legacy: &[],
        openai_id: Some("gpt-5.5"),
    },
    ModelSlot {
        key: "gpt_5_4",
        legacy: &[],
        openai_id: Some("gpt-5.4"),
    },
    ModelSlot {
        key: "gpt_5_4_mini",
        legacy: &[],
        openai_id: Some("gpt-5.4-mini"),
    },
    ModelSlot {
        key: "gpt_5_3_codex",
        legacy: &[],
        openai_id: Some("gpt-5.3-codex"),
    },
    ModelSlot {
        key: "gpt_5_2",
        legacy: &[],
        openai_id: Some("gpt-5.2"),
    },
];

pub static MODEL_ORDER: Lazy<Vec<&'static str>> =
    Lazy::new(|| MODEL_SLOTS.iter().map(|s| s.key).collect());

pub const DEFAULT_MODEL_KEY: &str = "default";
const INTERNAL_ONE_M_SUFFIX: &str = "[1m]";

pub fn openai_model_slot(openai_id: &str) -> Option<&'static str> {
    let requested = openai_id.trim().to_ascii_lowercase();
    if requested.is_empty() {
        return None;
    }
    MODEL_SLOTS
        .iter()
        .find(|slot| slot.openai_id == Some(requested.as_str()))
        .map(|slot| slot.key)
}

pub fn provider_slug(provider: &crate::Provider) -> String {
    let source = if !provider.id.is_empty() {
        provider.id.as_str()
    } else if !provider.name.is_empty() {
        provider.name.as_str()
    } else {
        "provider"
    };
    slugify_provider_source(source)
}

fn slugify_provider_source(source: &str) -> String {
    let mut slug = String::new();
    let mut last_was_replacement = false;
    for ch in source.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-' {
            slug.push(ch);
            last_was_replacement = false;
        } else if !last_was_replacement {
            slug.push('-');
            last_was_replacement = true;
        }
    }

    let trimmed = slug
        .trim_matches(|ch| ch == '-' || ch == '_')
        .chars()
        .take(56)
        .collect::<String>();
    if trimmed.is_empty() {
        "provider".to_owned()
    } else {
        trimmed
    }
}

pub fn has_internal_one_m_suffix(model: &str) -> bool {
    model
        .trim()
        .to_ascii_lowercase()
        .ends_with(INTERNAL_ONE_M_SUFFIX)
}

pub fn strip_internal_model_suffix(model: &str) -> String {
    let trimmed = model.trim();
    if !has_internal_one_m_suffix(trimmed) {
        return trimmed.to_owned();
    }
    trimmed[..trimmed.len() - INTERNAL_ONE_M_SUFFIX.len()]
        .trim_end()
        .to_owned()
}

pub fn empty_model_mappings() -> ModelMappings {
    let mut map = IndexMap::with_capacity(MODEL_SLOTS.len());
    for slot in MODEL_SLOTS {
        map.insert(slot.key.to_owned(), String::new());
    }
    map
}

/// 与 Python `normalize_model_mappings` 等价:旧四槽位与新槽位的合并.
///
/// 行为:
/// - 输入为空 / 非映射 → 返回所有槽位为空字符串的映射
/// - `default` 直接拷贝
/// - 其他槽位:在 `[key, ...legacy]` 中找到第一个非空值
pub fn normalize_model_mappings(input: Option<&serde_json::Value>) -> ModelMappings {
    let mut out = empty_model_mappings();
    let Some(serde_json::Value::Object(src)) = input else {
        return out;
    };

    let default = src
        .get("default")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_owned())
        .unwrap_or_default();
    out.insert("default".to_owned(), default);

    for slot in MODEL_SLOTS {
        if slot.key == DEFAULT_MODEL_KEY {
            continue;
        }
        let mut filled = String::new();
        let candidates = std::iter::once(slot.key).chain(slot.legacy.iter().copied());
        for cand in candidates {
            if let Some(v) = src.get(cand).and_then(|v| v.as_str()) {
                let trimmed = v.trim();
                if !trimmed.is_empty() {
                    filled = trimmed.to_owned();
                    break;
                }
            }
        }
        out.insert(slot.key.to_owned(), filled);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_input_returns_all_blank() {
        let m = normalize_model_mappings(None);
        assert_eq!(m.len(), MODEL_SLOTS.len());
        for slot in MODEL_SLOTS {
            assert_eq!(m[slot.key], "");
        }
    }

    #[test]
    fn default_slot_passthrough() {
        let v = json!({"default": "deepseek-v4-pro"});
        let m = normalize_model_mappings(Some(&v));
        assert_eq!(m["default"], "deepseek-v4-pro");
    }

    #[test]
    fn legacy_default_carries_to_default_slot() {
        // Python 版的 legacy = ("default",) 仅对 default 槽适用,这里验证它
        let v = json!({"default": "  glm-5.1  "});
        let m = normalize_model_mappings(Some(&v));
        assert_eq!(m["default"], "glm-5.1", "应 trim 空白");
    }

    #[test]
    fn openai_model_slot_maps_current_codex_slugs() {
        assert_eq!(openai_model_slot("gpt-5.5"), Some("gpt_5_5"));
        assert_eq!(openai_model_slot("gpt-5.4-mini"), Some("gpt_5_4_mini"));
        assert_eq!(openai_model_slot(" GPT-5.5 "), Some("gpt_5_5"));
        assert_eq!(openai_model_slot("unknown"), None);
    }

    #[test]
    fn provider_slug_matches_legacy_python_rules() {
        let mut provider = crate::Provider {
            id: "OpenAI.Custom_1".into(),
            name: "Ignored Name".into(),
            base_url: String::new(),
            auth_scheme: String::new(),
            api_format: String::new(),
            api_key: String::new(),
            models: IndexMap::new(),
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        };
        assert_eq!(provider_slug(&provider), "openai-custom_1");

        provider.id.clear();
        provider.name = "七牛 / Qiniu++".into();
        assert_eq!(provider_slug(&provider), "qiniu");

        provider.name = "___".into();
        assert_eq!(provider_slug(&provider), "provider");
    }

    #[test]
    fn strip_internal_model_suffix_only_strips_one_m_marker() {
        assert_eq!(
            strip_internal_model_suffix("deepseek-v4-pro[1m]"),
            "deepseek-v4-pro"
        );
        assert_eq!(
            strip_internal_model_suffix("deepseek-v4-pro [1M]"),
            "deepseek-v4-pro"
        );
        assert_eq!(
            strip_internal_model_suffix("deepseek-v4-pro[beta]"),
            "deepseek-v4-pro[beta]"
        );
        assert_eq!(
            strip_internal_model_suffix("deepseek-v4-pro[1m-preview]"),
            "deepseek-v4-pro[1m-preview]"
        );
    }

    #[test]
    fn key_order_is_stable() {
        let m = empty_model_mappings();
        let keys: Vec<_> = m.keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                "default",
                "gpt_5_5",
                "gpt_5_4",
                "gpt_5_4_mini",
                "gpt_5_3_codex",
                "gpt_5_2",
            ]
        );
    }
}
