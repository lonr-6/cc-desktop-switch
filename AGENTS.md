# AGENTS.md

## Communication

- 默认用中文沟通；代码、命令、提交信息、标识符使用英文。
- 先给结论，再给原因、下一步和验证方式。
- 不要声称运行过测试、构建或发布，除非真的执行过。
- 如果是推测，明确写“推测”，并说明需要什么证据来确认。

## Project Goal

本 worktree 是 CC Desktop Switch 的 Rust/Tauri 未来主线重构，不是 Python 稳定线的小修分支。

- 新目录：`D:\ccds-build\cc-desktop-switch-rust-mainline`
- 当前目标版本：`v1.1.0-rc1`
- 主架构：Tauri v2 + Rust runtime + Rust/WASM UI
- UI 决策：使用纯 Rust UI。默认目标是 `Leptos + Trunk` 编译到 WASM，由 Tauri WebView 承载。
- Python 稳定线先独立合并/处理社区 PR；Rust 主线只吸收稳定线确认后的行为，不在本 worktree 里修 Python runtime。

“纯 Rust UI”的项目含义：

- 产品 UI、状态、表单、路由、i18n 和交互逻辑用 Rust 写。
- 允许工具链生成 WASM glue 或极薄 bootstrap，但不得新增手写 JS 业务逻辑。
- 不把 UI 改成 React/Vue/Svelte，也不继续扩展旧 `frontend/js/app.js`。

## Must-Read Before Work

每个非简单任务开始前先读：

- `AGENTS.md`
- `project-docs/status.md`
- `project-docs/README.md`
- `PLANS.md`（T2/T3 或跨模块任务必须读）
- 与任务相关的 `project-docs/runbooks/*`
- 与任务相关的 `project-docs/bugs/*` 或 `project-docs/decisions/*`

涉及架构或 UI 时补读：

- `docs/architecture/rust-mainline-architecture.md`
- `docs/product/ui-ux-rust-mainline.md`
- `docs/testing/release-and-regression-gates.md`
- `docs/testing/python-rust-parity-matrix.md`
- `docs/testing/eval-harness.md`

如果任务明显匹配 repo-local skill，优先使用 `.agents/skills/*`：

- `ccds-rust-mainline-task`：Rust/Tauri 主线实现。
- `ccds-issue-triage`：issue、截图、诊断包、社区反馈。
- `ccds-release-gate`：RC、release、签名、`latest.json`。
- `ccds-review-pass`：合并或发布前审查。

## Current Product Decisions

- 本机 gateway 是唯一普通用户主路径：`Claude Desktop -> CC Desktop Switch local gateway -> provider`。
- 直连 provider 不在普通 UI 中出现；最多保留为隐藏高级调试能力。
- Claude Desktop 永远不直接看到原始第三方模型名。
- Desktop 可见模型必须是 Claude-safe route，例如 `claude-deepseek-v4-pro` 或 `claude-kimi-k2-6`。
- 默认只显示当前默认 Provider 的显式映射模型。
- 多 Provider 模型菜单可以作为高级模式设计，但默认关闭，且必须带清晰诊断。
- `Default` 只做表单/配置便利项，不参与任何 runtime fallback，不写入 Claude Desktop 模型菜单；Claude Desktop 发来未映射 route 时必须返回 400。
- 删除普通 UI 中的“显示全部 Provider 模型”；如果未来保留调试能力，只能放在隐藏高级入口，且不得影响 Claude Desktop 模型菜单。
- 删除旧的“实验转发模式”说法，统一叫“本机 gateway”。
- Tauri 桌面版不再需要本地 Admin HTTP API；管理界面通过 Tauri command 调用 Rust。
- “报告问题”是核心功能：复制诊断摘要、导出诊断包、打开 GitHub Issue。

## Architecture Rules

- `ModelCatalog` 是唯一模型真相。Desktop `inferenceModels`、gateway `/v1/models`、`/v1/messages` 映射、1M、Max 能力都必须从它生成。
- `DesktopApplyFlow` 必须是完整事务式流程：
  1. 保存 Provider
  2. 设为默认
  3. 生成模型映射
  4. 启动本机 gateway
  5. 写入 Claude Desktop 配置
  6. 读回校验
  7. 提示重启 Claude Desktop
- 任何一步失败都不能显示“已应用”。
- Windows/macOS platform writer 只负责写入和读回，不负责模型策略。
- Gateway adapter 只负责协议转换和上游通信，不负责 UI 决策。
- Diagnostics/Redaction 单独成模块，不散落在各处。
- Update flow 必须区分下载成功、sha256 校验成功、安装器启动成功。

## Documentation Workflow

本仓库采用轻量 harness 工作流，参考 `D:\app_WMO` 的治理方式并针对本项目裁剪。

- `project-docs/status.md`：当前真相、blocker、下一步，只由主控更新。
- `project-docs/handoff/`：任务卡、阶段总结、执行交接。
- `project-docs/bugs/`：bug、根因、修复策略、回归测试。
- `project-docs/decisions/`：长期产品/架构决策。
- `project-docs/runbooks/`：可重复执行的流程。
- `project-docs/templates/`：任务卡、bug 记录、审查报告模板。
- `PLANS.md`：长任务执行计划索引和计划门。
- `.agents/skills/`：可重复工作流，避免每次靠聊天重新描述步骤。

出现以下情况时必须更新项目文档：

- 修复了一个会复发的 bug。
- 改变了 Desktop 写入、gateway、模型菜单、更新发布、诊断包的长期规则。
- 引入或删除一个用户可见能力。
- 新增 release / 构建 / 验证门禁。
- 子代理产出了后续需要复用的调查结论。

## Codex App / GPT-5.5 Workflow

- 默认把 GPT-5.5 当作强推理主控使用，但仍要执行真实验证，不能用模型自信替代测试。
- 官方建议把复杂任务先计划、重复流程做成 skill；本项目采用 `AGENTS.md + PLANS.md + repo-local skills + eval harness` 的组合。
- 遇到 Tauri、Leptos、Rust crate、Claude Desktop、GitHub Actions 等可能更新的信息，优先查官方文档或当前 GitHub 记录。
- 子代理只在用户明确允许或任务明显需要并行调查时使用；子代理输出必须由主控整合，不直接当最终事实。
- 不在同一个大文件里堆功能；优先通过清晰模块边界降低调试成本。
- 长任务必须维护 `PLANS.md` 或 `project-docs/handoff/*` 计划状态，阶段完成后更新 `project-docs/status.md`。
- 发布前必须跑本地验证和平台验证；不能只靠静态阅读。

## Verification Rules

进入实现后，基础门禁至少包括：

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `trunk build --release` 或对应 UI build 命令
- `cargo tauri build`，按平台执行
- Windows/macOS Desktop 写入读回人工测试
- `latest.json`、sha256、sig、public key、平台资产完整性检查
- `v1.1.0-rc1` 起，Windows x64、macOS arm64、macOS x64 都是发布候选硬门禁

如果某条命令当前不存在，先在文档中标注“待建立”，不要假装已经执行。

## Safety

- 不要修改旧工作树：`D:\cc desktop swtich`。
- 不要修改 Python 稳定 worktree：`D:\ccds-build\cc-desktop-switch-v1.0.18`，除非用户明确要求。
- 不要提交 API key、gateway key、Authorization、cookie、URL token、用户对话内容。
- 不要用 `git add .`；提交前先看 `git status` 和 `git diff --stat`。
- 不执行 destructive Git 操作、force push、hard reset，除非用户明确要求并确认风险。
