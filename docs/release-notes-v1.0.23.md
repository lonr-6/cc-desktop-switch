# CC Desktop Switch v1.0.23

<p align="center">
  <a href="#english">English</a> |
  <a href="#simplified-chinese">简体中文</a>
</p>

<a id="english"></a>

## English

This patch focuses on startup and update stability. It stays on the stable Python line and does not change the provider configuration flow.

### Highlights

- **Fixed Windows in-app update installer launch**
  - Fixed a Windows helper launch issue where the update package downloaded successfully, the app exited, but the installer did not appear.
  - The update helper now keeps the PowerShell window hidden without using the Windows detached-process mode that caused the installer launch to be skipped.
  - Added update helper logging under `%TEMP%\CC-Desktop-Switch\updates\update-helper.log` for future diagnostics.

- **Reduced startup white screen**
  - Replaced CDN Bootstrap, Bootstrap Icons, and Google Fonts startup dependencies with bundled local frontend assets.
  - This avoids the initial blank window caused by waiting on external CSS/font resources.

- **Improved macOS menu bar behavior**
  - The macOS app now hides the Dock icon and uses a native menu bar status item for showing or quitting CC Desktop Switch.
  - Added Intel macOS and Apple Silicon build coverage in the release workflow.

### Notes

- Local test manifests such as `latest-local.json` and `latest-local-next.json` are only for manual testing and are not release assets.
- If you previously set a local update URL for testing, switch it back to the default GitHub `latest.json` URL before normal use.

<a id="simplified-chinese"></a>

## 简体中文

这个小版本主要修复启动和自动更新稳定性问题。仍然基于当前 Python 稳定线，不改变 provider 配置流程。

### 主要变化

- **修复 Windows 应用内更新安装器不弹出**
  - 修复了“更新包已下载、应用已退出，但安装器没有弹出”的问题。
  - 更新 helper 现在仍然隐藏 PowerShell 窗口，但不再使用会导致安装器启动被跳过的 Windows detached-process 模式。
  - 新增 helper 日志：`%TEMP%\CC-Desktop-Switch\updates\update-helper.log`，后续排查会更直接。

- **减少启动白屏**
  - 将 Bootstrap、Bootstrap Icons、Google Fonts 等启动依赖改为本地内置资源。
  - 避免打开软件时因为等待外部 CSS / 字体资源而出现短暂白屏。

- **改进 macOS 菜单栏体验**
  - macOS 版本会隐藏 Dock 图标，并使用原生菜单栏入口显示或退出 CC Desktop Switch。
  - 发布流程继续覆盖 Apple Silicon 和 Intel macOS 构建。

### 说明

- `latest-local.json`、`latest-local-next.json` 这类文件只用于本地测试，不会作为正式 release 资产上传。
- 如果你之前为了测试手动设置了本地更新地址，正常使用前请改回默认的 GitHub `latest.json` 地址。
