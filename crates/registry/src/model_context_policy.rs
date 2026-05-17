use serde_json::Value;

use crate::{has_internal_one_m_suffix, strip_internal_model_suffix};

pub const ONE_M_CONTEXT_WINDOW: u64 = 1_000_000;

/// 与 `presets_data.json` 中 builtin preset 的 `modelCapabilities.context_window`
/// 对齐。用于在未显式配置 capability 时提供保守默认值。
pub fn documented_context_window(model_id: &str) -> Option<u64> {
    match model_id.trim().to_ascii_lowercase().as_str() {
        // DeepSeek
        "deepseek-v4-pro" | "deepseek-v4-flash" => Some(ONE_M_CONTEXT_WINDOW),
        // Kimi (月之暗面) + Kimi Code
        "kimi-for-coding" | "kimi-k2.5" | "kimi-k2.6" | "kimi-2.6" => Some(262_144),
        "moonshot-v1-8k" | "moonshot-v1-8k-vision-preview" => Some(8192),
        "moonshot-v1-32k" | "moonshot-v1-32k-vision-preview" => Some(32768),
        "moonshot-v1-128k" | "moonshot-v1-auto" | "moonshot-v1-128k-vision-preview" => {
            Some(131_072)
        }
        // Xiaomi MiMo
        "mimo-v2-pro" | "mimo-v2.5" | "mimo-v2.5-pro" => Some(ONE_M_CONTEXT_WINDOW),
        "mimo-v2-flash" | "mimo-v2-omni" => Some(262_144),
        // 智谱 GLM
        "glm-5.1" | "glm-4.7" => Some(200_000),
        // 阿里云百炼 Qwen 3.6
        "qwen3.6-plus" | "qwen3.6-flash" => Some(ONE_M_CONTEXT_WINDOW),
        // MiniMax
        "minimax-m2.7" => Some(204_800),
        // Google Gemini (当前 preset 默认模型)
        "gemini-3.1-flash-lite" | "gemini-2.5-flash" | "gemini-3-flash" => {
            Some(ONE_M_CONTEXT_WINDOW)
        }
        _ => None,
    }
}

/// 统一的 1M 判定策略:
/// 1. `[1m]` 内部后缀
/// 2. 文档化 context_window >= 1M
/// 3. `modelCapabilities[model].supports1m = true/false`
/// 4. `modelCapabilities[model].context_window >= 1_000_000`
pub fn model_supports_1m(original_model: &str, model_capabilities: Option<&Value>) -> bool {
    if has_internal_one_m_suffix(original_model) {
        return true;
    }
    let clean_model = strip_internal_model_suffix(original_model);
    let clean_model = clean_model.trim();
    if clean_model.is_empty() {
        return false;
    }

    if documented_context_window(clean_model).is_some_and(|n| n >= ONE_M_CONTEXT_WINDOW) {
        return true;
    }

    if let Some(b) = capability_bool(
        model_capabilities,
        original_model,
        clean_model,
        "supports1m",
    ) {
        return b;
    }

    capability_u64(
        model_capabilities,
        original_model,
        clean_model,
        "context_window",
    )
    .is_some_and(|n| n >= ONE_M_CONTEXT_WINDOW)
}

fn capability_bool(
    model_capabilities: Option<&Value>,
    original_model: &str,
    clean_model: &str,
    field: &str,
) -> Option<bool> {
    let caps = model_capabilities.and_then(Value::as_object)?;
    for key in capability_lookup_keys(original_model, clean_model) {
        if let Some(b) = caps
            .get(key.as_str())
            .and_then(|v| v.get(field))
            .and_then(Value::as_bool)
        {
            return Some(b);
        }
    }
    None
}

fn capability_u64(
    model_capabilities: Option<&Value>,
    original_model: &str,
    clean_model: &str,
    field: &str,
) -> Option<u64> {
    let caps = model_capabilities.and_then(Value::as_object)?;
    for key in capability_lookup_keys(original_model, clean_model) {
        if let Some(n) = caps
            .get(key.as_str())
            .and_then(|v| v.get(field))
            .and_then(Value::as_u64)
        {
            return Some(n);
        }
    }
    None
}

fn capability_lookup_keys(original_model: &str, clean_model: &str) -> Vec<String> {
    let mut keys = Vec::<String>::with_capacity(3);
    for candidate in [
        clean_model.trim(),
        original_model.trim(),
        &clean_model.to_ascii_lowercase(),
    ] {
        let c = candidate.trim();
        if !c.is_empty() && !keys.iter().any(|k| k == c) {
            keys.push(c.to_owned());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn documented_context_window_contains_builtin_defaults() {
        assert_eq!(documented_context_window("moonshot-v1-8k"), Some(8192));
        assert_eq!(documented_context_window("qwen3.6-plus"), Some(1_000_000));
        assert_eq!(documented_context_window("MiniMax-M2.7"), Some(204_800));
        assert_eq!(documented_context_window("unknown-model"), None);
    }

    #[test]
    fn model_supports_1m_accepts_documented_and_capability_paths() {
        assert!(model_supports_1m("deepseek-v4-pro", None));
        assert!(model_supports_1m("mimo-v2.5-pro", None));
        assert!(!model_supports_1m("moonshot-v1-32k", None));
        assert!(model_supports_1m("any-model[1m]", None));

        let caps = json!({
            "custom": {"supports1m": true},
            "small": {"context_window": 8192},
            "big": {"context_window": 1000000}
        });
        assert!(model_supports_1m("custom", Some(&caps)));
        assert!(!model_supports_1m("small", Some(&caps)));
        assert!(model_supports_1m("big", Some(&caps)));
    }

    #[test]
    fn capability_lookup_accepts_original_case_key() {
        let caps = json!({
            "MiniMax-M2.7": {"supports1m": true}
        });
        assert!(model_supports_1m("MiniMax-M2.7", Some(&caps)));
    }
}
