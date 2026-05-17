//! 内置 provider 预设(对应 `backend/config.py` BUILTIN_PRESETS).
//!
//! 这里把 Python 中的 8 条预设固化为 JSON 字面量,运行时一次性 parse 成
//! `Vec<Value>`(保留键顺序).采用 JSON 而非 Rust struct 的原因:
//! - 不同预设字段集差异大(notices / baseUrlOptions / requestOptionPresets
//!   / extraHeaders / baseUrlHint 等只在部分项出现)
//! - 直接拷自 Python 源,人工 diff 容易,后续迁移负担最小
//! - 与 Python 版输出做字节级 diff 时,对象 key 顺序与 Python dict 一致
//!
//! **维护契约**:此处任何字段调整都必须 1:1 同步到 backend/config.py;
//! 测试 `presets_json_matches_python_dump` 会校验之.

use once_cell::sync::Lazy;
use serde_json::Value;

const BUILTIN_PRESETS_JSON: &str = include_str!("./presets_data.json");

pub fn builtin_presets() -> &'static [Value] {
    PRESETS.as_slice()
}

static PRESETS: Lazy<Vec<Value>> = Lazy::new(|| {
    let v: Value =
        serde_json::from_str(BUILTIN_PRESETS_JSON).expect("BUILTIN_PRESETS JSON parse failed");
    let arr = v
        .as_array()
        .expect("BUILTIN_PRESETS_JSON must be a JSON array");
    arr.clone()
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_count_matches_python() {
        // 当前 13 条 builtin presets:
        // deepseek / kimi / kimi-code / xiaomi-mimo-payg / xiaomi-mimo-token-plan
        // / zhipu / bailian / bailian-token-plan / minimax / grok-web / google-ai-studio
        // / gemini-cli-oauth / antigravity-oauth
        // (2026-05-10 加 Google AI Studio Gemini preset)
        // (2026-05-11 加 Gemini CLI OAuth login preset)
        // (2026-05-11 加 Antigravity OAuth preset)
        // (2026-05-12 加 Grok Web 反代 preset,见 R1 Plan A)
        assert_eq!(builtin_presets().len(), 13);
    }

    #[test]
    fn minimax_preset_exists_and_uses_official_minimaxi_base_url() {
        let minimax = builtin_presets()
            .iter()
            .find(|p| p["id"] == "minimax")
            .expect("MiniMax preset must exist as builtin entry");
        assert_eq!(minimax["baseUrl"], "https://api.minimaxi.com/v1");
        assert_eq!(minimax["apiFormat"], "openai_chat");
        assert_eq!(minimax["isBuiltin"], true);
        assert!(
            minimax["models"]["default"]
                .as_str()
                .unwrap_or("")
                .starts_with("MiniMax-M2"),
            "default model 必须是 MiniMax-M2.x 系列"
        );
    }

    #[test]
    fn google_ai_studio_preset_uses_native_generate_content_endpoint() {
        let g = builtin_presets()
            .iter()
            .find(|p| p["id"] == "google-ai-studio")
            .expect("Google AI Studio preset must exist as builtin entry");
        // 2026-05-10 起从 OpenAI compat 切到 native generateContent path:
        // ① baseUrl 不再带 /v1beta/openai(adapter 按 model 自动选 v1alpha vs v1beta)
        // ② apiFormat=gemini_native(GeminiNativeAdapter 路由)
        // ③ authScheme=google_api_key(`x-goog-api-key` header,不是 Bearer)
        assert_eq!(
            g["baseUrl"], "https://generativelanguage.googleapis.com",
            "baseUrl 不带版本前缀,adapter 按 Gemini 3+ 用 v1alpha / 2.x 用 v1beta 自动选"
        );
        assert_eq!(
            g["apiFormat"], "gemini_native",
            "Google AI Studio 走 native generateContent endpoint,不走 OpenAI compat"
        );
        assert_eq!(g["authScheme"], "google_api_key");
        assert_eq!(g["isBuiltin"], true);
        let default_model = g["models"]["default"].as_str().unwrap_or("");
        assert!(
            default_model.starts_with("gemini-3"),
            "default model 必须是 Gemini 3.x 系列(2026-05 主流),实际:{default_model}"
        );
    }

    #[test]
    fn every_preset_has_id_name_baseurl() {
        for p in builtin_presets() {
            let obj = p.as_object().expect("preset 必须是对象");
            assert!(obj.contains_key("id"));
            assert!(obj.contains_key("name"));
            assert!(obj.contains_key("baseUrl"));
            assert!(obj.contains_key("apiFormat"));
        }
    }

    #[test]
    fn every_preset_is_builtin_true() {
        for p in builtin_presets() {
            assert_eq!(p["isBuiltin"], serde_json::Value::Bool(true));
        }
    }

    /// 锚定 antigravity preset 关键字段 —— 不能漂移成 alias / 错 baseUrl,
    /// 否则跟 adapter / proxy / handler 的 hard-coded 匹配字符串不一致就 silently
    /// 走错路径(2026-05-11 加,与 review 反映的 alias 漂移问题对应)
    #[test]
    fn antigravity_preset_uses_canonical_authscheme_apiformat() {
        let a = builtin_presets()
            .iter()
            .find(|p| p["id"] == "antigravity-oauth")
            .expect("antigravity-oauth preset 必须存在");
        // 这两个字符串必须是 canonical 形式(加了 alias 也别在 preset 里用)—
        // adapter (gemini_cli/mod.rs is_antigravity_api_format) + proxy
        // (resolver.rs AuthScheme::parse) 都接受多种 alias,但 preset 必须用
        // canonical 形,任何变更要同步加 hard-coded 路径覆盖
        assert_eq!(
            a["authScheme"], "google_oauth_antigravity",
            "preset authScheme 必须用 canonical 形 google_oauth_antigravity"
        );
        assert_eq!(
            a["apiFormat"], "antigravity_oauth",
            "preset apiFormat 必须用 canonical 形 antigravity_oauth"
        );
        assert_eq!(
            a["baseUrl"], "https://daily-cloudcode-pa.googleapis.com",
            "antigravity 用 daily host(CLIProxyAPI antigravityBaseURLFallbackOrder \
             chat 路径主 host = daily),prod 是 429 fallback。跟 gemini-cli 不同 \
             (gemini-cli 用 prod cloudcode-pa)"
        );
        assert_eq!(a["isBuiltin"], true);
    }
}
