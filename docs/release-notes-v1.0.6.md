# CC Desktop Switch v1.0.6

本次版本聚焦 Claude 桌面版切换后的稳定性、Kimi 错误处理，以及已确认厂商的默认适配。

## 主要变化

- 点击 Provider “启用”后，会弹出重启 Claude 桌面版提醒；用户点击“我已知晓”后才关闭，可选择“不再提醒”。
- 修复 Kimi 等上游认证失败时被代理包装成 HTTP 200 的问题，避免 Claude 桌面版误按成功响应解析并出现 `$.input_tokens` 报错。
- Kimi 默认预设更新为 `https://api.moonshot.ai/anthropic`。
- 内置预设收敛为 DeepSeek、Kimi、智谱 GLM、阿里云百炼；七牛云和硅基流动不再默认展示，仍可手动添加。
- 阿里云百炼增加“开启千问 1M 上下文”勾选项。默认不启用，勾选后才把 `qwen3.6-plus` / `qwen3.6-flash` 的 `supports1m` 写入 Claude 桌面版配置。
- 增加阿里云百炼图标。
- Provider 测速改为在 `HEAD/GET` 不可用时发起极小真实请求，能更准确发现 API Key 或地址不匹配问题。

## 验证

- `python -m compileall -q backend main.py tests`
- `node --check frontend/js/api.js`
- `node --check frontend/js/app.js`
- `node --check frontend/js/i18n.js`
- `python -m unittest discover -s tests -v`

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.6-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.6-Windows-Portable.zip`。
