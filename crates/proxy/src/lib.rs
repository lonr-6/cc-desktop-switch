//! Codex App Transfer · 代理转发主干(Stage 2).
//!
//! 当前实现:
//! - **B1 多 provider 路由**:`StaticResolver` 按 body `model = "<slug>/<real>"`
//!   匹配 provider,失败 fallback 到 `default_provider_id`。
//! - **B2 鉴权改写**:剥掉 incoming `Authorization`,按 `provider.auth_scheme`
//!   注入 `Bearer <api_key>` 或 `X-Api-Key`,再叠 `provider.extra_headers`。
//! - **HTTP/SSE 透传**:body 完整读取 → 必要时改写 model → reqwest 发起 →
//!   响应字节流(`bytes_stream`)灌回 axum,流式 SSE 0 损耗。
//!
//! 未实现(下阶段):provider 协议转换(`crates/adapters`,Stage 3)、
//! OS 集成(`crates/codex_integration`,Stage 2.5)、WebSocket 透传。

pub mod diagnostics;
pub mod fixture;
pub mod forward;
pub mod resolver;
pub mod server;
pub mod telemetry;
pub mod validation;

pub use diagnostics::{feedback_bundle_dir, recent_feedback_bundles};
pub use forward::{forward_handler, ProxyState};
pub use resolver::{
    AuthScheme, ProviderResolver, ResolveError, ResolvedProvider, SharedResolver, StaticResolver,
};
pub use server::build_router;
pub use telemetry::{proxy_log_dir, proxy_telemetry, ProxyLogEntry, ProxyStatsSnapshot};
