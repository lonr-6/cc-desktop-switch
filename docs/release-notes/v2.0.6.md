# Codex App Transfer v2.0.6

> 本版本主线: 把 v2.0.5 stable 真实流量里暴露的几条问题(Kimi 多轮被空 messages 守卫阻塞、Windows 重启 Codex 时多次 flash 终端、Windows 托盘右键菜单选项点不动导致退不了)收敛成一个 point release。

## 中文

### 修复

- **空 messages 守卫回滚**(`crates/adapters/src/responses/request.rs`)
  v2.0.5 加的"input 空且 session_cache miss → 代理直接 BadRequest"实测在 Kimi 对话路径上会持续阻塞:Codex CLI 对 Kimi 的请求频繁带 `previous_response_id` + `input=[]`(心跳/状态查询),proxy 自身的 BadRequest 比上游 4xx 更难触发 Codex CLI 内置重试,持续报错"代理请求失败:adapter: bad request: messages 为空..."。本版本回滚为透传:messages 空时**仍写空数组**给上游,让上游回原生 4xx,Codex CLI 按它自己的重试策略覆盖。视觉剥离白名单(deepseek/xiaomi/mimo/qwen3.6 命中纯文本上游剥 image_url)**仍保留**。
- **Windows 重启 Codex 多次终端 flash**(`src-tauri/src/admin/handlers.rs`)
  借鉴 codex-account-switch `hide_console_window`,给 Windows 上的 `tasklist` / `taskkill` / `explorer` 等所有 spawn 加 `CREATE_NO_WINDOW`(`0x08000000`)flag。一次重启原本会 flash 大约 30 次黑框(轮询 4s+2s 探活 × 200ms 间隔的 tasklist 调用),修复后 0 次。helper 用 `#[cfg(target_os = "windows")]` 守卫,non-Windows 是 no-op,不影响 macOS / Linux。
- **Windows 托盘右键菜单失效**(`src-tauri/src/main.rs`)
  `on_tray_icon_event` 之前每个事件(包括右键 click)都立刻 `refresh_tray_menu` 重建+替换菜单,Windows 在呈现菜单时被替换 → 菜单引用失效 → 选项点不动 / 不显示 → 退不掉 app。本版本去掉 tray event 里的自动 refresh,菜单只在 `handle_tray_menu` 切 provider 后重建(那是真正会变内容的时机)。其他事件(左键开窗 / 悬停 / 双击)菜单不变,没必要重建。副作用:用户在 UI 加新 provider 后要等下次 provider 切换才会出现在 tray 子菜单里。

### 已保留(从 v2.0.5 起)

- TracedStream 整流耗时打点(`上游耗时 200 url TTFB=Xs total=Ys bytes=N`)
- 4xx/5xx 上游错误诊断 dump(请求体 + 响应体片段写日志)
- DeepSeek / MiMo 等纯文本上游视觉剥离(`provider_supports_vision` + `strip_image_blocks_in_place`)
- DeepSeek 内置预设 baseUrl 改回官方推荐 `https://api.deepseek.com`(无 /v1)
- Codex CLI catalog 始终写(让 Kimi/MiMo 也显示真实模型名而非 GPT 内置名)
- 启用按钮即时反馈 + 后台并发刷新

### 撤回(本版本未保留)

- 早期试做的"通用上游 4xx 视觉错误 catch-and-retry"(B 方案)经评估改方案可能显著增加逻辑冲突概率,撤回。仍保留原有视觉剥离白名单机制,效果对当前已知供应商等价。

### 发布和验证边界

- 版本同步到 `src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 的 `2.0.6`。
- Workspace 测试: 247+ 项全绿(adapters 92 / proxy 23+11+2 / tauri 31 / registry 15 / codex_integration 39 / 等)。
- macOS / Windows 代码签名 / notarisation 边界不变。

## English

> Theme: roll v2.0.5 stable's real-world findings (Kimi multi-turn blocked by the empty-messages guard, Windows restart flashing dozens of terminal windows, Windows tray right-click menu unusable so the app can't be exited cleanly) into a stable point release.

### Fixes

- **Empty-messages guard rolled back** (`request.rs`): v2.0.5 fail-fast on `input=[]` + cold session_cache turned out to persistently block Kimi conversations — Codex CLI sends frequent heartbeat-style requests with empty input + `previous_response_id`; the proxy's BadRequest is harder for Codex CLI to recover from than an upstream 4xx. Empty `messages` is now passed through (still as an empty array), letting upstream return its native error and Codex CLI follow its own retry policy. The known-text-only blacklist for vision stripping is preserved.
- **Windows: `tasklist` / `taskkill` / `explorer` no longer flash console windows** (`handlers.rs`): borrowed `hide_console_window` (CREATE_NO_WINDOW = `0x08000000`) from `codex-account-switch`. A restart that previously flashed ~30 black boxes during polling now flashes 0. Helper is `#[cfg(target_os = "windows")]` gated; macOS/Linux unchanged.
- **Windows tray right-click menu fixed** (`main.rs`): the previous `on_tray_icon_event` handler called `refresh_tray_menu` on every event including right-click. Windows replaces the menu reference while presenting it, leaving the menu unclickable and the app unable to quit. Refresh is now triggered only after a provider switch (the only event that actually changes content). Side effect: providers added through the UI won't appear in the tray submenu until the next provider switch.

### Carried over from v2.0.5

- TracedStream timing instrumentation
- 4xx/5xx upstream error diagnostic dump
- Vision stripping for known text-only upstreams (`provider_supports_vision`)
- DeepSeek preset baseUrl reverted to the official `https://api.deepseek.com` (no `/v1`)
- Catalog always written so Kimi/MiMo display real model names
- Enable-button instant feedback + background refresh

### Rolled back (not in this release)

- The earlier exploratory "generic upstream 4xx vision-error catch-and-retry" (Plan B) was withdrawn because it would have meaningfully increased logic-conflict surface. The original vision-strip whitelist remains and covers the known providers.

### Release & validation

- Version bumped to `2.0.6` in both `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
- Workspace tests green: 247+ across adapters / proxy / tauri / registry / codex_integration.
- macOS / Windows signing & notarisation boundaries unchanged.
