# 架构设计与协议添加规则

> 适用范围：`crates/adapters` 及与协议转换相关的转发路径。  
> 当前状态：基于 Phase 5 Anthropic Messages PR 实际代码结构整理。

## 1. 当前架构设计（落地版）

项目当前采用 `core + mapper + thin adapters` 分层：

- `core`：协议无关的共享生命周期逻辑
  - 路由归一化与本地路径判断（`core/routes.rs`）
  - 输入侧会话恢复与历史拼接（`core/input.rs`）
  - Responses SSE 事件拼装（`core/events.rs`）
- `mapper`：协议/供应商相关映射逻辑
  - `chat`：Responses <-> Chat 侧映射
  - `gemini_native`：Gemini native 请求/响应映射
  - `cloud_code`：Gemini CLI / Antigravity flavor 映射
  - `grok_web`：Grok Web 请求 flatten、父响应追踪与 SSE 回写映射
  - `anthropic_messages`：Responses <-> Anthropic Messages 请求/响应映射
  - `mapper/mod.rs`：`RequestMapper` / `ResponseMapper` trait + 契约测试
- `adapter`（薄编排）：
  - `responses/mod.rs`
  - `gemini_native/mod.rs`
  - `gemini_cli/mod.rs`
  - `grok_web/mod.rs`
  - `anthropic_messages/mod.rs`
  - 仅负责调用 mapper/core，不承载复杂 provider-specific 分支
- `openai_chat.rs` / `passthrough.rs`
  - 当前保持直接 `Adapter` 实现（透传路径），不强制纳入 mapper

## 2. 分层职责边界

### 2.1 必须放在 `core` 的逻辑

- 不依赖具体 provider 的通用协议流程
- 跨多个 adapter 共享，且行为应保持一致
- 与“输入恢复 / 事件输出 / 路由归一化”相关的基础设施能力

### 2.2 必须放在 `mapper` 的逻辑

- 明确依赖某类协议 wire 形态或某个 provider 的差异行为
- 例如字段兼容、请求体重排、SSE 解包/转换、flavor 分支

### 2.3 `adapter` 层禁止事项

- 禁止新增复杂 provider-specific 业务分支
- 禁止重复实现 mapper 中已有的生产逻辑
- 禁止跨层绕过：应优先通过 mapper/core 暴露能力

## 3. 协议添加规则（可按需调整）

> 以下规则是当前推荐实践，**可按需调整**。当业务目标、兼容压力或回归成本变化时，可在 RFC 中说明理由后做策略调整。

### 3.1 新增协议前置要求

1. 先写最小 RFC（目标、边界、风险、回滚策略）
2. 明确该协议是：
   - A. 新 mapper（推荐），还是
   - B. 透传型 adapter（仅在确无映射收益时）
3. 明确与现有协议的复用点（core/mapper 可复用能力）

### 3.2 新增协议实现步骤

1. 新增 mapper 文件（如 `mapper/<protocol>.rs`）
2. 实现 `RequestMapper` / `ResponseMapper`（优先）
3. 在对应 adapter 做薄编排接线
4. 在 `registry` 增加路由入口
5. 补齐测试：
   - 单元测试：请求映射、响应映射、错误分支
   - 契约测试：满足 mapper 共性断言
   - 回归测试：关键 provider/路径矩阵

### 3.3 字段与语义处理规则

- 优先“显式转换”，避免“静默丢字段”
- 兼容性降级必须可观测（日志或结构化错误）
- 与上游不兼容时，优先返回可诊断错误，而不是隐式吞错

### 3.4 文档与变更同步规则

- 实现完成后必须同步：
  - `docs/protocol-unification-rfc-phase4.md`（或后续 RFC，例如 Phase 5）
  - 变更清单（涉及文件、测试结果、已知风险）
- 保持“代码结构说明”与“实际目录结构”一致，避免文档漂移

### 3.5 已落地 canonical protocol

当前 registry 中应保持下列 canonical protocol 字面值稳定：

- `openai_chat`：OpenAI Chat-compatible 上游，默认 fallback。
- `responses`：OpenAI Responses 语义；custom direct mode 只允许 `responses` / `openai_responses` 旁路本地代理。
- `gemini_native`：Google AI Studio `generateContent` / `streamGenerateContent`。
- `gemini_cli_oauth`：Google Cloud Code Assist OAuth wire。
- `antigravity_oauth`：Antigravity OAuth flavor，复用 Cloud Code Assist wire。
- `grok_web`：grok.com Web 后端反代 wire。
- `anthropic_messages`：Anthropic `/v1/messages` wire，历史别名 `anthropic` / `claude` / `messages` / `claude_messages` 必须归一或路由到该协议。

新增协议时应优先新增明确 canonical 名称，避免把通用路径名（如 `messages`）作为 canonical，造成与本地兼容 route 混淆。

## 4. 验证与准入门槛

协议相关改动合入前至少满足：

- `cargo fmt --all`
- `cargo test -p codex-app-transfer-adapters`
- 必要时：`cargo check --workspace`
- 新增协议需有对应 mapper/adapter 回归测试，不得只改实现不补测试
- 如果新增协议同时暴露到 provider UI，还需覆盖：
  - provider `apiFormat` 归一化；
  - provider connection test URL/body/header；
  - model list endpoint 推导；
  - direct-mode bypass guard；
  - 前端保存 canonical 值和旧配置别名显示。

## 5. 例外处理机制

若需突破上述规则（例如直通实现、临时 shim、跳过某层抽象）：

1. 在 RFC/PR 中写明理由与时限
2. 标注回收计划（何时移除临时路径）
3. 增加防回归测试，避免例外路径长期失控

---

如需快速判断“某段逻辑该放哪一层”，默认顺序：

1. 先问：是否协议无关、可跨 provider 复用？是 -> `core`
2. 否则问：是否是某协议/provider 差异？是 -> `mapper`
3. `adapter` 只保留编排与接线，不承载复杂变换
