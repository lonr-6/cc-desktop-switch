# Codex Plugins 解锁集成方案 —— 运行时伴侣守护进程

## 目标
通过 codex-app-transfer 启动后自动向 Codex Desktop 注入解锁代码，保证只要 codex-app-transfer 在运行，Plugins 选项卡始终可用。

## 核心原理
1. Codex Desktop 以 `--remote-debugging-port=9222` 启动
2. codex-app-transfer 通过 Chrome DevTools Protocol (CDP) 连接
3. 注入 `setAuthMethod('chatgpt')` 脚本修改 React UI state
4. 监听页面刷新事件，自动重新注入

## 集成架构

```
┌─────────────────────┐     ┌─────────────────────────────┐
│  codex-app-transfer │     │      Codex Desktop          │
│  (Tauri v2 App)     │     │  (Electron, port 9222)      │
│                     │     │                             │
│  ┌───────────────┐  │     │  ┌─────────────────────┐    │
│  │ PluginUnlock  │  │ CDP │  │ WebView Renderer    │    │
│  │   Service     │◄─┼─────┼──►│  ┌───────────────┐  │    │
│  │               │  │WS   │  │  │ AuthProvider  │  │    │
│  │ - detect()    │  │     │  │  │  └─ setAuthMethod│   │
│  │ - connect()   │  │     │  │  └───────────────┘  │    │
│  │ - inject()    │  │     │  │         ▲           │    │
│  │ - monitor()   │  │     │  │  ┌──────┴───────┐   │    │
│  └───────┬───────┘  │     │  │  │   Sidebar    │   │    │
│          │          │     │  │  │  Plugins btn │   │    │
│  ┌───────▼───────┐  │     │  │  └──────────────┘   │    │
│  │  HTTP Handler │  │     │  └─────────────────────┘    │
│  │ /api/desktop/ │  │     └─────────────────────────────┘
│  │  /plugin-unlock│  │
│  └───────────────┘  │
│          ▲          │
│  ┌───────┴───────┐  │
│  │  Frontend UI  │  │
│  │ - 开关设置     │  │
│  │ - 状态显示     │  │
│  │ - 手动触发     │  │
│  └───────────────┘  │
└─────────────────────┘
```

## 文件变更清单

### Rust 后端
1. `src-tauri/Cargo.toml` — 新增 `tokio-tungstenite` 依赖
2. `src-tauri/src/codex_plugin_unlocker.rs` — 核心模块（新建）
3. `src-tauri/src/admin/handlers/plugin_unlock.rs` — HTTP API（新建）
4. `src-tauri/src/admin/handlers/mod.rs` — 注册新 handler
5. `src-tauri/src/admin/mod.rs` — 注册新路由
6. `src-tauri/src/main.rs` — 启动时初始化守护进程

### 前端
7. `frontend/index.html` — 添加 Plugins 解锁状态面板
8. `frontend/js/api.js` — 添加 API 调用函数

## 关键设计决策

### 1. 启动流程
- 用户开启 "自动解锁 Codex Plugins" 设置（默认关闭）
- codex-app-transfer 启动时检查设置
- 如果开启且 Codex Desktop 未运行 → 以调试端口启动 Codex
- 如果开启且 Codex Desktop 已运行但无调试端口 → 提示需要重启

### 2. 注入策略
- 连接 CDP 后立即注入一次
- 启用 `Page` domain，监听 `loadEventFired`
- 每次页面刷新后自动重新注入
- 断开连接时自动重连（指数退避）

### 3. 状态管理
- `disconnected` — Codex Desktop 未运行或无调试端口
- `connecting` — 正在连接 CDP
- `connected` — 已连接，等待注入时机
- `injected` — 注入成功，Plugins 已解锁
- `failed` — 注入失败（显示错误信息）

## 安全风险
1. `--remote-debugging-port` 会暴露 Codex Desktop 的内部状态
2. 建议绑定 `127.0.0.1:9222`（默认行为），不暴露到局域网
3. 设置开关默认关闭，用户主动选择后才启用

## 与现有代码的复用点
- `desktop.rs` 中的 `running_check_command()` — 检测 Codex 进程
- `desktop.rs` 中的启动/重启逻辑 — 附加调试参数
- `settings.rs` — 保存用户开关状态
