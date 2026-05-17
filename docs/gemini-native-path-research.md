# Gemini Native `generateContent` Path 调研 + 实施方案

**调研时间**:2026-05-10
**触发动机**:PR #94 走 Gemini OpenAI 兼容 chat 端点 + 注入 `extra_body.google.tools=[{google_search:{}}]`,curl 实测 5 种 variant 全部 400(`Cannot find field`),官方文档明列该字段**仅对 `gemini-3-pro-image-preview`** 有效,text chat 端点根本没法走 grounding。要 web_search 必须走 native `generateContent` API。

---

## TL;DR

- **OpenAI compat 路径不支持 google_search**(已实证 5 种 variant 全 400)
- **Native `generateContent` 路径完全可行**,LiteLLM 已实现完整双向转换可借鉴
- **本项目架构改动量**:加第 4 种 `apiFormat=gemini_native` + 新 adapter crate(~800-1200 LoC + ~400 LoC tests)
- **预估工期**:focused 3-5 天
- **风险**:Gemini wire schema 跟 OpenAI 差异大(role mapping / parts / tool_config / grounding 展开),mock 数据不可靠,必须用付费配额抓真 SSE wire 实证

---

## 1. 背景:为什么 OpenAI compat 死路

curl `https://generativelanguage.googleapis.com/v1beta/openai/chat/completions` 实测(`gemini-pro-latest` model + `Authorization: Bearer <key>`):

| Variant | 结果 |
|---|---|
| `extra_body.google.tools=[{google_search:{}}]` | 400 `Unknown name "tools" at 'extra_body.google'` |
| `extra_body.google_search={}` | 400 `Unknown name "google_search" at 'extra_body'` |
| `extra_body.tools=[{google_search:{}}]` | 400 `Unknown name "tools" at 'extra_body'` |
| `tools=[{type:"google_search"}]` | 400 `Invalid tool type: google_search` |
| `tools=[{type:"web_search"}]` | 400 `Invalid tool type: web_search` |
| `web_search_options={}` | 400 `Unknown name "web_search_options"` |

[官方文档](https://ai.google.dev/gemini-api/docs/openai)原文写 `extra_body.google.tools=[{google_search:{}}]`,但限定 **"Only for `gemini-3-pro-image-preview`"**(image generation 端点)。社区[讨论帖](https://discuss.ai.google.dev/t/is-it-possible-to-use-search-grounding-with-openai-api/89197)印证 text chat 不支持。

**结论**:OpenAI 兼容 chat completions 端点 = 死路,只能走 native。

---

## 2. LiteLLM 实现详解(可直接借鉴)

仓库:`BerriAI/litellm`(github.com/BerriAI/litellm)

### 2.1 Endpoint + 鉴权

```python
# litellm/llms/vertex_ai/common_utils.py:415
# Streaming
if stream is True:
    endpoint = "streamGenerateContent"
    url = "https://generativelanguage.googleapis.com/v1beta/{model}:streamGenerateContent?alt=sse"
# Non-streaming
else:
    url = "https://generativelanguage.googleapis.com/v1beta/{model}:generateContent"

# 鉴权(Google AI Studio,vs Vertex AI)
# vertex_and_google_ai_studio_gemini.py:484
auth_header = {"x-goog-api-key": gemini_api_key}     # AI Studio
default_headers["Authorization"] = f"Bearer {api_key}" # Vertex
```

**关键**:
- streaming 必须加 `?alt=sse` query 参数
- AI Studio 用 `x-goog-api-key` header(**不是** Bearer token)
- model 名字带 `models/` 前缀(`models/gemini-3.1-pro-preview`)

### 2.2 请求侧:messages → contents

```python
# litellm/llms/vertex_ai/gemini/transformation.py:311
def _gemini_convert_messages_with_history(messages):
    """
    1. 合并连续的 user/system → 单个 user content(Gemini 不支持独立 system role)
    2. role 映射: user/system → "user", assistant → "model"
    3. parts 构建: text → {text}, image_url → {inlineData:{mimeType,data}}
    4. 每个 user content 必须至少含一个 text part
    """
    contents = []
    msg_i = 0
    while msg_i < len(messages):
        user_content = []
        # 合并连续 user + system
        while msg_i < len(messages) and messages[msg_i]["role"] in {"user", "system"}:
            for element in messages[msg_i]["content"]:
                if element["type"] == "text":
                    parts.append({"text": element["text"]})
                elif element["type"] == "image_url":
                    parts.append({"inlineData": {"mimeType": "...", "data": "..."}})
        contents.append({"role": "user", "parts": user_content})
        # ... assistant 类似,role="model"
```

**关键差异点**(OpenAI vs Gemini):

| OpenAI Wire | Gemini Native Wire |
|---|---|
| `messages[].role: "system"` | (no native system role)合并到下一个 user content,或用顶层 `systemInstruction` 字段 |
| `messages[].role: "user"` | `contents[].role: "user"` |
| `messages[].role: "assistant"` | `contents[].role: "model"`(注意改名) |
| `messages[].role: "tool"` | `contents[].parts[].functionResponse: {name, response}` |
| `content: [{type:"text",text}]` | `parts: [{text:"..."}]` |
| `content: [{type:"image_url"}]` | `parts: [{inlineData:{mimeType,data}}]`(base64) |
| `tool_calls: [{id,function:{name,arguments}}]` | `parts: [{functionCall:{name,args}}]`(注意 args 是对象,不是 JSON string) |
| `tools=[{type:"function",function:{name,description,parameters}}]` | `tools=[{functionDeclarations:[{name,description,parameters}]}]`(扁平展开) |
| `tools=[{type:"web_search"}]` | `tools=[{googleSearch:{}}]` ← **native 才能 work** |
| `tool_choice: "auto"/"none"/"required"/{type:"function",function:{name}}` | `toolConfig: {functionCallingConfig:{mode:"AUTO"/"NONE"/"ANY", allowedFunctionNames}}` |
| `temperature/top_p/max_tokens` | 嵌套到 `generationConfig: {temperature, topP, maxOutputTokens}` |
| `stop` | `generationConfig.stopSequences[]` |
| `response_format: {type:"json_object"}` | `generationConfig.responseMimeType: "application/json"` |
| `response_format: {type:"json_schema",json_schema:{schema}}` | `generationConfig.{responseMimeType,responseSchema}` |
| `seed` | `generationConfig.seed` |

### 2.3 工具映射:web_search → googleSearch

```python
# litellm/llms/vertex_ai/gemini/vertex_and_google_ai_studio_gemini.py:596
elif "type" in tool and tool["type"] in ("web_search", "web_search_preview"):
    tool = {"google_search": {}}  # 注意 lower_snake,wire 实际是 googleSearch (camelCase)

# 同文件:358
def _map_web_search_options(self, value: dict) -> Tools:
    return Tools(googleSearch={})
```

**注意**:Gemini wire 用 camelCase(`googleSearch`),Python SDK 内部偶尔用 snake_case 后再转换。直接 wire-level 用 camelCase。

### 2.4 响应侧:streamGenerateContent SSE → OpenAI delta

Gemini SSE 形态(每行 `data: {json}`):
```json
{
  "candidates": [{
    "content": {
      "role": "model",
      "parts": [
        {"text": "纽约今天..."},
        {"functionCall": {"name": "search", "args": {"q": "weather"}}}
      ]
    },
    "finishReason": "STOP",
    "groundingMetadata": {
      "groundingChunks": [
        {"web": {"uri": "https://weather.com/...", "title": "Weather.com - NYC"}},
        {"web": {"uri": "https://accuweather.com/...", "title": "AccuWeather"}}
      ],
      "groundingSupports": [
        {
          "segment": {"startIndex": 0, "endIndex": 25, "text": "纽约今天 25°C 晴"},
          "groundingChunkIndices": [0, 1],
          "confidenceScores": [0.95, 0.88]
        }
      ],
      "webSearchQueries": ["纽约今天天气"]
    }
  }],
  "usageMetadata": {"promptTokenCount": 100, "candidatesTokenCount": 50, "totalTokenCount": 150}
}
```

**映射规则**:

| Gemini Field | OpenAI Chat Delta | OpenAI Responses Event |
|---|---|---|
| `candidates[0].content.parts[].text` | `choices[0].delta.content` | `response.output_text.delta` |
| `candidates[0].content.parts[].functionCall` | `choices[0].delta.tool_calls[]` | `response.function_call_arguments.delta` |
| `candidates[0].finishReason: "STOP"` | `choices[0].finish_reason: "stop"` | `response.completed` |
| `candidates[0].finishReason: "MAX_TOKENS"` | `choices[0].finish_reason: "length"` | 同上 |
| `candidates[0].finishReason: "SAFETY"` | `choices[0].finish_reason: "content_filter"` | 同上 |
| `candidates[0].finishReason: "TOOL_USE"` | `choices[0].finish_reason: "tool_calls"` | 同上 |
| `groundingMetadata.{groundingChunks,groundingSupports}` | `choices[0].delta.annotations[]` | `response.output_text.annotations.added` |
| `usageMetadata.promptTokenCount` | `usage.prompt_tokens` | `response.completed.response.usage.input_tokens` |
| `usageMetadata.candidatesTokenCount` | `usage.completion_tokens` | `response.completed.response.usage.output_tokens` |

### 2.5 Grounding Citations 展开公式(完整可抄)

```python
# litellm/llms/vertex_ai/gemini/vertex_and_google_ai_studio_gemini.py:2110
def _convert_grounding_metadata_to_annotations(grounding_metadata, content_text):
    annotations = []
    for metadata in grounding_metadata:
        chunks = metadata.get("groundingChunks", [])
        supports = metadata.get("groundingSupports", [])

        # Step 1: 建 chunk_idx → {url, title} map
        chunk_to_uri_map = {}
        for idx, chunk in enumerate(chunks):
            if "web" in chunk:
                chunk_to_uri_map[idx] = {
                    "url": chunk["web"].get("uri", ""),
                    "title": chunk["web"].get("title", ""),
                }

        # Step 2: 每个 support 展平成 annotation(只用首个 chunk_index)
        for support in supports:
            segment = support.get("segment", {})
            start_idx = segment.get("startIndex")
            end_idx = segment.get("endIndex")
            chunk_indices = support.get("groundingChunkIndices", [])

            if start_idx is not None and end_idx is not None and chunk_indices:
                first = chunk_indices[0]
                if first in chunk_to_uri_map:
                    uri = chunk_to_uri_map[first]
                    annotations.append({
                        "type": "url_citation",
                        "url_citation": {
                            "start_index": start_idx,
                            "end_index": end_idx,
                            "url": uri["url"],
                            "title": uri["title"],
                        },
                    })
    return annotations
```

**展开规则要点**:
1. 一个 `support` 可挂多个 `chunk_indices`,LiteLLM 只用第一个(可改成展平多条)
2. `segment.startIndex/endIndex` 是字符位置,Codex.app 显示来源时按 `[startIndex:endIndex]` 在原文上叠链接
3. `web` 字段只在网络搜索结果出现(其他 grounding 类型如 retrieval 走别的子 schema)

---

## 3. 本项目架构现状 + 可行性

### 3.1 现有 `apiFormat` 抽象

**已有**:
- `openai_chat`(`crates/adapters/src/openai_chat.rs`):chat completions 标准请求/响应
- `responses`(`crates/adapters/src/responses/{request,response,session,...}.rs`):Responses API → chat completions 转换
- `responses_passthrough`(`crates/adapters/src/passthrough.rs`):Responses 直接字节级透传(用于上游就支持 Responses 的 provider)

**Adapter trait**(`crates/adapters/src/lib.rs`):
```rust
pub trait Adapter: Send + Sync {
    fn prepare_request(&self, client_path: &str, body: Bytes, provider: &Provider) -> Result<Plan>;
    fn process_response(&self, ...) -> Result<...>;
}
```

`Plan { upstream_path, body, ... }` 已经支持任意 endpoint(不绑死 `/chat/completions`),路由层 `crates/proxy/src/forward.rs` 按 `plan.upstream_path` 拼 URL。

### 3.2 加 `gemini_native` 改动清单

#### A. 新建 `crates/adapters/src/gemini_native/`

```
crates/adapters/src/gemini_native/
├── mod.rs              # GeminiNativeAdapter(impl Adapter trait)
├── request.rs          # Responses/chat → generateContent body
│   ├── convert_messages_to_contents()   # role + parts + system 合并
│   ├── convert_tools_to_function_declarations()
│   ├── convert_tool_choice_to_tool_config()
│   ├── inject_google_search_tool()      # web_search → googleSearch (native 才支持)
│   └── build_generation_config()         # temperature/top_p/max_tokens 嵌套
├── response.rs         # streamGenerateContent SSE → chat/Responses SSE
│   ├── parse_gemini_chunk()
│   ├── translate_parts_to_delta_content()
│   ├── translate_function_call_to_tool_calls()
│   ├── translate_grounding_metadata()    # citations 展平公式(2.5 节)
│   └── translate_finish_reason()
└── tests.rs            # 单测 + golden fixture
```

#### B. `crates/adapters/src/registry.rs` 注册

```rust
// 当前(line ~38)
"openai_chat" | "" => Box::new(OpenAIChatAdapter::new(...)),
"responses" => Box::new(ResponsesAdapter::new(...)),
"responses_passthrough" => Box::new(ResponsesPassthroughAdapter::new(...)),
// 加
"gemini_native" => Box::new(GeminiNativeAdapter::new(...)),
```

#### C. Provider auth_scheme 扩展

`crates/adapters/src/types.rs`:
```rust
pub enum AuthScheme {
    Bearer,
    XApiKey,
    GoogleApiKey,  // 新增 — 注 `x-goog-api-key: <key>` header
}
```

`crates/proxy/src/forward.rs` 鉴权注入逻辑加 `AuthScheme::GoogleApiKey` 分支(`req.headers_mut().insert("x-goog-api-key", api_key)`)。

#### D. preset 改 Google AI Studio

`crates/registry/src/presets_data.json`:
```json
{
  "id": "google-ai-studio",
  "name": "Google AI Studio",
  "baseUrl": "https://generativelanguage.googleapis.com/v1beta",
  "authScheme": "google_api_key",
  "apiFormat": "gemini_native",
  ...
  "supportsWebSearch": true,
  "requestOptions": { "web_search_enabled": true }
}
```

注意 `baseUrl` 不再带 `/openai`(native API path 是 `/v1beta/models/{model}:streamGenerateContent`)。

#### E. upstream_path 拼接

`GeminiNativeAdapter::prepare_request` 里:
```rust
let model = body.get("model").and_then(|v| v.as_str()).unwrap_or("");
let stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
let endpoint = if stream { "streamGenerateContent?alt=sse" } else { "generateContent" };
let model_with_prefix = if model.starts_with("models/") { model.to_string() } else { format!("models/{}", model) };
let upstream_path = format!("/{}:{}", model_with_prefix, endpoint);
// 上游 URL = baseUrl + upstream_path = https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-pro-preview:streamGenerateContent?alt=sse
```

### 3.3 工程量估计

| 模块 | LoC(估) | 工时(估) |
|---|---|---|
| `gemini_native/request.rs`(messages/tools/tool_config/generation_config 转换) | 400-500 | 1 天 |
| `gemini_native/response.rs`(SSE 解析 + delta 拼装 + grounding 展开) | 350-450 | 1 天 |
| `gemini_native/mod.rs`(Adapter trait impl) | 100 | 0.5 天 |
| `auth_scheme=GoogleApiKey` 扩展 | 50 | 0.25 天 |
| preset + golden fixture 重生成 | 30 | 0.25 天 |
| 单测(~30 个)+ replay golden | 400 | 1 天 |
| 真实 wire 抓取(需付费配额)+ debug | — | 0.5-1 天 |
| **总计** | **~1300 + tests** | **3-5 天 focused** |

---

## 4. 实施前置条件

### 4.1 必须先解决的问题

1. **付费 Gemini API key**:Free tier 对 gemini-3.1-pro `limit:0`(2026-05-10 实测 429)。Native path 实施前必须有付费 key 抓真 SSE wire(grounding metadata schema 不能靠 mock 推测)
2. **决定 system message 处理**:Gemini 有顶层 `systemInstruction` 字段(单 string),也可以合并到 user content。Codex.app 系统 prompt 巨大(~80KB),用 `systemInstruction` 还是合并到首个 user content 需测试两种 wire 看哪种 input_token 计费更便宜
3. **决定 tool_calls 配对策略**:Gemini `functionCall` 没有 `id` 字段(OpenAI 标准 `tool_call_id` 配对 tool response),需要在转换层维护 sequence number 配对(LiteLLM 用消息顺序隐式配对)

### 4.2 测试方案

1. **单测**:mock chunks(参照 LiteLLM `tests/local_testing/test_amazing_vertex_completion.py`)
2. **Golden fixture replay**:抓真 SSE → 存 `tests/replay/fixtures/gemini_native/{request,response}.json` → 走转换 → diff 期望输出
3. **真机端到端**:配 paid key + Codex.app 触发 web_search query(如"今天纽约天气?引用来源")→ 检查 UI 是否显示 citations 链接

---

## 5. 风险点 + 已知陷阱

1. **role 合并坑**:Codex CLI 历史可能 `[user, user, user, assistant, user, user]` 这种连续重复,Gemini 严格要求 `user, model, user, model` 交替,合并算法必须 robust(LiteLLM 的合并逻辑 transformation.py:311 有完整处理可借鉴)

2. **inlineData base64 大小**:Gemini API 上限 20MB inline,>20MB 必须用 `fileData`(File API 上传)。Codex.app 可能贴大图,需要降级处理(降到 placeholder 或转 fileData)— 类似 DeepSeek `image omitted` 占位

3. **functionCall 没有 id**:OpenAI 标准 `tool_call.id` 用于配对 `tool_call_id`,Gemini 没有该字段。转换时需要 synthesize id(`call_<seq>`)并在历史里维护 mapping;转回 Gemini 时去掉 id

4. **grounding 展开多 chunk**:LiteLLM 只用 `chunk_indices[0]`,但实际一段文字可能引用多个来源。决定:展平成多条 annotation(每个 chunk 一条,共享 segment),还是保留 LiteLLM 的简化路径

5. **streaming finishReason 时机**:Gemini SSE 最后一个 chunk 才带 `finishReason` + `usageMetadata`,需要状态机检测最后 chunk 触发 OpenAI `[DONE]` 终止符。Vertex 跟 AI Studio 行为可能微差,要分别测试

6. **schema 漂移**:Gemini API 还在快速演进(`gemini-3.x` 全是 preview),response schema 可能变。Golden fixture 必须按月 review;CI 加 wire 真实抓取的 nightly 测试

7. **billing 配额监控**:Native path 比 OpenAI compat 计费更明显(独立 `googleSearch` tool 调用计费),代理需要在 telemetry 暴露 `usageMetadata.toolUseCount` 给 UI 显示

---

## 6. 推荐分阶段实施

### Phase 1(MVP,1 PR,~3 天)
- `GeminiNativeAdapter` 框架 + Adapter trait impl
- 请求侧:messages/tools/generation_config 转换(不含 system message 优化)
- 响应侧:基础 text + functionCall + finishReason 翻译(**不做 grounding**)
- preset 切到 `gemini_native`
- 单测覆盖 happy path

### Phase 2(grounding,1 PR,~1 天)
- `translate_grounding_metadata` 实现(Section 2.5 公式)
- 单测 mock + 真实 wire fixture
- UI 验证 citations 显示

### Phase 3(优化 + edge cases,1 PR,~1 天)
- `systemInstruction` vs 合并的 token 开销实测决策
- 大 image inlineData 降级
- `tool_call_id` synthesize 策略 + 历史维护
- 完整 replay test suite

---

## 7. 参考资料

- LiteLLM 关键文件:
  - `litellm/llms/vertex_ai/gemini/vertex_and_google_ai_studio_gemini.py`
  - `litellm/llms/vertex_ai/gemini/transformation.py`
  - `litellm/llms/vertex_ai/common_utils.py`
- 官方文档:
  - [Gemini API REST](https://ai.google.dev/api/rest)
  - [generateContent endpoint](https://ai.google.dev/api/generate-content)
  - [Grounding with Google Search](https://ai.google.dev/gemini-api/docs/google-search)
- 实测来源:
  - PR #94 wire dump:`~/.codex-app-transfer/logs/wire-dumps/b7c514b0-*.req.json`
  - 5 variant curl 实证(本文 §1)
- 本项目相关 PR:
  - PR #88(`responses_passthrough` 透传):跟 native path 同样的 "non-chat upstream" 模式参考
  - PR #94(本 PR):OpenAI compat 接入 + 撤 google_search 注入 + UI 拆 hint
