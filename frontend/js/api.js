(function () {
  'use strict';

  const BASE = '';

  async function api(method, path, body) {
    const opts = { method, headers: { 'X-CAS-Request': '1' } };
    if (body !== undefined) {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }
    const resp = await fetch(BASE + path, opts);
    const data = await resp.json();
    if (!resp.ok || data.success === false) {
      // baseMessage 直接用 backend message(可能是 i18n key 如 "models.fetchFailed",
      // 上层负责翻译;也可能是 raw string)。**不在这里 inline errors[0]**:
      // backend 现在返结构化 errors[] (object 数组,含 code/host/statusCode),
      // string 拼接会变 "[object Object]"。让上层(如 formatModelFetchError)
      // 按 i18n 翻译每条 error 后再拼。
      const baseMessage = data.message || `Request failed: ${method} ${path}`;
      const error = new Error(baseMessage);
      error.errors = Array.isArray(data.errors) ? data.errors : [];
      error.responseData = data;
      throw error;
    }
    return data;
  }

  // ── 工具 ──
  const ICON_MAP = {
    deepseek: { logo: 'assets/providers/deepseek.ico' },
    kimi: { logo: 'assets/providers/kimi.ico' },
    moonshot: { logo: 'assets/providers/kimi.ico' },
    xiaomi: { logo: 'assets/providers/xiaomi-mimo.png' },
    mimo: { logo: 'assets/providers/xiaomi-mimo.png' },
    qiniu: { logo: 'assets/providers/qiniu.ico' },
    qnaigc: { logo: 'assets/providers/qiniu.ico' },
    zhipu: { logo: 'assets/providers/zhipu.png' },
    bigmodel: { logo: 'assets/providers/zhipu.png' },
    glm: { logo: 'assets/providers/zhipu.png' },
    siliconflow: { icon: 'bi-diagram-3-fill' },
    anyrouter: { icon: 'bi-shuffle' },
    bailian: { logo: 'assets/providers/aliyun.ico' },
    dashscope: { logo: 'assets/providers/aliyun.ico' },
    aliyun: { logo: 'assets/providers/aliyun.ico' },
    minimax: { logo: 'assets/providers/minimax.ico' },
    minimaxi: { logo: 'assets/providers/minimax.ico' },
    // Gemini CLI(OAuth)provider — 用 Gemini 品牌四角星 spark 图标(brand mark
    // 跟 gemini.google.com 一致)。**必须放在 'gemini' 通用规则前**(JS object
    // iteration 顺序 = insertion 顺序),computeIcon 子串匹配 'gemini-cli' 命中
    // 这条 才能优先于下面的 'gemini' 走到 google-ai-studio.png。
    // 不加 'cloudcode' 子串(silent-failure-hunter C2 修):太宽会误命中任何含
    // cloudcode 的 provider 名 / baseUrl 包括用户自定义 — 仅 'gemini-cli' id
    // stable 匹配
    'gemini-cli': { logo: 'assets/providers/gemini.svg' },
    // Antigravity OAuth provider — Google Antigravity IDE 同样走 cloudcode-pa
    // 但 OAuth client_id / brand 不同,用专属箭头 mark 区分(避免跟 gemini 共用
    // 图标让用户分不清两个 OAuth provider)。`antigravity-oauth` 子串命中 preset
    // id;不加更宽的 'antigravity' 防误命中用户自定义 provider 名
    'antigravity-oauth': { logo: 'assets/providers/antigravity.png' },
    // Google AI Studio:用官方品牌图标(从 aistudio.google.com 抓的
    // ai_studio_favicon_2_128x128.png,圆形黑底带 sparkle/方框 mark)。
    // 子串命中 "google" / "gemini" / "aistudio" / "generativelanguage" 任一都映射到此。
    google: { logo: 'assets/providers/google-ai-studio.png' },
    gemini: { logo: 'assets/providers/google-ai-studio.png' },
    aistudio: { logo: 'assets/providers/google-ai-studio.png' },
    generativelanguage: { logo: 'assets/providers/google-ai-studio.png' },
    // Grok Web 反代 provider — 用 grok.com 官方 favicon SVG(从
    // https://grok.com/images/favicon.svg 抓的,黑色圆角方块 + 白色 grok mark)。
    // `grok-web` 子串命中 preset id / apiFormat;不加更宽的 `grok` 防误命中用户
    // 自定义 provider 名含 "grok" 但实际不是 web 反代场景
    'grok-web': { logo: 'assets/providers/grok.svg' },
  };

  function buildCustomThirdPartyPreset() {
    const i18n = window.CCI18n;
    const tr = (k, fallback) => (i18n && i18n.t(k)) || fallback;
    return {
      id: 'custom-third-party',
      name: tr('providersAdd.customThirdPartyName', '自定义第三方'),
      baseUrl: '',
      apiFormat: 'OpenAI',
      authScheme: 'bearer',
      models: {},
      modelOptions: {},
      baseUrlOptions: [],
      baseUrlHint: tr('providersAdd.customThirdPartyHint', ''),
      requestOptionPresets: {},
      extraHeaders: {},
      modelCapabilities: {},
      requestOptions: {},
      icon: 'bi-puzzle',
      allowApiFormatSelection: true,
    };
  }

  // Claude Desktop provider/preset 注入 logo/icon 字段(对齐 cas Codex
  // provider 的 iconMarkup 期望 — item.logo / item.icon 直接读)。
  function claudeDesktopWithIcon(p) {
    if (!p) return p;
    const inferred = computeIcon(p);
    return { ...p, logo: p.logo || inferred.logo, icon: p.icon || inferred.icon };
  }

  function computeIcon(provider) {
    // **包含 apiFormat** 让 user 自加的 OAuth provider(name 自填,id UUID)也
    // 能命中专属图标 — 否则会 fall through 到 baseUrl 的 'google' 子串撞
    // google-ai-studio.png(2026-05-11 修)。
    // **normalize**:把 lookup string 里所有 `_` / 空格 全部转 `-`,这样:
    //   - apiFormat="antigravity_oauth" 命中 ICON_MAP key 'antigravity-oauth'
    //   - name="Gemini CLI"(空格) 命中 'gemini-cli'(dash)
    // 用单一 dash 形态做规范化(ICON_MAP key 全是 dash 形)
    const raw = `${provider.id || ''} ${provider.name || ''} ${provider.baseUrl || ''} ${provider.apiFormat || ''}`.toLowerCase();
    const lookup = raw.replace(/[_\s]+/g, '-');
    for (const [key, val] of Object.entries(ICON_MAP)) {
      if (lookup.includes(key)) return val;
    }
    return { icon: 'bi-plug-fill' };
  }

  function mapProvider(provider, activeId) {
    const models = provider.models || {};
    return {
      id: provider.id,
      name: provider.name,
      baseUrl: provider.baseUrl,
      apiFormat: ['openai', 'openai_chat'].includes(provider.apiFormat) ? 'openai_chat' : (provider.apiFormat || 'openai_chat'),
      authScheme: provider.authScheme || 'bearer',
      hasApiKey: !!provider.hasApiKey,
      extraHeaders: provider.extraHeaders || {},
      modelCapabilities: provider.modelCapabilities || {},
      requestOptions: provider.requestOptions || {},
      default: provider.id === activeId,
      isBuiltin: !!provider.isBuiltin,
      mappings: {
        default: models.default || '',
        gpt_5_5: models.gpt_5_5 || '',
        gpt_5_4: models.gpt_5_4 || '',
        gpt_5_4_mini: models.gpt_5_4_mini || '',
        gpt_5_3_codex: models.gpt_5_3_codex || '',
        gpt_5_2: models.gpt_5_2 || '',
      },
      ...computeIcon(provider),
    };
  }

  function providerBody(payload, includeModels = true) {
    const body = {
      name: payload.name,
      baseUrl: payload.baseUrl,
      authScheme: payload.authScheme || 'bearer',
      // 未知值 / 缺失 → "openai_chat" fallback(跟后端 normalize_provider_api_format 对齐)。
      // 历史 v1.x 这里 fallback 是 "responses",造成 MiMo / 老配置升级时绕过代理 → 404。
      // **修复历史(2026-05-10)**:旧实现把白名单外任何 apiFormat(包括新加的
      // `gemini_native`)强制改写成 `'openai_chat'` → backend 收到 openai_chat
      // 走 /chat/completions 探测 → Gemini native 端点不存在 → 404(用户截图反馈)。
      // 改成 passthrough 已知协议(responses/openai_chat/gemini_native/anthropic_messages
      // + 别名),让后端 normalize_provider_api_format 唯一负责协议规范化。
      apiFormat: (() => {
        const v = (payload.apiFormat || '').toLowerCase().replace(/-/g, '_');
        if (['responses', 'openai_responses'].includes(v)) return 'responses';
        if (['anthropic_messages', 'anthropic', 'claude', 'messages', 'claude_messages'].includes(v)) return 'anthropic_messages';
        if (['gemini_native', 'google_ai_studio', 'gemini'].includes(v)) return 'gemini_native';
        // Cloud Code Assist OAuth(impersonate gemini-cli)— passthrough,
        // 后端 normalize_provider_api_format 识别 + GeminiCliAdapter 路由。
        // 漏 passthrough 会被 fallback 'openai_chat',OAuth provider 退化成
        // 用 api_key+/chat/completions 探测 cloudcode-pa 必 404。2026-05-11 实测
        if (['gemini_cli_oauth', 'gemini_oauth', 'google_oauth_cloud_code'].includes(v)) return 'gemini_cli_oauth';
        // Antigravity OAuth(Google Antigravity IDE,跟 gemini-cli 共用 cloudcode-pa
        // 上游但不同 OAuth client_id + 独立 token 文件)— passthrough,后端
        // GeminiCliAdapter 按 apiFormat 别名分流到 antigravity-oauth.json token。
        // 不接受裸 'antigravity' alias —— 怕 legacy 配置 / 用户手填把别的 provider
        // (apiFormat 历史漂移值)误归 OAuth 路径(silent-failure I3 修)
        if (['antigravity_oauth', 'google_oauth_antigravity'].includes(v)) return 'antigravity_oauth';
        // Grok Web 反代(R1 Plan A):passthrough 让后端 normalize_provider_api_format
        // + grok_web adapter 路由。漏这条 → fallback 'openai_chat' → save 后 healing
        // 强改 grok_web 但 grokWeb 字段也没 passthrough → 进半残态(2026-05-12 user
        // 真机 E2E 报错 "需要 grokWeb.cookies.sso" 的根因)
        if (['grok_web', 'grok', 'grok_com'].includes(v)) return 'grok_web';
        return 'openai_chat';  // openai / openai_chat / chat_completions / 空 / 未知 → openai_chat
      })(),
      extraHeaders: payload.extraHeaders || {},
      modelCapabilities: payload.modelCapabilities || {},
      requestOptions: payload.requestOptions || {},
    };
    if (payload.apiKey) {
      body.apiKey = payload.apiKey;
    }
    if (includeModels) {
      body.models = payload.models || {};
    }
    // R1 Plan A:grokWeb extra(cookies + statsigId override + UA override)必须
    // passthrough 到 backend payload。**此前漏掉**(2026-05-12 user E2E 真机
    // 反馈):前端 providerPayloadFromForm 拼了 payload.grokWeb,但这个 helper
    // 一直没 forward → backend AddProviderInput.grok_web 永远是 None → P2 必填
    // check 命中报错。passthrough 后 backend 正常持久化到 provider.grokWeb
    if (payload.grokWeb) {
      body.grokWeb = payload.grokWeb;
    }
    return body;
  }

  function mapLog(log) {
    return {
      at: log.time,
      level: log.level.toLowerCase(),
      message: log.message,
    };
  }

  // ── 公开 API ──
  window.CCApi = {
    async getStatus() {
      const data = await api('GET', '/api/status');
      const active = data.activeProvider;
      return {
        desktopConfigured: !!data.desktopConfigured,
        proxyRunning: !!data.proxyRunning,
        proxyPort: data.proxyPort || 18080,
        activeProvider: active ? { name: active.name, id: active.id } : { name: '-', id: null },
        activeProviderId: data.activeProviderId,
        desktopHealth: data.desktopHealth || { needsApply: false, issues: [] },
        exposeAllProviderModels: !!data.exposeAllProviderModels,
      };
    },

    async getProviders() {
      const data = await api('GET', '/api/providers');
      return (data.providers || []).map(p => mapProvider(p, data.activeId));
    },

    async getProviderSecret(id) {
      return api('GET', `/api/providers/${encodeURIComponent(id)}/secret`);
    },

    async getPresets() {
      const data = await api('GET', '/api/presets');
      const builtin = (data.presets || []).map(p => ({
        id: p.id,
        name: p.name,
        baseUrl: p.baseUrl,
        // 直接 passthrough 后端原值,让前端 normalizeApiFormat 唯一负责协议规范化。
        // **修复历史(2026-05-10)**:之前这里 hardcode `?'Responses':'OpenAI'`,
        // 把任何不在白名单的 apiFormat(包括新加的 `gemini_native`)强制改写成
        // 字面量 `'OpenAI'`,导致 normalizeApiFormat 永远命中 default openai_chat
        // 分支,UI 显示协议名错误。passthrough + 让 normalizeApiFormat 处理是
        // 唯一正确做法(它已识别 openai_chat / responses / anthropic_messages / gemini_native
        // 各种子值,加新协议只需更新 normalizeApiFormat,不需要改这里)。
        apiFormat: p.apiFormat || 'openai_chat',
        authScheme: p.authScheme || 'bearer',
        models: p.models || {},
        modelOptions: p.modelOptions || {},
        baseUrlOptions: p.baseUrlOptions || [],
        baseUrlHint: p.baseUrlHint || '',
        requestOptionPresets: p.requestOptionPresets || {},
        extraHeaders: p.extraHeaders || {},
        modelCapabilities: p.modelCapabilities || {},
        requestOptions: p.requestOptions || {},
        // supportsWebSearch:preset 标记是否支持 web_search 配置开关(MiMo /
        // Kimi / Gemini 三家)。frontend form 据此决定是否渲染开关 UI。
        supportsWebSearch: !!p.supportsWebSearch,
        ...computeIcon(p),
      }));
      return [...builtin, buildCustomThirdPartyPreset()];
    },

    async addProvider(payload) {
      const data = await api('POST', '/api/providers', providerBody(payload));
      return data.provider || data;
    },

    async updateProvider(id, payload) {
      const data = await api('PUT', `/api/providers/${encodeURIComponent(id)}`, providerBody(payload));
      return data.provider || data;
    },

    async deleteProvider(id) {
      return api('DELETE', `/api/providers/${encodeURIComponent(id)}`);
    },

    async setDefaultProvider(id) {
      return api('PUT', `/api/providers/${encodeURIComponent(id)}/default`);
    },

    async saveDraft(id, payload) {
      return api('POST', `/api/providers/${encodeURIComponent(id)}/draft`, providerBody(payload, true));
    },

    async activateProvider(id) {
      return api('POST', `/api/providers/${encodeURIComponent(id)}/activate`);
    },

    async reorderProviders(providerIds) {
      return api('PUT', '/api/providers/reorder', { providerIds });
    },

    async testProvider(id) {
      return api('POST', `/api/providers/${encodeURIComponent(id)}/test`);
    },

    async queryProviderUsage(id) {
      return api('POST', `/api/providers/${encodeURIComponent(id)}/usage`);
    },

    async getProviderCompatibility() {
      return api('GET', '/api/providers/compatibility');
    },

    async testProviderPayload(payload) {
      return api('POST', '/api/providers/test', providerBody(payload, true));
    },

    async saveModelMappings(id, mappings) {
      return api('PUT', `/api/providers/${encodeURIComponent(id)}/models`, { models: mappings });
    },

    async fetchProviderModels(id) {
      return api('GET', `/api/providers/${encodeURIComponent(id)}/models/available`);
    },

    async fetchProviderModelsPayload(payload) {
      return api('POST', '/api/providers/models/available', providerBody(payload, false));
    },

    async autofillProviderModels(id) {
      return api('POST', `/api/providers/${encodeURIComponent(id)}/models/autofill`);
    },

    async getDesktopStatus() {
      const data = await api('GET', '/api/desktop/status');
      const status = await api('GET', '/api/status');
      const proxyPort = status.proxyPort || 18080;
      const registryConfig = data.keys || {};
      return {
        configured: !!data.configured,
        health: data.health || { needsApply: false, issues: [] },
        config: {
          inferenceProvider: registryConfig.inferenceProvider || 'gateway',
          inferenceGatewayBaseUrl: registryConfig.inferenceGatewayBaseUrl || `http://127.0.0.1:${proxyPort}`,
          inferenceGatewayApiKey: registryConfig.inferenceGatewayApiKey ? '******' : '',
          inferenceGatewayAuthScheme: registryConfig.inferenceGatewayAuthScheme || 'bearer',
          inferenceModels: registryConfig.inferenceModels || '[]',
        },
      };
    },

    async configureDesktop() {
      const result = await api('POST', '/api/desktop/configure');
      return result;
    },

    async clearDesktop() {
      return api('POST', '/api/desktop/clear');
    },

    async getDesktopSnapshots() {
      const data = await api('GET', '/api/desktop/snapshots');
      return data.snapshots || [];
    },

    async restoreDesktopSnapshot(snapshotId) {
      return api('POST', '/api/desktop/restore', {
        snapshotId,
        cleanupAll: true,
      });
    },

    // ─── Claude Desktop(Anthropic-compat 直连,跟 cas proxy 无关)──────────
    // 借鉴自 lonr-6/cc-desktop-switch (Apache-2 / MIT) backend/{config.py,
    // model_alias.py,registry.py:13-122};baseUrl 体系跟 Codex CLI 端独立。
    async getClaudeDesktopStatus() {
      return api('GET', '/api/claude-desktop/status');
    },
    async listClaudeDesktopPresets() {
      const data = await api('GET', '/api/claude-desktop/presets');
      return (data.presets || []).map(claudeDesktopWithIcon);
    },
    async listClaudeDesktopProviders() {
      const data = await api('GET', '/api/claude-desktop/providers');
      return {
        activeProvider: data.activeProvider || null,
        providers: (data.providers || []).map(claudeDesktopWithIcon),
        presets: (data.presets || []).map(claudeDesktopWithIcon),
      };
    },
    async addClaudeDesktopProvider(provider) {
      return api('POST', '/api/claude-desktop/providers', provider);
    },
    async updateClaudeDesktopProvider(id, provider) {
      return api('PUT', `/api/claude-desktop/providers/${encodeURIComponent(id)}`, provider);
    },
    async deleteClaudeDesktopProvider(id) {
      return api('DELETE', `/api/claude-desktop/providers/${encodeURIComponent(id)}`);
    },
    async setClaudeDesktopDefaultProvider(id) {
      return api('PUT', `/api/claude-desktop/providers/${encodeURIComponent(id)}/default`);
    },
    async applyClaudeDesktop({ providerId, exposeAllModels, gatewayBaseUrl } = {}) {
      return api('POST', '/api/claude-desktop/apply', {
        providerId: providerId || null,
        exposeAllModels: !!exposeAllModels,
        gatewayBaseUrl: gatewayBaseUrl || null,
      });
    },
    async clearClaudeDesktop() {
      return api('POST', '/api/claude-desktop/clear');
    },
    async testClaudeDesktopBaseUrl({ baseUrl, apiKey, authScheme, apiFormat }) {
      return api('POST', '/api/claude-desktop/test-baseurl', {
        baseUrl, apiKey: apiKey || '',
        authScheme: authScheme || 'bearer',
        apiFormat: apiFormat || 'anthropic',
      });
    },
    async fetchClaudeDesktopModels({ baseUrl, apiKey, authScheme, apiFormat }) {
      return api('POST', '/api/claude-desktop/fetch-models', {
        baseUrl, apiKey: apiKey || '',
        authScheme: authScheme || 'bearer',
        apiFormat: apiFormat || 'anthropic',
      });
    },

    async startProxy(port) {
      if (port) {
        await this.saveSettings({ proxyPort: Number(port) });
      }
      await api('POST', '/api/proxy/start', port ? { port: Number(port) } : undefined);
      const status = await api('GET', '/api/status');
      return {
        running: !!status.proxyRunning,
        port: status.proxyPort || port || 18080,
      };
    },

    async stopProxy() {
      await api('POST', '/api/proxy/stop');
      const status = await api('GET', '/api/status');
      return {
        running: !!status.proxyRunning,
        port: status.proxyPort || 18080,
      };
    },

    async getProxyLogs() {
      const data = await api('GET', '/api/proxy/logs');
      return (data.logs || []).map(mapLog);
    },

    async getProxyStatus() {
      const data = await api('GET', '/api/proxy/status');
      return {
        running: !!data.running,
        port: data.port || 18080,
        stats: data.stats || { total: 0, success: 0, failed: 0, today: 0 },
      };
    },

    async clearLogs() {
      return api('POST', '/api/proxy/logs/clear');
    },

    async openLogDir() {
      return api('POST', '/api/proxy/logs/open-dir');
    },

    // ── Claude Desktop 转发(端口 18099,跟 Codex CLI 18080 分桶) ──
    async getClaudeProxyStatus() {
      const data = await api('GET', '/api/claude-desktop/proxy/status');
      return {
        running: !!data.running,
        listening: !!data.listening,
        port: data.port || 18099,
        bind: data.bind || '127.0.0.1',
        activeProviderId: data.activeProviderId || null,
        gatewayKeyConfigured: !!data.gatewayKeyConfigured,
        stats: data.stats || { total: 0, success: 0, failed: 0, today: 0 },
      };
    },

    async startClaudeProxy() {
      return api('POST', '/api/claude-desktop/proxy/start');
    },

    async stopClaudeProxy() {
      return api('POST', '/api/claude-desktop/proxy/stop');
    },

    async getClaudeProxyLogs() {
      const data = await api('GET', '/api/claude-desktop/proxy/logs');
      return (data.logs || []).map(mapLog);
    },

    async clearClaudeProxyLogs() {
      return api('POST', '/api/claude-desktop/proxy/logs/clear');
    },

    async openClaudeProxyLogDir() {
      return api('POST', '/api/claude-desktop/proxy/logs/open-dir');
    },

    async getSettings() {
      return api('GET', '/api/settings');
    },

    async getVersion() {
      return api('GET', '/api/version');
    },

    async saveSettings(settings) {
      const data = await api('PUT', '/api/settings', settings);
      return data.settings || data;
    },

    async checkUpdate(updateUrl) {
      const params = new URLSearchParams();
      if (updateUrl) params.set('url', updateUrl);
      return api('GET', `/api/update/check?${params.toString()}`);
    },

    async installUpdate(updateUrl) {
      return api('POST', '/api/update/install', updateUrl ? { url: updateUrl } : {});
    },

    async createBackup() {
      return api('POST', '/api/config/backup');
    },

    async listBackups() {
      const data = await api('GET', '/api/config/backups');
      return data.backups || [];
    },

    async exportConfig() {
      return api('GET', '/api/config/export');
    },

    async importConfig(configData) {
      return api('POST', '/api/config/import', configData);
    },

    async getDesktopSnapshotStatus() {
      return api('GET', '/api/desktop/snapshot-status');
    },

    async restartCodexApp() {
      return api('POST', '/api/desktop/restart-codex-app');
    },

    async submitFeedback(payload) {
      // 走 JSON 而不是 multipart/form-data —— pywebview 的 WebKit 对
      // fetch+FormData 组合存在 "the string did not match the expected pattern"
      // bug,JSON 路径稳定。文件以 base64 嵌入。
      return api('POST', '/api/feedback', payload);
    },

    async getActivities() {
      const data = await api('GET', '/api/proxy/logs');
      const logs = data.logs || [];
      return logs.slice(-5).reverse().map(log => ({
        time: log.time,
        text: log.message,
      }));
    },

    // ── Gemini CLI OAuth (P2.2) ──────────────────────────────────────────
    // 后端 admin handler 在 src-tauri/src/admin/handlers/gemini_oauth.rs。
    // login 是 long-poll 5min(浏览器登录 callback timeout),前端按钮要 disable
    // 直到 promise resolve;status / logout 是即时操作。

    async getGeminiOauthStatus() {
      return api('GET', '/api/gemini-oauth/status');
    },

    async loginGeminiOauth() {
      // **long polling** — fetch 会阻塞最长 5min 等待 OAuth callback
      return api('POST', '/api/gemini-oauth/login', {});
    },

    async logoutGeminiOauth() {
      return api('DELETE', '/api/gemini-oauth/logout');
    },

    // ── Antigravity OAuth ────────────────────────────────────────────────
    // 后端 admin handler 在 src-tauri/src/admin/handlers/antigravity_oauth.rs。
    // 跟 gemini-cli 完全 parallel:独立 cancel slot / done channel / token 文件,
    // 用户可同时登录两个 provider。endpoint shape 同 gemini-cli(login long-poll +
    // status/logout 即时操作)。
    async getAntigravityOauthStatus() {
      return api('GET', '/api/antigravity-oauth/status');
    },

    async loginAntigravityOauth() {
      // **long polling** — fetch 会阻塞最长 5min 等待 OAuth callback
      return api('POST', '/api/antigravity-oauth/login', {});
    },

    async logoutAntigravityOauth() {
      return api('DELETE', '/api/antigravity-oauth/logout');
    },

    /// 拉 antigravity 上游可用 model 列表(`:fetchAvailableModels`),后端
    /// 失败时退到静态种子。响应 OpenAI `/v1/models` shape:
    ///   `{ object: "list", data: [{id, object, owned_by, ...}], source: "upstream"|"static_seed" }`
    /// 跟 gemini-cli 不同 — gemini-cli 没 fetchAvailableModels endpoint,前端那边
    /// 是 hardcoded list;antigravity 真有 endpoint(CLIProxyAPI 实证)
    async getAntigravityOauthModels() {
      return api('GET', '/api/antigravity-oauth/models');
    },
  };
})();

// ── Codex Desktop Plugins 解锁 API ──

window.CCAPI = window.CCAPI || {};

window.CCAPI.pluginUnlock = {
  /** 查询解锁状态 */
  async status() {
    return api('GET', '/api/desktop/plugin-unlock/status');
  },

  /** 启动解锁服务 */
  async start() {
    return api('POST', '/api/desktop/plugin-unlock/start');
  },

  /** 停止解锁服务 */
  async stop() {
    return api('POST', '/api/desktop/plugin-unlock/stop');
  },

  /** 手动触发重新注入 */
  async reinject() {
    return api('POST', '/api/desktop/plugin-unlock/reinject');
  },
};
