//! `/api/providers/*` + `/api/presets` —— provider 增删改 / 测连通性 /
//! 模型抓取 / 余额 / 内置 presets.
//!
//! 二级拆分:
//! - `crud`:增删改 + activate / reorder / draft / update_models
//! - `test`:连通性测试 + compatibility
//! - `models`:模型列表抓取 + autofill
//! - `balance`:余额 / 用量查询
//! - `presets`:内置 presets

pub mod balance;
pub mod crud;
pub mod models;
pub mod presets;
pub mod test;

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use codex_app_transfer_registry::{
    model_supports_1m, normalize_model_mappings, strip_internal_model_suffix, RawConfig,
    MODEL_ORDER,
};
use serde_json::{json, Value};

static ID_COUNTER: AtomicU32 = AtomicU32::new(0);

pub(super) fn fresh_provider_id(existing: &[String]) -> String {
    loop {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u32)
            .unwrap_or(0);
        let counter = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let candidate = format!("{:08x}", nanos.wrapping_add(counter));
        if !existing.iter().any(|id| id == &candidate) {
            return candidate;
        }
    }
}

pub(super) fn provider_supports_1m(provider: &Value) -> bool {
    let default_raw = provider
        .get("models")
        .and_then(|m| m.get("default"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    model_supports_1m(default_raw, provider.get("modelCapabilities"))
}

pub(super) fn provider_default_model(provider: &Value) -> String {
    let raw = provider
        .get("models")
        .and_then(|m| m.get("default"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    strip_internal_model_suffix(raw)
}

pub(super) fn provider_model_mappings(provider: &Value) -> Value {
    provider.get("models").cloned().unwrap_or_else(|| json!({}))
}

pub(super) fn provider_model_capabilities(provider: &Value) -> Value {
    provider
        .get("modelCapabilities")
        .cloned()
        .unwrap_or_else(|| json!({}))
}

pub(super) fn provider_display_name(provider: &Value) -> String {
    provider
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Provider")
        .to_owned()
}

/// 把 `provider.apiFormat` 字段的字面值规范化成后端可持久化的 canonical 值。
///
/// **未知值 / 缺失 fallback 到 `"openai_chat"`**(跟 `Provider::api_format`
/// schema serde default 一致),这是项目的核心默认行为:**所有 provider 默认
/// 走代理,代理负责 chat ↔ responses 协议转换 + extras 注入 + model 改写**。
///
/// `responses` / `openai_responses` 保持 OpenAI Responses 语义。历史
/// `anthropic` / `claude` / `messages` 别名现在归一到 `anthropic_messages`,
/// 交给 AnthropicMessagesAdapter 做 Responses ↔ Anthropic Messages 转换。
pub(super) fn normalize_provider_api_format(api_format: Option<&str>) -> &'static str {
    let normalized = api_format
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_");
    match normalized.as_str() {
        "responses" | "openai_responses" => "responses",
        "anthropic_messages" | "anthropic" | "claude" | "messages" | "claude_messages" => {
            "anthropic_messages"
        }
        // Gemini native generateContent path(GeminiNativeAdapter)— 测速 / compat
        // 走 `/v1beta/models` 探测 + `x-goog-api-key` header,跟 chat completions
        // 完全不同的协议形态,必须独立分支。
        "gemini_native" | "google_ai_studio" | "gemini" => "gemini_native",
        "gemini_cli_oauth" | "gemini_cli" | "gemini_oauth" | "google_oauth_cloud_code" => {
            "gemini_cli_oauth"
        }
        "antigravity_oauth" | "antigravity" | "google_oauth_antigravity" => "antigravity_oauth",
        "grok_web" | "grok" | "grok_com" => "grok_web",
        // openai / openai_chat / chat_completions / 空字符串 / 未知值
        // → 一律走 "openai_chat" — 跟 schema serde default 一致
        _ => "openai_chat",
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_provider_api_format;

    #[test]
    fn normalize_provider_api_format_keeps_protocol_canonicals() {
        for alias in [
            "anthropic_messages",
            "anthropic",
            "claude",
            "messages",
            "claude-messages",
        ] {
            assert_eq!(
                normalize_provider_api_format(Some(alias)),
                "anthropic_messages"
            );
        }
        for alias in ["responses", "openai-responses"] {
            assert_eq!(normalize_provider_api_format(Some(alias)), "responses");
        }
        for alias in ["gemini_cli_oauth", "gemini-cli", "gemini_oauth"] {
            assert_eq!(
                normalize_provider_api_format(Some(alias)),
                "gemini_cli_oauth"
            );
        }
        for alias in [
            "antigravity_oauth",
            "antigravity",
            "google-oauth-antigravity",
        ] {
            assert_eq!(
                normalize_provider_api_format(Some(alias)),
                "antigravity_oauth"
            );
        }
        for alias in ["grok_web", "grok", "grok-com"] {
            assert_eq!(normalize_provider_api_format(Some(alias)), "grok_web");
        }
    }

    #[test]
    fn normalize_provider_api_format_defaults_to_openai_chat() {
        assert_eq!(normalize_provider_api_format(None), "openai_chat");
        assert_eq!(normalize_provider_api_format(Some("")), "openai_chat");
        assert_eq!(
            normalize_provider_api_format(Some("unknown-protocol")),
            "openai_chat"
        );
    }
}

pub(super) fn provider_api_key(provider: &Value) -> String {
    provider
        .get("apiKey")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

pub(super) fn provider_test_model(provider: &Value) -> String {
    let mappings = normalize_model_mappings(provider.get("models"));
    let default = mappings.get("default").map(|s| s.trim()).unwrap_or("");
    if !default.is_empty() {
        return strip_internal_model_suffix(default);
    }
    for slot in MODEL_ORDER
        .iter()
        .copied()
        .filter(|slot| *slot != "default")
    {
        let model = mappings.get(slot).map(|s| s.trim()).unwrap_or("");
        if !model.is_empty() {
            return strip_internal_model_suffix(model);
        }
    }
    "claude-sonnet-4-6".to_owned()
}

pub(super) fn provider_index(cfg: &RawConfig, id: &str) -> Option<usize> {
    cfg.get("providers")
        .and_then(|v| v.as_array())?
        .iter()
        .position(|p| {
            p.as_object()
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str())
                == Some(id)
        })
}

pub(super) fn active_provider(cfg: &RawConfig) -> Option<Value> {
    let active_id = cfg
        .get("activeProvider")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned());
    let providers = cfg.get("providers").and_then(|v| v.as_array())?;
    let chosen = match active_id {
        Some(id) => providers.iter().find(|p| {
            p.as_object()
                .and_then(|o| o.get("id"))
                .and_then(|v| v.as_str())
                == Some(id.as_str())
        }),
        None => providers.first(),
    };
    chosen.cloned()
}

pub(super) fn clean_base_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

pub(super) fn replace_path_suffix(url: &str, suffixes: &[&str], replacement: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return url.to_owned();
    };
    let mut path = parsed.path().trim_end_matches('/').to_owned();
    let lower = path.to_ascii_lowercase();
    for suffix in suffixes {
        if lower.ends_with(suffix) {
            let keep = path.len().saturating_sub(suffix.len());
            path.truncate(keep);
            break;
        }
    }
    let next = format!(
        "{}/{}",
        path.trim_end_matches('/'),
        replacement.trim_start_matches('/')
    );
    parsed.set_path(&next);
    parsed.set_query(None);
    parsed.set_fragment(None);
    parsed.to_string().trim_end_matches('/').to_owned()
}
