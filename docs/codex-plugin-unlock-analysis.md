# Codex Desktop Plugins 解锁方案分析

## 版本信息
- Codex Desktop: 26.506.31421
- 分析日期: 2026-05-14

## 核心发现

### 1. 路由层面
- `/plugins` 和 `/skills` 路由都指向同一个 `SkillsPage` 组件
- 路由守卫 `XS` 只检查 `authMethod` 是否存在（apikey 模式下存在，值为 `'apikey'`）
- **结论：/plugins 路由本身不阻止 apiKey 用户访问**

### 2. Sidebar 层面（问题根源）
- `gy` 函数中：`u = Xc(c) = c !== 'chatgpt'`（c 是 authMethod）
- 当 `authMethod !== 'chatgpt'` 时，显示 disabled 的 Plugins 选项卡
- **这是纯 UI 层面的判断，不影响实际路由访问**

### 3. Auth 状态推导
```javascript
function E(e){  // account type → openAIAuth
  if(e==null)return null;
  switch(e.type){
    case `apiKey`: return `apikey`;
    case `amazonBedrock`: return null;
    case `chatgpt`: return `chatgpt`;
  }
}

function D(e,t){  // 构建 authState
  let n=E(e.account),
      r=t.useCopilotAuthIfAvailable&&t.isCopilotApiAvailable?`copilot`
        :e.account?.type===`amazonBedrock`?`amazonBedrock`:n;
  return {
    openAIAuth:n,
    authMethod:r,
    requiresAuth:r===`copilot`||(e.requiresOpenaiAuth??!0),
    email:e.account?.type===`chatgpt`?e.account.email:null,
    planAtLogin:e.account?.type===`chatgpt`?e.account.planType:null
  }
}
```

### 4. 关键突破：setAuthMethod
- `setAuthMethod` 的实现：`u(t => ({...t ?? T(), authMethod: e}))`
- **这直接修改 React state 中的 authMethod，不修改底层 account 数据**
- 调用 `setAuthMethod('chatgpt')` 后，sidebar 会显示正常 Plugins 选项卡

### 5. Account 获取机制
- 渲染进程通过 `sendRequest('account/read', {refreshToken:!1})` 获取 account
- 使用 `AppServerRequestClient` 管理请求/响应 Promise
- 响应通过 Electron IPC 返回

## 方案对比

| 方案 | 侵入性 | 持久性 | 抗更新 | 复杂度 |
|------|--------|--------|--------|--------|
| 直接 URL 访问 | 无 | N/A | 高 | 低 |
| DevTools 注入 | 低 | 刷新丢失 | 高 | 中 |
| CDP 自动化启动器 | 中 | 每次启动 | 高 | 中 |
| app.asar Patch | 高 | 持久 | 低（需重patch）| 高 |
