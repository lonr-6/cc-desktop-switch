(function () {
  'use strict';

  const BASE = '';

  async function api(method, path, body) {
    const opts = { method, headers: { 'X-CCDS-Request': '1' } };
    if (body !== undefined) {
      opts.headers['Content-Type'] = 'application/json';
      opts.body = JSON.stringify(body);
    }
    const resp = await fetch(BASE + path, opts);
    const data = await resp.json();
    if (!resp.ok || data.success === false) {
      throw new Error(data.message || `Request failed: ${method} ${path}`);
    }
    return data;
  }

  // ── 工具 ──
  const ICON_MAP = {
    deepseek: { logo: 'assets/providers/deepseek.ico' },
    kimi: { logo: 'assets/providers/kimi.ico' },
    moonshot: { logo: 'assets/providers/kimi.ico' },
    qiniu: { logo: 'assets/providers/qiniu.ico' },
    qnaigc: { logo: 'assets/providers/qiniu.ico' },
    zhipu: { logo: 'assets/providers/zhipu.png' },
    bigmodel: { logo: 'assets/providers/zhipu.png' },
    glm: { logo: 'assets/providers/zhipu.png' },
    siliconflow: { icon: 'bi-diagram-3-fill' },
  };

  function computeIcon(provider) {
    const id = `${provider.id || ''} ${provider.name || ''} ${provider.baseUrl || ''}`.toLowerCase();
    for (const [key, val] of Object.entries(ICON_MAP)) {
      if (id.includes(key)) return val;
    }
    return { icon: 'bi-plug-fill' };
  }

  function mapProvider(provider, activeId) {
    const models = provider.models || {};
    return {
      id: provider.id,
      name: provider.name,
      baseUrl: provider.baseUrl,
      apiFormat: provider.apiFormat || 'anthropic',
      authScheme: provider.authScheme || 'bearer',
      default: provider.id === activeId,
      isBuiltin: !!provider.isBuiltin,
      mappings: {
        sonnet: models.sonnet || '',
        haiku: models.haiku || '',
        opus: models.opus || '',
        default: models.default || models.sonnet || models.haiku || models.opus || '',
      },
      ...computeIcon(provider),
    };
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
      };
    },

    async getProviders() {
      const data = await api('GET', '/api/providers');
      return (data.providers || []).map(p => mapProvider(p, data.activeId));
    },

    async getPresets() {
      const data = await api('GET', '/api/presets');
      return (data.presets || []).map(p => ({
        id: p.id,
        name: p.name,
        baseUrl: p.baseUrl,
        apiFormat: p.apiFormat === 'openai' ? 'OpenAI' : 'Anthropic',
        authScheme: p.authScheme || 'bearer',
        models: p.models || {},
        extraHeaders: p.extraHeaders || {},
        ...computeIcon(p),
      }));
    },

    async addProvider(payload) {
      const data = await api('POST', '/api/providers', {
        name: payload.name,
        baseUrl: payload.baseUrl,
        apiKey: payload.apiKey,
        authScheme: payload.authScheme || 'bearer',
        apiFormat: payload.apiFormat === 'OpenAI' ? 'openai' : 'anthropic',
        models: {
          sonnet: payload.models?.sonnet || '',
          haiku: payload.models?.haiku || '',
          opus: payload.models?.opus || '',
          default: payload.models?.default || '',
        },
        extraHeaders: payload.extraHeaders || {},
      });
      return data.provider || data;
    },

    async updateProvider(id, payload) {
      const data = await api('PUT', `/api/providers/${encodeURIComponent(id)}`, {
        name: payload.name,
        baseUrl: payload.baseUrl,
        apiKey: payload.apiKey,
        authScheme: payload.authScheme || 'bearer',
        apiFormat: payload.apiFormat === 'OpenAI' ? 'openai' : 'anthropic',
        models: {
          sonnet: payload.models?.sonnet || '',
          haiku: payload.models?.haiku || '',
          opus: payload.models?.opus || '',
          default: payload.models?.default || '',
        },
        extraHeaders: payload.extraHeaders || {},
      });
      return data.provider || data;
    },

    async deleteProvider(id) {
      return api('DELETE', `/api/providers/${encodeURIComponent(id)}`);
    },

    async setDefaultProvider(id) {
      return api('PUT', `/api/providers/${encodeURIComponent(id)}/default`);
    },

    async testProvider(id) {
      return api('POST', `/api/providers/${encodeURIComponent(id)}/test`);
    },

    async testProviderPayload(payload) {
      return api('POST', '/api/providers/test', {
        name: payload.name,
        baseUrl: payload.baseUrl,
        apiKey: payload.apiKey,
        authScheme: payload.authScheme || 'bearer',
        apiFormat: payload.apiFormat === 'OpenAI' ? 'openai' : 'anthropic',
        extraHeaders: payload.extraHeaders || {},
      });
    },

    async saveModelMappings(id, mappings) {
      return api('PUT', `/api/providers/${encodeURIComponent(id)}/models`, { models: mappings });
    },

    async getDesktopStatus() {
      const data = await api('GET', '/api/desktop/status');
      const status = await api('GET', '/api/status');
      const proxyPort = status.proxyPort || 18080;
      const registryConfig = data.keys || {};
      return {
        configured: !!data.configured,
        config: {
          inferenceProvider: registryConfig.inferenceProvider || 'gateway',
          inferenceGatewayBaseUrl: registryConfig.inferenceGatewayBaseUrl || `http://127.0.0.1:${proxyPort}`,
          inferenceGatewayApiKey: registryConfig.inferenceGatewayApiKey || '******',
          inferenceGatewayAuthScheme: registryConfig.inferenceGatewayAuthScheme || 'bearer',
          inferenceModels: registryConfig.inferenceModels || '["sonnet","haiku","opus"]',
        },
      };
    },

    async configureDesktop() {
      await api('POST', '/api/desktop/configure');
      return this.getDesktopStatus();
    },

    async clearDesktop() {
      await api('POST', '/api/desktop/clear');
      return this.getDesktopStatus();
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
      return { running: false };
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

    async getSettings() {
      return api('GET', '/api/settings');
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

    async getActivities() {
      const data = await api('GET', '/api/proxy/logs');
      const logs = data.logs || [];
      return logs.slice(-5).reverse().map(log => ({
        time: log.time,
        text: log.message,
      }));
    },
  };
})();
