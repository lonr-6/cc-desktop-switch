# Codex App Transfer

<p align="center">
  <a href="README.md">简体中文</a> |
  <a href="README.en.md">English</a> |
  <a href="docs/CHANGELOG.md">Changelog</a>
</p>

<p align="center">
  <a href="https://github.com/Cmochance/codex-app-transfer/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/Cmochance/codex-app-transfer?style=social"></a>
  <a href="LICENSE.txt"><img alt="License" src="https://img.shields.io/github/license/Cmochance/codex-app-transfer"></a>
  <a href="https://www.rust-lang.org/"><img alt="Rust" src="https://img.shields.io/badge/Rust-1.80%2B-orange?logo=rust"></a>
  <a href="https://v2.tauri.app/"><img alt="Tauri" src="https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri"></a>
  <a href="https://github.com/Cmochance/codex-app-transfer/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/Cmochance/codex-app-transfer/total?label=downloads"></a>
</p>

Codex App Transfer 是一个面向 **OpenAI Codex CLI** 的轻量桌面配置 + 转发工具。它在本机起一个网关,把 Codex CLI 发出的 Responses API 请求(HTTP 流式 / 非流式 + `/responses` 回退)翻译成 Chat Completions / Gemini Native / Anthropic Messages / Grok Web 等格式,转发到你选择的供应商。

跟 `farion1231/cc-switch` 这类偏 Claude Code 的 Anthropic 工具不同,本项目专注 **OpenAI Codex CLI** 的接入:用桌面 UI 管理供应商、模型映射、转发端口、日志面板,让 Codex CLI 无缝使用第三方 OpenAI / Gemini / Claude-compatible / Grok 等推理服务。

启动转发后,Codex CLI 通过本机 `127.0.0.1:18080` 与本工具通信。关闭窗口会缩到系统托盘继续运行,右键托盘"退出"才完全退出。

当前版本 **v2.1.6**(详见 [Changelog](docs/CHANGELOG.md) 与 [Releases](https://github.com/Cmochance/codex-app-transfer/releases))。

## 界面预览

| 仪表盘 | 供应商 |
|---|---|
| ![Board](docs/img/Board.png) | ![Providers](docs/img/Providers.png) |
| **设置** | **日志** |
| ![Settings](docs/img/Settings.png) | ![Logs](docs/img/Logs.png) |

### Codex CLI 实际接入

启用任意供应商后,Codex CLI 模型选择器会显示「<provider> / <real-model>」形式的真实模型名,对话过程中工具循环 / `previous_response_id` 历史回放 / thinking 模式 reasoning_content 注入全部由本地代理透明处理:

![Codex CLI 实际对话](docs/img/codex-cli-real-chat.png)

## 能做什么

- 管理多套供应商,按 OpenAI 模型名(`gpt-5.5` / `gpt-5.4` / `gpt-5.4-mini` / `gpt-5.3-codex` / `gpt-5.2`)映射到供应商真实模型 ID
- 把 Codex CLI 的 Responses API 流式 / 非流式请求转换为上游协议:Chat Completions、Gemini Native(`:streamGenerateContent`)、Gemini CLI OAuth(Cloud Code Assist)、Anthropic Messages(`/v1/messages`)、Grok Web(`/rest/app-chat/conversations/new`)、Responses 透传等
- 多轮工具对话上下文 + `previous_response_id` 历史回放 + autocompact 展开 + thinking / reasoning_content 注入全部对齐 OpenAI Responses API 协议
- 会话历史**两层持久化**:L1 内存 LRU + L2 sqlite 30 天 TTL(`~/.codex-app-transfer/sessions.db`),`.app` 重启不丢历史
- Codex CLI 原配置守护:apply 前自动快照 `~/.codex/{config.toml,auth.json}`,退出 / 下次启动按 key 智能合并还原
- 实时日志面板,2 秒自动刷新;统一 `tracing::warn!(error_id, detail)` + 稳定 token,operator 可 grep / 聚合
- 反馈弹窗附带诊断材料(环境信息、脱敏配置、最近错误快照及完整请求 / 响应),减少手工补材料
- 中文 / 英文界面,浅色 / 深色 / 绿色 / 橙色 / 灰色 / 白色多种主题
- 跨平台单实例锁定(双击启动自动唤起已有窗口)+ 跨进程 file lock 防多实例同时写 config 丢更新
- Windows / macOS / Linux 系统托盘

## 下载

最新版:`https://github.com/Cmochance/codex-app-transfer/releases/latest`

推荐资产命名:

```text
Codex-App-Transfer-v<版本>-Windows-x64-Setup.exe       Windows NSIS 安装版(推荐)
Codex-App-Transfer-v<版本>-Windows-x64.msi             Windows MSI(企业 MDM / GPO)
Codex-App-Transfer-v<版本>-macOS-arm64.dmg             macOS Apple Silicon
Codex-App-Transfer-v<版本>-macOS-x64.dmg               macOS Intel x64(v2.1.0+,close #61)
Codex-App-Transfer-v<版本>-Linux-x86_64.deb            Debian / Ubuntu
Codex-App-Transfer-v<版本>-Linux-x86_64.AppImage       通用 Linux x86_64,`chmod +x` 直接跑
```

每个二进制都附带 `.sha256` 与 `.sig`(RSA-3072 PKCS#1 v1.5 + SHA-256 签名);公钥 `Codex-App-Transfer-release-public.pem` 跟随每个 Release 一起发布,直接从 [Releases](https://github.com/Cmochance/codex-app-transfer/releases) 下载即可验签。

Windows 暂未做 Authenticode 代码签名,系统可能提示未知发布者,可用 `.sha256` / `.sig` 校验下载完整性。

## 快速开始

1. 启动 Codex App Transfer,弹出桌面窗口
2. 在仪表盘点右上角加号 → 选择 preset 或自定义供应商,填入 API Base URL、API Key、模型映射
3. 在"转发"页面点"启动转发",本机 `18080` 端口开始监听
4. 在 Codex CLI 配置文件(`~/.codex/config.toml`)里把 `base_url` 指向 `http://127.0.0.1:18080`,把 API Key 设为本工具显示的 Gateway API Key
5. 重新打开 Codex CLI,模型选项就会自动列出当前供应商的模型映射
6. ⚠️ **必须把 Codex CLI 切到 Full access**(`/approvals` → "Full access"):第三方 provider 在 Codex CLI 默认 `auto` 审批模式下,工具调用会卡审批弹窗;Full access 直接放行工具调用,这是接入第三方 provider 的**事实必要前提**

桌面窗口无法打开时(罕见,通常是 Tauri webview 初始化失败 / 系统 webview 缺失),先尝试重启;若仍异常,从 [Releases](https://github.com/Cmochance/codex-app-transfer/releases) 重新下载并查看 `~/.codex-app-transfer/logs/proxy-*.log`,或开 [Issue](https://github.com/Cmochance/codex-app-transfer/issues) 反馈。v2 架构无独立 HTTP admin UI(管理面板走 Tauri 同进程 `cas://`,**不再监听 18081 端口**)。

## 供应商兼容矩阵

| Provider | 多轮历史 | autocompact | tool_call_repair | 备注 |
|---|---|---|---|---|
| Kimi(Moonshot Platform / Kimi For Coding) | ✅ | ✅ | ✅ | thinking 三层防御 |
| DeepSeek V4(含 Max 思维) | ✅ | ✅ | ✅ | 视觉输入剥离避免 400 |
| Xiaomi MiMo(Token Plan / Pay for Token) | ✅ | ✅ | ✅ | 纯图请求兜底空格 text part |
| MiniMax M2.x / Text-01 | ✅ | ✅ | ✅ | `role=system` 转 user 防 400(v2.1.6) |
| Google AI Studio(`gemini_native`) | ✅ | ✅ | ✅ | Gemini 3 `/v1alpha` + Gemini 2.x `/v1beta` 自动选 |
| Google Gemini CLI OAuth | ✅ | ✅ | ✅ | 浏览器登录 Google 一次,免 API key |
| Anthropic Messages(custom Claude-compatible) | ✅(PR #153) | ✅(PR #153) | ✅(PR #153) | `apiFormat=anthropic_messages`,Claude preset 待真实验证后开放 |
| Grok Web(SuperGrok / X Premium+) | ✅ | ✅ | ✅(v2.1.6 加 tool_calls flatten) | 实验性,TOS 灰色,仅本机个人使用 |
| Google Antigravity OAuth | ✅ | ✅ | ✅ | 后端就绪,UI 待 PR |
| 智谱 GLM / 阿里云百炼 | ⚠️ 实验兼容 | — | — | OpenAI Chat 兼容反代 |
| Responses 协议透传(custom) | — | — | — | 直连上游不经代理(适合 OpenAI 官方 / 原生 Responses 反代) |

## 模型映射

Codex CLI 按 OpenAI 模型名提示;第三方 provider 用 `deepseek-v4-pro` / `kimi-k2.6` / `glm-5.1` / `gemini-3-pro` 等真实 ID。

本工具用 `provider.models[slot]`(`gpt-5.5` → `deepseek-v4-pro` 等)做槽位映射,Codex CLI 模型选择器看到 `<provider> / <real-model>` 形式真实模型名;上游 `chatcmpl-...` 应答 ID 自动重写为 Codex CLI 校验通过的 `resp_<base64>`,保留 deployment affinity 编码,`previous_response_id` 跨轮一致。

## 本地开发(v2 / Rust)

```bash
git clone https://github.com/Cmochance/codex-app-transfer.git
cd codex-app-transfer
cargo tauri dev          # 启动桌面窗口,代码改动自动重编译
cargo test --workspace --lib   # 跑单元测试
make mac-app             # macOS 本地打包到 dist/mac/
```

Fixture 反向 diff(契约测试):

```bash
cargo run --bin xtask -- gen-fixtures
```

打包(参考 `.github/workflows/release.yml`):

```bash
cargo tauri build --bundles app,dmg          # macOS arm64
cargo tauri build --bundles nsis,msi         # Windows x64
cargo tauri build --bundles deb,appimage     # Linux x86_64
```

## 常见问题

### Codex CLI 提示 `404 Not Found url: http://127.0.0.1:18080/responses`

老版本只有 `/v1/responses`,Codex CLI 0.126 起回退到 `/responses`(不带 `/v1/`)。本工具已加路由别名,更新到 v1.0.1+ 即可。

### Codex CLI 提示 `stream disconnected before completion`

通常是 `response.id` / `response.model` 没按 Codex CLI 期望填回。本工具把上游 `chatcmpl-...` 重写成 `resp_<base64>` 并保留请求模型名,请确认转发日志确实看到 `resp_...` 而不是 `chatcmpl-...`。

### 上游 400:`thinking is enabled but reasoning_content is missing`

Kimi / DeepSeek 开启 thinking 后强制要求历史中带 tool_call 的 assistant 消息提供 `reasoning_content`。v1.0.1+ 已自动补默认空字符串,并把 Responses 输入里的 reasoning items 映射到对应 assistant 消息。

### 上游 400:`'reasoning_effort' does not support 'xhigh'`

Codex 用户配置里若把 `model_reasoning_effort` 设成 `xhigh` / `max`,本工具自动降级到 `high`。`auto` / `none` 等 Chat 端不接受的值会被丢弃。

### MiniMax 400:`invalid message role: system (2013)`

v2.1.5 及之前的版本未把 `role=system` 转 `role=user`,导致 MiniMax `/v1/chat/completions` 整请求 400。v2.1.6+ 已修(close #139),所有 `role=system` 消息转 `role=user` + content 前置 `[System]\n` marker。

### 端口冲突

v2 默认监听 `18080`(转发);管理界面走 Tauri 同进程 `cas://`,不再占用 18081。`netstat -ano | findstr :18080` 查占用,或在 设置 → 端口 改成空闲端口后重启转发。

### Windows 提示未知发布者

当前 Windows 构建未做 Authenticode 代码签名。Release 页提供 `.sha256` 与 `.sig`,可用于校验安装包未被替换。

### 日志去哪了

- 应用界面:转发页面下方实时面板,2 秒自动刷新
- 磁盘文件:`~/.codex-app-transfer/logs/proxy-YYYY-MM-DD.log`,点"查看日志"按钮直接打开
- 清除日志:把当前日志移到 `logs/backup/` 并加时间戳后缀,不直接删除

## 技术栈

- **后端 / 转发**:Rust 1.80+ · axum 0.8 · reqwest 0.12(rustls-tls)· tokio
- **协议适配**:`crates/adapters/` — Responses ↔ Chat / Gemini Native / Gemini CLI OAuth / Anthropic Messages / Grok Web 互转(请求 body + 流式响应状态机 + reasoning_content + tool_calls)
- **前端**:HTML + CSS + 原生 JavaScript + Bootstrap 5.3.3(本地化,无 CDN 依赖)
- **桌面壳**:Tauri 2 + tray-icon 0.23,通过 `cas://` URI scheme 把 frontend/ 与 axum 同进程串起来,无 TCP loopback
- **存储**:`~/.codex-app-transfer/config.json`(配置,与 v1.x 互通)、`~/.codex-app-transfer/sessions.db`(L2 sqlite 会话持久化)、`~/.codex/{config.toml,auth.json}`(Codex CLI 集成)
- **打包**:`cargo tauri build` 单命令出 dmg/AppImage/deb/exe/msi;`xtask release-bundle` 收口出 sha256 + RSA-3072 sig + latest.json + draft GitHub release

## 免责声明

本项目专注 **OpenAI Codex CLI** 接入,**不是** OpenAI / Anthropic / Google / xAI 的官方项目,也不复用其商标 / Logo / 发布身份。

上游 API key / OAuth token 仅保存在本机 `~/.codex-app-transfer/`(Unix 0600 + atomic write);转发服务只监听 `127.0.0.1`,不接管系统代理。

部分实验性 provider(Grok Web / Gemini CLI OAuth / Antigravity OAuth)涉及上游 TOS 灰色地带 — Grok Web 反代 grok.com Web 后端协议、Gemini CLI OAuth 借用 `cloudcode-pa.googleapis.com/v1internal` 内部端点 — 严格限定**个人使用**,**不应**作为对外服务发布,**用户自担风险**。

## 致谢

- [`farion1231/cc-switch`](https://github.com/farion1231/cc-switch) — provider 切换形态启发
- [`lonr-6/cc-desktop-switch`](https://github.com/lonr-6/cc-desktop-switch) — v1.x 桌面壳骨架 + README 结构参考 + **Claude Desktop 配置写入功能 1:1 转写**(`crates/claude_desktop/` 全套,源自上游 `backend/config.py:18-185` Provider schema 与 BUILTIN_PRESETS、`backend/model_alias.py:1-225` MODEL_SLOTS 与多 provider 路由、`backend/registry.py:13-122` DESKTOP_CONFIG / CCDS_MARKER / serialize_gateway_headers 等;macOS plist + Application Support config.json 写入上游无实现,由 cas 自行设计)
- [`BerriAI/litellm`](https://github.com/BerriAI/litellm) — 协议双向转换思路
- [`tauri-apps/tauri`](https://tauri.app/) — v2 + `cas://` 架构基座
- [`Piebald-AI/claude-code-system-prompts`](https://github.com/Piebald-AI/claude-code-system-prompts) — autocompact prompt 蓝本
- [`7as0nch/mimo2codex`](https://github.com/7as0nch/mimo2codex) — MiMo 协议借鉴
- [`router-for-me/CLIProxyAPI`](https://github.com/router-for-me/CLIProxyAPI) — Gemini OAuth wire 参考
- [`chenyme/grok2api`](https://github.com/chenyme/grok2api) — Grok Web 反向工程参考 + dynamic statsig 算法 + tool_calls flatten 模式

### 社区贡献者

通过 PR 直接改进过本项目的贡献者(按首次提交时间倒序;完整列表见 [Contributors](https://github.com/Cmochance/codex-app-transfer/graphs/contributors)):

- [@lukegood](https://github.com/lukegood) — MiniMax M2.x 兼容性([#47](https://github.com/Cmochance/codex-app-transfer/pull/47))
- [@cw881014](https://github.com/cw881014) — 早期协议层 3 PR([#1](https://github.com/Cmochance/codex-app-transfer/pull/1) / [#7](https://github.com/Cmochance/codex-app-transfer/pull/7) / [#12](https://github.com/Cmochance/codex-app-transfer/pull/12))

如果提交过 PR 想改名 / 补链接 / 移除,开 issue 跟我说一声。

## 许可证

MIT License。完整文本见 [LICENSE.txt](LICENSE.txt)。
