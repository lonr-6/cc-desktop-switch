# Protocol Unification RFC (Draft)

## Background

当前 `proxy -> adapters` 主链路已经统一在 `forward_handler + Adapter trait`，但 `responses` 与 `gemini_native` 仍在请求归一化、Responses SSE 事件构造上各维护一套实现。随着 `previous_response_id`、autocompact、tool call 语义不断扩展，双轨演进会带来较高回归风险。

本 RFC 目标是把“上游/下游公共生命周期”上提为共享层，仅保留“中间协议映射”差异，降低新增协议与维护成本。

## Target Module Tree

```text
crates/adapters/src/
  types.rs                         # Adapter/RequestPlan/ResponsePlan 抽象
  responses/
    mod.rs
    compact.rs                     # compact 共享能力
    session.rs                     # ResponseSessionCache 共享能力
    tool_call_cache.rs             # 工具调用恢复共享能力
    events.rs                      # [Phase 1] Responses SSE 事件基础能力(新增)
    converter.rs                   # chat -> responses (复用 events)
  gemini_native/
    request.rs                     # gemini mapper (输入侧)
    response.rs                    # gemini -> responses (复用 events)
  gemini_cli/
    request.rs                     # cloud-code mapper
    response.rs                    # cloud-code SSE unwrap + responses event
```

后续目标是逐步形成更明确的 “core + mapper” 结构：

```text
responses_core/
  input/
  events/
  session/
  compact/
provider_mapper/
  chat/
  gemini_native/
  gemini_cli/
```

## Trait Sketch

```rust
/// 协议无关的中间表示
pub struct NormalizedChat { /* messages/tools/options... */ }

/// provider 请求映射
pub trait ProviderRequestMapper {
    fn to_provider_wire(
        &self,
        input: &NormalizedChat,
        provider: &Provider,
    ) -> Result<Bytes, AdapterError>;
}

/// provider 响应事件映射(输入是 provider 原生 chunk，输出是 Responses SSE 事件)
pub trait ProviderResponseMapper {
    fn feed(
        &mut self,
        chunk: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), AdapterError>;
    fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), AdapterError>;
}
```

> 说明：本次 Phase 1 不引入新 trait 到代码，只完成事件公共层抽取，确保小步、低风险。

## Phase Plan

### Phase 1 (this PR / 当前先落地)

- 抽取 Responses 事件公共能力：
  - `build_tool_namespace_map()`
  - `emit_sse_event()`
- `responses/converter.rs` 与 `gemini_native/response.rs` 改为复用公共实现。

### Phase 2

- 抽取输入侧共享能力：
  - `previous_response_id` 恢复链路
  - `tool_call_cache` 修复接线
  - 统一 `responses_body_to_normalized_chat` 管道

#### Phase 2 progress

- [x] 抽取 `previous_response_id` 历史恢复为共享模块：
  - `responses/input.rs::merge_messages_with_previous_response`
  - `responses/input.rs::response_id_for_session`
- [x] `responses/request.rs` 与 `gemini_native/request.rs` 接入共享实现
- [x] `tool_call_cache` 修复接线收敛：
  - `gemini_native` 改为复用 `responses_body_to_chat_body_for_provider_with_session(...)`
    的 `messages` 输出，统一使用 `responses/request.rs::repair_tool_call_ids`
    管道
- [x] 统一 `responses_body_to_normalized_chat` 主管道：
  - `gemini_native::responses_body_to_normalized_chat` 改为复用
    `responses_body_to_chat_body_for_provider_with_session(...)` 作为主骨架
  - Gemini 特有差异（tools/tool_choice/extra_body/safety_settings）在骨架上覆盖

### Phase 3

- 收敛 compact + endpoint 规则
- 将 provider 特有差异约束在 mapper 层
- 减少 adapter 内条件分支与重复路径归一化逻辑

#### Phase 3 progress

- [x] 路径归一化规则收敛：
  - 新增 `crates/adapters/src/routes.rs` 统一维护
    `normalize_local_responses_path` / `is_local_responses_route` /
    `is_responses_compact_subpath` / `rewrite_local_path_for_upstream` /
    `redirect_responses_to_chat`
  - `registry.rs` 与 `responses/mod.rs` 改为复用共享路由工具
- [x] compact endpoint 判定规则收敛：
  - 新增 `routes::is_exact_responses_compact_path`
  - `responses/compact.rs::is_compact_path` 改为复用共享规则(仅保留 endpoint 业务语义)
- [x] provider-specific mapper 边界继续收紧(第一步)：
  - `gemini_cli` 的 provider flavor 分流(`GeminiCli`/`Antigravity`)
    与 cloud-code request 兼容逻辑下沉到 `gemini_cli/request.rs`
  - `GeminiCliAdapter::prepare_request` 主流程仅保留编排(读取 project_id /
    inner 转换 / outer wrap / flavor transform),减少 adapter 内部协议细节分支

> 当前进度结论：Phase 3 计划项已全部落地，后续如继续演进，进入下一轮 RFC（面向更细粒度 trait 化与模块边界收紧）。

## Per-Phase File Change List

### Phase 1 file list

- `crates/adapters/src/responses/events.rs` (new)
- `crates/adapters/src/responses/mod.rs`
- `crates/adapters/src/responses/converter.rs`
- `crates/adapters/src/gemini_native/response.rs`

### Phase 2 tentative file list

- `crates/adapters/src/responses/request.rs`
- `crates/adapters/src/gemini_native/request.rs`
- `crates/adapters/src/responses/session.rs`
- `crates/adapters/src/responses/tool_call_cache.rs`

### Phase 3 tentative file list

- `crates/adapters/src/responses/compact.rs`
- `crates/adapters/src/gemini_native/mod.rs`
- `crates/adapters/src/registry.rs`
- `crates/adapters/src/gemini_cli/{request,response}.rs`

## Risk & Rollback Strategy

### Risks

1. SSE 事件序号或事件顺序偏差，导致客户端解析异常。
2. namespace tool map 规则变更导致工具路由退化。
3. 共用 helper 后，某一协议链路依赖的“历史兼容行为”被无意统一掉。

### Safeguards

1. 保持函数签名与调用点行为等价，先做逻辑搬迁，不做语义改写。
2. 保留现有 converter/gemini_native 的集成测试，新增 phase-specific 回归测试。
3. 每阶段独立 PR，禁止跨阶段混改。

### Rollback

1. Phase 1 失败时可单独回滚 `responses/events.rs` 接入点，不影响请求侧逻辑。
2. 回滚顺序：`gemini_native/response` -> `responses/converter` -> `mod.rs` -> 删除 `events.rs`。
3. 通过 tag 对照和 `gh pr revert` 快速恢复到上一稳定态。

