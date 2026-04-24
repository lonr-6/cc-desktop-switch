# CC Desktop Switch 使用说明

这份文档面向第一次使用的用户。目标是：安装并启动本工具后，在浏览器里配置 API 提供商，让 Claude Desktop 通过本地代理使用第三方模型。

## 适用场景

- 你已经安装 Claude Desktop。
- 你有 DeepSeek、Kimi、七牛云或智谱等平台的 API Key。
- 你希望 Claude Desktop 请求本机 `127.0.0.1:18080`，再由本工具转发到真实上游 API。

## 快速开始

### 1. 启动 CC Desktop Switch

开发环境：

```powershell
cd "D:\cc desktop swtich"
pip install -r requirements.txt
python main.py
```

发布版：

1. 下载 Windows 安装包或便携 ZIP。
2. 运行 `CC-Desktop-Switch.exe`。
3. 浏览器打开 `http://127.0.0.1:18081`。

默认端口：

- 管理界面：`18081`
- 本地代理：`18080`

### 2. 添加 API 提供商

进入“添加提供商”页面，推荐先点一个预设：

- DeepSeek
- Kimi
- 七牛云 AI
- 智谱 GLM

然后填入你自己的 API Key，点击“保存”。

注意：API Key 只应保存在你自己的电脑上，不要截图、上传或发给别人。

### 3. 检查模型映射

进入“模型映射”页面，确认 Claude 模型别名已经映射到目标平台模型。

当前默认映射：

| 提供商 | Sonnet / Opus | Haiku |
| --- | --- | --- |
| DeepSeek | `deepseek-v4-pro` | `deepseek-v4-flash` |
| Kimi | `kimi-k2.6` | `kimi-k2.6` |
| 七牛云 AI | `qwen3-max-2026-01-23` | `deepseek/deepseek-v3.2-251201` |
| 智谱 GLM | `glm-5.1` | `glm-5-turbo` |

模型名称会随厂商更新。如果请求失败，先到厂商控制台或文档确认模型 ID 是否仍然有效。

### 4. 配置 Claude Desktop

进入“Desktop 集成”页面，点击“配置 Desktop”。

这个动作会写入本机 Claude Desktop 的 managed policy：

- Windows：`HKCU\SOFTWARE\Policies\Claude`
- macOS：`com.anthropic.claudefordesktop`

写入后重启 Claude Desktop。它会把第三方推理请求发到：

```text
http://127.0.0.1:18080
```

本工具会生成一个本地 gateway key 写给 Claude Desktop。这个 key 只用于 Claude Desktop 调用本机代理，不是你的上游厂商 API Key。

### 5. 启动代理

进入“代理控制台”，点击“启动”。

启动后可以查看日志和请求统计。如果 Claude Desktop 请求失败，先看这里的错误信息。

### 6. 在 Claude Desktop 中验证

1. 确认 CC Desktop Switch 代理处于运行状态。
2. 重启 Claude Desktop。
3. 发一条简单消息。
4. 回到“代理控制台”查看是否有请求日志。

## 常见问题

### 是否还需要手动 Enable Developer Mode？

正常情况下不需要。本工具直接写入 Claude Desktop 支持的 managed policy，等价于官方配置界面导出的注册表或配置文件。

但你仍然需要：

- 安装支持第三方推理配置的 Claude Desktop 版本。
- 配置后重启 Claude Desktop。
- 保持本工具代理运行。

### 为什么 Windows 提示未知发布者？

MIT License 只是开源协议，不是代码签名证书。如果没有真实 Windows Authenticode 证书，Windows 仍可能提示未知发布者。

发布包会提供 `.sha256` 和 `.sig` 文件，用于校验文件没有被替换。但这不能替代 Windows 代码签名。

### 可以直接把 API Key 写进 Claude Desktop 吗？

不建议。本工具的设计是：

```text
Claude Desktop -> 本地代理 -> 第三方 API
```

Claude Desktop 只拿到本地 gateway key；真正的上游 API Key 保存在 CC Desktop Switch 配置中。

### 如何恢复 Claude Desktop 配置？

在“Desktop 集成”页面点击“清除配置”。这会移除本工具管理的 Desktop 配置项。

操作后重启 Claude Desktop。

## 安全建议

- 不要把 `~/.cc-desktop-switch/config.json` 上传到 GitHub。
- 不要把真实 API Key 写进 issue、截图、日志或聊天记录。
- 第一次测试建议使用额度较低或可随时删除的 API Key。
- 如果怀疑 API Key 泄露，立即到厂商控制台删除并重新生成。
