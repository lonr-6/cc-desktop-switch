# CC Desktop Switch v1.0.20

<p align="center">
  <a href="#english">English</a> |
  <a href="#simplified-chinese">简体中文</a>
</p>

<a id="english"></a>

## English

This release focuses on the v1.0.20 stability/trust pass: safer local admin access, redacted diagnostics, clearer upstream error reports, and a stricter cross-platform release pipeline.

### Highlights

- **Local gateway wording is now consistent**
  - Quick Start and Usage no longer describe Anthropic-compatible providers as direct-connect by default.
  - Third-party providers are described as `Claude Desktop -> CC Desktop Switch local gateway -> provider`.
  - Troubleshooting now points users to the local gateway logs instead of the old "experimental forwarding mode" wording.

- **Copilot boundary is documented**
  - GitHub Copilot subscriptions are not directly supported as a provider API.
  - If a user brings an OpenAI Chat or Anthropic-compatible endpoint, it is user-provided and used at the user's own risk.
  - Users should verify terms, account risk, API format, base URL, streaming behavior, and tool-call compatibility before relying on such endpoints.

- **Issue reporting is more structured**
  - Added GitHub issue forms for bug reports, provider requests, and questions.
  - The forms ask for OS, Claude Desktop version, CC Desktop Switch version, provider, API format, base URL, screenshots, and a diagnostics package or sanitized diagnostics.

- **Local admin API is protected**
  - The desktop UI now receives a runtime admin token and sends it as `X-CCDS-Admin-Token`.
  - `GET /api/ready` remains public for startup health checks.
  - `/api/app/activate` keeps the existing local activation header so launching the shortcut again can still bring the current window forward.

- **Redacted diagnostics are easier to share**
  - Added diagnostics summary, export, and check endpoints.
  - Diagnostics use the `ccds.diagnostics.v1` format and redact API keys, gateway keys, authorization headers, URL credentials, token-like query parameters, and recent gateway log secrets.

- **OpenAI/new-api relay errors are easier to diagnose**
  - If an upstream relay returns HTML/text instead of JSON, CC Desktop Switch now reports `invalid_upstream_response`.
  - The error includes HTTP status, content type, upstream host, API format, and a redacted response preview.
  - Streaming responses with incompatible content types now return an Anthropic-style SSE error event instead of failing silently.

- **Release publishing now waits for both platforms**
  - GitHub Actions now builds Windows and macOS artifacts separately, then publishes from a final staging directory.
  - `latest.json` is generated only after both `windows-x64` and `macos-arm64` assets are present.
  - Release assets, `latest.json`, hashes, signatures, and the public key are generated in one final publish step.

- **DeepSeek 1M issue #3 is tracked carefully**
  - Current DeepSeek 1M preset behavior keeps Sonnet, Opus, and Default on `deepseek-v4-pro[1m]`.
  - Haiku currently maps to `deepseek-v4-flash` while carrying 1M capability metadata when the option is enabled.
  - This release does not claim that #3 is fixed; it records the current behavior and asks for diagnostics before closing the loop.

### Upgrade Notes

No user configuration migration is required. After upgrading from older versions, re-apply the active provider and fully restart Claude Desktop if your Desktop configuration was written by an older release. If you use a third-party provider, keep CC Desktop Switch running in the background because Claude Desktop talks to the local gateway.

<a id="simplified-chinese"></a>

## 简体中文

本次版本主要收口 v1.0.20 稳定性 / 信任度工作：本机管理 API 防护、脱敏诊断、上游错误诊断，以及更严格的跨平台发布链。

### 主要变化

- **本机 gateway 口径统一**
  - 快速教程和使用说明不再把 Anthropic 兼容 provider 描述成默认直连。
  - 第三方 provider 统一描述为 `Claude Desktop -> CC Desktop Switch 本机 gateway -> provider`。
  - 排障说明不再使用旧的“实验转发模式”说法，改为引导用户查看本机 gateway 日志。

- **补充 Copilot 边界**
  - GitHub Copilot 订阅账号不能直接当作本项目的 provider API 使用。
  - 如果用户自行提供 OpenAI Chat 或 Anthropic 兼容端点，可以按自定义 provider 尝试，但风险由用户自行承担。
  - 使用前需要自行确认端点规则、账号风险、API 格式、Base URL、流式输出和工具调用兼容性。

- **issue 信息收集更结构化**
  - 新增 bug report、provider request、question 三类 GitHub issue form。
  - 模板要求提供 OS、Claude Desktop 版本、CC Desktop Switch 版本、provider、API format、Base URL、截图，以及诊断包或脱敏诊断信息。

- **本机管理 API 增加防护**
  - 桌面 UI 会拿到运行时 admin token，并在请求中发送 `X-CCDS-Admin-Token`。
  - `GET /api/ready` 保持公开，只用于启动探活。
  - `/api/app/activate` 保留旧的本机唤醒 header，确保重复点击快捷方式仍能唤起已有窗口。

- **脱敏诊断更适合发给维护者**
  - 新增诊断摘要、导出和检查接口。
  - 诊断包固定为 `ccds.diagnostics.v1`，会脱敏 API key、gateway key、Authorization、URL 凭据、token 类 query，以及本机 gateway 近期日志里的密钥信息。

- **OpenAI/new-api 中转错误更容易排查**
  - 如果上游中转返回 HTML/text 而不是 JSON，会返回 `invalid_upstream_response`。
  - 错误里会带 HTTP 状态码、content-type、上游 host、API format 和脱敏后的响应摘要。
  - 流式响应如果 content-type 明显不兼容，会返回 Anthropic 风格 SSE error event。

- **发布链要求 Windows 和 macOS 资产齐全**
  - GitHub Actions 会分别构建 Windows 和 macOS，再从最终 staging 目录统一发布。
  - `latest.json` 只会在 `windows-x64` 和 `macos-arm64` 资产都存在后生成。
  - release 资产、`latest.json`、hash、signature、公钥都在最终发布步骤统一生成。

- **谨慎记录 DeepSeek 1M 的 #3 当前行为**
  - 当前 DeepSeek 1M 预设会让 Sonnet、Opus 和 Default 使用 `deepseek-v4-pro[1m]`。
  - Haiku 当前映射到 `deepseek-v4-flash`，开启选项时会携带 1M 能力声明。
  - 本次不声明 #3 已修复，只记录当前行为，并要求补充诊断信息后再闭环。

### 升级说明

不需要迁移用户配置。从旧版本升级后，如果 Claude Desktop 配置来自旧版本，仍建议重新对当前 provider 执行“一键应用到 Claude 桌面版”，然后完整重启 Claude Desktop。使用第三方 provider 时，需要保持 CC Desktop Switch 在后台运行，因为 Claude Desktop 会调用本机 gateway。
