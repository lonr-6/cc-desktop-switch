# CC Desktop Switch v1.0.4

这版主要补齐 DeepSeek Max 思维深度的配置和说明，并继续收口 Claude 桌面版第三方推理的稳定性。

## 主要变化

- DeepSeek 预设新增“DeepSeek Max 思维”选项。
- 勾选后，本地 gateway 会把请求转发为 DeepSeek 支持的 `output_config.effort=max`。
- DeepSeek Max 说明里补充了 Low、Medium、High 的通俗解释，避免被 Claude 界面里的 `High` 显示误导。
- 未勾选 DeepSeek Max 时，继续按 Claude 当前默认配置转发。
- 保持 Kimi、智谱、七牛云、SiliconFlow、阿里云百炼的默认兼容策略，不伪造不确定的 Max 能力。

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.4-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.4-Windows-Portable.zip`。

## 升级提示

升级后如需使用 DeepSeek Max，请进入 DeepSeek 编辑页勾选“DeepSeek Max 思维”并保存。Claude 界面可能仍显示 `High`，但本工具会按 DeepSeek Max 转发请求。

## 已知边界

- Windows 版暂未做 Authenticode 代码签名，系统可能提示未知发布者。
- Release 资产提供 `.sha256` 和 `.sig`，用于校验文件完整性，但它们不能替代 Windows 代码签名证书。
- Claude 桌面版当前公开的 3P 配置没有稳定的 `Max` UI 能力声明字段，因此本工具不强行修改用户级 Claude Code 配置。
