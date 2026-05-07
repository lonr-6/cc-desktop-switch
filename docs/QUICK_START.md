# CC Desktop Switch 图文快速教程

这份教程给第一次使用的人看。按下面 4 步走，不需要手动打开 Claude Desktop 的开发者模式。

## 1. 打开桌面应用

安装或解压后运行：

```text
CC-Desktop-Switch.exe
```

macOS 版可以使用 DMG 或 PKG。DMG 打开后把应用拖到“应用程序”；如果该位置已有旧版，Finder 会提示是否替换。PKG 会安装到 `/Applications/CC Desktop Switch.app`，再次安装或安装新版本时会替换该位置的旧应用。

打开后先看首页。红框里的区域是主要操作区：选择提供商、查看状态、添加新提供商。

![首页主要区域](tutorial/assets/01-dashboard-redbox.png)

## 2. 添加 API 提供商

点击右上角 `+`，进入添加页面。先点右侧快捷预设，比如 DeepSeek、Kimi、智谱或阿里云百炼。

红框里的预设会自动填入 API 地址和推荐模型。API Key 需要你自己填写。

![选择快捷预设](tutorial/assets/02-add-provider-redbox.png)

## 3. 确认模型映射

模型映射已经在添加页面下方。简单说，它负责把 Claude 的 Sonnet / Haiku / Opus 对应到厂商自己的模型名。

如果你选 DeepSeek，可以按需勾选“解锁 1M 上下文”。勾选后，Sonnet、Opus 和默认模型会使用 `deepseek-v4-pro[1m]`，Haiku 对应的 `deepseek-v4-flash` 也会写入 1M 能力声明。

如果你选阿里云百炼，可以按需勾选“开启千问 1M 上下文”。勾选后，会把 `qwen3.6-plus` / `qwen3.6-flash` 的 1M 能力写入 Claude 桌面版。

如果需要更深的推理，可以勾选“DeepSeek Max 思维”。Claude 界面可能仍显示 `High`，但本工具会按 DeepSeek Max 转发；不勾选则按默认配置运行。

![DeepSeek 1M 上下文](tutorial/assets/03-deepseek-1m-redbox.png)

## 4. 一键应用到 Claude 桌面版

填好 API Key 和模型映射后，点击红框里的“一键应用到 Claude 桌面版”。

这个按钮会保存配置、设为默认，并写入 Claude 桌面版连接信息。v1.0.18 起，DeepSeek、Kimi、智谱、阿里云百炼这类第三方 provider 默认走本机 gateway；应用成功并重启 Claude Desktop 后，需要保持本工具在后台运行。

Windows 版会写入 Claude Desktop 的本机策略配置。macOS 版会自动定位 Claude Desktop 当前生效的 3P 配置条目，并写入本机 gateway 地址、gateway key 和模型列表，不需要手动复制别人的配置文件名。真实上游 API 地址、API Key、认证方案和额外请求头仍保存在 CC Desktop Switch 本机配置里。

完成后重启 Claude Desktop，再正常发消息即可。

![一键应用](tutorial/assets/04-apply-redbox.png)

## 原理一句话

```text
Claude 桌面版 -> CC Desktop Switch 本机 gateway -> 你的 API 提供商
```

本工具负责把本机 gateway 地址、网关 Key 和模型列表写入 Claude 桌面版的本机配置；真实上游 API 地址和 API Key 仍保存在 CC Desktop Switch 本机配置里。OpenAI / new-api / 反代类接口属于实验适配，需要额外确认协议和工具调用兼容性。

## 注意

- 不要把真实 API Key 放进截图、issue 或评论。
- Windows 版暂时没有 Authenticode 代码签名，系统可能提示未知发布者。
- macOS 版目前不提供系统托盘菜单；关闭窗口可隐藏应用，但使用第三方 provider 时不要从系统菜单完全退出 CC Desktop Switch。
- 如果请求失败，先核对 API Key、余额、模型名和 Claude Desktop 是否完整重启，再查看“代理”页面里的本机 gateway 日志。
