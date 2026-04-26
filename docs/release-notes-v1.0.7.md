# CC Desktop Switch v1.0.7

本次版本修复 `latest.json` 发布编码问题，并保留 v1.0.6 的厂商适配改进。

## 主要变化

- 修复发布脚本生成 `latest.json` 时带 UTF-8 BOM 的问题，避免严格 JSON 解析失败。
- 更新检查客户端兼容带 BOM 的历史 `latest.json`，提升旧版本更新稳定性。
- 继承 v1.0.6 改进：Provider 启用后重启提醒、Kimi 上游错误状态透传、阿里云百炼 1M 上下文改为勾选开启、内置预设收敛为 DeepSeek / Kimi / 智谱 GLM / 阿里云百炼。

## 验证

- `python -m compileall -q backend main.py tests`
- `node --check frontend/js/api.js`
- `node --check frontend/js/app.js`
- `node --check frontend/js/i18n.js`
- `python -m unittest discover -s tests -v`

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.7-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.7-Windows-Portable.zip`。
