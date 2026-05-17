//! `tracing` → `proxy_telemetry().logs` 桥接 Layer.
//!
//! # 背景
//!
//! workspace 多个 crate 用 `tracing::warn!` / `tracing::error!` 等记录关键事件
//! (`crates/registry/src/healing.rs::heal_builtin_provider_fields` 强制覆盖
//! apiFormat 警告;`crates/adapters/src/lib.rs::warn_once_drop_tool` /
//! `disable_web_search_for` 等)。但本 workspace 历史上**没有**注册任何
//! `tracing_subscriber`,Tauri 桌面用户从 .app 启 binary 也不连终端,
//! 所有 `tracing::*` 事件**默认 drop** —— 用户日志面板和文件都收不到,等于"假修复"。
//!
//! # 修法
//!
//! 注册一个轻量 Layer,把所有 tracing event 转发到 `proxy_telemetry().logs`
//! (跟 `forward.rs::build_and_send_upstream` 用同一通道,落到
//! `~/.codex-app-transfer/logs/proxy-*.log` + 设置面板 logs viewer 双可见)。
//!
//! Layer 仅消费 `tracing::Event`,不开启 span / fmt 等重量级特性,运行时
//! 几乎无开销;一次性根治整 workspace 所有 `tracing::*` silent。
//!
//! # 边界
//!
//! - 只在 src-tauri 桌面 binary `main()` 起手 init;test 按需手动调
//!   `init_global_subscriber()`(workspace 默认 cargo test 不 init)
//! - 不替代 `proxy_telemetry().logs.add(...)` 直接调用;forward.rs / desktop.rs
//!   等可观察性热路径仍直接调 telemetry,**省一次 tracing 中转**。tracing
//!   bridge 只 cover "原本用 tracing 但没桥到 telemetry" 的死代码
//! - **LevelFilter::INFO 兜底**:防未来 dep(reqwest / hyper 等)引入 TRACE 级
//!   per-byte event 淹没 logs viewer。workspace 现有 5 处 tracing 都用 warn,
//!   不受影响
//!
//! # 安全
//!
//! `StringVisitor` 在 push field 前过 `should_redact_field` 黑名单
//! (`api_key` / `authorization` / `bearer` / `token` / `secret` / `password`
//!  含子串 case-insensitive)→ 替换为 `[REDACTED]`,防未来调用方误把 secret
//! 放进 tracing field 直接进 logs viewer / 用户贴 issue 时泄露

use std::fmt::Write as _;

use codex_app_transfer_proxy::proxy_telemetry;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// 把 tracing event 转发到 `proxy_telemetry().logs.add(level, message)`.
#[derive(Debug, Default)]
pub struct TelemetryLogsLayer;

impl<S: Subscriber> Layer<S> for TelemetryLogsLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level_str = match *metadata.level() {
            tracing::Level::ERROR => "ERROR",
            tracing::Level::WARN => "WARN",
            tracing::Level::INFO => "INFO",
            tracing::Level::DEBUG => "DEBUG",
            tracing::Level::TRACE => "TRACE",
        };

        let mut visitor = StringVisitor::default();
        event.record(&mut visitor);

        let mut formatted = String::new();
        if !visitor.message.is_empty() {
            formatted.push_str(&visitor.message);
        }
        for (name, value) in &visitor.fields {
            if !formatted.is_empty() {
                formatted.push(' ');
            }
            let _ = write!(formatted, "{name}={value}");
        }

        // 始终带上 module path 让 logs viewer 可追溯;省略的话 healing.rs /
        // adapters/lib.rs / desktop.rs 等多处 warn 会无法区分来源。
        let target = metadata.target();
        let final_message = if formatted.is_empty() {
            format!("[{target}]")
        } else {
            format!("[{target}] {formatted}")
        };

        proxy_telemetry().logs.add(level_str, final_message);
    }
}

/// 可能含 secret 的 field name 子串(case-insensitive 匹配)。命中替换为
/// `[REDACTED]`,防未来调用方误把 secret 放进 tracing field 直接进 logs。
const SENSITIVE_FIELD_KEYWORDS: &[&str] = &[
    "api_key",
    "apikey",
    "authorization",
    "bearer",
    "token",
    "secret",
    "password",
];

fn should_redact_field(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    SENSITIVE_FIELD_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

/// 把 event 字段 record 到 String — message 字段单独提取,其余 K=V 拼接。
#[derive(Default)]
struct StringVisitor {
    message: String,
    fields: Vec<(&'static str, String)>,
}

impl StringVisitor {
    fn push_field(&mut self, name: &'static str, value: String) {
        if should_redact_field(name) {
            self.fields.push((name, "[REDACTED]".to_owned()));
        } else {
            self.fields.push((name, value));
        }
    }
}

impl Visit for StringVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.push_field(field.name(), value.to_owned());
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{value:?}");
        if field.name() == "message" {
            self.message.push_str(&formatted);
        } else {
            self.push_field(field.name(), formatted);
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.push_field(field.name(), value.to_string());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.push_field(field.name(), value.to_string());
    }
    fn record_i128(&mut self, field: &Field, value: i128) {
        self.push_field(field.name(), value.to_string());
    }
    fn record_u128(&mut self, field: &Field, value: u128) {
        self.push_field(field.name(), value.to_string());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.push_field(field.name(), value.to_string());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.push_field(field.name(), value.to_string());
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.push_field(field.name(), format!("{value}"));
    }
}

/// 在 src-tauri main 早期(tauri builder 之前)调一次,注册全局 subscriber。
///
/// - 用 `try_init()` 而非 `init()`:已经有 subscriber 时返 Err 而不是 panic
/// - **失败 fallback** 直接调 `proxy_telemetry().logs.add` 写 ERROR 让用户看到
///   bridge 没生效(否则就是用户角度新一轮 silent failure)
/// - **成功后 emit INFO** 让 logs viewer 有正向确认 bridge 已激活
pub fn init_global_subscriber() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let subscriber =
        tracing_subscriber::registry().with(TelemetryLogsLayer.with_filter(LevelFilter::INFO));
    match subscriber.try_init() {
        Ok(()) => {
            proxy_telemetry().logs.add(
                "INFO",
                format!(
                    "tracing-bridge active (v{}): all `tracing::*` events from workspace crates now flow into proxy_telemetry().logs",
                    env!("CARGO_PKG_VERSION")
                ),
            );
        }
        Err(e) => {
            proxy_telemetry().logs.add(
                "ERROR",
                format!(
                    "tracing-bridge init failed (likely another subscriber already global): {e}; tracing events from healing.rs / adapters/lib.rs etc. may continue to be silently dropped"
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain_logs() -> Vec<String> {
        proxy_telemetry()
            .logs
            .get_all()
            .into_iter()
            .map(|e| format!("{} {}", e.level, e.message))
            .collect()
    }

    /// 找 logs 里第一条匹配 target 的 entry,避免并行 test 共享全局
    /// `proxy_telemetry()` 时 `last()` 拿到别的 test 写的。
    fn find_log_with_target(target: &str) -> Option<String> {
        drain_logs()
            .into_iter()
            .find(|e| e.contains(&format!("[{target}]")))
    }

    /// 全栈 smoke:init + emit warn/error/info,各自落到 telemetry.logs 对应 level。
    /// 用唯一 target 区分,避免 cargo test 默认并行跑互相串扰。
    #[test]
    fn tracing_events_bridge_to_proxy_telemetry_logs() {
        init_global_subscriber();

        // WARN with fields + message
        tracing::warn!(target: "tb_smoke_warn_3f9c2a", marker = "abc", "smoke test message");
        let warn_entry =
            find_log_with_target("tb_smoke_warn_3f9c2a").expect("WARN log lands in telemetry");
        assert!(warn_entry.starts_with("WARN"));
        assert!(warn_entry.contains("smoke test message"));
        assert!(warn_entry.contains("marker=abc") || warn_entry.contains("marker=\"abc\""));
        assert!(warn_entry.contains("[tb_smoke_warn_3f9c2a]"));

        // ERROR mapping
        tracing::error!(target: "tb_smoke_err_3f9c2a", "err body");
        let err_entry = find_log_with_target("tb_smoke_err_3f9c2a").expect("ERROR landed");
        assert!(err_entry.starts_with("ERROR"));

        // INFO mapping
        tracing::info!(target: "tb_smoke_info_3f9c2a", "info body");
        let info_entry = find_log_with_target("tb_smoke_info_3f9c2a").expect("INFO landed");
        assert!(info_entry.starts_with("INFO"));
    }

    /// pr-test-analyzer #2:visitor 全字段类型(i64/u64/bool/f64/str/Debug)+
    /// 拼接顺序稳定 防回归未来 minor 版本变化导致字段静默丢失
    #[test]
    fn visitor_records_all_field_types() {
        init_global_subscriber();
        tracing::warn!(
            target: "tb_visitor_3f9c2a",
            n_i64 = -7_i64,
            n_u64 = 42_u64,
            ratio = 1.5_f64,
            flag = true,
            text = "hello",
            "msg body"
        );
        let entry = find_log_with_target("tb_visitor_3f9c2a").expect("visitor log lands");
        for needle in [
            "msg body",
            "n_i64=-7",
            "n_u64=42",
            "ratio=1.5",
            "flag=true",
            "text=hello",
        ] {
            assert!(
                entry.contains(needle),
                "field {needle} 必须保留,实际:{entry}"
            );
        }
    }

    /// H4(code-reviewer):sensitive field 必须 redact 防 secret 泄露到 logs viewer
    #[test]
    fn sensitive_fields_are_redacted() {
        init_global_subscriber();
        tracing::warn!(
            target: "tb_redact_3f9c2a",
            api_key = "sk-secret-value",
            authorization = "Bearer abc123",
            session_token = "t-12345",
            user_password = "p@ss",
            safe_field = "visible",
            "msg"
        );
        let entry = find_log_with_target("tb_redact_3f9c2a").expect("redact log lands");
        // 必须 redact
        assert!(
            !entry.contains("sk-secret-value"),
            "api_key 不能出现:{entry}"
        );
        assert!(
            !entry.contains("Bearer abc123"),
            "authorization 不能出现:{entry}"
        );
        assert!(!entry.contains("t-12345"), "token 不能出现:{entry}");
        assert!(!entry.contains("p@ss"), "password 不能出现:{entry}");
        // 必须保留 [REDACTED] 标记 + safe field
        assert!(entry.contains("api_key=[REDACTED]"));
        assert!(entry.contains("authorization=[REDACTED]"));
        assert!(entry.contains("session_token=[REDACTED]"));
        assert!(entry.contains("user_password=[REDACTED]"));
        assert!(entry.contains("safe_field=visible"));
    }

    /// H1 + level filter:DEBUG / TRACE 被 LevelFilter::INFO cap,不落 telemetry
    #[test]
    fn debug_and_trace_levels_are_filtered_out() {
        init_global_subscriber();
        tracing::debug!(target: "tb_filter_dbg_3f9c2a", "should be dropped");
        tracing::trace!(target: "tb_filter_trc_3f9c2a", "should be dropped");
        assert!(
            find_log_with_target("tb_filter_dbg_3f9c2a").is_none(),
            "DEBUG 必须被 LevelFilter::INFO 过滤掉"
        );
        assert!(
            find_log_with_target("tb_filter_trc_3f9c2a").is_none(),
            "TRACE 必须被 LevelFilter::INFO 过滤掉"
        );
    }

    /// 重复 init 不 panic + 第一次注册的 layer 仍 work(防回归 try_init 改成 init 致 panic)
    #[test]
    fn double_init_is_idempotent() {
        init_global_subscriber();
        init_global_subscriber(); // 不应 panic
        tracing::warn!(target: "tb_double_3f9c2a", "after second init");
        assert!(find_log_with_target("tb_double_3f9c2a").is_some());
    }

    /// pr-test-analyzer #4 e2e:healing.rs::heal_builtin_provider_fields 真实
    /// 触发 → bridge → telemetry.logs。证明本 PR 解决的实际场景(C1 假修复根治)
    /// 真的 work,而不只是合成 emit
    #[test]
    fn healing_apiformat_warn_lands_in_telemetry_via_bridge() {
        use codex_app_transfer_registry::heal_builtin_provider_fields;
        use serde_json::json;

        init_global_subscriber();
        // 构造一个 baseUrl 命中 builtin preset 但 apiFormat 跟 preset 不同的 provider
        // → healing 必触发 apiFormat 强制覆盖 + warn(C1 修复点)
        let mut cfg = json!({
            "providers": [{
                "id": "x-direct-instance",
                "name": "X Direct",
                "baseUrl": "https://api.deepseek.com",
                "apiFormat": "responses"
            }]
        });
        let changed = heal_builtin_provider_fields(&mut cfg);
        assert!(
            changed,
            "healing 必触发(用 builtin baseUrl + 不同 apiFormat)"
        );

        // 验证 healing 真把 apiFormat 改回 openai_chat
        assert_eq!(
            cfg["providers"][0]["apiFormat"].as_str(),
            Some("openai_chat")
        );

        // **关键 e2e 断言**:bridge 必把 healing 的 tracing::warn 转发到 telemetry.logs
        let logs = drain_logs();
        let healing_warn = logs.iter().find(|line| {
            line.starts_with("WARN")
                && line.contains("[codex_app_transfer_registry::healing]")
                && line.contains("apiFormat")
        });
        assert!(
            healing_warn.is_some(),
            "healing apiFormat 强制覆盖 warn 必须经 bridge 落到 telemetry.logs(C1 修复闭环);\n实际 logs:\n{}",
            logs.join("\n")
        );
        // 防 secret 泄露 — base_url field 不能被 redact(它不是 sensitive),但 user_value 也是公开值不该 redact
        let entry = healing_warn.unwrap();
        assert!(
            entry.contains("base_url=") || entry.contains("base_url ="),
            "healing warn 的 base_url field 必须保留(非 sensitive):{entry}"
        );
    }
}
