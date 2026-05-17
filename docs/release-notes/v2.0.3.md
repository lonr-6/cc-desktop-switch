# Codex App Transfer v2.0.3

> 本版本主线: 修复 v2.0.2 实测反馈中暴露的版本显示和 Codex App 重启提示问题, 并同步版本号到 2.0.3。本轮不新增供应商逻辑, 只收敛当前 UI 与已恢复的本地接口行为。

## 中文

### 修复

- **设置页版本显示恢复真实版本**: “关于”面板不再硬编码 `1.0.0`, 改为调用 `/api/version` 读取当前 `src-tauri/Cargo.toml` 构建版本。本版本升级后应显示 `2.0.3`。
- **应用配置后的重启提示改为 Codex App 重启确认**: 启用或一键应用 provider 后, 弹窗标题改为“是否立即重启 Codex App？”, 内容改为“已切换供应商并同步模型列表，需要重启 Codex App 才能生效。”。
- **移除“不再提醒”入口**: 旧版复选框和“我已知晓”按钮已移除, 防止用户误跳过后续必要重启提示。
- **新增立即重启动作**: 左侧“取消，稍后重启”仅关闭弹窗; 右侧“立即重启”会调用本地 `/api/desktop/restart-codex-app`, 按平台 best-effort 退出并重新打开外部 Codex App。

### 发布和验证边界

- 版本源已同步到 `src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 的 `2.0.3`。
- 本次仍按非正式打包验证处理, 通过 GitHub Actions 生成 draft 资产后再决定是否发布正式 Release。
- macOS / Windows 代码签名和 notarization 的既有边界不变。

## English

> Theme: fix the version display and Codex App restart prompt found during v2.0.2 testing, then bump the app to 2.0.3. This pass does not add new provider behavior.

### Fixes

- **Settings now shows the real app version**: the About panel no longer hard-codes `1.0.0`; it reads `/api/version`, which is backed by the Cargo package version. After this bump it should display `2.0.3`.
- **The post-apply restart prompt now targets Codex App**: the modal title and body now ask whether to restart Codex App immediately after provider/model sync.
- **Removed the “do not show again” option**: the old checkbox and acknowledgement button were removed so required restart prompts are not silently suppressed.
- **Added a restart action**: “Cancel, restart later” closes the modal, while “Restart now” calls `/api/desktop/restart-codex-app` for a platform best-effort restart of the external Codex App.

### Release boundary

- The version source is now `2.0.3` in both `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`.
- This build remains a non-official packaging rehearsal until the draft assets are reviewed.
- Existing macOS / Windows signing and notarization boundaries are unchanged.
