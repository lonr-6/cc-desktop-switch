# Codex App Transfer v1.0.1

> 开发中。本文件随每次合入持续更新；正式发布时再做最终定稿和翻译。

## 中文

### 多家提供商切换到 OpenAI Chat 兼容路径

- **DeepSeek、智谱 GLM、阿里云百炼、Xiaomi MiMo (PAYG / Token Plan)** 五家内置预设的默认 `apiFormat` 改为 `openai_chat`，转发路径统一为 `<baseUrl>/chat/completions`，与已端到端验证的 Kimi 路径一致。
- 智谱 GLM、阿里云百炼默认认证由 `x-api-key` 改为 `Authorization: Bearer`；DeepSeek 默认 baseURL 由 `/anthropic` 改为 `/v1`。
- 已有 `apiFormat: "responses"` 的存量配置不会被自动迁移，需手动在编辑页重新选预设。

### 编辑页新增「未端到端验证」提示

选择非 Kimi 系列的预设或编辑非 Kimi 提供商时，表单顶部显示「目前仅 Kimi Code 端到端验证」的提示横幅。

### DeepSeek V4 思维模式适配

「DeepSeek Max 思维」开关现支持 chat/completions 路径，请求体按 [DeepSeek V4 Thinking Mode 文档](https://api-docs.deepseek.com/guides/thinking_mode) 对齐。

### 修复 Windows 启动崩溃

v1.0.0 Windows 安装版启动报 `ModuleNotFoundError: jaraco.text` 已修复，原因是 PyInstaller 配置遗漏了 `pkg_resources` 的 vendored 依赖。

### 移除遗留的 CC-Switch 导入功能

清理从原项目带过来、本项目不再使用的 CC-Switch 导入按钮、Settings 面板及对应后端代码。已存在的 `cc-switch` 来源 provider 数据仍可正常使用。

### Codex CLI 原配置自动还原

- 第一次「应用配置」前自动把 `~/.codex/config.toml` 与 `auth.json` 备份到 `~/.codex-app-transfer/codex-snapshot/`；顶栏按钮由「清除配置」改名为**「还原 Codex 原配置」**，行为升级为按 key 智能合并 —— 仅回滚我们写过的字段（`auth_mode`、`OPENAI_API_KEY`、`openai_base_url`），其他内容原样保留。
- 应用退出 / 下次启动会自动还原一次，保证不开应用时 Codex CLI 仍是用户原配置。Settings 页新增「退出时还原 Codex 原配置」开关，默认开启；关掉可恢复 1.0.0 的"配置常驻"行为。
- **升级警告**：1.0.0 升级到 1.0.1 之前已经 apply 过的用户，原 `auth_mode` / 自定义 `openai_base_url` / 自有 `OPENAI_API_KEY` 已被静默覆盖且无法自动恢复。ChatGPT 登录用户建议检查 `~/.codex/auth.json` 的 `auth_mode` 是否被遗留为 `"apikey"`，必要时执行一次 `codex login`。

### 启动自动应用 + 退出综合清理

- 应用启动时按当前 active provider 自动写入 `~/.codex/`；如该 provider 走 chat/completions 路径会同时启动转发服务,不再需要每次手动点「应用配置」。
- Settings「开机自启」复选框改名为**「启动时自动应用配置」**，默认开启；关掉可保留 1.0.0 的"启动后等用户手动操作"行为。
- 应用退出时先停转发服务（避免端口残留进程），再按设置还原 Codex 原配置；切到不需要转发的 provider 时也会主动停掉空跑的转发。

### 修复 previous_response_id 多轮对话上下文丢失

- **影响**: 与 Kimi / DeepSeek 等 OpenAI Chat 兼容上游进行多轮工具调用对话从第 2-3 轮起,模型会出现"忘记用户原始问题、回复 'I'm here, what would you like me to help you with today?' 或类似通用问候、后续工具调用走向无关路径"等彻底失忆表现 —— 整个对话上下文被悄无声息丢光。
- **根因**: 响应阶段我们用 `response_id_codec.encode_response_id()` 把上游 `chatcmpl-xxx` 编码成 litellm 风格的 `resp_<base64>` 发给客户端(给 deployment affinity / 多 provider 路由用)。客户端把 encoded 形式作为 `previous_response_id` 回传,但 `session_cache` 的 key 是**原始的 `chatcmpl-xxx`** —— **100% cache miss**,messages 只剩当前轮的孤儿 tool 消息,`_repair_tool_call_ids` 默默插入占位 assistant + 从 `TOOL_CALLS_CACHE` 取 tool_call 定义,最终发给上游的是**没有 user message 的合成对话**,模型只能给出礼貌的通用问候。
- **修复**: cache 查询前先调 `decode_response_id()` 还原回原始 `chatcmpl-xxx`。3 行代码,等价于 litellm `session_handler.py:284-291` 的 `_decode_responses_api_response_id` 标准做法。本修复同时让前面三类 thinking 异常修复(单空格 reasoning_content / reasoning_summary_part 协议 / TOOL_CALLS_CACHE 重建)真正发挥作用 —— 没有这条,前面三个修复都因为 cache miss 而触发不到。

### 修复 Kimi / DeepSeek thinking 模式三类异常

- **共同影响范围**：使用 Kimi（含 Kimi Code，服务端默认开 thinking）或 DeepSeek 且「DeepSeek Max 思维」开关 ON 时撞上。其他 provider（智谱 GLM、阿里云百炼非 thinking 模型、Xiaomi MiMo 等）不受影响。
- **深层对话 `400 reasoning_content is missing`**：Codex CLI 历史里的 `reasoning` items 有时只带不透明的 `encrypted_content`、或被历史压缩剔除；我们之前补的占位 `reasoning_content` 是空字符串，被上游判为"缺失"。已改成单空格占位（非空、token 信号近乎为零），并把判断条件从"键不存在"改为"strip 后为空"，确保已写入的空串也被纠正。
- **思维内容流式 UI 卡住、体感慢**：streaming adapter 之前给 reasoning item 的 part 起点 / 收尾事件用了通用的 `response.content_part.added/done` + `content_index`,但 Codex CLI 严格匹配 `response.reasoning_summary_part.added/done` + `summary_index`,导致 reasoning item 出现后等不到对应的 summary part 起点,后续大量 `reasoning_summary_text.delta` 全部无处挂载被丢弃,UI 卡在 "Thinking..." 直到最终答案到来才整体闭合 —— 表现为"特别慢"。已改为正确事件名 + summary_index + part.type=summary_text,思维内容现在实时逐字展开。
- **思考结束后工具调用上游 400 `tool_call_id  is not found`**：Codex CLI 历史经过压缩后偶尔会把 `function_call_output` 的 `call_id` 字段丢掉,我们之前直接把空字符串透传给上游,Kimi 拼回错误模板时出现了招牌的双空格(`tool_call_id  is`),导致紧随思考之后的工具回报失败、最终答案无法显示。已加双层防御:① 提取阶段 fallback `call_id → tool_call_id → id`；② 消息合并后扫一遍,空 `tool_call_id` 按位置从前面紧邻的 assistant `tool_calls` 补上,无可配对的孤儿 tool message 直接丢弃避免上游 400。

## English

> Drafted in Chinese first. English notes will be finalized at release time.

## 参考链接

- [DeepSeek API Docs](https://api-docs.deepseek.com/)
- [DeepSeek V4 Preview Release](https://api-docs.deepseek.com/news/news260424)
- [DeepSeek Thinking Mode](https://api-docs.deepseek.com/guides/thinking_mode)
- [智谱 BigModel OpenAI 兼容](https://docs.bigmodel.cn/cn/guide/develop/openai/introduction)
- [阿里云百炼 OpenAI Chat 接口兼容](https://help.aliyun.com/zh/model-studio/compatibility-of-openai-with-dashscope)
- [Xiaomi MiMo 平台](https://www.mimo-v2.com/zh/docs/quick-start/first-api-call)
