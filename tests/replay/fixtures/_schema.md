# Fixture Schema(Python ↔ Rust 契约)

每个 fixture 是一个 JSON 文件,描述**一次完整的 client → proxy → upstream → proxy → client** 调用。Python 与 Rust 两套实现共享同一份 fixture,任一边改动都不应让 fixture 测试出现退化。

## 文件命名

`<provider>_<scenario>_<modifier>.json`,例如:

- `openai_chat_simple_streaming.json`
- `deepseek_responses_tool_call.json`
- `kimi_chat_long_context.json`

`_` 开头的 JSON 视为内部样例,被 `list_fixtures()` 忽略。

## 顶层字段

```jsonc
{
  "name": "openai_chat_simple_streaming",     // 必填,与文件名一致
  "description": "...",                        // 选填,人类可读
  "provider": "openai",                        // 必填,与 backend/api_adapters.py 中的 key 对齐
  "notes": "...",                              // 选填,记录此 fixture 抓取时的特殊条件

  "client_request": { ... },                   // 必填
  "upstream": [ ... ],                         // 必填,可为空数组(纯本地路由用)
  "expected": { ... }                          // 必填
}
```

## `client_request`

```jsonc
{
  "method": "POST",
  "path": "/v1/chat/completions",
  "headers": { "content-type": "application/json", "authorization": "<redacted>" },
  "body_json": { ... },        // 二选一
  "body_text": "..."           // 二选一
}
```

## `upstream`

数组,每项描述代理向某个上游发起的一次调用。

```jsonc
{
  "url_pattern": "https://api.openai.com/v1/chat/completions",
  // 也可写正则,前缀 "re:" 显式标记;否则尝试当 regex 编译,失败回退为字面量
  "method": "POST",
  "response": {
    "status": 200,
    "headers": { "content-type": "text/event-stream" },
    // 三种 body 形态选其一:
    "body_json": { ... },
    "body_text": "...",
    "stream": [
      { "data": "data: {...}\n\n", "delay_ms": 0 }
    ]
  }
}
```

**SSE 帧约定**:`data` 字段保留**原始字节序列**(含 `data: ` 前缀和结尾 `\n\n`)。`delay_ms` 用于未来重放真实节奏(当前 player 不强制等待)。

## `expected`

任意字段组合,所有出现的字段都必须满足:

```jsonc
{
  "status": 200,                                  // 严格相等
  "headers_contain": { "content-type": "text/event-stream" }, // 子串匹配
  "body_json": { ... },                            // 严格相等(dict)
  "body_text": "...",                              // 严格相等
  "body_substrings": ["chatcmpl-", "[DONE]"],      // 必须都出现
  "stream_frames": [                               // 拼接后与 body_text 严格相等
    { "data": "data: ...\n\n" }
  ],
  "stream_substrings": ["\"content\":\"hi\""]      // 必须都出现
}
```

**最小约束**:首版 fixture 推荐用 `status` + `headers_contain` + `stream_substrings`,因为 SSE 上的字段顺序、`id` 自增等差异在 Python ↔ Rust 间不必死磕。等到 Stage 2/3 末期再逐步把宽松断言收紧为 `stream_frames` 严格匹配。

## 脱敏

录制时 recorder 自动把 `authorization` / `x-api-key` / `api-key` / `cookie` / `set-cookie` 替换为 `<redacted>`。**手动检查 fixture 中是否还有 token / 长度异常的 base64 / `sk-...` / `Bearer ...`** 后再提交。
