# CC Desktop Switch v1.0.25

## English

This patch release focuses on issue-batch stability for the Python line.

- Added a billing header rectifier for Claude Code through third-party gateways. It removes only the dynamic `x-anthropic-billing-header` system block before forwarding, which can improve prompt-cache stability on compatible relays.
- Added an advanced setting to disable the billing header rectifier when troubleshooting unusual gateways.
- Updated Xiaomi MiMo 1M presets so the checkbox maps Opus, Sonnet, and Haiku slots to `mimo-v2.5-pro` and marks the exposed Claude routes with `supports1m`.
- Kept DeepSeek 1M Haiku mapping intact when applying the preset option.
- Changed the missing Claude Code helper signal on macOS/Windows from a dashboard warning to an info-level diagnostics item. Normal third-party gateway chat is not treated as failed by this helper state.
- Kept diagnostics export as UTF-8 JSON and preserved Chinese diagnostic text without exposing secrets.
- Absorbed provider preset JSON extraction and `CCDS_CONFIG_DIR` support from community PRs after review.

## 简体中文

本次是 Python 稳定线的问题批量修复。

- 新增 Claude Code 第三方 gateway 的 billing header 清理器。转发前只移除动态 `x-anthropic-billing-header` system 块，用于降低兼容中转的 prompt cache 被打散概率。
- 在高级兼容设置里增加开关，排查特殊中转时可以关闭该清理器。
- 更新 Xiaomi MiMo 1M 预设，勾选后会把 Opus、Sonnet、Haiku 槽位映射到 `mimo-v2.5-pro`，并给暴露给 Claude Desktop 的 route 写入 `supports1m`。
- 保持 DeepSeek 1M 预设里的 Haiku 映射不丢失。
- 将 Claude Code helper 缺失从首页警告改为诊断页 info 提示。普通第三方 gateway 聊天不再因为 helper 状态被判定为失败。
- 诊断包继续以 UTF-8 JSON 导出，中文诊断文本不乱码，同时保持密钥脱敏。
- 审查后吸收社区 PR 里的 provider preset JSON 拆分和 `CCDS_CONFIG_DIR` 支持。
