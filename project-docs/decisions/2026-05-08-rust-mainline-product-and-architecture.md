# Decision: Rust Mainline Product and Architecture

Date: 2026-05-08

## Decision

CC Desktop Switch 新主线采用 Rust/Tauri 重构，并切换到纯 Rust UI。

默认技术路线：

- Tauri v2 负责桌面壳、窗口、托盘、单实例、系统集成和打包。
- Rust runtime 负责配置、Provider、模型目录、Desktop 写入、gateway、诊断、更新、发布工具。
- Leptos + Trunk 负责编译 Rust/WASM UI，承载在 Tauri WebView 中。
- 手写 JavaScript 不再承载产品逻辑。

## Why

当前项目的主要风险不在语言本身，而在几个策略分散：

- Desktop 可见模型和 gateway 返回模型不一致。
- 真实第三方模型名可能暴露给 Claude Desktop。
- Desktop 写入成功和读回状态没有统一事务。
- Windows/macOS 平台写入、更新安装、诊断散落在不同模块。
- 旧 UI 按钮太多，普通用户不知道失败发生在哪一步。

Rust 主线要解决的是这些边界问题，而不是简单把 Python 翻译成 Rust。

## Product Decisions Adopted

| Decision | Status | Reason |
| --- | --- | --- |
| 本机 gateway 作为唯一普通用户主路径 | Adopted | 模型映射、诊断、1M、Max、错误处理都可控 |
| 直连模式从普通 UI 删除 | Adopted | 新版 Claude Desktop 校验更严格，直连会绕开 gateway 诊断 |
| Claude Desktop 不直接看到原始第三方模型名 | Adopted | 避免 `configured model is not an Anthropic model` |
| 使用 Claude-safe route，例如 `claude-deepseek-v4-pro` | Adopted | 兼顾校验稳定性和用户可读性 |
| 默认只显示当前默认 Provider 模型 | Adopted | 普通用户最稳，诊断最清楚 |
| 多 Provider 模型菜单 | Deferred Advanced | 可做高级模式，默认关闭 |
| `Default` 只做表单/配置便利项 | Adopted | 不进入模型菜单，也不参与 runtime fallback |
| 删除“显示全部 Provider 模型” | Adopted | 容易暴露未映射模型和造成菜单混乱 |
| “实验转发模式”改为“本机 gateway” | Adopted | 用户口径统一 |
| 删除本地 Admin HTTP API | Adopted for Tauri | Tauri command 已足够，减少本机管理面攻击面 |
| 报告问题/脱敏诊断包 | Adopted | 社区 issue 需要可复现、可脱敏信息 |
| 多套发布脚本统一到 Rust `xtask` | Adopted | 减少 PowerShell/shell/batch 混杂 |

## Route Naming Rule

Claude Desktop 可见模型名必须是稳定 route，而不是真实上游模型。

推荐默认格式：

```text
claude-<provider-slug>-<model-slug>
```

示例：

```text
claude-deepseek-v4-pro -> deepseek-v4-pro
claude-kimi-k2-6 -> kimi-k2.6
claude-zhipu-glm-4-7 -> glm-4.7
claude-qwen3-6-plus -> qwen3.6-plus
```

高级多 Provider 模式允许显示更清楚的 display name：

```text
DeepSeek / claude-deepseek-v4-pro
Kimi / claude-kimi-k2-6
GLM / claude-zhipu-glm-4-7
```

内部日志必须同时记录：

```text
Claude selected: claude-deepseek-v4-pro
Provider: DeepSeek
Upstream model: deepseek-v4-pro
```

## One-Click Apply Contract

一键应用必须是完整流程：

```text
save provider
-> set active provider
-> build model catalog
-> start local gateway
-> write Claude Desktop config
-> read back config
-> compare health
-> ask user to restart Claude Desktop
```

失败处理：

- gateway 未启动：失败，不写“已应用”。
- macOS configLibrary 未写入：失败，不写“已应用”。
- Windows registry 读回不是预期地址：失败，不写“已应用”。
- 1M route 没有 `supports1m`：失败，给出 issue code。
- 未映射 route 被请求：gateway 返回 400，不 fallback `Default`。

## Diagnostics Decision

新增问题报告入口：

- 复制诊断摘要
- 导出诊断包
- 打开 GitHub Issue

诊断包必须包含：

- 系统版本
- CC Desktop Switch 版本
- Claude Desktop 版本，如果可检测
- 当前 Provider 名称
- API format
- Base URL 的域名和路径，不带密钥
- 模型映射槽位
- Claude Desktop 当前写入状态
- expected / actual base URL
- gateway 是否运行
- 最近 100 条 gateway 日志
- 最近一次失败请求错误类型
- 更新安装日志
- Desktop health issue code

诊断包不能包含：

- API Key
- gateway key
- Authorization
- cookie
- 自定义 header 里的密钥
- URL query 里的 token
- 用户真实对话内容，除非用户明确勾选

## Issue Fingerprints

所有核心错误必须有稳定问题指纹，例如：

- `desktop.config_library_missing`
- `desktop.raw_model_names_detected`
- `desktop.stale_base_url`
- `desktop.one_million_not_written`
- `gateway.invalid_upstream_response`
- `gateway.unmapped_model_route`
- `provider.max_not_supported`
- `update.installer_launch_failed`

## References

- Claude 3P configuration: https://claude.com/docs/cowork/3p/configuration
- Tauri Leptos frontend setup: https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/start/frontend/leptos.mdx
- Tauri updater docs: https://github.com/tauri-apps/tauri-docs/blob/v2/src/content/docs/plugin/updater.mdx
- Tauri single instance plugin: https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/single-instance/README.md
- Harness agents: https://developer.harness.io/docs/platform/harness-ai/harness-agents/
- OpenAI Evals: https://github.com/openai/evals
