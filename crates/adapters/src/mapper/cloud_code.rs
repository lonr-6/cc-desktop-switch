use crate::mapper::{RequestMapper, ResponseMapper};
use crate::types::AdapterError;
use crate::types::{ByteStream, RequestPlan, ResponsePlan};
use codex_app_transfer_registry::Provider;
use http::{header::HeaderValue, HeaderMap, StatusCode};
use serde_json::{json, Value};

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CloudCodeMapper;

/// cloud-code OAuth 路径 flavor:
/// - `GeminiCli`: gemini-cli client_id 路径
/// - `Antigravity`: antigravity client_id 路径
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloudCodeApiFlavor {
    GeminiCli,
    Antigravity,
}

impl CloudCodeApiFlavor {
    pub(crate) fn from_api_format(api_format: &str) -> Self {
        if matches!(
            api_format.to_ascii_lowercase().as_str(),
            "antigravity_oauth" | "antigravity" | "google_oauth_antigravity"
        ) {
            Self::Antigravity
        } else {
            Self::GeminiCli
        }
    }

    pub(crate) fn is_antigravity(self) -> bool {
        matches!(self, Self::Antigravity)
    }
}

/// 按 provider `api_format` 选择 token 文件名。
pub(crate) fn token_filename_for_api_format(api_format: &str) -> &'static str {
    if CloudCodeApiFlavor::from_api_format(api_format).is_antigravity() {
        "antigravity-oauth.json"
    } else {
        "gemini-oauth.json"
    }
}

/// 解析 cloud-code project_id，优先 provider.extra，再 fallback token store。
pub(crate) fn resolve_cloud_code_project_id(provider: &Provider) -> Result<String, AdapterError> {
    let token_filename = token_filename_for_api_format(&provider.api_format);
    provider
        .extra
        .get("cloud_code_project_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
        .or_else(|| {
            codex_app_transfer_gemini_oauth::TokenStore::for_token_filename(token_filename)
                .ok()
                .and_then(|store| store.load().ok().flatten())
                .and_then(|token| token.project_id)
        })
        .ok_or_else(|| {
            AdapterError::BadRequest(format!(
                "cloud_code_project_id missing in both provider.extra and \
                 ~/.codex-app-transfer/{token_filename} — run OAuth login \
                 to bootstrap project"
            ))
        })
}

/// cloud-code 固定上游路径(仅按 stream 区分)。
pub(crate) fn cloud_code_upstream_path(stream: bool) -> String {
    if stream {
        "/v1internal:streamGenerateContent?alt=sse".to_owned()
    } else {
        "/v1internal:generateContent".to_owned()
    }
}

/// 用 OS RNG 生成 UUID v4 字符串(`8-4-4-4-12` hex 形态)。
fn uuid_v4() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)?;
    // version 4(top 4 bits of byte 6 = 0100)
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    // variant 10(top 2 bits of byte 8 = 10)
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ))
}

/// 把 gemini_native 产出的 inner body 包成 Cloud Code outer envelope。
pub(crate) fn wrap_cloud_code_envelope(
    model: &str,
    project_id: &str,
    inner: Value,
) -> Result<Value, getrandom::Error> {
    Ok(json!({
        "model": model,
        "project": project_id,
        "user_prompt_id": uuid_v4()?,
        "request": inner,
    }))
}

/// Antigravity 专属 body 后处理。
pub(crate) fn apply_antigravity_transform(
    mut envelope: Value,
    model: &str,
) -> Result<Value, getrandom::Error> {
    let is_image = model.contains("image");

    let envelope_obj = envelope
        .as_object_mut()
        .ok_or_else(|| getrandom::Error::from(std::num::NonZeroU32::new(1).unwrap()))?;

    envelope_obj.insert("userAgent".into(), Value::String("antigravity".into()));
    envelope_obj.insert(
        "requestType".into(),
        Value::String(if is_image { "image_gen" } else { "agent" }.into()),
    );

    let request_id = if is_image {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("image_gen/{}/{}/12", now_ms, uuid_v4()?)
    } else {
        format!("agent-{}", uuid_v4()?)
    };
    envelope_obj.insert("requestId".into(), Value::String(request_id));

    if let Some(request_obj) = envelope_obj
        .get_mut("request")
        .and_then(|v| v.as_object_mut())
    {
        request_obj.remove("safetySettings");

        if !is_image {
            let session_id = stable_session_id_from_request(request_obj);
            request_obj.insert("sessionId".into(), Value::String(session_id));
        }
    }

    let top_tool_config = envelope_obj.get("toolConfig").cloned();
    if let Some(tc) = top_tool_config {
        let req_has_tc = envelope_obj
            .get("request")
            .and_then(|v| v.as_object())
            .map(|r| r.contains_key("toolConfig"))
            .unwrap_or(false);
        if !req_has_tc {
            if let Some(request_obj) = envelope_obj
                .get_mut("request")
                .and_then(|v| v.as_object_mut())
            {
                request_obj.insert("toolConfig".into(), tc);
            }
            envelope_obj.remove("toolConfig");
        }
    }

    Ok(envelope)
}

/// 从 request 对象拿第一条 user message text,SHA256 → int64 正值 → "-<n>"。
fn stable_session_id_from_request(request_obj: &serde_json::Map<String, Value>) -> String {
    let text = request_obj
        .get("contents")
        .and_then(|c| c.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|content| {
                if content.get("role").and_then(|r| r.as_str()) == Some("user") {
                    content
                        .get("parts")
                        .and_then(|p| p.as_array())
                        .and_then(|parts| parts.first())
                        .and_then(|p0| p0.get("text"))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_owned())
                } else {
                    None
                }
            })
        });

    if let Some(t) = text {
        let hash = sha256_first_8_bytes(t.as_bytes());
        let n = i64::from_be_bytes(hash) & 0x7FFFFFFFFFFFFFFFi64;
        return format!("-{n}");
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64 & 0x7FFFFFFFFFFFFFFFi64)
        .unwrap_or(1);
    format!("-{now}")
}

fn sha256_first_8_bytes(input: &[u8]) -> [u8; 8] {
    let full = sha256(input);
    let mut out = [0u8; 8];
    out.copy_from_slice(&full[..8]);
    out
}

/// SHA-256 实现(RFC 6234)。手动 — 避免新引 sha2 crate
fn sha256(message: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let bit_len = (message.len() as u64) * 8;
    let mut padded = message.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// cloud-code 请求侧编排：
/// - 解析入站 responses body
/// - 调 gemini_native 做 inner request 映射
/// - 应用 cloud-code flavor 兼容规则
/// - 包装 outer envelope(+ antigravity transform)
/// - 生成 cloud-code upstream path
pub(crate) fn prepare_cloud_code_request(
    body: bytes::Bytes,
    provider: &Provider,
) -> Result<RequestPlan, AdapterError> {
    let parsed: Value = serde_json::from_slice(&body)?;
    let stream = parsed
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let model = parsed
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AdapterError::BadRequest("model field required".into()))?
        .to_owned();

    let flavor = CloudCodeApiFlavor::from_api_format(&provider.api_format);
    let project_id = resolve_cloud_code_project_id(provider)?;

    // **task 25 / silent-failure-hunter PR #145 HIGH-2 修(2026-05-13)**:旧
    // 实现调 `responses_body_to_gemini_request`(无 _with_session 版本),完全
    // 不传 session_cache → cloud_code prod 路径(gemini_cli OAuth)multi-turn
    // 历史完全丢失,autocompact / function_call_output 全断,触发 task #24 加
    // 的 `CORE_INPUT_PREV_ID_WITHOUT_CACHE` warn。
    //
    // 修复:改用 `_with_session` + `global_response_session_cache()`,跟 gemini_native
    // mapper(主路径 `mapper/gemini_native.rs:63-67`)对齐。response_session 同步
    // 注入 RequestPlan,让 SSE 流末 converter 把 user+assistant messages append
    // 进 cache 供下轮 `previous_response_id` 拉历史。
    //
    // OAuth 路径跟 API key 路径共用同一全局 cache(`~/.codex-app-transfer/sessions.db`)。
    // 不需要 user/session 隔离 — `response_id` 形如 `resp_<unix_nanos_hex>`
    // (`core/input::response_id_for_session()`),OAuth 跟 API key 走**同一个生成器**,
    // collision domain 跟既有 gemini_native 主路径完全一致,本 PR 不引入新碰撞风险;
    // 切换用户重新 OAuth 后旧 response_id 不可能被新 session 反查到,行为天然隔离。
    let conversion = crate::gemini_native::request::responses_body_to_gemini_request_with_session(
        &parsed,
        provider,
        Some(crate::responses::session::global_response_session_cache()),
    )?;
    let mut inner_value =
        serde_json::to_value(&conversion.request).map_err(AdapterError::BodyDecode)?;
    apply_cloud_code_request_compat(&mut inner_value, flavor);

    let outer = wrap_cloud_code_envelope(&model, &project_id, inner_value).map_err(|e| {
        AdapterError::BadRequest(format!("OS RNG unavailable for user_prompt_id: {e}"))
    })?;
    let outer = if flavor.is_antigravity() {
        apply_antigravity_transform(outer, &model).map_err(|e| {
            AdapterError::BadRequest(format!("OS RNG unavailable for antigravity requestId: {e}"))
        })?
    } else {
        outer
    };
    let outer_body = serde_json::to_vec(&outer).map_err(AdapterError::BodyDecode)?;

    Ok(RequestPlan {
        upstream_path: cloud_code_upstream_path(stream),
        body: bytes::Bytes::from(outer_body),
        upstream_headers: http::HeaderMap::new(),
        response_session: Some(conversion.response_session),
        adapter_metadata: None,
        is_compact: false,
        original_responses_request: Some(parsed),
    })
}

/// cloud-code 响应流转换：
/// - 非 2xx：复用 gemini_native failure stream 转换
/// - 2xx：先 unwrap cloud-code SSE 外层，再喂 gemini_native SSE->Responses 状态机
pub(crate) fn transform_cloud_code_response_stream(
    upstream_status: StatusCode,
    mut upstream_headers: HeaderMap,
    upstream_stream: ByteStream,
    request_plan: &RequestPlan,
) -> Result<ResponsePlan, AdapterError> {
    upstream_headers.remove(http::header::CONTENT_LENGTH);
    upstream_headers.remove(http::header::CONTENT_ENCODING);
    upstream_headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    if !upstream_status.is_success() {
        let stream =
            crate::gemini_native::response::convert_gemini_error_to_responses_failure_stream(
                upstream_status,
                upstream_stream,
                request_plan.original_responses_request.clone(),
            );
        return Ok(ResponsePlan {
            status: StatusCode::OK,
            headers: upstream_headers,
            stream,
        });
    }
    let unwrapped = crate::gemini_cli::response::unwrap_cloud_code_sse_envelope(upstream_stream);
    let stream = crate::gemini_native::response::convert_gemini_to_responses_stream(
        unwrapped,
        request_plan.original_responses_request.clone(),
        request_plan.response_session.clone(),
    );
    Ok(ResponsePlan {
        status: upstream_status,
        headers: upstream_headers,
        stream,
    })
}

/// cloudcode-pa 不识别 `toolConfig.includeServerSideToolInvocations`(camel/snake)。
/// 无条件 strip 两种形态，`toolConfig` 因此变空时同时移除外层 key。
pub(crate) fn strip_include_server_side_tool_invocations(obj: &mut serde_json::Map<String, Value>) {
    if let Some(tc) = obj.get_mut("toolConfig").and_then(|v| v.as_object_mut()) {
        tc.remove("includeServerSideToolInvocations");
        tc.remove("include_server_side_tool_invocations");
        if tc.is_empty() {
            obj.remove("toolConfig");
        }
    }
}

/// 在 inner Gemini request 上应用 cloud-code flavor 兼容规则。
///
/// - 所有 flavor: strip `includeServerSideToolInvocations`
/// - antigravity: 若 tools 含 functionDeclarations,则 strip built-in tools
///   (`googleSearch`/`urlContext`/`codeExecution`/`googleSearchRetrieval`)
pub(crate) fn apply_cloud_code_request_compat(inner: &mut Value, flavor: CloudCodeApiFlavor) {
    let Some(obj) = inner.as_object_mut() else {
        return;
    };
    strip_include_server_side_tool_invocations(obj);

    if !flavor.is_antigravity() {
        return;
    }
    let Some(tools) = obj.get_mut("tools").and_then(|v| v.as_array_mut()) else {
        return;
    };
    let has_function_decls = tools.iter().any(|t| {
        t.as_object()
            .map(|o| {
                o.contains_key("functionDeclarations") || o.contains_key("function_declarations")
            })
            .unwrap_or(false)
    });
    if !has_function_decls {
        return;
    }

    let before = tools.len();
    tools.retain(|t| {
        t.as_object()
            .map(|o| {
                !o.contains_key("googleSearch")
                    && !o.contains_key("google_search")
                    && !o.contains_key("urlContext")
                    && !o.contains_key("url_context")
                    && !o.contains_key("codeExecution")
                    && !o.contains_key("code_execution")
                    && !o.contains_key("googleSearchRetrieval")
                    && !o.contains_key("google_search_retrieval")
            })
            .unwrap_or(true)
    });
    let stripped = before - tools.len();
    if stripped > 0 {
        tracing::info!(
            error_id = "GEMINI_CLI_BUILTIN_TOOLS_STRIPPED",
            stripped_count = stripped,
            tool_keys = ?["googleSearch", "urlContext", "codeExecution", "googleSearchRetrieval"],
            "antigravity wire 不接受 built-in tools + functionDeclarations 共存,strip 内置工具(模型走 exec_command/curl 自适应)"
        );
    }
    if tools.is_empty() {
        obj.remove("tools");
    }
}

impl RequestMapper for CloudCodeMapper {
    fn map_request(
        &self,
        _client_path: &str,
        body: bytes::Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError> {
        prepare_cloud_code_request(body, provider)
    }
}

impl ResponseMapper for CloudCodeMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        _provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError> {
        transform_cloud_code_response_stream(
            upstream_status,
            upstream_headers,
            upstream_stream,
            request_plan,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uuid_v4_format_matches_rfc_4122() {
        let id = uuid_v4().unwrap();
        assert_eq!(id.len(), 36);
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
        assert!(parts[2].starts_with('4'));
        let variant_char = parts[3].chars().next().unwrap();
        assert!(matches!(variant_char, '8' | '9' | 'a' | 'b'));
    }

    #[test]
    fn uuid_v4_is_random_each_call() {
        let a = uuid_v4().unwrap();
        let b = uuid_v4().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn uuid_v4_returns_result_not_silent_zero() {
        // 锁定签名语义：必须是 fallible Result，不能回退成吞错 zero UUID。
        let result: Result<String, _> = uuid_v4();
        let id = result.expect("uuid_v4 should either return Err or valid UUID string");
        assert_eq!(id.len(), 36);
    }

    #[test]
    fn sha256_matches_rfc_6234_test_vectors() {
        let empty = sha256(b"");
        let empty_hex = empty
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(
            empty_hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let abc = sha256(b"abc");
        let abc_hex = abc
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(
            abc_hex,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn wrap_cloud_code_envelope_has_required_fields() {
        let wrapped =
            wrap_cloud_code_envelope("gemini-2.5-pro", "proj-abc", json!({"k":"v"})).unwrap();
        assert_eq!(
            wrapped.get("model").and_then(|v| v.as_str()),
            Some("gemini-2.5-pro")
        );
        assert_eq!(
            wrapped.get("project").and_then(|v| v.as_str()),
            Some("proj-abc")
        );
        let user_prompt_id = wrapped
            .get("user_prompt_id")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(user_prompt_id.len(), 36);
        assert_eq!(wrapped.get("request"), Some(&json!({"k":"v"})));
    }

    #[test]
    fn cloud_code_api_flavor_aliases_are_stable() {
        for v in [
            "antigravity_oauth",
            "antigravity",
            "google_oauth_antigravity",
            "ANTIGRAVITY",
            "Antigravity",
        ] {
            assert!(
                CloudCodeApiFlavor::from_api_format(v).is_antigravity(),
                "{v}"
            );
        }
        for v in ["gemini_cli_oauth", "google_oauth_cloud_code", "unknown"] {
            assert!(
                !CloudCodeApiFlavor::from_api_format(v).is_antigravity(),
                "{v}"
            );
        }
    }

    #[test]
    fn token_filename_selection_is_stable() {
        assert_eq!(
            token_filename_for_api_format("gemini_cli_oauth"),
            "gemini-oauth.json"
        );
        assert_eq!(
            token_filename_for_api_format("antigravity_oauth"),
            "antigravity-oauth.json"
        );
    }

    #[test]
    fn apply_cloud_code_request_compat_strips_toolconfig_flags_for_all_flavors() {
        for flavor in [
            CloudCodeApiFlavor::GeminiCli,
            CloudCodeApiFlavor::Antigravity,
        ] {
            let mut inner = json!({
                "toolConfig": {
                    "includeServerSideToolInvocations": true,
                    "include_server_side_tool_invocations": true
                }
            });
            apply_cloud_code_request_compat(&mut inner, flavor);
            assert!(inner.get("toolConfig").is_none());
        }
    }

    #[test]
    fn antigravity_compat_strips_builtin_tools_only_when_function_declarations_present() {
        let mut inner = json!({
            "tools": [
                {"googleSearch": {}},
                {"functionDeclarations": [{"name":"f","parameters":{"type":"object"}}]}
            ]
        });
        apply_cloud_code_request_compat(&mut inner, CloudCodeApiFlavor::Antigravity);
        let tools = inner.get("tools").and_then(|v| v.as_array()).unwrap();
        assert_eq!(tools.len(), 1);
        assert!(tools[0].get("functionDeclarations").is_some());

        let mut no_fn_decl = json!({
            "tools": [{"googleSearch": {}}]
        });
        apply_cloud_code_request_compat(&mut no_fn_decl, CloudCodeApiFlavor::Antigravity);
        let tools = no_fn_decl.get("tools").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            tools.len(),
            1,
            "without functionDeclarations should keep built-ins"
        );
    }

    #[test]
    fn wrap_envelope_handles_empty_inner() {
        let wrapped = wrap_cloud_code_envelope("g", "p", json!({})).unwrap();
        assert_eq!(wrapped.get("request"), Some(&json!({})));
    }

    #[test]
    fn antigravity_transform_adds_required_fields_for_text_model() {
        let envelope = json!({
            "model": "gemini-3-pro-low",
            "project": "proj-x",
            "user_prompt_id": "u",
            "request": {
                "contents": [{"role":"user","parts":[{"text":"hello world"}]}],
                "safetySettings": [{"category":"HARM_CATEGORY_HATE_SPEECH","threshold":"BLOCK_NONE"}]
            },
            "toolConfig": {"functionCallingConfig":{"mode":"AUTO"}}
        });

        let out = apply_antigravity_transform(envelope, "gemini-3-pro-low").unwrap();
        assert_eq!(
            out.get("userAgent").and_then(|v| v.as_str()),
            Some("antigravity")
        );
        assert_eq!(
            out.get("requestType").and_then(|v| v.as_str()),
            Some("agent")
        );

        let rid = out.get("requestId").and_then(|v| v.as_str()).unwrap();
        assert!(rid.starts_with("agent-"));

        let req = out.get("request").and_then(|v| v.as_object()).unwrap();
        assert!(!req.contains_key("safetySettings"));
        assert!(req.contains_key("sessionId"));
        assert!(req.contains_key("toolConfig"));
        assert!(out.get("toolConfig").is_none());
    }

    #[test]
    fn antigravity_transform_image_model_uses_image_gen_request_id() {
        let envelope = json!({
            "request": {
                "contents": [{"role":"user","parts":[{"text":"draw cat"}]}],
                "safetySettings": [{"category":"HARM_CATEGORY_HATE_SPEECH","threshold":"BLOCK_NONE"}]
            }
        });
        let out = apply_antigravity_transform(envelope, "gemini-3.1-flash-image").unwrap();
        assert_eq!(
            out.get("requestType").and_then(|v| v.as_str()),
            Some("image_gen")
        );
        let rid = out.get("requestId").and_then(|v| v.as_str()).unwrap();
        assert!(rid.starts_with("image_gen/"));
        let req = out.get("request").and_then(|v| v.as_object()).unwrap();
        assert!(!req.contains_key("sessionId"));
        assert!(!req.contains_key("safetySettings"));
    }

    #[test]
    fn antigravity_transform_does_not_overwrite_existing_request_tool_config() {
        let envelope = json!({
            "request": {
                "contents": [{"role":"user","parts":[{"text":"hi"}]}],
                "toolConfig": {"functionCallingConfig":{"mode":"ANY"}}
            },
            "toolConfig": {"functionCallingConfig":{"mode":"AUTO"}}
        });
        let out = apply_antigravity_transform(envelope, "gemini-3-pro-low").unwrap();
        let req_tc = out
            .get("request")
            .and_then(|v| v.get("toolConfig"))
            .cloned()
            .unwrap();
        assert_eq!(req_tc, json!({"functionCallingConfig":{"mode":"ANY"}}));
        assert!(
            out.get("toolConfig").is_some(),
            "top-level remains when request already has one"
        );
    }
}
