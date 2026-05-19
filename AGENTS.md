# Agent 工作规范

> **项目身份提示**:本仓库是 `Cmochance/cc-desktop-switch`(fork 自 `lonr-6/cc-desktop-switch`)。v2 起以 Rust + Tauri 2 全量重写,**把姊妹仓库 `codex-app-transfer` 的 Codex CLI 网关与协议转换能力整合进 cc-desktop-switch**。
>
> 因此仓库内 **产品名 = CC Desktop Switch**,但仍能看到大量 `codex-app-transfer` / `codex_app_transfer_*` 字面 —— 这些是 Rust crate 名 / 配置目录 / Bundle ID / Gateway key 前缀等**内部 ID**,属脚手架历史遗留,不要把它们当作待清理的 typo 误改;改名会破坏现有用户配置与 app 自动更新链,需走专门的迁移 PR。详见 README 顶部"项目身份与沿革"一节。

1. **只有用户强调过的内容才记录入本文档;未经用户同意,不得自行添加其他内容。**

2. **上下文丢失时必须向用户确认，禁止猜测执行。**
   当对用户需求、任务范围或历史上下文产生任何不确定时，必须暂停执行并向用户询问确认，而不是自行推断后继续操作。

3. **`litellm/` 作为参考目录不得直接修改或移除。** 如需借鉴其中代码，先复制到 `tmp/` 中操作，完成后删除 `tmp/`。

4. **后续任务完成后自动处理 PR 流程。** 任务完成后先检查是否存在用户创建且仍处于 open 状态的 PR；如果有，则先等待该 PR 达到明确完成态（已合并或已关闭），并定期检查 CI、review 和 merge state，若出现失败检查、阻塞 review 或分支冲突则先处理阻塞；如果没有 open PR，则直接自动提交代码、创建 PR、review，并在检查和 review 门禁通过后合并。
