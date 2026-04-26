# CC Desktop Switch v1.0.3

这版主要修复 Claude 桌面版第三方推理的模型发现和兼容性问题，尤其是 DeepSeek 1M 上下文和 Kimi Anthropic 兼容响应。

## 主要变化

- DeepSeek “解锁 1M 上下文”会写入 Claude 桌面版 `inferenceModels`，并标记 `supports1m: true`。
- 本地 gateway 新增 `/v1/models`，Claude 桌面版可以读取当前提供商的真实模型 ID。
- 代理会保留 `deepseek-v4-pro[1m]` 这类精确模型名，不会再映射回普通模型。
- 编辑提供商时可以看到已保存的 API Key，默认仍以密码框展示，点眼睛可查看。
- 规范 Anthropic 兼容响应里的 `usage.input_tokens` 和 `usage.output_tokens`，降低 Kimi 等兼容接口在 Claude 桌面版里报错的概率。
- 自动化测试覆盖所有内置预设，确认模型能进入 Desktop 配置、`/v1/models` 和实际模型映射。

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.3-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.3-Windows-Portable.zip`。

## 升级提示

如果要让 DeepSeek 1M 配置生效，升级后需要在应用里重新点击“一键应用到 Claude 桌面版”，然后重启 Claude Desktop。

## 已知边界

- Windows 版暂未做 Authenticode 代码签名，系统可能提示未知发布者。
- Release 资产提供 `.sha256` 和 `.sig`，用于校验文件完整性，但它们不能替代 Windows 代码签名证书。
- Kimi、智谱等厂商是否稳定可用，取决于它们当前的 Anthropic 兼容接口返回是否完整。本工具会做常见字段补齐，但不能替代厂商服务端能力。
