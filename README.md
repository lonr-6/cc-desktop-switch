# CC Desktop Switch

CC Desktop Switch 是一个轻量桌面工具，用本地桌面界面管理第三方 API 提供商，并把 Claude Desktop 的第三方推理请求转发到 DeepSeek、Kimi、智谱、阿里云百炼等平台。
安装版和便携版默认会打开独立桌面窗口；浏览器地址只作为调试和备用入口。
点击窗口关闭按钮时，应用会缩小到系统托盘继续运行；需要完全退出时，请右键托盘图标选择“退出”。

项目当前主要面向 Windows。macOS 保留了 plist 配置入口；Linux 可以运行管理后台和代理，但 Claude Desktop 没有对应 GUI 版本。

## 下载

最新版本在 GitHub Release：

```text
https://github.com/lonr-6/cc-desktop-switch/releases/latest
```

推荐普通用户下载：

- `CC-Desktop-Switch-v1.0.9-Windows-Setup.exe`：安装版
- `CC-Desktop-Switch-v1.0.9-Windows-Portable.zip`：便携版

Windows 版目前还没有 Authenticode 代码签名证书，系统可能提示未知发布者。Release 页面提供了 `.sha256` 和 `.sig` 文件用于校验下载完整性。

## 能做什么

- 管理 DeepSeek、Kimi、智谱、阿里云百炼等 API 提供商。
- 一键写入 Claude 桌面版第三方推理配置。
- 启动本机转发服务，把 Claude 模型名映射到上游模型。
- 对提供商 API 地址做基础连通测速。
- 主流程使用 Anthropic 兼容接口；后端保留 OpenAI 转换兼容，用于旧配置或自定义接口。
- 支持 SSE 流式转发。
- 提供中文/英文界面和浅色/深色模式。

## 基本用法

1. 启动 CC Desktop Switch。
2. 在弹出的桌面窗口里操作。
3. 选择快捷预设，填写自己的 API Key，必要时调整模型映射。
4. 点击“一键应用到 Claude 桌面版”。
5. 重启 Claude Desktop 后测试。

更详细的步骤见 [使用说明](docs/USAGE.md) 和 [图文快速教程](docs/QUICK_START.md)。

如果桌面窗口无法打开，可以手动访问备用地址：

```text
http://127.0.0.1:18081
```

## 默认端口

- 管理界面：`18081`
- 本机转发服务：`18080`

## 本地开发

```powershell
git clone https://github.com/lonr-6/cc-desktop-switch.git
cd cc-desktop-switch
pip install -r requirements.txt
python main.py
```

默认会打开桌面窗口。调试时也可以用浏览器模式：

```powershell
python main.py --browser
```

## 验证

```powershell
python -m compileall -q backend main.py
python -m unittest discover -s tests -v
node --check frontend/js/api.js
node --check frontend/js/app.js
node --check frontend/js/i18n.js
```

## 技术栈

- 后端：Python, FastAPI, httpx, uvicorn
- 前端：HTML, CSS, Vanilla JavaScript, Bootstrap 5.3 CDN
- 存储：`~/.cc-desktop-switch/config.json`
- 打包：PyInstaller, NSIS

## 安全说明

- API Key 只保存在本机配置文件中，不要上传 `~/.cc-desktop-switch/config.json`。
- “一键应用到 Claude 桌面版”会写入 Claude Desktop 的本机 managed policy。
- Claude Desktop 使用本工具生成的本地 gateway key 调用代理；真正的上游 API Key 不直接写进 Claude Desktop。

## 致谢

本项目的方向参考了 CC-Switch 这类社区工具的思路：用更轻的桌面界面降低 Claude Desktop / Claude Code 第三方 API 配置门槛。本项目不是 Anthropic 或 CC-Switch 官方项目，也不复用它们的商标、Logo 或发布身份。

## 许可证

MIT License。完整文本见 [LICENSE.txt](LICENSE.txt)。
