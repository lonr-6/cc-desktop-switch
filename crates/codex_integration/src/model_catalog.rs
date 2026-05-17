//! Codex model catalog updater.
//!
//! Codex CLI 0.128+ reads `model_catalog_json` for per-model context windows.
//! The older `model_context_window` root key is kept by `apply.rs` as a
//! compatibility hint, but the catalog is the path verified against current
//! Codex releases.
//!
//! The catalog is merged into Codex App Transfer's existing
//! `~/.codex-app-transfer/config.json` file instead of creating another file
//! under `~/.codex`. Codex ignores unrelated top-level fields and reads the
//! `models` array from the configured JSON path.

use codex_app_transfer_registry::{
    documented_context_window, load_raw_config, model_supports_1m, normalize_model_mappings,
    save_raw_config, strip_internal_model_suffix, MODEL_SLOTS,
};
use serde_json::{json, Value};

use crate::CodexError;

pub const CODEX_MODEL_CATALOG_KEY: &str = "model_catalog_json";

const DEFAULT_EFFECTIVE_CONTEXT_WINDOW_PERCENT: u64 = 95;
const DEFAULT_CONTEXT_WINDOW: u64 = 258_400;
const ONE_M_CONTEXT_WINDOW: u64 = 1_000_000;

/// Codex CLI 触发自动 compact 的阈值百分比:`auto_compact_token_limit = context_window × 75%`。
///
/// 根因:Codex CLI 在 `total_usage_tokens >= auto_compact_token_limit` 时触发摘要
/// (`codex-rs/core/src/session/turn.rs:736-748`),如果 catalog model 没写这个字段
/// 会 fallback `i64::MAX` → **永不触发**(实测 245K 上限 90% 仍不动)。
///
/// 75% 留 25% buffer 给 summarize 请求本身(~20K max_output_tokens + system
/// prompt + 当轮 input + 工具开销)。256K 触发于 192K,1M 触发于 750K,与
/// litellm `effective_context_window_percent` 行业惯例对齐。
const AUTO_COMPACT_TRIGGER_PERCENT: u64 = 75;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    pub slug: String,
    pub display_name: String,
    pub context_window: u64,
    pub effective_context_window_percent: u64,
}

pub fn upsert_catalog_models(
    path: &std::path::Path,
    models: &[CatalogModel],
) -> Result<(), CodexError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut data = read_json_object(path)?;
    data["models"] = Value::Array(models.iter().map(model_to_json).collect::<Vec<_>>());
    save_raw_config(path, &data)?;
    Ok(())
}

pub fn clear_catalog_models(path: &std::path::Path) -> Result<(), CodexError> {
    let mut data = match load_raw_config(path) {
        Ok(Value::Object(map)) => Value::Object(map),
        Ok(_) => return Ok(()),
        Err(codex_app_transfer_registry::IoError::NotFound(_)) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    let Some(obj) = data.as_object_mut() else {
        return Ok(());
    };
    if obj.remove("models").is_some() {
        save_raw_config(path, &data)?;
    }
    Ok(())
}

fn read_json_object(path: &std::path::Path) -> Result<Value, CodexError> {
    match load_raw_config(path) {
        Ok(Value::Object(map)) => Ok(Value::Object(map)),
        Ok(_) => Ok(default_registry_config_value()),
        Err(codex_app_transfer_registry::IoError::NotFound(_)) => {
            Ok(default_registry_config_value())
        }
        Err(e) => Err(e.into()),
    }
}

fn default_registry_config_value() -> Value {
    serde_json::to_value(codex_app_transfer_registry::Config::default())
        .unwrap_or_else(|_| json!({}))
}

pub fn catalog_models_for_provider(
    provider_name: &str,
    default_model: &str,
    supports_1m: bool,
    model_mappings: Option<&Value>,
    model_capabilities: Option<&Value>,
) -> Vec<CatalogModel> {
    let default_model_clean = strip_internal_model_suffix(default_model);
    let default_model = default_model_clean.trim();
    let mappings = normalize_model_mappings(model_mappings);
    let mut models = Vec::new();
    for slot in MODEL_SLOTS {
        let Some(openai_id) = slot.openai_id else {
            continue;
        };
        let mapped = mappings.get(slot.key).map(|s| s.trim()).unwrap_or("");
        let target = if mapped.is_empty() {
            default_model
        } else {
            mapped
        };
        let target_clean = strip_internal_model_suffix(target);
        let context_window = context_window_for_model(
            target,
            target_clean.trim(),
            default_model,
            supports_1m,
            model_capabilities,
        );
        models.push(catalog_model(
            openai_id,
            provider_name,
            target_clean.trim(),
            context_window,
        ));
    }
    if !default_model.is_empty() && !models.iter().any(|m| m.slug == default_model) {
        // 与上面 MODEL_SLOTS 走同一条 context_window_for_model,保证显式
        // model_capabilities[<default>].context_window 在 fallback entry 上
        // 也生效(2026-05-07 fix:之前这里硬编码二档值,绕过显式 override)。
        let fallback_window = context_window_for_model(
            default_model,
            default_model,
            default_model,
            supports_1m,
            model_capabilities,
        );
        models.push(catalog_model(
            default_model,
            provider_name,
            default_model,
            fallback_window,
        ));
    }
    models
}

pub fn strip_model_suffix(model: &str) -> String {
    strip_internal_model_suffix(model)
}

fn context_window_for_model(
    original_model: &str,
    clean_model: &str,
    default_model: &str,
    default_supports_1m: bool,
    model_capabilities: Option<&Value>,
) -> u64 {
    if clean_model.is_empty() {
        return DEFAULT_CONTEXT_WINDOW;
    }
    // 1. 最高优先级:`model_capabilities[<model>].context_window` 数值显式声明
    //    (2026-05-07 新增,替代旧版只能在 258_400 / 1_000_000 二档之间的限制)
    //    数值 ≥ 1024 才认(防止误填导致 Codex CLI 把 context_window 设成 0
    //    或负值);clean_model 优先,fallback 到 original_model(含可能的 [1m]
    //    后缀)。
    if let Some(n) = explicit_context_window(original_model, clean_model, model_capabilities) {
        return n;
    }
    if let Some(n) = documented_context_window(clean_model) {
        return n;
    }
    // 2. 二档 fallback:default_model + supports_1m / known prefix / supports1m bool
    if clean_model == default_model {
        if default_supports_1m {
            ONE_M_CONTEXT_WINDOW
        } else {
            DEFAULT_CONTEXT_WINDOW
        }
    } else if model_supports_1m(original_model, model_capabilities) {
        ONE_M_CONTEXT_WINDOW
    } else {
        DEFAULT_CONTEXT_WINDOW
    }
}

fn explicit_context_window(
    original_model: &str,
    clean_model: &str,
    model_capabilities: Option<&Value>,
) -> Option<u64> {
    let caps = model_capabilities.and_then(Value::as_object)?;
    for key in [clean_model, original_model.trim()] {
        if key.is_empty() {
            continue;
        }
        if let Some(n) = caps
            .get(key)
            .and_then(|v| v.get("context_window"))
            .and_then(Value::as_u64)
        {
            // 防御:< 1024 token 没法跑 Codex 系统提示,认为是配置错误,
            // 让 fallback 接管(走 supports_1m 二档)。
            if n >= 1024 {
                return Some(n);
            }
        }
    }
    None
}

fn catalog_model(
    slug: &str,
    provider_name: &str,
    default_model: &str,
    context_window: u64,
) -> CatalogModel {
    let target = if default_model.is_empty() {
        slug
    } else {
        default_model
    };
    CatalogModel {
        slug: slug.to_owned(),
        display_name: format!("{provider_name} / {target}"),
        context_window,
        effective_context_window_percent: DEFAULT_EFFECTIVE_CONTEXT_WINDOW_PERCENT,
    }
}

fn model_to_json(model: &CatalogModel) -> Value {
    let mut entry = codex_builtin_template(&model.slug).unwrap_or_else(generic_model_template);
    entry["slug"] = Value::String(model.slug.clone());
    entry["display_name"] = Value::String(model.display_name.clone());
    entry["description"] = Value::String(format!(
        "Routed through Codex App Transfer as {}.",
        model.display_name
    ));
    entry["context_window"] = json!(model.context_window);
    entry["max_context_window"] = json!(model.context_window);
    entry["effective_context_window_percent"] = json!(model.effective_context_window_percent);
    // 不写这个字段会让 Codex CLI fallback i64::MAX(永不触发自动 compact)。
    // catalog 字段格式见 codex-rs/protocol/src/openai_models.rs:298。
    entry["auto_compact_token_limit"] = json!(
        model
            .context_window
            .saturating_mul(AUTO_COMPACT_TRIGGER_PERCENT)
            / 100
    );
    entry
}

fn codex_builtin_template(slug: &str) -> Option<Value> {
    match slug {
        "gpt-5.5" => Some(codex_model_template(
            slug,
            "GPT-5.5",
            "Frontier model for complex coding, research, and real-world work.",
            "medium",
            codex_reasoning_levels(),
            0,
            json!(["fast"]),
            json!({
                "message": "GPT-5.5 is now available in Codex. It's our strongest agentic coding model yet, built to reason through large codebases, check assumptions with tools, and keep going until the work is done.\n\nLearn more: https://openai.com/index/introducing-gpt-5-5/\n\n"
            }),
            Value::Null,
            "none",
            "low",
            "text_and_image",
            json!({"mode": "tokens", "limit": 10000}),
            true,
        )),
        "gpt-5.4" => Some(codex_model_template(
            slug,
            "gpt-5.4",
            "Strong model for everyday coding.",
            "xhigh",
            codex_reasoning_levels(),
            2,
            json!(["fast"]),
            Value::Null,
            Value::Null,
            "none",
            "low",
            "text_and_image",
            json!({"mode": "tokens", "limit": 10000}),
            true,
        )),
        "gpt-5.4-mini" => Some(codex_model_template(
            slug,
            "GPT-5.4-Mini",
            "Small, fast, and cost-efficient model for simpler coding tasks.",
            "medium",
            codex_reasoning_levels(),
            4,
            json!([]),
            Value::Null,
            Value::Null,
            "none",
            "medium",
            "text_and_image",
            json!({"mode": "tokens", "limit": 10000}),
            true,
        )),
        "gpt-5.3-codex" => Some(codex_model_template(
            slug,
            "gpt-5.3-codex",
            "Coding-optimized model.",
            "medium",
            codex_reasoning_levels(),
            6,
            json!([]),
            Value::Null,
            gpt54_upgrade(),
            "none",
            "low",
            "text",
            json!({"mode": "tokens", "limit": 10000}),
            true,
        )),
        "gpt-5.2" => Some(codex_model_template(
            slug,
            "gpt-5.2",
            "Optimized for professional work and long-running agents.",
            "medium",
            gpt52_reasoning_levels(),
            10,
            json!([]),
            Value::Null,
            gpt54_upgrade(),
            "auto",
            "low",
            "text",
            json!({"mode": "bytes", "limit": 10000}),
            false,
        )),
        _ => None,
    }
}

fn codex_model_template(
    slug: &str,
    display_name: &str,
    description: &str,
    default_reasoning_level: &str,
    supported_reasoning_levels: Value,
    priority: u64,
    additional_speed_tiers: Value,
    availability_nux: Value,
    upgrade: Value,
    default_reasoning_summary: &str,
    default_verbosity: &str,
    web_search_tool_type: &str,
    truncation_policy: Value,
    supports_image_detail_original: bool,
) -> Value {
    json!({
        "slug": slug,
        "display_name": display_name,
        "description": description,
        "default_reasoning_level": default_reasoning_level,
        "supported_reasoning_levels": supported_reasoning_levels,
        "shell_type": "shell_command",
        "visibility": "list",
        "supported_in_api": true,
        "priority": priority,
        "additional_speed_tiers": additional_speed_tiers,
        "availability_nux": availability_nux,
        "upgrade": upgrade,
        "base_instructions": "",
        "supports_reasoning_summaries": true,
        "default_reasoning_summary": default_reasoning_summary,
        "support_verbosity": true,
        "default_verbosity": default_verbosity,
        "apply_patch_tool_type": "freeform",
        "web_search_tool_type": web_search_tool_type,
        "truncation_policy": truncation_policy,
        "supports_parallel_tool_calls": true,
        "supports_image_detail_original": supports_image_detail_original,
        "context_window": 272000,
        "max_context_window": 272000,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": true
    })
}

fn generic_model_template() -> Value {
    json!({
        "slug": "",
        "display_name": "",
        "description": "",
        "default_reasoning_level": "high",
        "supported_reasoning_levels": [
            {"effort": "low", "description": "Fast responses with lighter reasoning"},
            {"effort": "medium", "description": "Balanced speed and reasoning depth"},
            {"effort": "high", "description": "Greater reasoning depth for complex tasks"}
        ],
        "shell_type": "default",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 10,
        "additional_speed_tiers": [],
        "availability_nux": null,
        "upgrade": null,
        "base_instructions": "",
        "supports_reasoning_summaries": false,
        "default_reasoning_summary": "auto",
        "support_verbosity": false,
        "default_verbosity": null,
        "apply_patch_tool_type": null,
        "web_search_tool_type": "text",
        "truncation_policy": {"mode": "bytes", "limit": 4000000},
        "supports_parallel_tool_calls": false,
        "supports_image_detail_original": false,
        "context_window": 258400,
        "max_context_window": 258400,
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text", "image"],
        "supports_search_tool": false
    })
}

fn codex_reasoning_levels() -> Value {
    json!([
        {"effort": "low", "description": "Fast responses with lighter reasoning"},
        {"effort": "medium", "description": "Balances speed and reasoning depth for everyday tasks"},
        {"effort": "high", "description": "Greater reasoning depth for complex problems"},
        {"effort": "xhigh", "description": "Extra high reasoning depth for complex problems"}
    ])
}

fn gpt52_reasoning_levels() -> Value {
    json!([
        {"effort": "low", "description": "Balances speed with some reasoning; useful for straightforward queries and short explanations"},
        {"effort": "medium", "description": "Provides a solid balance of reasoning depth and latency for general-purpose tasks"},
        {"effort": "high", "description": "Maximizes reasoning depth for complex or ambiguous problems"},
        {"effort": "xhigh", "description": "Extra high reasoning for complex problems"}
    ])
}

fn gpt54_upgrade() -> Value {
    json!({
        "model": "gpt-5.4",
        "migration_markdown": "Introducing GPT-5.4\n\nCodex just got an upgrade with GPT-5.4, our most capable model for professional work. It outperforms prior models while being more token efficient, with notable improvements on long-running tasks, tool calling, computer use, and frontend development.\n\nLearn more: https://openai.com/index/introducing-gpt-5-4\n\nYou can always keep using GPT-5.3-Codex if you prefer.\n"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_suffix_keeps_upstream_model_id_clean() {
        assert_eq!(strip_model_suffix("deepseek-v4-pro[1m]"), "deepseek-v4-pro");
        assert_eq!(
            strip_model_suffix("deepseek-v4-pro [1M]"),
            "deepseek-v4-pro"
        );
        assert_eq!(strip_model_suffix("deepseek-v4-pro"), "deepseek-v4-pro");
        assert_eq!(
            strip_model_suffix("deepseek-v4-pro[beta]"),
            "deepseek-v4-pro[beta]"
        );
    }

    #[test]
    fn one_m_catalog_uses_95_percent_effective_window() {
        let models =
            catalog_models_for_provider("DeepSeek", "deepseek-v4-pro[1m]", true, None, None);
        let deepseek = models.iter().find(|m| m.slug == "deepseek-v4-pro").unwrap();
        assert_eq!(deepseek.context_window, 1_000_000);
        assert_eq!(deepseek.effective_context_window_percent, 95);
        assert!(models.iter().any(|m| m.slug == "gpt-5.5"));
    }

    #[test]
    fn builtin_slug_catalog_preserves_codex_capabilities() {
        let models = catalog_models_for_provider("DeepSeek", "deepseek-v4-pro", true, None, None);
        let gpt55 = models.iter().find(|m| m.slug == "gpt-5.5").unwrap();
        let entry = model_to_json(gpt55);

        assert_eq!(entry["context_window"], 1_000_000);
        assert_eq!(entry["max_context_window"], 1_000_000);
        assert_eq!(entry["apply_patch_tool_type"], "freeform");
        assert_eq!(entry["supports_parallel_tool_calls"], true);
        assert_eq!(entry["supports_search_tool"], true);
        assert_eq!(entry["supports_reasoning_summaries"], true);
        assert_eq!(entry["web_search_tool_type"], "text_and_image");
    }

    #[test]
    fn catalog_model_writes_auto_compact_token_limit_at_75_percent() {
        // 1M context: 触发于 750K(留 25% buffer)
        let big = catalog_models_for_provider("Big", "deepseek-v4-pro", true, None, None);
        let entry = model_to_json(big.iter().find(|m| m.slug == "gpt-5.5").unwrap());
        assert_eq!(entry["context_window"], 1_000_000);
        assert_eq!(
            entry["auto_compact_token_limit"], 750_000,
            "1M provider 应在 75% (750K) 触发自动 compact"
        );

        // 258_400 context(默认 supports_1m=false):触发于 193_800
        let mid = catalog_models_for_provider("Mid", "mock-model", false, None, None);
        let entry_mid = model_to_json(mid.iter().find(|m| m.slug == "gpt-5.5").unwrap());
        assert_eq!(entry_mid["context_window"], 258_400);
        assert_eq!(
            entry_mid["auto_compact_token_limit"], 193_800,
            "默认 258K provider 应在 75% (193_800) 触发自动 compact"
        );

        // 显式 32K context(moonshot-v1-32k): 触发于 24_576
        let mappings = json!({"default": "moonshot-v1-32k"});
        let capabilities = json!({"moonshot-v1-32k": {"context_window": 32_768}});
        let small = catalog_models_for_provider(
            "Moonshot",
            "moonshot-v1-32k",
            false,
            Some(&mappings),
            Some(&capabilities),
        );
        let entry_small = model_to_json(small.iter().find(|m| m.slug == "gpt-5.5").unwrap());
        assert_eq!(entry_small["context_window"], 32_768);
        assert_eq!(
            entry_small["auto_compact_token_limit"], 24_576,
            "32K context 应在 75% (24_576) 触发"
        );
    }

    #[test]
    fn catalog_uses_slot_mapping_and_per_model_windows() {
        let mappings = json!({
            "default": "deepseek-v4-pro",
            "gpt_5_5": "short-context-model",
            "gpt_5_4": "qwen3.6-plus",
            "gpt_5_4_mini": "custom-long-model"
        });
        let capabilities = json!({
            "short-context-model": {"supports1m": false},
            "custom-long-model": {"supports1m": true}
        });

        let models = catalog_models_for_provider(
            "Mixed",
            "deepseek-v4-pro",
            true,
            Some(&mappings),
            Some(&capabilities),
        );
        let gpt55 = models.iter().find(|m| m.slug == "gpt-5.5").unwrap();
        let gpt54 = models.iter().find(|m| m.slug == "gpt-5.4").unwrap();
        let mini = models.iter().find(|m| m.slug == "gpt-5.4-mini").unwrap();
        let codex = models.iter().find(|m| m.slug == "gpt-5.3-codex").unwrap();

        assert_eq!(gpt55.display_name, "Mixed / short-context-model");
        assert_eq!(gpt55.context_window, 258_400);
        assert_eq!(gpt54.display_name, "Mixed / qwen3.6-plus");
        assert_eq!(gpt54.context_window, 1_000_000);
        assert_eq!(mini.context_window, 1_000_000);
        assert_eq!(
            codex.display_name, "Mixed / deepseek-v4-pro",
            "empty slot mappings should still document the default fallback target"
        );
        assert_eq!(codex.context_window, 1_000_000);
    }

    // ── 新增 (2026-05-07):model_capabilities[<model>].context_window 数值
    // ── 显式声明优先级最高,替代旧版只能 258_400/1_000_000 二档限制
    //
    // 用户实际接的非 GPT 模型 context window 五花八门,旧版只能在两个写死值
    // 之间选,导致:
    // - mimo-v2.5-pro 真实 1M 但被 catalog 标 258_400(用户实测被早压缩 75%)
    // - moonshot-v1-8k 真实 8192 但被 catalog 标 258_400(理论上 codex 不
    //   截输入,上游收到大 body 直接 400)

    #[test]
    fn explicit_context_window_overrides_two_tier_default() {
        // user 显式标 mimo-v2.5-pro: { context_window: 1_000_000 } → 走数值
        let mappings = json!({"default": "mimo-v2.5-pro"});
        let capabilities = json!({
            "mimo-v2.5-pro": {"context_window": 1_000_000}
        });
        let models = catalog_models_for_provider(
            "Xiaomi MiMo",
            "mimo-v2.5-pro",
            false, // supports_1m=false 但显式 capability 应当胜出
            Some(&mappings),
            Some(&capabilities),
        );
        let entry = models.iter().find(|m| m.slug == "mimo-v2.5-pro").unwrap();
        assert_eq!(
            entry.context_window, 1_000_000,
            "显式 context_window 必须越过 supports_1m=false 的二档默认 258_400"
        );
    }

    #[test]
    fn explicit_context_window_supports_arbitrary_values() {
        // moonshot-v1-32k 真实 32768 — 既不是 258_400 也不是 1_000_000
        let mappings = json!({"default": "moonshot-v1-32k"});
        let capabilities = json!({
            "moonshot-v1-32k": {"context_window": 32_768}
        });
        let models = catalog_models_for_provider(
            "Moonshot",
            "moonshot-v1-32k",
            false,
            Some(&mappings),
            Some(&capabilities),
        );
        let entry = models.iter().find(|m| m.slug == "moonshot-v1-32k").unwrap();
        assert_eq!(entry.context_window, 32_768);
    }

    #[test]
    fn explicit_context_window_per_slot_independent() {
        // 同一 provider 内不同模型不同 context_window:gpt-5.5 → moonshot-v1-8k
        // (8192),gpt-5.4 → kimi-k2.6 (262144)。各 slot 不互相污染。
        let mappings = json!({
            "default": "kimi-k2.6",
            "gpt_5_5": "moonshot-v1-8k",
            "gpt_5_4": "kimi-k2.6",
        });
        let capabilities = json!({
            "moonshot-v1-8k": {"context_window": 8_192},
            "kimi-k2.6":     {"context_window": 262_144},
        });
        let models = catalog_models_for_provider(
            "Moonshot",
            "kimi-k2.6",
            false,
            Some(&mappings),
            Some(&capabilities),
        );
        let m55 = models.iter().find(|m| m.slug == "gpt-5.5").unwrap();
        let m54 = models.iter().find(|m| m.slug == "gpt-5.4").unwrap();
        assert_eq!(m55.context_window, 8_192, "gpt-5.5 → moonshot-v1-8k");
        assert_eq!(m54.context_window, 262_144, "gpt-5.4 → kimi-k2.6");
    }

    #[test]
    fn explicit_context_window_below_minimum_falls_back() {
        // 防御:context_window < 1024 视为配置错误,fallback 到二档逻辑。
        let mappings = json!({"default": "deepseek-v4-pro"});
        let capabilities = json!({
            "deepseek-v4-pro": {"context_window": 100}
        });
        let models = catalog_models_for_provider(
            "DeepSeek",
            "deepseek-v4-pro",
            true, // supports_1m
            Some(&mappings),
            Some(&capabilities),
        );
        let entry = models.iter().find(|m| m.slug == "deepseek-v4-pro").unwrap();
        assert_eq!(
            entry.context_window, 1_000_000,
            "非法值 100 应被忽略,fallback 到 supports_1m=true 的 1M"
        );
    }

    #[test]
    fn explicit_context_window_overrides_supports1m_capability_too() {
        // 既显式 supports1m=true 又显式 context_window=512_000 → context_window 胜出
        let mappings = json!({"default": "custom"});
        let capabilities = json!({
            "custom": {"supports1m": true, "context_window": 512_000}
        });
        let models = catalog_models_for_provider(
            "Custom",
            "custom",
            false,
            Some(&mappings),
            Some(&capabilities),
        );
        let entry = models.iter().find(|m| m.slug == "custom").unwrap();
        assert_eq!(entry.context_window, 512_000);
    }

    #[test]
    fn no_explicit_context_window_keeps_two_tier_fallback() {
        // 没填 context_window、且非文档内置模型:旧逻辑,supports_1m=true 走 1M,false 走 258_400
        let mappings = json!({"default": "undocumented-custom-model"});
        let models = catalog_models_for_provider(
            "Custom",
            "undocumented-custom-model",
            false,
            Some(&mappings),
            None,
        );
        let entry = models
            .iter()
            .find(|m| m.slug == "undocumented-custom-model")
            .unwrap();
        assert_eq!(entry.context_window, 258_400, "fallback to 258_400");
    }

    #[test]
    fn kimi_for_coding_uses_kimi_cli_documented_context_window() {
        // Kimi Code CLI 官方示例: max_context_size = 262144 (≠ DEFAULT_CONTEXT_WINDOW)
        let models = catalog_models_for_provider("Kimi Code", "kimi-for-coding", false, None, None);
        let gpt55 = models.iter().find(|m| m.slug == "gpt-5.5").unwrap();
        assert_eq!(gpt55.context_window, 262_144);
    }

    #[test]
    fn kimi_k2_6_and_mimo_v2_5_use_documented_context_without_capabilities() {
        let kimi = catalog_models_for_provider("Kimi", "kimi-k2.6", false, None, None);
        assert_eq!(
            kimi.iter()
                .find(|m| m.slug == "gpt-5.5")
                .unwrap()
                .context_window,
            262_144
        );
        let kimi_alt = catalog_models_for_provider("Kimi", "kimi-2.6", false, None, None);
        assert_eq!(
            kimi_alt
                .iter()
                .find(|m| m.slug == "gpt-5.5")
                .unwrap()
                .context_window,
            262_144
        );

        let mimo = catalog_models_for_provider("MiMo", "mimo-v2.5", false, None, None);
        assert_eq!(
            mimo.iter()
                .find(|m| m.slug == "gpt-5.5")
                .unwrap()
                .context_window,
            1_000_000
        );
        let mimo_pro = catalog_models_for_provider("MiMo", "mimo-v2.5-pro", false, None, None);
        assert_eq!(
            mimo_pro
                .iter()
                .find(|m| m.slug == "gpt-5.5")
                .unwrap()
                .context_window,
            1_000_000
        );
    }

    #[test]
    fn documented_context_aligns_with_builtin_preset_model_capabilities() {
        let cases = [
            ("moonshot-v1-8k", 8192u64),
            ("moonshot-v1-32k", 32_768),
            ("moonshot-v1-auto", 131_072),
            ("glm-5.1", 200_000),
            ("minimax-m2.7", 204_800),
            ("qwen3.6-plus", 1_000_000),
            ("gemini-3.1-flash-lite", 1_000_000),
            ("deepseek-v4-flash", 1_000_000),
            ("mimo-v2-omni", 262_144),
        ];
        for (model_id, want) in cases {
            let models = catalog_models_for_provider("P", model_id, false, None, None);
            let row = models.iter().find(|m| m.slug == "gpt-5.5").unwrap();
            assert_eq!(row.context_window, want, "model_id={model_id}");
        }
    }

    #[test]
    fn clear_catalog_models_removes_only_top_level_catalog_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = serde_json::json!({
            "version": "1.0.4",
            "providers": [],
            "models": [{"slug": "gpt-5.5"}],
            "settings": {"theme": "default"}
        });
        codex_app_transfer_registry::save_raw_config(&path, &original).unwrap();

        clear_catalog_models(&path).unwrap();

        let v = codex_app_transfer_registry::load_raw_config(&path).unwrap();
        assert_eq!(v["version"], "1.0.4");
        assert_eq!(v["settings"]["theme"], "default");
        assert!(v.get("models").is_none());
    }

    #[test]
    fn upsert_catalog_models_preserves_existing_config_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = serde_json::json!({
            "version": "1.0.4",
            "activeProvider": null,
            "gatewayApiKey": "cas_test",
            "providers": [],
            "settings": {
                "theme": "default",
                "language": "zh",
                "proxyPort": 18080,
                "adminPort": 18081,
                "autoStart": false,
                "autoApplyOnStart": true,
                "exposeAllProviderModels": false,
                "restoreCodexOnExit": true,
                "updateUrl": "https://github.com/Cmochance/codex-app-transfer/releases/latest/download/latest.json"
            }
        });
        codex_app_transfer_registry::save_raw_config(&path, &original).unwrap();

        let models = catalog_models_for_provider("DeepSeek", "deepseek-v4-pro", true, None, None);
        upsert_catalog_models(&path, &models).unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes.ends_with(b"\n"),
            "main config.json keeps existing no-newline convention"
        );
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["version"], "1.0.4");
        assert_eq!(v["gatewayApiKey"], "cas_test");
        assert_eq!(v["settings"]["theme"], "default");
        assert!(v["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["slug"] == "deepseek-v4-pro"));
        let _typed: codex_app_transfer_registry::Config =
            serde_json::from_value(v).expect("top-level models must not break registry config");
    }
}
