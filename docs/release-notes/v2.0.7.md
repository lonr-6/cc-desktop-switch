# Codex App Transfer v2.0.7

> 本版本主线:**让 DeepSeek 在 Codex CLI 真实工作流中可用**。修掉之前阻断 DeepSeek 端到端的 `response_format: json_schema` 兼容问题、还原 Codex CLI 续轮空 messages 的容忍度,并修复两条 Windows 端 UX 卡点(重启 Codex 时多次 flash 终端、托盘右键菜单选项点不动)。

## 中文

### DeepSeek `response_format: json_schema` 自动降级为 `json_object`

[DeepSeek 官方 JSON Mode](https://api-docs.deepseek.com/guides/json_mode) 仅支持 `{"type": "json_object"}`,收到 `{"type": "json_schema", ...}` 直接返回 `400 This response_format type is unavailable now`。Codex CLI 在 action approval 子调用里会带 `text.format = json_schema` 强约束输出格式,前几版 proxy 把这个字段原样转译成 `response_format: json_schema` 上游,DeepSeek 整 body deserialize 失败 → Codex CLI 拿不到裁决结果 → 自动化审批连续超时 → 最终告诉用户"请手动批准"。

本版本在 `build_response_format_for_provider()` 里加 provider-aware 降级:命中黑名单(实测仅 `deepseek`)时,把 `{"type":"json_schema", json_schema:{...}}` 降级为 `{"type":"json_object"}`。Codex CLI 的 system prompt 通常已经写明 required keys(实测 DeepSeek `json_object` 模式下,模型按 prompt 指示输出 schema 一致 JSON,所有 required key 全部到位)。判定优先级:

1. `provider.modelCapabilities[<default_model>].supports_json_schema_response_format` 显式 true / false 优先(给未来能力变更预留口子)
2. fallback 名单只放经实测确认拒绝的上游:DeepSeek
3. 其他默认放行 — Kimi 月之暗面 / Kimi Code / MiMo (Token Plan) / MiMo (Pay for Token) 实测均支持完整 OpenAI `json_schema` 语义,**不**进入降级名单

不复用同一份"视觉剥离"白名单(`xiaomi-mimo` 不支持视觉但支持 json_schema),两个 capability 各管一套。

### 空 messages 透传

v2.0.5 加了"input 空且 session_cache 未命中 → 代理直接 BadRequest"的 fail-fast 守卫,初衷是避免向上游打没有 messages 的 body 浪费扣费。**实测在 Kimi 路径上持续阻塞**:Codex CLI 对 Kimi 的请求会频繁带 `previous_response_id` + `input=[]`(心跳 / 状态查询类),proxy 自身的 BadRequest 比上游 4xx 更难触发 Codex CLI 内置重试,用户感知就是"代理请求失败:adapter: bad request: messages 为空"反复出现。

本版本回滚为透传:messages 空时仍写空数组给上游,让上游回它的原生 4xx(Kimi 通常会自带消息),Codex CLI 按它自己的重试策略覆盖。视觉剥离白名单不变,继续保留。

### Windows 重启 Codex 不再 flash 终端

借鉴 [codex-account-switch](https://github.com/Cmochance/Codex_Account_Switch) 的 `hide_console_window`,给所有 Windows 上的 `Command::spawn` / `output` / `status` 调用统一加 `CREATE_NO_WINDOW = 0x08000000` flag。一次重启原本会 flash 大约 30 次终端黑框(轮询 4s+2s 探活 × 200ms 间隔的 `tasklist` 调用 + `taskkill` + `explorer.exe`),现在 0 次。

helper 用 `#[cfg(target_os = "windows")]` 守卫,non-Windows 是 no-op,不影响 macOS / Linux 行为。

### Windows 托盘右键菜单修复

`on_tray_icon_event` 之前对**每个事件**(包括右键 click)都立刻 `refresh_tray_menu` 重建 + 替换菜单引用。Windows 平台正在呈现菜单时被替换 → 菜单引用失效 → 选项点不动 / 显示异常 → 用户退不掉 app。

本版本去掉 tray event 里的自动 refresh,菜单只在 `handle_tray_menu` 切 provider 后重建(那是真正会变内容的时机)。其他事件(左键开窗 / 悬停 / 双击)菜单内容不变,无需重建。**副作用**:用户在 UI 加新 provider 后要等下次 provider 切换才会出现在 tray 子菜单里 — 后续可加定时刷新解决。

### 顺带

- DeepSeek 内置预设 `baseUrl` 改回官方推荐 `https://api.deepseek.com`(无 `/v1`)。DeepSeek docs 原话:"Out of compatibility with OpenAI, you can also use https://api.deepseek.com/v1 as the base_url. But note that, the v1 here has NOTHING TO DO with the model's version." 两条都能工作,这次换回去和官方文档对齐。
- proxy 4xx/5xx 上游错误诊断 dump:失败时把请求体 + 响应体片段写到日志面板,辅助后续排查。本版本附带的诊断日志:`上游错误诊断 <code> <url>` + `→ request body (...)` + `← response body (...)`。
- proxy 整流耗时打点:每条成功的 SSE 流被 Drop 时输出一行 `上游耗时 <code> <url> TTFB=Xs total=Ys bytes=N`,便于区分单次 reasoning 慢和多轮工具循环放大。

## English

> Theme: **make DeepSeek work in real Codex CLI workflows**. Fixes the `response_format: json_schema` incompatibility that previously blocked DeepSeek from completing approval/judgment subcalls, restores tolerance for empty-messages continuation requests, and patches two Windows UX issues (terminal flashing during Codex restart, tray right-click menu unresponsive).

### Auto-downgrade `response_format: json_schema` for DeepSeek

[DeepSeek's JSON Mode](https://api-docs.deepseek.com/guides/json_mode) only supports `{"type": "json_object"}` and rejects `{"type": "json_schema", ...}` with `400 This response_format type is unavailable now`. Codex CLI's action-approval subcall sends `text.format = json_schema` to constrain output, which earlier versions of this proxy forwarded as `response_format: json_schema` — DeepSeek's whole-body deserialize failed, Codex CLI never received the verdict, automation approval timed out, and the agent eventually asked the user to approve manually.

This release adds a provider-aware downgrade in `build_response_format_for_provider()`: when the upstream is on the blacklist (currently only `deepseek`), `{"type":"json_schema", json_schema:{...}}` is rewritten to `{"type":"json_object"}`. Codex CLI's system prompt typically already lists the required keys, and DeepSeek under `json_object` mode reliably produces JSON matching that schema (verified end-to-end on real API: all required keys present, valid JSON content). Decision order:

1. `provider.modelCapabilities[<default_model>].supports_json_schema_response_format = true | false` takes precedence (escape hatch for future capability changes).
2. Fallback blacklist contains only providers verified to reject `json_schema`: DeepSeek.
3. Everyone else passes through — Kimi Moonshot / Kimi Code / MiMo (Token Plan) / MiMo (Pay for Token) all verified to support the full OpenAI `json_schema` semantics, none on the downgrade list.

This is a separate capability from the vision-strip whitelist (`xiaomi-mimo` doesn't support vision but does support `json_schema`), so each capability has its own list.

### Empty-messages passthrough

v2.0.5 added a "fail fast when input is empty and session_cache missed" guard, intended to avoid wasting upstream tokens on bodies without `messages`. In practice this **persistently blocked Kimi**: Codex CLI sends frequent `previous_response_id` + `input=[]` requests (heartbeats / status checks), and a proxy-side BadRequest is harder for Codex CLI's built-in retry to recover from than an upstream 4xx. From the user side: "代理请求失败: adapter: bad request: messages 为空" kept repeating.

This release reverts to passthrough — when `messages` ends up empty, the empty array is still written and forwarded; the upstream returns its native 4xx and Codex CLI follows its own retry strategy. The vision-strip whitelist is unchanged.

### Windows: terminal no longer flashes during Codex restart

Borrowed `hide_console_window` from [codex-account-switch](https://github.com/Cmochance/Codex_Account_Switch). All Windows `Command::spawn` / `output` / `status` calls now apply `CREATE_NO_WINDOW = 0x08000000`. A restart that used to flash ~30 console boxes (the polling loop calls `tasklist` every 200ms during 4s+2s of liveness checks, plus `taskkill` and `explorer.exe`) now flashes none.

Helper is `#[cfg(target_os = "windows")]` gated; macOS / Linux paths are unaffected.

### Windows: tray right-click menu fixed

`on_tray_icon_event` previously called `refresh_tray_menu` on **every** event, including right-click. Windows replaces the menu reference while the menu is being presented, so the dropdown either showed stale entries or its options stopped responding — and the user couldn't quit the app cleanly.

The auto-refresh on tray events is removed. The menu now rebuilds only after a provider switch in `handle_tray_menu` (the only event that actually changes content). **Side effect**: providers added through the UI won't appear in the tray submenu until the next provider switch — a timer-based refresh can address this later.

### Misc

- DeepSeek built-in preset `baseUrl` reverted to the officially recommended `https://api.deepseek.com` (no `/v1`). Per DeepSeek docs: "the v1 here has NOTHING TO DO with the model's version, only kept for OpenAI SDK compatibility." Both work; matching the official docs reduces onboarding confusion.
- Upstream 4xx/5xx error diagnostic dump: failures now log the request body + response body fragments under `上游错误诊断 <code> <url>`, easier post-mortem.
- Stream-level timing instrumentation: every successful SSE stream emits a `上游耗时 <code> <url> TTFB=Xs total=Ys bytes=N` line on Drop, useful for separating single-turn reasoning latency from multi-turn tool-loop amplification.
