//! 7 个内置 Provider 预设 —— 1:1 转写自
//! `lonr-6/cc-desktop-switch backend/config.py:34-185`(Apache-2 / MIT)。
//!
//! 用 JSON 字面量内嵌(`include_str!`)避免在 Rust 端手写 7 个大 IndexMap
//! initializer —— Python 原文本身就是 JSON-like dict,直接以字符串持有 +
//! 运行时 deserialize 最贴近"1:1 转写"原则,并且 review 时可以直接 diff
//! 跟上游内容一致。
//!
//! 任何对 BUILTIN_PRESETS 的修订**必须**先在 cc-desktop-switch 上游确认,
//! 再同步本文件 + README 致谢段(memory rule "上游借鉴必须 file:line 引用")。

use crate::schema::Provider;
use crate::ClaudeDesktopError;

const BUILTIN_PRESETS_JSON: &str = include_str!("presets_data.json");

/// 加载 7 个内置 preset(运行时 deserialize 一次,后续可复制 Vec)。
pub fn builtin_presets() -> Result<Vec<Provider>, ClaudeDesktopError> {
    let parsed: Vec<Provider> = serde_json::from_str(BUILTIN_PRESETS_JSON)
        .map_err(|e| ClaudeDesktopError::SchemaCorrupt(format!(
            "builtin preset JSON 解析失败(crates/claude_desktop/src/presets_data.json): {e}"
        )))?;
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_presets_load_seven_anthropic_providers() {
        let presets = builtin_presets().expect("内置 preset 必须能加载");
        // 跟 backend/config.py:34-185 完全对齐 7 个 preset
        // 7 上游 1:1 + 1 个 anyrouter (cas 自加,无 upstream)
        assert_eq!(presets.len(), 8);

        let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "deepseek",
                "kimi",
                "kimi-code",
                "xiaomi-mimo-payg",
                "xiaomi-mimo-token-plan",
                "zhipu",
                "anyrouter",
                "bailian",
            ],
            "preset id 顺序必须跟 backend/config.py:34-185 一致(anyrouter 是 cas 自加)"
        );

        for p in &presets {
            assert_eq!(p.api_format, "anthropic", "全部 7 个 preset api_format 必须是 anthropic");
            assert!(!p.base_url.is_empty(), "{} baseUrl 不能为空", p.id);
            assert!(p.is_builtin, "{} 必须标 isBuiltin=true", p.id);
        }
    }

    #[test]
    fn deepseek_preset_matches_upstream_line_35_77() {
        let presets = builtin_presets().unwrap();
        let deepseek = presets.iter().find(|p| p.id == "deepseek").unwrap();
        assert_eq!(deepseek.name, "DeepSeek");
        assert_eq!(deepseek.base_url, "https://api.deepseek.com/anthropic");
        assert_eq!(deepseek.auth_scheme, "bearer");
        assert_eq!(deepseek.models.get("sonnet").map(String::as_str), Some("deepseek-v4-pro"));
        assert_eq!(deepseek.models.get("haiku").map(String::as_str), Some("deepseek-v4-flash"));
        assert!(deepseek.model_options.contains_key("deepseek_1m"));
        assert!(deepseek.request_option_presets.contains_key("deepseek_max_effort"));
        assert_eq!(
            deepseek.extra_headers.get("x-api-key").map(String::as_str),
            Some("{apiKey}")
        );
    }

    #[test]
    fn zhipu_uses_x_api_key_auth_scheme() {
        // backend/config.py:146-158 智谱用 authScheme = x-api-key 不是 bearer
        let presets = builtin_presets().unwrap();
        let zhipu = presets.iter().find(|p| p.id == "zhipu").unwrap();
        assert_eq!(zhipu.auth_scheme, "x-api-key");
        assert_eq!(zhipu.base_url, "https://open.bigmodel.cn/api/anthropic");
    }

    #[test]
    fn bailian_uses_x_api_key_and_qwen_1m_option() {
        // backend/config.py:160-184 阿里云百炼 authScheme = x-api-key + qwen_1m modelOption
        let presets = builtin_presets().unwrap();
        let bailian = presets.iter().find(|p| p.id == "bailian").unwrap();
        assert_eq!(bailian.auth_scheme, "x-api-key");
        assert_eq!(bailian.base_url, "https://dashscope.aliyuncs.com/apps/anthropic");
        assert!(bailian.model_options.contains_key("qwen_1m"));
    }

    #[test]
    fn xiaomi_mimo_token_plan_has_base_url_options() {
        // backend/config.py:121-143 Xiaomi MiMo Token Plan 有 2 个 baseUrl 选项
        let presets = builtin_presets().unwrap();
        let mimo = presets.iter().find(|p| p.id == "xiaomi-mimo-token-plan").unwrap();
        assert_eq!(mimo.base_url_options.len(), 2);
        assert!(!mimo.base_url_hint.is_empty());
    }
}
