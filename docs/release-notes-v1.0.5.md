# CC Desktop Switch v1.0.5

这一版修复 Provider 切换和应用内更新两个核心问题。

## 主要变化

- 在 Provider 页面点击“启用”后，会同步当前厂商的模型列表到 Claude 桌面版配置，并提示重启 Claude 桌面版。
- 切换到 Kimi、智谱、七牛云、硅基流动、百炼等厂商时，Claude 桌面版会读取对应厂商的模型名，不再残留上一个厂商的显示。
- 检查更新时不再复用陈旧缓存，只有当前版本确实落后时才显示更新提醒。
- 首页更新标签可以点击；设置页新增“下载并安装”按钮，可在应用内下载安装包并启动安装器。
- Windows 安装器会在安装新版前检测旧版本并先执行卸载流程。

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.5-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.5-Windows-Portable.zip`。

## 重要提示

- 切换 Provider 后请重启 Claude 桌面版，让主面板重新读取模型列表。
- 应用内更新会启动安装器；如果安装器提示文件正在使用，请先退出当前应用再继续。
- Windows 版目前还没有 Authenticode 代码签名证书，Release 资产提供 `.sha256` 和 `.sig` 用于校验完整性。
