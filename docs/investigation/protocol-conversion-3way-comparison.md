# Phase 4 · 协议转换三方比对与优化方案

**比对日期**:2026-05-06
**触发**:Codex CLI ↔ MiMo / DeepSeek / Kimi 真实流量出现"对话结束但回复为空" / 工具循环上游 400 等问题,需复盘当前 Rust 实现相对于改造前 Python 与 litellm 参考实现的覆盖度。

## 一、范围与版本基线

| 实现 | 引用 | 备注 |
|---|---|---|
| Python pre-refactor | `git show 904095d^{tree} -- backend/`(2026-05-05) | 改造前最后一次状态,含 DeepSeek thinking 工具历史修复;9 个 .py / 5798 行 |
| litellm 参考 | `docs/litellm/litellm/responses/litellm_completion_transformation/` | 1.84.0,2026-05-05 GitHub main HEAD;泛化场景实现 |
| Rust 当前 | `crates/adapters/src/responses/` 等 | branch `fix/proxy-responses-to-chat-routing`,本会话已含 4 处增量(usage 规范化 / `tool_calls:null` 与 `choices:null` 容忍 / `lookup_for_request`) |

**Codex CLI 实际请求形态裁剪**:多轮 message + function_call + function_call_output + tools(function 类型)。**不会触发** image_generation / file_search / mcp / computer_use / annotation 等专属工具或 web_search 引文,这些 litellm 行为不在评估范围。

## 二、覆盖度对比表

### A. Request body Responses → Chat (`request.rs` ↔ `responses_adapter.py` ↔ `transformation.py`)

| 字段 / 行为 | Python pre | litellm | Rust 当前 | 备注 |
|---|---|---|---|---|
| model / temperature / top_p / seed / stop / user / parallel_tool_calls / freq+pres penalty | ✓ | ✓ | ✓ | 透传一致 |
| instructions(str) → system | ✓ | ✓ | ✓ | |
| instructions(dict.text/content) | ✓ | ✓ | ✓ | |
| max_output_tokens → max_tokens | ✓ | ✓ | ✓ | |
| tools.function | ✓ | ✓ | ✓ | parameters 缺 type 自动补 object |
| tools.custom → function (input:string) | ✓ | 部分 | ✓ | litellm 走 apply_patch 单分支 |
| tools.web_search/file_search/mcp/computer_use 等丢弃 | ✓ | ✗(转译) | ✓ | Codex 不需要 |
| tool_choice "auto"/"none"/"required" | ✓ | ✓ | ✓ | |
| tool_choice {type:function, function:{name}} | ✓ | ✓ | ✓ | |
| tool_choice {type:any/required/tool} → "required" | ✓ | ✓ | ✓ | |
| input.message(含多模态 blocks) | ✓ | ✓ | ✓ | |
| input.function_call | ✓ | ✓ | ✓ | |
| input.function_call_output(call_id 别名 tool_call_id/id 兜底) | ✓ | ✓ | ✓ | |
| input.input_image / input_file / input_audio / input_video | ✓ | ✓ | ✓ | |
| input.reasoning(opaque)挂下一条 assistant | ✓ | 部分 | ✓ | Rust 用单空格占位 |
| 连续 user / assistant 合并 | ✓ | 部分 | ✓ | |
| developer → system(非 OpenAI 官方) | ✓ | n/a | ✓ | |
| text.format → response_format(json_schema/json_object) | ✓ | ✓ | ✓ | |
| reasoning(str/{effort}) → reasoning_effort | ✓ | ✓ | ✓ | xhigh/max → high |
| reasoning {summary:...} 保留 | ✓ | ✓ | 部分 | Rust 没单独写回 reasoning 字段,语义对 Chat 端无影响 |
| store / metadata / prediction / service_tier / modalities / audio | ✓ | 部分 | ✓ | |
| previous_response_id 历史回放 | ✓ | ✓ | ✓ | |
| **previous_response_id 解码(resp_<base64> → upstream chatcmpl-xxx)** | ✓ | ✓ | **✗** | Rust 用合成 nanos id,无 codec |
| **TOOL_CALLS_CACHE 兜底重建** | ✓ | ✓ | **✗** | Rust 只按位置补空 id |
| 占位 assistant 插入(孤儿 tool 前没有 assistant) | ✓ | ✓ | 部分 | Rust 直接 drop |
| stream + stream_options.include_usage | ✓ | ✓ | ✓ | |

### B. Response stream Chat SSE → Responses SSE (`converter.rs` ↔ `chat_responses_adapter.py` ↔ `streaming_iterator.py`)

| 事件 / 行为 | Python pre | litellm | Rust 当前 | 备注 |
|---|---|---|---|---|
| response.created | ✓ | ✓ | ✓ | |
| **response.in_progress** | ✓ | ✓ | **✗** | Rust 不发,严格客户端会卡住 |
| response.output_item.added (message/reasoning/function_call) | ✓ | ✓ | ✓ | |
| response.content_part.added / output_text.delta / done / content_part.done | ✓ | ✓ | ✓ | |
| response.reasoning_summary_part.added / text.delta / done / part.done | ✓ | ✓ | ✓ | |
| response.function_call_arguments.delta / done | ✓ | ✓ | ✓ | |
| response.completed (status / output[] / usage / incomplete_details) | ✓ | ✓ | ✓ | |
| EOF without [DONE] → incomplete + interrupted | ✓ | ✓ | ✓ | |
| finish_reason 全套映射(stop / length → max_output_tokens / content_filter / tool_calls / function_call) | ✓ | ✓ | ✓ | |
| usage 规范化(prompt→input、completion→output、total 兜底) | ✓ | ✓ | ✓ | 本会话已补 |
| usage 子字段重命名(prompt_tokens_details → input_tokens_details 等) | ✓ | ✓ | ✓ | 已补 |
| usage 缺失发零兜底 | ✓ | ✓ | ✓ | 已补 |
| `choices[0].usage`(Kimi 非标位置) | 部分 | ✗ | ✓ | Rust 同时收顶层 + choices 内 |
| `choices: null` / `tool_calls: null` 容忍 | ✗ | ✗ | ✓ | Rust 独有补丁(MiMo) |
| legacy `delta.function_call`(单工具 v1) → function_call item | ✓ | ✓ | ✓ | |
| 多 tool_call 同 chunk(index=0,1) 各自独立 output_item | ✓ | ✓ | ✓ | |
| tool_call.id 后续帧补全 / 缺失生成兜底 | ✓ | 部分 | ✓ | litellm 缺 id 直接 skip 整个 delta |
| 大 arguments 切小块(chunk_size=10) | ✗ | ✓ | ✗ | 仅美化,可不补 |
| annotations / sequence_number / provider_specific_fields | ✗ | ✓ | ✗ | Codex 用不到 |
| status:failed 单独事件 | 部分 | ✓ | ✗ | 当前都映射为 incomplete |

### D. reasoning_content

| 行为 | Python pre | litellm | Rust 当前 | 备注 |
|---|---|---|---|---|
| `delta.reasoning_content` → reasoning_summary_text.delta | ✓ | ✓ | ✓ | |
| reasoning 多 chunk 拼接 | ✓ | ✓ | ✓ | |
| reasoning → message 切换时关闭 reasoning | ✓ | ✓ | ✓ | |
| 历史回放时 assistant.tool_calls 必带 reasoning_content(DeepSeek thinking 修复) | ✓ | 部分 | ✓ | Rust 已移植,且 provider 配置 thinking + DeepSeek 关键字双重判断 |
| reasoning summary 多 part(part_index)拼接 | ✗ | 部分 | ✗ | Codex CLI UI 当前只用 summary_index=0 |
| `delta.thinking_blocks`(Anthropic 风格) | ✗ | ✓ | ✗ | 与 Codex/Chat 上游均无关 |

### E. usage 规范化

完全对齐 litellm `_transform_chat_completion_usage_to_responses_usage`(`transformation.py:2066-2157`)。Rust `normalize_usage_to_responses_shape` 是简化版:把整块 details 直接搬过去,而 litellm 在子对象里逐字段重命名。功能等价,无差异。

### F. session / previous_response_id

| 行为 | Python pre | litellm | Rust 当前 | 备注 |
|---|---|---|---|---|
| 内存 LRU + TTL | ✓ (1000 / 3600s) | ✓(DB 可选) | ✓ (1000 / 3600s) | |
| build_messages_with_history(history + current) | ✓ | ✓ | ✓ | |
| **response_id 编码(provider/model/upstream_id base64)** | ✓ | ✓ | **✗** | Rust 用 `resp_<nanos:x>` |
| **previous_response_id 解码** | ✓ | ✓ | **✗** | Rust 直接拿原文做 key |
| TOOL_CALLS_CACHE(call_id → tool_call 定义)二级缓存 | ✓ | ✓ | **✗** | 缺 |

### G. provider-specific quirks

| Quirk | Python pre | litellm | Rust 当前 | 备注 |
|---|---|---|---|---|
| DeepSeek thinking 历史 reasoning_content 单空格占位 | ✓ | ✗ | ✓ | Rust 已移植 |
| Kimi `choices[0].usage` 非标位置 | ✓ | ✗ | ✓ | |
| MiMo `tool_calls: null` / `choices: null` 帧 | ✗ | ✗ | ✓ | Rust 独有补丁 |
| Bedrock 大 arguments 拆小块 | ✗ | ✓ | ✗ | 美化,可不补 |
| Anthropic 连续 function_call 合并到同一 assistant | ✗ | ✓ | ✗ | Codex 链路无 Anthropic 直连 |
| Azure / OpenAI 官方区分(developer 角色保留) | ✓ | ✗ | ✓ | |

## 三、缺失项清单(Rust 缺,Python 或 litellm 有)

### P0 — 影响日常对话 / 工具循环

1. **TOOL_CALLS_CACHE 二级缓存 + 工具调用历史重建**
   - 来自:Python `responses_adapter.py:466-597 _repair_tool_call_ids` + `session_cache.py:202-247 ToolCallCache`;litellm `transformation.py:802-948 _ensure_tool_results_have_corresponding_tool_calls`
   - 现状:Rust `request.rs:688-741 repair_tool_call_ids` 只做"空 tool_call_id 按位置补"。当 Codex 增量发 `function_call_output` 但前一条 assistant 已被 session compress 掉 / 用户 history 截断,tool_call_id 非空但找不到归属时,Rust 直接保留导致上游 400(Kimi/DeepSeek 已遇到)。
   - 建议补在:`crates/adapters/src/responses/session.rs` 增加 `ToolCallCache`(call_id → {name, arguments, type});`request.rs:repair_tool_call_ids` 在路径 B 增加 cache 兜底 + 占位 assistant 插入。

2. **response.in_progress 事件**
   - 来自:Python `streaming_adapter.py:266-281`,litellm `streaming_iterator.py:434-444`
   - 现状:Rust `converter.rs:236-254` 只 emit `response.created` 就直接进 streaming。OpenAI Responses 协议规定 created 后立即跟一个 in_progress;Codex CLI 0.x/1.x 实测能容忍,但严格客户端(litellm 自己当 client、Anthropic 工具链)会卡住。
   - 建议补在:`converter.rs::handle_frame` 在 emit `response.created` 后立刻 emit 同 payload 的 `response.in_progress`(只做一次,用 `state` 守卫)。

### P1 — 影响特定供应商 / 边缘情况

3. **占位 assistant 插入(孤儿 tool 前没有 assistant)**
   - 来自:Python `responses_adapter.py:566-597`
   - 现状:Rust `repair_tool_call_ids` 把找不到 assistant 的孤儿 tool message 直接 `continue` 丢弃。当历史压缩只剩一个 `function_call_output` 时,Rust 把它扔了,会让模型完全失去工具结果上下文。
   - 建议补在:`request.rs::repair_tool_call_ids` 路径 B 找不到 assistant 时插占位 `{"role":"assistant","content":"","tool_calls":[<占位>]}` 而不是 drop。

4. **response_id 编码 / 解码(provider+model affinity)**
   - 来自:Python `response_id_codec.py:19-78` + `responses_adapter.py:395-403`;litellm `session_handler.py:284-291`
   - 现状:Rust 用 `resp_<nanos:x>`,无 provider 信息。多 provider 部署 / 客户端跨进程重连场景下,无法把 previous_response_id 路由回原 provider。Codex 单机本地代理用例下不暴露,但若以后做多账号 / 多 provider 同时运行可能踩坑。
   - 建议补在:新建 `crates/adapters/src/response_id_codec.rs`,模仿 Python `encode_response_id(provider, model, upstream_id)`;`session.rs::save` 用编码后的 ID 做 key;`request.rs::build_messages_from_input` 在用 cache 前先 decode。

5. **status:failed 显式事件**(litellm only,Codex 不区分,优先级很低)

### P2 — 锦上添花

6. Bedrock arguments 切片(litellm 独有,Codex 无 Bedrock,可不补)
7. annotations / web_search 引文事件(Codex 用不到)
8. provider_specific_fields 透传(Codex 用不到)
9. sequence_number 字段(OpenAI 协议 optional)

## 四、过时项清单(Rust 沿用 Python 老逻辑,litellm 已改进)

| # | 项 | 评估 |
|---|---|---|
| 10 | reasoning summary 多 part_index | Python/Rust 都只用 summary_index=0,litellm 在 thinking_blocks 多片场景会拆 part。Codex CLI UI 只读 `summary[0].text`,**保持现状** |
| 11 | fc_id 命名 (`fc_<seed>_<idx>`) | Python/Rust 把 fc 内部 id 与 call_id 区分,litellm 统一成 `call_<id>`。功能等价,Codex 历史回放只看 call_id,**保持现状** |
| 12 | previous_response_id 编码 | 见 P1#4 |

## 五、优化方案(按优先级 + 颗粒度,可独立成 PR)

| PR | 内容 | 改动文件 | 工作量 | 优先级 |
|---|---|---|---|---|
| **PR-2** | `response.in_progress` 事件 | `converter.rs::handle_frame` | ~30 行 | **先行**(最小、快速验证 CI) |
| **PR-1** | TOOL_CALLS_CACHE + 历史重建 | 新增 `tool_call_cache.rs` + `converter.rs::close_tool_call` 写入 + `request.rs::repair_tool_call_ids` 读取 + 占位 assistant | ~250 行(含测试) | **再行**(影响最大) |
| **PR-3** | 孤儿 tool 占位 assistant | `request.rs::repair_tool_call_ids` 路径 B | ~50 行 | 之后 |
| **PR-4** | response_id codec | 新增 `response_id_codec.rs` + `session.rs` / `request.rs` / `mod.rs` 配套 | ~200 行 | 可选(多 provider 场景才需要) |

### PR-2 详细步骤

- 文件:`crates/adapters/src/responses/converter.rs::handle_frame`
- 改动:在 emit `response.created` 后紧跟 emit `response.in_progress`(同 payload),用结构体字段 `sent_in_progress: bool` 守卫防重复
- 测试:更新所有现有 SSE 单测的 `names()` 期望(在 `response.created` 后插入 `response.in_progress`)

### PR-1 详细步骤

- 新增 `crates/adapters/src/responses/tool_call_cache.rs`(参照 `session.rs` 的 LRU+TTL 结构,key=`call_id`,value=`{name, arguments, type}`)
- `crates/adapters/src/responses/converter.rs::close_tool_call`:每次 close 时把 `(call_id, name, args_acc)` 存入全局 `ToolCallCache`
- `crates/adapters/src/responses/request.rs::repair_tool_call_ids`:路径 B 增加 cache 命中 → 把 tool_call 注回前 assistant.tool_calls;cache 不命中 → 插占位 assistant
- 测试:
  - 单测覆盖 cache 命中重建 / cache miss 但 tools 中有定义可重建 / 完全无信息 → 占位 / 跨多轮 LRU eviction

### PR-3 详细步骤

- 文件:`crates/adapters/src/responses/request.rs::repair_tool_call_ids`
- 改动:路径 B `tcid not in available_call_ids` 时,若 `last_assistant_idx` 是 `None`,插占位 assistant 而非 `continue`
- 测试:`fn orphan_tool_message_with_call_id_inserts_placeholder_assistant`

### PR-4 详细步骤

- 文件:
  - 新建 `crates/adapters/src/response_id_codec.rs`(encode/decode + 校验)
  - `session.rs` 用编码后的 ID 做 key
  - `request.rs::build_messages_from_input` 在用 cache 前先 decode
  - `mod.rs::transform_response_stream` emit `response.created` 时用编码后的 ID
- 测试:encode/decode round-trip + 解码失败 fallback

## 六、风险与权衡

**Python 老逻辑可以不补**:
- Bedrock arguments 切片 / annotations / mcp / computer_use / file_search / image_generation / web_search 事件 — Codex 链路无对应工具或不消费
- thinking_blocks 多 part_index — Codex CLI UI 只读 `summary[0]`
- Anthropic 连续 function_call 合并 — Codex 不直连 Anthropic Messages

**litellm 行为对 Codex 过度**:
- DB-backed session(litellm 用 PostgreSQL/Redis),Rust 内存版足够
- sequence_number 协议字段:Codex 0.x/1.x 不读
- provider_specific_fields:litellm 给上层 LLM 客户端用,Codex CLI 不消费

**当前 Rust 的"保守 id 方案"实际不阻塞日常使用**:Codex CLI 实测会原样回传 resp_id,所以 cache 命中正常。多 provider 同时切换时(P1#4 场景)才暴露。

## 七、关键文件路径(供 follow-up 引用)

- `crates/adapters/src/responses/converter.rs` — Chat SSE → Responses SSE 状态机
- `crates/adapters/src/responses/request.rs` — Responses → Chat 请求体转换
- `crates/adapters/src/responses/session.rs` — ResponseSessionCache
- `/tmp/cas-compare/python-pre-refactor/responses_adapter.py:466-597` — `_repair_tool_call_ids` 完整版(若 /tmp 已清,从 `git show 904095d:backend/responses_adapter.py` 重新取)
- `/tmp/cas-compare/python-pre-refactor/streaming_adapter.py:266-281` — `in_progress` emit 时机
- `docs/litellm/litellm/responses/litellm_completion_transformation/transformation.py:802-948` — `_ensure_tool_results_have_corresponding_tool_calls`
- `docs/litellm/litellm/responses/litellm_completion_transformation/streaming_iterator.py:52-110` — 完整事件 sequence 状态机
