/// 规范化本地 Responses/Messages 路径:
/// - 剥 `/openai` 前缀
/// - `/claude/v1/messages` 归一到 `/messages`
/// - 剥 `/v1` 前缀
pub(crate) fn normalize_local_responses_path(path: &str) -> String {
    let path = path.strip_prefix("/openai").unwrap_or(path);
    if path == "/claude/v1/messages" {
        return "/messages".to_owned();
    }
    if let Some(stripped) = path.strip_prefix("/v1") {
        return if stripped.is_empty() {
            "/".to_owned()
        } else {
            stripped.to_owned()
        };
    }
    path.to_owned()
}

/// 给 passthrough adapter 用:把 client_path(可能含 query)normalize 成上游
/// 标准 path,处理:
/// - `/openai/v1/responses` → `/responses`(剥 legacy `/openai` prefix)
/// - `/claude/v1/messages` → `/messages`(legacy alias)
/// - `/v1/responses` → `/responses`(剥 `/v1`,因 provider.base_url 已带 `/v1`)
/// - 保留 query string
pub(crate) fn rewrite_local_path_for_upstream(client_path: &str) -> String {
    let (path, query) = match client_path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (client_path, None),
    };
    let normalized = normalize_local_responses_path(path);
    match query {
        Some(q) => format!("{normalized}?{q}"),
        None => normalized,
    }
}

/// 是否是 `/responses/compact*` 子路径(本仓库私有扩展,OpenAI 上游不实现)。
/// passthrough adapter 必须排除这条路径,留给 ResponsesAdapter 在本地包装实现。
pub(crate) fn is_responses_compact_subpath(client_path: &str) -> bool {
    let path = client_path.split('?').next().unwrap_or(client_path);
    let normalized = normalize_local_responses_path(path);
    let normalized = normalized.as_str();
    normalized == "/responses/compact" || normalized.starts_with("/responses/compact/")
}

/// 是否精确命中 `/responses/compact` 端点(允许 query 和尾部 `/`)。
/// 不匹配 `/responses/compact/*` 子路径。
pub(crate) fn is_exact_responses_compact_path(client_path: &str) -> bool {
    let path = client_path.split('?').next().unwrap_or(client_path);
    let normalized = normalize_local_responses_path(path);
    normalized.trim_end_matches('/') == "/responses/compact"
}

/// 是否是本地 Responses 路由:
/// `/responses`、`/responses/*`、`/messages`、`/messages/*`
pub(crate) fn is_local_responses_route(client_path: &str) -> bool {
    let path = client_path.split('?').next().unwrap_or(client_path);
    let normalized = normalize_local_responses_path(path);
    let normalized = normalized.as_str();
    normalized == "/responses"
        || normalized.starts_with("/responses/")
        || normalized == "/messages"
        || normalized.starts_with("/messages/")
}

/// 把 `/v1/responses` / `/responses` / `/openai/v1/responses` 以及旧版 message
/// aliases 重定向到 `/chat/completions`(上游 OpenAI Chat 的标准入口)。其它路径透传不动。
pub(crate) fn redirect_responses_to_chat(path: &str) -> String {
    let (path_only, query) = path.split_once('?').unwrap_or((path, ""));
    let normalized = normalize_local_responses_path(path_only);

    let target = if let Some(after) = normalized.strip_prefix("/responses") {
        format!("/chat/completions{after}")
    } else if let Some(after) = normalized.strip_prefix("/messages") {
        format!("/chat/completions{after}")
    } else {
        normalized
    };

    if query.is_empty() {
        target
    } else {
        format!("{target}?{query}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_compact_path_matches_only_endpoint() {
        assert!(is_exact_responses_compact_path("/responses/compact"));
        assert!(is_exact_responses_compact_path("/v1/responses/compact"));
        assert!(is_exact_responses_compact_path(
            "/openai/v1/responses/compact"
        ));
        assert!(is_exact_responses_compact_path(
            "/responses/compact?stream=false"
        ));
        assert!(is_exact_responses_compact_path("/responses/compact/"));

        assert!(!is_exact_responses_compact_path("/responses"));
        assert!(!is_exact_responses_compact_path("/responses/compact/extra"));
        assert!(!is_exact_responses_compact_path("/responses/compact_alt"));
    }
}
