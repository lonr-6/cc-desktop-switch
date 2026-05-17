//! grok.com Web 鉴权头注入。
//!
//! ## 现行协议
//!
//! 实测 2026-05-11 SuperGrok 账号(三次 cURL 抓包),grok.com 现行鉴权头:
//!
//! - **Cookie**:`sso=<JWT>; sso-rw=<JWT>; cf_clearance=<TOKEN>` (核心)
//!   - 可选:`x-userid=<UUID>`(已登录用户 UUID)、`__cf_bm=<TOKEN>`(Cloudflare Bot Management)
//!   - 可选:`mp_..._mixpanel` / `__stripe_*` / `OptanonConsent` / `i18nextLng`(分析/支付/合规,可省)
//! - **x-statsig-id**:Base64-encoded Statsig feature flag context(每次请求不同)
//! - **x-xai-request-id**:UUID v4(client 生成,每次请求不同)
//! - **traceparent**:W3C trace context(`00-<32hex>-<16hex>-00`)
//! - **sentry-trace** / **baggage**:Sentry distributed tracing
//! - **User-Agent**:必须是真实浏览器 UA(防风控)
//!
//! ## UX 简化策略(Plan A,参考 chenyme/grok2api dynamic_statsig)
//!
//! grok.com 服务端**不严格校验** `x-statsig-id` 内容,只校验 base64 解码后字符
//! 像是 Statsig SDK 上报的 JS error message。chenyme 实证只要伪造一段
//! `e:TypeError: Cannot read properties of null (reading 'children['<rand>']')`
//! base64 就过。本模块复刻这个动态生成思路,**用户不需要从浏览器抠 statsigId**。
//!
//! 同样,`sso-rw` cookie 实测里跟 `sso` 是同一个 JWT(chenyme 直接 `sso={t}; sso-rw={t}`),
//! `cf_clearance` 在很多网络环境下并非强制(走代理 / 已通过 CF 时 grok.com 直接放行),
//! User-Agent / Origin / Referer / Sec-Ch-* 后端硬编码默认值即可。
//!
//! → **UI 只需必填 1 个字段:`sso` JWT。其他全部 optional 或动态生成。**
//!
//! ## chenyme 用的旧 headers 已过时(transport/http.py)
//!
//! `x-anonuserid` / `x-challenge` / `x-signature` 三个 header **现行协议不再使用**。
//! 不要复用 chenyme 旧版的 anonymous-user 鉴权头组合。
//!
//! ## 本模块职责
//!
//! - 提供 [`apply_grok_headers`]:在 `RequestPlan` headers 上注入 cookie + statsig 等
//! - 提供 [`GrokCookies`]:用户提供的 cookie 集合(从 Provider.extra.grokWeb.cookies 读)
//! - 提供 [`generate_statsig_id`]:动态伪造 statsig ID(参考 chenyme `_statsig_id`)
//!
//! 实际 header 注入在 [`crate::mapper::grok_web::GrokWebMapper`](../mapper/grok_web.rs)
//! 准备 `RequestPlan` 时调用本模块函数。

use codex_app_transfer_registry::Provider;
use http::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

/// 用户提供的 grok.com cookie 集合。
///
/// 从 `Provider.extra["grokWeb"]["cookies"]` JSON object 中读取。
///
/// **必需字段(Plan A)**:`sso` JWT —— 这是 grok.com 真正验账号的唯一字段。
///
/// **可选字段**:
/// - `sso-rw`:缺失时复用 `sso`(实测同 JWT,chenyme `sso={t}; sso-rw={t}` 已验证)
/// - `cf_clearance`:Cloudflare 通过 token,缺失时不带这个 cookie segment
///   (走代理 / 网络已通过 CF 时 grok.com 直接放行,~7 天过期)
/// - `x-userid`:用户 UUID,grok.com 用于路由优化
/// - `__cf_bm`:Cloudflare bot management
/// - 其他 grok.com 设置的 cookie(mixpanel/stripe/optanon/i18next 等)透传
///
/// **不实现 `Default`**(review-feedback TD4):empty `GrokCookies` 没有意义,
/// `to_cookie_header()` 会拼出 `sso=` 让上游 401。唯一合法构造路径是
/// [`GrokCookies::from_provider`]。
#[derive(Debug, Clone)]
pub struct GrokCookies {
    /// JWT session token(写入 `Cookie: sso=...`),**唯一必需字段**
    pub sso: String,
    /// JWT session token(读写,写入 `Cookie: sso-rw=...`)。
    /// 缺失时 [`to_cookie_header`](Self::to_cookie_header) 复用 `sso` —— 实测 grok.com
    /// 这两个字段是同一个 JWT(chenyme `_statsig_id` 上游证实)。
    pub sso_rw: Option<String>,
    /// Cloudflare 通过 token(可选,~7 天过期)
    pub cf_clearance: Option<String>,
    /// 用户 UUID(可选,推测 grok.com 用于路由优化)
    pub x_userid: Option<String>,
    /// Cloudflare bot management token(可选)
    pub cf_bm: Option<String>,
    /// 其他 cookie 透传(mixpanel/stripe/optanon/i18next 等)
    pub others: Vec<(String, String)>,
    /// **完整 Cookie 字符串 paste**(对齐 chenyme `proxy.clearance.cf_cookies`)。
    ///
    /// 用户从浏览器 DevTools Network → grok.com 请求 → Request Headers →
    /// `Cookie: ...` 整段 value 粘贴。包含 `__cf_bm` / `cf_clearance` /
    /// `mp_xxx_mixpanel` / `__stripe_*` / `OptanonConsent` / `i18nextLng` 等。
    ///
    /// 拼接策略([`to_cookie_header`](Self::to_cookie_header)):忠实 chenyme
    /// `build_sso_cookie` 行为 —— 总是先输出 `sso=X; sso-rw=X`,然后追加这一
    /// 整段(可能内部含同名 sso/sso-rw —— grok.com 按 cookie 顺序最后值 wins,
    /// 但实测 chenyme 不去重,我们 follow)。
    ///
    /// 来源:`provider.extra.grokWeb.cookies.cookieString`(单一 JSON string)。
    pub cookie_string: Option<String>,
}

impl GrokCookies {
    /// 从 Provider extra 中提取。
    ///
    /// 路径:`provider.extra["grokWeb"]["cookies"]` —— JSON object,key 是 cookie 名,
    /// value 是 string。
    ///
    /// 仅 `sso` 必需;缺失时返回 `Err`,让 forward 层 surface 401 给客户端。
    pub fn from_provider(provider: &Provider) -> Result<Self, GrokAuthError> {
        let grok_web = provider
            .extra
            .get("grokWeb")
            .and_then(Value::as_object)
            .ok_or(GrokAuthError::MissingGrokWebConfig)?;
        let cookies = grok_web
            .get("cookies")
            .and_then(Value::as_object)
            .ok_or(GrokAuthError::MissingCookies)?;

        // 区分三态(silent-failure-hunter H3 反馈):
        //   - key 缺失:None,fallback 路径(sso-rw 复用 sso / cf_clearance 跳过 segment)
        //   - key 存在且为非空 string:Some(value)
        //   - key 存在但 empty string / 非 string:**返 InvalidCookie 错误**
        //     —— 用户填了空字符串显然不是想 fallback,而是 typo / paste 错误,
        //     不该被静默当成 absent;否则 sso-rw 空字符串会被复用为 sso,用户不知情
        let get_optional = |key: &'static str| -> Result<Option<String>, GrokAuthError> {
            let Some(v) = cookies.get(key) else {
                return Ok(None);
            };
            match v.as_str() {
                Some(s) if !s.is_empty() => Ok(Some(s.to_owned())),
                Some(_) => Err(GrokAuthError::InvalidCookie {
                    name: key,
                    reason: "value is empty string; either remove the field or supply a real value"
                        .into(),
                }),
                None => Err(GrokAuthError::InvalidCookie {
                    name: key,
                    reason: "value is not a JSON string".into(),
                }),
            }
        };

        let sso = get_optional("sso")?.ok_or(GrokAuthError::MissingSsoCookie)?;
        let sso_rw = get_optional("sso-rw")?;
        let cf_clearance = get_optional("cf_clearance")?;
        let x_userid = get_optional("x-userid")?;
        let cf_bm = get_optional("__cf_bm")?;
        // **整段 Cookie paste**(对齐 chenyme `proxy.clearance.cf_cookies`):
        // user 从 DevTools Network 复制 `Cookie:` header value 整段。容忍
        // 用户复制时多带的 "Cookie: " 前缀(常见 Chrome / Safari Copy as cURL
        // 行为),trim 掉再存。
        let cookie_string = get_optional("cookieString")?.map(|s| {
            let trimmed = s.trim();
            let stripped = trimmed
                .strip_prefix("Cookie:")
                .or_else(|| trimmed.strip_prefix("cookie:"))
                .map(str::trim)
                .unwrap_or(trimmed);
            stripped.to_owned()
        });

        // 收集"其他" cookie(实测里有:mixpanel/stripe/optanon/i18nextLng 等)
        let known_keys = [
            "sso",
            "sso-rw",
            "cf_clearance",
            "x-userid",
            "__cf_bm",
            "cookieString", // 不当 cookie 透传(它是单独 paste 入口,见下方拼接)
        ];
        let others: Vec<(String, String)> = cookies
            .iter()
            .filter_map(|(k, v)| {
                if known_keys.contains(&k.as_str()) {
                    return None;
                }
                let val = v.as_str()?.to_owned();
                Some((k.clone(), val))
            })
            .collect();

        Ok(Self {
            sso,
            sso_rw,
            cf_clearance,
            x_userid,
            cf_bm,
            others,
            cookie_string,
        })
    }

    /// 拼成 `Cookie:` header 单行(按 RFC 6265 用 `; ` 分隔)。
    ///
    /// 拼接顺序(忠实 chenyme `build_sso_cookie`):
    /// 1. `sso=<JWT>` 总是出现(必填)
    /// 2. `sso-rw=<JWT>`(缺失时复用 sso,chenyme 行为)
    /// 3. `cf_clearance` / `x-userid` / `__cf_bm`(各自单字段,可选)
    /// 4. `others` 透传(mixpanel/stripe/optanon 等已知 key 之外的)
    /// 5. **`cookieString` 整段**(可选,user 从浏览器 paste 的 Cookie header
    ///    value;追加在最后,可能含同名重复段 —— grok.com 按 cookie 顺序处理,
    ///    后值 wins,chenyme 也不去重 follow)
    pub fn to_cookie_header(&self) -> String {
        let mut parts: Vec<String> = Vec::with_capacity(5 + self.others.len() + 1);
        parts.push(format!("sso={}", self.sso));
        let sso_rw_val = self.sso_rw.as_deref().unwrap_or(&self.sso);
        parts.push(format!("sso-rw={sso_rw_val}"));
        if let Some(cf) = &self.cf_clearance {
            parts.push(format!("cf_clearance={cf}"));
        }
        if let Some(uid) = &self.x_userid {
            parts.push(format!("x-userid={uid}"));
        }
        if let Some(bm) = &self.cf_bm {
            parts.push(format!("__cf_bm={bm}"));
        }
        for (k, v) in &self.others {
            parts.push(format!("{k}={v}"));
        }
        if let Some(s) = &self.cookie_string {
            if !s.is_empty() {
                parts.push(s.clone());
            }
        }
        parts.join("; ")
    }
}

/// grok.com 鉴权失败分类(`forward.rs` 据此 surface 友好错误给客户端)。
#[derive(Debug, thiserror::Error)]
pub enum GrokAuthError {
    #[error("provider.extra missing `grokWeb` object")]
    MissingGrokWebConfig,
    #[error("provider.extra.grokWeb missing `cookies` object")]
    MissingCookies,
    /// Plan A 下 `sso` 是唯一必填 cookie;sso-rw / cf_clearance 都改 optional,
    /// 所以错误枚举从 stringly-typed `MissingCookie(String)` 收敛到这个具体变体
    /// (type-design-analyzer 反馈:零下游模式匹配字符串内容,无信息损失)。
    #[error("provider.extra.grokWeb.cookies missing required cookie `sso`")]
    MissingSsoCookie,
    /// cookie key 存在于 JSON object 但 value 是 empty string / 非 string。
    /// silent-failure-hunter H3 反馈:不该当 absent 静默 fallback,而是显式报错。
    #[error("provider.extra.grokWeb.cookies has invalid value for `{name}`: {reason}")]
    InvalidCookie { name: &'static str, reason: String },
    /// `extra.grokWeb.statsigId` key 存在但 value 是 empty / 非 string。
    /// silent-failure-hunter H1 反馈:用户显式填空字符串不该被静默替换成动态生成。
    #[error("provider.extra.grokWeb.statsigId is invalid: {0}")]
    InvalidStatsigId(String),
    /// 用户提供的 header value(cookie / statsigId / userAgent)含 invalid 字符
    /// (newline / control byte / non-ASCII 等),无法通过 `HeaderValue::from_str`。
    ///
    /// **R1 PR-4 / H1 完整版**:silent-failure-hunter H1 标记原 `insert` 静默
    /// `tracing::warn!` + drop header → 用户 IP/UA 带空 header 发请求 → grok.com
    /// 401 → 用户看不到清晰错误。改成 propagate 到 forward 主路径 surface 400。
    #[error("invalid header value for `{name}`: {reason}")]
    InvalidHeaderValue { name: &'static str, reason: String },
}

/// 注入 grok.com 所需 headers 到一个新构造的 [`HeaderMap`] 并返回。
///
/// 推荐入口(替代 [`apply_grok_headers`] 的 `&mut HeaderMap` 接口),错误会
/// 显式 propagate 给调用方;Cookie / x-statsig-id 自动 `set_sensitive(true)`
/// 防止落进 tracing 结构化日志(review-feedback I6)。
///
/// 调用方:`crates/proxy/src/forward.rs::build_and_send_upstream` GrokCookie 分支。
pub fn apply_grok_headers_typed(provider: &Provider) -> Result<HeaderMap, GrokAuthError> {
    let mut headers = HeaderMap::with_capacity(14);
    apply_grok_headers(&mut headers, provider)?;
    // 对 sensitive headers 标记后,reqwest 在日志/debug 序列化时不会暴露 value
    for name in ["cookie", "x-statsig-id"] {
        if let Some(value) = headers.get_mut(name) {
            value.set_sensitive(true);
        }
    }
    Ok(headers)
}

/// 注入 grok.com 所需 headers 到 `RequestPlan` 的 HeaderMap。
///
/// 调用方:`mapper::grok_web::prepare_grok_web_request` 在构造 RequestPlan 时调用。
///
/// **注入项**:
/// - `Cookie: sso=...; sso-rw=...; cf_clearance=...; ...`
/// - `User-Agent: <浏览器 UA>`(默认 macOS Safari,可被 Provider extra override)
/// - `Origin: https://grok.com`
/// - `Referer: https://grok.com/`
/// - `Accept: text/event-stream, */*`
/// - `Accept-Language: zh-CN,zh-Hans;q=0.9`(让 grok 默认中文回答;可被 Provider override)
/// - `x-statsig-id: <用户提供>`
/// - `x-xai-request-id: <每次请求生成 UUID>`
/// - `traceparent: 00-<32hex>-<16hex>-00`(自动生成)
///
/// **不注入**:`__cf_bm` cookie 单独 set(它在 Cookie 里已合并)、`sentry-trace`(可选,
/// 实测无该 header 也能 work)。
pub fn apply_grok_headers(
    headers: &mut HeaderMap,
    provider: &Provider,
) -> Result<(), GrokAuthError> {
    let cookies = GrokCookies::from_provider(provider)?;
    let statsig_id = read_statsig_id_or_generate(provider)?;
    let user_agent = read_user_agent(provider);

    insert(headers, "Cookie", &cookies.to_cookie_header())?;
    insert(headers, "User-Agent", &user_agent)?;
    insert(headers, "Origin", "https://grok.com")?;
    insert(headers, "Referer", "https://grok.com/")?;
    insert(headers, "Accept", "text/event-stream, */*")?;
    insert(headers, "Accept-Language", "zh-CN,zh-Hans;q=0.9")?;
    insert(headers, "x-statsig-id", &statsig_id)?;
    insert(headers, "x-xai-request-id", &generate_uuid_v4())?;
    insert(headers, "traceparent", &generate_traceparent())?;

    // CORS hints —— 模拟浏览器行为,降低风控触发概率
    insert(headers, "Sec-Fetch-Site", "same-origin")?;
    insert(headers, "Sec-Fetch-Mode", "cors")?;
    insert(headers, "Sec-Fetch-Dest", "empty")?;

    Ok(())
}

/// 读取用户显式提供的 `statsigId`,否则动态生成一个伪造的(参考 chenyme `_statsig_id`)。
///
/// **三态语义**(silent-failure-hunter H1 反馈):
///   - key 缺失或顶层 grokWeb 缺失 → 动态生成
///   - key 存在且为非空 string → 用 user-supplied 值(escape hatch)
///   - key 存在但 empty string / 非 string → 报错,不静默 fallback
///     (用户填 `""` 显然不是想"用空 statsig",而是手滑;静默替换成动态生成
///      会让用户在 debug 时困惑"我明明设了空值怎么还有 header")
fn read_statsig_id_or_generate(provider: &Provider) -> Result<String, GrokAuthError> {
    let Some(grok_web) = provider.extra.get("grokWeb").and_then(Value::as_object) else {
        return Ok(generate_statsig_id());
    };
    match grok_web.get("statsigId") {
        None => Ok(generate_statsig_id()),
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        Some(Value::String(_)) => Err(GrokAuthError::InvalidStatsigId(
            "value is empty string; remove the field to use dynamic generation".into(),
        )),
        Some(_) => Err(GrokAuthError::InvalidStatsigId(
            "value is not a JSON string".into(),
        )),
    }
}

/// 动态伪造一个 `x-statsig-id` header value。
///
/// **算法**(参考 chenyme/grok2api `app/dataplane/proxy/adapters/headers.py::_statsig_id`):
///
/// grok.com 服务端把 `x-statsig-id` 当成 Statsig SDK 客户端上报的 JS error message
/// blob 读(base64 解码后期望看到 `e:TypeError: ...` 这种格式),但**只看格式不看内容**。
/// chenyme 实证只要伪造两种之一的 error message 然后 base64 编码即可:
///
/// 1. `e:TypeError: Cannot read properties of null (reading 'children['{rand5}']')`(rand5 = 5 字符 alnum)
/// 2. `e:TypeError: Cannot read properties of undefined (reading '{rand10}')`(rand10 = 10 字符 alpha)
///
/// 每次请求随机一个 variant + 随机 padding,base64 编码后即可通过 grok.com 验证。
///
/// 用户**不需要**从浏览器 DevTools 抠这个 header,UI 上完全隐藏。
///
/// **可见性**:`pub(crate)` —— grok.com 反指纹专用,不是通用工具,不应外部使用。
/// 跟 `generate_uuid_v4` 同 visibility(type-design-analyzer 反馈一致性)。
pub(crate) fn generate_statsig_id() -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let mut entropy = [0u8; 16];
    getrandom::getrandom(&mut entropy).expect("OS RNG should not fail");
    let pick_variant = (entropy[0] & 1) == 0;
    let alnum: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let alpha: &[u8] = b"abcdefghijklmnopqrstuvwxyz";

    // L1 修(silent-failure-hunter):用 `(i + 1) % 16` 让 entropy index 跟循环长度
    // 解耦,16 是 entropy 数组实际长度,index 边界靠 mod 兜底,不再"correctness-by-accident"
    let msg = if pick_variant {
        let rand: String = (0..5)
            .map(|i| alnum[entropy[(i + 1) % 16] as usize % alnum.len()] as char)
            .collect();
        format!("e:TypeError: Cannot read properties of null (reading 'children['{rand}']')")
    } else {
        let rand: String = (0..10)
            .map(|i| alpha[entropy[(i + 1) % 16] as usize % alpha.len()] as char)
            .collect();
        format!("e:TypeError: Cannot read properties of undefined (reading '{rand}')")
    };
    B64.encode(msg.as_bytes())
}

fn read_user_agent(provider: &Provider) -> String {
    provider
        .extra
        .get("grokWeb")
        .and_then(Value::as_object)
        .and_then(|o| o.get("userAgent"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            // 默认 UA:macOS Safari 26.4(对齐实测抓包)
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.4 Safari/605.1.15"
                .to_owned()
        })
}

/// 生成 UUID v4(随机)。供 `x-xai-request-id` 与 grok_web 内部 response_id 复用。
///
/// 用 `getrandom`(crate 已有依赖)而非 `uuid`,避免新增依赖项;
/// 手写 RFC 4122 v4 编码也只 ~20 行。
pub(crate) fn generate_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("OS RNG should not fail");
    // RFC 4122 v4 + variant bits
    bytes[6] = (bytes[6] & 0x0F) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3F) | 0x80; // variant 1 (RFC 4122)
    format_uuid_v4(&bytes)
}

fn format_uuid_v4(b: &[u8; 16]) -> String {
    format!(
        "{}-{}-{}-{}-{}",
        hex_encode(&b[0..4]),
        hex_encode(&b[4..6]),
        hex_encode(&b[6..8]),
        hex_encode(&b[8..10]),
        hex_encode(&b[10..16]),
    )
}

/// 生成符合 W3C Trace Context spec 的 `traceparent` header。
///
/// 格式:`00-<32hex>-<16hex>-00`
/// - `00`:version
/// - 32hex:trace-id(128 bit,随机)
/// - 16hex:parent-id(64 bit,随机)
/// - `00`:flags(`00` 表示不强制采样)
fn generate_traceparent() -> String {
    let mut trace_id = [0u8; 16];
    let mut parent_id = [0u8; 8];
    getrandom::getrandom(&mut trace_id).expect("OS RNG should not fail");
    getrandom::getrandom(&mut parent_id).expect("OS RNG should not fail");
    format!("00-{}-{}-00", hex_encode(&trace_id), hex_encode(&parent_id),)
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

/// 注入一个 grok-owned header,失败 propagate 给调用方(R1 PR-4 / H1 完整版)。
///
/// - header **名字**(`&'static str` literal)无效是 internal bug,panic-level
///   严重(说明我们 hardcoded 写错了),但保守起见返 Err 不 panic
/// - header **值**(用户提供的 cookie / statsigId / UA)无效是常见 config error
///   (用户从浏览器复制时带了 newline / 控制字符 / non-ASCII),必须 surface
///   到 forward 主路径,**不要**静默 drop(原 R3 PoC 行为)
fn insert(headers: &mut HeaderMap, name: &'static str, value: &str) -> Result<(), GrokAuthError> {
    let Ok(header_name) = HeaderName::try_from(name) else {
        return Err(GrokAuthError::InvalidHeaderValue {
            name,
            reason: "internal: hardcoded header name failed HeaderName::try_from".into(),
        });
    };
    let Ok(header_value) = HeaderValue::from_str(value) else {
        // SECURITY:**绝不**把 `value` 本身放进 reason —— grok_web cookie / sso JWT
        // 含敏感凭证,reason 字段可能进 tracing / 错误 surface 给 UI,只能放
        // 不含 value 的元信息(field name + length)。silent-failure-hunter L3 标记。
        return Err(GrokAuthError::InvalidHeaderValue {
            name,
            reason: format!(
                "user-supplied value contains invalid chars (control byte / non-ASCII / line break?), length={}",
                value.len()
            ),
        });
    };
    headers.insert(header_name, header_value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_app_transfer_registry::Provider;
    use indexmap::IndexMap;
    use serde_json::json;

    fn provider_with_grok_web(extra: Value) -> Provider {
        let mut p = Provider {
            id: "grok-web-supergrok".into(),
            name: "Grok Web".into(),
            base_url: "https://grok.com".into(),
            auth_scheme: "grok_cookie".into(),
            api_format: "grok_web".into(),
            api_key: String::new(),
            models: IndexMap::new(),
            extra_headers: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        };
        if let Value::Object(map) = extra {
            for (k, v) in map {
                p.extra.insert(k, v);
            }
        }
        p
    }

    #[test]
    fn from_provider_only_requires_sso() {
        // Plan A:仅 sso 必填,sso-rw / cf_clearance 都 optional。
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "jwt-1" }
            }
        }));
        let c = GrokCookies::from_provider(&p).unwrap();
        assert_eq!(c.sso, "jwt-1");
        assert!(c.sso_rw.is_none());
        assert!(c.cf_clearance.is_none());
        assert!(c.x_userid.is_none());
    }

    #[test]
    fn from_provider_reads_all_optional_when_present() {
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": {
                    "sso": "jwt-1",
                    "sso-rw": "jwt-2",
                    "cf_clearance": "cf-3"
                }
            }
        }));
        let c = GrokCookies::from_provider(&p).unwrap();
        assert_eq!(c.sso, "jwt-1");
        assert_eq!(c.sso_rw.as_deref(), Some("jwt-2"));
        assert_eq!(c.cf_clearance.as_deref(), Some("cf-3"));
    }

    #[test]
    fn from_provider_empty_string_sso_rw_errors_not_silent_fallback() {
        // silent-failure-hunter H3:用户填了空 sso-rw,不能静默复用 sso —— 那是手滑/typo,
        // 不是"想 fallback"。
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "jwt-1", "sso-rw": "" }
            }
        }));
        match GrokCookies::from_provider(&p).unwrap_err() {
            GrokAuthError::InvalidCookie { name, .. } => assert_eq!(name, "sso-rw"),
            other => panic!("expected InvalidCookie(sso-rw), got {other:?}"),
        }
    }

    #[test]
    fn from_provider_non_string_cookie_value_errors() {
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "jwt-1", "cf_clearance": 12345 }
            }
        }));
        match GrokCookies::from_provider(&p).unwrap_err() {
            GrokAuthError::InvalidCookie { name, .. } => assert_eq!(name, "cf_clearance"),
            other => panic!("expected InvalidCookie(cf_clearance), got {other:?}"),
        }
    }

    #[test]
    fn empty_statsig_id_string_errors_not_silent_dynamic_fallback() {
        // silent-failure-hunter H1:用户显式填 `""` 不该被静默替换成动态生成。
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "j1" },
                "statsigId": ""
            }
        }));
        let mut headers = HeaderMap::new();
        match apply_grok_headers(&mut headers, &p).unwrap_err() {
            GrokAuthError::InvalidStatsigId(_) => {}
            other => panic!("expected InvalidStatsigId, got {other:?}"),
        }
    }

    #[test]
    fn non_string_statsig_id_errors() {
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "j1" },
                "statsigId": 12345
            }
        }));
        let mut headers = HeaderMap::new();
        match apply_grok_headers(&mut headers, &p).unwrap_err() {
            GrokAuthError::InvalidStatsigId(_) => {}
            other => panic!("expected InvalidStatsigId, got {other:?}"),
        }
    }

    #[test]
    fn from_provider_missing_sso_errors() {
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": {
                    // sso 缺失,其它字段无意义
                    "cf_clearance": "c"
                }
            }
        }));
        match GrokCookies::from_provider(&p).unwrap_err() {
            GrokAuthError::MissingSsoCookie => {}
            other => panic!("expected MissingSsoCookie, got {other:?}"),
        }
    }

    #[test]
    fn cookie_header_reuses_sso_when_sso_rw_missing() {
        // chenyme `sso={t}; sso-rw={t}` 行为复刻:用户只提供 sso 时自动双写。
        let c = GrokCookies {
            sso: "JWT-X".into(),
            sso_rw: None,
            cf_clearance: None,
            x_userid: None,
            cf_bm: None,
            others: vec![],
            cookie_string: None,
        };
        let h = c.to_cookie_header();
        assert!(h.contains("sso=JWT-X"));
        assert!(h.contains("sso-rw=JWT-X"));
        assert!(!h.contains("cf_clearance="));
    }

    #[test]
    fn cookie_header_concatenates_all_optional_fields() {
        let c = GrokCookies {
            sso: "a".into(),
            sso_rw: Some("b".into()),
            cf_clearance: Some("c".into()),
            x_userid: Some("d".into()),
            cf_bm: None,
            others: vec![("i18nextLng".into(), "zh".into())],
            cookie_string: None,
        };
        let h = c.to_cookie_header();
        assert!(h.contains("sso=a"));
        assert!(h.contains("sso-rw=b"));
        assert!(h.contains("cf_clearance=c"));
        assert!(h.contains("x-userid=d"));
        assert!(h.contains("i18nextLng=zh"));
    }

    #[test]
    fn cookie_string_paste_appended_to_header() {
        // user E2E 反馈(2026-05-12):SuperGrok + CF challenge 网络要求完整
        // Cookie 整段,单字段 sso/cf_clearance 不够。textarea 让用户 paste
        // 整段 Cookie header value(可能含 Cookie: 前缀,trim 掉)。
        let c = GrokCookies {
            sso: "JWT-X".into(),
            sso_rw: None,
            cf_clearance: None,
            x_userid: None,
            cf_bm: None,
            others: vec![],
            cookie_string: Some(
                "__cf_bm=Z; mp_xxx_mixpanel=Y; OptanonConsent=isGpcEnabled=0".into(),
            ),
        };
        let h = c.to_cookie_header();
        // 顺序:sso → sso-rw → 整段追加
        assert!(h.starts_with("sso=JWT-X; sso-rw=JWT-X; __cf_bm=Z;"));
        assert!(h.contains("OptanonConsent="));
    }

    #[test]
    fn cookie_string_paste_strips_cookie_header_prefix() {
        // 容忍 user 复制时多带 "Cookie: " 前缀(Chrome / Safari Copy as cURL
        // 默认行为)
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": {
                    "sso": "JWT",
                    "cookieString": "Cookie: __cf_bm=A; cf_clearance=B"
                }
            }
        }));
        let c = GrokCookies::from_provider(&p).unwrap();
        let h = c.to_cookie_header();
        // "Cookie: " 前缀已剥(整段从 __cf_bm= 开始)
        assert!(h.ends_with("__cf_bm=A; cf_clearance=B"));
        assert!(!h.contains("Cookie: "));
    }

    #[test]
    fn apply_grok_headers_injects_full_set_with_dynamic_statsig() {
        // Plan A:用户只填 sso,statsig 后端动态生成。
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "jwt-only" }
            }
        }));
        let mut headers = HeaderMap::new();
        apply_grok_headers(&mut headers, &p).unwrap();
        assert!(headers.contains_key("cookie"));
        assert!(headers.contains_key("user-agent"));
        assert!(headers.contains_key("origin"));
        assert!(headers.contains_key("referer"));
        assert!(headers.contains_key("x-statsig-id"));
        assert!(headers.contains_key("x-xai-request-id"));
        assert!(headers.contains_key("traceparent"));
        let statsig = headers.get("x-statsig-id").unwrap().to_str().unwrap();
        assert!(!statsig.is_empty(), "动态生成的 statsig 不应为空");
    }

    #[test]
    fn dynamic_statsig_id_is_valid_base64_and_decodes_to_error_msg() {
        use base64::{engine::general_purpose::STANDARD as B64, Engine};
        let id = generate_statsig_id();
        let bytes = B64.decode(id.as_bytes()).expect("应为合法 base64");
        let decoded = String::from_utf8(bytes).expect("decode 后应为 UTF-8");
        assert!(
            decoded.starts_with("e:TypeError:"),
            "动态 statsig 应包含伪造的 TypeError 前缀,实际:{decoded}"
        );
    }

    #[test]
    fn dynamic_statsig_id_changes_between_calls() {
        // 防止动态生成实际成了 const(熵不够);128bit entropy 实际碰撞概率 ~0。
        let a = generate_statsig_id();
        let b = generate_statsig_id();
        assert_ne!(a, b, "两次动态生成的 statsig 应不同");
    }

    #[test]
    fn user_provided_statsig_id_takes_priority_over_dynamic() {
        // escape hatch:高级用户精确控制 statsig 时仍可显式提供。
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "jwt-1" },
                "statsigId": "user-overrides-this"
            }
        }));
        let mut headers = HeaderMap::new();
        apply_grok_headers(&mut headers, &p).unwrap();
        assert_eq!(
            headers.get("x-statsig-id").unwrap().to_str().unwrap(),
            "user-overrides-this"
        );
    }

    #[test]
    fn invalid_user_provided_statsig_id_with_control_char_propagates_error() {
        // R1 PR-4 / H1:用户显式提供的 statsigId 带 newline 时仍要 surface 400,
        // 不要静默 drop —— 用户期望"我填什么就用什么",值不合法必须报清楚。
        let p = provider_with_grok_web(json!({
            "grokWeb": {
                "cookies": { "sso": "jwt-1" },
                "statsigId": "stat-id\nwith-newline"
            }
        }));
        let mut headers = HeaderMap::new();
        let err = apply_grok_headers(&mut headers, &p).unwrap_err();
        match err {
            GrokAuthError::InvalidHeaderValue { name, reason } => {
                assert_eq!(name, "x-statsig-id");
                assert!(
                    reason.contains("invalid chars"),
                    "expected detailed reason, got: {reason}"
                );
            }
            other => panic!("expected InvalidHeaderValue, got: {other:?}"),
        }
    }
}
