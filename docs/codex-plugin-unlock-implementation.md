# Codex Plugins 解锁 —— 实现总结（最终完整版）

## 全部四项已完成 ✅

### 1. 前端 UI 面板 ✅

- **Dashboard 状态卡片**：实时显示解锁状态 + 控制按钮
- **Settings 开关**：「自动解锁 Codex Plugins」持久化到 config.json

### 2. 设置持久化 ✅

- `autoUnlockCodexPlugins: false` 默认配置
- 启动时自动读取并决定是否启动守护

### 3. 启动集成 ✅

修改 `desktop.rs`：
- `open_command()` 新增 `extra_args` 参数
- `should_attach_debug_port()` 读取设置，返回调试参数
- `open_codex_app()` 启动时自动附加 `--remote-debugging-port=9222`

**效果**：用户开启设置后，通过 codex-app-transfer 重启 Codex Desktop 时自动附带调试端口，无需手动命令行操作。

### 4. 页面刷新监控 ✅

- 持续 WebSocket 连接
- `tokio::select!` 监听 `Page.loadEventFired`
- 刷新后立即重新注入

---

## 变更文件清单

### Rust 后端（10 个文件）

| 文件 | 变更 |
|------|------|
| `src-tauri/Cargo.toml` | + tokio-tungstenite, futures |
| `src-tauri/src/main.rs` | + mod, 自动启动逻辑 |
| `src-tauri/src/codex_plugin_unlocker.rs` | **新建** ~430 行 |
| `src-tauri/src/admin/handlers/mod.rs` | + plugin_unlock |
| `src-tauri/src/admin/handlers/plugin_unlock.rs` | **新建** |
| `src-tauri/src/admin/mod.rs` | + 路由注册 |
| `src-tauri/src/admin/handlers/settings.rs` | + autoUnlockCodexPlugins 默认值 |
| `src-tauri/src/admin/handlers/desktop.rs` | + open_command extra_args, should_attach_debug_port |

### 前端（3 个文件）

| 文件 | 变更 |
|------|------|
| `frontend/index.html` | + 状态卡片 + 设置开关 |
| `frontend/js/api.js` | + CCAPI.pluginUnlock |
| `frontend/js/app.js` | + 状态刷新 + 事件监听 |

---

## 使用流程

1. 打开 codex-app-transfer
2. 进入 **Settings** → 开启「自动解锁 Codex Plugins」
3. 返回 **Dashboard** → 点击「重启 Codex Desktop」（或应用配置后自动重启）
4. Codex Desktop 会以调试端口启动
5. codex-app-transfer 自动连接 CDP 并注入解锁脚本
6. Dashboard 状态卡片显示「✅ 已解锁」

---

## 编译验证

```bash
cd src-tauri && cargo check
# Finished dev profile ✅ (0 errors, only pre-existing warnings)
```
