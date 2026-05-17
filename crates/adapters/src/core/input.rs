use serde_json::Value;

use crate::types::AdapterError;

use crate::responses::session::ResponseSessionCache;

/// 生成 `ResponseSessionPlan.response_id`，供 responses/gemini_native 共用。
pub(crate) fn response_id_for_session() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("resp_{nanos:x}")
}

/// 按 `previous_response_id` 把历史消息与当前消息合并。
///
/// 语义对齐现有 `responses`/`gemini_native` 路径:
/// - cache 命中: 历史 + 当前
/// - cache miss 且当前为空: `PreviousResponseNotFound`
/// - cache miss 且当前非空: 降级为仅当前
/// - 若历史里已有 system/developer 且当前首条是 system,去重当前首 system
pub(crate) fn merge_messages_with_previous_response(
    mut current_messages: Vec<Value>,
    original_body: &Value,
    session_cache: Option<&ResponseSessionCache>,
) -> Result<Vec<Value>, AdapterError> {
    let previous_response_id = original_body
        .get("previous_response_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if previous_response_id.is_empty() {
        return Ok(current_messages);
    }

    let Some(cache) = session_cache else {
        // **silent-failure-hunter task 21/22 → 24 MED-1 修(2026-05-13)**:
        // caller(adapter mapper / test harness)给了 `previous_response_id` 但
        // 没给 session_cache 时,旧实现 silent `Ok(current_messages)` → 历史
        // **完全丢**,用户视角是"我接着上一轮聊但 AI 不记得"。type-system 层面
        // 在 PR #144 (grok_web) 已经把入口收紧,本 warn 是兜底捕获**未来其他
        // adapter caller**(gemini_native / cloud_code 等)如果还没收紧时的
        // 信号,让 operator 能 grep 区分"主动无 prev id"vs"adapter 漏配 cache"。
        // **不抛 error**:历史上各 adapter 自己 fn 还接 Option,直接 Err 会破坏
        // 既有非主流路径(test fixture / gemini_native 简化 chat-normalize)。
        tracing::warn!(
            error_id = "CORE_INPUT_PREV_ID_WITHOUT_CACHE",
            previous_response_id = %previous_response_id,
            "core::input::build_messages_from_input 接到 previous_response_id={previous_response_id} \
             但 session_cache=None — adapter caller 漏配 cache,本轮历史完全丢失"
        );
        return Ok(current_messages);
    };

    if let Some(history) = cache.get(previous_response_id) {
        let history_has_system = history.iter().any(|msg| {
            matches!(
                msg.get("role").and_then(|v| v.as_str()),
                Some("system" | "developer")
            )
        });
        if history_has_system
            && current_messages
                .first()
                .and_then(|msg| msg.get("role"))
                .and_then(|v| v.as_str())
                == Some("system")
        {
            current_messages.remove(0);
        }
        let mut messages = history;
        messages.extend(current_messages);
        return Ok(messages);
    }

    if current_messages.is_empty() {
        return Err(AdapterError::PreviousResponseNotFound {
            previous_response_id: previous_response_id.to_owned(),
        });
    }

    // **silent-failure-hunter task 24 LOW-1 修(2026-05-13)**:cache 配置正常但
    // **key miss**(.app 重启后 cache 还没 warm / key 已 TTL evict / response_id
    // 在 tracker 但 cache 不对应)+ 当前 messages 非空 → 旧实现 silent 降级到
    // "仅本轮",历史完全丢但客户端拿到 200 SSE 不会重试。这是文档化的设计
    // ("cache miss 且当前非空 → 降级仅本轮"),但 silent drop 让 operator 看不
    // 出来"系统正在工作还是 cache 没起作用"。emit warn(stable error_id 跟
    // `CORE_INPUT_PREV_ID_WITHOUT_CACHE` 区分:本条意味"cache 配了但 key
    // 失效",前者意味"adapter 漏配 cache")。
    tracing::warn!(
        error_id = "CORE_INPUT_PREV_ID_CACHE_MISS",
        previous_response_id = %previous_response_id,
        "core::input::build_messages_from_input previous_response_id={previous_response_id} \
         在 session_cache 中未命中(TTL evict / 进程重启未 warm / response_id 漂移),\
         降级为仅本轮 messages(历史丢失)"
    );

    Ok(current_messages)
}
