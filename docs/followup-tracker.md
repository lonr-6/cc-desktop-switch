# Followup Tracker（项目级长期 backlog）

跨 session 长期持有的 followup 任务索引。Claude / Agent / 任何贡献者发现"当前 PR 范围内不修但值得跟踪"的问题时,**必须**在 `docs/followup/` 落详情文件 + 在本文档对应段加索引行。

## 文档结构(多级,索引轻量,详情按需读取)

```
docs/
├── followup-tracker.md           # 本文档 — 顶层索引(短行 + 一句话 hook),长期维护
└── followup/
    ├── 23-grok-web-url-citation-redundancy.md   # 单条 followup 详情(强制详细)
    └── <id>-<slug>.md
```

**核心约束**:
- **索引行短** — 每条 Active / Resolved 1 行,≤150 字符,只放"是什么 + 链接"
- **详情文件详细** — 写到"半年后回看不需要重新调研"的程度,见下方"详情文件强制格式"
- 这样 Claude / 用户读索引时只 pull 几 KB 进 context,需要细节才打开对应详情文件

## 详情文件强制格式

每个 `docs/followup/<id>-<slug>.md` 必须包含(顶部 YAML frontmatter + 正文章节):

```yaml
---
id: 23
priority: P0 | P1 | P2 | P3
type: bug | research | refactor | infra | nit
status: active | resolved | dropped
created: YYYY-MM-DD
related_pr: <PR# 或 null>
---
```

正文章节(顺序固定,缺一不可):

1. **触发上下文** — 原 task / agent finding / 反馈来源 + 具体 file:line 引用
2. **问题描述** — 现状代码做了什么 / 期望应该做什么 / 差距具体在哪
3. **已有调研** — 已经看过的代码 / 文档 / 真实数据 / 假设验证结果(file:line + 引用片段)
4. **风险 / 不确定性** — 实施前需要先解决的疑问(尤其跨项目 / 上游行为依赖)
5. **建议方向** — 下次接手时第一步该做啥(不要重新调研),含决策树
6. **关联资源** — 相关 PR / docs / 上游 repo / 真机数据样本路径

**关键**:写得**够详细**,半年后回看不需要重新研究代码 / 重新抓包 / 重新读 agent finding。如果读起来"得重新看一遍才能下手",说明背景没写够 — 加更多 file:line 引用 / 真实数据片段 / 决策推导链。

## 维护规则

### 何时新增条目

任何以下情况:

- review agent / human reviewer 找到非 BLOCKER 但有价值的发现(MED / LOW / NIT / deferred)
- 实施过程发现"超出当前 PR scope 但 prod 真问题"
- 跨 adapter / 跨 crate / 跨架构层的重构建议(touch 太多 caller,当前 PR 不适合)
- 上游协议 / 标准 / 客户端行为研究 ticket(需要抓包 / 真机 / 跨项目调研)
- 测试基础设施 / fixture / CI 改进点

操作:

1. 在 `docs/followup/` 新建 `<id>-<slug>.md`(id 递增,slug = kebab-case 短描述)
2. 按"详情文件强制格式"写完整背景
3. 在本文档 Active 段加 1 行索引:`- [#N P? Title](followup/<id>-<slug>.md) — 一句话 hook(≤80 字符)`
4. 跟代码 PR 同 commit 落仓库(不靠 task list / commit message / memory)

### 何时移到 Resolved

条目完整实施 + 合并 main 时:

1. 把详情文件 frontmatter `status:` 改成 `resolved`,加 `resolved_pr` 跟 `resolved_date`
2. 本文档 Active 段索引行**移到** Resolved 段,改成 `- ~~#N Title~~ → PR #M (YYYY-MM-DD)` 形式
3. 详情文件**保留**作历史归档(不删,便于回溯)
4. Resolved 段每 30 天 review 一次,真正过期且 PR 已合很久(>90d)可批量归档到 `docs/followup/archive/`

### 何时 drop(误判 / 不再适用)

详情文件 frontmatter `status:` 改成 `dropped` + 加 `dropped_reason` 字段 + 索引行删掉。详情文件保留作历史回溯。

---

## Active

- [#23 P3 grok_web 末尾 url_citation 列表是否冗余](followup/23-grok-web-url-citation-redundancy.md) — 跟正文 markdown link 重复,实施前需真机看 Codex CLI 渲染

---

## Resolved

(完成条目移这里,1 行索引 + PR ref;详情文件保留作历史归档,30 天后批量进 archive/)

<!-- 示例:
- ~~#25 cloud_code Gemini mapper 漏配 session_cache~~ → PR #146 (2026-05-13)
-->
