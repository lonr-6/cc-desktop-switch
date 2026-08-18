# CC Desktop Switch v1.0.26

## English

This patch release hardens provider editing, upstream diagnostics, uninstall cleanup, and the cross-platform release gate.

- Fixed provider model mappings with legacy aliases being restored after the user explicitly removed Opus, Sonnet, or Haiku slots.
- Tightened DeepSeek Max compatibility detection. Authentication, URL, model, context-length, and unrelated parameter errors now keep their original upstream diagnosis instead of being mislabeled as “Max is unsupported.”
- Fixed lowercase and canonical OpenAI-compatible preset metadata so presets are routed through the existing API-format normalizer.
- On Windows, a normal uninstall now removes only Claude Desktop policy values marked with `ccds_managed=true`. Silent in-place upgrades preserve the active provider configuration through `/UPGRADE`.
- Added Ubuntu, Windows, and macOS test coverage, frontend JavaScript syntax checks, and NSIS installer compilation.
- Updated GitHub Actions to the current Node 24 action generations and documented the boundaries for Claude account features and GitHub Copilot BYOK.

### Upgrade notes

No provider migration is required. After upgrading, re-apply the active provider to Claude Desktop and fully restart Claude Desktop. Users uninstalling v1.0.25 or earlier should use **Clear Desktop configuration** before removing the old version.

## 简体中文

本次补丁版重点加固 provider 编辑、上游错误诊断、卸载清理和跨平台发布门禁。

- 修复带旧别名的模型映射在用户明确删除 Opus、Sonnet 或 Haiku 槽位后又自动恢复的问题。
- 收紧 DeepSeek Max 兼容错误判断。鉴权、URL、模型、上下文长度和其他参数错误会保留真实上游诊断，不再被误报为“模型不支持 Max”。
- 修复小写及规范 OpenAI 兼容预设元数据的处理，统一交给现有 API 格式归一化逻辑。
- Windows 正常卸载时只清理由 `ccds_managed=true` 标记、确由 CCDS 写入的 Claude Desktop 策略值；静默覆盖升级通过 `/UPGRADE` 保留当前 provider 配置。
- 新增 Ubuntu、Windows、macOS 三平台测试、前端 JavaScript 语法检查和 NSIS 安装器编译。
- GitHub Actions 升级到当前 Node 24 action 代际，并补充 Claude 账号功能和 GitHub Copilot BYOK 的产品边界说明。

### 升级说明

无需迁移 provider。升级后请重新对当前 provider 执行“一键应用到 Claude 桌面版”，并完整重启 Claude Desktop。卸载 v1.0.25 及更早版本前，建议先执行“清除桌面版配置”。
