---
id: 23
priority: P3
type: research
status: active
created: 2026-05-13
related_pr: null
---

# #23 grok_web 末尾 url_citation 列表是否冗余

## 触发上下文

- **原 task**: `task #23`(grok_web inline `[N]` 精确位置 citation)
- **降级评估**: 在 task #25 流程(PR #146)中重新评估,确认 inline `[N]` 假设不成立 → 换成"末尾列表是否冗余"研究 ticket
- **代码引用**:
  - `crates/adapters/src/grok_web/response.rs:1631 accumulate_web_search_url_citations`
  - `crates/adapters/src/grok_web/response.rs:1658 accumulate_x_search_url_citations`
  - `crates/adapters/src/grok_web/response.rs:1277 accumulate_generic_search_url_citations`
  - `crates/adapters/src/grok_web/response.rs:1591 flush_pending_url_citations`(末尾 emit 入口)
- **原 README 描述**: `crates/registry/src/presets_data.json:261` 说"已知限制:inline `[N]` 精确位置 citation 暂未实现(仅追加在结尾)"— 措辞需配合本 ticket 决议同步修

## 问题描述

**现状**:`accumulate_*_url_citations` 把 grok 后端的 `webSearchResults` / `xSearchResults` / `connectorSearchResults` 全部 dump 成 `url_citation` annotation 数组,在 message 末尾通过 `response.output_text.annotation.added` 事件 emit。

**期望**(假设):OpenAI Responses 协议下 url_citation 跟正文 `[N]` 编号 1:1 绑定,作"角标 → 末尾参考文献"的导航元数据。

**差距**:grok 模型 final text **没有 `[N]` 编号**,直接用 markdown inline link(`[官网](https://example.com)`)写在正文里 — Codex 客户端按 markdown 渲染可点跳。结果:
- **同一 URL 在正文 + 末尾列表各显示一次**(冗余)
- 对于"grok 后端搜了但模型没引用"的 URL,末尾还会列出用户从没见过的链接(噪音)
- 当前 `start_index: 0 / end_index: 0`(`response.rs:1651-1652`)等于告诉客户端"没绑定具体位置"— 跟设真实 byte offset **用户视角行为无区别**(因为没角标可定位)

## 已有调研

1. **真实抓包**: `docs/grok/img/docs/R1.js` final text 拼接(grep `"messageTag":"final"` + python 反序列化拼 token):**完全没有 `[N]` 编号 marker**,grok 用纯 markdown inline link 形式。样本拼接结果片段:
   > "...**Official site & docs**: [modelcontextprotocol.io](https://modelcontextprotocol.io)..."
2. **OpenAI Responses spec**: `url_citation` 的 `start_index` / `end_index` 字段设计是"正文中 `[N]` token 的字符偏移",grok 数据没 `[N]` → 我们设 `0/0` 跟设真实偏移行为无区别
3. **Codex CLI 渲染**: 客户端实际渲染 url_citation 列表的代码路径**没抓到**(本仓库不含 Codex CLI 源码)— 假设是"列表式末尾显示",可能错

## 风险 / 不确定性

- **不确定 1**: Codex CLI 是否真把 url_citation 渲染成"末尾列表"?如果它已经"看到 markdown link 就**不**再渲染对应 citation"(智能去重),那当前 dump 不算冗余,只是没生效。需真机或客户端代码确认
- **不确定 2**: 是否有用户依赖"末尾 reference page"作 fact-check 入口?如果有,删除会变 regression
- **不确定 3**: 删除 url_citation dump 后,reasoning 段(thinking 阶段已有的 `connector_search_results_appends_to_reasoning_and_emits_citation` 等 markdown bullet 渲染)是否能替代审计追溯职责?

## 建议方向

实施前**先**:

1. 真机收集 v2.1.6+ 用户反馈,问"末尾 citation 列表对你有用吗?跟正文链接重复你觉得冗余吗?"
2. 真机起 Codex CLI + grok provider 观察 url_citation 实际渲染形态(截图)

决策树:

- **用户觉得冗余 + Codex CLI 简单列表式** → 删 `accumulate_*_url_citations` 三条路径,保留 reasoning 段 bullet list(thinking 阶段透明性不变)
- **用户依赖 / Codex CLI 智能去重** → 保留现状,本 ticket 关闭。同时改 `presets_data.json:261` 措辞,把"暂未实现 [N]"换成"按 markdown link 直接引用,无角标"

## 关联资源

- `docs/grok/img/docs/R1.js` — grok 真实抓包样本
- `crates/adapters/src/grok_web/response.rs` — 当前实现
- `crates/registry/src/presets_data.json:261` — 用户面 description(需同步修)
- PR #135-#138 — grok_web web search 初始引入
- PR #146 流程 — 本 ticket 降级评估发生地
