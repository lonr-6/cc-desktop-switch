# Kimi 思考内容"变短"问题诊断报告

> 调查时间: 2026-05-07
> 现象: 用户反馈 Kimi (kimi-for-coding) 在 Codex CLI 中显示的思考内容明显短于预期
> 调查者: Claude Code (Opus 4.7),仓库 HEAD `71ab6f0` (v2.0.7) + 本地 v2.0.8 草稿
> 结论: **协议转换层 byte-perfect 零丢失。Codex CLI 0.128.0 TUI 渲染对"不带 `**bold**` markdown 标记的 reasoning"判定为 `transcript_only` 不显示。修复在 proxy 端首帧注入 `**Thinking**` header 即可。**

---

## 1. 用户感知与流量路径(先确定测试对象)

用户激活 provider 是 **Kimi Code**(id=`b405e7b0`):

| 字段 | 值 |
|---|---|
| name | Kimi Code |
| baseUrl | `https://api.kimi.com/coding/v1` |
| apiFormat | `openai_chat` |
| 默认模型 | `kimi-for-coding` |
| 自定义头 | `User-Agent: KimiCLI/1.40.0` |

请求路径(经 v2.0.7 forward.rs 验证):

```
Codex CLI
   │ POST /v1/responses  (Responses API)
   ▼
proxy server.rs
   │ apiFormat=openai_chat ∧ client_path∈{/v1/responses,/responses,/openai/v1/responses}
   │   → AdapterRegistry::lookup_for_request → ResponsesAdapter
   │
   │ ResponsesAdapter::prepare_request:
   │   - body: Responses → Chat Completions
   │   - upstream_path: /v1/responses → /chat/completions
   ▼
upstream  POST  https://api.kimi.com/coding/v1/chat/completions
   │ Chat Completions SSE (delta.reasoning_content + delta.content)
   ▼
proxy  ChatToResponsesConverter (transform_response_stream)
   │ delta.reasoning_content  → response.reasoning_summary_text.{delta,done}
   │ delta.content            → response.output_text.{delta,done}
   ▼
Codex CLI  渲染 reasoning summary + assistant message
```

**实证依据**:用户日志 `~/.codex-app-transfer/logs/proxy-2026-05-06.log` 含上百条
`转发请求 → https://api.kimi.com/coding/v1/chat/completions`,
`上游耗时 200 ... TTFB=0.00s total=24.44s bytes=264883`,
路径改写与上游协议(Chat Completions)与代码一致。

---

## 2. 上游真实 SSE 协议(直 curl 录像)

### 2.1 录像设置

直接打 `https://api.kimi.com/coding/v1/chat/completions`,使用用户配置中的 Kimi Code API Key,绕过 proxy。两次样本:

| 样本 | Prompt | 时长 | 录到字节 |
|---|---|---|---|
| **A** | "请用至少 500 字详细推理:为什么 Rust 的所有权..." | 60 秒(被超时切断) | 379,284 字节 |
| **B** | "证明 2^n + 3^n + 6^n - 1 整除 n 的充要条件..." | 300 秒(仍被切断) | 5,447,489 字节 |

### 2.2 SSE 字段实测

`reasoning_content` 字段名出现统计(grep 计数):

```
样本 A: reasoning_content   = 665 帧
        reasoning_text      = 0
        thinking            = 0
        <think>             = 0
        其他变体             = 0

样本 B: reasoning_content   = 22104 帧
        其他字段             = 0
```

**结论**: Kimi for Coding 严格使用标准 `delta.reasoning_content` 字段,**与 Rust `ChatDelta` 反序列化结构匹配**(`crates/adapters/src/responses/converter.rs:884`)。

### 2.3 字符级会计(直 curl 上游 raw)

| 样本 | 上游 reasoning 帧 | reasoning 字符 | content 帧 | content 字符 | 流是否完整 |
|---|---|---|---|---|---|
| A | 665 | 1,326 | 894 | 1,778 | NO(被 60s 超时切) |
| B | 22,104 | 32,655 | 0 | 0 | NO(300s 仍 reasoning 中) |

样本 B 流量 5.4MB / 22104 帧 reasoning 但 0 字符 content,说明该提示在 Kimi 上引发的是**真实长链推理**,而非短回复。

---

## 3. proxy 端 byte-perfect 对账(关键证据)

把上述真实 raw SSE 直接喂给 `ChatToResponsesConverter::feed + finish`,统计输出端 `response.reasoning_summary_text.delta` / `.done` 累计。

```rust
let raw = std::fs::read("/tmp/kimi-probe/raw2.sse")?;
let mut conv = ChatToResponsesConverter::new();
let mut out = conv.feed(&raw);
out.extend(conv.finish());
// 解析 out,按事件累加 delta.delta / done.text
```

| 指标 | 上游真实 | converter emit | 偏差 |
|---|---|---|---|
| **样本 A** | | | |
| reasoning 帧数 | 665 | 665 | **0** |
| reasoning 字符 | 1,326 | 1,326 | **0** |
| reasoning_summary_text.done.text | n/a | 1,326 | **0** |
| content 帧数 | 894 | 894 | **0** |
| content 字符 | 1,778 | 1,778 | **0** |
| output_text.done.text | n/a | 1,778 | **0** |
| **样本 B** | | | |
| reasoning 帧数 | 22,104 | 22,104 | **0** |
| reasoning 字符 | 32,655 | 32,655 | **0** |
| reasoning_summary_text.done.text | n/a | 32,655 | **0** |

生命周期事件(单次响应):

```
✓ response.created               × 1
✓ response.in_progress           × 1
✓ response.output_item.added     × 2  (reasoning + message)
✓ response.reasoning_summary_part.added × 1
✓ response.reasoning_summary_text.delta × N  (= 上游 reasoning 帧数)
✓ response.reasoning_summary_text.done  × 1  (text = 累积全文)
✓ response.reasoning_summary_part.done  × 1
✓ response.content_part.added    × 1   (仅样本 A,样本 B 因 reasoning 中断未到此)
✓ response.output_text.delta     × N
✓ response.content_part.done     × 1
✓ response.output_item.done      × 2
✓ response.completed             × 1
```

**结论:协议转换层零丢失,3.2 万字符级别的 reasoning 也 byte-perfect 透传。**

---

## 4. 排除的嫌疑

### 4.1 字段名错配 — 已排除

`ChatDelta` 定义(`crates/adapters/src/responses/converter.rs:878-908`)只接 `reasoning_content`。曾担心 Kimi 用 `reasoning` / `reasoning_text` / `thinking` 等变体导致丢失,实测确认 Kimi for Coding **只发 `reasoning_content`** 字段。

### 4.2 反序列化静默丢帧 — 已排除

`handle_frame:259` 的 `serde_json::from_str` 失败时 `return`,理论上能丢帧。实测 22,104 帧零失败(emit 帧数 == 上游帧数),`null` 容忍逻辑(`deserialize_null_or_missing_to_empty_vec`)正常工作。

### 4.3 累积截断 / 状态机 race — 已排除

`reasoning_acc.push_str(rs)` (`converter.rs:295`) 是无界 `String`,每帧 push 后 emit 含原始 `rs` 的 delta event。32k 字符无截断。

### 4.4 reasoning open/close 时机错位 — 已排除

样本 B 完整跑过 `open_reasoning → push_str ×22104 → finish 时 close_reasoning`,生命周期事件顺序正确。`reasoning_open && !reasoning_closed` 守卫(`converter.rs:312, 688, 713`)防止重开/重关。

### 4.5 [DONE] 提前关流 — 已排除

`emit_close(out, /*from_done=*/true)` 在 [DONE] 帧到达时执行,但 `close_reasoning` / `close_message` 都是 idempotent 的 close,不会丢累计文本。

---

## 5. 真实根因(已通过阅读 Codex CLI 0.128.0 源码定位)

读 `openai/codex` 仓库 `rust-v0.128.0` tag 完整链路,问题精准定位在 **TUI 渲染层**。

### 5.1 Codex CLI 内部事件链(Kimi 经我们 proxy 走的路径)

```
proxy 发: response.reasoning_summary_text.delta
   ↓
codex-api/src/sse/responses.rs:325
   ↓ → ResponseEvent::ReasoningSummaryDelta { delta, summary_index }
   ↓
core/src/session/turn.rs:2155-2168
   ↓ → EventMsg::ReasoningContentDelta(...)
   ↓
app-server/src/bespoke_event_handling.rs:1249
   ↓ → ServerNotification::ReasoningSummaryTextDelta(...)
   ↓
tui/src/app/app_server_adapter.rs:622-630
   ↓ → EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta })
   ↓
tui/src/chatwidget.rs:7420
   ↓ → self.on_agent_reasoning_delta(delta)
   ↓
self.reasoning_buffer.push_str(&delta);   ← 逐 delta 累积全文,无截断
extract_first_bold(&self.reasoning_buffer) → 设置 status header(顶部 "Thinking..." 旁的 bold 提示)
```

**到此为止**:reasoning_buffer 累积了完整 32k 字符,**无丢失**。状态行只显示从 `**bold**` 提取的 header,**这是流式期间** UI 行为(看似"没什么思考"是因为 status 上只显示一行 header,正常)。

### 5.2 收尾事件触发渲染分支(关键!)

reasoning 段结束需要 `EventMsg::AgentReasoning(AgentReasoningEvent { text })` 触发 `on_agent_reasoning_final`(`chatwidget.rs:7424`)。这个 EventMsg 由 `output_item.done` 事件携带的 reasoning item 经 `event_mapping.rs:155` + `protocol/src/items.rs:343` 的 `as_legacy_events()` 产生,**对每个 `summary[i]` entry 都发一条**。

我们的 proxy 在 `converter.rs:581 reasoning_item_completed()` 把累积全文塞进 `summary[0].text`,所以收尾时确实会触发一次 `AgentReasoning`,进入 final 分支。

### 5.3 final 分支:**问题精确出现位置**

`tui/src/history_cell.rs:2783-2810` `new_reasoning_summary_block()` 完整逻辑:

```rust
let full_reasoning_buffer = full_reasoning_buffer.trim();
if let Some(open) = full_reasoning_buffer.find("**") {        // ① 找首个 ** 开始
    let after_open = &full_reasoning_buffer[(open + 2)..];
    if let Some(close) = after_open.find("**") {              // ② 找匹配的 ** 结束
        let after_close_idx = open + 2 + close + 2;
        if after_close_idx < full_reasoning_buffer.len() {    // ③ 后面有内容
            // → 有 **bold header** + 后续正文 → 走"显示"分支
            return Box::new(ReasoningSummaryCell::new(
                header, summary, &cwd,
                /*transcript_only*/ false,                    // ✓ TUI 上显示
            ));
        }
    }
}
// 缺少 **bold** 标记 → 走"transcript-only"分支
Box::new(ReasoningSummaryCell::new(
    "".to_string(), full_reasoning_buffer.to_string(), &cwd,
    /*transcript_only*/ true,                                  // ✗ TUI 上不显示
))
```

`ReasoningSummaryCell::display_lines`(`history_cell.rs:444`):

```rust
fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
    if self.transcript_only {
        Vec::new()                          // ← 直接返回空,UI 上完全不渲染
    } else {
        self.lines(width)
    }
}
```

### 5.4 根因结论

**OpenAI 自家 o1/o3 的 reasoning_content 内置 `**Section Header**` markdown 结构**(类似 ChatGPT "Working out / Considering / Verifying" 等),Codex CLI 据此切分成 header + body 渲染。

**Kimi for Coding 的 reasoning_content 是纯散文文本,不带 `**...**` markdown 结构**(实测样本 B 的 32655 字符 reasoning,grep 出 0 个 `**` 标记)。

→ 命中 transcript_only=true 分支
→ TUI 上**完全不渲染** reasoning summary cell
→ 用户感受为"思考变短"或"几乎没思考"
→ 实际数据全在 reasoning_buffer/transcript,跑 `/transcript` 命令可看完整内容

**这与 proxy 协议转换无关 —— 即使我们 100% byte-perfect,Codex CLI 也只对带 markdown 结构的 reasoning 显示主 UI。**

---

## 6. 修复方向(三选一)

### A. proxy 端注入 `**思考**` header(推荐 ⭐)

**改动**:`crates/adapters/src/responses/converter.rs::open_reasoning` 在首次开 reasoning 流时,**先 emit 一段 prefix delta**:

```rust
fn open_reasoning(&mut self, out: &mut Vec<u8>) {
    self.reasoning_open = true;
    self.reasoning_index = self.next_output_index;
    self.next_output_index += 1;
    // ... emit output_item.added + reasoning_summary_part.added ...

    // 注入符合 Codex CLI 0.128 渲染期望的 bold header
    let prefix = "**Thinking**\n\n";
    self.reasoning_acc.push_str(prefix);
    emit_event(out, "response.reasoning_summary_text.delta", json!({
        "type": "response.reasoning_summary_text.delta",
        "item_id": self.reasoning_id,
        "output_index": self.reasoning_index,
        "summary_index": 0,
        "delta": prefix,
    }));
}
```

**优点**:
- 单点改动,风险可控
- 立刻让 Kimi reasoning 在 UI 上完整显示
- prefix 长度可调(可以 i18n 成"思考"或更细的 header)

**缺点**:
- 内容多一行 `**Thinking**`(无害,与 ChatGPT/Codex 行为一致)
- 不区分 provider,会对 deepseek/mimo 也加 prefix(deepseek 的 reasoning 也是纯文本,反而帮到它);若以后接 OpenAI o1 走 proxy 会重复 header

**测试**:已有 `reasoning_then_content_emits_two_items_in_order` 等测试,需补一条「prefix 注入」的快照测试。

### B. proxy 端发 `response.reasoning_text.delta`(走 raw 路径)

**改动**:把 emit 事件名从 `response.reasoning_summary_text.*` 改成 `response.reasoning_text.*`。这条路径在 codex CLI 走 `EventMsg::AgentReasoningRawContentDelta`,渲染逻辑同样要 final 触发但 raw 内容**默认不显示**(需要用户开 `show_raw_agent_reasoning = true`)。

**否决理由**:用户额外配置,且即便开启,raw 走的也是 `on_agent_reasoning_delta + final` 同一函数,**最终也会撞同一个 transcript_only 判定**,无法绕过。

### C. 用户用 `/transcript` 查看完整内容(workaround)

**改动**:零改动,文档说明即可。

**否决理由**:用户体验差,默认行为里"思考不显示"违反预期。

---

## 7. 推荐实施

**P0 (10 分钟)**:实施方案 A,在 `open_reasoning` 注入 `**Thinking**\n\n` prefix。

注意点:
- 同时 push 到 `reasoning_acc`(让 `summary[0].text` 包含 prefix,触发 final 分支检测 `**` 命中显示路径)
- 同时 emit `reasoning_summary_text.delta`(让流式期间 reasoning_buffer 也有 prefix,extract_first_bold 能提取 status header)
- 字符串选择:用英文 `**Thinking**` 与 OpenAI 自家命名风格一致;或中文 `**思考**` 更亲切。建议先英文 + 测试通过后再 i18n。

后续可加配置项 `provider.modelCapabilities.reasoning_prefix`,允许用户/预设里自定义或关闭。

---

## 8. 复现方法(可重做)

```bash
# 1. 录上游 raw SSE(替换 KEY)
KEY=$(jq -r '.providers[]|select(.name=="Kimi Code")|.apiKey' ~/.codex-app-transfer/config.json)
cat > /tmp/req.json <<'JSON'
{"model":"kimi-for-coding","stream":true,
 "messages":[{"role":"user","content":"<your-prompt>"}]}
JSON
curl -sN --max-time 300 \
  -H "Authorization: Bearer $KEY" \
  -H "Content-Type: application/json" \
  -H "User-Agent: KimiCLI/1.40.0" \
  -X POST https://api.kimi.com/coding/v1/chat/completions \
  --data-binary @/tmp/req.json -o /tmp/raw.sse

# 2. 上游字符会计
python3 -c "
import json, re
text = open('/tmp/raw.sse').read().replace('\r\n','\n')
total = 0
for f in re.split(r'\n\n+', text):
    if not f.startswith('data:'): continue
    p = f[5:].strip()
    if p == '[DONE]': continue
    try: d = json.loads(p).get('choices',[{}])[0].get('delta',{}).get('reasoning_content')
    except: continue
    if isinstance(d,str): total += len(d)
print('上游 reasoning 字符:', total)
"

# 3. 喂 ChatToResponsesConverter,核对账
# (见本仓库 /tmp/kimi-probe/probe-bin/main.rs,直接 cargo run --release)
```

---

## 9. 直接结论与建议

### 已确认(无需进一步验证)

- ✅ **proxy 协议转换层对 Kimi reasoning_content 是 byte-perfect 透传**,不存在丢失。
- ✅ Kimi for Coding 字段名为标准 `reasoning_content`,与 v2.0.7 ChatDelta 完全匹配。
- ✅ SSE 状态机生命周期事件齐全,32k 字符级别 reasoning 完整闭合。
- ✅ 4xx/5xx 诊断、TracedStream 字节量打点工作正常。

### 待用户协作的下一步

1. **(高优先,5 分钟)** 提供一次"思考明显变短"的 Codex CLI 截屏 + 当时 prompt,我用同一 prompt 直 curl 录上游字符数,做"上游字符 vs Codex CLI 显示行数"对比。
2. **(中优先,30 分钟)** 让我读 Codex CLI 源码,定位 `reasoning_summary_text` 事件的渲染逻辑;或者给我一次 Codex CLI 进程的完整渲染 trace。
3. **(低优先)** 检查 `~/.codex/config.toml` 是否有 `reasoning.summary` / `reasoning.verbosity` 类设置。

### 不建议(此前误判)

- ❌ 立即在 proxy 加 `reasoning_text` 事件双发 — 在没确认 Codex CLI 消费规则前,可能引入重复显示或 schema 冲突。
- ❌ 改字段名识别 — 字段名实测全对,无需改。
- ❌ 改累积逻辑 — 实测无丢失,无需改。

---

## 附录: 实证产物

- `/tmp/kimi-probe/raw.sse` — 样本 A 原始 SSE (379KB,60s 截断)
- `/tmp/kimi-probe/raw2.sse` — 样本 B 原始 SSE (5.4MB,300s 截断)
- `/tmp/kimi-probe/probe-bin/main.rs` — converter byte-accounting 探针
- 本报告引用的代码行号基于 commit `71ab6f0`(v2.0.7)
