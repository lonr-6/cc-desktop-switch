# CC Desktop Switch v1.0.8

本次版本修复 Kimi 两套 API Key 对应不同地址的问题，并补强托盘切换后的重启提醒。

## 主要变化

- Kimi（月之暗面）默认预设改为 `https://api.moonshot.cn/anthropic`，适配 Kimi Platform API Key。
- 新增 `Kimi Code` 预设，默认地址为 `https://api.kimi.com/coding`，模型为 `kimi-for-coding`。
- Kimi 鉴权失败时给出更具体的提示，说明 Kimi Platform Key 和 Kimi Code 会员 Key 应分别使用哪个地址。
- 从系统托盘切换 provider 后，会弹窗提醒用户重启 Claude 桌面版，避免模型列表和当前 provider 不一致。

## 隐私说明

- 本版本没有提交本机 `~/.cc-desktop-switch/config.json`、配置备份、`.env`、PFX、私钥或真实 API Key。
- API Key 仍只保存在用户本机配置文件中，不会写入 Release 资产。

## 验证

- `python -m compileall -q backend main.py tests`
- `node --check frontend/js/api.js`
- `node --check frontend/js/app.js`
- `node --check frontend/js/i18n.js`
- `python -m unittest discover -s tests -v`

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.8-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.8-Windows-Portable.zip`。
