# Codex App Transfer v1.0.3

> 本版本主线:**让 DeepSeek 实战可用 + 已验证供应商扩容到 5 家**。修掉一批阻断 DeepSeek V4 的协议兼容问题、让 1M 上下文真正在 Codex CLI 端生效、补上上游错误的终止信号让 Codex 不再"假死 thinking"，并修了几处长期存在的 UX 卡点。

## 中文

### 已验证供应商扩展到 5 家

DeepSeek V4、Kimi 月之暗面（Moonshot Platform）、Xiaomi MiMo (Pay for Token) 三家本版本完成端到端测试,从「实验兼容」升级为「已验证供应商」,与上一版的 Kimi Code、Xiaomi MiMo (Token Plan) 合并为完整 5 家:

- **Kimi Code**(`kimi-for-coding` UA 网关)
- **Kimi 月之暗面**(Moonshot Platform API,`api.moonshot.cn`)
- **DeepSeek V4**(含「Max 思维」思考模式 + 1M 上下文)
- **Xiaomi MiMo (Token Plan)**(中国 / 新加坡 / 欧洲三集群)
- **Xiaomi MiMo (Pay for Token)**

`isVerifiedProviderId()` 与添加面板的 unverified banner 同步刷新;选这 5 家中任意一家时,「未端到端验证」黄色提示条不会再出现。

### DeepSeek V4 端到端打通

- **`[1m]` 模型名后缀剥离**:旧版"解锁 1M 上下文"开关把模型名改成 `deepseek-v4-pro[1m]` 当作 1M 变体的内部标记,但上游 DeepSeek 只认 `deepseek-v4-pro` / `deepseek-v4-flash`,直接 400。本版本新增 `outbound_model_id()`,在转发到上游前自动剥掉模型名末尾 `[…]` 修饰后缀,内部仍保留含后缀的 ID 用于 session_cache / response_id 编码。老用户配置不需要清理,自动兜底。
- **删除 `deepseek_1m` / `qwen_1m` 预设开关**:[DeepSeek V4 全系](https://api-docs.deepseek.com/quick_start/pricing)和 [Qwen 3.6 Plus / Flash](https://www.alibabacloud.com/blog/qwen3-6-plus-towards-real-world-agents_603005) 现在默认就是 1M 上下文,这两个手动勾选项已无意义且容易误导用户以为"没开"。两个 `modelOptions` 整段移除。
- **`model_supports_1m()` 改为按 provider_kind 默认判定**:不再依赖模型名后缀,新增内置识别表:DeepSeek `deepseek-v4-*` 和 Bailian `qwen3.6-*` 默认声明 `supports1m: true`。用户手动配置的 `modelCapabilities[].supports1m` 仍然生效,`[1m]` 后缀作为旧版兼容兜底也保留识别。
- **预设页加 1M 默认提示框**:DeepSeek / 阿里云百炼预设打开后,模型映射区下方出现一条蓝色信息条 — "DeepSeek V4 全系模型默认提供 1M 上下文,本应用直接透传,无需额外开关" / "Qwen 3.6 Plus / Flash 默认提供 1M 上下文,本应用直接透传,无需额外开关"。底层用预设的新增 `notices: [{type, text}]` 字段驱动,后续其它预设可复用。

### Codex CLI 端 1M 上下文生效(写 `model_context_window`)

Codex CLI 终端 TUI 默认按内置模型名(`gpt-5.5` 等)推断窗口为 256k,**不读上游 `/v1/models` 返回的 `supports1m` 字段** — 那是 Codex 桌面 App 的能力声明字段。因此即使本工具上游把模型映射成了 1M 模型,Codex CLI 仍按 256k 显示,会触发提前压缩。

本版本在「一键应用」时根据激活 provider 的 default 模型是否支持 1M,自动往 `~/.codex/config.toml` 写 `model_context_window = 1000000`,不支持时反向删除该键。配套基础设施改动:

- `_sync_codex_toml_value` 升级为接受**已格式化的 TOML 字面量**(让调用方决定字符串带引号 / 整数裸写),不再硬走 `json.dumps`
- `_snapshot_toml_managed_values` 改为存原始 raw 字符串,restore 时原样回写,避免 string ↔ int 在 round-trip 中走样
- `_MANAGED_TOML_KEYS` 加入 `model_context_window`,「还原 Codex 原配置」时正确清理

参考 [Codex CLI Configuration Reference](https://developers.openai.com/codex/config-reference) 与 [issue #19185](https://github.com/openai/codex/issues/19185)。

### DeepSeek `response_format: json_schema` 自动剥离

Codex CLI 在 plan / tool schema 场景会发 `response_format: {"type": "json_schema", ...}`,但 [DeepSeek 官方 JSON Mode](https://api-docs.deepseek.com/guides/json_mode) 仅支持 `{"type": "json_object"}`,收到 `json_schema` 直接返回 400 `This response_format type is unavailable now`。

本版本在 `provider_workarounds.apply_request_workarounds` 的 DeepSeek 分支新增 `deepseek_strip_unsupported_response_format()`:遇到 `json_schema` 类型整段剥掉,保留 `json_object`(DeepSeek 支持)。**不降级为 `json_object`** 是有意为之 — DeepSeek 的 `json_object` 模式强制要求 prompt 含 "json" 字样,Codex prompt 不一定满足,强行降级会触发二次 400。

### 流式 read timeout 调整(支持 reasoning 模型长时思考)

`get_http_client()` 用的是全局 `httpx.Timeout(120.0, connect=30.0)` —— 第一个位置参数会同时赋给 read / write / pool。reasoning 模型(Kimi k2.6、DeepSeek max thinking 等)在思考阶段两个 SSE chunk 之间的 idle gap 经常 > 120s,被默认配置 `ReadTimeout` 误杀,用户感知就是"思考过程一直累积到最后超时"。

本版本流式请求显式传入独立 timeout `httpx.Timeout(connect=30.0, read=600.0, write=120.0, pool=120.0)`,read 给 10 分钟覆盖绝大多数 reasoning 场景;非流式仍走 120s 默认值。

### 上游错误 / 客户端异常后 Codex 一直 thinking 的修复

上游返回非 2xx(余额不足 402、请求体不合规 400 等)或客户端侧抛 `httpx.TimeoutException` / 任意异常时,本工具原先只发一个 `{type: "error"}` 事件就 return,但 Codex CLI 通过 WebSocket 接收时**只把 `response.failed` / `response.completed` / `response.incomplete` 当作流终止信号**。结果是 error 事件被发出但 Codex spinner 不停。

本版本上游错误分支和客户端异常分支**两条路径都修齐**,同时 yield 两个事件:

- `{type: "error", sequence_number: 0, error: {…}}` — 错误内容,供 Codex 显示
- `{type: "response.failed", sequence_number: 1, response: {status: "failed", error: {…}, …}}` — 终止信号,让 Codex 停止 thinking

同时尝试解析上游错误体的 `message` / `code` 字段(给用户更友好的提示);402 自动归类为 `insufficient_quota` code,客户端 `TimeoutException` 归 `timeoutexception`,其它异常归 `stream_error`。

### 「获取模型」改为只填 default 槽

旧逻辑会把所有 6 个槽位(`gpt_5_5` / `gpt_5_4` / `gpt_5_4_mini` / `gpt_5_3_codex` / `gpt_5_2` / `default`)都填同一个模型 ID,UI 上一片重复,用户改起来很烦。本版本只填 `default` 槽(优先选含 `pro` / `plus` / `coder` / `max` / `reasoner` / `v4` 关键字的旗舰模型,否则取第一个可用模型),其它槽位留空 — 请求时通过 default 降级机制兜底。需要差异化映射的高级用户手动加即可。

### 「正在应用」按钮改为提示框,「启用」按钮恢复可点

老版本 active provider 的「启用」按钮变成不可选中的「默认」,导致用户想重新点一下"再确认应用"做不到。本版本拆成两个 UI 元素:

- 一个绿色 `.active-indicator` 小提示框(带呼吸动画 + broadcast 图标 + "正在应用" / "Active" 文案)放在按钮左侧
- 「启用」按钮始终可点击,文字恒为「启用」 — 重新点会再走一次 `setDefaultProvider` → `startProxy` → 弹 toast 确认,正是用户想要的"再确认一次真的应用到本地"

### Bug fix

- **编辑已有 provider 时强制跳到 Responses 协议**:`api.js:130` 在 `getPresets()` 时把 `apiFormat` 标准化成大写字符串 `"OpenAI"` / `"Responses"`,但 `app.js:1105` 编辑路径的白名单只包含小写 `["openai", "openai_chat"]`,匹配失败 → 兜底到 `"responses"`,导致已配置的 OpenAI Chat 协议 provider 一打开编辑就被错误回退。已与新建路径对齐,白名单同时包含大小写。
- **provider 测速对 Kimi 月之暗面误报"认证失败"**:`https://api.moonshot.cn/v1/chat/completions` 上 HEAD/GET 都返回 404,本工具回退到带 body 的 POST 探测;但 `_provider_test_model()` 选的是 OpenAI 端的模型 ID(`gpt-5.5`),Moonshot 不认识 → 401,被误判成"Kimi 认证失败"(实际 key 是有效的,所以同一时段的"获取模型"`/v1/models` GET 能成功)。已改为优先用 provider `models.default` 的真实模型 ID(`kimi-k2.6` / `deepseek-v4-pro` 等),所有槽位都为空时才回退到 OpenAI ID。

## English

> Two themes this release: **make DeepSeek actually usable**, and **the verified-providers list grows to 5**. Fixes a batch of protocol-compatibility issues blocking DeepSeek V4, makes the 1M context window actually take effect on the Codex CLI side, supplies the missing termination signal so Codex stops "ghost-thinking" after upstream errors, and cleans up several long-standing UX papercuts.

### Verified-providers list expanded to 5

DeepSeek V4, Kimi Moonshot (Platform API), and Xiaomi MiMo (Pay for Token) all passed end-to-end testing this release, joining last version's Kimi Code and Xiaomi MiMo (Token Plan) for a full lineup of 5:

- **Kimi Code** (`kimi-for-coding` UA gateway)
- **Kimi Moonshot** (Moonshot Platform API at `api.moonshot.cn`)
- **DeepSeek V4** (with "Max thinking" mode + 1M context)
- **Xiaomi MiMo (Token Plan)** (China / Singapore / Europe clusters)
- **Xiaomi MiMo (Pay for Token)**

`isVerifiedProviderId()` and the add-provider panel's unverified banner are updated accordingly — pick any of these five and the yellow "not end-to-end verified" notice no longer appears.

### DeepSeek V4 end-to-end fixes

- **Strip `[1m]` model-name suffix on the wire**: the legacy "Unlock 1M context" toggle would rewrite the model name to `deepseek-v4-pro[1m]` as an internal marker for the 1M variant — but DeepSeek upstream only accepts `deepseek-v4-pro` / `deepseek-v4-flash`, so requests 400'd outright. This release adds `outbound_model_id()`, which strips the trailing `[…]` decoration before forwarding while keeping the suffixed ID internally for session_cache / response_id encoding. Existing user configs need no manual cleanup — the strip is automatic.
- **Removed `deepseek_1m` / `qwen_1m` preset toggles**: [DeepSeek V4 across the board](https://api-docs.deepseek.com/quick_start/pricing) and [Qwen 3.6 Plus / Flash](https://www.alibabacloud.com/blog/qwen3-6-plus-towards-real-world-agents_603005) both default to 1M context now. The manual toggles were not only useless but actively misleading (users would assume "1M is off"). Both `modelOptions` blocks are gone.
- **`model_supports_1m()` now keys on provider_kind**: instead of relying on a model-name suffix, there's a built-in lookup table — DeepSeek `deepseek-v4-*` and Bailian `qwen3.6-*` declare `supports1m: true` automatically. User-set `modelCapabilities[].supports1m` still works; the legacy `[1m]` suffix is still recognized as a fallback for old configs.
- **Preset notice for 1M default**: choosing the DeepSeek or Bailian preset now shows a blue info banner under the model-mapping section: "DeepSeek V4 全系模型默认提供 1M 上下文,本应用直接透传,无需额外开关" / "Qwen 3.6 Plus / Flash 默认提供 1M 上下文,本应用直接透传,无需额外开关". Driven by a new `notices: [{type, text}]` field on presets — other presets can reuse it.

### Make 1M context window actually take effect (write `model_context_window`)

The Codex CLI terminal TUI infers context windows from the built-in model name (`gpt-5.5` etc., ~256k by default) and **does not read the upstream `/v1/models` `supports1m` field** — that's a capability declaration for the Codex desktop app. So even when this tool maps to a 1M-capable upstream model, Codex CLI would still treat it as 256k and trigger premature compaction.

This release writes `model_context_window = 1000000` to `~/.codex/config.toml` on apply, when the active provider's default model supports 1M; deletes the key otherwise. Plumbing changes:

- `_sync_codex_toml_value` now takes a **pre-formatted TOML literal** (caller decides whether to add quotes for strings or write a bare integer), no longer hard-coded `json.dumps`
- `_snapshot_toml_managed_values` now stores the raw post-`=` text and writes it back verbatim on restore, avoiding string ↔ int round-trip drift
- `_MANAGED_TOML_KEYS` includes `model_context_window`, so "Restore Codex original config" cleans it up correctly

References: [Codex CLI Configuration Reference](https://developers.openai.com/codex/config-reference), [issue #19185](https://github.com/openai/codex/issues/19185).

### Auto-strip `response_format: json_schema` for DeepSeek

Codex CLI sends `response_format: {"type": "json_schema", ...}` in plan / tool-schema scenarios. [DeepSeek's official JSON Mode](https://api-docs.deepseek.com/guides/json_mode) only supports `{"type": "json_object"}` and returns a 400 `This response_format type is unavailable now` for `json_schema`.

This release adds `deepseek_strip_unsupported_response_format()` to the DeepSeek branch of `provider_workarounds.apply_request_workarounds`: removes the field entirely when type is `json_schema`, preserves it when type is `json_object` (which DeepSeek supports). **No automatic downgrade to `json_object`** — DeepSeek's `json_object` mode requires the prompt to contain the literal word "json", which Codex prompts don't always satisfy, so a forced downgrade would just trigger a second 400.

### Streaming read-timeout bumped (room for long reasoning)

`get_http_client()` was using a global `httpx.Timeout(120.0, connect=30.0)` — the first positional argument is also assigned to read / write / pool. Reasoning models (Kimi k2.6, DeepSeek max thinking, etc.) often have idle gaps > 120s between SSE chunks during the thinking phase, hitting `ReadTimeout`. From the user side it looks like "the thinking output piles up and finally times out".

Streaming requests now pass an explicit per-call `httpx.Timeout(connect=30.0, read=600.0, write=120.0, pool=120.0)` — read window of 10 minutes covers the vast majority of reasoning scenarios; non-streaming still uses the 120s default.

### Codex no longer hangs in "thinking" after upstream errors / client exceptions

When the upstream returned a non-2xx (insufficient balance 402, malformed request 400, etc.) **or** the client side raised `httpx.TimeoutException` / any other exception, this tool used to send only a `{type: "error"}` event and return — but Codex CLI's WebSocket handler **only treats `response.failed` / `response.completed` / `response.incomplete` as stream-termination signals**. Result: the error event fired but Codex's spinner never stopped.

Both the upstream-error branch and the client-exception branch now yield two events:

- `{type: "error", sequence_number: 0, error: {…}}` — error payload, for Codex to display
- `{type: "response.failed", sequence_number: 1, response: {status: "failed", error: {…}, …}}` — termination signal, so Codex stops thinking

Best-effort parsing of the upstream error body's `message` / `code` for friendlier display; HTTP 402 is auto-classified as `insufficient_quota`, client `TimeoutException` as `timeoutexception`, others as `stream_error`.

### "Fetch models" now fills only the default slot

Old behavior: filled all 6 slots (`gpt_5_5` / `gpt_5_4` / `gpt_5_4_mini` / `gpt_5_3_codex` / `gpt_5_2` / `default`) with the same model ID. UI was a wall of duplicates that users had to manually clean up. Now it fills only `default` (preferring flagship-keyword models — `pro` / `plus` / `coder` / `max` / `reasoner` / `v4` — falling back to the first available), leaving other slots empty. Requests use the default-fallback path; advanced users can still set per-slot mappings manually.

### "Active" indicator + "Enable" button stays clickable

Previously the active provider's "Enable" button became a disabled "Default" pill, so users couldn't re-click to re-confirm the apply. Now split into two UI elements:

- A green `.active-indicator` pill (subtle pulse animation + broadcast icon + "正在应用" / "Active" text) sits to the left of the button
- The "Enable" button stays clickable with its label unchanged — re-clicking re-runs `setDefaultProvider` → `startProxy` → toast, exactly the "let me verify it actually applied" affordance users wanted

### Bug fix

- **Editing an existing provider was force-switched to the Responses protocol**: `api.js:130` normalizes preset `apiFormat` to capitalized `"OpenAI"` / `"Responses"`, but the edit path's whitelist at `app.js:1105` only contained lowercase `["openai", "openai_chat"]`. Match failed → fell through to `"responses"`, so any pre-configured OpenAI Chat provider would silently flip to Responses on open. Aligned with the new-add path; whitelist now contains both casings.
- **Provider speed-test misreported "auth failed" for Kimi (Moonshot)**: `https://api.moonshot.cn/v1/chat/completions` returns 404 for both HEAD and GET, so the tool fell back to a POST probe with a body — but `_provider_test_model()` was picking an OpenAI-side model ID (`gpt-5.5`) that Moonshot doesn't recognize, returning 401 and being misclassified as "Kimi auth failed" (the key was actually fine — same-time `/v1/models` GET succeeded). Now picks `provider.models.default` (`kimi-k2.6`, `deepseek-v4-pro`, etc.) first and only falls back to OpenAI IDs when every slot is empty.
