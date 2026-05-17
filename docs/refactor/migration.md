# 迁移方案:Python → Tauri 2 + Rust + Leptos

> 状态:**已确认,启动 Stage 0**
> 起草:2026-05-03(对话内由 Claude 起草,经用户确认)
> 适用范围:`codex-app-transfer` 全仓
> 目标版本:v2.0.0(Python 主线 v1.x 继续作为 hotfix 通道至少 1 个版本周期)

---

## 1. 决策摘要

将现有 Python(FastAPI + pywebview + pystray + PyInstaller)+ Bootstrap/原生 JS + 多种构建脚本(.bat/.ps1/.sh/.spec/.nsi)的混合栈,统一迁移到 **Rust 全栈**:

- **桌面壳**:Tauri 2(原生 WebView,体积/启动远优于 Electron 与 PyInstaller)
- **业务后端**:Rust(axum + reqwest + tokio + serde),替代 `backend/*.py` 与 `main.py` 中的代理与适配逻辑
- **前端**:Leptos(Rust→WASM,RSX 语法),替代 HTML + Bootstrap + 原生 JS
- **构建/打包**:`cargo tauri build` + `xtask`,替代 PyInstaller spec / NSIS / .bat / .ps1 / .sh
- **最终语言占比**:Rust ≥ 95%,其余为 TOML、`index.html`、少量 CSS

### 为什么不是其他方案

| 方案 | 否决理由 |
|---|---|
| Electron + TS | 二进制 ~150 MB、启动慢,与"更快"目标矛盾 |
| Wails + Go | 仍需 Go + JS 双语言,统一度低于 Tauri+Leptos |
| 纯 Rust 原生 GUI(Slint/egui/iced) | 视觉离 Bootstrap 当前观感差距明显,与"美观"目标矛盾 |
| Tauri + SolidJS/Svelte | 仍需 Rust + TS,统一度次优;**保留为兜底**,若 Leptos 写起来阻力过大可退到此方案 |

### 取舍承认

- **迁移成本高**:12.8k 行 Python(其中 933 行 `provider_workarounds.py`、972 行 `responses_adapter.py` 是边缘 case 的密集区)需逐步翻译
- **Leptos 生态比 React/Vue 小**,常见组件库少;但本项目 UI 不复杂(5 个路由、表单为主),可承受
- **WASM 包体积**:Stage 0 实测 release+wasm-opt 后仅 **93 KB**(原估 300-800 KB),远低于预期;远小于 PyInstaller 25 MB,与 SolidJS 量级相当

---

## 2. 当前代码盘点

| 层 | 语言/技术 | 行数/规模 |
|---|---|---|
| 核心后端(代理 + 多家厂商适配器) | Python(FastAPI/uvicorn/httpx) | ~12,200 行 / 18 个 `.py`|
| 启动入口 + 托盘 + 窗口 | Python(pywebview + pystray + pyobjc + ctypes) | 674 行(`main.py`) |
| 前端 UI | HTML + Bootstrap 5 + 原生 JS(无框架) | 1 html / 1 css / 4 js |
| 打包/构建 | PyInstaller `build.spec` + `.bat` + `.ps1` + `.sh` + NSIS | 8 sh / 3 ps1 / 2 bat |
| 安装产物 | Windows `.exe`(~25 MB,`Codex-App-Transfer-Setup-1.0.3.exe`) | — |

**显著痛点**:
1. PyInstaller 体积大、冷启动 1-2 秒
2. pywebview 在 macOS / Windows 行为不一致(已观察到的差异在 `main.py` 中通过 `ctypes` / `pyobjc` 双分支 workaround)
3. 构建脚本散落在 4 种语言里,新成员上手成本高
4. provider 适配器边缘 case 多,Python 弱类型让 refactor 风险高

---

## 3. 目标架构

```text
codex-app-transfer/
├── Cargo.toml                  # workspace 根
├── src-tauri/                  # Tauri 壳:窗口 + 托盘 + 系统集成 + 自动更新
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   └── src/
│       ├── main.rs             # 替代 main.py 启动入口
│       ├── tray.rs             # 替代 pystray
│       ├── window.rs           # 替代 pywebview
│       └── commands.rs         # IPC 命令(替代 FastAPI 内部 /admin/* 路由)
├── crates/
│   ├── proxy/                  # 替代 backend/proxy.py + main.py 路由部分
│   │   └── src/lib.rs          # axum + reqwest + tokio,流式 SSE/WebSocket 转发
│   ├── adapters/               # 替代 backend/*_adapter.py + provider_workarounds.py
│   │   └── src/
│   │       ├── openai.rs
│   │       ├── chat_responses.rs
│   │       ├── responses.rs
│   │       ├── streaming.rs
│   │       ├── base.rs
│   │       └── workarounds.rs
│   ├── registry/               # 替代 backend/registry.py + config.py + model_alias.py
│   ├── session_cache/          # 替代 backend/session_cache.py + response_id_codec.py
│   └── update/                 # 替代 backend/update.py(改用 tauri-plugin-updater)
├── ui/                         # Leptos 前端(纯 Rust → WASM)
│   ├── Cargo.toml
│   ├── Trunk.toml
│   ├── index.html
│   ├── style/                  # 复用现有 frontend/css/style.css → tailwindcss(渐进迁移)
│   └── src/
│       ├── main.rs
│       ├── routes/             # dashboard / providers / proxy / settings / guide
│       └── components/
└── xtask/                      # 替代 build.bat / build.spec / installer.nsi / build.sh
    └── src/main.rs             # 用 Rust 写构建/打包/签名编排
```

**最终语言占比**:Rust 95%+ / TOML / 极少量 CSS / 1 个 `index.html`;一条 `cargo` 命令搞定开发与打包。

---

## 4. 七阶段路线图

| 阶段 | 工作 | 预计工作量 | 风险点 | 验收标准 |
|---|---|---|---|---|
| **0. 脚手架** | 初始化 Tauri 2 项目 + Leptos 前端;Rust workspace + CI | 0.5 周 | — | `cargo tauri dev` 出空白窗口;`cargo check --workspace` 通过 |
| **1. Registry/Config**(数据层) | 移植 `config.py`(全部)+ `model_alias.py`(全部)到 `crates/registry`;读写**同一份** JSON 配置(向后兼容,Rust 版能读旧 Python 版配置)。**`registry.py` 中 OS 集成层(Windows 注册表 / macOS plist / Codex TOML 重写,~1100 行)挪到独立 `crates/codex_integration`,作为 Stage 2.5 处理(在 proxy 主干完成、UI 还未替换的窗口期补**) | 0.5 周(数据层)+ 0.5 周(后置 OS 层) | 配置文件 schema 漂移;字段默认值不一致;`indent=2` + 非 ASCII 不转义 + 末尾换行三处细节差异都会破坏字节级 diff | 用 Python 版生成的配置文件,Rust 版读出后再写回,字节级 diff 为空(或仅声明过的字段排序差异) |
| **2. 代理转发主干** | `proxy.py` → `crates/proxy`;axum + reqwest + `tokio_stream` 实现 SSE / WebSocket / 普通 HTTP 透传 | 1 周 | 流式细节(SSE 分包边界、心跳、`[DONE]` 帧、连接半关闭) | 录制回放 fixture 全部通过;Python 与 Rust 对同一上游响应输出字节流 diff 为空 |
| **3. 适配器** | 5 家厂商(OpenAI / DeepSeek / Kimi / MiMo / 第 5 家)的请求/响应转换全部翻译 | **2-3 周(工作量最大)** | `provider_workarounds.py` 933 行 + `responses_adapter.py` 972 行,边缘 case 多;tool_call / chat→responses 变换易踩坑 | 录制的所有 provider × scenario fixture 全绿;手动端到端跑通主要模型 |
| **4. UI 移植** | Leptos 重写 5 个路由(dashboard / providers / proxy / settings / guide);i18n 用 `leptos-i18n`;视觉先 1:1 复刻 Bootstrap | 1-1.5 周 | 表单状态管理;反馈面板交互 | 视觉走查所有页面;中英切换、深浅色切换正常 |
| **5. 托盘 + 单实例** | `tauri-plugin-single-instance` + Tauri tray API 替代 pystray + ctypes;macOS Dock/menubar 行为对齐 | 0.5 周 | 双平台行为差异(尤其 Windows 的 windowed exe 无控制台分支) | mac/win 双平台手测:启动、托盘点击、显示/隐藏、退出、单实例约束 |
| **6. 打包** | `cargo tauri build` 生成 `.dmg`/`.exe`/`.msi`;签名走现有 `.release-signing/` 流程;**前置:多平台 icon pipeline + 完整 Xcode(详见修订日志 2026-05-03)** | 0.5 周 | macOS notarize、Windows codesign 凭据接入;icon RGBA + 多尺寸 | 双平台干净机器装机测试通过;`trunk build --release` 后 WASM ≤ 500 KB(校准方案 §1 估算) |
| **7. 切换 + 下线 Python** | 发 v2.0.0;保留 v1.x 分支做 hotfix 通道;过渡期(≥1 个版本周期)后归档 `backend/` `main.py` | 0.5 周 | — | v2.0.0 release notes 完成;v1.x 明确 EOL 日期 |

**总计 ~6-7 周一人工**(不含调试与发布周期 buffer)。

---

## 5. 关键技术选型

| 领域 | 选择 | 理由 |
|---|---|---|
| 异步运行时 | `tokio` | 唯一现实选择,生态最完整 |
| HTTP 服务端 | `axum` | 与 `tokio` / `hyper` 心智模型一致;Tower middleware 生态 |
| HTTP 客户端 | `reqwest` | 与 `hyper` 共享底层,流式 API 自然 |
| JSON | `serde` + `serde_json` | `#[derive(Serialize, Deserialize)]` 直接替代手写转换,工作量大降 |
| WebSocket | `tokio-tungstenite` | 替代 Python `websockets` |
| SSE | `axum::response::sse` + `eventsource-stream` | 流式响应一等公民 |
| 配置文件 | 沿用现有 JSON,用 `serde` 直接映射 | 向后兼容旧 Python 版本 |
| 前端框架 | `leptos`(0.7+) | RSX 语法 + reactive primitive,Rust 全栈最成熟选 |
| 前端构建 | `trunk`(WASM 打包,**无需 Node**) | 与现仓 Node 损坏状态解耦 |
| 样式 | 阶段 4 起用 `tailwindcss`(走 Tauri 内嵌 CDN 或本地编译) | 渐进迁移,初期可继续套 Bootstrap CSS 类 |
| 国际化 | `leptos-i18n` | 复用现有 zh/en 词条 JSON |
| 自动更新 | `tauri-plugin-updater` | 替代 `backend/update.py`,签名机制走 Tauri 标准流程 |
| 安装包 | Tauri 内置 NSIS / WiX(MSI)/ DMG | **直接干掉** `installer.nsi` + `build.spec` + `build.bat` |
| 单实例 | `tauri-plugin-single-instance` | 替代 `main.py` 的 ctypes 实现 |
| 托盘 | Tauri tray API | 替代 `pystray` |
| 系统对话框 | `tauri-plugin-dialog` | 替代 `ctypes.windll.user32.MessageBoxW` |

---

## 6. 风险与对冲

### 风险 R1:适配器翻译是真正的工作量,而非"换皮"

12k+ 行 Python 业务逻辑改 Rust,SSE/streaming 边缘 case 极易踩坑。

**对冲**:**Stage 0 与 Stage 1 之间插入"录制-回放"测试基建**(本方案前置项,见 `tests/replay/`)。把现有 Python 版面对真实上游的输入/输出固化为 JSON fixture,Rust 版照着 fixtures 做 TDD;Stage 2 / Stage 3 每个 PR 必须维持 fixture 全绿。

### 风险 R2:Leptos 学习曲线 + 生态不足

reactive primitive 与 React/Vue 不同;复杂组件库少。

**对冲**:UI 不复杂,初期不引外部组件库,直接 RSX + Bootstrap CSS 类。如 Stage 4 推进受阻,**退到 SolidJS** 兜底方案(只损失语言统一度,不损失整体进度)。

### 风险 R3:Stage 7 切换时新版有未发现 regression

**对冲**:
1. v1.x Python 版保留至少 1 个版本周期作为 hotfix 通道
2. v2.0.0 首版默认开启"对照模式":同时启动 Python 旧代理(隐藏端口)与 Rust 新代理(主端口),后台异步对照 1% 流量,差异写本地日志(可选,影响发布节奏可砍)

### 风险 R4:macOS 签名 / 公证流程接不上

现有 `.release-signing/` 是为 PyInstaller 构建设计的。

**对冲**:Stage 6 单独留 0.5 周;若卡住,先发 dev-signed 内测版,正式签名作为 follow-up。

### 风险 R5:不要试图一次性切换

**对冲**:严格按阶段 0→7 推进;每阶段结束 Python 版仍可用,Rust 版与 Python 版双跑对拍。

---

## 7. 配套测试基建(前置)

在启动 Stage 0 的同时,先在 Python 仓库内建立 `tests/replay/`:

- **fixture 格式**(JSON,语言无关,详见 `tests/replay/fixtures/_schema.md`):记录 `client_request` / `upstream_request` / `upstream_response` / `client_response` 四元组,流式响应记录每帧的 `delay_ms` + `data`
- **recorder**:`tests/replay/recorder.py`,通过 httpx `Transport` 钩子,把真实上游响应(含 SSE 流帧)固化为 fixture
- **player**:`tests/replay/player.py`,加载 fixture 作为 mock 上游,驱动 FastAPI 应用并捕获 client-facing 输出
- **断言**:`tests/test_replay_*.py`,逐 fixture 跑 player → diff `client_response`

**契约**:fixture 是 Python 与 Rust 两个实现之间的契约。任何 Rust 实现必须在同一 fixture 上输出字节级一致(允许的字段排序差异需在 schema 中显式声明)。

---

## 8. 修订日志

记录方案落地过程中的偏差与原因。每条遵循格式:`日期 | 来源 | 偏差 | 原因`。

| 日期 | 来源 | 偏差 | 原因 |
|---|---|---|---|
| 2026-05-03 | 初稿 | — | 用户确认整体方向 |
| 2026-05-03 | Stage 0 落地 | **本机 Node 25 dyld 损坏(`libllhttp.9.3.dylib` 缺失)**;原方案中"前端构建走 trunk(无需 Node)"被实战验证为正确选择,无须任何调整。明确写入"前端工具链不依赖 Node"作为硬约束 | 避免后续阶段因 Node 不可用回退到 Vite/SolidJS |
| 2026-05-03 | Stage 0 落地 | **图标必须 RGBA**:`frontend/assets/app-icon.png` 是 RGB,Tauri 2 `generate_context!` 宏在编译期 panic("icon ... is not RGBA")。Stage 6 前需建立"icon 预处理"步骤(已在 `src-tauri/icons/` 用 PIL 现场转换通过) | tauri-build 2.6 强制要求 RGBA;**Stage 6 打包章节需新增子任务**:把 `macos/prepare-icon.py` 扩展为多平台 icon pipeline,产出 32×32 / 128×128 / 128×128@2x 全套 |
| 2026-05-03 | Stage 0 落地 | **`ui/Cargo.toml` 不能同时声明 `[lib]` + `[[bin]]`,只用 `[[bin]]`**(初版尝试 `crate-type = ["cdylib","rlib"]` 但没建 `src/lib.rs`,trunk 直接报 `cargo metadata` 错) | trunk CSR 模式下,默认用 bin 的 `main()` 作为 wasm 入口;若以后要拆 lib,要同步建 `src/lib.rs` |
| 2026-05-03 | Stage 0 落地 | **本机未装完整 Xcode**(只有 CLT)。`cargo check` / `cargo build` / `cargo tauri dev` 不受影响,但 `cargo tauri build` 生成 `.dmg` 需要 `xcodebuild` | Stage 6 前置任务:在签名机器上确认 Xcode + 公证证书。dev 阶段不阻塞 |
| 2026-05-03 | Stage 0 落地 | **WASM debug 产物 791 KB**(`codex-app-transfer-ui_bg.wasm`),与方案 §1 估算 "300-800 KB gzip" 中的上界一致但需注意是 **debug 版**;Stage 4 末/Stage 6 必须用 `trunk build --release` + `wasm-opt -Oz` 重新测体积 | 早期防止"看起来很大"误判;数字需在 Stage 6 验收时校准 |
| 2026-05-03 | 用户反馈 | **Node 25 / Xcode 已用户重装**:Node v25.9.0 验证恢复;Xcode 安装但 `xcode-select` 仍指 CLT,需用户手动 `sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer` 切换才能解锁 `xcodebuild`(Stage 6 出 `.dmg` 必需)。**dev 阶段不阻塞** | 修订日志 1/4 中"Node 损坏"的硬约束改为"软约束(便于解耦)";Xcode 打包前置任务保留 |
| 2026-05-03 | `cargo tauri dev` 实测失败 | **`beforeDevCommand` 用了 `cd ../ui`**,假设 CWD 是 `src-tauri/`;但 tauri-cli 在用户 CWD(repo 根)下执行,`../ui` 解析为仓库父目录失败。已改为 `trunk serve --config ui/Trunk.toml --port 1420`,从 repo 根目录调用即可工作(同步改 `beforeBuildCommand`) | trunk `--config` 让路径解析与 CWD 解耦,比依赖 `cd ../ui` 更稳;**README 后续需写明"从 repo 根跑 `cargo tauri dev`"** |
| 2026-05-03 | trunk warning | `Trunk.toml` 中 `[serve] address = "127.0.0.1"` 已废弃 → 改为 `addresses = ["127.0.0.1"]` | trunk 未来版本会移除单数字段 |
| 2026-05-03 | 推进选项 C 落地 | **新增 `.github/workflows/ci.yml`**:三 job 并行 —— Python replay smoke / Rust workspace check + fmt + test / Leptos UI trunk release build + WASM 体积预算(500 KB 警告,Stage 6 硬限);Linux runner 安装 webkit2gtk-4.1 等 Tauri 系统依赖 | 把 Stage 0 的双绿固化为回归门禁,Stage 1-7 每次 PR 自动校验;**Stage 6 验收"WASM ≤ 500 KB"**已在 CI 体现 |
| 2026-05-03 | release 体积实测 | **release WASM 仅 93 KB**(`trunk build --release` + 自动下载的 `wasm-opt v123`);方案 §1 估算 300-800 KB 大幅偏保守 → 把估算更新为 ~100 KB,CI 软预算 500 KB 仍保留作早期警报上限 | Stage 4 加 i18n + 路由 + 实际业务后体积会增长;预算先松后紧 |
| 2026-05-03 | Stage 1 启动 | **拆分原 Stage 1**:数据层(`config.py` + `model_alias.py`,可独立,~0.5 周)与 OS 集成层(`registry.py` 中 1100 行 Codex CLI 注入 / 注册表 / plist,与代理生命周期耦合)分开。OS 层挪到 **Stage 2.5** | `registry.py` 真正的工作量在 OS 集成而非"读写 JSON";原方案估的 0.5 周如果硬塞进 Stage 1 会跑挂。拆开后两段都能保 0.5 周 |
| 2026-05-03 | Stage 1 数据层落地 | `crates/registry` 完成:`schema.rs`(Config / Settings / Provider 类型化 view + flatten 透传 extra)/ `raw_io.rs`(serde_json `preserve_order` 走字节级 round-trip,主配置无尾换行 / Library 条目带尾换行)/ `model_alias.rs`(MODEL_SLOTS 与 Python 1:1)/ `presets.rs`(7 条内置预设以 JSON 字面量内嵌)/ `paths.rs`. **测试 17 项全绿**:10 单测 + 4 byte-identical round-trip + 1 Rust embedded vs Python dump 等价 + 2 typed parse | Stage 1 验收"Rust 版读出后再写回,字节级 diff 为空"达成 |
| 2026-05-03 | CI 增加 fixture 漂移检测 | `rust-workspace-check` job 新增一步:CI 中重跑 `python scripts/gen_registry_fixtures.py`,然后 `git diff --exit-code` 校验产物没变。如果有人改了 `backend/config.py` 但没刷新 fixtures,CI 立刻红 | 防止"Python 改了/Rust 没跟"或反之的双源真相不一致 |
| 2026-05-04 | Stage 2 骨架落地 | `crates/proxy` 完成 axum 0.8 + reqwest 0.12 + rustls 的纯透传转发;`forward.rs` 实现 hop-by-hop 头剔除 + body 收齐 + URL 重拼 + 响应字节流灌回(`Body::from_stream`);`fixture.rs` 复刻 `tests/replay/fixture.py` 的 schema(Python ↔ Rust 通用)。集成测试 `streaming_passthrough.rs` 在两个临时端口上跑了真 TCP 链路:reqwest → 代理 → mock 上游 → 回灌,**字节级断言上游 SSE 帧拼接 == 代理输出**(0 转换 / 0 丢失) | Stage 3 adapter 的"上游字节流不动" baseline 已建立;后续插入的转换层每一处差异都能精确归因 |
| 2026-05-04 | reqwest 特性精简 | reqwest 取消 default-features,只开 `stream` + `rustls-tls` + `http2`;dev-test 用 `serde_json::to_string` 手写 body 而非 `.json()`。**减少二进制依赖**:Linux 编译不再需要系统 OpenSSL | Stage 6 容器化 / 跨平台分发时降低系统库摩擦面 |
| 2026-05-04 | Stage 2 完整版(B1+B2)落地 | `crates/proxy` 新增 `resolver.rs`(`ProviderResolver` trait + `StaticResolver` 把 registry::Provider 列表 + gateway_key + default_id 映射成 `ResolvedProvider`)。`forward_handler` 接入:gateway 鉴权 → 路由(`<slug>/<model>` 形式)→ 剥离 incoming Authorization → 注入上游凭据(Bearer / X-Api-Key)→ 注入 `provider.extra_headers` → 必要时改写 body model 字段。**新增 6 个端到端集成测试**(`auth_and_routing.rs`):401 缺/错 gateway key、Bearer 路径、X-Api-Key 路径、extra-headers 注入、fallback 默认 provider、path+query 透传 | Stage 3 adapter 接入的硬前置已全部就位:adapter 拿到的"已选 provider + 已写好凭据 + 已剥 slug"是稳定不变量 |
| 2026-05-04 | resolver provider_slug 简化 | Python 版 `provider_slug` 会把 id/name 小写并替换非 [a-z0-9_-];Rust 版 Stage 2 暂只支持 `slug == provider.id` 这一最常见情况 | Stage 3 接入 adapter 时再补完整 slug 算法;期间不影响内置 7 家 provider(其 id 已是合法 slug) |
| 2026-05-04 | 录到第一份真实 SSE fixture | `tests/replay/fixtures/kimi_chat_minimal_streaming.json` —— Kimi (Moonshot) 4 帧:role chunk / `delta.reasoning_content` / 末帧 `usage` / `[DONE]`。recorder 的 `SENSITIVE_HEADERS` 同步加 OpenAI / Moonshot 的 org / project / uid / request-id / trace-id 等账号标识(非凭据但公网仓库泄漏会暴露用户)。新加 Rust 集成测试 `sse_passthrough_real_kimi`,代理对真实 SSE 字节级 0 损耗 | Stage 3 adapter 真正面对的是 `reasoning_content` 这种厂商扩展字段,合成 fixture 没法覆盖;同时验证了 fixture-server 在真实 chunked / Connection 头存在时的兼容性(已修:mock 也剔除 hop-by-hop 头) |
| 2026-05-04 | fixture mock hop-header 修正 | `crates/proxy/src/fixture.rs::build_upstream_mock` 之前把 fixture 里的 `transfer-encoding: chunked` / `connection: keep-alive` 直接照搬到 axum Response,与 axum 自动算的 content-length 冲突,导致 reqwest 客户端报 502 | 找到方法:mock 与代理一样过滤 hop-by-hop 头,让 axum/hyper 重新决定分包 |
| 2026-05-04 | Stage 3.1 落地 | 新建 `crates/adapters`:`Adapter` trait(`prepare_request` + `transform_response_stream`,带默认透传实现)+ `AdapterRegistry`(按 `apiFormat` 字符串归一查找)+ `OpenAiChatAdapter`(覆盖现仓库 7 内置预设 / 5 用户 provider 的 100%)。proxy 的 `forward_handler` 接入:resolved → adapter.prepare_request → 出站 URL = base + upstream_path → reqwest → adapter.transform_response_stream → axum Body。**全量 50 项测试全绿**(含 8 项新 adapters 单测) | Stage 3 trait 接口已对外稳定;Stage 3.2 起的 ResponsesAdapter 只需新增实现并在 registry 注册,**无需再动 forward_handler** |
| 2026-05-04 | 路径双 `/v1` bug 修复 | 早期代理直接把入站 `/v1/chat/completions` 拼到 `baseUrl=https://api.moonshot.cn/v1` 后会得到 `/v1/v1/chat/completions`,真请求会 404。骨架阶段 mock 不校验路径所以测试仍绿。Stage 3.1 由 `OpenAiChatAdapter` 剥前导 `/v1`,base_url 自带的 `/v1` 与 adapter 输出的 `/foo` 合起来恰好一份。新加 2 个端到端断言:入站 `/v1/foo?q` → 上游收到 `/foo?q`;入站 `/foo`(无前缀)→ 上游照搬 `/foo` | 此 bug 在 Stage 4 接通真实 Codex CLI 流量前会爆;现在被锁死,后续不会复发 |
| 2026-05-04 | 澄清 Anthropic Messages 协议状态 | grep 全仓后确认:**Python 版没有 Anthropic Messages → Responses 的协议转换**。`/v1/messages` / `/claude/v1/messages` 路由只是 `handle_responses` 的别名(handler 把 body 当 Responses 形态读);`api_adapters.py::normalize_api_format` 把历史配置值 `"anthropic"/"claude"/"messages"` 归一为 `"responses"`(配置兼容,非协议转换);`provider_workarounds.py::anthropic_*` 是给"上游 provider 说 Anthropic Messages"准备的方向相反 workaround,7 内置预设 + 5 用户配置都不命中 | 我之前在路线图把 Stage 3.2a 描述成"Anthropic → Responses"是用词错;正确说法:**OpenAI Responses ↔ OpenAI Chat 互转**。整套 Codex App Transfer 全程不解析 Anthropic 协议 |
| 2026-05-04 | Stage 3.2c 落地 | 新建 `crates/adapters/src/responses/`(`converter.rs` 同步状态机 + `stream.rs` 异步 wrapper + `mod.rs` `ResponsesAdapter`)。状态机:Idle→Streaming→Done,emit `response.created` / `output_item.added` / `content_part.added` / `output_text.delta*` / `output_text.done` / `content_part.done` / `output_item.done` / `response.completed`;`finish_reason` 映射到 OpenAI Responses 标准 `incomplete_details.reason`(stop→completed,length→max_output_tokens,content_filter→content_filter,流被中断→interrupted)。AdapterRegistry 把 `responses` / `openai_responses` / `anthropic` / `claude` / `messages` 全归到 ResponsesAdapter。**新增 23 项测试**(8 状态机单测 + 4 路径/注册 + 3 集成测试 + 真 Kimi fixture 跑通 4 帧含 reasoning_content + Kimi-non-standard usage 位置)| Stage 3.3 工作量定位:tool_calls / reasoning_content / function_call —— 都在状态机 `handle_frame` 里增量加 emit,**不动 trait 接口、不动 stream wrapper**。|
| 2026-05-04 | 发现 Kimi usage 非标准位置 | OpenAI 标准把 `usage` 放 chunk 顶层;Kimi(Moonshot)在末帧把 `usage` 塞进 `choices[0].usage`。状态机两个位置都收(顶层优先) | 此差异未来若有更多 provider 出现还会有,现在的解析路径已留兜底 |
| 2026-05-04 | Stage 3.3a 落地(reasoning 通路) | 状态机重构:emit_open 拆为"`response.created` 立即 + reasoning/message 各自懒开"。新增字段 `reasoning_id` / `reasoning_open` / `reasoning_acc` / `next_output_index`,reasoning 与 message 在同一响应里并存,output_index 按"实际出现顺序"动态分配。新增事件:`response.reasoning_summary_part.added` / `reasoning_summary_text.delta` / `reasoning_summary_text.done` / `reasoning_summary_part.done` + `output_item.added`(item.type=reasoning) + `output_item.done`(reasoning)。**测试 4 项新单测**:reasoning-only / reasoning→content 切换 / reasoning 跨多帧累计 / 首帧空 content 仅 emit response.created。Kimi fixture 集成测试期望从"message-only"改为"reasoning-only(只 reasoning='The' 没 content)" | Stage 3.3b/c 加 tool_calls / function_call 时,只需在 `handle_frame` 里再加一段"如果 delta.tool_calls 非空 → 走 tool 懒开闭",**output_index 动态分配机制已就位**,无需再改架构 |
| 2026-05-04 | Stage 3.3b 落地(tool_calls 通路) | 状态机加 `BTreeMap<u32, PendingToolCall>`(key=OpenAI 自带的 tool_call index),首次见到某 index → 分配 `output_index` + emit `output_item.added`(item.type=function_call,status=in_progress);后续帧累计 `function.arguments` 字符串 → emit `function_call_arguments.delta`;close 阶段按 OpenAI index 升序闭合。call_id 透传上游 `tool_calls[i].id`,缺失时按 `call_<seed>_<idx>` 兜底。**新增 5 项 tool_call 单测**:single 全生命周期 / 多 tool_call 各占独立 output_index / args 跨多 chunk 累计 / call_id 兜底 / message+tool_call 混合按出现顺序 | Stage 3.3 主体完成。3.3c legacy `function_call` 现在所有主流 provider 已迁走,触发概率极低,可推迟到 v2.0.0 发布前再补 |
| 2026-05-04 | converter 内部排序契约 | output[] 严格按 output_index 升序排列(reasoning/message/tool_calls 全部混在一起按数字排序)。BTreeMap 让 tool_calls 迭代天然有序;reasoning/message 用各自的 _index 字段记录;close 阶段拼 output[] 时统一 sort_by_key | 行为可预测;Stage 4 UI 渲染时不需要单独排 |
| 2026-05-04 | Stage 3.2a stateless 落地(请求侧) | 新建 `crates/adapters/src/responses/request.rs::responses_body_to_chat_body`,接到 `ResponsesAdapter::prepare_request`。覆盖:`model`/`instructions`(→ system 头)/ `input`(string + array,3 种 item 类型 message/function_call/function_call_output)/ `tools`(function 透传 + custom 降级)/ `tool_choice` 透传 / `max_output_tokens`→`max_tokens` / 11 个标准透传字段 / `stream`+`stream_options.include_usage`。**新增 19 项单测**(string-input / instructions / message-array / 文本块拼接 / 多模态降级 / function_call / function_call_output(含 JSON 序列化兜底)/ tools 5 形态 / max_output_tokens 重命名 / stream 选项 / 全字段透传 / previous_response_id 静默丢弃 / 完整 Codex CLI 工具循环模式 / 非对象 body 报 BadRequest)。**stateless**:不接 session_cache;`previous_response_id` 静默丢弃;input_image/audio/file/reasoning 顶层 / text.format / store/metadata 等留 3.2a'/3.3c | 端到端协议层闭环就位:Codex CLI(Responses) → 代理 → ResponsesAdapter 双向(请求 + 流式响应)→ 上游(Chat)。剩下"接通真实 Codex CLI"只差 Stage 2.5 OS 集成把 Codex CLI 指向代理端口 |
| 2026-05-04 | Stage 2.5 落地(Codex CLI 集成) | 新建 `crates/codex_integration`:`paths`(`~/.codex/{config.toml,auth.json}` + `~/.codex-app-transfer/codex-snapshot/`)/ `toml_sync`(line-based 根级别同步,1:1 对齐 Python `_sync_codex_toml_value`,**保留用户注释/sections/前缀同名 key**)/ `auth`(JSON R/W,Unix 0600)/ `snapshot`(整文件备份 + manifest,**幂等**:同会话多次 apply 不污染原始备份)/ `apply`:写 `openai_base_url` + 可选 `model_context_window=1000000` + auth.json 的 `auth_mode/OPENAI_API_KEY`,**保留 OAuth tokens 等用户字段** / `restore_codex_state`:基于快照精确还原 managed key,有快照走 key-merge 还原、无快照退化为"删除我们的 key"。**测试 30 项**:toml_sync 8(空文件/替换/删除/section 前插/前缀同名不误改/注释保留/整数/字符串转义)+ auth 4(空文件/round-trip/0600 权限)+ snapshot 5(无文件/复制/幂等/drop/字面量解析)+ paths 1 + apply 8(空 base_url 删除/空 key 删除/1M 注入/用户字段保留/restore 含快照/restore 无快照退化/二次 apply 不覆盖原始/前缀同名不误改/OAuth tokens 全程不动) | **不**接 ChatGPT 桌面客户端的 plist + Windows 注册表(那是另一条线,本仓库主线是 Codex CLI,留 Stage 2.5b 处理) |
| 2026-05-04 | Stage 2.5 范围切割 | Python `backend/registry.py` 中 1100 行 OS 集成,实际是两条独立子系统:**Codex CLI**(`~/.codex/*`,跨平台,本次完成)和 **ChatGPT desktop app**(macOS plist/JSON、Windows 注册表,~800 行)。我把第二条挪到 Stage 2.5b 单独追,主线只走 Codex CLI 路径 | Codex App Transfer 的字面意思就是 Codex CLI 集成,ChatGPT desktop 集成是历史遗留的辅助功能;v2.0.0 即使只发 Codex CLI 路径也是完整产品 |
| 2026-05-04 | Stage 4.1 落地(UI 路由 + IPC) | ui crate 加 leptos_router 0.7(去掉 nightly feature)+ serde-wasm-bindgen + js-sys。建 `ui/src/{components,routes,ipc}/` 模块:5 路由(dashboard/providers/proxy/settings/guide,后 4 为占位)+ AppHeader 组件 + Tauri IPC `invoke<T>()` / `invoke_with<P,T>()` 包装(`wasm-bindgen(catch)` 路径走 `window.__TAURI_INTERNALS__.invoke`)。src-tauri 依赖加 `registry` + `codex_integration`,注册 4 个只读命令:`app_info` / `list_providers` / `active_provider_id` / `codex_status`(后者读 `~/.codex/config.toml` 解出 `openai_base_url`)。Dashboard 调通展示真实数据。新增 `ui/style/app.css`(3.1 KB,响应深色模式) | 协议层 + 文件层 + UI 接通点全打通,Stage 4.2 起就是纯页面工作量 |
| 2026-05-04 | UI WASM 体积变化 | release WASM 从 93 KB(Stage 0 helloworld)→ **430 KB**(Stage 4.1,加 leptos_router + 4 个 IPC 命令的反序列化 + Dashboard signals)。仍**在 CI 500 KB 软预算内**;Stage 4 末加完所有路由后预计 ~600 KB,届时把 CI 软预算上调到 800 KB(release+wasm-opt 后实际不会更高),Stage 6 验收硬限再决定是 1 MB 还是更紧 | Leptos 0.7 主体(reactive_graph + tachys + leptos_dom)是固定开销,后续路由再加 4 个 + adapter 反序列化结构带来的增长 < 200 KB |
| 2026-05-04 | Stage 4.2 落地(Providers 实页面) | src-tauri 新增 mutation commands:`set_active_provider` / `delete_provider` / `add_provider_from_preset` / `list_presets`;每次调用走 load → mutate → `save_raw_config` 完整链路,**与 Python 版字节级兼容**(`serde_json/preserve_order`)。Providers 页:列表(主信息/元信息/操作三列)+ 激活按钮(active 行变 "✓ 当前激活" 徽章)+ 删除按钮(`window.confirm` 二次确认)+ 从预设添加表单(7 预设下拉 + API key 输入 + 可选显示名)。状态消息条用绿色/红色区分。新增 4.2 专属 CSS:btn-primary/secondary/danger / form fields / status banners | Stage 4.2b 编辑表单(自定义 baseUrl + 修改 apiKey)与 Stage 4.2c 测速可推迟;现有功能足以覆盖"切换 + 删除 + 加新 provider"主路径 |
| 2026-05-04 | UI Leptos 闭包 prop 写法 | 学到的:Leptos 0.7 `view!` 宏在组件 prop 位置不能直接写 `move \|x\| {...}`(被解析为方法调用导致 `r#move` 找不到),要么先 `let cb = move \|x\| {...}` 绑定再传入,要么用 `Callback::new(move \|x\| {...})`。后者更地道,跨 reactive context 也安全 | 后续组件间回调统一用 `Callback<T>` |
| 2026-05-04 | WASM release 实测 530 KB | Stage 4.2 后 release WASM 涨到 **533 KB**,略超原 500 KB CI 软预算。**计划在 Stage 4.5 末把软预算放宽到 800 KB**;Stage 6 验收硬限暂保留 1 MB | Leptos 表单组件 + serde-wasm-bindgen 的反序列化代码膨胀比较明显,正常范围内 |
| 2026-05-04 | Stage 4.3 落地(Proxy 启停) | 新建 `src-tauri/src/proxy_runner.rs::ProxyManager`(`Mutex<Option<ProxyHandle>>` 单实例 + `oneshot` graceful shutdown);src-tauri 加 deps:`codex-app-transfer-proxy` + `tokio[sync,net,rt-multi-thread]` + `axum`。新增 3 commands:`start_proxy(port)` / `stop_proxy()` / `proxy_status()`。**`start` 时重新读 `~/.codex-app-transfer/config.json`** 反序列化为 `Config` 强类型构造 `StaticResolver`(provider/active/gateway_key 全装载)。Proxy 实页:status 卡片(运行/停止 badge + addr + gateway_auth + provider_count + active)+ 控制卡片(端口输入 + 启动/停止按钮 + 操作反馈),按钮根据 `running` 状态自动 disable 防误操作。CSS 加 badge-running/stopped + proxy-controls 布局 | **首次端到端可用形态成立**:UI 启动后,Codex CLI 把 `OPENAI_BASE_URL` 指到 `127.0.0.1:18080` 即可走 Rust 代理打通到上游 |
| 2026-05-04 | Tauri State 注入 | `tauri::Builder::default().manage(ProxyManager::new())`;async commands 用 `State<'_, ProxyManager>` 接入,内部 `Mutex` 异步锁。**Tauri 2 默认 tokio 运行时**与 `tokio::spawn` / `tokio::sync::oneshot` 兼容,无需 `tauri::async_runtime` wrapper | 代理服务器作为后台 task 与主进程同生命周期(进程退出即 drop)。Stage 5 加 graceful shutdown on app exit |
| 2026-05-04 | Stage 4.4 落地(Settings) | src-tauri 加 deps:`base64` + `getrandom`。新增 5 commands:`get_settings` / `update_proxy_port` / `regenerate_gateway_key` / `apply_codex_config` / `restore_codex_config`。`apply_codex_config` 端到端串起所有上下文:读 active provider → 推断 supports_1m(deepseek-v4-* / qwen3.6-* / `[1m]` 标记 / modelCapabilities.supports1m)→ 必要时**自动生成 gateway key**(`cas_<base64url(32 字节熵)>` 等价 Python `secrets.token_urlsafe(32)`)→ 调 `codex_integration::apply_provider`。Settings 实页:Codex 集成卡(状态 + 应用/还原)+ Gateway 卡(脱敏 key + 重新生成)+ 端口卡 | **v2.0.0 alpha 闭环成立**:用户 UI 上点几下即可让 Codex CLI 走 Rust 代理 → 上游真实模型,无需手动 export 环境变量 |
| 2026-05-04 | Gateway key 脱敏策略 | `mask_gateway_key`:前 8 字符 + `…` + 后 4 字符(例:`cas_abcd…WxYz`)。空字符串显示 `(未设置)`,过短(≤12 字符)统一显示 `***` | 防止 UI 截屏 / 长时间显示导致泄漏;`cas_` 前缀保留方便辨识 |
| 2026-05-04 | Stage 4 主线 UI 工作量 | 4.1 + 4.2 + 4.3 + 4.4 全部 ~3 天,与原方案估值(1-1.5 周)对齐;剩 4.5(Guide + i18n)非阻塞,可挤到 v2.1.0 | UI 主轴的核心交互全部就绪;v2.0.0 发布前剩 Stage 5 托盘 + Stage 6 打包 |
| 2026-05-04 | Stage 5 落地(托盘 + 单实例 + 后台常驻) | src-tauri 加 deps:tauri 启用 `tray-icon` feature + `tauri-plugin-single-instance`;`tauri.conf.json` 主窗口加 `label: "main"`。`ProxyManager` 内部 Mutex 改 `std::sync::Mutex`(锁不跨 await),`stop` / `stop_silent` / `status` 全部变同步,方便从 `RunEvent::Exit` 同步路径调。`main.rs` 重写:`.setup()` 建托盘菜单(显示/隐藏窗口 + 启停代理 + 退出);关窗事件 `prevent_close()` + `window.hide()`;退出走 `RunEvent::Exit` → `manager.stop_silent()` 优雅关。单实例插件:第二次启动把已有窗口拉前(`show + unminimize + set_focus`) | v2.0.0 行为成立:**关窗 = 后台,代理仍跑**;**Cmd+Q / 托盘退出 = 优雅停代理**;**双击图标不会双开**。这是日常长驻应用的最低形态 |
| 2026-05-04 | Mutex 选型理由 | ProxyManager 之前用 `tokio::sync::Mutex`(异步)。Stage 5 改 `std::sync::Mutex`(同步)是因为 `RunEvent::Exit` 是同步事件回调,异步锁需要 `block_on` 容易在主线程上死锁。改后 stop/stop_silent/status 全同步,锁持有时间极短(O(1) 操作),不需要异步抢占;**`start` 仍是 async**(TcpListener::bind 必需),但锁取放都在显式 scope,无跨 await | 简化 1 处异步路径;同步锁在这种"轻量 mutate Option<T>"场景下表现更好 |
| 2026-05-04 | Stage 4.5 落地(i18n + Guide) | **不引 leptos-i18n 大依赖**(避免 200+KB WASM 膨胀),自建轻量 `ui/src/i18n.rs`:`Lang` / `Theme` 各自 `RwSignal`,挂 root context;`t(key) -> String` 同步函数,字典就在 `dict_zh` / `dict_en` 静态 match 里,未命中返回 key 本身便于发现漏翻;`localStorage` 持久化(`cas.lang` / `cas.theme`)。Header 激活语言切换(中/EN)+ 主题循环(◐ 自动 / ☀ 浅色 / ☾ 深色,显式优先 prefers-color-scheme)。Guide 页:4 步流程(添加 provider → 启代理 → apply Codex → 测试)+ 3 条提示(数据兼容 / 后台常驻 / 本地优先)。5 个 page-title 接 t() | i18n 系统总开销:lib.rs +200 行 + 字典约 80 个 key,WASM 涨幅 ~55KB(617 → 673);可控 |
| 2026-05-04 | i18n 设计 trade-off | dict 函数返回 `Option<&'static str>`,命中时零成本 `&'static`,未命中返回 `None` → t() 拷成 String 兜底。这样 known key 走快路径,unknown key 不 leak 内存。语言切换时所有 `t()` 调用点通过 reactive signal 自动重渲染 | 替代方案(`fn dict(key) -> &'a str` lifetime polymorphic)会让 unknown key 复用 input 引用,避免拷贝但语义更复杂;选当前方案是因为字典几乎全命中,String 拷贝可忽略 |
| 2026-05-04 | 主题三态 | `Theme::Auto / Light / Dark`,`Auto` 由 `prefers-color-scheme` 走系统;`Light` / `Dark` 通过 `<html data-theme=...>` 显式覆盖 | CSS:`@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { ... } }` + `:root[data-theme="dark"] { ... }` 两条规则解决三态 |
| 2026-05-05 | 清理推进中 (cleanup-plan.md Phase 1-4) | 4 PR 序列把 v1 残留 Python / PyInstaller / Wine / NSIS / Docker 清出主线: **Phase 1** (PR #2 已合) 删 `backend/` + 老 PyInstaller spec + 老 Python 集成测试 + Setup.exe + 老 PS1 脚本 + 死 CI job;**Phase 2** (PR #3 已合, dispatch test 暴露 release pipeline 3 bug 见 PR #6 hotfix; PR #4 已合并 hotfix) release pipeline 切 GH Actions matrix + Tauri bundler,删 `macos/build-macos.sh` + `docker/{linux,windows}-builder/` + `scripts/build-{linux,windows}-on-mac.sh`;**Phase 3** (PR #4 已合) 新增 `xtask` crate 重写 `gen_registry_fixtures.py` + `release_assets.py` 为 Rust(`xtask gen-fixtures` + `xtask release-bundle`),删 `tests/replay/*.py` + `test_replay_smoke.py` + `requirements.txt` + `pyproject.toml`;**Phase 4** (PR #5 最终收尾) README/migration-plan/cleanup-plan 收尾归档。Phase 1-4 完成后 `git ls-files '*.py'` 应空,仓库主线只剩 Rust + 静态前端 + Cloudflare Worker JS + 少量 shell | v2.0.0 起的"单二进制 / 无 Python 解释器依赖"承诺已落到仓库实际状态;开发者上手将只需 `rustup install stable + cargo tauri dev` |
