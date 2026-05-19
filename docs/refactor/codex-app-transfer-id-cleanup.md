# codex-app-transfer 内部 ID 清理 (Phase 2)

> 起草: 2026-05-19
> 状态: **待启动**(Phase 1 文本/品牌已落,本文跟踪 ID 级 rename 余项)
> 适用范围: `Cmochance/cc-desktop-switch` 全仓
> 前置: 见 README "项目身份与沿革"、`docs/refactor/migration.md` §0

## 背景

v2 Rust 重写脚手架取自姊妹仓库 `Cmochance/codex-app-transfer`。Phase 1(本 commit)
把**用户可见的产品名**全部归位到 CC Desktop Switch:productName / window title /
README × 4 / Makefile 产物名 / frontend `<title>` + About 链接 / feedback-worker
邮件署名 / docs/build.md / AGENTS.md / ARCHITECTURE_PROTOCOL_GUIDE.md / CHANGELOG
顶部引导 / migration.md 第 0 节身份说明。

但**内部 ID** 全部保留 `codex-app-transfer` 字面,因为它们牵涉:用户现存配置目录、
app 升级链(Bundle Identifier / Cloudflare 资源 / 已发 release 公钥),代码级 rename
(80+ 处 `use codex_app_transfer_*`),改起来必须配套数据迁移代码 + CI/CD 改造,不
适合混进 Phase 1 文档 PR。本文逐项列出 Phase 2 要做什么。

## Phase 2 清单

### 1. Cargo workspace crate 重命名

| 当前 | 目标 | 影响面 |
|---|---|---|
| `codex-app-transfer` (src-tauri package + bin) | `cc-desktop-switch` | `[[bin]]` 名变,macOS 产物 `Contents/MacOS/cc-desktop-switch`;`Makefile` `cargo` 调用同步;`.github/workflows/ci.yml` 中 `-p codex-app-transfer` / `--exclude codex-app-transfer` 同步 |
| `codex-app-transfer-registry` | `cc-desktop-switch-registry` | `use codex_app_transfer_registry::*` 全仓 ~25 处 |
| `codex-app-transfer-proxy` | `cc-desktop-switch-proxy` | `use codex_app_transfer_proxy::*` ~5 处 |
| `codex-app-transfer-adapters` | `cc-desktop-switch-adapters` | `use codex_app_transfer_adapters::*` ~40 处 |
| `codex-app-transfer-codex-integration` | `cc-desktop-switch-codex-integration` | `use codex_app_transfer_codex_integration::*` ~8 处 |
| `codex-app-transfer-claude-desktop` | `cc-desktop-switch-claude-desktop` | 少量,主要 src-tauri admin handler |
| `codex-app-transfer-gemini-oauth` | `cc-desktop-switch-gemini-oauth` | `use codex_app_transfer_gemini_oauth::*` ~15 处 |

执行手法: workspace 根用 `cargo install cargo-edit` 后 `cargo set-package-name` 或
直接 `sed` 批量替换 + 全仓 `find -name Cargo.toml -exec sed -i ...`,然后
`cargo check --workspace` 一次性纠正所有 import path,最后跑 `cargo test --workspace`
+ `cargo build --release` 验证。

### 2. Bundle Identifier 迁移

- `src-tauri/tauri.conf.json` `identifier`: `store.alyse.codex-app-transfer` → `store.alyse.cc-desktop-switch`
- 影响: macOS 的 `~/Library/Application Support/<identifier>/`、Launch Services
  把它识为不同 app,自动更新(`tauri-plugin-updater`)的 endpoint
  `https://github.com/.../latest.json` 内部签名也跟 identifier 绑(取决于 plugin
  版本)。
- 迁移策略: app 启动时检测旧 identifier 的 LaunchAgents / preferences,如有就一次
  性 copy 到新 identifier 路径并提示用户(类似 `crates/codex_integration/src/
  snapshot.rs` 的幂等模式)。

### 3. 用户配置目录 `~/.codex-app-transfer/` → `~/.cc-desktop-switch/`

涉及代码:
- `crates/registry/src/paths.rs::CONFIG_DIR_NAME = ".codex-app-transfer"`
- `crates/codex_integration/src/paths.rs::CodexPaths::new` 中 `home.join(".codex-app-transfer")`
- `crates/proxy/src/forward.rs` 注释里提到 token 落盘位置
- `crates/registry/src/paths.rs::sessions_db_path` / `tool_artifacts_db_path`
- `frontend/js/i18n.js` 12+ 中英文文案(`geminiOauth.hint` / `antigravityOauth.hint`
  / `geminiOauth.logoutFailedManual` / `antigravityOauth.logoutFailedManual` /
  `guide.tsLogsText`)
- `frontend/index.html` 1 处 `guide.tsLogsText` 默认文案
- `frontend/js/app.js::downloadJson('codex-app-transfer-config-...')` 导出文件名
- 文档: `README.md` × 2 + `docs/build.md` + 各 release notes 与 RFC 中引用路径

实施手法: 加 `crates/registry/src/migrate.rs::migrate_app_home()`,启动时若 `~/.codex-app-transfer/` 存在而 `~/.cc-desktop-switch/` 不存在则 atomic rename;不存在的话静默跳过。i18n 文案改后由 i18n 走最新串。

### 4. Gateway key 前缀 `cas_` → `cds_`

- `crates/registry/`(token 生成)/`src-tauri/src/admin/handlers/settings.rs::regenerate_gateway_key`
- 影响: 用户已签发的 key(`cas_<base64>`)全部要让用户重新生成 + 重新 apply 到 Codex CLI / Claude Desktop,否则反代鉴权失败。
- 决策点: 是否值得改。`cas_` 字面已不直接对应产品名 (CC Desktop Switch ≠ Codex App Service);若改 `cds_` 算彻底切断脚手架痕迹,代价是一次用户操作中断。**建议留到 v3 才动**;Phase 2 内可以接受 `cas_` 作为"无意义随机前缀"沉默继承。

### 5. Cloudflare 资源名 `codex-app-transfer-feedback`

- `feedback-worker/wrangler.toml` `name` + `bucket_name`
- 影响: Worker URL `codex-app-transfer-feedback.<sub>.workers.dev` / R2 bucket 名都要重建 + DNS 迁移 + 旧反馈数据复制
- 决策点: 内部反馈管道,改名只有审美收益,无功能价值。**建议保留**,在 README 标"历史 ID"即可(Phase 1 已做)。

### 6. release.yml binary 路径对齐

当前 `.github/workflows/release.yml` 期待 macOS bundle 内 `Contents/MacOS/CC-Desktop-Switch`,但
`src-tauri/Cargo.toml` `[[bin]] name = "codex-app-transfer"` 会产出
`Contents/MacOS/codex-app-transfer`。release pipeline 真正能跑前必须解决,**与本文 §1 (crate rename) 一起做**。

### 7. 历史 release notes / archive 不动

`docs/release-notes/v1.0.x.md` / `v2.0.x.md` / `v2.1.x.md` / `docs/archive/*` / `docs/refactor/cleanup.md` 修订日志 / `docs/refactor/migration.md` 修订日志条目 —— 这些是史料,记录"当时的代码叫什么名字",**不**应回填新名;改了等于伪造历史。

### 8. 测试 fixture 路径

`tests/replay/fixtures/registry/*.json` 中如有 `codex-app-transfer` 字面用作 mock home(检查 `golden_compat.rs` 行为)。Phase 2 §3 路径改名后需要同步刷 fixture。

## 验收

- `cargo check --workspace` + `cargo test --workspace --no-fail-fast` 全绿
- 干净机器装 v2.2.0 不报错,启动后旧 v2.1.x 用户数据自动迁移(检测 `~/.codex-app-transfer/` 存在则 rename,新 install 直接走新路径)
- `make mac-app` 产出 `dist/mac/CC Desktop Switch.app/Contents/MacOS/cc-desktop-switch`
- `gh workflow run release.yml` 三平台全绿,产物名 `CC-Desktop-Switch-v<版本>-…`
- `grep -rE 'codex.app.transfer|codex_app_transfer|Codex App Transfer'` 全仓仅剩历史 release notes / archive / migration 修订日志 / `cas_` 前缀
