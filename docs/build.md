# Building Codex App Transfer

Phase 2 起 release pipeline 全部走 GitHub Actions
(`.github/workflows/release.yml`)。本地仓库**不再**需要 Docker / Wine /
PyInstaller / Python venv 来出三平台 release;本地 Makefile 只保留:

- `make mac-app` — 出未签 `.app` 自测
- `make clean` — 清理 `build/` `dist/` `release/` `.release-signing/` `.tmp/`

唯一版本源:`src-tauri/Cargo.toml` 的 `[package].version`(`tauri.conf.json`
也带一份,**保持两处一致**;后续 Phase 4 会做单向同步)。
发布工作流会在打包前执行 `python3 scripts/check_release_version.py <version>`,
如果 workflow/tag 解析出的版本、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`
三者不一致,会直接 fail-fast,避免再次出现资产文件名 / `latest.json`
是新版本但安装包内部版本仍是旧版本的 release。

## 三平台 release 触发

### 推荐:tag 触发

```bash
# 在本地 main 上
git tag v2.0.1
git push --tags
```

这会触发 `release.yml`,三平台同时 build → rename → upload artifact →
收口 job 跑 `cargo run -p xtask --release -- release-bundle` 出
sha256/.sig/`latest.json` → 创建一个 **draft** GitHub release
(用户手动 ready/publish)。

### 备用:手动触发

```bash
gh workflow run release.yml -f version=2.0.1
# 或 workflow_dispatch 也接受 RC 版本
gh workflow run release.yml -f version=2.0.1-rc1
```

`workflow_dispatch` 不要求先打 tag,适合 release rehearsal。

### 监控

```bash
gh run watch
gh run list --workflow=release.yml --limit 3
```

## 一次性 secret 配置

仓库 Settings → Secrets and variables → Actions → New repository secret:

### 必须

| Secret 名 | 内容 |
|---|---|
| `RELEASE_PRIVATE_KEY_PEM` | RSA-3072 PKCS#8 PEM 全文(`.release-signing/release-private-key.pem` 的内容) |

`RELEASE_PRIVATE_KEY_PEM` 缺失会让 release-bundle job fail-fast。如果是
首次 release,在本地跑一次:

```bash
cargo run -p xtask --release -- release-bundle \
  --version 0.0.0-init --include macos --incoming-dir <空目录>
```

(失败没关系,会正常 exit 1 报 "no platform artifacts found",但路径上
已经先生成了 keypair) → keypair 落到 `.release-signing/`,把
`release-private-key.pem` 全文复制进 secret 即可。**`.release-signing/`
已 .gitignored,公钥(`release-public-key.pem`)用户分发后让最终用户验签**。

> **历史**:Phase 3 之前用 `python scripts/release_assets.py ...`,
> Phase 3 (PR #4) 改用 Rust `xtask release-bundle`,RSA-3072 PKCS#1 v1.5 +
> SHA-256 算法 1:1 复刻,公私钥 PKCS#8 PEM 双向兼容(私钥可被 Python
> `cryptography` 读、公钥也可)。

### 可选

| Secret 名 | 用途 | 缺失时 |
|---|---|---|
| `APPLE_CERTIFICATE` | Apple Developer ID `.p12`(base64) | macOS .dmg 退化为 ad-hoc 签名(用户首次启动需右键打开) |
| `APPLE_CERTIFICATE_PASSWORD` | 上面 .p12 的密码 | 同上 |
| `APPLE_SIGNING_IDENTITY` | 形如 `Developer ID Application: Foo (TEAMID)` | 同上 |
| `APPLE_API_KEY_BASE64` | App Store Connect API .p8(base64) | 跳过公证,Gatekeeper 第一次启动会再 quarantine |
| `APPLE_API_KEY` | 上面 key 的 ID | 同上 |
| `APPLE_API_ISSUER` | App Store Connect Issuer ID | 同上 |

> **Windows 签名**:Phase 2 暂不接入。后续如有 EV / OV 证书,在
> `tauri.conf.json` 的 `bundle.windows.signCommand` 注入 signtool /
> Azure Trusted Signing 命令即可。

## 本地自测出 .app

```bash
make mac-app
# → dist/mac/Codex App Transfer.app (未签, 双击启动有 Gatekeeper 警告)
```

只用于本地开发自测。要分发的 release 一律走 GitHub Actions。

## 产物清单

`release.yml` 一次成功执行后落到 GitHub release 的资产:

```
Codex-App-Transfer-v2.0.1-macOS-arm64.dmg
Codex-App-Transfer-v2.0.1-macOS-arm64.dmg.sha256
Codex-App-Transfer-v2.0.1-macOS-arm64.dmg.sig
Codex-App-Transfer-v2.0.1-Linux-x86_64.deb
Codex-App-Transfer-v2.0.1-Linux-x86_64.deb.sha256
Codex-App-Transfer-v2.0.1-Linux-x86_64.deb.sig
Codex-App-Transfer-v2.0.1-Linux-x86_64.AppImage
Codex-App-Transfer-v2.0.1-Linux-x86_64.AppImage.sha256
Codex-App-Transfer-v2.0.1-Linux-x86_64.AppImage.sig
Codex-App-Transfer-v2.0.1-Windows-x64-Setup.exe
Codex-App-Transfer-v2.0.1-Windows-x64-Setup.exe.sha256
Codex-App-Transfer-v2.0.1-Windows-x64-Setup.exe.sig
Codex-App-Transfer-v2.0.1-Windows-x64.msi
Codex-App-Transfer-v2.0.1-Windows-x64.msi.sha256
Codex-App-Transfer-v2.0.1-Windows-x64.msi.sig
Codex-App-Transfer-release-public.pem
latest.json
latest.json.sha256
latest.json.sig
```

格式说明:
- macOS 只发 `.dmg`(Tauri 不直出 `.pkg`,`.pkg` 已退役)
- Linux 双格式:`.deb`(Debian/Ubuntu 系,自动拉运行时依赖)+
  `.AppImage`(免安装,任何 distro 直接 `chmod +x` 跑)
- Windows 双格式:`-Setup.exe`(NSIS 安装包,推荐)+ `.msi`(企业 MDM
  / GPO 部署用)。Portable.zip 退役

## 验签

公钥:`Codex-App-Transfer-release-public.pem`(随每个 release 一起发布)

签名协议:RSA-3072 PKCS#1 v1.5 over the raw file bytes,SHA-256 哈希,
签名 base64 存在 `<file>.sig`。

任意平台(需要 Python + cryptography):

```bash
python -c "
import base64, sys
from pathlib import Path
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding
pub = serialization.load_pem_public_key(
    Path('Codex-App-Transfer-release-public.pem').read_bytes())
asset = sys.argv[1]
sig = base64.b64decode(Path(asset + '.sig').read_text())
pub.verify(sig, Path(asset).read_bytes(), padding.PKCS1v15(), hashes.SHA256())
print('OK')
" Codex-App-Transfer-v2.0.1-Windows-x64-Setup.exe
```

`latest.json` 和它的 `.sig` 同样可用上面命令验签。

## CI 链路全图

```
push tag v* / workflow_dispatch
        │
        ▼
 ┌──────────────────────────────────────────────────────────────────┐
 │ build (matrix)                                                    │
 │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐   │
 │  │ macos-14        │  │ ubuntu-22.04    │  │ windows-latest  │   │
 │  │ aarch64-apple-  │  │ x86_64-unknown- │  │ x86_64-pc-      │   │
 │  │ darwin          │  │ linux-gnu       │  │ windows-msvc    │   │
 │  │ bundles=app,dmg │  │ bundles=deb,    │  │ bundles=nsis,msi│   │
 │  │                 │  │   appimage      │  │                 │   │
 │  │ cargo tauri     │  │ apt+retry →     │  │ cargo tauri     │   │
 │  │ build           │  │ cargo tauri     │  │ build           │   │
 │  │  ↓              │  │ build           │  │  ↓              │   │
 │  │ rename → staging│  │  ↓              │  │ rename → staging│   │
 │  │  ↓              │  │ rename → staging│  │  ↓              │   │
 │  │ upload-artifact │  │  ↓              │  │ upload-artifact │   │
 │  └─────────────────┘  │ upload-artifact │  └─────────────────┘   │
 │                       └─────────────────┘                         │
 └────────────────────────────┬─────────────────────────────────────┘
                              ▼
                  ┌────────────────────────┐
                  │ release-bundle         │
                  │ ubuntu-22.04           │
                  │  ↓                     │
                  │ download-artifact      │
                  │  ↓                     │
                  │ xtask release-bundle   │
                  │  → release/*.sig+.sha256+latest.json │
                  │  ↓                     │
                  │ softprops/action-      │
                  │ gh-release@v2          │
                  │  → DRAFT GH release    │
                  └────────────────────────┘
```

draft release 由用户手动 ready/publish。失败重跑:`gh run rerun <ID>` 或
重新触发 `gh workflow run release.yml -f version=...`。

## Phase 2 之前的旧路径

如果你看到老 PR / issue 提到 `make mac-release` / `make linux-release` /
`make win-release` / `docker build` / Wine / PyInstaller / NSIS,这些是
Phase 2 之前的本地路径,现已**全部删除**(Phase 2 PR)。详见
`docs/refactor/cleanup.md` Phase 2。

## 故障排查

| 症状 | 可能原因 | 修法 |
|---|---|---|
| release-bundle job 报 `RELEASE_PRIVATE_KEY_PEM secret 未配置` | 仓库 secret 未配 | 见上"必须 secret"章节 |
| build job rename step 报 `if-no-files-found: error` | Tauri 没出该格式产物 | 看 `cargo tauri build` 输出,确认 `--bundles` 字符串拼写 |
| Linux apt-get 卡住 / 失败 | archive.ubuntu.com 偶发抖动 | `release.yml` 已加 3 次 retry + 8min 硬上限,继续重跑;持续问题用 `gh run rerun` |
| macOS 公证 fail | API key 过期或 Bundle ID 不在 App Store Connect 注册 | App Store Connect → Users and Access → Keys 重新生成 |
| Windows .msi 安装失败 | WiX 模板缺 upgrade code | `tauri.conf.json` 加 `bundle.windows.wix.upgradeCode` |
