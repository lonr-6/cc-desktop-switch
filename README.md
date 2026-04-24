# CC Desktop Switch

超轻量桌面工具，用浏览器界面管理第三方 API 提供商，并让 Claude Desktop 通过本地代理进入 3P 模式。

## 功能

- 管理 DeepSeek、Kimi、七牛云、智谱等 API 提供商。
- 通过本地代理完成 Claude 模型名到上游模型名的映射。
- 支持 Anthropic 和 OpenAI 两类上游格式。
- 支持 SSE 流式转发。
- 支持 Windows 注册表和 macOS plist 写入 Claude Desktop 3P 配置。
- 提供中文/英文界面和浅色/深色模式。

## 许可证

本项目使用 MIT License，许可证全文见 `LICENSE.txt`。MIT 许可证允许使用、复制、修改、发布和再分发代码，但需要保留原始版权声明和许可证文本。

注意：MIT License 是开源授权协议，不是 Windows 代码签名证书。它能说明代码如何被合法使用，但不能让 Windows 显示“已验证发布者”，也不能降低 SmartScreen 对未知发布者 EXE 的拦截概率。

## 致谢

本项目的产品方向和发布形态参考了 CC-Switch 这类社区工具的思路：用轻量桌面管理界面降低 Claude Desktop / Claude Code 第三方 API 配置门槛。本项目不是 Anthropic 或 CC-Switch 官方项目，也不复用它们的商标、Logo 或发布身份。

## 技术栈

- 后端：Python、FastAPI、httpx、uvicorn
- 前端：HTML、CSS、Vanilla JavaScript、Bootstrap 5.3 CDN
- 配置存储：`~/.cc-desktop-switch/config.json`
- 打包：PyInstaller，NSIS 可选

## 本地启动

如果你只是想使用软件，先看 [使用说明](docs/USAGE.md)。下面内容主要面向开发、测试和发布。

```powershell
cd "D:\cc desktop swtich"
pip install -r requirements.txt
python main.py
```

启动后打开：

```text
http://127.0.0.1:18081
```

默认端口：

- 管理后台：`18081`
- 本地代理：`18080`

## 前端路由

前端是 hash SPA，共 8 个路由：

- `#dashboard` 仪表盘
- `#providers` 提供商列表
- `#providers/add` 添加提供商
- `#models` 模型映射
- `#desktop` Desktop 集成
- `#proxy` 代理控制台
- `#settings` 设置
- `#guide` 使用引导

## API 概览

所有接口都在同一个 FastAPI 应用中，静态前端挂载在 `/`，API 使用 `/api/` 前缀。

| 方法 | 路径 | 说明 |
| --- | --- | --- |
| GET | `/api/status` | 全局状态 |
| GET | `/api/providers` | 提供商列表 |
| POST | `/api/providers` | 添加提供商 |
| PUT | `/api/providers/{provider_id}` | 编辑提供商 |
| DELETE | `/api/providers/{provider_id}` | 删除提供商 |
| PUT | `/api/providers/{provider_id}/default` | 设为默认 |
| GET | `/api/providers/{provider_id}/models` | 获取模型映射 |
| PUT | `/api/providers/{provider_id}/models` | 保存模型映射 |
| GET | `/api/desktop/status` | Desktop 配置状态 |
| POST | `/api/desktop/configure` | 写入 Desktop 3P 配置 |
| POST | `/api/desktop/clear` | 清除本工具写入的 Desktop 配置 |
| GET | `/api/proxy/status` | 代理运行状态和统计 |
| POST | `/api/proxy/start` | 启动代理 |
| POST | `/api/proxy/stop` | 停止代理 |
| GET | `/api/proxy/logs` | 代理日志 |
| POST | `/api/proxy/logs/clear` | 清除代理日志 |
| GET | `/api/settings` | 获取设置 |
| PUT | `/api/settings` | 保存设置 |
| GET | `/api/update/check` | 根据 `latest.json` 检查更新，不自动安装 |
| GET | `/api/presets` | 内置预设 |

## 验证命令

```powershell
python -m compileall -q backend main.py
node --check frontend/js/api.js
node --check frontend/js/app.js
node --check frontend/js/i18n.js
```

API 快速检查：

```powershell
Invoke-RestMethod http://127.0.0.1:18081/api/status
Invoke-RestMethod http://127.0.0.1:18081/api/providers
Invoke-RestMethod http://127.0.0.1:18081/api/presets
Invoke-RestMethod http://127.0.0.1:18081/api/proxy/status
Invoke-RestMethod http://127.0.0.1:18081/api/desktop/status
```

## 打包

文件夹模式：

```powershell
python -m PyInstaller build.spec
```

单文件模式：

```powershell
$env:CCDS_ONEFILE = "1"
python -m PyInstaller build.spec
Remove-Item Env:\CCDS_ONEFILE
```

ZIP 和安装包可用 `build.bat` 菜单生成。NSIS 安装包需要先安装 NSIS，并确保 `makensis` 在 PATH 中。

生成发布资产、签名和 `latest.json`：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\New-Release.ps1 -Version 1.0.0 -Build -TryInstaller
```

发布脚本会输出到 `release/`，包含：

- `CC-Desktop-Switch-v1.0.0-Windows-Portable.zip`
- `CC-Desktop-Switch-v1.0.0-Windows-x64.exe`
- `CC-Desktop-Switch-v1.0.0-Windows-Setup.exe`（仅当 NSIS 可用）
- 每个资产对应的 `.sha256` 和 `.sig`
- `latest.json`
- `CC-Desktop-Switch-release-public.pem`

签名说明：

- `.sig` 是对文件字节做 RSA-SHA256 签名后的 Base64 文本。
- 私钥生成在本机 `.release-signing/release-private-key.pem`，已加入 `.gitignore`，不要提交或发布。
- 公钥在 `release/CC-Desktop-Switch-release-public.pem`，用于用户或更新器验证签名。当前公钥文件是 Windows PowerShell 兼容的 RSA CSP blob 文本格式。

验证签名：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\Test-ReleaseSignature.ps1 -File release\CC-Desktop-Switch-v1.0.0-Windows-x64.exe
```

### Windows 代码签名证书

`.sig` 是本项目自己的发布资产签名，用于验证下载文件没有被替换；它不等于 Windows 认可的代码签名。要让 Windows SmartScreen 和文件属性识别发布者，需要真实的 Windows 代码签名证书（PFX，含私钥），通常来自 DigiCert、Sectigo、GlobalSign 等 CA。

本地使用证书签名：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\New-Release.ps1 `
  -Version 1.0.0 `
  -Build `
  -TryInstaller `
  -CodeSign `
  -CodeSigningCertificatePath "C:\path\codesign.pfx" `
  -CodeSigningCertificatePassword "your-pfx-password"
```

CI 使用证书签名时，把 PFX 转成 Base64 后放到 GitHub Secrets：

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("C:\path\codesign.pfx")) | Set-Clipboard
```

需要配置的 Secrets：

- `WINDOWS_CODESIGN_PFX_BASE64`：PFX 文件的 Base64 内容。
- `WINDOWS_CODESIGN_PFX_PASSWORD`：PFX 密码。

没有配置证书时，发布流程仍会生成 ZIP、EXE、安装包、`.sha256`、`.sig` 和 `latest.json`，但不会做 Windows Authenticode 签名。

无 PFX 证书时，推荐发布策略是：

- 保持 MIT License 随仓库和便携包一起发布。
- 在 GitHub Release 上传 `.sha256` 和 `.sig`，让用户能验证文件完整性。
- 在 Release Notes 里明确说明 Windows 版暂未做 Authenticode 签名。
- 后续拿到真实证书后，再启用 `-CodeSign` 或 GitHub Secrets 自动签名。

### 自动更新协议

发布脚本会生成 `release/latest.json`。上传到 GitHub Release 后，推荐更新地址为：

```text
https://github.com/<owner>/<repo>/releases/latest/download/latest.json
```

应用内“设置 -> 更新地址”保存这个 URL 后，可以点击“检查更新”。当前协议只做检查，不自动下载安装。接口返回字段包括：

- `updateAvailable`：是否发现新版本。
- `currentVersion` / `latestVersion`：当前版本和最新版本。
- `assets`：可下载资产列表，包含文件名、URL、SHA256 和 `.sig` 文件名。
- `updateProtocol`：协议版本，目前为 `1`。

### GitHub Release 自动化

仓库内已经提供 `.github/workflows/release.yml`。触发方式：

```powershell
git tag v1.0.0
git push origin v1.0.0
```

也可以在 GitHub Actions 页面手动运行 `Release` workflow 并输入版本号。流水线会在 `windows-latest` 上构建 Windows 资产、生成安装包、验证 `.sig`，然后创建或覆盖上传 GitHub Release 资产。

当前目录还不是 Git 仓库。要真正启用这条流水线，需要先初始化或接入 GitHub 仓库，并把上述代码推送到 GitHub。

## 高风险操作

以下动作不要在未确认的情况下自动执行：

- 点击或调用 `/api/desktop/configure`：会写入 Windows 注册表或 macOS plist。
- 点击或调用 `/api/desktop/clear`：会清除本工具管理的 Desktop 配置项。
- 删除 `prompt-codex.md`、`image/`、`.playwright-mcp/`、截图或构建产物。
- 安装 NSIS、初始化 GitHub 仓库、发布 GitHub Release。

## 发布前检查清单

- 前端 8 个路由都能打开。
- 添加提供商、设为默认、保存模型映射、启动代理、查看日志可闭环。
- 使用测试 API Key 验证 `/v1/messages` 非流式和流式请求。
- 确认 `extraHeaders` 对 DeepSeek 等需要额外认证头的提供商生效。
- 文件夹模式打包后能启动管理后台并加载前端静态资源。
- 安装包模式只在 NSIS 已安装后验证。
