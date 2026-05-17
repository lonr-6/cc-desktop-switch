# Protocol Unification RFC (Phase 4 Final)

> Status: Completed on 2026-05-11 (no behavior change).

## Goal

在不改变线上行为的前提下，把当前 `adapters` 目录从“按协议分块但仍有共享逻辑散落”推进到“core + mapper”结构：

- `core` 负责协议无关生命周期（输入归一化、会话恢复、SSE 事件输出、compact 包装）。
- `mapper` 仅负责 provider/protocol 特有的 request/response 映射。
- `Adapter` 层仅保留编排（调用 core 与 mapper），减少分支和重复实现。

> 非目标：本阶段不引入新的 provider 行为，不调整 wire 语义，不改变现有测试断言含义。

## Target Module Tree (Phase 4 End State)

```text
crates/adapters/src/
  core/
    mod.rs
    routes.rs                  # 本地 responses/messages/compact 路由规则
    input.rs                   # previous_response_id + tool_call_cache 恢复
    events.rs                  # Responses SSE 事件拼装
  mapper/
    mod.rs                     # RequestMapper / ResponseMapper trait
    chat.rs                    # responses adapter mapper
    gemini_native.rs           # Gemini request/response mapper
    cloud_code.rs              # gemini_cli + antigravity flavor mapper
  responses/mod.rs             # 仅编排（调用 ChatResponsesMapper）
  gemini_native/mod.rs         # 仅编排（调用 GeminiNativeMapper）
  gemini_cli/mod.rs            # 仅编排（调用 CloudCodeMapper）
  registry.rs
  types.rs
```

## Trait Sketch (Implemented)

```rust
pub(crate) trait RequestMapper {
    fn map_request(
        &self,
        client_path: &str,
        body: Bytes,
        provider: &Provider,
    ) -> Result<RequestPlan, AdapterError>;
}

pub(crate) trait ResponseMapper {
    fn map_response(
        &self,
        upstream_status: StatusCode,
        upstream_headers: HeaderMap,
        upstream_stream: ByteStream,
        provider: &Provider,
        request_plan: &RequestPlan,
    ) -> Result<ResponsePlan, AdapterError>;
}
```

说明：

- Phase 4 中已按上面的 trait 落地并完成 adapter 接线。
- `Adapter` 保持外层统一入口，mapper 负责 request/response 映射。

## Phase Plan

### Phase 4.1 (Structural Move, Zero Semantics Change)

- 新建 `core/*` 与 `mapper/*` 目录。
- 将已收敛的 `routes`/`events`/`input` 迁移到 `core` 命名空间（先 `pub use` 兼容）。
- 将 `gemini_cli/request.rs` 中 flavor 兼容函数迁移到 `mapper/cloud_code`。

#### Phase 4.1 progress

- [x] 新建 `core` 命名空间并落地 `core/{routes,events,input}.rs`
- [x] 原 `routes.rs`、`responses/{events,input}.rs` 改为 shim re-export
- [x] 新建 `mapper/cloud_code.rs` 并迁移 cloud-code flavor 兼容函数
- [x] `gemini_native` 与 `registry/responses` 调用点切换到 `core::*` 命名空间
- [x] adapter 目录重整不执行（采用 `*/mod.rs` 形态，保持现有目录稳定）

### Phase 4.2 (Adapter Orchestration Narrowing)

- `responses` / `gemini_native` / `gemini_cli` adapter 改为“薄编排”：
  - 解析入站 body
  - 调 request mapper
  - 填 `RequestPlan`
  - 调 response mapper
- provider-specific 分支从 adapter 继续下沉到 mapper 模块。

#### Phase 4.2 progress

- [x] `gemini_cli` adapter 进一步瘦身：
  - `project_id` 解析下沉到 `mapper/cloud_code::resolve_cloud_code_project_id`
  - cloud-code upstream path 选择下沉到 `mapper/cloud_code::cloud_code_upstream_path`
  - cloud-code 响应流转换下沉到 `mapper/cloud_code::transform_cloud_code_response_stream`
- [x] `responses` adapter 的 provider 特化判断下沉：
  - `provider_needs_think_tag_split` 迁移到 `mapper/chat.rs`
- [x] `gemini_native` 响应侧编排下沉：
  - `transform_response_stream` 主分支迁移到
    `mapper/gemini_native::transform_gemini_native_response_stream`
- [x] `gemini_native` 请求侧编排下沉：
  - `prepare_request` 主分支迁移到
    `mapper/gemini_native::prepare_gemini_native_request`
- [x] `responses` adapter 请求/响应编排下沉：
  - `prepare_request`/`transform_response_stream` 主分支迁移到
    `mapper/chat::{prepare_responses_request, transform_responses_response_stream}`
- [x] `gemini_cli` 请求侧编排下沉：
  - `prepare_request` 主分支迁移到
    `mapper/cloud_code::prepare_cloud_code_request`
- [x] 其余 adapter 编排收口（Phase 4.2 完成）

### Phase 4.3 (Trait Wiring + Compatibility Freeze)

- 在内部引入 `RequestMapper` / `ResponseMapper` trait 并接线。
- 旧调用点保留 shim（`pub use` + wrapper）一段时间，保证 PR 可小步回滚。
- 补充“行为等价”回归测试集（快照 + 路径矩阵 + 关键 provider 用例）。

#### Phase 4.3 progress

- [x] 在 `mapper/mod.rs` 引入统一 trait：
  - `RequestMapper::map_request`
  - `ResponseMapper::map_response`
- [x] 三个 mapper 提供 ZST 实现并接线：
  - `mapper/chat::ChatResponsesMapper`
  - `mapper/gemini_native::GeminiNativeMapper`
  - `mapper/cloud_code::CloudCodeMapper`
- [x] 三个 adapter 改为调用 mapper trait 实现：
  - `responses::ResponsesAdapter`
  - `gemini_native::GeminiNativeAdapter`
  - `gemini_cli::GeminiCliAdapter`
- [x] `responses` 内部 shim 最小化：
  - `responses/converter.rs` 改为直连 `core::events`
  - `responses/request.rs` 改为直连 `core::input`
  - 删除 `responses/events.rs`、`responses/input.rs` shim 文件
- [x] `gemini_cli` 测试侧 shim 最小化（cloud_code helper）：
  - `gemini_cli/mod.rs` 测试改为直连 `mapper/cloud_code`
  - `gemini_cli/request.rs` 删除仅测试用途的 `pub use` re-export
- [x] `cloud_code` 生产链路去耦：
  - `mapper/cloud_code` 不再依赖 `gemini_cli/request` helper
  - envelope/antigravity 逻辑内聚到 `mapper/cloud_code`
- [x] `gemini_cli/request` 重复实现收敛：
  - 删除与 `mapper/cloud_code` 重复的 envelope/antigravity/hash/uuid 生产实现
  - 保留兼容包装函数（委托到 mapper）与回归测试
- [x] 覆盖补齐 + 文档对齐：
  - `mapper/cloud_code.rs` 增加 UUID/SHA/envelope 内部回归测试
  - RFC target tree / file list 改为当前真实落地结构
- [x] compatibility shim 进一步清理（Phase 4.3 收尾完成）

## Per-Phase File Change List

### Phase 4.1 file list

- `crates/adapters/src/core/{mod,routes,input,events}.rs`
- `crates/adapters/src/mapper/cloud_code.rs`
- `crates/adapters/src/lib.rs`
- `crates/adapters/src/registry.rs`
- `crates/adapters/src/responses/{events,input}.rs`（后续已删除 shim）

### Phase 4.2 file list

- `crates/adapters/src/{responses/mod.rs,gemini_native/mod.rs,gemini_cli/mod.rs}`
- `crates/adapters/src/mapper/{chat,gemini_native,cloud_code}.rs`
- `crates/adapters/src/types.rs`

### Phase 4.3 file list

- `crates/adapters/src/mapper/mod.rs`
- `crates/adapters/src/mapper/{chat,gemini_native,cloud_code}.rs`
- `crates/adapters/src/{responses/mod.rs,gemini_native/mod.rs,gemini_cli/mod.rs}`
- `crates/adapters/src/responses/{converter,request}.rs`
- `crates/adapters/src/mapper/cloud_code.rs`（补齐内部回归测试）

## Validation Matrix

- `cargo fmt --all`
- `cargo test -p codex-app-transfer-adapters`
- 路由回归：`/responses`、`/messages`、`/responses/compact`（含 legacy 前缀）
- 连续会话回归：`previous_response_id` 命中/miss/空输入
- Gemini 双路径回归：`gemini_native` 与 `gemini_cli`（含 antigravity flavor）
- 错误语义回归：`previous_response_not_found`、OAuth 401 结构化错误体

## Phase 4 Outcome

- 共享逻辑已集中到 `core`（`routes/input/events`）。
- provider/protocol 差异已集中到 `mapper`（`chat/gemini_native/cloud_code`）。
- `responses` / `gemini_native` / `gemini_cli` adapter 已收敛为薄编排层。
- `RequestMapper` / `ResponseMapper` trait 已落地并在三条主链路完成接线。
- 兼容 shim 已最小化到必要 wrapper；重复生产实现已去重。

## Phase 5 Candidates

- [x] 移除 `gemini_cli/request.rs` 兼容 wrapper（确认无外部引用后已删除）。
- [x] 为 mapper trait 增加更明确的契约测试（跨 mapper 的共用行为断言）：
  - 新增 `mapper/mod.rs::contract_tests`
  - 覆盖 `ChatResponsesMapper` / `GeminiNativeMapper` / `CloudCodeMapper`
  - 断言共性：`map_request` 产物结构约束（path/body/original request）
  - 断言共性：`map_response` 成功路径 `Content-Type: text/event-stream`
- [x] 评估把 `openai_chat` / passthrough 路径纳入同一 mapper 接口（结论：暂不接入）。

### Phase 5.3 Evaluation Notes (`openai_chat` / `responses_passthrough`)

- **现状**：
  - `openai_chat` 仅做路径归一化 + body 透传；响应走 trait 默认 passthrough。
  - `responses_passthrough` 仅做本地路径 rewrite + body 透传；响应同样 passthrough。
- **收益评估**：
  - 若强行接入 mapper，可获得“接口形态一致性”，但几乎不带来行为复用增益。
  - 现有两条路径逻辑非常薄，且已稳定；迁入 mapper 的维护收益有限。
- **风险/成本**：
  - 增加一层 mapper 包装后，调用链更长，排查路径问题时认知负担上升。
  - 需额外维护两套“几乎空实现”的 mapper 与对应测试夹具，性价比偏低。
- **结论**：
  - 维持现状（`openai_chat.rs` / `passthrough.rs` 直接实现 `Adapter`）更合适。
  - 后续仅在这两条路径出现可复用的 provider-specific 逻辑时再评估迁入 mapper。

## Final Decision Summary

- Phase 4 目标已完成：`adapters` 架构收敛为 `core + mapper + thin adapters`。
- 共享协议生命周期逻辑统一进入 `core`，provider/protocol 差异统一进入 `mapper`。
- 三条主链路（`responses` / `gemini_native` / `gemini_cli`）已完成 mapper trait 接线并通过回归。
- 兼容层已最小化：删除 `gemini_cli/request.rs` wrapper，重复生产实现去重为单一来源。
- `openai_chat` / `responses_passthrough` 暂不纳入 mapper；待出现明确复用收益再评估迁移。

## Risks

1. 模块搬迁导致调用路径变更，出现漏改或循环依赖。
2. shim 期间存在双入口，若测试覆盖不全可能出现行为分叉。
3. mapper 下沉时无意触发 provider-specific 策略顺序变化。

## Safeguards

1. 每个子阶段只做一种类型改动（先搬迁、再编排、最后 trait 接线）。
2. 关键行为使用 golden tests 固化（请求体/路径/事件序列）。
3. 所有迁移 PR 保持“小而可回滚”，禁止跨阶段混改。

## Rollback Strategy

1. 以阶段为单位回滚（4.3 -> 4.2 -> 4.1）。
2. 每个阶段保留兼容 shim，失败时可一键回退到旧入口。
3. 若线上出现协议偏差，优先回滚 mapper 接线变更，保留 core 抽取成果。

