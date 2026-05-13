# CC Desktop Switch v1.0.24

## English

This patch release focuses on small issue fixes without changing the main local gateway flow.

- Added packaged SOCKS proxy support so `socks5://` upstream proxies no longer fail because `socksio` is missing.
- Added a desktop health warning for Claude Desktop errors such as `Host Claude Code binary not available`.
- Added a Xiaomi MiMo 1M context option that marks mapped `mimo-v2.5-pro` Claude routes with `supports1m`.
- Improved the macOS menu bar icon so it adapts to light and dark menu bars without a white background.
- Added a runtime Dock-hiding safeguard for the macOS app bundle.
- Clarified that third-party providers use the local gateway and require CC Desktop Switch to keep running in the background.
- Clarified that Claude Desktop Connectors/Skills are account or enterprise extension features and are not fully replaced by the local gateway.

## 简体中文

本次是小范围 issue 修复，不改变本机 gateway 主流程。

- 补齐打包版 SOCKS 上游代理支持，使用 `socks5://` 时不再因为缺少 `socksio` 直接失败。
- 增加 Claude Desktop `Host Claude Code binary not available` 的健康检查提示。
- 为 Xiaomi MiMo 增加 1M 上下文选项，只给已映射到 `mimo-v2.5-pro` 的 Claude route 写入 `supports1m`。
- 优化 macOS 状态栏图标，深色/浅色菜单栏下不再显示白色背景。
- 为 macOS 应用增加运行时 Dock 隐藏兜底。
- 明确第三方 provider 通过本机 gateway 工作，使用时需要让 CC Desktop Switch 在后台运行。
- 明确 Claude Desktop Connectors/Skills 属于账号或企业扩展能力，本机 gateway 不能完整替代。
