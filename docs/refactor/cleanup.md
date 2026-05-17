# Python → Rust 旧码清理方案

> 状态:**Phase 1-4 全部完成;PR #5 为最终收尾归档**
> 起草:2026-05-05
> 适用范围:`codex-app-transfer` 全仓
> 前置:`docs/refactor/migration.md` 全部 7 个 Stage 已落地,v2.0.0 已发版
> 落地总览:见 §5 修订日志末尾

---

## 1. 背景

v2.0.0 完成 Python → Rust/Tauri 全栈重写后,仓库里仍残留两类旧码:

1. **已被 Rust 完全替换的死码**:`backend/`、`main.py`、PyInstaller spec / NSIS / 老 PS1 脚本等。
2. **形式上是 Python 但仍在产线上的工具**:`scripts/release_assets.py`、`scripts/gen_registry_fixtures.py`、`tests/replay/`、PyInstaller release pipeline (`make {mac,linux,win}-release`)。

直接一次性切换会砍断签名 release 通道。需要分阶段推进,每个 Phase 一次 PR(走仓库的 main 分支保护流程)。

## 2. 现状盘点

### 活码(保留)

| 路径 | 角色 |
|---|---|
| `src-tauri/` | Tauri 2 壳 + 内嵌 axum admin server + cas:// scheme |
| `crates/{registry,proxy,adapters,codex_integration}` | ~7k 行 Rust,替代 backend/*.py |
| `frontend/` | Bootstrap + 原生 JS,字节级保留,通过 `include_dir!` 编进二进制 |
| `feedback-worker/worker.js` | Cloudflare Worker(独立微服务,不进 app 二进制) |

### 待清理 Python / 老打包

| 类别 | 路径 | 仍被引用? |
|---|---|---|
| 旧后端 | `backend/` 全部 18 个 .py | 不被 Rust 引用,但 `macos/build-macos.sh` / Dockerfile / Makefile 还在导版本号 |
| 旧入口 | `main.py` (root) | 仅老 PyInstaller 用 |
| 老打包 | `build.spec`、`build.bat`、`start.bat`、`installer.nsi` | 仅老路径用 |
| 老 Mac 打包 | `macos/build-macos.{sh,spec}`、`make-{dmg,pkg}.sh`、`prepare-icon.py`、`entitlements.plist` | **`make mac-release` 仍在调** |
| 老跨平台打包 | `docker/{linux,windows}-builder/Dockerfile`、`scripts/build-{linux,windows}-on-mac.sh` | `make {linux,win}-release` 仍在调,基于 PyInstaller + Wine |
| 老签名 | `scripts/{Invoke-CodeSigning,New-Release,Test-ReleaseSignature}.ps1` | 老 PowerShell 路径 |
| 老 Python 测试 | `tests/test_{deepseek_thinking_tool_history,isolation,kimi_real,tool_call}.py` | 全部 import `backend.*`,backend 删掉就死 |
| 配置/缓存 | `requirements.txt`、`pyproject.toml`、`.venv/`、`.pytest_cache/`、`Codex-App-Transfer-Setup-1.0.3.exe` | 残留 |
| 跨语言契约 | `scripts/gen_registry_fixtures.py`、`tests/replay/{fixture,player,recorder}.py`、`tests/test_replay_smoke.py` | **CI 还在跑,`crates/registry/tests/python_compat.rs` 依赖产物** |
| Release 打包 | `scripts/release_assets.py` | **`make *-release` 还在调,签名 + latest.json 由它生成** |
| Tauri 配置 | `src-tauri/tauri.conf.json` 里 `"version": "2.0.0-stage0"` | 跟 README v2.0.0 不一致 |
| Workspace | `Cargo.toml` 注释里还提 `ui` crate (Leptos),实际没目录 | 死注释 |
| CI | `.github/workflows/ci.yml` 里 `python-replay-tests` job + `ui-wasm-build` job(目录已不存在) | 半死 |

## 3. 分 Phase 清理方案

### Phase 1 — 无争议直接删

零风险,只是把 v2 路径下已经没用的东西干掉。**目标**:删掉 backend/ + 老 PyInstaller spec + 老 Python 集成测试 + Setup .exe + 老 PS1 脚本 + 死 CI job + tauri 版本号修正。

清单:
- `backend/` 全目录(18 个 .py)
- `main.py`(root)
- `tests/test_deepseek_thinking_tool_history.py`、`test_isolation.py`、`test_kimi_real.py`、`test_tool_call.py`
- `build.spec`、`build.bat`、`start.bat`、`installer.nsi`
- `Codex-App-Transfer-Setup-1.0.3.exe`(committed 二进制)
- `scripts/Invoke-CodeSigning.ps1`、`New-Release.ps1`、`Test-ReleaseSignature.ps1`
- CI workflow:删 `ui-wasm-build` job(`ui/` 目录已不存在)
- `Cargo.toml`:删 `# ui` 死注释
- `src-tauri/tauri.conf.json`:`2.0.0-stage0` → 真实版本号

**不删**(留给后续 Phase):
- `requirements.txt`、`pyproject.toml`(release_assets.py 还要用)
- `.venv/`、`.pytest_cache/`(本来就在 .gitignore)
- `macos/build-macos.*` 等 release pipeline(Phase 2)
- `scripts/gen_registry_fixtures.py`、`tests/replay/`(Phase 3)
- `python-replay-tests` CI job(Phase 3)

**验收**:
- `cargo build --workspace` 通过
- `cargo test --workspace` 通过(Rust 测试不依赖 backend/)
- `make mac-app` 仍能本地出 `.app`(纯 Rust 路径)
- Phase 1 PR 不动 release pipeline,`make mac-release` 暂时会因 `backend/config.py` 缺失而失败 —— 这是预期,Phase 2 修复

### Phase 2 — 用 `cargo tauri build` 替掉 PyInstaller release pipeline

**目标**:三平台 release 全部走 Tauri 原生 bundler,删掉 PyInstaller / Wine / NSIS / Docker 旧链路;**构建载体从"本机 Mac + Docker"切到 GitHub Actions matrix**(macos-14 / ubuntu-22.04 / windows-latest)。

#### 2.1 设计决策矩阵

| 决策点 | 选项 | 选择 | 理由 |
|---|---|---|---|
| 构建载体 | (a) 本机 Mac + Docker 沿用 / (b) GitHub Actions matrix | **(b)** | Tauri 官方推荐;免 Wine 5GB 镜像;签名 secret 用 GH Secrets;CI 自然出三平台 artifact |
| `release_assets.py` 何时退役 | (a) Phase 2 顺手用 Rust 重写 / (b) Phase 2 只调输入路径,Phase 3 再 xtask 重写 | **(b)** | Phase 2 已经动 release pipeline + GH Actions + 签名密钥三件大事,工具不变更安全;~30 行调路径 vs ~380 行重写 |
| `.pkg` 去留 | (a) 保留(Tauri 出 .app 后手动 productbuild) / (b) 退役只发 .dmg | **(b)** | Tauri 2 macOS bundler 不直接出 .pkg;.dmg 是 macOS 主流分发;.pkg 主要给企业 MDM,不在本项目场景 |
| `entitlements.plist` 去留 | (a) 删 / (b) 留,改由 `tauri.conf.json` 引用 | **(b)** | 沙箱权限/钥匙串访问声明仍需要;tauri 支持 `bundle.macOS.entitlements` 字段 |
| 产物命名 | (a) 接受 Tauri 默认名 / (b) workflow 内 mv 成老命名 | **(b)** | 用户既有下载链接 / 镜像不应失效;`release_assets.py` 的 PLATFORM_PATTERNS 也基于老命名,改 mv 比改正则简单 |
| `latest.json` 自签 RSA 密钥位置 | (a) 仍 `.release-signing/` 本地 / (b) GitHub Secret | **(b)** | CI 上签需要;本地仍可用 secret 备份的同一份私钥手动签 |
| Linux 出哪些格式 | (a) 仅 .tar.gz(沿用) / (b) .deb + .AppImage(Tauri 默认) | **(b) + 兼容性** | 接受 Tauri 默认 .deb / .AppImage,**额外**保留 `.tar.gz` 作为最低依赖兜底(workflow 内 tar 一下 .AppImage 解出的目录) |
| `make` 留多少 | (a) 全删走 GH Actions / (b) 保留 `mac-app` 本地自测 | **(b)** | 本地开发自测仍需快速出 .app;`make help` 收敛到 1-2 个 target |

#### 2.2 新增 `.github/workflows/release.yml` 蓝图

```yaml
name: release
on:
  push:
    tags: ['v*']
  workflow_dispatch:
    inputs:
      version: { description: '不带 v 前缀(e.g. 2.0.1)', required: true }

permissions: { contents: write }   # gh release create

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: macos-14,        target: aarch64-apple-darwin,   bundles: 'app,dmg' }
          - { os: ubuntu-22.04,    target: x86_64-unknown-linux-gnu, bundles: 'deb,appimage' }
          - { os: windows-latest,  target: x86_64-pc-windows-msvc, bundles: 'nsis,msi' }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: ${{ matrix.target }} }
      - uses: Swatinem/rust-cache@v2
      - if: runner.os == 'Linux'
        run: |   # 复用 ci.yml 已经打磨好的 retry 模式
          set -e
          for i in 1 2 3; do
            sudo apt-get update && sudo apt-get install -y --no-install-recommends \
              libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev \
              librsvg2-dev patchelf build-essential file && break || sleep 30
          done
      - name: cargo tauri build
        env:
          # 仅 Mac/Win 需要;Linux 无平台原生签名
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE_P12_BASE64 }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_API_KEY: ${{ secrets.APPLE_API_KEY }}
          APPLE_API_KEY_ID: ${{ secrets.APPLE_API_KEY_ID }}
          APPLE_API_ISSUER: ${{ secrets.APPLE_API_ISSUER }}
          # Windows: 通过 tauri.conf.json bundle.windows.signCommand 调 signtool
        run: |
          cargo install tauri-cli@^2 --locked
          cargo tauri build --target ${{ matrix.target }} --bundles ${{ matrix.bundles }}
      - name: rename to project naming
        shell: bash
        run: bash .github/workflows/scripts/rename-bundles.sh "${{ matrix.target }}"
      - uses: actions/upload-artifact@v4
        with: { name: bundle-${{ matrix.target }}, path: dist-renamed/ }

  release-bundle:
    needs: build
    runs-on: ubuntu-22.04
    steps:
      - uses: actions/checkout@v4
      - uses: actions/download-artifact@v4
        with: { path: release/, merge-multiple: true }
      - uses: actions/setup-python@v5
        with: { python-version: '3.13' }
      - run: pip install cryptography
      - name: write signing key from secret
        env: { KEY: ${{ secrets.RELEASE_PRIVATE_KEY_PEM }} }
        run: |
          mkdir -p .release-signing
          printf '%s\n' "$KEY" > .release-signing/release-private-key.pem
      - run: python scripts/release_assets.py --version "${VERSION}" --include macos linux windows --repo "${{ github.repository }}"
      - uses: softprops/action-gh-release@v2
        with: { files: 'release/*', tag_name: 'v${{ env.VERSION }}' }
```

> **实施修正(已迭代)**:**Tauri 2 没有 `bundle.fileName` 字段**(`tauri-build` schema 校验报 "unknown field");Phase 2 接受 Tauri 默认产物名 `Codex App Transfer_<V>_<arch>.<ext>`(productName 带空格透传),`release.yml` 的 rename step 用引号包裹 glob (`"$BDIR"/dmg/*.dmg`) 处理空格,直接 cp 成项目老命名,~30 行 bash case,不再需要单独的 `rename-bundles.sh`。

#### 2.3 文件级 diff 清单

**新增**:
- `.github/workflows/release.yml`(三平台 matrix build + release-bundle 收口)
- `src-tauri/tauri.conf.json` 加 `bundle.macOS.entitlements`、`bundle.linux.deb.depends` 子段(**`bundle.fileName` 和 `bundle.windows` 字段在 Tauri 2 schema 不存在,落地时撤回**)

**修改**:
- `Makefile`:删除 `mac-release` `linux-release` `win-release` `release` `release-bundle` `linux-image` `win-image`、变量 `PYTHON` `VERSION` `WIN_IMAGE_TAG` `LINUX_IMAGE_TAG` `REPO_FLAG`;只留 `mac-app`(本地自测)+ `clean` + `help`
- `docs/build.md`:大幅重写。主线变 GitHub Actions / `gh workflow run release.yml`,本地路径只剩 `make mac-app`
- `scripts/release_assets.py`:**整体重写适配 Tauri 输出形态** — 删除假设 PyInstaller 输出的 `collect_windows/mac/linux`(`dist/{mac,linux-folder,linux-onefile}` 路径),换成统一的 `collect_from_incoming(dist-incoming/, ...)`;`PLATFORM_PATTERNS` 改:macOS `.pkg` 退役、Linux `.tar.gz`/无后缀退役改 `.deb`/`.AppImage`、Windows `Portable.zip`/`-x64.exe` 退役改 `-Setup.exe`/`.msi`;新增 `--incoming-dir` CLI 参数

**删除**:
- `macos/build-macos.sh`、`build-macos.spec`、`make-dmg.sh`、`make-pkg.sh`、`prepare-icon.py`、`pkg-scripts/`、`README.md`(本目录的)
- `macos/entitlements.plist`:**保留**,被 `tauri.conf.json` 引用
- `docker/linux-builder/`、`docker/windows-builder/`(整个 docker/ 目录可空,`rmdir docker`)
- `scripts/build-linux-on-mac.sh`、`scripts/build-windows-on-mac.sh`

#### 2.4 签名密钥迁移到 GH Secrets

| 用途 | Secret 名 | 来源 / 必须? |
|---|---|---|
| `release_assets.py` RSA-3072 自签 | `RELEASE_PRIVATE_KEY_PEM` | 复制 `.release-signing/release-private-key.pem` 全文 / **必须** |
| Apple Developer ID 证书 | `APPLE_CERTIFICATE_P12_BASE64` | `base64 -i Cert.p12` / 可选(无则 ad-hoc 签名) |
| 同上密码 | `APPLE_CERTIFICATE_PASSWORD` | / 可选 |
| 同上 identity | `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Name (TEAMID)` / 可选 |
| Apple Notary API Key | `APPLE_API_KEY` (P8 全文)、`APPLE_API_KEY_ID`、`APPLE_API_ISSUER` | App Store Connect 生成 / 可选 |
| Windows Authenticode 证书 | `WIN_CODESIGN_CERT_BASE64`、`WIN_CODESIGN_PASSWORD` | / 可选 |

**首次切换**:用户在 GH 仓库 Settings → Secrets and variables → Actions 配置;**必须只配 `RELEASE_PRIVATE_KEY_PEM` 一条**就能跑通(Mac 退化为 ad-hoc 签名,Win 退化为未签名,Linux 本来就不平台签)。

#### 2.5 产物命名映射

**Tauri 2 没有 `bundle.fileName` 字段**(尝试加被 `tauri-build` schema 拒)。Tauri 默认产物名 `<productName>_<V>_<arch>.<ext>` 即 `Codex App Transfer_<V>_<arch>.<ext>` (带空格)。`release.yml` 的 rename step 用引号包裹 glob 命中带空格文件名,cp 成项目老命名:

| Tauri 2 默认 (productName="Codex App Transfer") | → 项目老名(staging/) |
|---|---|
| `target/<T>/release/bundle/dmg/Codex App Transfer_<V>_aarch64.dmg` | `Codex-App-Transfer-v<V>-macOS-arm64.dmg` |
| `target/<T>/release/bundle/deb/Codex App Transfer_<V>_amd64.deb` | `Codex-App-Transfer-v<V>-Linux-x86_64.deb` |
| `target/<T>/release/bundle/appimage/Codex App Transfer_<V>_amd64.AppImage` | `Codex-App-Transfer-v<V>-Linux-x86_64.AppImage` |
| `target/<T>/release/bundle/nsis/Codex App Transfer_<V>_x64-setup.exe` | `Codex-App-Transfer-v<V>-Windows-x64-Setup.exe` |
| `target/<T>/release/bundle/msi/Codex App Transfer_<V>_x64_en-US.msi` | `Codex-App-Transfer-v<V>-Windows-x64.msi` |

**调整说明**:
- `.pkg` 退役 (Tauri 2 macOS bundler 不直出);macOS 只发 `.dmg`
- Linux 不再用 `.tar.gz` (PyInstaller folder build 时代的形态);改 `.deb`+`.AppImage`
- Windows `Portable.zip` 退役 (Tauri 不直出);改 `-Setup.exe`(NSIS)+ `.msi`(WiX)
- `release_assets.py` 的 `PLATFORM_PATTERNS` 同步更新

#### 2.6 验收

- 本地 `make mac-app` 仍出 `.app`(无回归)
- `gh workflow run release.yml -f version=2.0.1-rc1` 触发,三平台全跑成功,artifact 全部生成 + 签名 + `latest.json` 含 3 个 platforms 项
- 用 `release/Codex-App-Transfer-release-public.pem` 验签所有 `.sig` 通过(沿用 `Test-ReleaseSignature.ps1` 的协议)
- `find . -name 'Dockerfile' -o -name '*.spec'` 应为空
- `make help` 输出只剩 `mac-app` / `clean`

#### 2.7 回滚策略

- **PR 内 commit 顺序**:先加 `release.yml` + `rename-bundles.sh`,再改 `tauri.conf.json` / `release_assets.py`,最后才删 `macos/build-macos.sh` / `docker/`。如果在合 PR 之前 dispatch 测试 release 跑挂,只 revert 最后那条删除 commit 即可继续用老路径
- **合 PR 之前的硬验收**:用户必须先用 `gh workflow run release.yml -f version=2.0.1-rc1` 在 feature 分支上完整跑通一次三平台 + 签名,artifact 落到 GH release(可以是 draft / pre-release),验过再 squash-merge
- **如果合 PR 后第一次 tag release 翻车**:`git revert` Phase 2 PR + 重新 tag 旧版本号 + 通过临时手工跑 `release_assets.py` 应急

#### 2.8 范围明确**不做**(留给 Phase 3+)

- 不重写 `release_assets.py` 为 Rust(只调输入路径,Phase 3 用 `xtask release-bundle` 统一替换)
- 不动 `tests/replay/*.py` 和 `gen_registry_fixtures.py`
- 不删 `requirements.txt`(`release_assets.py` 还要 `cryptography`;Phase 3 完成后 Phase 4 再删)
- 不改 `feedback-worker/`(独立微服务,不在 cleanup 范围)

### Phase 3 — 跨语言契约工具改造

**目标**:把仓库剩余的 Python 工具(`gen_registry_fixtures.py` + `release_assets.py` + `tests/replay/*.py` + `test_replay_smoke.py`)全部用 Rust `xtask` 重写或删除,实现"仓库只有 Rust + JS(frontend + worker) + 少量 shell"。

#### 3.1 现状盘点(Phase 2 完成后)

| 路径 | 状态 | Phase 3 处置 |
|---|---|---|
| `scripts/gen_registry_fixtures.py` (107 行) | **当前已死**:`from backend.config import ...` 在 Phase 1 删 backend 后失败 | 删除,改 `xtask gen-fixtures` |
| `tests/replay/fixtures/registry/*.json` (4 份) | 仍 commit 在仓库,`python_compat.rs` 读它们做 round-trip | **保留**(权威源从 Python 改成 xtask + commit) |
| `crates/registry/tests/python_compat.rs` (146 行, 5 测试) | 仍跑通,但名字 + 注释提"Python 比对"已过时 | 改名 `golden_compat.rs` + 删冗余测试 + 加自检 |
| `tests/replay/__init__.py` / `fixture.py` / `player.py` / `recorder.py` | Python 录制 / 回放工具,Rust 集成测试不走它们 | 全删 |
| `tests/test_replay_smoke.py` | CI `python-replay-tests` job 跑它 | 删 + 删 CI job |
| `scripts/release_assets.py` (重写后约 280 行) | Phase 2 调通的 Tauri 输出适配版 | Rust 重写为 `xtask release-bundle` |
| `requirements.txt` / `pyproject.toml` | release_assets.py 用 `cryptography` | xtask 替代后删(留给 Phase 4 收尾,但 Phase 3 已可删) |

#### 3.2 设计决策矩阵

| 决策点 | 选项 | 选择 | 理由 |
|---|---|---|---|
| RSA crate | (a) `ring`(快,FFI) / (b) `rsa`(纯 Rust) | **(b) `rsa = "0.9"`** | release-bundle 不是性能敏感(每个 release 跑 1 次,几十个文件签名 < 1s);`rsa` 纯 Rust 不依赖 system crypto,GH Actions 跑无环境差异;API 比 `ring` 友好 |
| xtask binary 命名 | (a) `cargo xtask <sub>` (符号链接 alias) / (b) `cargo run -p xtask --release -- <sub>` | **(b)** | 不引入 `.cargo/config.toml` alias 复杂度;CI 直接用,本地可加 `make` shortcut |
| fixture round-trip 校验 | (a) 删 `python_compat.rs` 全部 5 测试,只在 CI 跑反向 diff / (b) 留 round-trip 测试,**额外**加 CI 反向 diff | **(b)** | round-trip 测试是单元级保险,反向 diff 是端到端保险,两层都要 |
| `recorder.py` 去留 | (a) Rust 重写 / (b) 删,以后录新 fixture 用 `curl + tee` 临时手工 | **(b)** | recorder 一年用不到几次,Rust 重写工作量 > 收益;留 README 一行说明 "录新 fixture: curl ... > target.json" 即可 |
| `tests/replay/fixtures/` 路径 | (a) 移到 `crates/registry/tests/fixtures/` 让物理位置贴近 / (b) 保留现路径 | **(b)** | 跨多个 crate 用(registry/proxy/adapters tests 都读),共享一份就行;移路径还要改一堆 `..` 相对引用,不值 |
| Python 残留删除时机 | (a) Phase 3 同 PR 删 / (b) Phase 4 收尾再删 | **(a)** | xtask 一旦上线 Python 即彻底无用,留着只是干扰;Phase 4 留 README/Makefile/migration-plan 修订日志收尾 |

#### 3.3 xtask crate 蓝图

新增 `xtask/Cargo.toml`:

```toml
[package]
name = "xtask"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
publish = false

[[bin]]
name = "xtask"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = { version = "1", features = ["preserve_order"] }
anyhow = "1"

# release-bundle only
rsa = { version = "0.9", features = ["sha2"] }
sha2 = "0.10"
base64 = "0.22"
chrono = { version = "0.4", default-features = false, features = ["clock", "serde"] }
walkdir = "2"
regex = "1"

# gen-fixtures only
codex-app-transfer-registry = { path = "../crates/registry" }
```

`Cargo.toml` workspace.members 把注释里的 `# "xtask"` 启用。

`xtask/src/main.rs`:用 clap derive 出两个子命令 `gen-fixtures` 和 `release-bundle`。

#### 3.4 `xtask gen-fixtures` 实现要点

复刻 `gen_registry_fixtures.py` 的 4 份输出:
1. `default_config.json` — `Config::default()` 序列化(无末尾换行)
2. `with_provider.json` — default + 1 个合成 provider(中文 name、所有必备字段、6 model 槽位)
3. `builtin_presets.json` — `builtin_presets()` 的 `Vec<Provider>` 序列化
4. `library_entry.json` — 1 个 library 条目(末尾**带** `\n`,经 `save_raw_library` 写)

关键约束:必须**字节级**与 commit 在仓库的当前 fixture 一致。验证手段:
- 实现完先本地跑 `cargo run -p xtask -- gen-fixtures`
- `git diff --exit-code -- tests/replay/fixtures/registry/` 应为空
- 不空则要么 fixture 落后(Rust schema 已更新),要么实现有 bug;两种情况下手工修正

输出格式细节:`serde_json::to_string_pretty`(默认 indent=2、key 顺序保留(`preserve_order` feature)、非 ASCII 不转义)— 跟 Python 的 `json.dump(ensure_ascii=False, indent=2)` 等价(Phase 0 已验证)。Library 条目额外 `f.write("\n")`。

#### 3.5 `xtask release-bundle` 实现要点

CLI 兼容现 `release_assets.py`:

```
xtask release-bundle \
  --version <X.Y.Z> \
  [--include macos linux windows] \
  [--incoming-dir dist-incoming] \
  [--output-dir release] \
  [--repo owner/repo]
```

核心算法逐项 1:1 复刻:

| 函数 | Python | Rust |
|---|---|---|
| `get_or_create_key` | `cryptography.rsa.generate_private_key` PKCS#8 PEM | `rsa::RsaPrivateKey::new` + `rsa::pkcs8::EncodePrivateKey::to_pkcs8_pem` |
| `sha256_of` | `hashlib.sha256` 流式 1MB | `sha2::Sha256` + `BufReader` 1MB chunks |
| `sign_file` | `private_key.sign(bytes, PKCS1v15(), SHA256())` | `rsa::Pkcs1v15Sign::new::<sha2::Sha256>()` + `private_key.sign(...)`,base64 encode |
| `latest.json` schema | `dict` → `json.dumps(indent=2, ensure_ascii=False)` | `serde_json::to_string_pretty` + `serde_json::Value`(preserve_order) |
| `PLATFORM_PATTERNS` | Python regex | `regex` crate |
| `pub_date` | `datetime.utcnow()` | `chrono::Utc::now()` 同样格式 `%Y-%m-%dT%H:%M:%SZ` |

**密钥兼容性硬要求**:Rust 生成的 PKCS#8 PEM 必须能被 `cryptography` 读(以及反向)。`rsa = "0.9"` 的 `EncodePrivateKey`/`DecodePrivateKey` 走标准 PKCS#8,`cryptography` 也走同标准 — 兼容。**实施时单测验证**:加一个 test 读 Phase 2 已存在的 `.release-signing/release-private-key.pem`,sign 同一段 bytes,验证签名能被 Python `cryptography` 验过(reverse 测试)。

#### 3.6 `python_compat.rs` → `golden_compat.rs` 改造

文件改名 `crates/registry/tests/golden_compat.rs`,**保留全部 7 个测试,只改名 + 改注释**(原 plan 想删 `typed_config_can_parse_*` 2 个,落地时发现它们检查"字段值正确"是反向 diff + round-trip 都不覆盖的另一维度,**留着不亏**):

| 原测试名 | 新测试名 | 调整 |
|---|---|---|
| `default_config_roundtrip` | (同) | 无 |
| `with_provider_roundtrip` | (同) | 无 |
| `builtin_presets_roundtrip_python_dump` | `builtin_presets_roundtrip` | 去 `_python_dump` 后缀 |
| `library_entry_roundtrip` | (同) | 无 |
| `rust_embedded_presets_match_python_dump` | `rust_embedded_presets_match_committed_fixture` | 语义改"对照 commit golden"而非"对照 Python dump" |
| `typed_config_can_parse_python_default` | `typed_config_can_parse_default` | 去 `_python` 中缀 |
| `typed_config_can_parse_python_with_provider` | `typed_config_can_parse_with_provider` | 同上 |

文件头注释 + panic message 中 `python` / `gen_registry_fixtures.py` 提法换成 `golden` / `xtask gen-fixtures` + git 维护权威源闭环说明。

#### 3.7 CI 反向 diff(ci.yml 加一 step)

在 `cargo test --workspace` 之后加:

```yaml
- name: Verify fixtures regenerable
  run: |
    cargo run -p xtask --release -- gen-fixtures
    git diff --exit-code -- tests/replay/fixtures/registry/
```

任何 `crates/registry` 的 schema / 序列化器变动 + 忘记重新 commit fixture → CI 立即红,error message 引导用户 `cargo run -p xtask -- gen-fixtures && git add -p tests/replay/fixtures/registry/`。

同步**删**老的 `python-replay-tests` job(整段)。

#### 3.8 release.yml 切 xtask

改 `.github/workflows/release.yml` 的 release-bundle job:
- **删** `actions/setup-python@v5` step
- **删** `pip install cryptography` step
- **改** `python scripts/release_assets.py --version ... --include ...` →
  `cargo run -p xtask --release -- release-bundle --version ... --include ...`
- **加** `Swatinem/rust-cache@v2`(workspaces "." key xtask) 复用 build job 的 cache

#### 3.9 文件级 diff 清单

**新增**:
- `xtask/Cargo.toml`
- `xtask/src/main.rs`(clap 解析 + 子命令分发)
- `xtask/src/gen_fixtures.rs`
- `xtask/src/release_bundle.rs`

**修改**:
- `Cargo.toml`(根) — workspace.members 把 `# "xtask"` 取消注释
- `Cargo.lock` — 自动更新(rsa / sha2 / base64 / chrono / walkdir / regex / clap 等新增 transitive deps)
- `crates/registry/tests/python_compat.rs` → 改名 `golden_compat.rs`,删 3 测试,改名 1 测试
- `tests/replay/fixtures/registry/*.json` 注释/header 不变(JSON 文件本身无注释,只是文件 header 在文档里说明权威源)
- `.github/workflows/ci.yml`(加反向 diff step + 删 `python-replay-tests` job)
- `.github/workflows/release.yml`(切 xtask)
- `docs/refactor/cleanup.md` 修订日志追加

**删除**:
- `scripts/gen_registry_fixtures.py`
- `scripts/release_assets.py`
- `tests/replay/__init__.py`
- `tests/replay/fixture.py`
- `tests/replay/player.py`
- `tests/replay/recorder.py`(以后录新 fixture 改用 `curl ... > tests/replay/fixtures/<X>.json`)
- `tests/test_replay_smoke.py`
- `requirements.txt`
- `pyproject.toml`
- `tests/replay/__pycache__/`(本地缓存,本来就 .gitignored,不在 commit 里)

**保留**:
- `tests/replay/fixtures/`(Rust 测试要读)

#### 3.10 验收

- `cargo run -p xtask -- gen-fixtures && git diff --exit-code -- tests/replay/fixtures/registry/` 干净
- `cargo run -p xtask --release -- release-bundle --version 0.0.0-dryrun --include macos --incoming-dir <空目录>` 正常 fail("no platform artifacts found")而不 panic
- 加一个临时 .release-signing/release-private-key.pem(Phase 2 留下的)+ 假 dist-incoming/ 含一份 .dmg → `xtask release-bundle` 出 release/{*.sig, *.sha256, latest.json} 与 Python 旧版本 byte-for-byte 一致(关键兼容性硬要求)
- `release.yml` `gh workflow run -f version=2.0.2-rc1 --ref feature/cleanup-phase-3` dispatch 跑通三平台 + 签名 + draft release
- `find . -name '*.py'` 应为空(排 `.venv/`)
- `find . -name 'requirements.txt' -o -name 'pyproject.toml'` 应为空

#### 3.11 回滚策略

- **PR 内 commit 顺序**(关键防回滚):B(xtask gen-fixtures) → C(python_compat 改造 + CI 反向 diff)→ D(xtask release-bundle 实现 + 单测验证签名兼容)→ E(release.yml 切 xtask)→ F(删 Python 残留)。F 是最后一步,前面任何一步挂掉都不影响 Python 路径仍能跑
- **DELETE-ONLY commit 单独成 F**:revert 只回 1 个 commit 就能恢复 Python 路径
- **xtask release-bundle 必须先单测验证密钥兼容**(读 Phase 2 .release-signing/ 私钥,sign 一段 bytes,Python `cryptography` 反向验签),否则 Phase 2 release 出的老资产无法被新 xtask 验证 → 链断

#### 3.12 范围明确**不做**(留给 Phase 4)

- 不动 `README.md` / `docs/refactor/migration.md` / `Makefile` 注释里的 Python 提法(Phase 4 收尾统一改)
- 不删 `.gitignore`(Phase 4 检查 `.venv/` `.pytest_cache/` 是否漏)
- 不重命名 `tests/replay/` 路径(不动文件位置)

### Phase 4 — 收尾

- 删 `requirements.txt`、`pyproject.toml`(Phase 3 完成后无 Python 文件)
- `.gitignore` 加 `.venv/`、`.pytest_cache/`(若未加),并删除已 track 的副本
- `README.md`:更新所有"Python"提法
- `Makefile`:更新注释,version 来源改为 `Cargo.toml`
- `docs/refactor/migration.md`:在文末追加"清理已完成"修订日志
- 仓库里 `find . -name '*.py'` 应只剩 0 个或仅 `feedback-worker/`(无)

## 4. 时间线 / 里程碑

| Phase | 内容 | 风险 | 估时 | 触发条件 |
|---|---|---|---|---|
| 1 | 删死码 + tauri 版本号 + 死 CI job | 零 | 1 PR / 1 小时 | **现在** |
| 2 | release pipeline 全部切 Tauri bundler | 中(签名 / 公证 / 三平台 bundler 验证) | 1 PR + 1 个 v2.0.x release 周期 | 下个 release 之前 |
| 3 | 契约工具改造(xtask gen-fixtures + xtask release-bundle) | 低(已有 Rust 替身) | 1-2 PR | Phase 2 落地后 |
| 4 | 收尾(删 requirements.txt 等) | 零 | 1 PR | Phase 3 落地后 |

## 5. 修订日志

| 日期 | 来源 | 偏差 | 原因 |
|---|---|---|---|
| 2026-05-05 | 初稿 | — | 用户确认整体方向,启动 Phase 1 |
| 2026-05-05 | Phase 1 PR CI 反馈 | 顺手在 Phase 1 修了 main 上 pre-existing 红:`cargo fmt` drift + pytest 9 import error(加 `pythonpath = ["."]`)+ `src-tauri` 跨平台 dep 误放 macos 块导致 Linux 编译失败 | 不修 CI 永远绿不了,后续 Phase 用不上回归门禁 |
| 2026-05-05 | Phase 1 → Phase 3 范围微调 | CI 中"Python 重生 registry fixture → diff" 步骤直接删除(原属 Phase 3 范围)。原因:Phase 1 删了 backend/ 后 `gen_registry_fixtures.py` 失去数据源,该 CI 步骤会一直红;`python_compat.rs` 仍读 commit 的 fixture 做 round-trip,反向校验(Rust → diff)留给 Phase 3 xtask 重建 | 最小化 CI 红区,保住回归门禁;真正的 xtask 替身仍按 Phase 3 计划做 |
| 2026-05-05 | Phase 1 后期 CI 修(3 个补 commit) | (1) `aebf8cd` 补 `Cargo.lock` 与 `src-tauri` dep 删除的同步(16bb9fb 漏);(2) `c53de00` `apt-get install` 加 `timeout-minutes:8` + 3 次 retry + `--no-install-recommends`,抗 archive.ubuntu.com 偶发抖动(上一次 run 卡 17 分钟);(3) `e4d3382` 修 `crates/registry/src/raw_io.rs::tests::tempdir()` 并发 race(共享 `cas-registry-test-{pid}` 目录,加 `AtomicU64` counter 保唯一) | 都是 Phase 1 范围内的 CI 修,记录在案以便 Phase 2 不重蹈覆辙 |
| 2026-05-05 | Phase 2 详细方案落地 | 把 §3 Phase 2 概念性段落扩充为 §2.1-2.8 子章节(决策矩阵 / release.yml 蓝图 / 文件 diff 清单 / 签名密钥迁移 / 产物命名映射 / 验收 / 回滚 / 不做项) | 概念描述不足以直接动手,细化后让 reviewer 在动代码之前先评设计 |
| 2026-05-05 | Phase 2 §2.3/§2.5 修正 | 实施时尝试 `tauri.conf.json` 加 `bundle.fileName: "codex-app-transfer"` 让 Tauri 产物名摆脱空格,rename 逻辑直接写在 `release.yml` 的 ~30 行 bash case,**不再需要单独的 `rename-bundles.sh`**;`release_assets.py` 不是"微调路径"而是**整体重写**:删 `collect_windows/mac/linux`(假设 PyInstaller 输出),换 `collect_from_incoming(dist-incoming/)`,`PLATFORM_PATTERNS` 同步换为 `.dmg`/`.deb`/`.AppImage`/`-Setup.exe`/`.msi` | 调研 Tauri 2 实际产物形态后发现原设计估计过粗,落地修正 |
| 2026-05-05 | Phase 2 cargo check 第一次红, 二次修正 tauri.conf.json | CI `cargo check` 报 `tauri-build` schema 校验失败:`unknown field "fileName"`(允许的 bundle.* 字段为 `active/targets/createUpdaterArtifacts/publisher/homepage/icon/resources/copyright/license/category/fileAssociations/short-description/long-description/use-local-tools-dir/external-bin/windows/linux/macOS/iOS/android`)。同时 `bundle.windows.{wix,nsis}` 空对象也无效。**撤回 `bundle.fileName` 和 `bundle.windows` 子段**,接受 Tauri 默认带空格产物名 `Codex App Transfer_<V>_<arch>.<ext>`,glob 用引号处理 | 之前的 plan 来自外部调研建议,Tauri 2 实际 schema 与建议不符;落地必须以 `tauri-build` 实际校验结果为准 |
| 2026-05-05 | Phase 3 详细方案落地 | 把 §3 Phase 3 概念性段落扩充为 §3.1-3.12 子章节(现状盘点 / 决策矩阵 / xtask crate 蓝图 / gen-fixtures 实现要点 / release-bundle 实现要点 / golden_compat 改造 / CI 反向 diff / release.yml 切 xtask / 文件 diff 清单 / 验收 / 回滚策略 / 不做项)。RSA crate 选 `rsa = "0.9"` 而非 `ring`(纯 Rust 不依赖 system crypto + API 更友好,release-bundle 非性能敏感) | Phase 3 涉及 xtask 重写 + 删大量 Python,概念描述不足;细化后让 reviewer 在动代码之前先评关键决策(RSA crate / xtask CLI 命名 / fixture 路径 / Python 删除时机) |
| 2026-05-05 | Phase 3 §3.6 修正 | 落地 golden_compat 改造时**保留全部 7 测试只改名**,而非原 plan 想删 `typed_config_can_parse_*` 2 个。原因:`typed_config_can_parse_*` 验证"typed Config struct 能消化 fixture + 字段值正确",这是 round-trip 字节级 / 反向 diff 都不覆盖的另一维度(语义层断言,如 `proxy_port == 18080`)| 删测试丢覆盖,改名 + 调注释成本极低 |
| 2026-05-05 | Phase 3 ci.yml stacked PR trigger 修 | Phase 3 全 6 commit push 后 PR #4 (base=feature/cleanup-phase-2) 没触发 CI, 因 ci.yml `on.pull_request.branches: [main]` 只对 base=main 的 PR 跑. 加第 7 commit 删该 branches 限制, 让 stacked PR 也走回归门禁. push trigger 仍限 main (避免 feature 分支 push 双触发) | stacked PR 是 cleanup 系列必然形态(三层依赖), CI 触发条件必须同步放宽 |
| 2026-05-05 | Phase 2 dispatch test 失败 + PR #6 hotfix | PR #3 squash 到 main 后 dispatch `release.yml` 暴露 3 bug: (1) Linux upload-artifact "No files found" — `BDIR` 错指 `src-tauri/target/`, Cargo workspace 实际 target 是仓库根 `target/`; (2) rename step 静默成功但 staging 空 (#1 次生症状); (3) Windows `cargo binstall tauri-cli` 卡 30+ min. 单独 hotfix PR #6 (base=main) 修这 3 个 + `release_assets.py` 公钥从私钥派生 (CI secret 只配 `RELEASE_PRIVATE_KEY_PEM` 时缺公钥). PR #4 后来把 PR #6 的 build job 改动直接合进 phase-3 commit `65fe5a2`, PR #4 独立合并即获完整 hotfix; PR #6 在 PR #4 合并后变冗余可关 | dispatch test 在 squash-merge 前必须跑;**stacked PR 在底层 PR squash 后, GitHub 不会自动切 base, 必须 `gh pr edit X --base main` 显式 retarget** |
| 2026-05-05 | PR #4/#5 review 反馈 + 双 PR rebase + retarget | (1) PR #4 (Phase 3) 原 base=feature/cleanup-phase-2, 没含 PR #6 hotfix → 操作: `git reset --hard origin/main` + cherry-pick phase-3 自己的 7 commits + 加第 8 commit 把 PR #6 build job 改动合进, force-push, `gh pr edit 4 --base main`. (2) PR #5 (Phase 4) 原 base=feature/cleanup-phase-3 + 顶部状态 over-claim "Phase 1-4 全部完成" + README 推荐下载产物清单仍是旧 PyInstaller 时代 (Portable.zip / pkg / tar.gz / 无后缀 Linux 单文件) → 操作: reset 到新 phase-3 head + cherry-pick phase-4 unique commit + 改 README 产物为 Tauri 实际 (Windows-x64-Setup.exe / .msi / macOS-arm64.dmg / Linux-x86_64.deb / .AppImage) + 改 plan 顶部为实际进度状态 + retarget base=main | over-claim 文档归档必须等 PR 真正合并后再写完成态;否则 review 会发现"事实不一致"|
