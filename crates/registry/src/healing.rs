//! 配置自愈:对**通过 baseUrl 命中 builtin preset** 的 provider,把"非用户
//! 配置"字段强制覆盖为 `builtin_presets()` 的字面值。
//!
//! ## 策略变更(2026-05-08)
//!
//! 早期 healing 只在 extras 缺失/空时补齐,且仅内存修改不写回磁盘 —— 但实测
//! 发现 v1.x 老配置 / 用户手改 / 升级路径漏字段会导致一系列功能性 bug:
//!
//! - **Kimi For Coding Windows 403**:`extraHeaders` 是空 `{}` → 不注入 KimiCLI UA
//!   → Codex CLI 客户端 `codex_cli_rs/...` UA 透传 → Kimi 反爬 403
//! - **MiMo Token Plan 404**:`apiFormat` 缺失/空 → 旧版 fallback `responses` →
//!   apply 走 direct_provider → Codex CLI 直连 MiMo 上游 → MiMo 不支持 `/responses`
//!   → 404,且整个请求**完全跳过我们代理**(零日志、零观测)
//!
//! 用户决定:**这些字段属于"不支持用户配置"的内部协议路由信号**,以后修这类
//! 问题采取**直接覆盖用户旧配置的实际内容**,避免老残留 / 用户手改导致 bug。
//!
//! ## 识别规则(2026-05-08 v2 扩展)
//!
//! **早期版本只用 `isBuiltin=true && id == preset.id` 识别,但实测真机配置
//! 全部 `isBuiltin=false`、id 是随机 hex,完全跳过 healing。** 改成:
//!
//! 1. 把 provider.baseUrl 经 [`normalize_base_url`] 规范化(去 scheme / 末尾
//!    `/` / 末尾 `/v\d+` 版本后缀 / 大小写统一)
//! 2. 把每个 preset 的 `baseUrl` 与 `baseUrlOptions[*].value` 同样规范化
//! 3. 用户 normalized baseUrl 命中**任一** preset 的规范化集合 → 视作该 preset
//!    的 provider,触发 healing
//!
//! ### 命中后做什么 / 不做什么
//!
//! 强制覆盖(`ENFORCED_BUILTIN_FIELDS`,以及 `isBuiltin = true`):
//! - `apiFormat` —— 协议路由信号,决定 ResponsesAdapter / OpenaiChatAdapter 选择
//! - `authScheme` —— `bearer` / `x-api-key` / `none`,鉴权方式由 preset 定
//! - `extraHeaders` —— 反爬 UA 等 client 标识头,由 preset 内置
//! - `isBuiltin` —— 命中即视作内置,UI 后续会防止用户改 baseUrl / authScheme
//!
//! 保留用户配置(**绝不动**):
//! - `id` —— 改 id 会破坏 `activeProvider` 引用
//! - `name` —— 用户可改显示名
//! - `baseUrl` —— 用户可能选了 `baseUrlOptions` 里的备选集群(MiMo 的 sgp/ams
//!   等),原样保留;preset 命中靠 normalize 后比对,不强制把 baseUrl 改回 preset 默认值
//! - `apiKey` —— 用户的 API key,绝不能覆盖
//! - `models` / `modelCapabilities` / `requestOptions` —— 用户可调
//! - `sortIndex` —— 排序,用户可改

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::presets::builtin_presets;

/// 必须强制覆盖为 builtin preset 字面值的字段(忽略用户编辑).
/// 见模块头注 §"覆盖范围"小节.
const ENFORCED_BUILTIN_FIELDS: &[&str] = &["apiFormat", "authScheme", "extraHeaders"];

/// 对 cfg 里所有 **baseUrl 命中 builtin preset** 的 provider,把
/// `ENFORCED_BUILTIN_FIELDS` 列出的字段(以及 `isBuiltin`)强制覆盖为对应
/// builtin preset 的字面值。
///
/// **返回 `true`** 当且仅当有字段被实际修改(用于决定是否写回磁盘)。
///
/// 用户**不该自定义**这几个字段(它们是协议路由 / 反爬适配的内部信号)。
/// 当前/历史的所有"用户手改 / 老版本残留导致的 bug"几乎都源于这几个字段被
/// 改坏或缺失 —— 强制覆盖是最稳的根治方案。
///
/// 用户可定制的字段(`apiKey` / `baseUrl` / `models` / 等)**绝不动**;详见
/// 模块头注 "命中后做什么 / 不做什么" 小节。
pub fn heal_builtin_provider_fields(cfg: &mut Value) -> bool {
    let presets_index = build_preset_index();
    if presets_index.is_empty() {
        return false;
    }

    let Some(providers) = cfg.get_mut("providers").and_then(|v| v.as_array_mut()) else {
        return false;
    };

    let mut changed = false;
    for provider in providers.iter_mut() {
        let Some(obj) = provider.as_object_mut() else {
            continue;
        };
        // 1. 拿 baseUrl,normalize 后查 preset
        let base_url = obj
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let normalized = normalize_base_url(&base_url);
        if normalized.is_empty() {
            continue;
        }
        let Some(candidates) = presets_index.get(&normalized) else {
            continue;
        };
        // **多 preset 共 baseUrl 的歧义解决**:cloudcode-pa.googleapis.com 同
        // 时给 gemini-cli-oauth 和 antigravity-oauth 两个 preset 用。原实现按
        // baseUrl 反查 preset(单 map)会让后插入的 preset 覆盖前一个,user 加
        // antigravity provider 后 healing 把它的 apiFormat 错改成 gemini_cli_oauth
        // (404 / 路由错)。改用 candidate list + user 字段 disambiguate
        // (2026-05-11 实测 user.apiFormat=gemini_cli_oauth 但 user.name=Antigravity
        // 被 healing 错覆盖 gemini-cli 修)
        let user_name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let user_api_format = obj
            .get("apiFormat")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned();
        let Some(preset) = pick_matching_preset(candidates, &user_name, &user_api_format) else {
            continue;
        };

        // 2. isBuiltin → true(若不是)
        let is_builtin_now = obj
            .get("isBuiltin")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !is_builtin_now {
            obj.insert("isBuiltin".into(), Value::Bool(true));
            changed = true;
        }

        // 3. ENFORCED_BUILTIN_FIELDS 强制覆盖为 preset 字面值。
        //    特殊处理:preset 字面值是 `null`(serde_json::Value::Null)视作"未
        //    指定"—— 不覆盖用户字段。多数 preset 把空 extraHeaders 写成 null
        //    而用户配置写成 `{}`,行为等价但语义不同;不应该把用户的 `{}` 改成
        //    `null`,反之亦然。
        for field in ENFORCED_BUILTIN_FIELDS {
            let preset_value = preset.get(*field).cloned();
            let current_value = obj.get(*field).cloned();
            let preset_specifies = !matches!(preset_value, None | Some(Value::Null));
            if !preset_specifies {
                continue;
            }
            let preset_value = preset_value.unwrap();

            // **grok_web 半残防御**(2026-05-12,user E2E 反馈):healing 把 apiFormat
            // 改成 grok_web 时,如果 provider 上**没有合法的 grokWeb.cookies.sso**,
            // 改完后是个"半残" provider —— forward 走 GrokCookie scheme 但找不到
            // cookies,chat 401 失败。这种情况下**不要**强改 apiFormat(否则用户
            // 在 UI 上看不出协议错配,只看到神秘 chat 失败),保留 user_value
            // (如 "openai_chat")+ ERROR 级 telemetry 让用户在日志面板看清问题。
            //
            // 修复路径:用户在 UI 上看 chat 报"apiFormat=openai_chat 但 baseUrl 是
            // grok.com"对应错误更直观,引导用户**删了 provider 重建走 Grok(Web)
            // preset 卡片**(那条路径才会带上 grokWeb form 收集 cookies)。
            if *field == "apiFormat"
                && preset_value.as_str() == Some("grok_web")
                && !has_valid_grok_web_credentials(obj)
            {
                tracing::error!(
                    error_id = "GROK_WEB_INVARIANT_VIOLATION",
                    provider_id = %obj.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    base_url = %obj.get("baseUrl").and_then(|v| v.as_str()).unwrap_or(""),
                    user_value = %current_value.as_ref().map(ToString::to_string).unwrap_or_default(),
                    "healing: provider baseUrl 命中 grok-web preset 但缺 grokWeb.cookies.sso;不强改 apiFormat 避免半残状态。修复:UI 上删了 provider,从 Grok(Web) preset 卡片重建并填 sso JWT"
                );
                continue; // 跳过这条 apiFormat 字段的 enforce(其它字段照样走)
            }

            match current_value {
                Some(c) if c == preset_value => {}
                Some(c) => {
                    // C1 (2026-05-10):apiFormat 被强制覆盖时 telemetry warn,让用户在
                    // 日志面板看清"我的 direct 透传为啥失效了"—— baseUrl 命中 builtin
                    // preset 即触发,direct 透传需要 baseUrl 不命中任何 builtin。
                    if *field == "apiFormat" {
                        tracing::warn!(
                            provider_id = %obj.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                            base_url = %obj.get("baseUrl").and_then(|v| v.as_str()).unwrap_or(""),
                            user_value = %c,
                            preset_value = %preset_value,
                            "healing: apiFormat 被强制覆盖回 preset 字面值(baseUrl 命中 builtin preset);direct 透传需要 baseUrl 不命中任何 builtin"
                        );
                    }
                    obj.insert((*field).to_owned(), preset_value);
                    changed = true;
                }
                None => {
                    obj.insert((*field).to_owned(), preset_value);
                    changed = true;
                }
            }
        }
    }
    changed
}

/// 迁移历史 fork 残留的 `settings.updateUrl` 到当前规范仓库（Cmochance）。
///
/// 背景：本项目由 lonr-6/cc-desktop-switch 及后续 fork 演化而来，早期默认
/// updateUrl 指向 `lonr-6/codex-app-transfer`。用户老 config.json 里残留该值时，
/// “设置页检查更新”就会去另一个项目的 releases 拉 latest.json（用户反馈的根源）。
///
/// 策略：
/// - 只认已知 legacy owner（"lonr-6/codex-app-transfer" 等）才迁移，避免误伤
///   用户故意指向自己 fork 的自定义 updateUrl。
/// - 有变更就写回磁盘（与 provider healing 一致的 aggressive 策略）。
/// - 未来若出现新 legacy，可在此列表追加。
pub fn heal_legacy_update_url(cfg: &mut Value) -> bool {
    const LEGACY_OWNERS: &[&str] = &["lonr-6/codex-app-transfer"];

    let Some(settings) = cfg.get_mut("settings").and_then(|v| v.as_object_mut()) else {
        return false;
    };

    let Some(update_url) = settings.get_mut("updateUrl").and_then(|v| v.as_str()) else {
        return false;
    };

    let is_legacy = LEGACY_OWNERS.iter().any(|owner| update_url.contains(owner));

    if !is_legacy {
        return false;
    }

    // 统一写回当前库常量（Cmochance）。注意：这里直接用字符串字面，
    // 因为 healing 运行在 registry 上下文，build-time env 只对 Tauri binary 有效。
    // 这样保证 healing 永远把老用户“拉回”官方源，实现“统一为 Cmochance”。
    let canonical = crate::DEFAULT_UPDATE_URL;
    if update_url == canonical {
        return false;
    }

    settings.insert("updateUrl".to_owned(), Value::String(canonical.to_owned()));
    true
}

/// provider 是否有合法的 `grokWeb.cookies.sso` JWT(非空 string)。
///
/// 用于 [`heal_builtin_provider_fields`] 检测 grok-web preset 半残不变量:
/// **provider.apiFormat=grok_web ⇒ provider.grokWeb.cookies.sso 必须是非空 string**。
fn has_valid_grok_web_credentials(provider_obj: &serde_json::Map<String, Value>) -> bool {
    provider_obj
        .get("grokWeb")
        .and_then(|v| v.as_object())
        .and_then(|gw| gw.get("cookies"))
        .and_then(|v| v.as_object())
        .and_then(|c| c.get("sso"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// 把 baseUrl 规范化为可比对的形式 —— 用于 healing 通过 baseUrl 反查 preset.
///
/// 处理:
/// - trim
/// - 大小写归一(全小写)
/// - 去 `http://` / `https://` 前缀(scheme 不参与匹配,即 http→https 升级
///   或反之均视作同一上游)
/// - 去 query / fragment(`?...` / `#...`)
/// - 去末尾 `/`
/// - 去末尾 `/v\d+`(API 版本号,例如 `/v1` / `/v2` / `/v10`)—— 解决 preset
///   写 `https://api.deepseek.com` 但用户配 `https://api.deepseek.com/v1` 的
///   差异;**只 strip 一次**,避免 `https://api.kimi.com/coding/v1` 被错误地
///   strip 成 `api.kimi.com`(应为 `api.kimi.com/coding`)
/// - 末尾 `/v\d+` strip 后再 strip 一次末尾 `/`(以防 `/v1/` 形式)
///
/// 不做的事(故意保守):
/// - 不识别 host 别名(`api.example.com` ≠ `api2.example.com`)
/// - 不展开 path 中的 `/v\d+/`(只 strip 末尾)
/// - 不识别端口(用户加端口会导致不命中,这是合理隔离)
pub fn normalize_base_url(s: &str) -> String {
    let mut t = s.trim().to_ascii_lowercase();
    // strip query / fragment
    if let Some(idx) = t.find(['?', '#']) {
        t.truncate(idx);
    }
    // strip scheme
    for prefix in ["https://", "http://"] {
        if let Some(stripped) = t.strip_prefix(prefix) {
            t = stripped.to_owned();
            break;
        }
    }
    // strip trailing `/`
    while t.ends_with('/') {
        t.pop();
    }
    // strip 末尾 `/v\d+`(只一次)
    if let Some(stripped) = strip_trailing_version_segment(&t) {
        t = stripped;
        // 再去一次末尾 `/`(防 `/v1/` strip 后留空 path 末 `/`)
        while t.ends_with('/') {
            t.pop();
        }
    }
    t
}

/// 若 `s` 末尾是 `/v\d+`(版本号 segment),返回去掉该 segment 的字符串;
/// 否则返回 None。要求版本号至少 1 个数字。
fn strip_trailing_version_segment(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    // 末尾连续数字
    let mut digits = 0usize;
    while end > 0 && bytes[end - 1].is_ascii_digit() {
        end -= 1;
        digits += 1;
    }
    if digits == 0 {
        return None;
    }
    // 数字前应是 `v`
    if end == 0 || bytes[end - 1] != b'v' {
        return None;
    }
    end -= 1;
    // `v` 前应是 `/`
    if end == 0 || bytes[end - 1] != b'/' {
        return None;
    }
    end -= 1;
    Some(s[..end].to_owned())
}

/// **向后兼容别名** —— 历史 API 名,保留供老调用方;新代码直接用
/// `heal_builtin_provider_fields`。
#[deprecated(
    since = "2.0.11",
    note = "use heal_builtin_provider_fields which covers more fields and reports changes"
)]
pub fn heal_builtin_extra_headers(cfg: &mut Value) {
    heal_builtin_provider_fields(cfg);
}

/// 索引:normalized baseUrl → preset Map<String,Value>。
///
/// 一个 preset 通常贡献 1 条(baseUrl) + N 条(baseUrlOptions[*].value),
/// 其中任一被用户使用都视作命中该 preset。如果两个 preset 在 normalize 后
/// 撞到同一 key(理论上不该出现,因为不同上游的 host 不同),后写入的覆盖前者
/// —— 这种冲突应在 preset 数据评审时拦截,运行时不做特殊处理。
fn build_preset_index() -> HashMap<String, Vec<Map<String, Value>>> {
    let mut idx: HashMap<String, Vec<Map<String, Value>>> = HashMap::new();
    for preset in builtin_presets().iter() {
        let Some(obj) = preset.as_object() else {
            continue;
        };
        // 1. preset.baseUrl
        let mut urls: Vec<String> = Vec::new();
        if let Some(u) = obj.get("baseUrl").and_then(|v| v.as_str()) {
            urls.push(u.to_owned());
        }
        // 2. preset.baseUrlOptions[*].value
        if let Some(opts) = obj.get("baseUrlOptions").and_then(|v| v.as_array()) {
            for opt in opts {
                if let Some(v) = opt.get("value").and_then(|v| v.as_str()) {
                    urls.push(v.to_owned());
                }
            }
        }
        for url in urls {
            let norm = normalize_base_url(&url);
            if norm.is_empty() {
                continue;
            }
            idx.entry(norm).or_default().push(obj.clone());
        }
    }
    idx
}

/// 多 preset 共用同一 baseUrl 时(eg gemini-cli-oauth + antigravity-oauth 都
/// 走 cloudcode-pa 系列)按用户 provider 字段挑最匹配的那条。
/// 优先级:**name 完全匹配** > **apiFormat 一致** > 单 preset(无 ambiguity)。
/// 三个都不命中 → 返 None(不强制 healing,避免把 antigravity 错改成 gemini-cli)
fn pick_matching_preset<'a>(
    candidates: &'a [Map<String, Value>],
    user_name: &str,
    user_api_format: &str,
) -> Option<&'a Map<String, Value>> {
    if candidates.len() == 1 {
        return candidates.first();
    }
    // 1. name 完全匹配(case-insensitive trim)— 最强信号,user 自定义 name
    //    一般会跟 preset.name 一致(或者改了但保留前缀)
    let un = user_name.trim().to_lowercase();
    if !un.is_empty() {
        if let Some(p) = candidates.iter().find(|p| {
            p.get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_lowercase() == un)
                .unwrap_or(false)
        }) {
            return Some(p);
        }
    }
    // 2. apiFormat 匹配 — user provider 用 antigravity_oauth 想要 antigravity preset
    let uaf = user_api_format.trim().to_lowercase();
    if !uaf.is_empty() {
        if let Some(p) = candidates.iter().find(|p| {
            p.get("apiFormat")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_lowercase() == uaf)
                .unwrap_or(false)
        }) {
            return Some(p);
        }
    }
    // 3. 多个候选但 name + apiFormat 都没法 disambiguate → 不强制 healing
    //    (返 None 让 user 字段保留,避免错覆盖。preset evolution 时这是更稳的选择)
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── normalize_base_url ───────────────────────────────────────────

    #[test]
    fn normalize_base_url_strips_scheme_and_trailing_slash() {
        assert_eq!(
            normalize_base_url("https://api.deepseek.com/"),
            "api.deepseek.com"
        );
        assert_eq!(
            normalize_base_url("http://api.deepseek.com"),
            "api.deepseek.com"
        );
    }

    #[test]
    fn normalize_base_url_strips_trailing_v1_v2_v10() {
        // /v1
        assert_eq!(
            normalize_base_url("https://api.moonshot.cn/v1"),
            "api.moonshot.cn"
        );
        // /v1/
        assert_eq!(
            normalize_base_url("https://api.moonshot.cn/v1/"),
            "api.moonshot.cn"
        );
        // /v2
        assert_eq!(
            normalize_base_url("https://api.deepseek.com/v2"),
            "api.deepseek.com"
        );
        // /v10(多位数字)
        assert_eq!(
            normalize_base_url("https://api.example.com/v10"),
            "api.example.com"
        );
    }

    #[test]
    fn normalize_base_url_keeps_path_before_version() {
        // Kimi Code: 用户配 `https://api.kimi.com/coding/v1`,preset 也是
        // `https://api.kimi.com/coding/v1` —— normalize 后都该是 `api.kimi.com/coding`
        assert_eq!(
            normalize_base_url("https://api.kimi.com/coding/v1"),
            "api.kimi.com/coding"
        );
        assert_eq!(
            normalize_base_url("https://api.kimi.com/coding/v1/"),
            "api.kimi.com/coding"
        );
    }

    #[test]
    fn normalize_base_url_only_strips_one_version_segment() {
        // 防御:`/v1/v2`(理论不会出现)只 strip 末尾一次,留 `/v1`
        assert_eq!(
            normalize_base_url("https://api.example.com/v1/v2"),
            "api.example.com/v1"
        );
    }

    #[test]
    fn normalize_base_url_lowercases_and_strips_query() {
        assert_eq!(
            normalize_base_url("HTTPS://API.DeepSeek.COM/v1?x=1#anchor"),
            "api.deepseek.com"
        );
    }

    #[test]
    fn normalize_base_url_does_not_strip_non_version_suffix() {
        // `/chat` 不该被 strip(不是 v\d+ 形态)
        assert_eq!(
            normalize_base_url("https://api.example.com/chat"),
            "api.example.com/chat"
        );
        // `/version1`(不是 `/v1` 边界)不 strip
        assert_eq!(
            normalize_base_url("https://api.example.com/version1"),
            "api.example.com/version1"
        );
    }

    #[test]
    fn normalize_base_url_handles_empty_and_whitespace() {
        assert_eq!(normalize_base_url(""), "");
        assert_eq!(normalize_base_url("   "), "");
    }

    // ── heal_builtin_provider_fields ─────────────────────────────────

    #[test]
    fn fills_empty_extras_for_builtin_kimi_code() {
        let mut cfg = json!({
            "providers": [
                {
                    "id": "kimi-code",
                    "name": "Kimi Code",
                    "baseUrl": "https://api.kimi.com/coding/v1",
                    "isBuiltin": true,
                    "extraHeaders": {}
                }
            ]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(changed, "应当报告有改动");
        let extras = &cfg["providers"][0]["extraHeaders"];
        assert_eq!(
            extras["User-Agent"], "KimiCLI/1.40.0",
            "Kimi Code 的 KimiCLI UA 应被强制写入"
        );
    }

    #[test]
    fn heals_user_built_provider_when_baseurl_matches_preset() {
        // 关键回归(2026-05-08):真机配置里所有 builtin-类 provider 都
        // `isBuiltin=false`、id 是随机 hex —— 老识别规则 (id == preset.id)
        // 完全跳过 healing。新规则:baseUrl 命中 preset → 强制 healing 并把
        // isBuiltin 设为 true。
        let mut cfg = json!({
            "providers": [
                {
                    "id": "b405e7b0",                                  // 随机 hex,不在 preset.id 列表
                    "name": "Kimi Code",
                    "baseUrl": "https://api.kimi.com/coding/v1",       // 命中 kimi-code preset.baseUrl
                    "isBuiltin": false,                                // 老配置遗留
                    "apiFormat": "openai_chat",
                    "extraHeaders": {}                                 // 反爬 UA 缺失 → Windows 403 root cause
                }
            ]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(changed);
        let p = &cfg["providers"][0];
        assert_eq!(
            p["isBuiltin"],
            json!(true),
            "命中 preset → isBuiltin 应被设为 true"
        );
        assert_eq!(p["id"], "b405e7b0", "id 不动(避免破坏 activeProvider 引用)");
        assert_eq!(p["extraHeaders"]["User-Agent"], "KimiCLI/1.40.0");
    }

    #[test]
    fn heals_via_baseurl_options_alternate_cluster() {
        // MiMo Token Plan:preset.baseUrl 是 cn 集群,但 baseUrlOptions 包含 sgp/ams
        // 用户选了 sgp 集群 → normalize 后命中 baseUrlOptions → 触发 healing,
        // 但 baseUrl 本身**不被改回 cn**(用户的集群选择保留)。
        let mut cfg = json!({
            "providers": [
                {
                    "id": "b863a67c",
                    "name": "Xiaomi MiMo (Token Plan)",
                    "baseUrl": "https://token-plan-sgp.xiaomimimo.com/v1",
                    "isBuiltin": false,
                    "apiFormat": "responses"   // 错误值,需被覆盖回 preset 的 openai_chat
                }
            ]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(changed);
        let p = &cfg["providers"][0];
        assert_eq!(p["apiFormat"], "openai_chat");
        assert_eq!(p["isBuiltin"], json!(true));
        assert_eq!(
            p["baseUrl"], "https://token-plan-sgp.xiaomimimo.com/v1",
            "baseUrl 应保留用户的 sgp 集群选择,不被改回 preset 默认 cn"
        );
    }

    #[test]
    fn forces_apiformat_override_even_if_user_edited_to_bogus_value() {
        // 关键回归(2026-05-08 MiMo 404):用户手改把 apiFormat 改成 "responses"
        // → apply 跳过代理直连上游 → 404
        // 新策略:命中 preset 即强制覆盖,不管用户改成了什么。
        let mut cfg = json!({
            "providers": [
                {
                    "id": "xiaomi-mimo-token-plan",
                    "name": "Xiaomi MiMo (Token Plan)",
                    "baseUrl": "https://token-plan-cn.xiaomimimo.com/v1",
                    "isBuiltin": true,
                    "apiFormat": "responses",
                    "extraHeaders": {}
                }
            ]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(changed);
        assert_eq!(
            cfg["providers"][0]["apiFormat"], "openai_chat",
            "MiMo apiFormat 必须被强制覆盖回 preset 的 openai_chat"
        );
    }

    #[test]
    fn forces_authscheme_override() {
        let mut cfg = json!({
            "providers": [
                {
                    "id": "kimi-code",
                    "baseUrl": "https://api.kimi.com/coding/v1",
                    "isBuiltin": true,
                    "authScheme": "none"
                }
            ]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(changed);
        assert_eq!(cfg["providers"][0]["authScheme"], "bearer");
    }

    #[test]
    fn does_not_touch_truly_user_built_provider() {
        // 真正的用户自建 provider(baseUrl 不命中任何 preset)绝不动
        let mut cfg = json!({
            "providers": [
                {
                    "id": "mock-provider",
                    "name": "My Reverse Proxy",
                    "baseUrl": "http://127.0.0.1:29090",   // 不在任何 preset 的 baseUrl 集合里
                    "isBuiltin": false,
                    "apiFormat": "responses",
                    "extraHeaders": {}
                }
            ]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(!changed, "baseUrl 未命中 preset → 视作用户自建,不动");
        assert_eq!(cfg["providers"][0]["apiFormat"], "responses");
        assert_eq!(cfg["providers"][0]["isBuiltin"], json!(false));
    }

    #[test]
    fn does_not_touch_user_apikey_or_baseurl_or_models() {
        // 用户可定制字段绝不能被 healing 覆盖
        let mut cfg = json!({
            "providers": [
                {
                    "id": "xiaomi-mimo-token-plan",
                    "name": "My MiMo",
                    "baseUrl": "https://token-plan-sgp.xiaomimimo.com/v1",  // 命中 sgp 集群
                    "isBuiltin": true,
                    "apiKey": "sk-user-custom-key",
                    "models": {"default": "user-overridden-model"},
                    "modelCapabilities": {"foo": {"context_window": 12345}},
                    "apiFormat": "openai_chat",
                    "extraHeaders": {"User-Agent": "Test/1.0"}  // 强制覆盖时会被替换
                }
            ]
        });
        let _ = heal_builtin_provider_fields(&mut cfg);
        let p = &cfg["providers"][0];
        assert_eq!(p["apiKey"], "sk-user-custom-key", "apiKey 绝不动");
        assert_eq!(p["name"], "My MiMo", "name 绝不动(用户自定义显示名)");
        assert_eq!(
            p["baseUrl"], "https://token-plan-sgp.xiaomimimo.com/v1",
            "baseUrl 绝不动(支持 baseUrlOptions 选择)"
        );
        assert_eq!(
            p["models"]["default"], "user-overridden-model",
            "models 绝不动"
        );
        assert_eq!(p["modelCapabilities"]["foo"]["context_window"], 12345);
    }

    #[test]
    fn no_op_when_already_aligned_with_preset() {
        let mut cfg = json!({
            "providers": [
                {
                    "id": "kimi-code",
                    "baseUrl": "https://api.kimi.com/coding/v1",
                    "isBuiltin": true,
                    "apiFormat": "openai_chat",
                    "authScheme": "bearer",
                    "extraHeaders": {"User-Agent": "KimiCLI/1.40.0"}
                }
            ]
        });
        assert!(
            !heal_builtin_provider_fields(&mut cfg),
            "字段已与 preset 一致 → 不改"
        );
    }

    #[test]
    fn no_op_when_baseurl_does_not_match_any_preset() {
        let mut cfg = json!({
            "providers": [
                {
                    "id": "totally-unknown-id",
                    "name": "Random",
                    "baseUrl": "https://my-private-llm.example.org/api",
                    "isBuiltin": true,                  // 即使 isBuiltin=true,baseUrl 不命中 → 不视作 builtin
                    "apiFormat": "responses"
                }
            ]
        });
        assert!(!heal_builtin_provider_fields(&mut cfg));
        assert_eq!(
            cfg["providers"][0]["apiFormat"], "responses",
            "baseUrl 未命中 preset → 字段不动(用户用了自有反代)"
        );
    }

    #[test]
    fn handles_missing_baseurl_gracefully() {
        // 配置损坏:provider 没 baseUrl → 跳过,不报错
        let mut cfg = json!({
            "providers": [
                {"id": "broken", "isBuiltin": true, "apiFormat": "responses"}
            ]
        });
        assert!(!heal_builtin_provider_fields(&mut cfg));
    }

    #[test]
    fn handles_missing_providers_array_gracefully() {
        let mut cfg = json!({"version": "1.0.4"});
        assert!(!heal_builtin_provider_fields(&mut cfg));
    }

    #[test]
    fn heals_multiple_providers_in_one_pass() {
        // 模拟用户真机:5 个 builtin-类 + 1 个真自建,id 都是随机 hex,
        // isBuiltin 全 false,只看 baseUrl 命中。
        let mut cfg = json!({
            "providers": [
                {"id": "h1", "name": "Kimi Code", "baseUrl": "https://api.kimi.com/coding/v1", "isBuiltin": false, "apiFormat": "responses", "extraHeaders": {}},
                {"id": "h2", "name": "DeepSeek",  "baseUrl": "https://api.deepseek.com/v1",    "isBuiltin": false},
                {"id": "h3", "name": "Mock",      "baseUrl": "http://127.0.0.1:29090",         "isBuiltin": false, "apiFormat": "responses"}
            ]
        });
        assert!(heal_builtin_provider_fields(&mut cfg));
        // h1 Kimi Code 命中 preset → apiFormat 强制覆盖回 openai_chat
        assert_eq!(cfg["providers"][0]["apiFormat"], "openai_chat");
        assert_eq!(cfg["providers"][0]["isBuiltin"], json!(true));
        assert_eq!(
            cfg["providers"][0]["extraHeaders"]["User-Agent"],
            "KimiCLI/1.40.0"
        );
        // h2 DeepSeek 命中 preset(/v1 后缀已 normalize)→ isBuiltin 设为 true,
        // apiFormat 写入 preset 字面值 openai_chat
        assert_eq!(cfg["providers"][1]["apiFormat"], "openai_chat");
        assert_eq!(cfg["providers"][1]["isBuiltin"], json!(true));
        // h3 Mock(localhost)未命中任何 preset → 不动
        assert_eq!(
            cfg["providers"][2]["apiFormat"], "responses",
            "真用户自建(baseUrl 不命中)绝不动"
        );
        assert_eq!(cfg["providers"][2]["isBuiltin"], json!(false));
    }

    // ── grok_web 半残不变量 ────────────────────────────────────────────

    #[test]
    fn grok_web_missing_credentials_skips_apiformat_enforce() {
        // user E2E 反馈(2026-05-12):baseUrl=https://grok.com + apiFormat=openai_chat
        // + 缺 grokWeb cookies。原行为:healing 强改 apiFormat=grok_web → provider
        // 进入"apiFormat=grok_web 但缺 cookies"半残态 → forward 走 GrokCookie
        // scheme 找不到 cookies → chat 神秘 401。
        // 修复后:apiFormat 保留 user_value(openai_chat),isBuiltin 仍 enforce true,
        // ERROR 级 telemetry 让用户在日志面板看清问题。
        let mut cfg = json!({
            "providers": [
                {"id": "broken-grok", "name": "Grok(Web)", "baseUrl": "https://grok.com",
                 "isBuiltin": false, "apiFormat": "openai_chat"}
            ]
        });
        assert!(heal_builtin_provider_fields(&mut cfg));
        // isBuiltin 仍 enforce 改 true(它跟 grok_web 半残不冲突)
        assert_eq!(cfg["providers"][0]["isBuiltin"], json!(true));
        // apiFormat **保留** user_value(skip 防半残)
        assert_eq!(
            cfg["providers"][0]["apiFormat"], "openai_chat",
            "缺 grokWeb 时不该强改 apiFormat 让 provider 进半残态"
        );
    }

    #[test]
    fn grok_web_with_valid_credentials_still_enforces_apiformat() {
        // 正常 case:user 走 Grok(Web) preset 卡片 → form 填了 sso JWT → grokWeb
        // 合法 → healing 正常 enforce apiFormat=grok_web。
        let mut cfg = json!({
            "providers": [
                {"id": "good-grok", "name": "Grok(Web)", "baseUrl": "https://grok.com",
                 "isBuiltin": false, "apiFormat": "openai_chat",
                 "grokWeb": {"cookies": {"sso": "real-jwt-token"}}}
            ]
        });
        assert!(heal_builtin_provider_fields(&mut cfg));
        assert_eq!(cfg["providers"][0]["isBuiltin"], json!(true));
        assert_eq!(
            cfg["providers"][0]["apiFormat"], "grok_web",
            "有合法 sso 时正常 enforce"
        );
    }

    #[test]
    fn grok_web_empty_sso_string_treated_as_missing() {
        // 防御:user 在 update_provider 把 sso 设成 "" → 跟没设一样,半残
        let mut cfg = json!({
            "providers": [
                {"id": "empty-sso-grok", "name": "Grok(Web)", "baseUrl": "https://grok.com",
                 "isBuiltin": false, "apiFormat": "openai_chat",
                 "grokWeb": {"cookies": {"sso": ""}}}
            ]
        });
        assert!(heal_builtin_provider_fields(&mut cfg));
        assert_eq!(
            cfg["providers"][0]["apiFormat"], "openai_chat",
            "sso 空字符串等同于缺失,不强改 apiFormat"
        );
    }
}
