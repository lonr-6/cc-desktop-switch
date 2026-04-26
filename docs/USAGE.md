# CC Desktop Switch 使用说明

这份文档面向第一次使用的用户。目标很简单：在 CC Desktop Switch 里选一个 API 提供商，填自己的 API Key，然后一键让 Claude 桌面版通过本工具转发请求。

## 适用场景

- 你已经安装 Claude Desktop。
- 你有 DeepSeek、Kimi、七牛云、智谱、SiliconFlow 或阿里云百炼等平台的 API Key。
- 你希望少手改配置，让 Claude 桌面版走本机的 CC Desktop Switch，再转发到真实 API。

## 快速开始

### 1. 启动 CC Desktop Switch

普通用户推荐下载 Release 里的安装版：

```text
CC-Desktop-Switch-v1.0.3-Windows-Setup.exe
```

也可以使用便携版：

```text
CC-Desktop-Switch-v1.0.3-Windows-Portable.zip
```

启动后会打开一个桌面窗口。浏览器地址只是备用入口：

```text
http://127.0.0.1:18081
```

默认端口：

- 管理界面：`18081`
- 本机转发服务：`18080`

### 2. 添加提供商

点击右上角 `+`，进入“添加提供商”页面。

推荐先点右侧快捷预设：

- DeepSeek
- Kimi（月之暗面）
- 七牛云 AI
- 智谱 GLM
- SiliconFlow（硅基流动）
- 阿里云百炼

选择预设后，API 地址和推荐模型会自动填好。你只需要填自己的 API Key。

注意：API Key 只保存在你自己的电脑上，不要截图、上传或发给别人。

### 3. 确认模型映射

“模型映射”已经放在添加/编辑页面下方，不需要再去单独页面。

简单理解：Claude 桌面版会说“我要 Sonnet / Haiku / Opus”，但国内厂商的模型名不一样，所以这里负责把名字对上。

当前默认映射：

| 提供商 | Sonnet / Opus | Haiku |
| --- | --- | --- |
| DeepSeek | `deepseek-v4-pro` | `deepseek-v4-flash` |
| Kimi | `kimi-k2.6` | `kimi-k2.6` |
| 七牛云 AI | `moonshotai/kimi-k2-thinking` | `moonshotai/kimi-k2-thinking` |
| 智谱 GLM | `glm-5.1` | `glm-4.7` |
| SiliconFlow | `Pro/moonshotai/Kimi-K2.5` | `Pro/moonshotai/Kimi-K2.5` |
| 阿里云百炼 | `qwen3.6-plus` | `qwen3.6-flash` |

如果厂商更新了模型名，可以点“自动获取模型”，或手动改成厂商控制台里显示的模型 ID。

DeepSeek 额外提供“解锁 1M 上下文”选项。勾选后，Sonnet、Opus 和默认模型会使用 `deepseek-v4-pro[1m]`。

### 4. 一键应用到 Claude 桌面版

确认 API Key 和模型映射后，点击：

```text
一键应用到 Claude 桌面版
```

这个按钮会做四件事：

1. 保存提供商。
2. 保存模型映射。
3. 把这个提供商设为默认。
4. 写入 Claude 桌面版需要的本机连接信息，并启动转发服务。

原理很直白：

```text
Claude 桌面版 -> CC Desktop Switch -> 你的 API 提供商
```

Claude 桌面版只连接到本机的 CC Desktop Switch。你的上游 API Key 不会直接写进 Claude 桌面版。

### 5. 重启 Claude Desktop

应用完成后，关闭并重新打开 Claude Desktop。

然后在 Claude Desktop 里发一条简单消息。如果失败，回到 CC Desktop Switch 的“代理”页面看日志。

## 常见问题

### 是否还需要手动 Enable Developer Mode？

正常情况下不需要。本工具会直接写入 Claude Desktop 支持的 managed policy。你只需要重启 Claude Desktop。

但前提是你安装的 Claude Desktop 版本支持第三方推理配置。

### 为什么 Windows 提示未知发布者？

MIT License 是开源协议，不是代码签名证书。如果没有真实 Windows Authenticode 证书，Windows 仍可能提示未知发布者。

Release 会提供 `.sha256` 和 `.sig` 文件，用来校验下载文件没有被替换。但这不能替代 Windows 代码签名。

### 为什么要通过 CC Desktop Switch 转一下？

因为 Claude 桌面版说的是 Claude 模型名，而不同 API 厂商的模型名和请求入口不一样。本工具负责保存 API Key、翻译模型名、转发请求。

### 如何恢复 Claude Desktop 配置？

当前版本主流程是“一键应用”。如果需要清除本工具写入的 Claude Desktop 配置，可以进入隐藏的 `#desktop` 页面或后续版本的设置入口执行清除。

清除后需要重启 Claude Desktop。

## 安全建议

- 不要把 `~/.cc-desktop-switch/config.json` 上传到 GitHub。
- 不要把真实 API Key 写进 issue、截图、日志或聊天记录。
- 第一次测试建议使用额度较低或可随时删除的 API Key。
- 如果怀疑 API Key 泄露，立即到厂商控制台删除并重新生成。
