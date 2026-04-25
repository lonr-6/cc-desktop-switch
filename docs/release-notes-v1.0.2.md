# CC Desktop Switch v1.0.2

这版主要把工具从“网页管理后台感”继续收口成更接近桌面应用的使用方式，并修正了内置提供商的 Anthropic 兼容地址。

## 主要变化

- 添加/编辑提供商页面整合模型映射，不再要求用户单独进入“模型映射”模块。
- 主流程只展示 Anthropic 兼容接口，减少 OpenAI / Anthropic 格式选择带来的误解。
- 内置预设更新为 DeepSeek、Kimi、七牛云、智谱、SiliconFlow、阿里云百炼的 Anthropic 兼容地址。
- DeepSeek 增加“解锁 1M 上下文”模型选项。
- “一键应用到 Claude 桌面版”会保存提供商、保存模型映射、设为默认、配置 Claude 桌面版并启动转发服务。
- README 和使用说明同步为当前桌面应用流程。

## 下载建议

- 普通用户优先下载 `CC-Desktop-Switch-v1.0.2-Windows-Setup.exe`。
- 不想安装可以下载 `CC-Desktop-Switch-v1.0.2-Windows-Portable.zip`。

## 已知边界

- Windows 版暂未做 Authenticode 代码签名，系统可能提示未知发布者。
- Release 资产提供 `.sha256` 和 `.sig`，用于校验文件完整性，但它们不能替代 Windows 代码签名证书。
- 第一次测试建议使用低额度或可随时删除的 API Key。
