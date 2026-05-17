# Codex App Transfer v1.0.2

> 本版本两条主线:**让用户能反馈、让新人能上手**。新增完整匿名反馈系统、重做使用引导页、Xiaomi MiMo (Token Plan) 端到端验证升级,并修复若干 bug。

## 中文

### 新增用户反馈系统(完全匿名,无需登录)

- **入口**:Dashboard 顶栏 +「反馈」按钮、Settings 页底部「反馈与建议」section
- **Modal**:标题(选填) + 描述(必填),附件支持点击 / 拖拽 / **Cmd+V 粘截图**,单文件 5MB,默认勾选"附加诊断信息"(应用版本 / OS / 当前 provider 名 / 最近 200 行 proxy 日志,**不含 API Key**)
- **链路**:应用 → 后端 → Cloudflare Worker → R2 存储 + Resend 邮件通知
- **隐私**:不存原始 IP(只存 SHA-256 加盐 hash 前 16 位用于去重);Modal 顶/底各一条警告提醒检查截图
- **节流**:成功提交后 60s 冷却,失败不立即计入(给改完即重试空间),5 分钟内连续 5 次失败才触发冷却,Worker 端 IP 限流每天 10 次
- **基础设施开源**:`feedback-worker/` 含完整代码 + 部署脚本,fork 用户能用自己的 Cloudflare 账户跑同样一套

### 指南页重新设计

老指南页只有简单的 3 步 timeline,跟项目能力不匹配。重做后包含 Hero + 「开始之前」前置条件 + 5 步快速开始 timeline + 进阶用法 4 卡片 + 遇到问题 4 卡片(含"测速失败 → 协议改 chat/completions"提示),卡片有 hover 上浮效果,深色模式 / 调色板自动适配。

### Xiaomi MiMo Token Plan 端到端验证 + 多集群

**Xiaomi MiMo (Token Plan) 已端到端验证通过**,从「实验兼容」升级为「已验证供应商」(目前仅 Kimi Code 与 Xiaomi MiMo Token Plan 享此标签)。同时更新 Token Plan 的 baseUrlOptions 为官方 3 个集群:中国 / 新加坡 / 欧洲。Pay for Token 仍在「实验兼容」列表 — 只有 Token Plan 跑过端到端测试。

### 单实例锁(修复多开)

老版本 README 写的"单实例锁定"实际未实现,双击 `.exe` 两次会启两个进程,各自抢 18080 转发端口冲突。本版本启动时探测 admin 端口的 `/api/instance-info`,识别到已有实例则把它的窗口拉前台,新实例 `sys.exit(0)`。Windows 上额外弹原生提示。跨平台一致(macOS / Linux / Windows 同一份逻辑)。

### Dashboard provider 卡片官方文档跳转

老版本 baseUrl 是个超链接,点击直接打开 `https://api.deepseek.com/v1` 这种 API 接入地址 — 是死链。本版本改为指向官方文档(每个 builtin preset 有 `docsUrl` 字段),点击触发"是否跳转到 X 的官方文档?"确认框,确认后浏览器打开。点击区域只覆盖 URL 文本本身,避免误触。

### Provider 编辑表单 — 移除 Auth Scheme 选择器

7 个内置预设全部用 `bearer`(OpenAI 兼容标准),`x-api-key` / `none` 选项只会让用户错改弄坏 provider。UI 控件下线,后端默认 `bearer` 兜底,旧 provider 数据不丢失。

### 日志面板 — 自动滚动改进 + 列宽优化

老版本每 2s 全量刷新都强制 `scrollTop = scrollHeight`,用户向上翻历史日志会被反复弹回底部。本版本跟踪用户实际滚动位置,在底部时维持自动跟随,向上滚停止跟随并保留位置;重新滚回底部 / 切走切回页面恢复跟随。同时时间戳列 220px → 96px(原宽度对 `HH:MM:SS` 严重过剩),整体更紧凑。

### Bug fix

- **pywebview WebKit FormData 兼容**:WKWebView 在 `fetch + FormData` 上抛 `the string did not match the expected pattern`。前端 → 后端改走 JSON + base64,后端 → Worker 仍用 multipart(httpx 拼包不受 WebKit 影响)
- **CSRF 防护头**:反馈端点漏加 `X-CAS-Request: 1` 头,被中间件拦截。已补
- **i18n HTML 渲染**:老的 i18n loader 用 `textContent`,会把翻译值里的 `<strong>` / `<code>` / `<a>` 当字面文本显示。改为检测到 HTML 标签时用 `innerHTML`(所有 i18n 值都是项目内置静态字符串,无 XSS 风险)
- **Kimi 默认协议**:部分老用户保存的 Kimi provider 误存了 `apiFormat: "responses"`(早期 bug),编辑表单显示成 Responses 协议。前后端都按 baseUrl 匹配 builtin preset 强制矫正

### 打包流程改进

- **Windows Setup 安装包默认强制构建**:v1.0.1 release 实际只有 portable + onefile 没 Setup,原因是 build pipeline 没在 Setup 缺失时报错。本版本 `makensis` 后强制检查产物存在性,缺失立刻退出 1;`release_assets.py` 在 portable/onefile 已收集但 Setup 缺失时 fail-fast(除非显式 `CCDS_SKIP_INSTALLER=1`)
- 增强了打包产物的反编译保护(macOS / Linux 平台)

## English

> Two themes this release: **let users send feedback** and **help newcomers get started**. Adds an end-to-end anonymous feedback system, redesigns the getting-started guide, promotes Xiaomi MiMo (Token Plan) to a verified provider, and fixes several bugs.

### Anonymous user feedback system (no login required)

- **Entry points**: top-bar "Feedback" button on Dashboard, "Feedback & suggestions" section at the bottom of Settings.
- **Modal**: optional title + required description, attachments via click / drag / **Cmd+V to paste screenshot** (5MB per file), "Attach diagnostics" checkbox enabled by default (app version / OS / current provider name / last 200 lines of proxy log — **no API key**).
- **Pipeline**: app → backend → Cloudflare Worker → R2 storage + Resend email notification.
- **Privacy**: no raw IPs stored (only the first 16 chars of a salted SHA-256 hash, for dedup); two warnings in the modal remind users to scrub sensitive content from screenshots.
- **Throttle**: 60s cooldown after a successful submit; failures don't count immediately (so users can fix and retry); 5 consecutive failures within 5 minutes triggers cooldown; Worker-side IP cap at 10/day.
- **Open infrastructure**: `feedback-worker/` ships the full Worker code + deploy scripts so forks can run their own feedback pipeline on their own Cloudflare account.

### Redesigned getting-started guide

The old 3-step timeline didn't match the project's actual capabilities. The new guide adds a Hero header, "Before you start" prerequisites, a 5-step quick start timeline, 4 advanced-usage cards, and 4 troubleshooting cards (including a "speed test failing? switch protocol to chat/completions" tip). Cards have hover lift + primary-color border, dark-mode and palette aware.

### Xiaomi MiMo (Token Plan) end-to-end verified + multi-cluster

**Xiaomi MiMo (Token Plan) is now end-to-end verified** and promoted from "experimental" to "verified provider" (joining Kimi Code as the only two with this badge). The Token Plan preset's `baseUrlOptions` now lists the official 3 clusters: China / Singapore / Europe. **Pay for Token** stays in the experimental list — only Token Plan has been e2e-tested.

### Single-instance lock (fix for multi-launch)

The previous version's README claimed a single-instance lock that wasn't actually implemented — double-clicking `.exe` would spawn two processes that fought over forwarding port 18080. This release probes the admin port's `/api/instance-info` at startup; if an existing instance answers, it's brought to the foreground and the new instance exits cleanly. Windows additionally shows a native MessageBox. Same logic across macOS / Linux / Windows.

### Dashboard provider card → official docs link

The old card's baseUrl was a clickable link, but `https://api.deepseek.com/v1` etc. are API endpoints, not browsable pages — the link was dead. Each builtin preset now has a `docsUrl` field pointing to the official documentation; clicking the URL triggers a "Open X's official docs?" confirm dialog before opening the browser. Click area is constrained to the URL text only, avoiding accidental clicks.

### Provider edit form — Auth Scheme selector removed

All 7 builtin presets use `bearer` (the OpenAI-compatible standard); `x-api-key` / `none` only existed to confuse users into breaking working providers. The control is gone from the UI; backend defaults to `bearer`, and existing provider data (including any `x-api-key` users) is preserved untouched.

### Log panel — auto-scroll fix + column tightening

Previously, the every-2s full refresh forced `scrollTop = scrollHeight`, snapping users back to the bottom whenever they tried to read older logs. The panel now tracks the actual scroll position: auto-follow when at the bottom (with 8px tolerance), pause auto-follow when the user scrolls up, resume when they scroll back to the bottom or leave/return to the page. The "Auto-scroll" toggle remains the user-level master switch. Timestamp column shrunk from 220px → 96px (the original was wildly oversized for `HH:MM:SS`), overall layout more compact.

### Bug fixes

- **pywebview WebKit FormData compatibility**: WKWebView throws `the string did not match the expected pattern` on `fetch + FormData`. Frontend → backend now uses JSON + base64; backend → Worker still uses multipart (httpx assembles it server-side, unaffected by WebKit).
- **CSRF header**: feedback endpoint was missing the `X-CAS-Request: 1` header and was rejected by middleware. Fixed.
- **i18n HTML rendering**: the old loader used `textContent`, rendering `<strong>` / `<code>` / `<a>` in translation values as literal text. Now switches to `innerHTML` when an HTML tag is detected (all i18n values are project-internal static strings, no XSS surface).
- **Kimi default protocol**: some users had legacy Kimi providers with `apiFormat: "responses"` (early bug); both frontend and backend now match preset by baseUrl and force-correct it.

### Build pipeline improvements

- **Windows Setup installer enforced by default**: v1.0.1 shipped only the portable + onefile (no Setup) because the build pipeline didn't fail when the Setup file was missing. This release hard-fails after `makensis` if the artifact isn't produced, and `release_assets.py` fails fast if Setup is missing while portable/onefile have been collected (unless `CCDS_SKIP_INSTALLER=1` is set explicitly).
- Hardened decompilation resistance for shipped binaries (macOS / Linux).

## 部署 Worker(给 fork 用户)

```bash
cd feedback-worker
# 1. 装 wrangler 并 OAuth 登录
npm install -g wrangler
wrangler login

# 2. 创建 R2 + KV(替换 wrangler.toml 里的 KV id)
wrangler r2 bucket create <your-bucket-name>
wrangler kv namespace create FEEDBACK_RATE_LIMIT

# 3. Resend 注册账号 + 创建 API Key + 填 .env

# 4. 推送 secret + 部署
./setup-secrets.sh
wrangler deploy

# 5. 把输出的 Worker URL 配到 backend/main.py 的 FEEDBACK_WORKER_URL
```
