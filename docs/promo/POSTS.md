# 发布文案

这些文案面向公开平台。语气尽量真实，不夸大，不把它包装成官方工具。

项目链接：

```text
https://github.com/lonr-6/cc-desktop-switch
```

Release：

```text
https://github.com/lonr-6/cc-desktop-switch/releases/latest
```

配图：

- 小红书封面：`docs/promo/assets/xhs-01-cover.png`
- 小红书教程图：`docs/promo/assets/xhs-02-steps.png`
- X 配图：`docs/promo/assets/x-card.png`
- Linux.do 配图：`docs/promo/assets/linuxdo-cover.png`

## 小红书

### 标题备选

1. Claude Desktop 接入国产 API，少一点手工配置
2. 给 Claude Desktop 配 DeepSeek / Kimi，我做了个本地小工具
3. 不想手改 Claude Desktop 3P 配置？可以试试这个

### 正文

最近折腾 Claude Desktop 的第三方 API 配置，发现步骤其实不复杂，但对普通用户不太友好：要进开发者模式、填 URL 和 key、重启，还要处理模型名映射。

所以做了一个小工具：CC Desktop Switch。

它主要做三件事：

1. 管理 DeepSeek、Kimi、七牛云、智谱这些 API 提供商。
2. 在本机启动代理，把 Claude 模型名转成上游模型名。
3. 一键写入 Claude Desktop 支持的 3P managed policy。

不是破解，也不是绕过官方机制。它只是把官方支持的配置方式做成了一个本地界面。

目前 v1.0.0 已经发到 GitHub：

https://github.com/lonr-6/cc-desktop-switch

现在更适合愿意自己折腾的用户：

- Windows 优先。
- 需要你自己有 API Key。
- 安装包还没有 Windows 代码签名，所以系统可能提示未知发布者。
- 第一次建议用测试 key 跑通流程。

如果你只是想少点手工配置，可以试试。真实 API Key 不要截图，不要发到 issue，也不要贴到评论区。

### 标签

```text
#ClaudeDesktop #ClaudeCode #DeepSeek #Kimi #开源工具 #AI工具 #效率工具 #Windows软件
```

## X

### Single post

I released CC Desktop Switch v1.0.0.

It is a small local tool for configuring Claude Desktop with third-party API providers.

What it does:
- provider presets for DeepSeek, Kimi, Qiniu, Zhipu
- local proxy on 127.0.0.1
- model name mapping
- Anthropic/OpenAI-style upstream support
- Windows build + portable ZIP

It is not an official Anthropic tool, and it does not include a Windows Authenticode signature yet.

Repo:
https://github.com/lonr-6/cc-desktop-switch

### Thread

1/ I built a small tool called CC Desktop Switch.

It helps configure Claude Desktop for third-party API providers through a local proxy.

Repo:
https://github.com/lonr-6/cc-desktop-switch

2/ The flow is simple:

Claude Desktop -> 127.0.0.1:18080 -> DeepSeek / Kimi / Qiniu / Zhipu

The app manages providers, API keys, model mapping, and the local proxy.

3/ It writes the local managed policy used by Claude Desktop's third-party inference mode.

So this is not a bypass or a patch. It is a UI around the supported configuration path.

4/ v1.0.0 includes:

- Windows installer
- portable ZIP
- latest.json
- sha256 files
- detached .sig files

No Authenticode certificate yet, so Windows may still show an unknown publisher warning.

5/ It is MIT licensed.

Use a test API key first. Do not paste real keys into issues, screenshots, or comments.

## Linux.do

### 标题备选

1. 开源了一个 Claude Desktop 3P 配置小工具：CC Desktop Switch
2. CC Desktop Switch v1.0.0：本地代理方式接 DeepSeek / Kimi / 七牛云 / 智谱
3. 把 Claude Desktop 的第三方推理配置做成了一个 Windows 本地工具

### 正文

开源了一个小工具：CC Desktop Switch。

项目地址：

```text
https://github.com/lonr-6/cc-desktop-switch
```

Release：

```text
https://github.com/lonr-6/cc-desktop-switch/releases/latest
```

它解决的问题比较具体：Claude Desktop 的第三方推理配置能用，但手动流程对普通用户不太顺。这个工具把 provider 管理、本地代理、模型映射和 Desktop 配置写入放到一个浏览器界面里。

基本链路：

```text
Claude Desktop
  -> http://127.0.0.1:18080
  -> DeepSeek / Kimi / 七牛云 / 智谱
```

目前做了这些：

- FastAPI 管理后台。
- Vanilla JS + Bootstrap 前端。
- Provider 预设：DeepSeek、Kimi、七牛云、智谱。
- 支持 Anthropic 和 OpenAI 两类上游格式。
- Claude 模型名到上游模型名的映射。
- SSE 流式转发。
- Windows 注册表和 macOS plist 写入入口。
- GitHub Actions 自动构建 Windows 安装包和便携包。
- Release 里有 `latest.json`、`.sha256` 和 `.sig`。

几个边界也说清楚：

- 不是 Anthropic 官方工具。
- 不是 CC-Switch 官方项目，只是方向上参考了这类工具。
- Windows 安装包暂时没有 Authenticode 证书，可能会提示未知发布者。
- Linux 可以跑管理后台和代理，但 Claude Desktop 没有 Linux GUI 版本，所以主要还是 Windows 场景。
- 建议第一次用测试 API Key，别把真实 key 放进截图、issue 或评论。

如果你也在折腾 Claude Desktop 3P / 国产 API / 模型映射，可以帮忙试试。更希望收到的是具体报错、provider 字段差异、模型名更新这类反馈。
