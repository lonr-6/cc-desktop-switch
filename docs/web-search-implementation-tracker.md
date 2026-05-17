# web_search 全 provider 实施跟踪(2026-05-09 起)

> 修复目标:Codex.app 入站每个 turn tools 数组都有 `{type:"web_search", external_web_access:true, search_content_types:["text","image"]}`(实测 dump 确认,**默认开,不依赖用户配置**),代理把它**按上游 provider chat API 真实支持的形态**转换出去 + 入站响应通用处理 `delta.annotations` URL citation。
>
> 严格规则:**每家 provider 必须基于官方文档原文实证,禁止 agent 推测。1:1 复刻参考实现优先**。

## 状态总览

| Provider | 文档实证 | 实施 | 单测 | 实地测试 | 备注 |
|---|---|---|---|---|---|
| **Xiaomi MiMo** | ✅ mimo2codex fresh 源码 1:1 对照 + dump 实证 4xx 错误 | ✅ A 配置开关 + B 运行时 cache + transparent retry | ✅ 13 用例 + transparent retry 集成路径 | ✅ **实测全通过**(2026-05-09):默认关 / `=true` plugin 未开 transparent retry 无感降级秒出结果 / `=false` 显式关 三场景全验证;log 流 `WARN auto-disabled → INFO retry status 200 → SUCCESS upstream status 200` 完美 | **完成,进入 Kimi 移植阶段** |
| **Kimi (Moonshot)** | ✅ WebFetch `platform.kimi.ai/docs/guide/use-web-search` 真文档实证 | ✅ Kimi/Moonshot 分支 + 自动注入 `thinking.disabled` 顶级字段 | ✅ 5 用例 | ✅ **实测通过**(2026-05-09):Kimi For Coding + Moonshot `=true` 上游接受 builtin_function 无降级警告;Moonshot 单条 429 是账号 TPD 超额(跟 PR 无关) | 完成,进入 DeepSeek 阶段 |
| **DeepSeek** | ✅ WebFetch `api-docs.deepseek.com/api/create-chat-completion` 实证 `"Currently, only function is supported"` | ✅ 显式 drop 分支 + warn key `web_search:not-supported-by-deepseek-api` | ✅ 2 用例 | ✅ 不需要实测(文档实证不支持,代码层只 drop;用户联网走 P5 已通的 MCP 路径) | 完成 |
| **MiniMax M2.x** | ✅ WebFetch `platform.minimaxi.com/docs/api-reference/` + liteLLM 三方实证:chat tools 仅 `type:"function"`,web_search 仅作 Token Plan MCP 工具存在 | ✅ 显式 drop + warn key `web_search:not-supported-by-minimax-api` + **新加 MiniMax builtin preset(2026-05-09)** + 官方 favicon 图标 | ✅ 1 用例 + preset 完整性测试 | ✅ 不需实测(文档实证不支持;preset 显示可后续 UI 验证) | 完成 |
| **(实验兼容)阿里 Qwen** | ⚠️ **部分实证 + 部分阻断**:Responses API 端点 (`/compatible-mode/v1/responses`) WebFetch 真原文实证 `tools:[{type:"web_search"}]` ✓;但本仓 `bailian` preset 走 chat completions 端点,chat 端 `extra_body.enable_search` 形态文档入口全 404 拿不到一手源 | ⏸️ **暂停**(等用户提供可访问的 chat completions web search 文档 URL) | — | — | follow-up,详见 §5 |
| **(实验兼容)智谱 GLM** | ❌ **未实证**:agent 当时给的 `browser.search` 是综合推理无 quote 原文;`docs.bigmodel.cn` 各路径全 404 | ⏸️ **暂停**(等用户提供可访问的官方文档 URL) | — | — | follow-up,详见 §6 |
| **入站 `delta.annotations` 通用处理** | ✅ mimo2codex `streamToSse.ts:156-163, 338-352` | ✅ `handle_annotations_delta` + `translate_annotation` | ✅ 5 用例 | ⏳ 跟 MiMo 一起实测 | 跨 provider 通用 |

## 1. Xiaomi MiMo(优先实施)

### 1.1 文档实证

来源:`/tmp/mimo2codex-fresh@fe79178/src/translate/reqToChat.ts:140-209`(已落地:`docs/litellm/litellm/` 内置 + 临时 clone)

```typescript
// reqToChat.ts:140-145 注释明确
// "MiMo 自家 chat 端原生支持 type:"web_search",需要在
// MiMo 控制台开 Web Search Plugin (https://platform.xiaomimimo.com/#/console/plugin)"

// reqToChat.ts:196-209 实施
if (t.type === "web_search" || t.type === "web_search_preview") {
  const w = t as {
    user_location?: ChatWebSearchTool["user_location"];
    max_keyword?: number;
    force_search?: boolean;
    limit?: number;
  };
  const tool: ChatWebSearchTool = { type: "web_search" };
  if (w.user_location) tool.user_location = w.user_location;
  if (typeof w.max_keyword === "number") tool.max_keyword = w.max_keyword;
  if (typeof w.force_search === "boolean") tool.force_search = w.force_search;
  if (typeof w.limit === "number") tool.limit = w.limit;
  return tool;
}
```

`ChatWebSearchTool` 类型(`types.ts`):

```typescript
export interface ChatWebSearchTool {
  type: "web_search";
  user_location?: {
    type?: "approximate";
    country?: string;
    region?: string;
    city?: string;
    district?: string;
    longitude?: number;
    latitude?: number;
  };
  max_keyword?: number;
  force_search?: boolean;
  limit?: number;
}
```

### 1.2 实施要点

- `convert_responses_tool_to_chat_tool` 加 `provider: Option<&Provider>` 参数
- `web_search`/`web_search_preview` 分支:仅 MiMo provider 转换,其他暂返 `vec![]`(等单家文档实证后再加)
- 字段透传 4 个:`user_location` / `max_keyword` / `force_search` / `limit`
- **OpenAI 的 `search_context_size` / `external_web_access` / `search_content_types` 字段在 MiMo 无等价,silent drop**(对齐 mimo2codex)
- provider 识别:`provider.id` / `base_url` 含 `xiaomimimo` / `mimo`(沿用 `provider_looks_like` 模式)

### 1.2.1 A+B 双层防错链路(2026-05-09 dump 实证)

**A 层**:`Provider.request_options.web_search_enabled`(boolean,默认 false)。用户必须显式标 true 才发 web_search 工具。默认关闭原因:MiMo Token Plan 套餐没开 Web Search Plugin 时上游 400 `"web search tool found in the request body, but webSearchEnabled is false"`。UI 提示文案:**"web_search 需要先在 Xiaomi MiMo 控制台付费启用后才能正常使用"**。

**B 层**:运行时自动 disable cache(`adapters::disable_web_search_for(provider_id)`)。`forward.rs::is_web_search_upstream_reject` 4xx 路径识别实测错误关键字(`webSearchEnabled is false` / `web search tool found` / 通用 `web search.*not enabled|not supported|not activated|disabled` 兜底)→ 命中即调 `disable_web_search_for`。`convert_web_search_tool` 检查 cache,命中即 drop(B 层在 A 层之后)。本次启动有效,应用重启后 cache 重置。

**未做**(留 follow-up):transparent retry without web_search / 持久化写回 config.json / UI provider 编辑加 web_search switch。

### 1.3 入站 annotations 通用处理(跟 MiMo 同 commit)

来源:`/tmp/mimo2codex-fresh@fe79178/src/translate/streamToSse.ts:156-163, 338-352`

```typescript
function translateAnnotation(a: ChatAnnotation): ResponsesAnnotation {
  return {
    type: a.type ?? "url_citation",
    url: a.url ?? "",
    title: a.title ?? "",
    ...(a.summary !== undefined ? { snippet: a.summary } : {}),
  };
}

// processChunk 里:
if (delta.annotations && delta.annotations.length > 0) {
  if (state.activeKind !== "message") openMessage(sink, state);
  for (const a of delta.annotations) {
    const translated = translateAnnotation(a);
    const annotationIndex = state.activeAnnotations.length;
    state.activeAnnotations.push(translated);
    emit(sink, state, "response.output_text.annotation.added", {
      item_id: state.activeItemId!,
      output_index: state.outputIndex - 1,
      content_index: 0,
      annotation_index: annotationIndex,
      annotation: translated,
    });
  }
}
```

字段重命名:**`summary` → `snippet`**。最后写到 final message item `content[0].annotations`(替换当前写死 `[]`)。

### 1.4 实施 / 测试 checklist

- [ ] `RequestPlan.original_responses_request` 已在(P5),无需新加
- [ ] `convert_responses_tool_to_chat_tool` 加 `provider` 参数
- [ ] `web_search`/`web_search_preview` 分支:MiMo 转换,其他暂 drop(留 TODO 标其他 provider 待文档实证)
- [ ] `ChatToResponsesConverter` 加 `active_annotations: Vec<Value>` 字段
- [ ] `ChatChunkDelta` 加 `annotations: Option<Vec<Value>>` 解析
- [ ] `handle_frame` 处理 delta.annotations + emit `response.output_text.annotation.added`
- [ ] 修 3 处 final message item 把 `[]` 替换成 `self.active_annotations.clone()`
- [ ] 单测(MiMo web_search 转换 + annotations emit + final message annotations 累积)
- [ ] 用户实地测:Codex.app + MiMo provider + 控制台开 Web Search Plugin → "搜索 X 最新进展" → 看模型是否用 web_search 工具(不是绕路 Node Repl) + 响应里有 url 引用

### 1.5 用户验收标准

- 模型不再绕路 `mcp__node_repl__` 写 JS fetch DDG(对比当前行为)
- 响应里附 url citation,Codex.app UI 展示来源链接
- MiMo 控制台账单显示 web_search 调用记录

## 2. Kimi (Moonshot)

### 2.1 文档实证

来源:**WebFetch** `https://platform.kimi.ai/docs/guide/use-web-search`(2026-05-09 真原文,跟 `platform.moonshot.cn/docs/api/tool_use` 301 重定向到同一处)。

#### 2.1.1 Tool 声明

```json
{
  "type": "builtin_function",
  "function": {
    "name": "$web_search"
  }
}
```

> "The `$web_search` function is prefixed with a dollar sign `$`, which is our agreed way to indicate Kimi built-in functions."

#### 2.1.2 强制约束:Thinking 必须 disabled

> "When using `$web_search` function, you must disable the thinking ability of the model."

通过 `extra_body.thinking.type = "disabled"` 设置。OpenAI Python SDK `extra_body` 在 wire 上等价于 request body 顶级加 `thinking: {type: "disabled"}` 字段。

**Side effect**:用户启用 web_search 时 Kimi 模型 thinking 能力被强制禁用 — UI 提示文案需要补:
**"启用 web_search 时模型 thinking 能力会被禁用(Kimi API 限制)。"**

#### 2.1.3 执行流程

- `finish_reason == "tool_calls"` 时模型请求搜索
- 实现:**"return the arguments as-is"**(代理不需特殊处理,Codex CLI 拿到 tool_call 后会回灌)
- 提交结果:`role=tool` + 匹配 `tool_call_id`

#### 2.1.4 计费 / Token

- 搜索结果计入 `prompt_tokens`
- usage 含 `search_content_total_tokens`(示例 13046)
- **每次搜索独立计费 $0.005**(超 token 费)

#### 2.1.5 兼容性

> "The tool works alongside regular function tools without code restructuring needed for switching implementations."

Kimi `$web_search` 跟普通 function tools 共存,不冲突。

### 2.2 实施要点

- `convert_web_search_tool` 加 Kimi 分支:provider 识别 `provider_looks_like(p, "kimi") || provider_looks_like(p, "moonshot")`,输出 `{"type":"builtin_function", "function":{"name":"$web_search"}}`
- **不透传任何 OpenAI 字段**(`user_location` / `max_keyword` / `force_search` / `limit` / `external_web_access` / `search_content_types` 全 drop —— Kimi 文档明确只要 type + function.name)
- **Body 后处理**:扫 outbound tools,命中 `type:"builtin_function"` + `function.name == "$web_search"` → 设 `body["thinking"] = {"type": "disabled"}`(注入顶级字段,wire-equivalent of OpenAI SDK `extra_body`)
- A 层(`web_search_enabled`)+ B 层(运行时 cache)+ transparent retry 全部复用 MiMo 阶段已实施的基础设施,无需重复

### 2.3 实施 / 测试 checklist

- [ ] `convert_web_search_tool` Kimi 分支:输出固定 builtin_function schema,不透传任何子字段
- [ ] 顶级 `thinking.disabled` 注入 hook(在 `responses_body_to_chat_body_for_provider_with_session` body 后处理)
- [ ] 单测:Kimi web_search 转换正确 / 命中时 thinking 注入 / 不启用 web_search 时 thinking 不动 / 用户原配置 thinking 时被覆盖
- [ ] 用户实地测:Codex.app + Kimi provider + `web_search_enabled=true` → "搜索 X" → 看模型直接用 $web_search 工具(不绕路 Node Repl)+ 响应 url citation 通过 delta.annotations 展示

## 3. DeepSeek

### 3.1 文档实证

来源:**WebFetch** `https://api-docs.deepseek.com/api/create-chat-completion`(2026-05-09 真原文)。

#### 3.1.1 tools 字段约束

> **"The type of the tool. Currently, only `function` is supported."**

最大 128 functions,naming `[a-zA-Z0-9_-]{1,64}`,parameters 用 JSON Schema。

#### 3.1.2 不支持的能力

WebFetch 全文中**完全没有**:
- `web_search` / `web_search_options` / `builtin_function` 等 tool type
- 顶级 `enable_search` / `web_search` / `search_options` 字段
- search-specific model id(如 deepseek-search 等)
- `delta.annotations` / url citation 响应字段提及

#### 3.1.3 结论

DeepSeek chat completions API **完全不支持原生 web search**。用户启用
`web_search_enabled=true` 时代理只能 drop。用户需联网搜索 → 走 P5 修通的
namespace MCP 工具路径(`mcp_servers.<...>` 配置 + 模型用 `read_mcp_resource` /
Node Repl 之类绕路)。

### 3.2 实施

`crates/adapters/src/responses/request.rs::convert_web_search_tool`:
- DeepSeek 分支显式 drop:`provider_looks_like(p, "deepseek")` 命中 → `warn_once_drop_tool("web_search:not-supported-by-deepseek-api")` + `vec![]`
- 用户在 log 看到 warn key 立即知道是 DeepSeek API 不支持

### 3.3 测试

`#[cfg(test)] mod tests` 2 用例:
- `deepseek_web_search_dropped_with_explicit_warn_key` — `web_search_enabled=true` 时 web_search 仍 drop,只剩其他 function tools;不触发 Kimi thinking 注入
- `deepseek_web_search_drop_independent_of_web_search_enabled_flag` — A 层默认行为(未启用),B 层和 A 层都把 DeepSeek web_search drop

`cargo test --workspace` 全 193 pass(无回归)。

## 4. MiniMax M2.x

### 4.1 待办

- **WebFetch** 官方文档:https://platform.minimaxi.com/document/Function-call-Web-Search 或 https://www.minimax.io/platform/document/web_search
- 关注:
  - 是否支持 `tools:[{type:"web_search"}]` 或类似?
  - 字段格式

(等 MiMo 实测通过后再启动)

## 5. 阿里 Qwen(实验兼容)— **暂停状态**

### 5.1 实证现状(2026-05-09)

**Responses API 端点已实证支持**:WebFetch `https://help.aliyun.com/zh/model-studio/qwen-api-via-openai-responses` 拿到原文:

```json
{
  "model": "qwen3.6-plus",
  "input": "...",
  "tools": [{"type": "web_search"}, {"type": "code_interpreter"}, {"type": "web_extractor"}]
}
```

> "内置联网搜索、网页抓取、代码解释器、文搜图、图搜图、知识库搜索等工具"

端点:`POST https://dashscope.aliyuncs.com/compatible-mode/v1/responses`

响应:`web_search_call` output items 含 `query` + `sources` 数组。

### 5.2 暂停原因

本仓现有 `bailian` builtin preset 配:
- `baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1"`
- `apiFormat: "openai_chat"`(走 **chat completions** 端点 `/v1/chat/completions`,**不是** Responses)

**Qwen chat completions 端点是否支持 web_search 没找到一手文档**:
- `https://help.aliyun.com/zh/model-studio/qwen-via-openai-chat-completions` → 404
- `https://help.aliyun.com/zh/model-studio/use-the-qwen-text-generation-model` → 404
- 其他 Aliyun 入口均 404 / SSR 拿不到

agent 之前给的 `extra_body.enable_search: true + search_options.forced_search` 形态**没附具体页面 quote 原文**(可能 agent 综合多页拼出),不算严格 WebFetch 真原文实证。

### 5.3 后续启动条件(任一满足即可)

1. **用户提供可访问的 Qwen chat completions web_search 文档 URL**(我 WebFetch 验证后做 explicit `extra_body.enable_search` 分支)
2. **本仓 `bailian` preset 升级到 Responses 端点**(`apiFormat` 改 `responses` / 加新 preset)→ 直接走 Qwen Responses API 已实证的 `tools:[{type:"web_search"}]` 路径
3. 用户手上有 Qwen chat completions web search 真实请求 JSON,贴给我做实证依据

未启动前,bailian preset 用户 `web_search_enabled=true` 走通用 fallback(`web_search:provider-not-implemented`)。

## 6. 智谱 GLM(实验兼容)— **暂停状态**

### 6.1 实证现状(2026-05-09)

**完全没找到一手文档**:
- `https://docs.bigmodel.cn/cn/api-reference/api-trial/web-search` → 404
- `https://docs.bigmodel.cn/cn/api-reference/llm/web-search` → 404
- `https://docs.bigmodel.cn/api-reference/搜索能力` → 404
- `https://www.bigmodel.cn/dev/api` → redirect to docs.bigmodel.cn 然后 404

agent 之前给的 "GLM-4.5+ tool-integrated reasoning 通过预定义工具如 `browser.search`" **是 agent 自己综合推理,完全没 quote 原文**。

### 6.2 暂停原因

按 "严格不靠推测、必须 WebFetch 真原文实证" 原则,无文档源就不动。

### 6.3 后续启动条件(用户任一满足即可)

1. 用户提供可访问的 GLM 官方 web_search / 联网搜索文档 URL(我 WebFetch 验证)
2. 用户手上有 GLM 真实 web_search 请求 JSON 示例,贴给我做实证依据

未启动前,zhipu preset 用户 `web_search_enabled=true` 走通用 fallback。

## 7. 通用入站 annotations 处理(跟 MiMo 同实施)

跨所有 provider 通用 — 任何 chat 上游返回 `delta.annotations` 时都受益(不只 web_search 用,模型回答里引用网页时也用)。详见 §1.3。

## 8. 实施流水线

```
MiMo 完成(本轮 PR)
    ↓ 用户实测通过
Kimi 文档调研 + 实施(下一轮 PR)
    ↓ 用户实测通过
DeepSeek 文档调研 + 实施(drop)
    ↓
MiniMax 文档调研 + 实施(待证)
    ↓
Qwen / GLM(实验兼容,可作 follow-up)
```

每家完成后:
1. 更新本 doc 的"状态总览"表格
2. 用户实测 + commit "feat(web-search): <provider> support" 单独 PR(stacked)
3. 累计到 v2.1.2 / v2.1.3 release notes

## 9. 关联记忆

- [`feedback_audit_before_borrow.md`](../../.claude/projects/-Users-alysechen-alysechen-github-codex-app-transfer/memory/feedback_audit_before_borrow.md) — 上游借鉴必先 grep / 实证(每家 provider 严格执行)
- [`feedback_litellm_first.md`](../../.claude/projects/-Users-alysechen-alysechen-github-codex-app-transfer/memory/feedback_litellm_first.md) — 协议 / prompt / session 类工作先查上游
- [`feedback_credit_upstream_in_readme.md`](../../.claude/projects/-Users-alysechen-alysechen-github-codex-app-transfer/memory/feedback_credit_upstream_in_readme.md) — 借鉴上游同步 README 致谢
- [`feedback_real_config_validation.md`](../../.claude/projects/-Users-alysechen-alysechen-github-codex-app-transfer/memory/feedback_real_config_validation.md) — 改完真机 config 跑一遍验证(每家 provider 都要实测)
