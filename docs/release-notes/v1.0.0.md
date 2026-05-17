# Codex App Transfer v1.0.0

## 中文

首个公开发布版本。Codex App Transfer 是一个面向 OpenAI Codex CLI 的本地网关，把 Codex CLI 发出的 Responses API（WebSocket 流 + `/responses` HTTP 回退）翻译成 Chat Completions 格式，再转发到 Kimi For Coding、DeepSeek、智谱 GLM、阿里云百炼等第三方供应商。

### 主要变化

- **协议转换**：参考 `litellm` 的 Responses ↔ Chat Completions 双向映射策略，实现流式 / 非流式双通道转换。
- **Codex CLI 0.126+ 兼容**：同时支持 `responses_websocket` 主连接和 `responses_http` 回退，新增 `/responses` 路由别名（无 `/v1/` 前缀），解决 Codex CLI 升级后报 `404 /responses` 的问题。
- **Response ID 重写**：把上游 `chatcmpl-...` 应答 ID 改写成 Codex CLI 期望的 `resp_<base64>`，并通过 base64 保留 deployment affinity，让 `previous_response_id` 跨请求继续生效。
- **thinking 模式适配**：Kimi / DeepSeek 在开启 thinking 后强制要求历史 assistant tool_call 消息带 `reasoning_content`。本版本会从 Responses 输入里提取 reasoning items 映射到对应 assistant 消息，并对仍缺失字段的消息补默认空字符串，避免 `400 thinking is enabled but reasoning_content is missing`。
- **`reasoning_effort` 归一化**：`xhigh` / `max` 自动降级到 `high`，`auto` / `none` 直接丢弃，适配只接受 `minimal/low/medium/high` 的 Chat 端。
- **日志持久化与备份**：转发日志按天写入 `~/.codex-app-transfer/logs/proxy-YYYY-MM-DD.log`；新增"查看日志"按钮直接调用系统资源管理器打开日志目录；"清除日志"会把当前日志移动到 `logs/backup/<basename>_<YYYYMMDD-HHMMSS>.log` 后开启新日志，永远不直接删除文件。
- **桌面 UI 重构**：移除头部 logo / 软件名 / 提供商标题区，导入 / 清除 / 添加供应商按钮统一到顶部一行；转发 / 设置 / 引导页移除冗余的左上角标题；设置面板美化（iOS 风格分段控件 + 主题色块 + 圆角面板）。
- **主题修正**：仅深色主题改动应用背景色，其它主题（绿 / 橙 / 灰 / 白）保留默认白底，避免之前主题色"溢出"到原本应该是白色的区域。
- **桌面壳**：pywebview 桌面窗口 + pystray 系统托盘 + 单实例锁定，关闭窗口缩小到托盘，右键托盘选择"退出"完全关闭。
- **打包与发布**：macOS 本机 PyInstaller + Docker 跨编译 Linux / Windows（`tobix/pywine` + NSIS）三平台一键发布，`Makefile` 串联 `mac-release` → `linux-release` → `win-release` → `release-bundle`。每个产物附带 RSA-3072 PKCS#1 v1.5 + SHA-256 签名（PEM 公钥）和 `release/latest.json` 版本元数据。

### 下载建议

- Windows 用户优先下载 `Codex-App-Transfer-v1.0.0-Windows-Setup.exe`，或选 `Codex-App-Transfer-v1.0.0-Windows-Portable.zip` 解压即用。
- macOS Apple Silicon 用户可下载 `Codex-App-Transfer-v1.0.0-macOS-arm64.pkg`（安装到 `/Applications`）或 `Codex-App-Transfer-v1.0.0-macOS-arm64.dmg`（拖拽安装）。
- Linux x86_64 用户可下载 `Codex-App-Transfer-v1.0.0-Linux-x86_64.tar.gz`（folder 模式 tar 包）或 `Codex-App-Transfer-v1.0.0-Linux-x86_64`（onefile 单文件可执行），运行需系统已装 GTK3 + WebKit2GTK 4.0 + libayatana-appindicator3。
- 每个二进制都附带 `.sha256` 和 `.sig`，可用 `scripts/Test-ReleaseSignature.ps1 -File <asset>`（Windows）或 `cryptography` Python 脚本（macOS / Linux，参见 `docs/build.md`）校验完整性。

### 已知边界

- Windows 版未做 Authenticode 代码签名，系统可能提示未知发布者；`.sha256` / `.sig` 仅校验文件完整性，不能替代 Authenticode 证书。
- 当前已端到端验证的供应商是 Kimi For Coding。DeepSeek / GLM / 阿里云百炼属实验兼容，遇到字段差异欢迎提 Issue 附上转发日志。
- 第一次接入新供应商时建议使用低额度或可随时撤销的 API Key。

### 致谢

- [CC-Switch](https://github.com/farion1231/cc-switch)：产品形态启发。
- [CC Desktop Switch](https://github.com/lonr-6/cc-desktop-switch)：完整的桌面应用框架（pywebview + pystray + FastAPI 双端口 + PyInstaller / NSIS + 发布签名链路 + GitHub Actions + 前端模板）。
- [litellm](https://github.com/BerriAI/litellm)：Responses ↔ Chat Completions 协议转换策略参考。

## English

The first public release. Codex App Transfer is a local gateway for the OpenAI Codex CLI: it translates the CLI's Responses API traffic (WebSocket stream + `/responses` HTTP fallback) into Chat Completions format and forwards it to third-party providers such as Kimi For Coding, DeepSeek, Zhipu GLM, and Alibaba Cloud Bailian.

### Highlights

- **Protocol conversion** modeled on `litellm`'s Responses ↔ Chat Completions mapping strategy, covering both streaming and non-streaming paths.
- **Codex CLI 0.126+ transport compatibility**: supports both the `responses_websocket` primary connection and the `responses_http` fallback, with a `/responses` route alias (no `/v1/` prefix) that fixes the `404 /responses` regression after upgrading Codex CLI.
- **Response ID rewrite**: upstream `chatcmpl-...` IDs are re-encoded into Codex-friendly `resp_<base64>` while preserving deployment affinity through base64, so `previous_response_id` continues to work across turns.
- **Thinking-mode adapter**: Kimi / DeepSeek require `reasoning_content` on every historical assistant tool-call message when thinking is enabled. This release extracts reasoning items from the Responses input, attaches them to the matching assistant message, and back-fills an empty placeholder when needed to avoid `400 thinking is enabled but reasoning_content is missing`.
- **`reasoning_effort` normalization**: `xhigh` / `max` are downgraded to `high`; `auto` / `none` are dropped, so providers that only accept `minimal/low/medium/high` won't reject the request.
- **Log persistence and backup**: forwarding logs append to `~/.codex-app-transfer/logs/proxy-YYYY-MM-DD.log` (one file per day). A new `Open log folder` button launches the OS file manager at the log directory; `Clear logs` moves the active log to `logs/backup/<basename>_<YYYYMMDD-HHMMSS>.log` and starts a fresh file — files are never deleted.
- **Desktop UI refresh**: the header logo / app name / provider title block is gone; import / clear / add-provider actions sit on a single top row. Proxy / Settings / Guide pages drop their redundant top-left titles. The settings panel is restyled with iOS-style segmented controls, theme swatches, and rounded panels.
- **Theme fix**: only the dark theme changes the application background color. Green / orange / gray / white themes now leave originally-white surfaces untouched, fixing the prior theme-color bleed.
- **Desktop shell**: pywebview window + pystray tray icon + single-instance lock. Closing the window minimizes to tray; right-click the tray icon and choose "Exit" to fully quit.
- **Build and release**: tri-platform one-shot release driven from a macOS host — native PyInstaller for macOS plus Docker cross-builds for Linux (Ubuntu 22.04 + GTK3 + WebKit2GTK) and Windows (`tobix/pywine` + NSIS). The `Makefile` chains `mac-release` → `linux-release` → `win-release` → `release-bundle`. Each artifact carries an RSA-3072 PKCS#1 v1.5 + SHA-256 signature (PEM public key) and `release/latest.json` metadata.

### Downloads

- Windows users can choose `Codex-App-Transfer-v1.0.0-Windows-Setup.exe` (recommended) or `Codex-App-Transfer-v1.0.0-Windows-Portable.zip` (extract and run).
- macOS Apple Silicon users can choose `Codex-App-Transfer-v1.0.0-macOS-arm64.pkg` (installs to `/Applications`) or `Codex-App-Transfer-v1.0.0-macOS-arm64.dmg` (drag-to-install).
- Linux x86_64 users can choose `Codex-App-Transfer-v1.0.0-Linux-x86_64.tar.gz` (folder-mode tarball) or `Codex-App-Transfer-v1.0.0-Linux-x86_64` (single-file executable). Both require GTK3 + WebKit2GTK 4.0 + libayatana-appindicator3 on the host.
- Each binary ships with a `.sha256` and a `.sig` — verify with `scripts/Test-ReleaseSignature.ps1 -File <asset>` (Windows) or the `cryptography` Python snippet in `docs/build.md` (macOS / Linux).

### Known limitations

- Windows builds are not Authenticode code-signed; the OS may show an unknown-publisher warning. The `.sha256` / `.sig` files verify integrity but do not replace an Authenticode certificate.
- Kimi For Coding is the end-to-end validated upstream. DeepSeek / GLM / Bailian are experimental — please file an issue with forwarding logs if you hit schema mismatches.
- When connecting a new provider for the first time, prefer a low-quota or easily-revocable API key.

### Acknowledgements

- [CC-Switch](https://github.com/farion1231/cc-switch) — product-form inspiration.
- [CC Desktop Switch](https://github.com/lonr-6/cc-desktop-switch) — full desktop application framework (pywebview + pystray + FastAPI dual-port + PyInstaller / NSIS + release signing chain + GitHub Actions + frontend templates).
- [litellm](https://github.com/BerriAI/litellm) — Responses ↔ Chat Completions protocol conversion strategies.
