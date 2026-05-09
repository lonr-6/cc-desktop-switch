# UI/UX Plan for Rust Mainline

## Direction

保持旧布局和旧使用路径，但让状态更清楚、失败更可解释、普通用户更少看到高级细节。

普通用户只需要理解：

```text
填 API Key -> 检查配置 -> 一键应用 -> 重启 Claude Desktop
```

## Main Navigation

新版只保留四个主要区域：

- 首页
- Provider
- 诊断/高级
- 设置

## 首页

首页只放高频信息：

- 当前 Provider
- Claude Desktop 状态
- Gateway 状态
- 一键应用
- 报告问题

不再把“导入 CC-Switch 配置”长期放在首页主按钮。它进入 Provider 导入入口。

## Provider

Provider 页面负责：

- 添加/编辑 Provider
- API Key
- API format
- 模型映射
- 1M / Max 能力
- 检查配置

把这些按钮合并为一个主操作：

```text
检查配置
```

内部执行：

- 识别协议
- 测试 Base URL
- 获取模型
- 验证映射模型
- 检查 1M / Max 能力
- 启动或探测本机 gateway
- 执行一个最小真实请求 smoke test

`检查配置` 不能只做静态检查。结果必须分层显示，避免“假绿”：

- 静态配置是否有效；
- Provider 鉴权是否通过；
- 模型列表是否可取；
- Gateway 是否能启动；
- Claude Desktop 写入读回是否一致；
- 最小真实请求是否成功。

## 诊断/高级

诊断页负责：

- 报告问题
- 复制诊断摘要
- 导出诊断包
- 打开 GitHub Issue
- Gateway 状态
- Gateway 日志
- 更新安装日志
- 高级调试开关
- 分层 readiness 状态，区分 static check、readback、provider smoke、gateway smoke

普通用户不直接看到复杂技术日志，除非进入诊断/高级。

## 设置

设置页负责：

- 语言
- 主题
- Gateway 端口
- 更新检查
- 高级开关

直连模式如果保留，只能出现在高级开关，并标注“调试用，不是普通路径”。

## Model Menu

默认模式：

- 只显示当前默认 Provider 的显式映射模型。
- `Default` 不显示。
- 未配置槽位不显示。
- 1M 只在对应 route 支持时显示。

高级模式：

- 可以显示多个 Provider 的模型。
- 必须明确显示 Provider 名称。
- 必须在诊断中能看到 route -> provider -> upstream model。

禁止：

- 显示全部 Provider 原始模型。
- 显示未映射模型。
- 显示原始第三方模型名给 Claude Desktop。

## Status Copy

错误文案必须告诉用户：

- 哪一步失败；
- 期望值是什么；
- 实际值是什么；
- 用户下一步能做什么。

示例：

```text
Claude Desktop 配置读回不一致。
Expected: http://127.0.0.1:18080
Actual: https://api.deepseek.com/anthropic
请点击“一键应用”重新写入，并完整重启 Claude Desktop。
```

## Visual Style

- 工具型、清晰、紧凑。
- 不做营销首页。
- 不使用大面积装饰图或渐变背景。
- 保留浅色/深色模式。
- 控件尺寸稳定，避免按钮文字导致布局跳动。
- 错误、警告、成功状态用一致色彩和 icon。

## Pure Rust UI Rule

- UI 组件使用 Rust/Leptos。
- 状态使用 Rust signal/store。
- Tauri command client 用 Rust 类型封装。
- CSS 可保留为样式资产，但不承载业务逻辑。
- 不新增手写 JavaScript 业务层。
