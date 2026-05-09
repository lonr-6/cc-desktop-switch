# Project Docs

这是 Rust/Tauri 主线重构的项目事实层。聊天记录不能替代这里的文档。

## 入口

- 当前状态：`project-docs/status.md`
- 执行计划索引：`PLANS.md`
- 架构方案：`docs/architecture/rust-mainline-architecture.md`
- UI/UX 方案：`docs/product/ui-ux-rust-mainline.md`
- 测试和发布门禁：`docs/testing/release-and-regression-gates.md`
- 功能等价矩阵：`docs/testing/python-rust-parity-matrix.md`
- 本地 eval harness：`docs/testing/eval-harness.md`
- 主线执行 runbook：`project-docs/runbooks/rust-mainline-workflow.md`
- 外部标杆吸收：`project-docs/decisions/2026-05-08-oh-my-codex-benchmark-lessons.md`

## 目录约定

- `handoff/`：任务卡、阶段总结、子代理交接。
- `bugs/`：已知 bug、根因、修复策略、回归测试。
- `decisions/`：长期产品和架构决策。
- `runbooks/`：重复执行的流程。
- `templates/`：任务卡、bug 记录、审查报告模板。
- `.agents/skills/`：repo-local Codex skills，记录重复执行的工作流；`catalog.json` 记录 skill 分类和状态。

## 文档更新规则

- `status.md` 保持短，只写当前真相、blocker、下一步和关键入口。
- bug 修复不能只留在聊天里；必须落到 `bugs/`，并写回归测试。
- 长期产品取舍必须落到 `decisions/`。
- 复杂任务先写 task-card，再进入实现。
- 阶段结束后用 handoff 或 stage-summary 收口。

## 工作流来源

本项目采用轻量 harness 思路：

- 用 `AGENTS.md` 固定项目规则。
- 用 repo-local skills 固化重复流程，例如主线实现、issue triage、release gate、review pass。
- 用 `PLANS.md` 管住长任务，避免跨会话漂移。
- 用 `status.md` 维护跨会话当前真相。
- 用 task-card / bugs / decisions 记录可复用上下文。
- 用 eval harness、测试和人工验收作为发布门禁。

公开资料中 Harness AI agents 强调把 agent 步骤和构建、测试、审批放进同一工作流；OpenAI Evals 强调可重复评估；社区 harness 经验强调 `AGENTS.md` 和 progress/status 文档。本项目只吸收这些轻量机制，不引入复杂平台依赖。

## 历史文档边界

`docs/release-notes-*`、旧排查报告和旧教程可以保留历史事实，但不代表 Rust 主线当前规则。Rust 主线当前规则以 `AGENTS.md`、`PLANS.md`、`project-docs/status.md`、`project-docs/decisions/*` 和 `docs/architecture/*` 为准。
