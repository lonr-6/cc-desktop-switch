# UI/UX Plan for Rust Mainline

## Direction

保持当前 CC Desktop Switch 前端的桌面工具布局和使用路径，但让状态更清楚、失败更可解释、普通用户更少看到高级细节。

普通用户只需要理解：

```text
填 API Key -> 检查配置 -> 一键应用 -> 重启 Claude Desktop
```

## Main Navigation

Rust 主线默认跟随当前最新前端的顶层入口：

- 首页
- 提供商
- 转发
- 设置
- 引导

说明：`Claude 桌面版` 详情页可以由首页/清理/应用动作进入，不必单独放在居中顶层 tab。Provider 添加页属于提供商入口下的子页面。普通 UI 仍只走本机 gateway，不暴露直连 provider 路径。

## Legacy Visual Baseline

Rust 主线 UI 必须以当前 CC Desktop Switch 前端为视觉基线：

- 顶部白色 header：左侧反馈、导入 CC Switch 配置、清除桌面版配置；中间 5 个图标 tab；右侧设置、主题、添加按钮。
- 首页使用 provider switch-board/card 列表，不再使用 P71 的三张大状态卡作为默认基线。
- Provider 管理页显示已配置 Provider，并提供添加、选择、设为默认、编辑、删除等操作。
- 添加 Provider 页面采用“左侧添加/编辑表单，右侧快捷预设”的双栏结构。
- 添加表单包含 API Base URL 管理/测速入口、API Key 显示切换、Auth Scheme、红框第三方兼容接口、协议格式选择、模型映射、一键应用说明。
- 一键应用按钮使用当前前端的橙色主按钮视觉，并必须对应完整事务式 Apply 流程。
- 高级 import/export、backup、diagnostics 工具不能压过普通添加流程；默认首屏应优先服务“填 key -> 映射 -> 一键应用”。

P72 以后，UI 优化可以提升状态解释、响应式布局和错误可读性，但不能脱离当前前端的布局结构、密度、按钮层级和颜色 token。P71 的旧截图式大卡片布局不再作为最新 UI 基线。

## 首页

首页只放高频信息：

- 当前 Provider
- Provider switch cards
- 快捷预设
- 添加/刷新入口
- 必要的 Desktop warning

“报告问题 / 导入 CC Switch 配置 / 清除桌面版配置”位于 header 左侧，和当前前端保持一致。

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
