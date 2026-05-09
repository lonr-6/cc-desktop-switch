# Current Status

## current_truth

- 本 worktree 是 CC Desktop Switch 的 Rust/Tauri 未来主线重构。
- 当前目录：`D:\ccds-build\cc-desktop-switch-rust-mainline`
- 当前分支：`codex/rust-mainline-rewrite`
- 基线：`origin/main`，创建时 HEAD 为 `e3aab8e`。
- 目标版本：先使用 `v1.1.0-rc1` 作为本地测试候选名。
- 主线架构：Tauri v2 + Rust runtime + Rust/WASM UI。
- UI 决策：纯 Rust UI，默认采用 `Leptos + Trunk` 编译到 WASM，由 Tauri WebView 承载。
- P1 skeleton 已建立：Cargo workspace、`src-tauri`、`ui`、`xtask` 最小结构存在；Tauri app 可打开窗口；Leptos UI 可调用 Rust commands 完成 provider save / health / apply dry-run。
- P2 `model_catalog` 初始边界已建立：显式映射生成 Claude-safe route，`Default` 不进 Desktop menu / 不参与 resolve，未映射 route 返回 `gateway.unmapped_model_route`，Max capability 返回 `provider.max_not_supported`。
- P3 config migration 初始边界已建立：旧 Python stable-line config 可迁移到 Rust schema，保存前会备份原 config，`providerId` 和已生成 route ID 在 Provider rename 后保持稳定。
- P4 provider service 初始边界已建立：Tauri commands 可持久化 provider、列出 provider summary、设置 active provider；summary 不回显 API key。
- P5 DesktopApplyFlow planner 初始边界已建立：从 active provider 和 `ModelCatalog` 生成 local gateway plan，并可比较 Desktop readback 的 base URL、route、1M 和 raw-name 问题。
- P6 gateway core 初始边界已建立：`/v1/models` 响应只来自 Desktop-safe catalog；`/v1/messages` 只接受映射 route，未映射和不支持 Max 都返回 400 级结构化错误。
- P7 post-review hardening 已完成：Desktop planner 不再 fallback 到 DeepSeek fixture 或吞掉 `ModelCatalog` 错误；readback 严格比较 mode/auth/key/header/route set/1M/Max；provider metadata update 保留既有 model mappings；custom `proxyPort` 进入 health/dry-run/Desktop plan。
- P8 foundations 已建立：Axum local gateway router/server skeleton 存在；公开 `/v1/messages` handler 在 upstream adapter 未完成前返回 501 且不回显 upstream model；diagnostics redaction core 覆盖 key、gateway key、Authorization、cookie、URL token、URL userinfo。
- P9 upstream adapter 初始边界已建立：Anthropic passthrough 会把 safe route 替换为 upstream model；OpenAI Chat conversion 会转换 system/messages/max_tokens/temperature/top_p/stream；非 JSON/错误 stream 上游响应会生成 Anthropic-style structured error 并脱敏 preview。
- P10 non-stream upstream forwarding 初始边界已建立：gateway 可通过 `reqwest` 向 upstream 发起非流式请求；Anthropic response 会把 raw upstream `model` 改回 safe route；OpenAI Chat response 会归一化为 Anthropic-style message；text/html 等非法上游响应返回脱敏错误 envelope。
- P11 SSE runtime 初始边界已建立：gateway 可转发 upstream `text/event-stream` body；stream chunk 中 JSON `model` 字段会从 raw upstream model 归一化回 safe route；stream=true 但 upstream 不是 `text/event-stream` 时返回 `gateway.invalid_stream_content_type` 并脱敏 preview。
- P12 gateway lifecycle 初始边界已建立：Tauri state 管理 local gateway start/stop/status；app startup 会在已有 active provider 时尝试启动 gateway；health 会暴露 running 状态和最近 gateway 启动错误码；Leptos UI 可调用 gateway status/start/stop。
- P13 Desktop local configLibrary writer 初始边界已建立：普通本地 Apply 的 writer 先面向 Claude 3P local user configLibrary fixture，写入 gateway provider/base URL/key/auth/model routes，读回后用 `DesktopHealth` 比较；Windows registry / macOS mobileconfig 暂作为 managed/export 后续能力。
- P14 apply transaction fixture 初始边界已建立：`apply_local_config` command 和 state apply flow 可执行 provider snapshot -> gateway ensure running -> Desktop plan -> local configLibrary write -> readback compare；`success=true` 只在全链路通过时出现，缺少 provider 或端口冲突会在写入前失败。
- P15 Desktop config probe 初始边界已建立：可探测当前平台 local configLibrary 路径，Windows 优先 `%LOCALAPPDATA%\Claude-3p\configLibrary`，macOS 使用 `~/Library/Application Support/Claude-3p/configLibrary`；可报告 managed policy/config evidence 并给出 `desktop.managed_config_detected`。
- P16 user-facing Apply 初始边界已建立：`apply_detected_local_config` command 会先做 Desktop config probe，若检测到 managed config 则在启动 gateway 和写入前失败；Leptos UI 已能显示 Apply 步骤结果。
- P17 OpenAI streaming semantic conversion 初始边界已建立：OpenAI Chat `chat.completion.chunk` SSE 会转换为 Anthropic-style `message_start` / `content_block_*` / `message_delta` / `message_stop` 事件，并继续隐藏 raw upstream model。
- P18 Provider parity 初始边界已建立：Provider edit/delete/reorder 可通过 Rust state/Tauri command 持久化；删除 active provider 会切到剩余首个 provider；reorder 要求 exact set；Leptos spike 已接入 list/delete/move-first 最小链路；active provider 配置变化会重启 local gateway，避免旧 provider runtime 继续服务。
- P19 Provider import/export 初始边界已建立：Rust Provider export package、legacy CC-Switch/Python stable config import preview、conflict dry-run、replace import、duplicate provider ID 拒绝、raw route ID 拒绝、`Default` 非 runtime 映射保持均有测试覆盖；Tauri commands 暴露 export/preview/import。
- P20 diagnostics export package 初始边界已建立：结构化 diagnostics package、redacted config JSON、false-green readiness issue codes、Desktop probe evidence/error、copy summary 和 export package Tauri commands 均已覆盖；诊断包不会回显 API key、gateway key、Authorization、cookie、URL token。
- P21 Leptos UI 初始扩展已完成：Provider list/select/edit/save/set-active/delete/reorder、Provider import/export preview/apply、Gateway/Apply、Diagnostics summary/package 操作已进入纯 Rust UI；编辑 Provider 时空 API key 会保留既有 key，避免密码框不回显造成误清空。
- P22 report issue 本地动作初始边界已建立：diagnostics summary 可复制到系统剪贴板，diagnostics package 可保存到 config 旁 `diagnostics/` 目录，GitHub Issue draft 可生成并打开；issue title/body/url 均走 redaction，不上传 release、不发布资产。
- P23 smoke check 初始边界已建立：Provider static smoke、local gateway `/v1/models` smoke、Provider real smoke command 已接入 state/Tauri/UI；缺 API key 时 real smoke 会在联网前返回 `provider.api_key_missing`，上游错误会走 redacted `provider.real_smoke_failed`。
- P24 model mapping / config backup 初始边界已建立：Provider model mappings 可通过 Rust state/Tauri command/Leptos UI 读取和更新；非 `Default` route 必须是 `claude-*` 且不能重复，`Default` 继续强制不进 runtime；config backups 可列出并按文件名读取，返回 UI 前走 diagnostics redaction。
- P25 Provider preset import 初始边界已建立：内置 DeepSeek/Kimi Provider presets 可 list / preview / import / replace；preset 使用显式 `claude-*` safe routes，冲突沿用 dry-run/replace，summary 不回显 API key。
- P26 Tauri file picker 初始边界已建立：接入官方 `tauri-plugin-dialog`，Provider export 和 diagnostics package 都有 Save as 命令/UI，可区分 cancel 与已保存路径；diagnostics save-as 继续使用 redacted diagnostics package。
- P27 gateway runtime logs 初始边界已建立：gateway start/stop/no-provider/route/port/start failure 等事件进入内存 runtime log，diagnostics package 带 redacted `runtimeLogs`，summary 显示 runtime log 数量。
- P28 release manifest gate 初始边界已建立：`cargo xtask verify --stage release` 会校验 `latest.json` JSON、`latest.json.sha256`、`latest.json.sig`、public key、每个资产的 `.sha256`/`.sig`，并强制 Windows x64、macOS arm64、macOS x64 required asset IDs；缺 macOS x64 会失败。
- P29 single-instance app shell 初始边界已建立：接入官方 `tauri-plugin-single-instance`，插件在 dialog 等插件前注册；第二次启动会向现有 app 发出 `single-instance` event，并 show/focus 主窗口。
- P30 tray close behavior 初始边界已建立：启用 Tauri `tray-icon` feature；窗口 close request 会 prevent close 并隐藏主窗口；托盘菜单支持 Show 和 Quit；左键托盘图标会恢复并聚焦主窗口。
- P31 Provider import merge UX 初始边界已建立：默认冲突仍阻断；`skipExisting` 模式会跳过冲突 Provider、只导入新增 Provider，并保留既有 Provider/API key；Leptos UI 增加 Preview new only / Import new only 和冲突摘要。
- Windows packaged app shell smoke 已通过：`target\release\cc-desktop-switch.exe` 使用临时 `CCDS_CONFIG_FILE` 启动；第二次启动退出且只保留一个进程；关闭主窗口后进程保留且标题窗口不可见；再次启动恢复主窗口。
- P32 Windows PDB collision cleanup 已完成：内部 lib crate 从 `cc_desktop_switch` 改为 `cc_desktop_switch_lib`，exe 名仍为 `cc-desktop-switch`；最新 `cargo xtask verify --all` 不再出现 PDB filename collision warning。
- P33 Windows real Claude Desktop local config smoke harness 已建立：新增显式 opt-in ignored test，负责真实路径 probe、文件级备份、write/readback、loopback gateway smoke 和恢复校验；本机实跑被 `HKCU\SOFTWARE\Policies\Claude` managed policy 阻断，临时删除该 key 返回 Access denied，测试没有写入 `%LOCALAPPDATA%\Claude-3p\configLibrary`；UI probe/apply 结果会解释 managed policy blocker。
- P34 Provider template import 初始边界已建立：支持粘贴无密钥 `ccds.providerTemplate` JSON，通过现有 dry-run/conflict/replace/import 流程导入；模板导入接受 `openai_chat` / `open_ai_chat`，强制 `claude-*` route，继续压制 `Default` runtime route，并拒绝重复 template ID。
- P35 旧 managed policy 诊断细分已建立：Windows probe 会识别 `ccds_managed=true` 并额外给出 `desktop.ccds_managed_policy_detected`，不读取或输出 gateway key 等 secret 值。
- P36 Windows managed policy cleanup runbook 已建立：新增默认只读维护脚本 `scripts/windows/ccds-managed-policy-maintenance.ps1` 和 `project-docs/runbooks/windows-managed-policy-cleanup.md`；`status` 模式已在本机验证，只输出 value names 并识别 `ccdsManaged=True`，cleanup 必须显式 opt-in 且先导出 `.reg` 备份。
- P37 macOS Rust mainline platform smoke 路径已建立：新增非发布 workflow `.github/workflows/rust-mainline-platform-smoke.yml`，覆盖 `macos-14` arm64 和 `macos-15-intel` x64，执行 Rust/UI/Tauri gate、app bundle/DMG/PKG smoke 并上传 workflow artifacts；旧 release workflow 的 manifest required platforms 已补 `macos-x64`，缺 x64 资产时必须失败。
- P38 release manifest macOS x64 默认门禁已补齐：`scripts/New-ReleaseManifest.ps1` 默认 `RequiredPlatforms` 已包含 `macos-x64`，本地离线 smoke 验证缺 x64 pkg/dmg 会失败，完整 Windows x64 + macOS arm64 + macOS x64 fixture 会生成 `latest.json`、sha256、sig 和 public key。
- P39 release metadata directory verifier 已补强：`src-tauri/src/release_gate.rs` 可从 staging directory 读取 `latest.json`，校验 `latest.json.sha256`、`latest.json.sig`、public key、manifest 引用资产文件、资产 `.sha256`、资产 `.sig`，并保留 macOS x64 必需资产门禁；`cargo xtask verify --stage release` 覆盖完整目录、missing referenced asset、missing sidecars/public key 和 invalid `latest.json` fixture。
- P40 Provider template 安全边界已补强：`ccds.providerTemplate` 导入会拒绝 `apiKey`、gateway key、Authorization、Cookie、headers、secret、token 等 secret-bearing 字段，模板 `baseUrl` 必须是 `http://` 或 `https://`；`cargo xtask verify --stage provider-import` 覆盖 10 个导入测试。
- P41 Leptos UI readiness dashboard 已补强：首页状态卡绑定最新 `health` snapshot，readiness list 显示 static config / desktop readback / provider smoke / gateway smoke 的 pass/check/pending 状态，首页增加 Health / Apply / Report issue 常用操作条；仍保持纯 Rust UI，没有新增手写 JS 业务逻辑。
- P42 Provider marketplace 离线 manifest 安全边界已建立：新增 `ccds.providerMarketplace` import source，只接受 `https://` 且无 query/fragment/userinfo 的 source URL，要求内嵌 `ccds.providerTemplate` package 的 canonical sha256 匹配，再复用 P40 secretless/safe-route/Default 校验；这仍不是联网 marketplace，也不是签名信任链。
- P43 Windows packaged app smoke 已用最新 build 重跑通过：使用临时 `CCDS_CONFIG_FILE` 启动 `target\release\cc-desktop-switch.exe`，第二次启动退出且只保留一个进程，关闭主窗口后进程保留且 `CC Desktop Switch` 主窗口不可见，第三次启动退出并恢复主窗口；测试后已停止测试进程。
- P44 release sha256 内容校验已补强：`validate_release_directory` 不只检查 `.sha256` sidecar 存在，还会读取 `latest.json` 和每个 manifest asset 的实际 bytes，计算 sha256 并和 sidecar 内容比对；hash mismatch 或非法 sidecar 会失败。
- P45 release signature 内容校验已补强：`validate_release_directory` 会解析 `RSA-CSP-BLOB-SHA256` public key，校验 `latest.json.sig` 和 manifest asset `.sig` 的 base64 形状与 RSA/SHA256 签名匹配；Windows-only 测试会实际调用 `scripts/New-ReleaseManifest.ps1` 生成签名并由 Rust verifier 校验。
- P46 RC readiness audit stage 已建立：`cargo xtask verify --stage rc-readiness` 会输出 prompt-to-artifact checklist；当前本地运行 7 项 pass、3 项 missing，并因 Windows real Desktop smoke、macOS arm64/x64 workflow smoke、macOS real Desktop smoke 缺证据而返回非零退出码。
- P47 macOS real Claude Desktop local config smoke harness 已建立：macOS ignored test 复用 Windows real smoke 的备份、写入、读回、loopback gateway smoke 和恢复校验 helper；Windows 本机 guard run 只验证了平台 skip，真实 macOS 实机仍未执行。
- P48 macOS platform smoke evidence artifact 已建立：`.github/workflows/rust-mainline-platform-smoke.yml` 会把 `platform-smoke-evidence.md` 与 pkg/dmg 一起作为 non-release workflow artifact 上传；`rc-readiness` 会静态检查该 evidence artifact 路径，但仍要求真实 arm64/x64 workflow handoff 证据。
- P49 current-platform full gate 已刷新：`cargo xtask verify --all` 在 Windows x64 通过，包含 fmt、workspace tests、clippy、`trunk build --release` 和 `cargo tauri build`；Tauri build 产出 MSI 与 NSIS bundle。只读 blocker 复核显示 `HKCU\SOFTWARE\Policies\Claude` 仍 `ccdsManaged=True`，`%LOCALAPPDATA%\Claude-3p\configLibrary` 仍不存在。
- P50 Windows real Desktop smoke evidence wrapper 已建立：`scripts/windows/run-real-desktop-smoke.ps1` 默认 `preflight` 只读，写入 `target\real-desktop-smoke\windows-real-desktop-smoke-evidence.md`；`run` 模式必须显式传 `-AllowRealDesktopWrite` 才会设置 `CCDS_ALLOW_REAL_DESKTOP_WRITE=1` 并调用 ignored real smoke。
- P51 macOS real Desktop smoke evidence wrapper 已建立：`scripts/macos/run-real-desktop-smoke.sh` 默认 `preflight` 只读，写入 `target/real-desktop-smoke/macos-real-desktop-smoke-evidence.md`；`run` 模式必须显式传 `--allow-real-desktop-write` 才会设置 `CCDS_ALLOW_REAL_DESKTOP_WRITE=1` 并调用 ignored real smoke；Windows Git Bash 验证返回 `UnsupportedPlatform`，未写 Desktop 配置。
- P52 real Desktop smoke evidence marker 对齐已完成：Windows/macOS wrapper 生成的 evidence 增加 `Readiness Markers`，与 `cargo xtask verify --stage rc-readiness` 的 handoff 匹配关键词对齐；preflight/unsupported 仍不会被误判为 pass，因为 readiness 仍要求 `## Result` + `Pass`。
- P53 Windows real Desktop smoke wrapper 静态门禁已补齐：`rc-readiness` 现在会检查 Windows wrapper 的 evidence fingerprint、test name、`-AllowRealDesktopWrite`、`CCDS_ALLOW_REAL_DESKTOP_WRITE` 和 `Readiness Markers`；本地运行结果为 10 pass / 3 missing，仍因真实 Windows/macOS smoke evidence 缺失而返回非零。
- P54 macOS platform workflow 静态门禁已补强：`rc-readiness` 现在会检查 `workflow_dispatch`、arm64/x64 `expected_uname`、`uname -m`、Rust/UI/Tauri gate、Info.plist/DMG/PKG smoke、artifact name 和 retention；官方 GitHub hosted runner 文档确认 `macos-14` 是 arm64、`macos-15-intel` 是 Intel；本地运行结果为 11 pass / 3 missing，仍因真实 Windows/macOS smoke evidence 缺失而返回非零。
- P55 macOS platform smoke evidence collector 已建立：`scripts/macos/Collect-PlatformSmokeEvidence.ps1` 会校验下载后的 arm64/x64 `platform-smoke-evidence.md`，要求 result pass、fingerprint、runner、actual uname、Rust/UI/Tauri/DMG/PKG markers，并生成可匹配 `rc-readiness` 的 combined handoff；本地 fixture 自检通过，`rc-readiness` 现在为 12 pass / 3 missing 的预期 incomplete。
- P56 Windows real Desktop smoke evidence collector 已建立：`scripts/windows/Collect-RealDesktopSmokeEvidence.ps1` 会校验 `run-real-desktop-smoke.ps1` 产出的 pass evidence 和 cargo test log，要求 `mode: run`、`exit_code: 0`、`## Result` / `Pass`、`test result: ok`、`loopback gateway`、`restored`，并生成可匹配 `rc-readiness` 的 handoff；本地 fixture 自检通过，`rc-readiness` 现在为 13 pass / 3 missing 的预期 incomplete。
- P57 macOS real Desktop smoke evidence collector 已建立：`scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` 会校验 `run-real-desktop-smoke.sh` 产出的 pass evidence 和 cargo test log，要求 `platform: Darwin`、`mode: run`、`exit_code: 0`、`## Result` / `Pass`、`test result: ok`、`configLibrary`、`safe route`，并生成可匹配 `rc-readiness` 的 handoff；本地 fixture 自检和 Windows Git Bash `UnsupportedPlatform` 拒绝验证通过，`rc-readiness` 现在为 14 pass / 3 missing 的预期 incomplete。
- P58 RC readiness final evidence matching 已收紧：`rc-readiness` 对最终 Windows/macOS real Desktop smoke handoff 额外要求 collector-style fingerprint、test name、evidence/log 字段；对 macOS platform handoff 额外要求 arm64/x64 workflow run 和 artifact 字段，避免自由文本总结误判为 pass；本地运行仍为 14 pass / 3 missing 的预期 incomplete。
- P59 current-platform full gate 已刷新：`cargo xtask verify --all` 在当前 Windows x64 工作树通过，包含 `cargo fmt --all -- --check`、`cargo test --workspace`（110 passed, 2 ignored real Desktop smoke）、`cargo clippy --workspace --all-targets -- -D warnings`、`trunk build --release`、`cargo tauri build`，并产出 MSI 与 NSIS bundle；补跑 `cargo xtask verify --stage rc-readiness` 仍为 14 pass / 3 missing，确认本机 full-gate handoff 没有误满足真实 Windows/macOS evidence。
- P60 external evidence execution decision card 已补：剩余 Windows/macOS 真实证据门禁被整理成显式授权项，记录在 `project-docs/handoff/2026-05-09-p60-external-evidence-execution-decision-card.md`；该卡不是 pass evidence。
- P61 Windows managed-policy cleanup 授权执行被本机权限阻断：`status` 确认 `HKCU\SOFTWARE\Policies\Claude` 存在且 `ccdsManaged=True`；`cleanup` 已先导出备份 `C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-20260509152525.reg`，随后 `reg delete` 返回 `Access is denied`；preflight 仍显示 `configLibraryExists=False`，`cargo xtask verify --stage rc-readiness` 仍为 14 pass / 3 missing，未生成 Windows pass handoff。
- P62 Windows real Claude Desktop local config smoke 已通过：按 runbook 通过 elevated PowerShell 清理旧 `HKCU\SOFTWARE\Policies\Claude`，额外备份为 `C:\Users\15618\AppData\Local\CC Desktop Switch\policy-backups\claude-policy-elevated-20260509153014.reg`；`status` 确认 policy 不存在，`scripts/windows/run-real-desktop-smoke.ps1 -Mode run -AllowRealDesktopWrite` 通过，collector 生成 `project-docs/handoff/2026-05-09-windows-real-desktop-smoke-evidence-summary.md`；`cargo xtask verify --all` 在 P62 后重新通过（110 passed, 2 ignored real Desktop smoke；clippy/UI/Tauri build 通过并产出 MSI/NSIS），`cargo xtask verify --stage rc-readiness` 现在确认 Windows pass，剩余 15 pass / 2 missing（macOS platform workflow、macOS real Desktop smoke）。
- P63 macOS workflow 远端可触发性只读核查完成：`gh workflow list --all` 远端仅有 `Release`；`gh workflow view rust-mainline-platform-smoke.yml` 返回 404，说明本地 `.github/workflows/rust-mainline-platform-smoke.yml` 尚未存在于 GitHub 默认分支；当前 `codex/rust-mainline-rewrite` 仍跟踪 `origin/main` 且没有远端分支证据。在不 push/PR 的边界下，无法取得 macOS arm64/x64 workflow 实跑证据。
- P64 macOS workflow 已接入 real Desktop local config smoke 自动化路径：`rust-mainline-platform-smoke.yml` 每个 macOS matrix job 会用临时 `HOME` 跑 `scripts/macos/run-real-desktop-smoke.sh --mode run --allow-real-desktop-write`，并把 `macos-real-desktop-smoke-evidence.md` 与 cargo log 放入 workflow artifact；`run-real-desktop-smoke.sh` 现在在 evidence 中写相对 log 文件名，便于下载到 Windows 后 collector 解析；`cargo xtask verify --stage rc-readiness` 新增 workflow real-smoke 静态项通过，临时 fixture 验证 `Collect-RealDesktopSmokeEvidence.ps1` 可解析相对 log。该阶段仍不是 macOS pass evidence。
- P65 macOS workflow 远端触发路径调整：测试分支已 push 并创建 draft PR #21；`gh workflow run rust-mainline-platform-smoke.yml --ref codex/rust-mainline-rewrite` 因新 workflow 不在默认分支返回 404，draft PR 初始也没有 checks；因此 non-publishing workflow 增加仅限 `codex/**` 分支和主线相关路径的 `push` trigger，用下一次测试分支 push 触发 macOS smoke。该阶段仍不触发 `Release`，也不发布 `latest.json`。
- P66 首次远端 macOS workflow 已真实触发但失败在 Rust workspace gate：run `25596145265` 的 arm64/x64 runner 架构校验均通过，失败根因为 Tauri `generate_context!()` 在 macOS 拒绝 RGB PNG icon（`frontend/assets/app-icon.png is not RGBA`）；已将该 PNG 转为 RGBA，并把 Windows-only PowerShell release test 的 `std::process::Command` import 加上 `#[cfg(windows)]`。仍需重新 push 触发 workflow。
- P67 第二轮远端 macOS workflow 通过 Rust workspace gate 和 Leptos build，但失败在 Tauri macOS app bundle icon：run `25596697486` 的 arm64/x64 job 均在 `Build Tauri app` 报 `Failed to create app icon: No matching IconType`；已将 `frontend/assets/app-icon.png` 规范化为 1024x1024 RGBA，新增 `frontend/assets/app-icon.icns`，并把 `.icns` 写入 `src-tauri/tauri.conf.json`。仍需重新 push 触发 workflow。
- P68 第三轮远端 macOS workflow 已通过 Rust gate、Leptos build、Tauri build、bundle smoke、DMG verify 和 PKG create/expand，但失败在 real Desktop smoke 脚本执行权限：run `25597520723` 的 arm64/x64 job 均报 `scripts/macos/run-real-desktop-smoke.sh: Permission denied`；已将 workflow 改为 `bash scripts/macos/run-real-desktop-smoke.sh ...`，并给该脚本设置 git executable bit。仍需重新 push 触发 workflow。
- P69 第五轮远端 macOS workflow 已通过：run `25599626985` 的 macOS arm64 和 macOS x64 job 均通过 Rust gate、Leptos build、Tauri build、bundle smoke、DMG verify、PKG create/expand 和 real Desktop smoke；已下载 artifacts 并运行 `scripts/macos/Collect-PlatformSmokeEvidence.ps1` 与 `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1`，生成 macOS platform 和 real Desktop pass handoff；`cargo xtask verify --stage rc-readiness` 已通过。
- P70 UI/UX 主导航收尾已完成：Leptos UI 现在有真实 Dashboard / Provider / Diagnostics / Settings 四区，首页只保留状态和主操作，Provider 区承载配置/导入/映射，Diagnostics 区承载 Gateway/Apply/readiness/diagnostics，Settings 区承载语言/主题和 gateway 默认项；Playwright 已检查桌面和移动视口，`trunk build --release`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo tauri build` 通过。
- P71 legacy UI parity 已完成本地验证：P70 的四区信息架构保留，但视觉回到旧版 CC Desktop Switch 的主要布局感受；Dashboard 恢复三张大状态卡、三枚大操作按钮和最近操作面板；Provider 恢复“添加提供商 + 快捷预设”双栏；高级 import/export、model mapping、backup 下沉到 Provider 下方；Playwright 桌面/移动截图保存在 `target/ui-smoke/p71/`，console 为 0 error / 1 个既有 Trunk/SRI warning；`cargo fmt --all -- --check`、`trunk build --release`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo tauri build`、`cargo xtask verify --stage rc-readiness` 均通过。下一步是 push 触发当前 UI commit 的 non-publishing macOS arm64/x64 workflow。
- P72 current frontend parity 已完成本地验证：P71 被用户指出不是当前最新前端基线，因此不再作为 UI 方向；P72 改为只读核对 `D:\cc desktop swtich\frontend\index.html`、`frontend\css\style.css`、`frontend\js\app\20-routes.js`、`frontend\js\app\10-ui-core.js` 后重做 Rust/Leptos UI。
- P73 desktop UI parity correction 已完成本地验证：P72 被用户指出仍未完美复刻且误把移动端作为目标；P73 明确 Rust 主线 UI 是桌面应用 UI，移动端不是验收目标。Rust/Leptos UI 现在按当前前端桌面形态收紧：顶部动作和 5 个 icon tab 使用图标样式，Dashboard 去掉错误的“选择当前提供商”标题并恢复 provider switch-board + 继续添加提供商区，Provider 卡片和预设卡片使用当前 provider assets，Add Provider 使用空表单初始态、双栏快捷预设、红框第三方兼容接口、行式模型映射和橙色“一键应用到 Claude 桌面版”按钮。桌面 Playwright 截图保存在 `target/ui-smoke/p73/`，覆盖 `dashboard-desktop-r3.png` 和 `provider-add-desktop-r2.png`；mocked Tauri bridge 下 console 为 0 error / 1 个既有 Trunk/SRI warning；不再跑移动端截图作为验收项。P73 已通过 `cargo fmt --all -- --check`、`trunk build --release`、`cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo tauri build`、`cargo xtask verify --stage rc-readiness`。
- RC1 readiness audit 已写入 `project-docs/handoff/2026-05-09-rc1-readiness-audit.md`：当前结论是不应标记完成，缺口是 macOS arm64/x64 workflow 实跑记录、macOS 实机 Claude Desktop local config smoke。
- Windows release GUI subsystem 已设置，release build 不应打开黑终端。
- 当前 Windows x64 本地门禁已通过：P59 最新 `cargo xtask verify --all` 在当前工作树通过，内部包含 `cargo fmt --all -- --check`、`cargo test --workspace`（110 passed, 2 ignored real Desktop smoke）、`cargo clippy --workspace --all-targets -- -D warnings`、`trunk build --release`、`cargo tauri build`，并产出 Windows MSI/NSIS bundle；release manifest PowerShell smoke 也已覆盖 macOS x64 默认必需项。P39-P43 后本轮又运行了 `cargo fmt --all -- --check`、`cargo xtask verify --stage release`、`cargo xtask verify --stage provider-import`（13 provider import tests）、`trunk build --release`、`cargo test --workspace`（103 passed, 1 ignored real Desktop smoke）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo tauri build` 和 Windows packaged app smoke。P44 后已运行 `cargo fmt --all -- --check`、`cargo xtask verify --stage release`（10 release gate tests）、`cargo test --workspace`（106 passed, 1 ignored real Desktop smoke）、`trunk build --release`、`cargo clippy --workspace --all-targets -- -D warnings` 和 `cargo tauri build`。P45 后已运行 `cargo fmt --all -- --check`、`cargo xtask verify --stage release`（14 release gate tests，含 PowerShell signature compatibility fixture）、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`（110 passed, 1 ignored real Desktop smoke）、`trunk build --release` 和 `cargo tauri build`。P46 后已运行 `cargo fmt --all -- --check`、`cargo test --workspace`（110 passed, 1 ignored real Desktop smoke）、`cargo clippy --workspace --all-targets -- -D warnings`，并运行 `cargo xtask verify --stage rc-readiness` 得到预期 incomplete（7 pass / 3 missing）。P47 已运行 `cargo fmt --all -- --check`、macOS real smoke guard test（Windows 平台 skip passed）、Windows real smoke env guard test（未设置写入环境变量 skip passed）、`cargo test --workspace`（110 passed, 2 ignored real Desktop smoke）、`cargo clippy --workspace --all-targets -- -D warnings`，并确认 `cargo xtask verify --stage rc-readiness` 仍预期 incomplete（3 missing）。P48 已运行 `cargo fmt --all -- --check`、`cargo test --workspace`（110 passed, 2 ignored real Desktop smoke）、`cargo clippy --workspace --all-targets -- -D warnings`，并确认 `cargo xtask verify --stage rc-readiness` 新增 evidence artifact 静态项通过但整体仍预期 incomplete（3 missing）。P49 又重跑 `cargo xtask verify --all` 并通过，随后只读确认 Windows managed policy blocker 仍存在。P53 已运行 `cargo fmt --all -- --check`，并确认 `cargo xtask verify --stage rc-readiness` 为 10 pass / 3 missing 的预期 incomplete。P54 已运行 `cargo fmt --all -- --check`，并确认 `cargo xtask verify --stage rc-readiness` 为 11 pass / 3 missing 的预期 incomplete。P55 已运行 `scripts/macos/Collect-PlatformSmokeEvidence.ps1` fixture 自检、`cargo fmt --all -- --check`，并确认 `cargo xtask verify --stage rc-readiness` 为 12 pass / 3 missing 的预期 incomplete。P56 已运行 `scripts/windows/Collect-RealDesktopSmokeEvidence.ps1` fixture 自检、`cargo fmt --all -- --check`，并确认 `cargo xtask verify --stage rc-readiness` 为 13 pass / 3 missing 的预期 incomplete。P57 已运行 `scripts/macos/Collect-RealDesktopSmokeEvidence.ps1` fixture 自检、Windows Git Bash `UnsupportedPlatform` 拒绝验证、`cargo fmt --all -- --check`，并确认 `cargo xtask verify --stage rc-readiness` 为 14 pass / 3 missing 的预期 incomplete。P58 已运行 `cargo fmt --all -- --check`，并确认收紧后的 `cargo xtask verify --stage rc-readiness` 仍为 14 pass / 3 missing 的预期 incomplete。
- P21-P26、P31 和 P41 UI 浏览器验证已执行：`trunk serve --address 127.0.0.1 --port 1421 --open false` 后用 Playwright 检查桌面 1440x1000 和移动 390x900 视口，页面无 console error；P41 仅有 Trunk/SRI Chromium warning。Trunk dev cache 偶尔返回旧 error overlay，手动 `trunk build` 后重启 trunk serve 并 cache-buster 刷新消失。
- Windows Tauri bundle metadata 版本必须使用 numeric `1.1.0`；`v1.1.0-rc1` 继续作为阶段/发布候选名记录在文档和后续 release metadata 中。
- Python 稳定线先独立合并/处理社区 PR；Rust 主线吸收稳定线确认后的行为，不在本 worktree 继续修补 Python runtime。
- 旧 Tauri 分支 `D:\cc desktop swtich` 是参考资料，不再继续叠补丁。

## adopted_decisions

- 本机 gateway 是唯一普通用户主路径。
- 直连模式从普通 UI 删除，只保留隐藏高级调试能力。
- Claude Desktop 不直接看到原始第三方模型名。
- 默认只显示当前默认 Provider 的显式映射模型。
- `Default` 只做表单/配置便利项，不参与任何 runtime fallback，也不进入 Claude Desktop 模型菜单。
- 删除普通 UI 中的“显示全部 Provider 模型”；隐藏高级调试入口不得影响 Claude Desktop 模型菜单。
- “实验转发模式”统一改名为“本机 gateway”。
- Tauri 桌面管理界面不使用本地 Admin HTTP API。
- 新版必须提供“报告问题”：复制诊断摘要、导出诊断包、打开 GitHub Issue。
- 一键应用是完整流程，任何一步失败都不能显示“已应用”。
- Codex workflow 采用 `AGENTS.md + PLANS.md + repo-local skills + eval harness`，不是单靠聊天上下文。
- macOS x64 是 `v1.1.0-rc1` 硬门禁，不能作为首个 RC 的 deferred 项。
- 稳定线 PR 先在 Python 稳定线合并/验证，Rust 主线保持独立重构，只吸收最终行为和测试要求。

## known_blockers

- P1-P32 仍是最小主线边界，不包含 update 迁移、外部 preset marketplace / 模板导入、带真实 API key 的 Provider smoke 现场验证。
- provider service 已持久化到 config path，edit/delete/reorder/import/export/model-mapping/preset-import/template-import/backup-list 初始边界已覆盖；Provider import 已有 skip-existing merge UI、无密钥 template package 导入和离线 marketplace manifest 校验，模板导入会拒绝 secret-bearing 字段和非 HTTP(S) `baseUrl`；尚未实现联网 signed preset marketplace。
- Desktop planner 生成 platform-neutral plan 和 readback health；local configLibrary writer、path probe、managed config conflict block、fixture apply transaction、user-facing Apply command、Windows/macOS real smoke harness 和 evidence wrapper 已接入；Windows 旧 `HKCU\SOFTWARE\Policies\Claude` managed policy 已在 P62 通过 elevated cleanup 清理并保留 `.reg` 备份，Windows real local config smoke 已通过 backup/readback/loopback gateway/restore 证据链；macOS real local config smoke 仍需实机执行。
- Gateway 目前有 Axum router/server skeleton、核心映射/拒绝规则、Anthropic/OpenAI upstream request conversion、非流式 HTTP forwarding、OpenAI Chat response -> Anthropic response conversion、SSE event-stream runtime forwarding、OpenAI streaming chunk -> Anthropic-style semantic conversion、redacted upstream error envelope、Tauri state lifecycle 和 app startup 尝试启动。
- Diagnostics 已有 readiness、redaction core、copy summary、结构化 export package、gateway runtime logs、本地保存、file picker save-as、系统剪贴板复制和打开 GitHub Issue draft。
- Pure Rust UI spike 的核心 command bridge / build / package 已通过；UI 已能触发 provider CRUD/reorder/import/export/save-as/preset-import、model mapping edit、config backup readback、health、gateway status-start-stop、desktop config probe、apply dry-run/apply、diagnostics summary/package/save-as；首页 readiness dashboard 已绑定 `health` snapshot 并提供 Health / Apply / Report issue 常用操作；app shell 已接入 single-instance 和 tray close-to-hide，最新 Windows packaged app smoke 已重跑通过；仍需后续补复杂键盘流、启动时间/包体积基线和 macOS bundle smoke。
- macOS 1M / configLibrary 问题需要 macOS 1.6259.1+ 实机验证；macOS arm64/x64 CI build 和 bundle smoke 路径及 evidence artifact 已建立但尚未在 GitHub Actions 或实机上运行；macOS real local config smoke harness 和 evidence wrapper 已建立但尚未在 macOS 实机执行。
- Windows 更新安装器不弹出问题需要能复现的 Windows 11 更新路径验证。
- OpenCode Go / new-api / 中转兼容性需要用户提供可测 API key 或脱敏诊断包。
- `project-docs/decisions/2026-05-08-config-migration-and-route-identity.md` 已确认 `Default` 只做配置便利项；route rename 策略仍需实现时验证。

## next_actions

1. Push P73 后等待 non-publishing macOS arm64/x64 platform smoke workflow；通过后下载 artifacts 并刷新 macOS evidence handoff。
2. 按 `project-docs/runbooks/final-human-rc-smoke.md` 执行最终人工测试：Windows x64、macOS arm64、macOS x64。
3. 人工测试通过后，再决定是否进入 release metadata / signing / publish 准备；当前仍不要打 tag、不要 GitHub Release、不要更新 `latest.json`。
4. 如继续做 preset marketplace，需要在 P42 离线 manifest/hash gate 之上补签名信任链、远端 fetch 策略和 UI trust wording；不要直接启用联网拉取。
5. 等真实 macOS artifacts 都产出后，用 release manifest gate 和 directory verifier 复核 `latest.json` 引用、sha256、sig、public key 和平台资产完整性。

## key_entrypoints

- 项目规则：`AGENTS.md`
- 执行计划：`PLANS.md`
- 文档入口：`project-docs/README.md`
- 产品/架构决策：`project-docs/decisions/2026-05-08-rust-mainline-product-and-architecture.md`
- Codex workflow 决策：`project-docs/decisions/2026-05-08-agent-workflow-skills-and-plans.md`
- 配置迁移/route 身份决策：`project-docs/decisions/2026-05-08-config-migration-and-route-identity.md`
- 稳定线/Rust 主线分支策略：`project-docs/decisions/2026-05-08-stable-line-and-rust-mainline-branching.md`
- 重构任务卡：`project-docs/handoff/2026-05-08-rust-mainline-rebuild-task-card.md`
- P1 阶段总结：`project-docs/handoff/2026-05-08-p1-rust-ui-spike-summary.md`
- P2 model catalog 总结：`project-docs/handoff/2026-05-08-p2-model-catalog-boundary-summary.md`
- P3 config migration 总结：`project-docs/handoff/2026-05-08-p3-config-migration-boundary-summary.md`
- P4 provider service 总结：`project-docs/handoff/2026-05-08-p4-provider-service-boundary-summary.md`
- P5 Desktop planner 总结：`project-docs/handoff/2026-05-08-p5-desktop-planner-boundary-summary.md`
- P6 gateway core 总结：`project-docs/handoff/2026-05-08-p6-gateway-core-boundary-summary.md`
- P7/P8 post-review hardening 总结：`project-docs/handoff/2026-05-08-p7-post-review-hardening-summary.md`
- P9 upstream adapter 总结：`project-docs/handoff/2026-05-08-p9-upstream-adapter-boundary-summary.md`
- P10 upstream forwarding 总结：`project-docs/handoff/2026-05-08-p10-upstream-forwarding-boundary-summary.md`
- P11 SSE runtime 总结：`project-docs/handoff/2026-05-08-p11-sse-runtime-boundary-summary.md`
- P12 gateway lifecycle 总结：`project-docs/handoff/2026-05-08-p12-gateway-lifecycle-boundary-summary.md`
- P13 Desktop writer 总结：`project-docs/handoff/2026-05-08-p13-desktop-local-config-writer-summary.md`
- P14 apply flow 总结：`project-docs/handoff/2026-05-08-p14-apply-flow-fixture-summary.md`
- P15/P16 Desktop config probe 与 Apply 总结：`project-docs/handoff/2026-05-08-p15-p16-desktop-config-probe-apply-summary.md`
- P17 OpenAI stream conversion 总结：`project-docs/handoff/2026-05-08-p17-openai-stream-conversion-summary.md`
- P18 Provider CRUD/reorder 总结：`project-docs/handoff/2026-05-08-p18-provider-crud-reorder-summary.md`
- P19 Provider import/export 总结：`project-docs/handoff/2026-05-08-p19-provider-import-export-summary.md`
- P20 diagnostics export package 总结：`project-docs/handoff/2026-05-08-p20-diagnostics-export-package-summary.md`
- P21 Leptos Provider/Diagnostics UI 总结：`project-docs/handoff/2026-05-08-p21-leptos-provider-diagnostics-ui-summary.md`
- P22 report issue actions 总结：`project-docs/handoff/2026-05-08-p22-report-issue-actions-summary.md`
- P23 smoke checks 总结：`project-docs/handoff/2026-05-08-p23-smoke-checks-summary.md`
- P24 model mapping / backup 总结：`project-docs/handoff/2026-05-08-p24-model-mapping-backup-summary.md`
- P25 Provider preset import 总结：`project-docs/handoff/2026-05-08-p25-provider-preset-import-summary.md`
- P26 file picker save-as 总结：`project-docs/handoff/2026-05-08-p26-file-picker-save-as-summary.md`
- P27 gateway runtime logs 总结：`project-docs/handoff/2026-05-08-p27-gateway-runtime-logs-summary.md`
- P28 release manifest gate 总结：`project-docs/handoff/2026-05-08-p28-release-manifest-gate-summary.md`
- P29 single-instance app shell 总结：`project-docs/handoff/2026-05-08-p29-single-instance-app-shell-summary.md`
- P30 tray close behavior 总结：`project-docs/handoff/2026-05-08-p30-tray-close-behavior-summary.md`
- P31 Provider import merge UX 总结：`project-docs/handoff/2026-05-08-p31-provider-import-merge-ux-summary.md`
- P32 Windows PDB collision cleanup 总结：`project-docs/handoff/2026-05-08-p32-windows-pdb-collision-cleanup-summary.md`
- 已知问题登记：`project-docs/bugs/2026-05-08-known-issues-root-cause-register.md`
- 主线 runbook：`project-docs/runbooks/rust-mainline-workflow.md`
- 功能等价矩阵：`docs/testing/python-rust-parity-matrix.md`
- 本地 eval harness：`docs/testing/eval-harness.md`
