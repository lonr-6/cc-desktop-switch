(function () {
  const routes = ["dashboard", "providers/add", "providers", "desktop", "claude-desktop", "claude-desktop-providers", "claude-desktop-providers/add", "proxy", "settings", "guide"];
  const providerFormModelSlots = [
    { key: "default", label: "Default", icon: "bi-circle-fill", iconClass: "default", source: "未配置映射时默认使用这一项", required: true },
    { key: "gpt_5_5", label: "gpt-5.5", icon: "bi-circle", iconClass: "default", source: "gpt-5.5" },
    { key: "gpt_5_4", label: "gpt-5.4", icon: "bi-circle", iconClass: "default", source: "gpt-5.4" },
    { key: "gpt_5_4_mini", label: "gpt-5.4-mini", icon: "bi-circle", iconClass: "default", source: "gpt-5.4-mini" },
    { key: "gpt_5_3_codex", label: "gpt-5.3-codex", icon: "bi-circle", iconClass: "default", source: "gpt-5.3-codex" },
    { key: "gpt_5_2", label: "gpt-5.2", icon: "bi-circle", iconClass: "default", source: "gpt-5.2" },
  ];
  const availableThemes = ["default", "green", "orange", "gray", "dark", "white"];
  // **2026-05-10 修复**:加 google_api_key(Gemini native 用 `x-goog-api-key` header)。
  // 旧实现白名单只有 bearer / x-api-key / none,Google AI Studio preset 的
  // authScheme=google_api_key 经 setAuthSchemeValue 校验失败 → fallback 'bearer'
  // → backend 用 Authorization: Bearer 调 Gemini /v1beta/models → 401(Google 不接 Bearer)
  // → 测速看似绿(401 走 auth_not_verified 路径)但实际从未真正鉴权过 + 列模型失败。
  const providerAuthSchemes = ["bearer", "x-api-key", "google_api_key", "grok_cookie", "none"];
  const providerFormDefaultRows = ["default", "gpt_5_5", "gpt_5_4", "gpt_5_4_mini", "gpt_5_3_codex", "gpt_5_2"];
  let pendingDeleteId = null;
  let selectedPreset = null;
  let presetCache = [];
  let formApiFormatValue = "openai_chat";
  let formModelCapabilities = {};
  let formRequestOptions = {};
  let providerFormMappings = {};
  let providerFormRows = [...providerFormDefaultRows];
  let providerFormCustomLabels = {};
  let customRowCounter = 0;
  let providerAvailableModels = [];
  let openProviderSlotMenuIndex = null;
  let openProviderModelMenuKey = null;
  let baseUrlMenuOpen = false;
  let editingProviderId = null;
  let deleteModal = null;
  let restartReminderModal = null;
  let toast = null;
  let updateCheckCache = null;
  let updateInstallPhase = "idle";

  function $(selector, root = document) {
    return root.querySelector(selector);
  }

  function $all(selector, root = document) {
    return Array.from(root.querySelectorAll(selector));
  }

  function routeFromHash() {
    const hash = window.location.hash.replace(/^#/, "");
    return routes.includes(hash) ? hash : "dashboard";
  }

  function showToast(message) {
    $("#toastBody").textContent = message;
    toast.show();
  }

  function showRestartReminder() {
    restartReminderModal?.show();
  }

  function dismissRestartReminderLater() {
    restartReminderModal?.hide();
  }

  async function restartCodexAppNow() {
    const button = $("#restartReminderNow");
    const original = button?.textContent;
    try {
      if (button) {
        button.disabled = true;
        button.textContent = t("restartReminder.restarting");
      }
      await CCApi.restartCodexApp();
      restartReminderModal?.hide();
      showToast(t("toast.codexAppRestartRequested"));
    } catch (error) {
      console.error(error);
      showToast(error.message || t("toast.codexAppRestartFailed"));
    } finally {
      if (button) {
        button.disabled = false;
        button.textContent = original || t("restartReminder.now");
      }
    }
  }

  function t(key) {
    return CCI18n.t(key);
  }

  // formatI18n removed (M2 migration) — 使用 tFmt 统一(line ~1131),tFmt 多了
  // missing-key + unsubstituted-placeholder warning,行为更安全。所有原 callsite
  // 已迁到 tFmt


  function iconMarkup(item) {
    if (item.logo) return `<img src="${item.logo}" alt="">`;
    if (item.iconText) return `<span>${item.iconText}</span>`;
    return `<i class="bi ${item.icon || "bi-plug-fill"}"></i>`;
  }

  function escapeHtml(value) {
    return String(value ?? "").replace(/[&<>"']/g, (char) => ({
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      "\"": "&quot;",
      "'": "&#39;",
    }[char]));
  }

  function safeHttpUrl(value) {
    try {
      const parsed = new URL(String(value || ""), window.location.origin);
      if (["http:", "https:"].includes(parsed.protocol)) return parsed.href;
    } catch (error) {
      return "#";
    }
    return "#";
  }

  function normalizePresetKey(value) {
    return String(value || "").trim().toLowerCase().replace(/\/+$/, "");
  }

  function presetExists(preset, providers) {
    // 「自定义第三方」是无限重复添加入口卡片(用户每次填不同 baseUrl + apiKey),
    // 永远视为不存在 → 永远在 dashboard available presets 列表显示
    if (preset.id === "custom-third-party") return false;
    const presetName = normalizePresetKey(preset.name);
    const presetUrl = normalizePresetKey(preset.baseUrl);
    const presetApiFormat = String(preset.apiFormat || "").toLowerCase();
    return providers.some((provider) => {
      // **多 preset 共享上游场景**:eg gemini-cli-oauth + antigravity-oauth 都
      // 走 cloudcode-pa.googleapis.com 上游但 apiFormat 不同 (前者
      // gemini_cli_oauth 后者 antigravity_oauth) — 加一个另一个不能被
      // baseUrl 去重隐藏。同 apiFormat 才视为同 preset(2026-05-11 修)
      if (presetApiFormat && String(provider.apiFormat || "").toLowerCase() !== presetApiFormat) {
        return false;
      }
      return (
        normalizePresetKey(provider.name) === presetName
        || normalizePresetKey(provider.baseUrl) === presetUrl
      );
    });
  }

  function updatePresetSelection() {
    const selectedId = selectedPreset?.id || "";
    $all("#presetList [data-preset]").forEach((button) => {
      const active = button.dataset.preset === selectedId;
      button.classList.toggle("active", active);
      button.setAttribute("aria-pressed", active ? "true" : "false");
      const icon = $("i:last-child", button);
      if (icon) icon.className = `bi ${active ? "bi-check2" : "bi-chevron-right"}`;
    });
  }

  function normalizeApiFormat(apiFormat) {
    const v = String(apiFormat || "").toLowerCase().replace(/-/g, "_");
    if (["responses", "openai_responses"].includes(v)) return { key: "responses", canonical: "responses" };
    if (["anthropic_messages", "anthropic", "claude", "messages", "claude_messages"].includes(v)) {
      return { key: "anthropic", canonical: "anthropic_messages" };
    }
    if (["gemini_native", "google_ai_studio", "gemini"].includes(v)) return { key: "geminiNative", canonical: "gemini_native" };
    if (["gemini_cli_oauth", "gemini_cli", "google_oauth_cloud_code"].includes(v)) return { key: "geminiCliOauth", canonical: "gemini_cli_oauth" };
    if (["grok_web", "grok", "grok_com"].includes(v)) return { key: "grokWeb", canonical: "grok_web" };
    if (["antigravity_oauth", "antigravity", "google_oauth_antigravity"].includes(v)) return { key: "antigravityOauth", canonical: "antigravity_oauth" };
    return { key: "openaiChat", canonical: "openai_chat" };
  }

  /// Cloud Code Assist OAuth provider 的 per-canonical 配置:i18n key 前缀(决定
  /// 整套 UI 文案 namespace)+ CCApi 方法名(login/status/logout)。新增 OAuth
  /// provider 时往这里加一条即可,setOauthRowState/refreshOauthStatusUi/
  /// handleOauthLogin/handleOauthLogout 都 share 这个 dispatch 表
  const OAUTH_PROVIDER_CONFIGS = {
    gemini_cli_oauth: {
      i18nPrefix: "geminiOauth",
      api: {
        getStatus: () => CCApi.getGeminiOauthStatus(),
        login: () => CCApi.loginGeminiOauth(),
        logout: () => CCApi.logoutGeminiOauth(),
      },
    },
    antigravity_oauth: {
      i18nPrefix: "antigravityOauth",
      api: {
        getStatus: () => CCApi.getAntigravityOauthStatus(),
        login: () => CCApi.loginAntigravityOauth(),
        logout: () => CCApi.logoutAntigravityOauth(),
      },
    },
  };

  /// 当前 form 已选 apiFormat 对应的 OAuth provider config(none → null)。
  /// setOauthRowState 时缓存,refresh / login / logout 复用避免每次重 lookup
  let activeOauthConfig = null;

  /// Cloud Code Assist OAuth 路径不需要 apiKey input,改 OAuth login UI block。
  /// 返 true 表示当前协议是 OAuth 模式(gemini_cli_oauth 或 antigravity_oauth),
  /// 调用方据此切换 form 显示
  function isOauthApiFormat(apiFormat) {
    const { canonical } = normalizeApiFormat(apiFormat);
    return Object.prototype.hasOwnProperty.call(OAUTH_PROVIDER_CONFIGS, canonical);
  }

  function renderApiFormatDisplay(apiFormat) {
    const { key, canonical } = normalizeApiFormat(apiFormat);
    formApiFormatValue = canonical;
    const nameEl = $("#providerApiFormatName");
    const detailEl = $("#providerApiFormatDetail");
    if (nameEl) {
      const nameKey = `apiFormatDisplay.${key}.name`;
      nameEl.dataset.i18n = nameKey;
      nameEl.textContent = t(nameKey);
    }
    if (detailEl) {
      const detailKey = `apiFormatDisplay.${key}.detail`;
      detailEl.dataset.i18n = detailKey;
      detailEl.textContent = t(detailKey);
    }
  }

  function updateApiFormatSelectDetail(value) {
    const { key, canonical } = normalizeApiFormat(value);
    formApiFormatValue = canonical;
    const detailEl = $("#providerApiFormatSelectDetail");
    if (detailEl) {
      const detailKey = `apiFormatDisplay.${key}.detail`;
      detailEl.dataset.i18n = detailKey;
      detailEl.textContent = t(detailKey);
    }
    // 协议切换 → 重渲 mappings UI 让 default required 状态跟当前协议同步
    // (direct 模式 default 解锁为可空,其他场景仍 required)
    setProviderMappings(providerFormMappings);
    // OAuth 模式切换:apiFormat=gemini_cli_oauth 时隐藏 apiKey input,显示 OAuth UI
    setOauthRowState(canonical);
  }

  // 控制 web_search 配置开关 row 的显示 + 初始 checkbox state + provider-specific
  // hint 文案。preset.supportsWebSearch === true 才显示(Kimi / Kimi Code / MiMo
  // PAYG / MiMo Token Plan 四家;Gemini OpenAI compat chat 不支持 grounding,
  // 已实测 5 种 variant 全 400,故不开)。
  // hint 文案按 presetId 选 i18n key,fallback 到 .default;切换 preset 时
  // 同步更新 dataset.i18n + textContent,语言切换 + preset 切换都不会留旧文案。
  function setWebSearchRow(supports, enabled, presetId) {
    const row = $("#providerWebSearchRow");
    const cb = $("#providerWebSearchEnabled");
    const hint = $("#providerWebSearchHint");
    if (row) row.hidden = !supports;
    if (cb) cb.checked = !!enabled;
    if (hint) {
      const specificKey = presetId
        ? `providersAdd.webSearchEnabledHint.${presetId}`
        : null;
      const fallbackKey = "providersAdd.webSearchEnabledHint.default";
      const useKey = specificKey && t(specificKey) !== specificKey ? specificKey : fallbackKey;
      hint.dataset.i18n = useKey;
      hint.textContent = t(useKey);
    }
  }

  function setApiFormatMode(allowSelect, currentValue) {
    const displayEl = $("#providerApiFormatDisplay");
    const selectableEl = $("#providerApiFormatSelectable");
    const selectEl = $("#providerApiFormatSelect");
    if (displayEl) displayEl.hidden = allowSelect;
    if (selectableEl) selectableEl.hidden = !allowSelect;
    if (allowSelect && selectEl) {
      const { canonical } = normalizeApiFormat(currentValue);
      selectEl.value = canonical;
      updateApiFormatSelectDetail(canonical);
    }
  }

  function firstHealthMessage(health) {
    return health?.issues?.[0]?.message || "";
  }

  function renderDesktopHealthWarning(selector, health) {
    const warning = $(selector);
    if (!warning) return;
    const message = firstHealthMessage(health);
    warning.hidden = !message;
    const text = $("span", warning);
    if (text) text.textContent = message;
  }

  function renderUpdateBadge(result) {
    const badge = $("#dashboardUpdateBadge");
    const available = !!result?.updateAvailable;
    const installButton = $("#settingsInstallUpdate");
    const busy = updateInstallPhase !== "idle";
    if (badge) {
      badge.hidden = !(available || busy);
      badge.disabled = busy;
      badge.title = available && !busy ? t("settings.installUpdate") : "";
      badge.setAttribute("aria-label", available ? t("settings.installUpdate") : t("dashboard.updateAvailable"));
    }
    if (installButton) {
      installButton.hidden = !(available || busy);
      installButton.disabled = busy;
    }
    const badgeIcon = badge ? $("i", badge) : null;
    if (badgeIcon) {
      badgeIcon.className = busy ? "bi bi-arrow-repeat" : "bi bi-cloud-arrow-down";
    }
    const installIcon = installButton ? $("i", installButton) : null;
    if (installIcon) {
      installIcon.className = busy ? "bi bi-arrow-repeat" : "bi bi-download";
    }
    const text = badge ? $("span", badge) : null;
    if (text) {
      if (updateInstallPhase === "downloading") {
        text.textContent = t("settings.downloadingUpdate");
      } else if (updateInstallPhase === "installing") {
        text.textContent = t("settings.installingUpdate");
      } else if (available) {
        text.textContent = result.latestVersion
          ? `${t("dashboard.updateAvailable")} ${result.latestVersion}`
          : t("dashboard.updateAvailable");
      }
    }
    const installText = installButton ? $("span", installButton) : null;
    if (installText) {
      if (updateInstallPhase === "downloading") {
        installText.textContent = t("settings.downloadingUpdate");
      } else if (updateInstallPhase === "installing") {
        installText.textContent = t("settings.installingUpdate");
      } else {
        installText.textContent = t("settings.installUpdate");
      }
    }
  }

  function setUpdateInstallPhase(phase = "idle") {
    updateInstallPhase = phase;
    renderUpdateBadge(updateCheckCache);
  }

  async function refreshUpdateBadge(force = false) {
    try {
      updateCheckCache = await CCApi.checkUpdate("");
      renderUpdateBadge(updateCheckCache);
    } catch (error) {
      console.warn(error);
      updateCheckCache = null;
      renderUpdateBadge(null);
    }
  }

  function emptyMappings() {
    return Object.fromEntries(providerFormModelSlots.map((slot) => [slot.key, ""]));
  }

  const predefinedSlotKeys = new Set(providerFormModelSlots.map((s) => s.key));

  function isCustomMappingRow(key) {
    return key.startsWith("_custom_");
  }

  function normalizeMappings(mappings = {}) {
    const normalized = emptyMappings();
    if (!mappings || typeof mappings !== "object") return normalized;
    normalized.default = String(mappings.default || "").trim();
    normalized.gpt_5_5 = String(mappings.gpt_5_5 || "").trim();
    normalized.gpt_5_4 = String(mappings.gpt_5_4 || "").trim();
    normalized.gpt_5_4_mini = String(mappings.gpt_5_4_mini || "").trim();
    normalized.gpt_5_3_codex = String(mappings.gpt_5_3_codex || "").trim();
    normalized.gpt_5_2 = String(mappings.gpt_5_2 || "").trim();
    // preserve custom (non-predefined) keys
    for (const [key, value] of Object.entries(mappings)) {
      if (!predefinedSlotKeys.has(key)) {
        const trimmed = String(value || "").trim();
        if (trimmed) normalized[key] = trimmed;
      }
    }
    return normalized;
  }

  function normalizeCapabilities(capabilities = {}) {
    if (!capabilities || typeof capabilities !== "object") return {};
    return Object.fromEntries(Object.entries(capabilities).filter(([, value]) => (
      value && typeof value === "object" && value.supports1m === true
    )));
  }

  const EFFORT_VALUES = ["low", "medium", "high", "xhigh", "max"];

  function normalizeResponsesBlock(source = {}) {
    if (!source || typeof source !== "object") return {};
    const block = {};
    if (["enabled", "disabled"].includes(source.thinking?.type)) {
      block.thinking = { type: source.thinking.type };
    }
    if (EFFORT_VALUES.includes(source.output_config?.effort)) {
      block.output_config = { effort: source.output_config.effort };
    }
    return block;
  }

  function normalizeChatBlock(source = {}) {
    if (!source || typeof source !== "object") return {};
    const block = {};
    // DeepSeek V4：thinking 对象只接受 type；reasoning_effort 在请求体顶层
    const thinkingType = source.thinking?.type;
    if (["enabled", "disabled"].includes(thinkingType)) {
      block.thinking = { type: thinkingType };
    }
    if (EFFORT_VALUES.includes(source.reasoning_effort)) {
      block.reasoning_effort = source.reasoning_effort;
    }
    return block;
  }

  function normalizeRequestOptions(options = {}) {
    if (!options || typeof options !== "object") return {};
    // 兼容旧配置：anthropic 键重命名为 responses
    const responsesSource = options.responses && typeof options.responses === "object"
      ? options.responses
      : (options.anthropic && typeof options.anthropic === "object" ? options.anthropic : null);
    const result = {};
    const responsesBlock = normalizeResponsesBlock(responsesSource || (responsesSource === null ? options : {}));
    if (Object.keys(responsesBlock).length) result.responses = responsesBlock;
    const chatBlock = normalizeChatBlock(options.chat);
    if (Object.keys(chatBlock).length) result.chat = chatBlock;
    // **顶级 boolean / 标量字段**:web_search_enabled 由 backend
    // `convert_web_search_tool` 读 `provider.request_options.web_search_enabled`
    // 决定是否启用 web 搜索(MiMo / Kimi / Gemini 等支持 web_search 的 provider)。
    // 之前版本 normalize 不保留此字段 → frontend 编辑保存即剥光,用户必须
    // 手改 config.json,UX 痛点。本字段必须保留(boolean),否则功能失效。
    if (typeof options.web_search_enabled === "boolean") {
      result.web_search_enabled = options.web_search_enabled;
    }
    return result;
  }

  function mergeRequestOptions(base = {}, extra = {}) {
    const baseNorm = normalizeRequestOptions(base);
    const extraNorm = normalizeRequestOptions(extra);
    const merged = {};
    const responsesMerged = { ...(baseNorm.responses || {}), ...(extraNorm.responses || {}) };
    if (Object.keys(responsesMerged).length) merged.responses = responsesMerged;
    const chatMerged = { ...(baseNorm.chat || {}), ...(extraNorm.chat || {}) };
    if (Object.keys(chatMerged).length) merged.chat = chatMerged;
    return normalizeRequestOptions(merged);
  }

  function clearRequestOptions(base = {}, option = {}) {
    const baseNorm = normalizeRequestOptions(base);
    const optionNorm = normalizeRequestOptions(option);
    const next = {};
    if (baseNorm.responses) {
      const block = { ...baseNorm.responses };
      Object.keys(optionNorm.responses || {}).forEach((key) => { delete block[key]; });
      if (Object.keys(block).length) next.responses = block;
    }
    if (baseNorm.chat) {
      const block = { ...baseNorm.chat };
      Object.keys(optionNorm.chat || {}).forEach((key) => { delete block[key]; });
      if (Object.keys(block).length) next.chat = block;
    }
    return normalizeRequestOptions(next);
  }

  function requestOptionsMatch(left = {}, right = {}) {
    return JSON.stringify(normalizeRequestOptions(left)) === JSON.stringify(normalizeRequestOptions(right));
  }

  function capabilitiesMatch(left = {}, right = {}) {
    return JSON.stringify(normalizeCapabilities(left)) === JSON.stringify(normalizeCapabilities(right));
  }

  function mergeCapabilities(base = {}, extra = {}) {
    return {
      ...normalizeCapabilities(base),
      ...normalizeCapabilities(extra),
    };
  }

  function clearCapabilities(base = {}, option = {}) {
    const current = normalizeCapabilities(base);
    Object.keys(normalizeCapabilities(option)).forEach((modelId) => {
      delete current[modelId];
    });
    return current;
  }

  function optionEnabled(option = {}, currentMappings = collectProviderMappings()) {
    const hasModels = option.models && typeof option.models === "object";
    const hasRequestOptions = option.requestOptions && typeof option.requestOptions === "object";
    const hasCapabilities = option.modelCapabilities && typeof option.modelCapabilities === "object";
    const modelsOk = !hasModels || modelsMatch(option.models, currentMappings);
    const requestOptionsOk = !hasRequestOptions || requestOptionsMatch(option.requestOptions, formRequestOptions);
    const optionChangesModels = hasModels && !modelsMatch(option.models, selectedPreset?.models || {});
    const capabilitiesOk = !hasCapabilities || optionChangesModels || capabilitiesMatch(option.modelCapabilities, formModelCapabilities);
    if (hasModels || hasRequestOptions || hasCapabilities) {
      return modelsOk && requestOptionsOk && capabilitiesOk;
    }
    return false;
  }

  function modelsMatch(left = {}, right = {}) {
    const a = normalizeMappings(left);
    const b = normalizeMappings(right);
    return providerFormModelSlots.every((slot) => (a[slot.key] || "") === (b[slot.key] || ""));
  }

  function presetMatchesProvider(preset, provider) {
    if (!preset || !provider) return false;
    const baseUrlOptions = Array.isArray(preset.baseUrlOptions) ? preset.baseUrlOptions : [];
    // **多 preset 共享上游场景**:apiFormat 不同就不算同 preset
    // (gemini-cli-oauth vs antigravity-oauth 同 baseUrl 但不同协议;2026-05-11 修)
    if (preset.apiFormat && provider.apiFormat
        && String(preset.apiFormat).toLowerCase() !== String(provider.apiFormat).toLowerCase()) {
      return false;
    }
    return normalizePresetKey(preset.name) === normalizePresetKey(provider.name)
      || normalizePresetKey(preset.baseUrl) === normalizePresetKey(provider.baseUrl)
      || baseUrlOptions.some((option) => normalizePresetKey(option?.value) === normalizePresetKey(provider.baseUrl));
  }

  function presetBaseUrlOptions(preset = null) {
    return Array.isArray(preset?.baseUrlOptions) ? preset.baseUrlOptions.filter((option) => option?.value) : [];
  }

  function closeBaseUrlMenu() {
    if (!baseUrlMenuOpen) return;
    baseUrlMenuOpen = false;
    renderBaseUrlOptions();
  }

  function toggleBaseUrlMenu() {
    if (!presetBaseUrlOptions(selectedPreset).length) return;
    baseUrlMenuOpen = !baseUrlMenuOpen;
    renderBaseUrlOptions();
  }

  function setBaseUrlValue(value) {
    const input = $("#providerBaseUrl");
    if (!input) return;
    input.value = value;
    closeBaseUrlMenu();
  }

  function renderBaseUrlOptions(preset = selectedPreset) {
    const input = $("#providerBaseUrl");
    const trigger = $("#providerBaseUrlTrigger");
    const menu = $("#providerBaseUrlMenu");
    const wrap = $("#providerBaseUrlControl");
    const hint = $("#providerBaseUrlHint");
    if (!input || !trigger || !menu || !wrap || !hint) return;
    const options = presetBaseUrlOptions(preset);
    const helpText = String(preset?.baseUrlHint || "").trim();
    trigger.hidden = !options.length;
    trigger.disabled = !options.length;
    trigger.setAttribute("aria-expanded", options.length && baseUrlMenuOpen ? "true" : "false");
    wrap.classList.toggle("open", !!options.length && baseUrlMenuOpen);
    menu.innerHTML = options.map((option) => {
      const selected = input.value.trim() === option.value;
      return `
        <button
          class="baseurl-option ${selected ? "selected" : ""}"
          type="button"
          role="option"
          data-action="select-baseurl-option"
          data-baseurl-value="${escapeHtml(option.value)}"
          aria-selected="${selected ? "true" : "false"}"
        >
          <span>${escapeHtml(option.value)}</span>
          <small>${escapeHtml(option.label || "")}</small>
          ${selected ? '<i class="bi bi-check2"></i>' : ""}
        </button>
      `;
    }).join("");
    hint.textContent = helpText;
    hint.hidden = !helpText;
  }

  function capabilitiesForCurrentMappings(mappings = collectProviderMappings()) {
    const usedModelIds = new Set(Object.values(mappings).filter(Boolean));
    return Object.fromEntries(Object.entries(normalizeCapabilities(formModelCapabilities)).filter(([modelId]) => (
      usedModelIds.has(modelId)
    )));
  }

  function formMappingRowsFromMappings(mappings = {}) {
    const rows = [...providerFormDefaultRows];
    providerFormModelSlots.forEach((slot) => {
      if (slot.key !== "default" && mappings[slot.key] && !rows.includes(slot.key)) {
        rows.push(slot.key);
      }
    });
    // restore custom mapping rows (keys not in predefined slots)
    providerFormCustomLabels = {};
    for (const [key, value] of Object.entries(mappings)) {
      if (!predefinedSlotKeys.has(key) && String(value || "").trim()) {
        const customKey = `_custom_${customRowCounter++}`;
        rows.push(customKey);
        providerFormMappings[customKey] = String(value || "").trim();
        providerFormCustomLabels[customKey] = key;
      }
    }
    return rows;
  }

  function slotByKey(key) {
    return providerFormModelSlots.find((slot) => slot.key === key) || providerFormModelSlots[0];
  }

  function slotOptionsForRow(currentKey) {
    const used = new Set(providerFormRows.filter((key) => key !== currentKey));
    return providerFormModelSlots.filter((slot) => !used.has(slot.key));
  }

  function providerModelOptionsMarkup(currentValue = "") {
    return providerAvailableModels.map((modelId) => (`
      <button
        class="mapping-slot-option ${modelId === currentValue ? "selected" : ""}"
        type="button"
        role="option"
        data-action="select-provider-model-option"
        data-model-value="${escapeHtml(modelId)}"
        aria-selected="${modelId === currentValue ? "true" : "false"}"
      >
        <span>${escapeHtml(modelId)}</span>
        ${modelId === currentValue ? '<i class="bi bi-check2"></i>' : ""}
      </button>
    `)).join("");
  }

  function slotMenuMarkup(rowKey, index) {
    const slot = slotByKey(rowKey);
    const isRequired = rowKey === "default";
    const expanded = openProviderSlotMenuIndex === index;
    const options = slotOptionsForRow(rowKey).map((option) => (`
      <button
        class="mapping-slot-option ${option.key === rowKey ? "selected" : ""}"
        type="button"
        role="option"
        data-action="select-provider-model-slot"
        data-row-index="${index}"
        data-slot-key="${escapeHtml(option.key)}"
        aria-selected="${option.key === rowKey ? "true" : "false"}"
      >
        <span>${escapeHtml(option.label)}</span>
        ${option.key === rowKey ? '<i class="bi bi-check2"></i>' : ""}
      </button>
    `)).join("");
    return `
      <div class="mapping-slot-menu-wrap ${expanded ? "open" : ""}">
        <button
          class="form-select mapping-slot-trigger"
          id="providerMappingSlot-${index}"
          type="button"
          ${isRequired ? "disabled" : ""}
          data-action="toggle-provider-model-slot-menu"
          data-row-index="${index}"
          aria-haspopup="listbox"
          aria-expanded="${expanded ? "true" : "false"}"
        >
          <span>${escapeHtml(slot.label)}</span>
          <i class="bi bi-chevron-down"></i>
        </button>
        ${isRequired ? "" : `
          <div class="mapping-slot-menu" role="listbox" aria-labelledby="providerMappingSlot-${index}">
            ${options}
          </div>
        `}
      </div>
    `;
  }

  function isDirectResponsesMode() {
    // 自定义第三方 + apiFormat=responses → Codex.app 直连上游(direct 模式),
    // 模型透传给上游,代理不做 alias 翻译 → default mapping 可空。
    return (
      formApiFormatValue === "responses" && !!selectedPreset?.allowApiFormatSelection
    );
  }

  function customMappingRowMarkup(rowKey, index) {
    const customLabel = providerFormCustomLabels[rowKey] || "";
    const currentProviderModel = providerFormMappings[rowKey] || "";
    return `
      <article class="form-mapping-row">
        <div class="form-mapping-left">
          <label class="form-label visually-hidden" for="providerMappingSlot-${index}">${t("providersAdd.claudeModel")}</label>
          <div class="mapping-select-wrap">
            <span class="mapping-icon default"><i class="bi bi-pencil"></i></span>
            <input
              class="form-control form-select custom-model-name-input"
              id="providerMappingSlot-${index}"
              data-custom-model-label="${escapeHtml(rowKey)}"
              value="${escapeHtml(customLabel)}"
              placeholder="${escapeHtml(t("providersAdd.customModelPlaceholder"))}"
            >
          </div>
        </div>
        <div class="form-mapping-right">
          <label class="form-label visually-hidden" for="providerMappingValue-${index}">${t("providersAdd.providerModel")}</label>
          <div class="provider-model-input-wrap ${openProviderModelMenuKey === rowKey ? "open" : ""}">
            <input
              class="form-control provider-model-input"
              id="providerMappingValue-${index}"
              data-provider-model-input="${escapeHtml(rowKey)}"
              value="${escapeHtml(currentProviderModel)}"
              placeholder="${escapeHtml(t("providersAdd.providerModelPlaceholder"))}"
            >
            <button
              class="provider-model-trigger"
              type="button"
              data-action="toggle-provider-model-menu"
              data-row-key="${escapeHtml(rowKey)}"
              ${providerAvailableModels.length ? "" : "disabled"}
              aria-haspopup="listbox"
              aria-expanded="${providerAvailableModels.length && openProviderModelMenuKey === rowKey ? "true" : "false"}"
              aria-label="${escapeHtml(t("providersAdd.providerModel"))}"
            >
              <i class="bi bi-chevron-down" aria-hidden="true"></i>
            </button>
            ${providerAvailableModels.length ? `
              <div class="mapping-slot-menu provider-model-menu" role="listbox" aria-labelledby="providerMappingValue-${index}">
                ${providerModelOptionsMarkup(currentProviderModel)}
              </div>
            ` : ""}
          </div>
        </div>
        <div class="form-mapping-actions">
          <button class="btn btn-outline-secondary btn-sm mapping-remove-button" type="button" data-action="remove-provider-model-row" data-row-index="${index}" aria-label="${escapeHtml(t("providersAdd.removeMapping"))}">${escapeHtml(t("providersAdd.removeMapping"))}</button>
        </div>
      </article>
    `;
  }

  function formMappingMarkup() {
    return providerFormRows.map((rowKey, index) => {
      if (isCustomMappingRow(rowKey)) {
        return customMappingRowMarkup(rowKey, index);
      }
      const slot = slotByKey(rowKey);
      // direct 模式不需要 model alias 映射,default 字段也可空;其他场景仍 required
      const isRequired = rowKey === "default" && !isDirectResponsesMode();
      const currentProviderModel = providerFormMappings[rowKey] || "";
      return `
        <article class="form-mapping-row">
          <div class="form-mapping-left">
            <label class="form-label visually-hidden" for="providerMappingSlot-${index}">${t("providersAdd.claudeModel")}</label>
            <div class="mapping-select-wrap">
              <span class="mapping-icon ${slot.iconClass}"><i class="bi ${slot.icon}"></i></span>
              ${slotMenuMarkup(rowKey, index)}
            </div>
          </div>
          <div class="form-mapping-right">
            <label class="form-label visually-hidden" for="providerMappingValue-${index}">${t("providersAdd.providerModel")}</label>
            <div class="provider-model-input-wrap ${openProviderModelMenuKey === rowKey ? "open" : ""}">
              <input
                class="form-control provider-model-input"
                id="providerMappingValue-${index}"
                data-provider-model-input="${escapeHtml(rowKey)}"
                value="${escapeHtml(providerFormMappings[rowKey] || "")}"
                placeholder="${escapeHtml(t("providersAdd.providerModelPlaceholder"))}"
                ${isRequired ? "required" : ""}
              >
              <button
                class="provider-model-trigger"
                type="button"
                data-action="toggle-provider-model-menu"
                data-row-key="${escapeHtml(rowKey)}"
                ${providerAvailableModels.length ? "" : "disabled"}
                aria-haspopup="listbox"
                aria-expanded="${providerAvailableModels.length && openProviderModelMenuKey === rowKey ? "true" : "false"}"
                aria-label="${escapeHtml(t("providersAdd.providerModel"))}"
              >
                <i class="bi bi-chevron-down" aria-hidden="true"></i>
              </button>
              ${providerAvailableModels.length ? `
                <div class="mapping-slot-menu provider-model-menu" role="listbox" aria-labelledby="providerMappingValue-${index}">
                  ${providerModelOptionsMarkup(currentProviderModel)}
                </div>
              ` : ""}
            </div>
          </div>
          <div class="form-mapping-actions">
            ${isRequired
              ? '<span class="mapping-remove-placeholder" aria-hidden="true"></span>'
              : `<button class="btn btn-outline-secondary btn-sm mapping-remove-button" type="button" data-action="remove-provider-model-row" data-row-index="${index}" aria-label="${escapeHtml(t("providersAdd.removeMapping"))}">${escapeHtml(t("providersAdd.removeMapping"))}</button>`}
          </div>
        </article>
      `;
    }).join("");
  }

  function renderProviderMappings() {
    const stack = $("#providerMappingStack");
    if (!stack) return;
    if (openProviderSlotMenuIndex !== null && !providerFormRows[openProviderSlotMenuIndex]) {
      openProviderSlotMenuIndex = null;
    }
    if (openProviderModelMenuKey !== null && !providerFormRows.includes(openProviderModelMenuKey)) {
      openProviderModelMenuKey = null;
    }
    stack.innerHTML = `
      <div class="provider-mapping-card">
        <div class="provider-mapping-list">
          ${formMappingMarkup()}
        </div>
        <div class="provider-mapping-footer">
          <button class="btn btn-outline-primary btn-sm" type="button" data-action="add-provider-model-row">
            <i class="bi bi-plus-lg"></i><span>${escapeHtml(t("providersAdd.addMapping"))}</span>
          </button>
        </div>
      </div>
    `;
  }

  function setProviderMappings(mappings = {}, options = {}) {
    providerFormMappings = normalizeMappings(mappings);
    providerFormRows = formMappingRowsFromMappings(providerFormMappings);
    if (Array.isArray(options.availableModels)) {
      providerAvailableModels = options.availableModels.slice();
    }
    openProviderSlotMenuIndex = null;
    openProviderModelMenuKey = null;
    renderProviderMappings();
  }

  function collectProviderMappingsWithCustom() {
    const base = normalizeMappings(providerFormMappings);
    // convert _custom_N internal keys to the user-typed model name
    const result = {};
    for (const [key, value] of Object.entries(base)) {
      if (isCustomMappingRow(key)) {
        const label = (providerFormCustomLabels[key] || "").trim();
        if (label && String(value || "").trim()) {
          result[label] = String(value).trim();
        }
      } else {
        result[key] = value;
      }
    }
    return result;
  }

  function updateProviderModelInput(slotKey, value) {
    providerFormMappings[slotKey] = value.trim();
  }

  function moveProviderMappingRow(index, nextKey) {
    const prevKey = providerFormRows[index];
    if (!nextKey || prevKey === nextKey) return;
    const currentValue = providerFormMappings[prevKey] || "";
    providerFormRows[index] = nextKey;
    if (!providerFormMappings[nextKey]) {
      providerFormMappings[nextKey] = currentValue;
    }
    if (prevKey !== "default") {
      providerFormMappings[prevKey] = "";
    }
    openProviderSlotMenuIndex = null;
    renderProviderMappings();
  }

  function addProviderMappingRow() {
    const remaining = providerFormModelSlots
      .map((slot) => slot.key)
      .find((key) => !providerFormRows.includes(key));
    if (remaining) {
      providerFormRows = [...providerFormRows, remaining];
    } else {
      // all predefined slots used — add a custom row
      const customKey = `_custom_${customRowCounter++}`;
      providerFormRows = [...providerFormRows, customKey];
      providerFormMappings[customKey] = "";
      providerFormCustomLabels[customKey] = "";
    }
    openProviderSlotMenuIndex = null;
    openProviderModelMenuKey = null;
    renderProviderMappings();
  }

  function removeProviderMappingRow(index) {
    const key = providerFormRows[index];
    if (!key || key === "default") return;
    providerFormRows = providerFormRows.filter((_, rowIndex) => rowIndex !== index);
    if (isCustomMappingRow(key)) {
      delete providerFormMappings[key];
      delete providerFormCustomLabels[key];
    } else {
      providerFormMappings[key] = "";
    }
    openProviderSlotMenuIndex = null;
    if (openProviderModelMenuKey === key) openProviderModelMenuKey = null;
    renderProviderMappings();
  }

  function toggleProviderSlotMenu(index) {
    openProviderSlotMenuIndex = openProviderSlotMenuIndex === index ? null : index;
    renderProviderMappings();
  }

  function closeProviderSlotMenu() {
    if (openProviderSlotMenuIndex === null) return;
    openProviderSlotMenuIndex = null;
    renderProviderMappings();
  }

  function toggleProviderModelMenu(rowKey) {
    openProviderModelMenuKey = openProviderModelMenuKey === rowKey ? null : rowKey;
    renderProviderMappings();
  }

  function closeProviderModelMenu() {
    if (openProviderModelMenuKey === null) return;
    openProviderModelMenuKey = null;
    renderProviderMappings();
  }

  function setAuthSchemeValue(value) {
    const input = $("#providerAuth");
    if (!input) return;
    input.value = providerAuthSchemes.includes(value) ? value : "bearer";
  }

  function renderPresetOptions(preset = null, mappings = null) {
    const container = $("#providerPresetOptions");
    if (!container) return;
    const modelOptions = preset?.modelOptions && typeof preset.modelOptions === "object"
      ? Object.entries(preset.modelOptions)
      : [];
    const requestOptionPresets = preset?.requestOptionPresets && typeof preset.requestOptionPresets === "object"
      ? Object.entries(preset.requestOptionPresets)
      : [];
    const options = [...modelOptions, ...requestOptionPresets];
    const notices = Array.isArray(preset?.notices) ? preset.notices.filter((n) => n && n.text) : [];

    if (!options.length && !notices.length) {
      container.hidden = true;
      container.innerHTML = "";
      return;
    }

    const currentMappings = normalizeMappings(mappings || collectProviderMappings());
    container.hidden = false;

    const noticeIcon = (type) => {
      if (type === "warning") return "bi-exclamation-triangle-fill";
      if (type === "success") return "bi-check-circle-fill";
      return "bi-info-circle-fill";
    };
    const noticesHtml = notices.map((n) => `
      <div class="preset-notice preset-notice-${escapeHtml(n.type || "info")}" role="note">
        <i class="bi ${noticeIcon(n.type)}" aria-hidden="true"></i>
        <span>${escapeHtml(n.text)}</span>
      </div>
    `).join("");

    const optionsHtml = options.map(([id, option]) => `
      <label class="preset-option-item">
        <input class="form-check-input" type="checkbox" data-preset-model-option="${escapeHtml(id)}" ${optionEnabled(option, currentMappings) ? "checked" : ""}>
        <span>
          <strong>${escapeHtml(option.label || id)}</strong>
          <small>${escapeHtml(option.description || "")}</small>
        </span>
      </label>
    `).join("");

    container.innerHTML = noticesHtml + optionsHtml;
  }

  function applyPresetModelOption(optionId, enabled) {
    const option = selectedPreset?.modelOptions?.[optionId] || selectedPreset?.requestOptionPresets?.[optionId];
    if (!option) return;
    const hasModels = option.models && typeof option.models === "object";
    const hasCapabilities = option.modelCapabilities && typeof option.modelCapabilities === "object";
    const mappings = option.models
      ? (enabled ? option.models : selectedPreset.models || emptyMappings())
      : collectProviderMappings();
    if (hasModels) {
      setProviderMappings(mappings);
    }
    if (hasCapabilities) {
      formModelCapabilities = enabled
        ? mergeCapabilities(formModelCapabilities || selectedPreset.modelCapabilities || {}, option.modelCapabilities)
        : clearCapabilities(formModelCapabilities, option.modelCapabilities);
    } else if (hasModels) {
      formModelCapabilities = normalizeCapabilities(enabled
        ? option.modelCapabilities || selectedPreset.modelCapabilities || {}
        : selectedPreset.modelCapabilities || {});
    }
    if (option.requestOptions) {
      formRequestOptions = enabled
        ? mergeRequestOptions(selectedPreset.requestOptions || {}, option.requestOptions)
        : clearRequestOptions(formRequestOptions, option.requestOptions);
    }
    renderPresetOptions(selectedPreset, mappings);
    showToast(`${option.label || optionId} ${t("providersAdd.optionApplied")}`);
  }

  function collectProviderMappings() {
    return collectProviderMappingsWithCustom();
  }

  function providerPayloadFromForm(includeModels = true) {
    const apiKey = $("#providerApiKey").value.trim();
    const mappings = includeModels ? collectProviderMappings() : null;
    // Web Search 开关:仅当 row 显示(preset.supportsWebSearch === true)时,从
    // checkbox 收集 web_search_enabled 写入 formRequestOptions;否则保留 form
    // 状态(preset 不支持时 normalize 阶段会自动剥)。
    const webSearchRow = $("#providerWebSearchRow");
    const webSearchToggle = $("#providerWebSearchEnabled");
    if (webSearchRow && webSearchToggle && !webSearchRow.hidden) {
      formRequestOptions = {
        ...formRequestOptions,
        web_search_enabled: webSearchToggle.checked,
      };
    }
    const payload = {
      name: $("#providerName").value.trim(),
      baseUrl: $("#providerBaseUrl").value.trim(),
      authScheme: $("#providerAuth").value,
      apiFormat: formApiFormatValue,
      extraHeaders: selectedPreset?.extraHeaders || {},
      modelCapabilities: mappings ? capabilitiesForCurrentMappings(mappings) : normalizeCapabilities(formModelCapabilities),
      requestOptions: normalizeRequestOptions(formRequestOptions),
    };
    if (apiKey) {
      payload.apiKey = apiKey;
    }
    if (includeModels) {
      payload.models = mappings;
    }
    // R1 PR-7:apiFormat=grok_web 时打包 extra.grokWeb(cookies + statsigId)。
    // Provider 后端 schema 用 `#[serde(flatten)] extra`,任何不在已知字段的 key
    // 自动收进 provider.extra,所以前端 payload 顶层加 `grokWeb` 就 work。
    const grokWebPayload = collectGrokWebPayload();
    if (grokWebPayload) {
      payload.grokWeb = grokWebPayload;
    }
    return payload;
  }

  function findDocsUrlForProvider(provider) {
    if (!presetCache.length) return null;
    const stripSlash = (s) => String(s || "").replace(/\/+$/, "");
    const target = stripSlash(provider.baseUrl);
    const providerId = String(provider.id || "");
    for (const preset of presetCache) {
      const candidates = new Set([stripSlash(preset.baseUrl)]);
      for (const opt of (preset.baseUrlOptions || [])) {
        if (opt && opt.value) candidates.add(stripSlash(opt.value));
      }
      if (preset.id === providerId || (target && candidates.has(target))) {
        return preset.docsUrl || null;
      }
    }
    return null;
  }

  function providerCardMarkup(provider) {
    const mapping = [
      provider.mappings.default,
      provider.mappings.gpt_5_5,
      provider.mappings.gpt_5_4,
      provider.mappings.gpt_5_4_mini,
      provider.mappings.gpt_5_3_codex,
      provider.mappings.gpt_5_2,
    ]
      .filter(Boolean)
      .slice(0, 2)
      .join(" / ");
    const providerId = escapeHtml(provider.id);
    const providerName = escapeHtml(provider.name);
    const providerUrl = escapeHtml(provider.baseUrl);
    const mappingText = escapeHtml(mapping || provider.apiFormat);
    const docsUrl = findDocsUrlForProvider(provider);
    const baseUrlMarkup = docsUrl
      ? `<a class="truncate baseurl-docs-link" href="#" data-action="open-docs" data-docs-url="${escapeHtml(docsUrl)}" data-provider-name="${providerName}" title="${t("providers.openDocsHint")}">${providerUrl}<i class="bi bi-box-arrow-up-right baseurl-docs-icon"></i></a>`
      : `<span class="truncate">${providerUrl}</span>`;
    return `
      <article class="provider-switch-card ${provider.default ? "active" : ""}" draggable="true" data-provider-id="${providerId}">
        <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
        <span class="provider-logo">${iconMarkup(provider)}</span>
        <span class="provider-main">
          <strong>${providerName}</strong>
          ${baseUrlMarkup}
        </span>
        <span class="provider-meta truncate">${mappingText}</span>
        <span class="provider-actions">
          ${provider.default ? `<span class="active-indicator" role="status" aria-label="${escapeHtml(t("status.active"))}"><i class="bi bi-broadcast" aria-hidden="true"></i><span>${escapeHtml(t("status.active"))}</span></span>` : ""}
          <button class="btn btn-primary compact-enable" type="button" data-action="set-default" data-id="${providerId}">
            <i class="bi bi-play-fill"></i><span>${t("providers.enable")}</span>
          </button>
          <button class="icon-action" type="button" data-action="test-provider" data-id="${providerId}" title="${t("providers.testSpeed")}" aria-label="${t("providers.testSpeed")}"><i class="bi bi-lightning-charge"></i></button>
          <button class="icon-action" type="button" data-action="query-usage" data-id="${providerId}" title="${t("providers.usage")}" aria-label="${t("providers.usage")}"><i class="bi bi-wallet2"></i></button>
          <button class="icon-action" type="button" data-action="edit-provider" data-id="${providerId}" title="${t("common.edit")}" aria-label="${t("common.edit")}"><i class="bi bi-pencil-square"></i></button>
          <button class="icon-action" type="button" data-action="copy-url" data-url="${providerUrl}" title="${t("common.copy")}" aria-label="${t("common.copy")}"><i class="bi bi-copy"></i></button>
          <a class="icon-action" href="#proxy" title="${t("nav.proxy")}" aria-label="${t("nav.proxy")}"><i class="bi bi-terminal"></i></a>
          <button class="icon-action danger" type="button" data-action="delete-provider" data-id="${providerId}" title="${t("common.delete")}" aria-label="${t("common.delete")}"><i class="bi bi-trash"></i></button>
        </span>
        <span class="provider-feedback">
          <span class="speed-result inline" data-speed-for="${providerId}"></span>
          <span class="usage-result inline" data-usage-for="${providerId}"></span>
        </span>
      </article>
    `;
  }

  function providerPresetCardMarkup(preset, added = false) {
    const presetId = escapeHtml(preset.id);
    return `
      <button class="provider-switch-card preset-card ${added ? "added" : ""}" type="button" data-action="new-from-preset" data-preset="${presetId}" ${added ? "disabled" : ""}>
        <span class="drag-handle preset-plus"><i class="bi ${added ? "bi-check2" : "bi-plus-lg"}"></i></span>
        <span class="provider-logo">${iconMarkup(preset)}</span>
        <span class="provider-main"><strong>${escapeHtml(preset.name)}</strong><span class="truncate">${escapeHtml(preset.baseUrl)}</span></span>
        <span class="provider-meta">${escapeHtml(t(`apiFormatDisplay.${normalizeApiFormat(preset.apiFormat).key}.name`))}</span>
        <span class="provider-actions"><span class="compact-enable ghost"><i class="bi ${added ? "bi-check2" : "bi-plus-lg"}"></i><span>${added ? t("providers.added") : t("providers.add")}</span></span></span>
      </button>
    `;
  }

  function dashboardPresetSectionMarkup(providers, presets) {
    const available = presets.filter((preset) => !presetExists(preset, providers));
    if (!available.length) return "";
    const hint = t("dashboard.availablePresetsHint");
    return `
      <section class="dashboard-preset-section" aria-label="${escapeHtml(t("dashboard.availablePresets"))}">
        <div class="section-title-row compact">
          <div>
            <h2>${escapeHtml(t("dashboard.availablePresets"))}</h2>
            ${hint ? `<p>${escapeHtml(hint)}</p>` : ""}
          </div>
        </div>
        <div class="provider-preset-grid">
          ${available.map((preset) => providerPresetCardMarkup(preset)).join("")}
        </div>
      </section>
    `;
  }

  function getDragAfterElement(container, y) {
    const items = [...container.querySelectorAll("[data-provider-id]:not(.dragging)")];
    return items.reduce((closest, child) => {
      const box = child.getBoundingClientRect();
      const offset = y - box.top - box.height / 2;
      if (offset < 0 && offset > closest.offset) return { offset, element: child };
      return closest;
    }, { offset: Number.NEGATIVE_INFINITY, element: null }).element;
  }

  function enableProviderReorder(listEl) {
    if (!listEl || listEl.dataset.reorderBound === "1") return;
    listEl.dataset.reorderBound = "1";

    listEl.addEventListener("dragstart", (event) => {
      const card = event.target.closest("[data-provider-id]");
      if (!card) return;
      card.classList.add("dragging");
      event.dataTransfer.effectAllowed = "move";
      event.dataTransfer.setData("text/plain", card.dataset.providerId);
    });

    listEl.addEventListener("dragover", (event) => {
      const dragging = listEl.querySelector(".dragging");
      if (!dragging) return;
      event.preventDefault();
      const afterElement = getDragAfterElement(listEl, event.clientY);
      if (afterElement) {
        listEl.insertBefore(dragging, afterElement);
      } else {
        listEl.appendChild(dragging);
      }
    });

    listEl.addEventListener("drop", async (event) => {
      const dragging = listEl.querySelector(".dragging");
      if (!dragging) return;
      event.preventDefault();
      dragging.classList.remove("dragging");
      const providerIds = $all("[data-provider-id]", listEl).map((item) => item.dataset.providerId);
      try {
        await CCApi.reorderProviders(providerIds);
        showToast(t("toast.providersReordered"));
        await renderProviders();
        if (routeFromHash() === "dashboard") await renderDashboard();
      } catch (error) {
        console.error(error);
        if (routeFromHash() === "dashboard") {
          await renderDashboard();
        } else {
          await renderProviders();
        }
        showToast(error.message || t("toast.requestFailed"));
      }
    });

    listEl.addEventListener("dragend", (event) => {
      event.target.closest("[data-provider-id]")?.classList.remove("dragging");
    });
  }

  async function renderProviderCards(targetSelector, options = {}) {
    const target = $(targetSelector);
    if (!target) return;
    const providers = await CCApi.getProviders();
    if (!presetCache.length) presetCache = await CCApi.getPresets();
    const providerList = providers.length
      ? `<div class="provider-configured-list" data-provider-list>${providers.map(providerCardMarkup).join("")}</div>`
      : "";
    if (!providers.length) {
      target.innerHTML = `<div class="provider-preset-grid">${presetCache.map((preset) => providerPresetCardMarkup(preset)).join("")}</div>`;
      return;
    }
    if (options.includePresets) {
      target.innerHTML = `${providerList}${dashboardPresetSectionMarkup(providers, presetCache)}`;
    } else {
      target.innerHTML = providerList;
    }
    enableProviderReorder($("[data-provider-list]", target));
  }


  // ── Plugin Unlock 状态刷新 ──
  async function refreshPluginUnlockStatus() {
    try {
      const unlock = await CCAPI.pluginUnlock.status();
      const icon = $("#pluginUnlockIcon");
      const statusText = $("#pluginUnlockStatus");
      const actions = $("#pluginUnlockActions");
      if (!icon || !statusText) return;

      icon.classList.remove("muted", "success", "warning", "danger");
      const statusMap = {
        disconnected: { icon: "bi-lock", class: "muted", text: t("pluginUnlock.disconnected") || "未运行" },
        connecting: { icon: "bi-arrow-repeat", class: "warning", text: t("pluginUnlock.connecting") || "连接中..." },
        connected: { icon: "bi-plug", class: "warning", text: t("pluginUnlock.connected") || "已连接" },
        injected: { icon: "bi-unlock", class: "success", text: t("pluginUnlock.injected") || "已解锁" },
        failed: { icon: "bi-exclamation-triangle", class: "danger", text: unlock.message || "失败" },
      };
      const s = statusMap[unlock.status] || statusMap.disconnected;
      icon.innerHTML = `<i class="bi ${s.icon}"></i>`;
      icon.classList.add(s.class);
      statusText.classList.toggle("muted-text", s.class === "muted");
      statusText.textContent = s.text;
      if (actions) actions.style.display = unlock.status === "injected" || unlock.status === "connected" ? "block" : "none";
    } catch (e) {
      console.log("[PluginUnlock] status refresh failed:", e);
    }
  }

  async function renderDashboard() {
    const status = await CCApi.getStatus();
    const activities = await CCApi.getActivities();
    const health = status.desktopHealth || {};
    const desktopReady = status.desktopConfigured && !health.needsApply;
    await renderProviderCards("#dashboardProviderCards", { includePresets: true });
    const desktopIcon = $("#dashboardDesktopIcon");
    desktopIcon.classList.toggle("muted", !desktopReady);
    desktopIcon.innerHTML = `<i class="bi ${desktopReady ? "bi-check-lg" : "bi-exclamation-lg"}"></i>`;
    const desktopStatus = $("#dashboardDesktopStatus");
    desktopStatus.classList.toggle("muted-text", !desktopReady);
    desktopStatus.textContent = health.needsApply
      ? t("status.needsApply")
      : status.desktopConfigured ? t("status.configured") : t("status.notConfigured");
    renderDesktopHealthWarning("#dashboardDesktopWarning", health);
    // ── proxy 状态卡片:图标颜色 + 文字颜色跟随 running 状态 ──
    const proxyIcon = $("#dashboardProxyIcon");
    if (proxyIcon) {
      proxyIcon.classList.toggle("success", !!status.proxyRunning);
      proxyIcon.classList.toggle("muted", !status.proxyRunning);
      proxyIcon.innerHTML = status.proxyRunning
        ? '<i class="bi bi-hdd-network"></i><i class="bi bi-activity badge-icon"></i>'
        : '<i class="bi bi-hdd-network"></i>';
    }
    const proxyStatusEl = $("#dashboardProxyStatus");
    proxyStatusEl.textContent = status.proxyRunning ? `${t("status.running")} :${status.proxyPort}` : t("status.stopped");
    proxyStatusEl.classList.toggle("muted-text", !status.proxyRunning);
    $("#dashboardProviderName").textContent = status.activeProvider.name;
    // Plugin Unlock 状态刷新
    refreshPluginUnlockStatus();
    $("#activityList").innerHTML = activities.map((item) => (
      `<div class="activity-row"><time>${escapeHtml(item.time)}</time><span>${escapeHtml(item.text)}</span></div>`
    )).join("");
    await refreshUpdateBadge();
  }

  async function renderPresets() {
    presetCache = await CCApi.getPresets();
    $("#presetList").innerHTML = presetCache.map((preset) => {
      const active = selectedPreset?.id === preset.id;
      const isCustom = preset.id === "custom-third-party";
      const nameMarkup = isCustom
        ? `<strong data-i18n="providersAdd.customThirdPartyName">${escapeHtml(preset.name)}</strong>`
        : `<strong>${escapeHtml(preset.name)}</strong>`;
      const subText = isCustom
        ? `<span data-i18n="providersAdd.customThirdPartyHint">${escapeHtml(preset.baseUrlHint || "")}</span>`
        : `<span>${escapeHtml(preset.baseUrl)}</span>`;
      return `
      <button class="preset-item ${active ? "active" : ""}" type="button" data-preset="${escapeHtml(preset.id)}" aria-pressed="${active ? "true" : "false"}">
        <span class="preset-logo">${iconMarkup(preset)}</span>
        <span>${nameMarkup}${subText}</span>
        <i class="bi ${active ? "bi-check2" : "bi-chevron-right"}"></i>
      </button>
    `;
    }).join("");
  }

  function setProviderFormMode(titleKey) {
    const title = $("#page-providers-add .page-title h1");
    if (title) title.textContent = t(titleKey);
    const submit = $("#providerSaveOnly");
    if (submit) submit.textContent = t("common.saveOnly");
    const result = $("#formSpeedResult");
    if (result) {
      result.textContent = "";
      result.className = "speed-result";
    }
    const modelResult = $("#providerModelFetchResult");
    if (modelResult) modelResult.textContent = "";
  }

  function setApiKeyInputState(hasSavedKey = false, savedKey = "") {
    const input = $("#providerApiKey");
    const label = $("label[for='providerApiKey']");
    if (!input) return;
    input.type = "password";
    input.value = savedKey || "";
    input.required = !hasSavedKey && !savedKey;
    input.placeholder = (hasSavedKey || savedKey) ? t("providers.keySavedPlaceholder") : t("providers.keyPlaceholder");
    const toggle = $("[data-action='toggle-key']");
    if (toggle) toggle.innerHTML = '<i class="bi bi-eye"></i>';
    if (label) label.classList.toggle("required", input.required);
  }

  /// i18n template fill — 替代 ad-hoc `t(key).replace("{var}",val)`。fallback 行为:
  /// 1) t() 返 key 字符串(missing key)→ console.warn + return key (silent-failure
  ///    M1 修)。2) replace 后仍残留 `{var}` 占位 → console.warn 让 i18n 不全暴露
  function tFmt(key, vars = {}) {
    const tmpl = t(key);
    if (tmpl === key) {
      console.warn(`[i18n] missing key: ${key}`);
    }
    let out = tmpl;
    for (const [k, v] of Object.entries(vars)) {
      out = out.split(`{${k}}`).join(String(v ?? ""));
    }
    if (/\{[a-zA-Z_]+\}/.test(out)) {
      console.warn(`[i18n] unsubstituted placeholder in "${key}": ${out}`);
    }
    return out;
  }

  /// Cloud Code Assist OAuth row 切换:apiFormat 是 OAuth 类(gemini_cli_oauth /
  /// antigravity_oauth)时隐藏 apiKey input,显示 OAuth login button + status
  /// widget;其他 apiFormat 隐藏 OAuth row。调用时机:form 加载 / apiFormat
  /// select 切换。
  /// 内部 update activeOauthConfig 让后续 refresh/login/logout 走对的 provider
  /// R1 PR-7:apiFormat=grok_web 时显示 grok web cookie 输入 row,隐藏 apiKey
  /// 输入(grok_web 不用 apiKey,用 extra.grokWeb.{cookies, statsigId})。
  /// 与 setOauthRowState 互斥 — 调用方应先确保 apiFormat 解析后路由到对的一个。
  function setGrokWebRowState(apiFormat) {
    const row = $("#providerGrokWebRow");
    const apiKeyRow = $("#providerApiKeyRow");
    const apiKeyInput = $("#providerApiKey");
    const { canonical } = normalizeApiFormat(apiFormat);
    const isGrokWeb = canonical === "grok_web";
    if (row) row.hidden = !isGrokWeb;
    if (apiKeyRow) {
      // grok_web 隐藏 apiKey input;非 grok_web 显示(避免与 OAuth 状态冲突
      // —— setOauthRowState 自己控制 isOauth case 的 apiKey 可见性,所以
      // 我们**只在切换到/离开 grok_web 时操作**,其它情况让 OAuth/默认逻辑接管)
      if (isGrokWeb) {
        apiKeyRow.hidden = true;
      }
    }
    if (apiKeyInput && isGrokWeb) {
      // required 兜底:grok_web 不需要 apiKey 必填,否则浏览器 form validation 会
      // 卡住 submit(即使 input 被 hidden div 包着,required 仍校验)。
      //
      // **chatgpt-codex P1 修(2026-05-12)**:**不**在非 grok_web 时无条件设
      // `required = true` —— OAuth modes(gemini_cli_oauth / antigravity_oauth)
      // 由 setOauthRowState 自己管理 required(它走 dataset.origRequired
      // 保存/恢复机制,1308-1313 行),无条件覆盖会让 OAuth 模式 hidden apiKey
      // 仍 required → form submit 静默被浏览器拒。chain 调用顺序是
      // setOauthRowState → setGrokWebRowState,我们离开 grok_web 时**不动**,
      // 让上游 setter 的决定生效。
      apiKeyInput.required = false;
    }
    if (isGrokWeb) {
      const authEl = $("#providerAuth");
      if (authEl) authEl.value = "grok_cookie";
    }
  }

  /// 从 grok_web form input 收集 grokWeb extra payload(用于 POST 时打包到
  /// provider.extra.grokWeb)。
  ///
  /// Plan A:仅 sso 必填;sso-rw / cf_clearance / statsigId / userAgent 都 optional
  /// (后端 auth.rs 缺失时分别复用 sso / 跳过 segment / 动态生成 / 用默认 UA)。
  ///
  /// 返回 null 表示不是 grok_web 模式或 input 全空(编辑模式留空 = 保留现值)。
  function collectGrokWebPayload() {
    const row = $("#providerGrokWebRow");
    if (!row || row.hidden) return null;
    const sso = $("#grokWebSso")?.value.trim() || "";
    const ssoRw = $("#grokWebSsoRw")?.value.trim() || "";
    const cf = $("#grokWebCfClearance")?.value.trim() || "";
    const cookieString = $("#grokWebCookieString")?.value.trim() || "";
    const statsigId = $("#grokWebStatsigId")?.value.trim() || "";
    const userAgent = $("#grokWebUserAgent")?.value.trim() || "";
    if (!sso && !ssoRw && !cf && !cookieString && !statsigId && !userAgent)
      return null;
    const cookies = { sso };
    if (ssoRw) cookies["sso-rw"] = ssoRw;
    if (cf) cookies.cf_clearance = cf;
    if (cookieString) cookies.cookieString = cookieString;
    const payload = { cookies };
    if (statsigId) payload.statsigId = statsigId;
    if (userAgent) payload.userAgent = userAgent;
    return payload;
  }

  /// 编辑现有 provider 时初始化 grok_web form。
  ///
  /// 后端 public_provider 已把 grokWeb 字段 mask 出去,只保留 `hasGrokWeb: bool`
  /// (cookies + statsigId 是高敏感凭证,跟 apiKey 一样不回传前端)。所以这里:
  ///   - 清空 input 值
  ///   - hasGrokWeb=true 时给 placeholder 提示"已保存凭证,留空则保持不变"
  ///   - 用户若真要替换才填新值,save 时 collectGrokWebPayload 返回新对象;
  ///     若空白 save → payload 不带 grokWeb → 后端 update_provider 不动现值
  function fillGrokWebFormFromProvider(provider) {
    const hasGrokWeb = !!provider?.hasGrokWeb;
    const ids = [
      "grokWebSso",
      "grokWebSsoRw",
      "grokWebCfClearance",
      "grokWebCookieString",
      "grokWebStatsigId",
      "grokWebUserAgent",
    ];
    const savedPlaceholder = t("grokWeb.savedPlaceholder") || "已保存,留空则保持";
    for (const id of ids) {
      const el = $(`#${id}`);
      if (!el) continue;
      el.value = "";
      el.placeholder = hasGrokWeb ? savedPlaceholder : "";
    }
  }

  function setOauthRowState(apiFormat) {
    const oauthRow = $("#providerOauthRow");
    const apiKeyRow = $("#providerApiKeyRow");
    const apiKeyInput = $("#providerApiKey");
    const baseUrlRow = $("#providerBaseUrlRow");
    const baseUrlInput = $("#providerBaseUrl");
    const { canonical } = normalizeApiFormat(apiFormat);
    const config = OAUTH_PROVIDER_CONFIGS[canonical] || null;
    activeOauthConfig = config;
    const isOauth = !!config;
    if (oauthRow) oauthRow.hidden = !isOauth;
    if (apiKeyRow) apiKeyRow.hidden = isOauth;
    // OAuth 模式 baseUrl 由 preset 写死(cloudcode-pa.googleapis.com),
    // user 不需要看 / 改;切回非 OAuth 显示
    if (baseUrlRow) baseUrlRow.hidden = isOauth;
    // **silent-failure-hunter H3 修**:原 `required = !isOauth && req` 单调毁掉
    // required 字段(切 OAuth 后 required=false,切回 openai_chat 仍 false)。改用
    // dataset.origRequired 缓存,switch 回非 OAuth 时恢复
    if (apiKeyInput) {
      if (apiKeyInput.dataset.origRequired === undefined) {
        apiKeyInput.dataset.origRequired = apiKeyInput.required ? "1" : "0";
      }
      if (isOauth) {
        apiKeyInput.required = false;
      } else {
        apiKeyInput.required = apiKeyInput.dataset.origRequired === "1";
      }
    }
    // baseUrl 同样的 required cache 处理 — OAuth 时解 required 防表单提交卡住
    if (baseUrlInput) {
      if (baseUrlInput.dataset.origRequired === undefined) {
        baseUrlInput.dataset.origRequired = baseUrlInput.required ? "1" : "0";
      }
      baseUrlInput.required = isOauth ? false : baseUrlInput.dataset.origRequired === "1";
      // **silent-failure-hunter H2 修**:hide 行不等于清 value。OAuth 模式下
      // baseUrl 由 preset 写死(cloudcode-pa.googleapis.com),user 切换 preset
      // 时残留的旧 value 可能跟着 form submit 上去让 backend 用错 endpoint。
      // 强制锁定 value 防止 hidden field 数据漂移
      if (isOauth) {
        baseUrlInput.value = "https://cloudcode-pa.googleapis.com";
      }
    }
    if (isOauth && config) {
      // 切换 provider 时,把 OAuth row 内**全部**静态 i18n 节点的 data-i18n key
      // 重写到当前 provider namespace。语言切换时 i18n.applyTranslations 会读
      // dataset.i18n 拿对的本地化 — 缺写就 stale 显错 provider 文案。
      // refreshOauthStatusUi 异步,resolve 前 user 切语言会撞旧 key,所以这里
      // 4 个节点全部同步重写一次(label / loginBtn / logoutBtn / statusText)
      const k = (suffix) => `${config.i18nPrefix}.${suffix}`;
      const label = $("#providerOauthRow > label.form-label");
      if (label) {
        label.dataset.i18n = k("title");
        label.textContent = tFmt(k("title"));
      }
      const loginBtn = $("#oauthLoginBtn");
      if (loginBtn) {
        loginBtn.dataset.i18n = k("loginBtn");
        loginBtn.textContent = tFmt(k("loginBtn"));
      }
      const logoutBtn = $("#oauthLogoutBtn");
      if (logoutBtn) {
        logoutBtn.dataset.i18n = k("logoutBtn");
        logoutBtn.textContent = tFmt(k("logoutBtn"));
      }
      const statusEl = $("#oauthStatusText");
      if (statusEl) {
        statusEl.dataset.i18n = k("statusLoading");
        statusEl.textContent = tFmt(k("statusLoading"));
        statusEl.classList.remove("text-warning");
      }
      refreshOauthStatusUi().catch((e) => {
        console.error("refresh oauth status failed:", e);
      });
    }
  }

  /// 调 GET /api/<provider>-oauth/status 同步 UI 状态(已登录 / 未登录 / partial)。
  /// 错误路径完整:清旧 i18n key + 复位 button visibility + structured error message
  /// (silent-failure-hunter C1 修)。i18n key 前缀按 activeOauthConfig 切换,
  /// 让 gemini-cli vs antigravity 用各自文案 namespace。
  ///
  /// **race 安全**(2026-05-11 codex-connector P2):入口 snapshot `activeOauthConfig`,
  /// await 后**identity check** 才动 DOM。否则 user 在 status fetch 飞行中切 provider
  /// (eg gemini-cli ↔ antigravity 互切),旧 provider 的延迟 response 会覆盖
  /// 新 provider UI,显错登录状态。同 handleOauthLogin/Logout 的 race fix 模式
  async function refreshOauthStatusUi() {
    const statusEl = $("#oauthStatusText");
    const loginBtn = $("#oauthLoginBtn");
    const logoutBtn = $("#oauthLogoutBtn");
    if (!statusEl) return;
    const config = activeOauthConfig;
    if (!config) return; // OAuth row 隐藏中,不刷新
    const k = (suffix) => `${config.i18nPrefix}.${suffix}`;
    try {
      const status = await config.api.getStatus();
      // **post-await identity check**:status 拿回时 user 可能已切到别 provider /
      // 关 OAuth row,这条 response 是过时数据,不该污染新 UI 上下文
      if (activeOauthConfig !== config) {
        return;
      }
      if (!status.loggedIn) {
        statusEl.dataset.i18n = k("statusNotLoggedIn");
        statusEl.textContent = tFmt(k("statusNotLoggedIn"));
        statusEl.classList.remove("text-warning");
        if (logoutBtn) logoutBtn.hidden = true;
        if (loginBtn) {
          loginBtn.hidden = false;
          loginBtn.dataset.i18n = k("loginBtn");
          loginBtn.textContent = tFmt(k("loginBtn"));
        }
      } else {
        const expiresStr = status.expiresAt
          ? new Date(status.expiresAt).toLocaleString()
          : "?";
        const tmplKey = status.projectId
          ? k("statusLoggedIn")
          : k("statusLoggedInNoProject");
        statusEl.dataset.i18n = tmplKey;
        statusEl.textContent = tFmt(tmplKey, {
          email: status.email || "?",
          projectId: status.projectId || "?",
          expiresAt: expiresStr,
        });
        // partial state(无 projectId)加 visual cue(M3 修)
        statusEl.classList.toggle("text-warning", !status.projectId);
        if (logoutBtn) logoutBtn.hidden = false;
        if (loginBtn) {
          loginBtn.hidden = false;
          // 已登录时改 button 文案为"切换账号"(reviewer #2 修)
          loginBtn.dataset.i18n = k("switchAccountBtn");
          loginBtn.textContent = tFmt(k("switchAccountBtn"));
        }
      }
    } catch (e) {
      // **catch 路径同 identity check**:fetch reject 后 user 可能也已切 provider
      if (activeOauthConfig !== config) {
        return;
      }
      const msg = e?.message || String(e);
      statusEl.dataset.i18n = k("statusFetchFailed");
      statusEl.textContent = tFmt(k("statusFetchFailed"), { error: msg });
      statusEl.classList.add("text-warning");
      // catch 路径**复位** button visibility 防 stale(C1 修)
      if (loginBtn) {
        loginBtn.hidden = false;
        loginBtn.dataset.i18n = k("loginBtn");
        loginBtn.textContent = tFmt(k("loginBtn"));
      }
      if (logoutBtn) logoutBtn.hidden = true;
    }
  }

  /// OAuth login click handler — long-poll 等浏览器授权 + bootstrap project。
  /// **silent-failure-hunter C2 修**:fetch 自带 timeout 风险(浏览器 ~ 90-300s vs
  /// 后端 5min),browser timeout 后端继续成功 → frontend 看 fail 但 status 看 success
  /// 矛盾 UX。修法:catch 不显 "failed" toast,而是显"timeout 等 status 刷新"+ refresh。
  /// 走 activeOauthConfig 拿对应 provider(gemini-cli vs antigravity)的 API + i18n
  async function handleOauthLogin() {
    const config = activeOauthConfig;
    if (!config) {
      // OAuth row 在隐藏状态下被点(button stale / e2e replay / 罕见 race)。
      // 不能 silent return —— user 看 click 没反应会怀疑 app hang。toast 给出提示
      console.warn("handleOauthLogin called with no active oauth config");
      showToast("OAuth login skipped: no active OAuth provider in form (switch apiFormat first)");
      return;
    }
    const k = (suffix) => `${config.i18nPrefix}.${suffix}`;
    const loginBtn = $("#oauthLoginBtn");
    const logoutBtn = $("#oauthLogoutBtn");
    if (loginBtn) {
      loginBtn.disabled = true;
      loginBtn.textContent = tFmt(k("loginBtnInProgress"));
    }
    if (logoutBtn) logoutBtn.disabled = true;
    try {
      const result = await config.api.login();
      if (result.loggedIn && result.projectId) {
        showToast(tFmt(k("loginSuccess"), {
          email: result.email || "?",
          projectId: result.projectId,
        }));
      } else if (result.loggedIn && !result.projectId) {
        // partial state — token 不该在此分支被持久化(后端 commit C C2 修),但 UI 防御
        showToast(tFmt(k("loginPartial")));
      } else if (result.error) {
        showToast(tFmt(k("loginFailed"), { error: result.error }));
      } else {
        // 未知 shape — 把整个 response 序列化进 toast 给 user 看到(silent H1 修)
        const dump = JSON.stringify(result).slice(0, 200);
        console.error("OAuth login unknown response shape:", result);
        showToast(tFmt(k("loginFailed"), { error: `unknown response: ${dump}` }));
      }
    } catch (e) {
      // C2:浏览器 fetch timeout 而后端可能仍成功 — 不显 "failed",改提示"refreshing"
      const msg = e?.message || String(e);
      console.warn("OAuth login fetch error (backend may have succeeded):", msg);
      showToast(`Login fetch interrupted (${msg}); refreshing status...`);
    } finally {
      if (loginBtn) loginBtn.disabled = false;
      if (logoutBtn) logoutBtn.disabled = false;
      // **silent-failure C1+C2 修**:long-poll 期间 user 可能切到非 OAuth /
      // 另一 OAuth provider,activeOauthConfig 已变。这种情况 refresh 会画错
      // provider 的状态进 row(toast 已给确认)— 跳过 refresh。回到原 provider
      // 时 setOauthRowState 会重新 fetch 一次 status,UI 收敛
      if (activeOauthConfig === config) {
        await refreshOauthStatusUi();
      }
    }
  }

  /// Logout click handler。**silent-failure-hunter H2 修**:logout 失败时显手动删
  /// 提示而不是 blindly refresh status (那会让 user 看 logged in 没意识 token 还在)。
  /// 走 activeOauthConfig 拿对应 provider 的 API + i18n
  async function handleOauthLogout() {
    const config = activeOauthConfig;
    if (!config) {
      console.warn("handleOauthLogout called with no active oauth config");
      showToast("OAuth logout skipped: no active OAuth provider in form");
      return;
    }
    const k = (suffix) => `${config.i18nPrefix}.${suffix}`;
    let failed = false;
    try {
      await config.api.logout();
      showToast(tFmt(k("logoutConfirmed")));
    } catch (e) {
      failed = true;
      const msg = e?.message || String(e);
      showToast(tFmt(k("logoutFailedManual"), { error: msg }));
      const statusEl = $("#oauthStatusText");
      if (statusEl) {
        statusEl.dataset.i18n = k("logoutFailedManual");
        statusEl.textContent = tFmt(k("logoutFailedManual"), { error: msg });
        statusEl.classList.add("text-warning");
      }
    } finally {
      // 失败时不刷新 status — 防覆盖 manual-delete 警告。成功时刷新让 UI 转 "未登录"。
      // **silent-failure C1+C2 修**:logout 长 poll 罕见但 race 同样适用 —
      // 切到别 provider 时不画旧 provider 的状态进 row
      if (!failed && activeOauthConfig === config) {
        await refreshOauthStatusUi();
      }
    }
  }

  function isVerifiedProviderId(id) {
    const value = String(id || "").toLowerCase();
    if (value === "kimi" || value === "kimi-code" || value.startsWith("kimi-")) return true;
    if (value === "xiaomi-mimo-token-plan" || value === "xiaomi-mimo-payg") return true;
    if (value === "deepseek") return true;
    if (value === "gemini-cli-oauth") return true;
    if (value === "antigravity-oauth") return true;
    return false;
  }

  function setUnverifiedBanner(show) {
    const banner = $("#providerUnverifiedBanner");
    if (banner) banner.hidden = !show;
  }

  function resetProviderForm() {
    editingProviderId = null;
    selectedPreset = null;
    providerAvailableModels = [];
    baseUrlMenuOpen = false;
    renderPresetOptions(null);
    updatePresetSelection();
    formModelCapabilities = {};
    formRequestOptions = {};
    setProviderFormMode("providersAdd.title");
    $("#providerName").value = "";
    $("#providerName").placeholder = "";
    $("#providerBaseUrl").value = "";
    $("#providerBaseUrl").placeholder = "";
    $("#providerBaseUrl").disabled = false;
    const trigger = $("#providerBaseUrlTrigger");
    if (trigger) trigger.hidden = true;
    renderBaseUrlOptions(null);
    setApiKeyInputState(false);
    $("#providerAuth").value = "bearer";
    renderApiFormatDisplay("openai_chat");
    setApiFormatMode(false, "openai_chat");
    setOauthRowState("openai_chat"); // P2.2 reset OAuth row 隐藏
    setGrokWebRowState("openai_chat"); // R1 PR-7 reset grok_web row 隐藏
    fillGrokWebFormFromProvider(null);
    setWebSearchRow(false, false, null);
    setProviderMappings(emptyMappings());
    setUnverifiedBanner(false);
  }

  function applyPresetToForm(preset, notify = true) {
    // 自定义第三方:不预填 name/baseUrl(用户必须自己填),用 placeholder 提示
    // builtin preset:直接预填 name + baseUrl,用户保存即可
    const isCustom = preset.id === "custom-third-party";
    if (isCustom) {
      $("#providerName").value = "";
      $("#providerName").placeholder = preset.name;
      $("#providerBaseUrl").value = "";
      $("#providerBaseUrl").placeholder = "https://api.example.com/v1";
    } else {
      $("#providerName").value = preset.name;
      $("#providerName").placeholder = "";
      $("#providerBaseUrl").value = preset.baseUrl;
      $("#providerBaseUrl").placeholder = "";
    }
    $("#providerBaseUrl").disabled = false;
    const trigger = $("#providerBaseUrlTrigger");
    if (trigger) trigger.hidden = true;
    baseUrlMenuOpen = false;
    renderBaseUrlOptions(preset);
    setAuthSchemeValue(preset.authScheme);
    setApiKeyInputState(false);
    selectedPreset = preset;
    renderApiFormatDisplay(preset.apiFormat);
    setApiFormatMode(!!preset.allowApiFormatSelection, preset.apiFormat);
    setOauthRowState(preset.apiFormat); // P2.2 OAuth UI 切换
    setGrokWebRowState(preset.apiFormat); // R1 PR-7 grok_web UI 切换
    formModelCapabilities = normalizeCapabilities(preset.modelCapabilities || {});
    formRequestOptions = normalizeRequestOptions(preset.requestOptions || {});
    // Web Search 配置开关:preset 标支持 + preset.requestOptions.web_search_enabled
    // 决定初始 checkbox state(kimi / kimi-code 默认 true,xiaomi-mimo-* 默认 false,
    // 跟 backend `provider_web_search_enabled` 读取契约一致);hint 文案按
    // preset.id 选 provider-specific 段落
    setWebSearchRow(
      !!preset.supportsWebSearch,
      !!formRequestOptions.web_search_enabled,
      preset.id
    );
    providerAvailableModels = [];
    setProviderMappings(preset.models || emptyMappings());
    renderPresetOptions(preset, preset.models || emptyMappings());
    updatePresetSelection();
    setUnverifiedBanner(!isVerifiedProviderId(preset.id));
    if (notify) showToast(`${preset.name} ${t("toast.presetFilled")}`);
  }

  async function fillProviderForEdit(providerId) {
    const providers = await CCApi.getProviders();
    const provider = providers.find((item) => item.id === providerId);
    if (!provider) return;
    editingProviderId = provider.id;
    const matchedPreset = presetCache.find((preset) => presetMatchesProvider(preset, provider));
    selectedPreset = matchedPreset
      ? { ...matchedPreset, extraHeaders: provider.extraHeaders || matchedPreset.extraHeaders || {} }
      : {
        models: provider.mappings,
        extraHeaders: provider.extraHeaders || {},
        modelCapabilities: provider.modelCapabilities || {},
        requestOptions: provider.requestOptions || {},
      };
    formModelCapabilities = normalizeCapabilities(provider.modelCapabilities || selectedPreset.modelCapabilities || {});
    formRequestOptions = normalizeRequestOptions(provider.requestOptions || selectedPreset.requestOptions || {});
    setProviderFormMode("providersAdd.editTitle");
    $("#providerName").value = provider.name;
    $("#providerName").placeholder = "";
    $("#providerBaseUrl").value = provider.baseUrl;
    $("#providerBaseUrl").placeholder = "";
    // 内置 provider 不允许修改 baseUrl
    $("#providerBaseUrl").disabled = !!provider.isBuiltin;
    const baseUrlTrigger = $("#providerBaseUrlTrigger");
    if (baseUrlTrigger) baseUrlTrigger.hidden = !!provider.isBuiltin;
    baseUrlMenuOpen = false;
    renderBaseUrlOptions(selectedPreset);
    setApiKeyInputState(provider.hasApiKey);
    if (provider.hasApiKey) {
      try {
        const secret = await CCApi.getProviderSecret(provider.id);
        setApiKeyInputState(true, secret.apiKey || "");
      } catch (error) {
        console.error(error);
        showToast(error.message || t("toast.requestFailed"));
      }
    }
    setAuthSchemeValue(provider.authScheme);
    const effectiveFormat = (matchedPreset && matchedPreset.apiFormat) || provider.apiFormat;
    renderApiFormatDisplay(effectiveFormat);
    setApiFormatMode(false, effectiveFormat);
    setOauthRowState(effectiveFormat); // OAuth UI 切换(P2.2)
    setGrokWebRowState(effectiveFormat); // R1 PR-7 grok_web UI 切换
    fillGrokWebFormFromProvider(provider);
    // 编辑场景:支持判定走 matchedPreset.supportsWebSearch(自定义 provider 不命中
    // builtin → matchedPreset undefined → 不显示开关);初始 checkbox state 读
    // provider 实际保存的 requestOptions.web_search_enabled;hint 文案按
    // matchedPreset.id(自定义 provider 时 fallback 到 .default 通用文案)
    setWebSearchRow(
      !!(matchedPreset && matchedPreset.supportsWebSearch),
      !!formRequestOptions.web_search_enabled,
      matchedPreset?.id || null
    );
    providerAvailableModels = [];
    setProviderMappings(provider.mappings || emptyMappings());
    renderPresetOptions(selectedPreset, provider.mappings || emptyMappings());
    updatePresetSelection();
    setUnverifiedBanner(!isVerifiedProviderId(matchedPreset?.id || provider.id));
  }

  async function renderProviderForm() {
    await renderPresets();
    if (editingProviderId) {
      await fillProviderForEdit(editingProviderId);
      return;
    }
    if (selectedPreset) {
      setProviderFormMode("providersAdd.title");
      applyPresetToForm(selectedPreset, false);
      return;
    }
    resetProviderForm();
  }

  async function renderProviders() {
    await renderModelMenuModePanel();
    await renderProviderCards("#providerRows");
  }

  function renderModelMenuModeState(settings = {}) {
    const enabled = !!settings.exposeAllProviderModels;
    const button = $("#modelMenuModeToggle");
    const hint = $("#modelMenuModeHint");
    if (button) {
      button.classList.toggle("btn-primary", enabled);
      button.classList.toggle("btn-outline-primary", !enabled);
      const span = $("span", button);
      if (span) span.textContent = enabled ? t("providers.showSingleModel") : t("providers.showAllModels");
      button.setAttribute("aria-pressed", enabled ? "true" : "false");
    }
    if (hint) {
      hint.textContent = enabled ? t("providers.modelMenuAllHint") : t("providers.modelMenuSingleHint");
    }
    const settingToggle = $("#exposeAllProviderModels");
    if (settingToggle) settingToggle.checked = enabled;
  }

  async function renderModelMenuModePanel() {
    const settings = await CCApi.getSettings();
    renderModelMenuModeState(settings);
  }

  async function renderModelSelectors() {
    const providers = await CCApi.getProviders();
    const select = $("#modelProvider");
    select.innerHTML = providers.map((provider) => `<option value="${escapeHtml(provider.id)}">${escapeHtml(provider.name)}</option>`).join("");
    const active = providers.find((provider) => provider.default) || providers[0];
    if (active) select.value = active.id;
    renderMappingCards();
  }

  async function renderMappingCards() {
    const providers = await CCApi.getProviders();
    const provider = providers.find((item) => item.id === $("#modelProvider").value) || providers[0];
    if (!provider) return;
    const defaultSelect = $("#defaultModel");
    if (defaultSelect) {
      const defaultValue = provider.mappings.default || provider.mappings.gpt_5_5 || "";
      const defaultKey = providerFormModelSlots.find((slot) => provider.mappings[slot.key] === defaultValue)?.key || "gpt_5_5";
      defaultSelect.value = defaultKey;
    }
    const result = $("#modelFetchResult");
    if (result) result.textContent = "";
    $("#mappingStack").innerHTML = providerFormModelSlots.slice(1).map((slot) => `
      <article class="mapping-card">
        <div class="mapping-title">
          <span class="mapping-icon ${slot.iconClass}"><i class="bi ${slot.icon}"></i></span>
          <strong>${slot.label}</strong>
          <span class="alias-pill">${slot.label}</span>
        </div>
        <input class="form-control form-control-lg" data-model-input="${slot.key}" value="${escapeHtml(provider.mappings[slot.key] || "")}">
        <span class="source-model"><i class="bi bi-arrow-left"></i>${slot.source}</span>
      </article>
    `).join("");
  }

  async function renderDesktop() {
    const desktop = await CCApi.getDesktopStatus();
    const entries = Object.entries(desktop.config || {});
    const health = desktop.health || {};
    const desktopReady = desktop.configured && !health.needsApply;
    const statusText = $("#desktopConfiguredText");
    statusText.textContent = health.needsApply
      ? t("status.needsApply")
      : desktop.configured ? t("status.configured") : t("status.notConfigured");
    statusText.classList.toggle("muted-text", !desktopReady);
    $(".desktop-card .circle-check")?.classList.toggle("warning", !desktopReady);
    renderDesktopHealthWarning("#desktopPageWarning", health);
    $("#desktopConfigList").innerHTML = entries.map(([key, value]) => `
      <div class="config-row"><i class="bi bi-check-circle-fill"></i><span>${escapeHtml(key)}:</span><code>${escapeHtml(Array.isArray(value) ? JSON.stringify(value) : value)}</code></div>
    `).join("");
    // Show env config commands instead of raw JSON
    const cmdBlock = desktop.commands?.temporary || JSON.stringify(desktop.config, null, 2);
    $("#desktopJson").textContent = cmdBlock;
  }

  let proxyLogTimer = null;
  let proxyLogInflight = false;
  let proxyLogAtBottom = true;
  const PROXY_LOG_BOTTOM_TOLERANCE = 8;

  function isProxyLogAtBottom(el) {
    return el.scrollTop + el.clientHeight >= el.scrollHeight - PROXY_LOG_BOTTOM_TOLERANCE;
  }

  function bindProxyLogScroll() {
    const logEl = $("#proxyLog");
    if (!logEl || logEl.dataset.scrollBound === "1") return;
    logEl.dataset.scrollBound = "1";
    logEl.addEventListener("scroll", () => {
      proxyLogAtBottom = isProxyLogAtBottom(logEl);
    }, { passive: true });
  }

  async function refreshProxyLog() {
    if (proxyLogInflight) return;
    const logEl = $("#proxyLog");
    if (!logEl) return;
    proxyLogInflight = true;
    try {
      const [proxyStatus, logs] = await Promise.all([
        CCApi.getProxyStatus(),
        CCApi.getProxyLogs(),
      ]);
      const wasAtBottom = proxyLogAtBottom;
      const prevScrollTop = logEl.scrollTop;
      logEl.innerHTML = logs.map((line) => `
        <div class="log-line"><span>${escapeHtml(line.at)}</span><span class="log-level ${escapeHtml(line.level)}">${escapeHtml(line.level.toUpperCase())}</span><span>${escapeHtml(line.message)}</span></div>
      `).join("");
      const userToggleOn = $("#autoScroll")?.checked !== false;
      if (userToggleOn && wasAtBottom) {
        logEl.scrollTop = logEl.scrollHeight;
        proxyLogAtBottom = true;
      } else {
        logEl.scrollTop = prevScrollTop;
        proxyLogAtBottom = isProxyLogAtBottom(logEl);
      }
      const statsEl = $("#proxyStats");
      if (statsEl) {
        const stats = [
          { label: t("proxy.stats.total"), value: proxyStatus.stats.total, icon: "bi-list-ul" },
          { label: t("proxy.stats.success"), value: proxyStatus.stats.success, icon: "bi-check-circle" },
          { label: t("proxy.stats.failed"), value: proxyStatus.stats.failed, icon: "bi-x-circle", danger: true },
          { label: t("proxy.stats.today"), value: proxyStatus.stats.today, icon: "bi-calendar3" },
        ];
        statsEl.innerHTML = stats.map((stat) => `
          <article class="stat-card ${stat.danger ? "danger" : ""}"><i class="bi ${stat.icon}"></i><div><span>${stat.label}</span><strong>${stat.value}</strong></div></article>
        `).join("");
      }
    } catch (err) {
      // 静默吞掉单次轮询失败，避免在控制台刷错误
    } finally {
      proxyLogInflight = false;
    }
  }

  function stopProxyLogAutoRefresh() {
    if (proxyLogTimer !== null) {
      clearInterval(proxyLogTimer);
      proxyLogTimer = null;
    }
    proxyLogAtBottom = true;
  }

  function startProxyLogAutoRefresh() {
    stopProxyLogAutoRefresh();
    proxyLogTimer = setInterval(() => {
      if (document.visibilityState === "hidden") return;
      refreshProxyLog();
    }, 2000);
  }

  async function renderProxy() {
    const status = await CCApi.getStatus();
    $("#proxyPort").value = status.proxyPort;
    $("#settingsProxyPort").value = status.proxyPort;
    $("#proxyStateText").textContent = status.proxyRunning ? t("status.running") : t("status.stopped");
    // ── 停止态视觉反馈:pulse-dot 灰色 + 状态文字灰色 ──
    const proxyRunningEl = document.querySelector(".proxy-running");
    if (proxyRunningEl) proxyRunningEl.classList.toggle("stopped", !status.proxyRunning);
    // ── toggle 按钮:running → Stop(danger),stopped → Start(success) ──
    const toggleBtn = $("#proxyToggleBtn");
    if (toggleBtn) {
      if (status.proxyRunning) {
        toggleBtn.className = "btn btn-danger btn-lg";
        toggleBtn.innerHTML = `<i class="bi bi-stop-fill"></i><span>${t("proxy.stop")}</span>`;
      } else {
        toggleBtn.className = "btn btn-success btn-lg";
        toggleBtn.innerHTML = `<i class="bi bi-play"></i><span>${t("proxy.start")}</span>`;
      }
    }
    bindProxyLogScroll();
    proxyLogAtBottom = true;
    await refreshProxyLog();
    startProxyLogAutoRefresh();
  }

  async function renderSettings() {
    const settings = await CCApi.getSettings();
    applyTheme(settings.theme || "default");
    $("#settingsProxyPort").value = settings.proxyPort;
    $("#settingsAdminPort").value = settings.adminPort;
    $("#autoApplyOnStart").checked = settings.autoApplyOnStart !== false;
   $("#autoUnlockCodexPlugins").checked = !!settings.autoUnlockCodexPlugins;
    $("#autoWakeCodexPet").checked = settings.autoWakeCodexPet !== false;
   $("#exposeAllProviderModels").checked = !!settings.exposeAllProviderModels;
    $("#restoreCodexOnExit").checked = settings.restoreCodexOnExit !== false;
    $("#settingsUpdateUrl").value = settings.updateUrl || "";
    renderModelMenuModeState(settings);
    await refreshAppVersion();
    await refreshBackupList();
    await refreshCodexSnapshotStatus();
  }

  async function refreshAppVersion() {
    const target = $("#appVersion");
    if (!target) return;
    try {
      const payload = await CCApi.getVersion();
      if (payload && payload.version) {
        target.textContent = payload.version;
      }
    } catch (error) {
      console.warn("Failed to load app version", error);
    }
  }

  async function refreshCodexSnapshotStatus() {
    const target = $("#codexSnapshotStatus");
    if (!target) return;
    try {
      const status = await CCApi.getDesktopSnapshotStatus();
      if (status && status.hasSnapshot) {
        target.textContent = tFmt("settings.codexSnapshotStatusActive", {
          time: status.snapshotAt || "",
        });
      } else if (status && status.restorableCount > 0) {
        target.textContent = tFmt("settings.codexSnapshotStatusRecovery", {
          count: status.restorableCount,
        });
      } else {
        target.textContent = t("settings.codexSnapshotStatusEmpty");
      }
    } catch (error) {
      target.textContent = t("settings.codexSnapshotStatusEmpty");
    }
  }

  function formatCodexSnapshotChoice(snapshot, index) {
    const kind = t(`settings.codexSnapshotKind.${snapshot.kind || "unknown"}`);
    const provider = snapshot.providerName || t("settings.codexSnapshotProviderUnknown");
    const time = snapshot.snapshotAt || t("settings.codexSnapshotTimeUnknown");
    const version = snapshot.appVersion || t("settings.codexSnapshotVersionUnknown");
    const files = [
      snapshot.configExisted ? "config.toml" : null,
      snapshot.authExisted ? "auth.json" : null,
    ].filter(Boolean).join(" + ") || t("settings.codexSnapshotFilesNone");
    return `${index + 1}. ${time} | ${kind} | ${provider} | ${version} | ${files}`;
  }

  async function chooseCodexRestoreTarget() {
    const snapshots = await CCApi.getDesktopSnapshots();
    if (!snapshots.length) {
      return window.confirm(t("confirm.desktopClearFallback")) ? { fallback: true } : null;
    }
    if (snapshots.length === 1) {
      const summary = formatCodexSnapshotChoice(snapshots[0], 0);
      return window.confirm(tFmt("confirm.desktopSnapshotRestoreSingle", { summary }))
        ? { snapshotId: snapshots[0].id }
        : null;
    }
    const list = snapshots.map(formatCodexSnapshotChoice).join("\n");
    const input = window.prompt(tFmt("confirm.desktopSnapshotSelect", { list }));
    if (input === null) return null;
    const selectedIndex = Number.parseInt(String(input).trim(), 10) - 1;
    if (!Number.isInteger(selectedIndex) || selectedIndex < 0 || selectedIndex >= snapshots.length) {
      showToast(t("toast.desktopSnapshotInvalid"));
      return null;
    }
    const summary = formatCodexSnapshotChoice(snapshots[selectedIndex], selectedIndex);
    if (!window.confirm(tFmt("confirm.desktopSnapshotRestoreSelected", { summary }))) return null;
    return { snapshotId: snapshots[selectedIndex].id };
  }

  async function renderRoute(route) {
    $all(".page").forEach((page) => page.classList.toggle("active", page.dataset.page === route));
    $all(".route-tab").forEach((tab) => {
      const key = route.startsWith("providers") ? "providers" : route;
      tab.classList.toggle("active", tab.dataset.nav === key);
    });
    if (route !== "proxy") stopProxyLogAutoRefresh();
    if (route === "dashboard") await renderDashboard();
    if (route === "providers/add") await renderProviderForm();
    if (route === "providers") await renderProviders();
    if (route === "desktop") await renderDesktop();
    if (route === "claude-desktop") await renderClaudeDesktop();
    if (route === "claude-desktop-providers") await renderClaudeDesktopProviders();
    if (route === "claude-desktop-providers/add") await renderClaudeDesktopProviderForm();
    if (route === "proxy") await renderProxy();
    if (route === "settings") await renderSettings();
  }

  // ─── Claude Desktop tab(macOS apply / status / preset → API Key 形式)─────
  let claudeDesktopState = { presets: [], providers: [], activeProvider: null };

  async function renderClaudeDesktop() {
    const status = await CCApi.getClaudeDesktopStatus().catch(() => null);
    const data = await CCApi.listClaudeDesktopProviders().catch(() => ({
      activeProvider: null,
      providers: [],
      presets: [],
    }));
    claudeDesktopState = {
      activeProvider: data.activeProvider,
      providers: data.providers || [],
      presets: data.presets || [],
    };

    // Configured row
    const row = $("#claudeDesktopConfiguredRow");
    const text = $("#claudeDesktopConfiguredText");
    const managed = !!(status && status.desktopStatus && status.desktopStatus.managedByUs);
    if (row && text) {
      row.querySelector("span").innerHTML = managed
        ? '<i class="bi bi-check-lg"></i>'
        : '<i class="bi bi-dash-lg"></i>';
      text.textContent = managed ? "已由本工具配置" : "未配置";
    }

    // Provider select(presets only — full CRUD UI 待后续 stacked PR)
    const select = $("#claudeDesktopProviderSelect");
    if (select) {
      const presets = claudeDesktopState.presets || [];
      select.innerHTML = presets
        .map((p) => {
          const selected = claudeDesktopState.activeProvider === p.id ? "selected" : "";
          return `<option value="${p.id}" ${selected}>${p.name}</option>`;
        })
        .join("");
    }

    // Config list:把 desktopStatus 关键字段做平面展示
    const list = $("#claudeDesktopConfigList");
    if (list && status && status.desktopStatus) {
      const ds = status.desktopStatus;
      list.innerHTML = [
        ["平台", status.platform || "-"],
        ["plist 存在", ds.plistExists ? "✓" : "—"],
        ["config.json 存在", ds.configJsonExists ? "✓" : "—"],
        ["当前 base_url", ds.currentBaseUrl || "—"],
        ["当前 provider", ds.currentInferenceProvider || "—"],
      ]
        .map(([k, v]) => `<div class="config-row"><span>${k}</span><strong>${v}</strong></div>`)
        .join("");
    } else if (list) {
      list.innerHTML = '<div class="config-row muted">读取状态失败</div>';
    }

    // Details JSON 块
    const jsonEl = $("#claudeDesktopJson");
    if (jsonEl) {
      jsonEl.textContent = JSON.stringify(status || {}, null, 2);
    }
  }

  async function handleApplyClaudeDesktop() {
    const select = $("#claudeDesktopProviderSelect");
    const apiKey = $("#claudeDesktopApiKeyInput");
    if (!select || !select.value) {
      showToast("请选择一个 Provider 预设");
      return;
    }
    const presetId = select.value;
    const preset = (claudeDesktopState.presets || []).find((p) => p.id === presetId);
    if (!preset) {
      showToast("找不到对应的预设");
      return;
    }
    const key = (apiKey && apiKey.value || "").trim();
    if (!key) {
      showToast("请输入 API Key");
      return;
    }

    // 用 preset 数据 + 用户填的 api_key 构造完整 provider,upsert + 设 default
    const existing = (claudeDesktopState.providers || []).find((x) => x.id === presetId);
    const provider = JSON.parse(JSON.stringify(preset));
    provider.apiKey = key;
    if (existing) {
      await CCApi.updateClaudeDesktopProvider(presetId, provider).catch((e) => {
        throw new Error(`更新 provider 失败: ${e.message || e}`);
      });
    } else {
      await CCApi.addClaudeDesktopProvider(provider).catch((e) => {
        throw new Error(`添加 provider 失败: ${e.message || e}`);
      });
    }
    await CCApi.setClaudeDesktopDefaultProvider(presetId).catch(() => null);
    const result = await CCApi.applyClaudeDesktop({ providerId: presetId }).catch((e) => {
      throw new Error(`apply 失败: ${e.message || e}`);
    });
    showToast(`已写入 Claude Desktop 配置(${result.platform || ""})。请重启 Claude Desktop 生效。`);
    await renderClaudeDesktop();
  }

  // ─── Claude Desktop providers 卡片列表(sidebar 第 2 个入口)──────────
  // **UI 1:1 复用 Codex providers 同款 markup**(providerCardMarkup /
  // providerPresetCardMarkup),只把 data-action 前缀改成 claude-desktop-* 防止
  // 跟 Codex CRUD action 冲突,字段 mapping 用 Claude Desktop schema
  // (models.sonnet/haiku/opus/default 而非 mappings.gpt_5_5)。
  async function renderClaudeDesktopProviders() {
    const target = $("#claudeDesktopProviderCards");
    if (!target) return;
    const data = await CCApi.listClaudeDesktopProviders().catch(() => ({
      activeProvider: null,
      providers: [],
      presets: [],
    }));
    claudeDesktopState = {
      activeProvider: data.activeProvider,
      providers: data.providers || [],
      presets: data.presets || [],
    };
    // 已配置 provider 用 provider-switch-card,未配置 preset 用 preset-card
    const configuredMarkup = claudeDesktopState.providers
      .map((p) => claudeDesktopProviderCardMarkup(p, p.id === claudeDesktopState.activeProvider))
      .join("");
    const providerList = claudeDesktopState.providers.length
      ? `<div class="provider-configured-list">${configuredMarkup}</div>`
      : "";
    const seen = new Set(claudeDesktopState.providers.map((p) => p.id));
    const availablePresets = claudeDesktopState.presets.filter((p) => !seen.has(p.id));
    const presetTitle = t("claudeDesktopProviders.availablePresets") || "添加提供商";
    const presetHint = t("claudeDesktopProviders.availablePresetsHint");
    const presetGrid = availablePresets.length
      ? `<section class="dashboard-preset-section" aria-label="${escapeHtml(presetTitle)}">
          <div class="section-title-row compact">
            <div>
              <h2>${escapeHtml(presetTitle)}</h2>
              ${presetHint ? `<p>${escapeHtml(presetHint)}</p>` : ""}
            </div>
          </div>
          <div class="provider-preset-grid">
            ${availablePresets.map((p) => claudeDesktopPresetCardMarkup(p)).join("")}
          </div>
        </section>`
      : "";
    target.innerHTML = providerList + presetGrid;
  }

  // 1:1 复用 providerCardMarkup 视觉,字段 / action 替换成 Claude Desktop 端。
  function claudeDesktopProviderCardMarkup(provider, isActive) {
    const models = provider.models || {};
    const mapping = [
      models.default,
      models.sonnet,
      models.haiku,
      models.opus,
      models.sonnet_4_6,
      models.opus_4_7,
    ]
      .filter(Boolean)
      .slice(0, 2)
      .join(" / ");
    const id = escapeHtml(provider.id);
    const name = escapeHtml(provider.name);
    const url = escapeHtml(provider.baseUrl);
    const mappingText = escapeHtml(mapping || provider.apiFormat || "anthropic");
    return `
      <article class="provider-switch-card ${isActive ? "active" : ""}" data-provider-id="${id}">
        <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
        <span class="provider-logo">${iconMarkup(provider)}</span>
        <span class="provider-main">
          <strong>${name}</strong>
          <span class="truncate">${url}</span>
        </span>
        <span class="provider-meta truncate">${mappingText}</span>
        <span class="provider-actions">
          ${
            isActive
              ? `<span class="active-indicator" role="status" aria-label="${escapeHtml(t("status.active"))}"><i class="bi bi-broadcast" aria-hidden="true"></i><span>${escapeHtml(t("status.active"))}</span></span>`
              : ""
          }
          <button class="btn btn-primary compact-enable" type="button" data-action="claude-desktop-activate" data-id="${id}">
            <i class="bi bi-play-fill"></i><span>${t("providers.enable") || "启用"}</span>
          </button>
          <a class="icon-action" href="#claude-desktop-providers/add" data-action="claude-desktop-edit" data-id="${id}" title="${t("common.edit")}" aria-label="${t("common.edit")}"><i class="bi bi-pencil-square"></i></a>
          <button class="icon-action danger" type="button" data-action="claude-desktop-delete" data-id="${id}" title="${t("common.delete")}" aria-label="${t("common.delete")}"><i class="bi bi-trash"></i></button>
        </span>
      </article>
    `;
  }

  // 1:1 复用 providerPresetCardMarkup 视觉。
  function claudeDesktopPresetCardMarkup(preset) {
    const id = escapeHtml(preset.id);
    return `
      <a class="provider-switch-card preset-card" href="#claude-desktop-providers/add" data-action="claude-desktop-preset-config" data-preset="${id}">
        <span class="drag-handle preset-plus"><i class="bi bi-plus-lg"></i></span>
        <span class="provider-logo">${iconMarkup(preset)}</span>
        <span class="provider-main"><strong>${escapeHtml(preset.name)}</strong><span class="truncate">${escapeHtml(preset.baseUrl)}</span></span>
        <span class="provider-meta">${escapeHtml(t("apiFormatDisplay.anthropic.name") || "Anthropic Messages")}</span>
        <span class="provider-actions"><span class="compact-enable ghost"><i class="bi bi-plus-lg"></i><span>${t("providers.add") || "添加"}</span></span></span>
      </a>
    `;
  }

  // ─── Claude Desktop 编辑/添加 提供商页 ───────────────────────────
  // editingProviderId:已有 provider 时取其完整数据 prefill;若 null 但 selectedPresetId
  // 非空 = 新建,从 preset 复制 prefill;两者都 null = 空白表单(罕见兜底)。
  let claudeDesktopFormState = {
    editingProviderId: null,
    selectedPresetId: null,
    formDirty: false,
  };

  const CD_MODEL_SLOTS = [
    { key: "default", label: "Default", icon: "bi-circle-fill", iconClass: "default", required: true },
    { key: "sonnet", label: "Sonnet", icon: "bi-circle", iconClass: "default" },
    { key: "haiku", label: "Haiku", icon: "bi-circle", iconClass: "default" },
    { key: "opus", label: "Opus", icon: "bi-circle", iconClass: "default" },
  ];

  async function renderClaudeDesktopProviderForm() {
    if (!claudeDesktopState.presets || !claudeDesktopState.presets.length) {
      const data = await CCApi.listClaudeDesktopProviders().catch(() => ({
        activeProvider: null,
        providers: [],
        presets: [],
      }));
      claudeDesktopState = {
        activeProvider: data.activeProvider,
        providers: data.providers || [],
        presets: data.presets || [],
      };
    }
    const editingId = claudeDesktopFormState.editingProviderId;
    const presetId = claudeDesktopFormState.selectedPresetId;
    const existing = editingId
      ? (claudeDesktopState.providers || []).find((p) => p.id === editingId)
      : null;
    const preset = presetId
      ? (claudeDesktopState.presets || []).find((p) => p.id === presetId)
      : null;
    const initial = existing || preset || {
      id: "",
      name: "",
      baseUrl: "",
      authScheme: "bearer",
      apiKey: "",
      models: {},
      baseUrlOptions: [],
      baseUrlHint: "",
    };
    const title = $("#claudeDesktopProviderFormTitle");
    if (title) {
      title.textContent = existing
        ? (t("claudeDesktopProvidersAdd.titleEdit") || "编辑 Claude Desktop 提供商")
        : (t("claudeDesktopProvidersAdd.titleAdd") || "添加 Claude Desktop 提供商");
    }
    setVal("#cdProviderName", initial.name || "");
    setVal("#cdProviderBaseUrl", initial.baseUrl || "");
    setVal("#cdProviderApiKey", initial.apiKey || "");
    const authScheme = initial.authScheme || "bearer";
    setVal("#cdProviderAuth", authScheme);
    const authLabel = $("#cdProviderAuthTrigger span");
    if (authLabel) authLabel.textContent = authScheme;
    $all("#cdProviderAuthMenu .auth-scheme-option").forEach((btn) => {
      const sel = btn.dataset.value === authScheme;
      btn.classList.toggle("selected", sel);
      btn.setAttribute("aria-selected", sel ? "true" : "false");
      const has = btn.querySelector("i.bi-check2");
      if (sel && !has) btn.insertAdjacentHTML("beforeend", '<i class="bi bi-check2"></i>');
      if (!sel && has) has.remove();
    });
    const authWrap = $("#cdProviderAuthControl");
    if (authWrap) authWrap.classList.remove("open");
    const authTrig = $("#cdProviderAuthTrigger");
    if (authTrig) authTrig.setAttribute("aria-expanded", "false");

    const apiFormat = initial.apiFormat === "openai_chat" ? "openai_chat" : "anthropic";
    claudeDesktopFormState.apiFormat = apiFormat;
    $all("#cdProviderFormatChoice .format-choice-button").forEach((btn) => {
      const sel = btn.dataset.cdApiFormat === apiFormat;
      btn.classList.toggle("active", sel);
    });

    const subtitleEl = $("#page-claude-desktop-providers-add .page-title p");
    if (subtitleEl) subtitleEl.textContent = "添加新的 Claude Desktop 提供商或选择预设";
    const mappingSubtitleEl = $("#page-claude-desktop-providers-add .provider-mapping-section .section-title-row p");
    if (mappingSubtitleEl) mappingSubtitleEl.textContent = "把 Claude 的 Sonnet / Haiku / Opus 映射到这个供应商真实支持的模型。";
    const applyHintEl = $("#page-claude-desktop-providers-add .apply-explain p");
    if (applyHintEl) applyHintEl.textContent = "一键应用会保存供应商、把它设为默认,并写入 Claude Desktop 配置。重启 Claude Desktop 后即可在客户端直接使用第三方模型。";

    const mappingStack = $("#cdProviderMappingStack");
    if (mappingStack) {
      const rows = CD_MODEL_SLOTS.map((slot, index) => {
        const value = (initial.models && initial.models[slot.key]) || "";
        return `
          <article class="form-mapping-row">
            <div class="form-mapping-left">
              <label class="form-label visually-hidden" for="cdProviderMappingValue-${index}">Claude 模型</label>
              <div class="mapping-select-wrap">
                <span class="mapping-icon ${slot.iconClass}"><i class="bi ${slot.icon}"></i></span>
                <div class="mapping-slot-menu-wrap">
                  <button class="form-select mapping-slot-trigger" id="cdProviderMappingSlot-${index}" type="button" disabled aria-haspopup="listbox" aria-expanded="false">
                    <span>${escapeHtml(slot.label)}</span>
                    <i class="bi bi-chevron-down"></i>
                  </button>
                </div>
              </div>
            </div>
            <div class="form-mapping-right">
              <label class="form-label visually-hidden" for="cdProviderMappingValue-${index}">Provider 模型</label>
              <div class="provider-model-input-wrap">
                <input
                  class="form-control provider-model-input"
                  id="cdProviderMappingValue-${index}"
                  data-cd-model-slot="${escapeHtml(slot.key)}"
                  value="${escapeHtml(value)}"
                  placeholder="provider 模型名"
                  autocomplete="off"
                  autocapitalize="off"
                  spellcheck="false"
                  ${slot.required ? "required" : ""}
                >
                <button class="provider-model-trigger" type="button" disabled aria-haspopup="listbox" aria-expanded="false" aria-label="provider 模型">
                  <i class="bi bi-chevron-down" aria-hidden="true"></i>
                </button>
              </div>
            </div>
            <div class="form-mapping-actions">
              <span class="mapping-remove-placeholder" aria-hidden="true"></span>
            </div>
          </article>
        `;
      }).join("");
      mappingStack.innerHTML = `
        <div class="provider-mapping-card">
          <div class="provider-mapping-list">
            ${rows}
          </div>
        </div>
      `;
    }

    const baseUrlOptions = initial.baseUrlOptions || [];
    const trigger = $("#cdProviderBaseUrlTrigger");
    const menu = $("#cdProviderBaseUrlMenu");
    const hint = $("#cdProviderBaseUrlHint");
    if (trigger) trigger.hidden = baseUrlOptions.length === 0;
    if (menu) {
      menu.classList.remove("open");
      if (baseUrlOptions.length > 0) {
        menu.innerHTML = baseUrlOptions
          .map((o) => {
            const sel = o.value === initial.baseUrl ? "selected" : "";
            return `<button class="baseurl-option ${sel}" type="button" role="option" data-action="cd-select-baseurl-option" data-value="${escapeHtml(o.value)}" aria-selected="${sel ? "true" : "false"}"><span class="baseurl-option-label">${escapeHtml(o.label)}</span><span class="baseurl-option-value">${escapeHtml(o.value)}</span></button>`;
          })
          .join("");
      } else {
        menu.innerHTML = "";
      }
    }
    if (hint) {
      if (initial.baseUrlHint) {
        hint.hidden = false;
        hint.textContent = initial.baseUrlHint;
      } else {
        hint.hidden = true;
        hint.textContent = "";
      }
    }

    const presetOptionsHost = $("#cdProviderPresetOptions");
    if (presetOptionsHost) {
      const modelOptions = (initial.modelOptions || []).filter((m) => m.value);
      const requestPresets = (initial.requestOptionPresets || []).filter((p) => p.id);
      const hasPanels = modelOptions.length > 0 || requestPresets.length > 0;
      presetOptionsHost.hidden = !hasPanels;
      if (hasPanels) {
        let html = "";
        if (modelOptions.length > 0) {
          html += `<div class="preset-options-group"><h3>${escapeHtml(t("claudeDesktopProvidersAdd.modelOptions") || "可选模型")}</h3><div class="preset-options-list">`;
          html += modelOptions
            .map((opt, idx) => {
              const checked = opt.default ? "checked" : "";
              return `<label class="preset-option-row"><input type="checkbox" data-cd-model-option-idx="${idx}" ${checked}> <span>${escapeHtml(opt.label || opt.value)}</span><code>${escapeHtml(opt.value)}</code></label>`;
            })
            .join("");
          html += `</div></div>`;
        }
        if (requestPresets.length > 0) {
          html += `<div class="preset-options-group"><h3>${escapeHtml(t("claudeDesktopProvidersAdd.requestPresets") || "请求参数预设")}</h3><div class="preset-options-list">`;
          html += requestPresets
            .map((p, idx) => {
              const checked = p.default ? "checked" : "";
              return `<label class="preset-option-row"><input type="checkbox" data-cd-request-preset-idx="${idx}" ${checked}> <span>${escapeHtml(p.label || p.id)}</span></label>`;
            })
            .join("");
          html += `</div></div>`;
        }
        presetOptionsHost.innerHTML = html;
      } else {
        presetOptionsHost.innerHTML = "";
      }
    }

    const delBtn = $("#cdProviderDeleteBtn");
    if (delBtn) delBtn.hidden = !existing;

    const presetList = $("#cdPresetList");
    if (presetList) {
      const presets = claudeDesktopState.presets || [];
      presetList.innerHTML = presets
        .map((p) => {
          const decorated = (window.CCApi && CCApi.claudeDesktopWithIcon
            ? CCApi.claudeDesktopWithIcon(p)
            : p) || p;
          return claudeDesktopPresetCardMarkup(decorated, presetId === p.id);
        })
        .join("");
    }
  }

  function setVal(sel, value) {
    const el = $(sel);
    if (el) el.value = value;
  }

  function readClaudeDesktopFormProvider() {
    const presetId =
      claudeDesktopFormState.selectedPresetId || claudeDesktopFormState.editingProviderId;
    const preset =
      (claudeDesktopState.presets || []).find((p) => p.id === presetId) ||
      (claudeDesktopState.providers || []).find((p) => p.id === presetId);
    const baseProvider = preset ? JSON.parse(JSON.stringify(preset)) : {};
    baseProvider.id = claudeDesktopFormState.editingProviderId || (preset && preset.id) || "";
    baseProvider.name = $("#cdProviderName")?.value.trim() || baseProvider.name || "";
    baseProvider.baseUrl = $("#cdProviderBaseUrl")?.value.trim() || baseProvider.baseUrl || "";
    baseProvider.apiKey = $("#cdProviderApiKey")?.value.trim() || "";
    baseProvider.authScheme = $("#cdProviderAuth")?.value || "bearer";
    baseProvider.apiFormat = "anthropic";
    const models = baseProvider.models || {};
    $all("#cdProviderMappingStack [data-cd-model-slot]").forEach((el) => {
      models[el.dataset.cdModelSlot] = el.value.trim();
    });
    baseProvider.models = models;
    return baseProvider;
  }

  async function handleClaudeDesktopSaveOnly() {
    const p = readClaudeDesktopFormProvider();
    if (!p.name || !p.baseUrl || !p.apiKey) {
      showToast("请填写 名称 / Base URL / API Key");
      return;
    }
    try {
      if (claudeDesktopFormState.editingProviderId) {
        await CCApi.updateClaudeDesktopProvider(claudeDesktopFormState.editingProviderId, p);
      } else {
        await CCApi.addClaudeDesktopProvider(p);
      }
      showToast("已保存");
      window.location.hash = "#claude-desktop-providers";
    } catch (e) {
      showToast(e.message || String(e));
    }
  }

  async function handleClaudeDesktopApplyFromForm() {
    const p = readClaudeDesktopFormProvider();
    if (!p.name || !p.baseUrl || !p.apiKey) {
      showToast("请填写 名称 / Base URL / API Key");
      return;
    }
    try {
      let providerId = claudeDesktopFormState.editingProviderId;
      if (providerId) {
        await CCApi.updateClaudeDesktopProvider(providerId, p);
      } else {
        await CCApi.addClaudeDesktopProvider(p);
        providerId = p.id;
      }
      await CCApi.setClaudeDesktopDefaultProvider(providerId);
      const result = await CCApi.applyClaudeDesktop({ providerId });
      showToast(`已应用 → 请重启 Claude Desktop(${result.platform || ""})`);
      window.location.hash = "#claude-desktop-providers";
    } catch (e) {
      showToast(e.message || String(e));
    }
  }

  async function handleClaudeDesktopDeleteFromForm() {
    const id = claudeDesktopFormState.editingProviderId;
    if (!id) return;
    try {
      await CCApi.deleteClaudeDesktopProvider(id);
      showToast("已删除");
      window.location.hash = "#claude-desktop-providers";
    } catch (e) {
      showToast(e.message || String(e));
    }
  }

  async function handleClaudeDesktopActivate(id) {
    if (!id) return;
    try {
      await CCApi.setClaudeDesktopDefaultProvider(id);
      const result = await CCApi.applyClaudeDesktop({ providerId: id });
      showToast(`已应用 → 请重启 Claude Desktop(${result.platform || ""})`);
      await renderClaudeDesktopProviders();
    } catch (e) {
      showToast(e.message || String(e));
    }
  }

  async function handleClaudeDesktopDelete(id) {
    if (!id) return;
    try {
      await CCApi.deleteClaudeDesktopProvider(id);
      showToast("已删除");
      await renderClaudeDesktopProviders();
    } catch (e) {
      showToast(e.message || String(e));
    }
  }

  async function handleClearClaudeDesktop() {
    const result = await CCApi.clearClaudeDesktop().catch((e) => {
      throw new Error(`clear 失败: ${e.message || e}`);
    });
    showToast(result.restored ? "已从快照还原 Claude Desktop 原配置" : "已清掉本工具写入的字段");
    await renderClaudeDesktop();
  }

  let currentTheme = "default";

  function normalizeTheme(theme) {
    if (!theme || theme === "light" || theme === "auto") return "default";
    return availableThemes.includes(theme) ? theme : "default";
  }

  function applyTheme(theme) {
    if (theme === "toggle") {
      theme = currentTheme === "dark" ? "default" : "dark";
    }
    const normalized = normalizeTheme(theme);
    currentTheme = normalized;
    document.documentElement.setAttribute("data-bs-theme", normalized === "dark" ? "dark" : "light");
    document.documentElement.setAttribute("data-theme-palette", normalized);
    $all(".theme-segment .btn").forEach((button) => {
      const active = button.dataset.themeAction === normalized;
      button.classList.toggle("active", active);
      button.setAttribute("aria-pressed", active ? "true" : "false");
    });
    const icon = $("[data-theme-action='toggle'] i");
    if (icon) icon.className = normalized === "dark" ? "bi bi-sun-fill" : "bi bi-moon-stars-fill";
    return normalized;
  }

  async function saveSettingsFromForm() {
    const settings = {
      theme: currentTheme,
      proxyPort: Number($("#settingsProxyPort").value),
      adminPort: Number($("#settingsAdminPort").value),
      autoApplyOnStart: $("#autoApplyOnStart")?.checked !== false,
     autoUnlockCodexPlugins: $("#autoUnlockCodexPlugins")?.checked || false,
      autoWakeCodexPet: $("#autoWakeCodexPet")?.checked !== false,
     exposeAllProviderModels: $("#exposeAllProviderModels")?.checked || false,
      restoreCodexOnExit: $("#restoreCodexOnExit")?.checked !== false,
      updateUrl: $("#settingsUpdateUrl").value.trim(),
    };
    await CCApi.saveSettings(settings);
    $("#proxyPort").value = settings.proxyPort;
    renderModelMenuModeState(settings);
  }

  function formatUsageItems(result) {
    if (result.supported === false) return result.message;
    if (!result.items || !result.items.length) return result.message || t("providers.usageUnavailable");
    return result.items.map((item) => {
      const unit = item.unit ? ` ${item.unit}` : "";
      if (item.remaining !== null && item.remaining !== undefined) {
        return `${item.label}: ${item.remaining}${unit}`;
      }
      if (item.used !== null && item.used !== undefined) {
        return `${item.label}: ${item.used}${unit}`;
      }
      return item.label;
    }).join(" · ");
  }

  function formatProviderTestResult(result) {
    if (result?.message) return result.message;
    if (Number.isFinite(result?.latencyMs)) return `${Math.round(result.latencyMs)} ms`;
    return t("providers.testDone");
  }

  // 测速结果是否要 UI 标黄(.bad class)。**白名单语义**(silent-failure-hunter
  // review H2):后端将来加新 authStatus 枚举(`tls_warn` / `rate_limited` /
  // `cert_expired` 等)/ 或返 `success: false` 不带 ok 字段,helper 默认标黄不漏判。
  //
  // **修复历史(2026-05-10)**:`auth_required_or_invalid`(401/403)以前被
  // 当 bad 标黄,但 backend `test.rs:312-318` 注释明确"401/403 = baseUrl 连接性
  // OK + 鉴权未验证,应绿色"(测连接性本来不需要 key,鉴权层跟连接层解耦)。
  // 显式 allow-list 这个 authStatus 走绿色,其他未来新增 authStatus 默认仍标黄。
  function isProviderTestResultBad(result) {
    if (!result) return true;
    if (result.success === false) return true;
    if (result.ok === false) return true;
    if (result.authStatus && result.authStatus !== "ok"
        && result.authStatus !== "auth_required_or_invalid") {
      return true;
    }
    return false;
  }

  // 把 backend errors[] (object 数组,含 code/host/statusCode)按当前 locale i18n 翻译。
  // 历史兼容:string 元素直接显示。未识别的 code → 走 unknown / unknown_with_status
  // fallback,把 statusCode 拼进文案("上游返回错误 (HTTP 502)" / "Upstream error (HTTP 502)")。
  function translateUpstreamError(err) {
    if (typeof err === "string") return err;
    if (!err || typeof err !== "object") return t("models.upstreamError.unknown");
    const code = err.code || "unknown";
    let translated = t(`models.upstreamError.${code}`);
    // 没命中(返了 key 自身)→ fallback 通用文案
    if (translated === `models.upstreamError.${code}`) {
      translated = t("models.upstreamError.unknown");
    }
    if (err.statusCode) {
      // 动态 key (`models.upstreamError.${code}`) 已 t() 完,这里只对模板字符串
      // 替换 `{status}` 占位 — split/join 而非 String.replace 防 statusCode 含
      // `$` / 正则元字符被 replace 误解析。tFmt 不适用于"已 t 完的字符串"
      translated = translated.split("{status}").join(String(err.statusCode));
    }
    return err.host ? `[${err.host}] ${translated}` : translated;
  }

  function formatModelFetchError(error) {
    const errs = (error && error.errors) || [];
    // 优先用第一个结构化 error(最相关) — 已 i18n;退化路径用 error.message(网络层异常)
    const detail = errs.length > 0 ? translateUpstreamError(errs[0]) : (error && error.message);
    const reason = detail || t("toast.requestFailed");
    return `${t("models.fetchFailedManual")}: ${reason}`;
  }

  function downloadJson(filename, data) {
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }

  async function refreshBackupList() {
    const target = $("#backupList");
    if (!target) return;
    try {
      const backups = await CCApi.listBackups();
      target.innerHTML = backups.length
        ? backups.slice(0, 5).map((item) => `<span>${escapeHtml(item.name)}</span><time>${escapeHtml(item.createdAt)}</time>`).join("")
        : `<span>${t("settings.noBackups")}</span>`;
    } catch (error) {
      target.innerHTML = `<span>${t("settings.backupLoadFailed")}</span>`;
    }
  }

  async function importConfigFile(file) {
    if (!file) return;
    if (!window.confirm(t("confirm.configImport"))) return;
    try {
      const text = await file.text();
      const configData = JSON.parse(text);
      await CCApi.importConfig(configData);
      await renderRoute(routeFromHash());
      showToast(t("toast.configImported"));
    } catch (error) {
      console.error(error);
      showToast(error.message || t("toast.configImportFailed"));
    } finally {
      const input = $("#configImportFile");
      if (input) input.value = "";
    }
  }

  function renderProviderCompatibilityList(result) {
    const target = $("#providerCompatibilityList");
    if (!target) return;
    const providers = result?.providers || [];
    if (!providers.length) {
      target.innerHTML = `<p class="compatibility-empty">${escapeHtml(t("settings.compatibilityEmpty"))}</p>`;
      return;
    }
    target.innerHTML = providers.map((provider) => `
      <article class="compatibility-item ${escapeHtml(provider.level)}">
        <div>
          <strong>${escapeHtml(provider.name)}</strong>
          <span>${escapeHtml(provider.message)}</span>
        </div>
        <em>${escapeHtml(provider.apiFormat)}</em>
      </article>
    `).join("");
  }

  async function saveProviderFromForm() {
    const payload = providerPayloadFromForm(true);
    // Responses 透传协议(direct mode)必须填齐 baseUrl + apiKey,否则 backend
    // 会 silent fallback 到 local_proxy → Codex.app 经代理 → 行为偏离用户预期。
    // 前端拦下让用户立即看到错误,而不是后端 fallback 后用户毫无察觉。
    if (payload.apiFormat === "responses" || payload.apiFormat === "openai_responses") {
      if (!payload.baseUrl) {
        throw new Error(t("toast.directModeBaseUrlRequired"));
      }
      if (!editingProviderId && !payload.apiKey) {
        throw new Error(t("toast.directModeApiKeyRequired"));
      }
    }
    // **code-reviewer #3 修**:OAuth provider 必须先登录才能 save。否则 backend
    // 存了 provider 但 extra.cloud_code_project_id 缺失,任何 chat 请求都返
    // BadRequest "cloud_code_project_id required" — 前端拦下让 user 立即知道要
    // 先登录,不是 save 后才发现 silently broken provider。两个 OAuth provider
    // 各自独立检查,错的 provider 不能 save
    {
      const config = OAUTH_PROVIDER_CONFIGS[payload.apiFormat];
      if (config) {
        const loginRequiredMsg = t(`${config.i18nPrefix}.loginRequired`);
        try {
          const status = await config.api.getStatus();
          if (!status.loggedIn || !status.projectId) {
            throw new Error(loginRequiredMsg);
          }
        } catch (e) {
          // 网络错也阻止 save — 不能让 unknown state 持久化
          if (e.message && e.message.includes(loginRequiredMsg)) throw e;
          throw new Error(`OAuth status check failed: ${e.message || e}`);
        }
      }
    }
    if (editingProviderId) {
      await CCApi.saveDraft(editingProviderId, payload);
      return { id: editingProviderId, ...payload };
    }
    const provider = await CCApi.addProvider(payload);
    editingProviderId = provider.id;
    await CCApi.saveDraft(provider.id, payload);
    return provider;
  }

  async function applyProviderToDesktop(actionEl) {
    // 表单页"启用"按钮 = 保存表单 + 走 set-default 同一条后端链路
    // (`switch_provider_and_sync` 写 activeProvider 并同步到 ~/.codex)。
    // 与 dashboard 的「启用」按钮(action="set-default")在用户感知上等价,
    // 唯一差异是这里要先把表单字段保存为 provider。**不弹 window.confirm**:
    // Tauri webview 在某些环境会静默忽略原生 confirm,导致用户看不到任何反馈
    // 误以为按钮失灵(2026-05-06 现场实测)。
    //
    // 响应延迟优化(2026-05-06):
    // 旧版按顺序 await 10+ RPC(saveProvider → setDefault → startProxy →
    // renderProviderCards × 1 → renderProviders 内 × 2 → renderDashboard 内
    // × 3,getProviders 重复 3 次),链路 1.5-3s 才解锁按钮。
    // 现在:
    // 1. 关键路径只 await `saveProviderFromForm` + `setDefaultProvider`(2-3
    //    RPC),拿到结果立刻 hash 跳页 / toast / 重启提示。
    // 2. hash → "dashboard" 由路由器自动触发 `renderDashboard`,不再手动调,
    //    避免与路由器重复 fetch 同一份 providers/status。
    // 3. `startProxy`(若 desktopSync.requiresProxy)放后台,不阻塞 UI。
    // 4. providers 页 / model 选择器等"用户没在看"的渲染留给下次进入时再
    //    跑,避免冗余 RPC。
    const form = $("#providerForm");
    if (form && !form.reportValidity()) return;

    actionEl.disabled = true;
    try {
      const provider = await saveProviderFromForm();
      const result = await CCApi.setDefaultProvider(provider.id);
      const desktopSync = result?.desktopSync || {};

      editingProviderId = null;
      selectedPreset = null;
      window.location.hash = "dashboard";
      if (desktopSync.attempted && desktopSync.success === false) {
        showToast(t("toast.defaultUpdatedDesktopFailed"));
      } else {
        showToast(t("toast.defaultUpdatedDesktop"));
      }
      showRestartReminder();

      if (desktopSync.requiresProxy) {
        CCApi.startProxy().catch((error) => {
          console.error("applyProviderToDesktop background startProxy failed:", error);
        });
      }
    } finally {
      actionEl.disabled = false;
    }
  }

  async function handleAction(target) {
    const action = target.closest("[data-action]")?.dataset.action;
    if (!action) return;
    const actionEl = target.closest("[data-action]");

    if (action === "toggle-key") {
      const input = $("#providerApiKey");
      input.type = input.type === "password" ? "text" : "password";
      actionEl.innerHTML = `<i class="bi ${input.type === "password" ? "bi-eye" : "bi-eye-slash"}"></i>`;
    }

    try {
      if (action === "set-default") {
        const result = await CCApi.setDefaultProvider(actionEl.dataset.id);
        if (result.desktopSync?.requiresProxy) {
          await CCApi.startProxy();
        }
        await renderProviderCards("#dashboardProviderCards", { includePresets: true });
        await renderProviders();
        await renderDashboard();
        const desktopSync = result.desktopSync || {};
        if (desktopSync.attempted && desktopSync.success) {
          showToast(t("toast.defaultUpdatedDesktop"));
          showRestartReminder();
        } else if (desktopSync.attempted && desktopSync.success === false) {
          showToast(t("toast.defaultUpdatedDesktopFailed"));
        } else {
          showToast(t("toast.defaultUpdated"));
          showRestartReminder();
        }
      }

      if (action === "new-from-preset") {
        const presets = await CCApi.getPresets();
        selectedPreset = presets.find((item) => item.id === actionEl.dataset.preset) || null;
        editingProviderId = null;
        window.location.hash = "providers/add";
      }

      if (action === "edit-provider") {
        editingProviderId = actionEl.dataset.id;
        selectedPreset = null;
        window.location.hash = "providers/add";
      }

      if (action === "copy-url") {
        await navigator.clipboard.writeText(actionEl.dataset.url || "");
        showToast(t("toast.copied"));
      }

      if (action === "open-docs") {
        event.preventDefault();
        const url = actionEl.dataset.docsUrl;
        const name = actionEl.dataset.providerName || "";
        if (!url) return;
        const message = tFmt("confirm.openDocs", { provider: name });
        if (window.confirm(message)) {
          window.open(url, "_blank", "noopener,noreferrer");
        }
      }

      if (action === "test-provider") {
        const resultEl = $(`[data-speed-for="${actionEl.dataset.id}"]`);
        actionEl.disabled = true;
        if (resultEl) {
          resultEl.textContent = t("providers.testing");
          resultEl.classList.remove("bad");
        }
        try {
          const result = await CCApi.testProvider(actionEl.dataset.id);
          const message = formatProviderTestResult(result);
          if (resultEl) {
            resultEl.textContent = message;
            resultEl.classList.toggle("bad", isProviderTestResultBad(result));
          }
          showToast(message);
        } catch (error) {
          const message = error?.message || t("toast.requestFailed");
          if (resultEl) {
            resultEl.textContent = message;
            resultEl.classList.add("bad");
          }
          showToast(message);
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "query-usage") {
        const resultEl = $(`[data-usage-for="${actionEl.dataset.id}"]`) || $(`[data-speed-for="${actionEl.dataset.id}"]`);
        actionEl.disabled = true;
        if (resultEl) {
          resultEl.textContent = t("providers.usageQuerying");
          resultEl.classList.remove("bad");
        }
        try {
          const result = await CCApi.queryProviderUsage(actionEl.dataset.id);
          const message = formatUsageItems(result);
          if (resultEl) {
            resultEl.textContent = message;
            resultEl.classList.toggle("bad", result.ok === false || result.supported === false);
          }
          showToast(message);
        } catch (error) {
          const message = error?.message || t("toast.requestFailed");
          if (resultEl) {
            resultEl.textContent = message;
            resultEl.classList.add("bad");
          }
          showToast(message);
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "test-provider-form") {
        const resultEl = $("#formSpeedResult");
        actionEl.disabled = true;
        resultEl.textContent = t("providers.testing");
        resultEl.classList.remove("bad");
        try {
          const payload = providerPayloadFromForm(true);
          if (editingProviderId && !payload.apiKey) {
            try {
              const secret = await CCApi.getProviderSecret(editingProviderId);
              if (secret.apiKey) payload.apiKey = secret.apiKey;
            } catch (e) { /* ignore */ }
          }
          if (editingProviderId) {
            await CCApi.saveDraft(editingProviderId, payload);
          }
          const result = await CCApi.testProviderPayload(payload);
          const message = formatProviderTestResult(result);
          resultEl.textContent = message;
          resultEl.classList.toggle("bad", isProviderTestResultBad(result));
          showToast(message);
        } catch (error) {
          const message = error?.message || t("toast.requestFailed");
          resultEl.textContent = message;
          resultEl.classList.add("bad");
          showToast(message);
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "fetch-form-models") {
        const resultEl = $("#providerModelFetchResult");
        actionEl.disabled = true;
        if (resultEl) resultEl.textContent = t("models.fetching");
        try {
          const payload = providerPayloadFromForm(false);
          if (editingProviderId && !payload.apiKey) {
            try {
              const secret = await CCApi.getProviderSecret(editingProviderId);
              if (secret.apiKey) payload.apiKey = secret.apiKey;
            } catch (e) { /* ignore */ }
          }
          if (editingProviderId) {
            await CCApi.saveDraft(editingProviderId, payload);
          }
          const result = await CCApi.fetchProviderModelsPayload(payload);
          providerAvailableModels = Array.isArray(result.models) ? result.models.slice() : [];
          // **不覆盖 user 已有 mappings,只刷新下拉选项**(2026-05-11 修):
          // 原 `setProviderMappings(result.suggested, ...)` 会用 suggested 整覆盖
          // (suggested 后端只填 default,其他 slot 是空串)→ user 自己设的
          // gpt_5_5 / gpt_5_4 等被清空。期望行为:获取模型只更新下拉可选项,
          // 不清 user 已有映射(default 留空时才允许 suggested.default 填进去)
          const suggestedDefault = (result.suggested && result.suggested.default) || "";
          if (suggestedDefault && !providerFormMappings.default) {
            providerFormMappings.default = suggestedDefault;
          }
          setProviderMappings(providerFormMappings, { availableModels: providerAvailableModels });
          if (resultEl) resultEl.textContent = t("models.fetchSuccess");
          showToast(t("toast.modelsAutofilled"));
        } catch (error) {
          providerAvailableModels = [];
          renderProviderMappings();
          const message = formatModelFetchError(error);
          if (resultEl) resultEl.textContent = message;
          showToast(message);
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "add-provider-model-row") {
        addProviderMappingRow();
      }

      if (action === "remove-provider-model-row") {
        removeProviderMappingRow(Number(actionEl.dataset.rowIndex));
      }

      if (action === "toggle-provider-model-slot-menu") {
        toggleProviderSlotMenu(Number(actionEl.dataset.rowIndex));
      }

      if (action === "toggle-baseurl-menu") {
        toggleBaseUrlMenu();
      }

      if (action === "select-baseurl-option") {
        setBaseUrlValue(actionEl.dataset.baseurlValue || "");
      }

      if (action === "select-provider-model-slot") {
        moveProviderMappingRow(Number(actionEl.dataset.rowIndex), actionEl.dataset.slotKey);
        renderPresetOptions(selectedPreset, collectProviderMappings());
      }

      if (action === "toggle-provider-model-menu") {
        toggleProviderModelMenu(actionEl.dataset.rowKey);
      }

      if (action === "select-provider-model-option") {
        const rowKey = openProviderModelMenuKey;
        if (rowKey) {
          updateProviderModelInput(rowKey, actionEl.dataset.modelValue || "");
          closeProviderModelMenu();
          renderPresetOptions(selectedPreset, collectProviderMappings());
        }
      }

      if (action === "delete-provider") {
        pendingDeleteId = actionEl.dataset.id;
        deleteModal.show();
      }

      if (action === "save-models") {
        const mappings = {};
        $all("[data-model-input]").forEach((input) => {
          mappings[input.dataset.modelInput] = input.value.trim();
        });
        const defaultKey = $("#defaultModel")?.value || "gpt_5_5";
        mappings.default = mappings[defaultKey] || mappings.gpt_5_5 || mappings.gpt_5_4 || mappings.gpt_5_4_mini || mappings.gpt_5_3_codex || mappings.gpt_5_2 || "";
        await CCApi.saveModelMappings($("#modelProvider").value, mappings);
        showToast(t("toast.modelsSaved"));
      }

      if (action === "fetch-models") {
        const providerId = $("#modelProvider").value;
        const resultEl = $("#modelFetchResult");
        actionEl.disabled = true;
        if (resultEl) resultEl.textContent = t("models.fetching");
        try {
          const result = await CCApi.autofillProviderModels(providerId);
          await renderMappingCards();
          if (resultEl) {
            resultEl.textContent = `${t("models.fetched")} ${result.models.length}`;
          }
          showToast(t("toast.modelsAutofilled"));
        } catch (error) {
          const message = formatModelFetchError(error);
          if (resultEl) resultEl.textContent = message;
          showToast(message);
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "reset-models") {
        await renderMappingCards();
        showToast(t("toast.modelsReset"));
      }

      if (action === "apply-desktop") {
        const result = await CCApi.configureDesktop();
        if (result && result.commands && result.commands.temporary) {
          await navigator.clipboard.writeText(result.commands.temporary);
          showToast(t("toast.desktopApplied"));
        } else {
          showToast(t("toast.desktopApplied"));
        }
        await renderDesktop();
      }

      if (action === "apply-claude-desktop") {
        try {
          await handleApplyClaudeDesktop();
        } catch (e) {
          showToast(e.message || String(e));
        }
      }

      if (action === "clear-claude-desktop" || action === "claude-desktop-clear-current") {
        try {
          await handleClearClaudeDesktop();
          if (routeFromHash() === "claude-desktop-providers") {
            await renderClaudeDesktopProviders();
          }
        } catch (e) {
          showToast(e.message || String(e));
        }
      }

      if (action === "claude-desktop-activate") {
        const id = actionEl.dataset.id;
        await handleClaudeDesktopActivate(id);
      }

      if (action === "claude-desktop-delete") {
        const id = actionEl.dataset.id;
        await handleClaudeDesktopDelete(id);
      }

      // 编辑现有 provider:set editing id 后浏览器自然跟 href 进入 add 页
      if (action === "claude-desktop-edit") {
        claudeDesktopFormState = {
          editingProviderId: actionEl.dataset.id || null,
          selectedPresetId: null,
          formDirty: false,
        };
      }

      // 点 preset 卡片:set preset id 进入 add 页(无 editing,prefill from preset)
      if (action === "claude-desktop-preset-config") {
        claudeDesktopFormState = {
          editingProviderId: null,
          selectedPresetId: actionEl.dataset.preset || null,
          formDirty: false,
        };
      }

      if (action === "cd-apply-provider") {
        await handleClaudeDesktopApplyFromForm();
      }

      if (action === "cd-save-only") {
        await handleClaudeDesktopSaveOnly();
      }

      if (action === "cd-delete-provider") {
        if (confirm("确定删除该 provider?")) {
          await handleClaudeDesktopDeleteFromForm();
        }
      }

      if (action === "cd-toggle-key") {
        const input = $("#cdProviderApiKey");
        if (input) {
          input.type = input.type === "password" ? "text" : "password";
          const icon = actionEl.querySelector("i");
          if (icon) {
            icon.className = input.type === "password" ? "bi bi-eye" : "bi bi-eye-slash";
          }
        }
      }

      if (action === "cd-test-provider-form") {
        const baseUrl = $("#cdProviderBaseUrl")?.value.trim();
        const apiKey = $("#cdProviderApiKey")?.value.trim();
        const authScheme = $("#cdProviderAuth")?.value || "bearer";
        const apiFormat = claudeDesktopFormState.apiFormat || "anthropic";
        if (!baseUrl) { showToast("请先填 Base URL"); return; }
        const resultEl = $("#cdFormSpeedResult");
        if (resultEl) resultEl.textContent = "测速中…";
        try {
          const r = await CCApi.testClaudeDesktopBaseUrl({ baseUrl, apiKey, authScheme, apiFormat });
          if (resultEl) resultEl.textContent = r.ok ? `OK · ${r.latencyMs}ms · HTTP ${r.status}` : `失败 · HTTP ${r.status} · ${r.message || ""}`;
        } catch (e) {
          if (resultEl) resultEl.textContent = `失败 · ${e.message || e}`;
        }
      }

      if (action === "cd-fetch-form-models") {
        const baseUrl = $("#cdProviderBaseUrl")?.value.trim();
        const apiKey = $("#cdProviderApiKey")?.value.trim();
        const authScheme = $("#cdProviderAuth")?.value || "bearer";
        const apiFormat = claudeDesktopFormState.apiFormat || "anthropic";
        if (!baseUrl || !apiKey) { showToast("请先填 Base URL 和 API Key"); return; }
        const resultEl = $("#cdProviderModelFetchResult");
        if (resultEl) resultEl.textContent = "拉取中…";
        try {
          const r = await CCApi.fetchClaudeDesktopModels({ baseUrl, apiKey, authScheme, apiFormat });
          const models = r.models || [];
          if (resultEl) resultEl.textContent = `获取到 ${models.length} 个模型` + (models.length ? ` · ${models.slice(0, 4).join(" / ")}${models.length > 4 ? " …" : ""}` : "");
          if (models.length && resultEl) {
            const inputs = $all("#cdProviderMappingStack .provider-model-input");
            inputs.forEach((inp) => {
              inp.setAttribute("list", "cdFetchedModelList");
            });
            const dl = document.getElementById("cdFetchedModelList") || (() => { const d = document.createElement("datalist"); d.id = "cdFetchedModelList"; document.body.appendChild(d); return d; })();
            dl.innerHTML = models.map((m) => `<option value="${escapeHtml(m)}">`).join("");
          }
        } catch (e) {
          if (resultEl) resultEl.textContent = `失败 · ${e.message || e}`;
        }
      }

      if (action === "cd-toggle-baseurl-menu") {
        const menu = $("#cdProviderBaseUrlMenu");
        if (menu) {
          const open = menu.classList.toggle("open");
          actionEl.setAttribute("aria-expanded", open ? "true" : "false");
        }
      }

      if (action === "cd-select-baseurl-option") {
        const value = actionEl.dataset.value || "";
        setVal("#cdProviderBaseUrl", value);
        const menu = $("#cdProviderBaseUrlMenu");
        if (menu) {
          menu.classList.remove("open");
          $all("#cdProviderBaseUrlMenu .baseurl-option").forEach((opt) => {
            const sel = opt.dataset.value === value;
            opt.classList.toggle("selected", sel);
            opt.setAttribute("aria-selected", sel ? "true" : "false");
          });
        }
        const trig = $("#cdProviderBaseUrlTrigger");
        if (trig) trig.setAttribute("aria-expanded", "false");
      }

      if (action === "cd-toggle-auth-scheme-menu") {
        const wrap = $("#cdProviderAuthControl");
        if (wrap) {
          const open = wrap.classList.toggle("open");
          actionEl.setAttribute("aria-expanded", open ? "true" : "false");
        }
      }

      if (action === "cd-select-auth-scheme") {
        const value = actionEl.dataset.value || "bearer";
        setVal("#cdProviderAuth", value);
        const label = $("#cdProviderAuthTrigger span");
        if (label) label.textContent = value;
        $all("#cdProviderAuthMenu .auth-scheme-option").forEach((opt) => {
          const sel = opt.dataset.value === value;
          opt.classList.toggle("selected", sel);
          opt.setAttribute("aria-selected", sel ? "true" : "false");
          const has = opt.querySelector("i.bi-check2");
          if (sel && !has) opt.insertAdjacentHTML("beforeend", '<i class="bi bi-check2"></i>');
          if (!sel && has) has.remove();
        });
        const wrap = $("#cdProviderAuthControl");
        if (wrap) wrap.classList.remove("open");
        const trig = $("#cdProviderAuthTrigger");
        if (trig) trig.setAttribute("aria-expanded", "false");
      }

      if (action === "cd-select-api-format") {
        const value = actionEl.dataset.cdApiFormat || "anthropic";
        claudeDesktopFormState.apiFormat = value;
        $all("#cdProviderFormatChoice .format-choice-button").forEach((btn) => {
          btn.classList.toggle("active", btn.dataset.cdApiFormat === value);
        });
        if (value === "openai_chat") {
          showToast("OpenAI Chat 模式需要本地代理协议转换,当前 Claude Desktop 集成仅支持 Anthropic 兼容,选择会保存但不会写入 inferenceGateway");
        }
      }


      if (action === "clear-desktop") {
        const target = await chooseCodexRestoreTarget();
        if (!target) return;
        const result = target.snapshotId
          ? await CCApi.restoreDesktopSnapshot(target.snapshotId)
          : await CCApi.clearDesktop();
        const route = routeFromHash();
        if (route === "dashboard") {
          await renderDashboard();
        } else if (route === "desktop") {
          await renderDesktop();
        } else if (route === "settings") {
          await refreshCodexSnapshotStatus();
        }
        const fellBackToLegacy = result && result.restored === false;
        showToast(t(fellBackToLegacy ? "toast.desktopClearedLegacy" : "toast.desktopCleared"));
      }

      if (action === "proxy-start") {
        await CCApi.startProxy($("#proxyPort") ? $("#proxyPort").value : 18080);
        await renderProxy();
        await renderDashboard();
        showToast(t("toast.proxyStarted"));
      }

      if (action === "proxy-stop") {
        await CCApi.stopProxy();
        await renderProxy();
        await renderDashboard();
        showToast(t("toast.proxyStopped"));
      }

      if (action === "proxy-toggle") {
        const currentStatus = await CCApi.getProxyStatus();
        if (currentStatus.running) {
          await CCApi.stopProxy();
          showToast(t("toast.proxyStopped"));
        } else {
          await CCApi.startProxy($("#proxyPort") ? $("#proxyPort").value : 18080);
          showToast(t("toast.proxyStarted"));
        }
        await renderProxy();
        await renderDashboard();
      }

      if (action === "clear-logs") {
        await CCApi.clearLogs();
        await renderProxy();
        showToast(t("toast.logsCleared"));
      }

      if (action === "open-log-dir") {
        try {
          await CCApi.openLogDir();
          showToast(t("toast.logDirOpened"));
        } catch (err) {
          showToast(t("toast.logDirOpenFailed"));
        }
      }

      if (action === "view-logs") {
        window.location.hash = "proxy";
      }

      if (action === "open-feedback") {
        openFeedbackModal();
      }

      if (action === "toggle-model-menu-mode") {
        const settings = await CCApi.getSettings();
        const next = !settings.exposeAllProviderModels;
        const saved = await CCApi.saveSettings({ exposeAllProviderModels: next });
        renderModelMenuModeState(saved);
        showToast(next ? t("toast.allModelsEnabled") : t("toast.singleModelEnabled"));
      }

      if (action === "check-provider-compatibility") {
        actionEl.disabled = true;
        try {
          const result = await CCApi.getProviderCompatibility();
          renderProviderCompatibilityList(result);
          showToast(t("toast.compatibilityChecked"));
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "check-update") {
        const result = await CCApi.checkUpdate($("#settingsUpdateUrl").value.trim());
        updateCheckCache = result;
        renderUpdateBadge(result);
        const message = result.updateAvailable
          ? `${t("toast.updateAvailable")} ${result.latestVersion}`
          : `${t("toast.noUpdate")} ${result.currentVersion}`;
        const status = $("#updateStatus");
        if (status) {
          status.textContent = message;
          status.classList.toggle("available", !!result.updateAvailable);
        }
        showToast(message);
      }

      if (action === "install-update") {
        if (!updateCheckCache?.updateAvailable) {
          updateCheckCache = await CCApi.checkUpdate($("#settingsUpdateUrl")?.value.trim() || "");
          renderUpdateBadge(updateCheckCache);
        }
        if (!updateCheckCache?.updateAvailable) {
          const message = `${t("toast.noUpdate")} ${updateCheckCache?.currentVersion || ""}`.trim();
          const status = $("#updateStatus");
          if (status) {
            status.textContent = message;
            status.classList.remove("available");
          }
          showToast(message);
          return;
        }
        if (!window.confirm(t("confirm.installUpdate"))) return;
        let keepBusyState = false;
        const status = $("#updateStatus");
        setUpdateInstallPhase("downloading");
        if (status) {
          status.textContent = t("toast.updateDownloading");
          status.classList.add("available");
        }
        try {
          const result = await CCApi.installUpdate($("#settingsUpdateUrl")?.value.trim() || "");
          updateCheckCache = result;
          keepBusyState = !!result.quitRequested;
          setUpdateInstallPhase(keepBusyState ? "installing" : "idle");
          renderUpdateBadge(result);
          const message = result.message || t("toast.updateInstallerStarted");
          if (status) {
            status.textContent = message;
            status.classList.toggle("available", !!result.updateAvailable);
          }
          showToast(message);
        } catch (error) {
          setUpdateInstallPhase("idle");
          throw error;
        } finally {
          if (!keepBusyState) setUpdateInstallPhase("idle");
        }
      }

      if (action === "backup-config") {
        await CCApi.createBackup();
        await refreshBackupList();
        showToast(t("toast.configBackedUp"));
      }

      if (action === "export-config") {
        const data = await CCApi.exportConfig();
        const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
        downloadJson(`codex-app-transfer-config-${stamp}.json`, data);
        showToast(t("toast.configExported"));
      }

      if (action === "choose-import-config") {
        $("#configImportFile").click();
      }

      if (action === "apply-provider-desktop") {
        await applyProviderToDesktop(actionEl);
      }
    } catch (error) {
      console.error(error);
      showToast(error.message || t("toast.requestFailed"));
    }
  }

  async function fillPreset(presetId) {
    if (!presetCache.length) presetCache = await CCApi.getPresets();
    const preset = presetCache.find((item) => item.id === presetId);
    if (!preset) return;
    editingProviderId = null;
    applyPresetToForm(preset);
  }

  // ── 用户反馈 modal ───────────────────────────────────────────────
  let feedbackAttachments = [];  // [{name, size, file}]
  let feedbackBsModal = null;

  function openFeedbackModal() {
    const el = $("#feedbackModal");
    if (!el) return;
    // 重置表单
    $("#feedbackTitle").value = "";
    $("#feedbackContactEmail").value = "";
    $("#feedbackBody").value = "";
    $("#feedbackIncludeDiagnostics").checked = true;
    feedbackAttachments = [];
    renderFeedbackAttachments();
    if (!feedbackBsModal) feedbackBsModal = new bootstrap.Modal(el);
    feedbackBsModal.show();
  }

  function renderFeedbackAttachments() {
    const list = $("#feedbackAttachmentList");
    if (!list) return;
    list.innerHTML = feedbackAttachments
      .map((a, i) => `<li class="feedback-attachment-item"><span>${escapeHtml(a.name)}</span><small>${formatBytes(a.size)}</small><button type="button" class="btn-link" data-idx="${i}">×</button></li>`)
      .join("");
    list.querySelectorAll("button[data-idx]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const idx = Number(btn.dataset.idx);
        feedbackAttachments.splice(idx, 1);
        renderFeedbackAttachments();
      });
    });
  }

  function addFeedbackFiles(files) {
    if (!files || !files.length) return;
    const max = 5 * 1024 * 1024;
    for (const f of files) {
      if (f.size > max) {
        showToast(tFmt("feedback.tooLargeFile", { name: f.name }));
        continue;
      }
      feedbackAttachments.push({ name: f.name, size: f.size, file: f });
    }
    renderFeedbackAttachments();
  }

  function formatBytes(n) {
    if (n < 1024) return `${n}B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}KB`;
    return `${(n / 1024 / 1024).toFixed(2)}MB`;
  }

  async function submitFeedback() {
    const titleEl = $("#feedbackTitle");
    const contactEmailEl = $("#feedbackContactEmail");
    const bodyEl = $("#feedbackBody");
    const submitBtn = $("#feedbackSubmitBtn");
    if (!bodyEl) return;

    const title = (titleEl?.value || "").trim();
    const contactEmail = (contactEmailEl?.value || "").trim();
    const body = bodyEl.value.trim();
    if (!body) {
      showToast(t("feedback.bodyRequired"));
      bodyEl.focus();
      return;
    }

    submitBtn.disabled = true;
    const originalText = submitBtn.textContent;
    submitBtn.textContent = t("feedback.submitting");

    try {
      // 把附件转成 base64 嵌进 JSON,避开 pywebview WebKit 对 FormData 的 bug
      const attachments = [];
      for (const a of feedbackAttachments) {
        try {
          const b64 = await fileToBase64(a.file);
          const isImg = /^image\//.test(a.file.type || "");
          const safeName = String(a.name || `attachment-${Date.now()}.bin`)
            .replace(/[\x00-\x1f\\/]/g, "_")
            .slice(0, 200);
          attachments.push({
            kind: isImg ? "screenshot" : "log",
            name: safeName,
            content_type: a.file.type || "application/octet-stream",
            content_b64: b64,
          });
        } catch (innerErr) {
          console.warn("[feedback] skipped attachment:", innerErr, a);
        }
      }

      const payload = {
        title,
        contact_email: contactEmail,
        body,
        include_diagnostics: $("#feedbackIncludeDiagnostics").checked,
        attachments,
      };

      const result = await CCApi.submitFeedback(payload);
      if (feedbackBsModal) feedbackBsModal.hide();
      showToast(tFmt("feedback.successToast", { id: result.id || "" }));
    } catch (err) {
      console.error("[feedback] submit failed:", err);
      let msg = err && err.message ? err.message : String(err);
      if (msg.includes("did not match the expected pattern")) {
        msg = "请求体构造异常,请重试或去掉附件";
      }
      showToast(tFmt("feedback.failToast", { message: msg }));
    } finally {
      submitBtn.disabled = false;
      submitBtn.textContent = originalText;
    }
  }

  function fileToBase64(file) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => {
        const r = String(reader.result || "");
        const i = r.indexOf(",");
        resolve(i >= 0 ? r.slice(i + 1) : r);
      };
      reader.onerror = () => reject(reader.error || new Error("FileReader failed"));
      reader.readAsDataURL(file);
    });
  }

  function bindFeedbackEvents() {
    const dropzone = $("#feedbackDropzone");
    const fileInput = $("#feedbackFiles");
    if (dropzone && fileInput) {
      dropzone.addEventListener("click", (e) => {
        // 不要在点击删除按钮 / 列表项时触发
        if (e.target.closest(".feedback-attachment-item")) return;
        fileInput.click();
      });
      fileInput.addEventListener("change", () => {
        addFeedbackFiles(Array.from(fileInput.files));
        fileInput.value = "";
      });
      dropzone.addEventListener("dragover", (e) => {
        e.preventDefault();
        dropzone.classList.add("dragover");
      });
      dropzone.addEventListener("dragleave", () => dropzone.classList.remove("dragover"));
      dropzone.addEventListener("drop", (e) => {
        e.preventDefault();
        dropzone.classList.remove("dragover");
        addFeedbackFiles(Array.from(e.dataTransfer.files));
      });
    }
    document.addEventListener("paste", (e) => {
      // 粘贴截图(只有 modal 打开时响应)
      const modalEl = $("#feedbackModal");
      if (!modalEl?.classList.contains("show")) return;
      const items = e.clipboardData?.items || [];
      for (const it of items) {
        if (it.kind === "file" && /^image\//.test(it.type)) {
          const f = it.getAsFile();
          if (f) addFeedbackFiles([new File([f], f.name || `pasted-${Date.now()}.png`, { type: f.type })]);
        }
      }
    });
    const submitBtn = $("#feedbackSubmitBtn");
    if (submitBtn) submitBtn.addEventListener("click", submitFeedback);
  }

  function bindEvents() {
    window.addEventListener("hashchange", () => renderRoute(routeFromHash()));
    window.addEventListener("cc:i18n", () => renderRoute(routeFromHash()));
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      if (currentTheme === "dark") applyTheme("dark");
    });

    document.addEventListener("click", async (event) => {
      if (!event.target.closest(".mapping-slot-menu-wrap")) {
        closeProviderSlotMenu();
      }
      if (!event.target.closest(".baseurl-input-wrap")) {
        closeBaseUrlMenu();
      }
      if (!event.target.closest(".provider-model-input-wrap")) {
        closeProviderModelMenu();
      }
      const langButton = event.target.closest("[data-lang]");
      if (langButton) CCI18n.apply(langButton.dataset.lang);
      const addLink = event.target.closest("a[href='#providers/add']");
      if (addLink) {
        editingProviderId = null;
        selectedPreset = null;
        updatePresetSelection();
      }
      const themeButton = event.target.closest("[data-theme-action]");
      if (themeButton) {
        const nextTheme = applyTheme(themeButton.dataset.themeAction);
        await CCApi.saveSettings({ theme: nextTheme });
      }
      const presetButton = event.target.closest("[data-preset]");
      if (presetButton && presetButton.closest("#presetList")) {
        event.preventDefault();
        await fillPreset(presetButton.dataset.preset);
        return;
      }
      const presetModelOption = event.target.closest("[data-preset-model-option]");
      if (presetModelOption) {
        applyPresetModelOption(presetModelOption.dataset.presetModelOption, presetModelOption.checked);
        return;
      }
      await handleAction(event.target);
    });

    document.addEventListener("change", (event) => {
      const mappingInput = event.target.closest("[data-provider-model-input]");
      if (mappingInput) {
        updateProviderModelInput(mappingInput.dataset.providerModelInput, mappingInput.value);
        renderPresetOptions(selectedPreset, collectProviderMappings());
      }
      if (event.target.id === "providerBaseUrl") {
        renderBaseUrlOptions();
      }
      if (event.target.id === "providerApiFormatSelect") {
        updateApiFormatSelectDetail(event.target.value);
        formApiFormatValue = event.target.value;
        // R1 PR-7:切换 apiFormat 时同步 OAuth / grok_web row 显隐
        setOauthRowState(event.target.value);
        setGrokWebRowState(event.target.value);
      }
    });

    // P2.2 OAuth login/logout buttons —— delegate via closest() 防 future 嵌套
    // <i> icon 时 event.target 是 <i> 而 .id 为空导致 dead button (silent-failure L1 修)
    document.addEventListener("click", (event) => {
      if (event.target?.closest?.("#oauthLoginBtn")) {
        handleOauthLogin();
      } else if (event.target?.closest?.("#oauthLogoutBtn")) {
        handleOauthLogout();
      }
    });

    document.addEventListener("input", (event) => {
      if (event.target.id === "providerBaseUrl") {
        renderBaseUrlOptions();
      }
      const customLabelInput = event.target.closest("[data-custom-model-label]");
      if (customLabelInput) {
        const customKey = customLabelInput.dataset.customModelLabel;
        providerFormCustomLabels[customKey] = customLabelInput.value;
      }
      const mappingInput = event.target.closest("[data-provider-model-input]");
      if (!mappingInput) return;
      updateProviderModelInput(mappingInput.dataset.providerModelInput, mappingInput.value);
    });

    document.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        closeBaseUrlMenu();
        closeProviderSlotMenu();
        closeProviderModelMenu();
      }
    });

    $("#providerForm").addEventListener("submit", async (event) => {
      event.preventDefault();
      try {
        const wasEditing = !!editingProviderId;
        await saveProviderFromForm();
        if (editingProviderId) {
          showToast(wasEditing ? t("toast.providerUpdated") : t("toast.providerSaved"));
        } else {
          showToast(t("toast.providerSaved"));
        }
        editingProviderId = null;
        selectedPreset = null;
        window.location.hash = "providers";
      } catch (error) {
        console.error(error);
        showToast(error.message || t("toast.requestFailed"));
      }
    });

    $("#modelProvider")?.addEventListener("change", renderMappingCards);
    $("#settingsProxyPort").addEventListener("change", saveSettingsFromForm);
    $("#settingsAdminPort").addEventListener("change", saveSettingsFromForm);
    $("#settingsUpdateUrl").addEventListener("change", saveSettingsFromForm);
    $("#autoApplyOnStart")?.addEventListener("change", saveSettingsFromForm);
   $("#autoUnlockCodexPlugins")?.addEventListener("change", saveSettingsFromForm);
    $("#autoWakeCodexPet")?.addEventListener("change", saveSettingsFromForm);

   // Plugin Unlock 按钮事件
    $("[data-action=plugin-unlock-start]")?.addEventListener("click", async () => {
      try {
        await CCAPI.pluginUnlock.start();
        showToast(t("pluginUnlock.started") || "解锁服务已启动");
        setTimeout(refreshPluginUnlockStatus, 1000);
      } catch (e) { showToast(e.message); }
    });
    $("[data-action=plugin-unlock-stop]")?.addEventListener("click", async () => {
      try {
        await CCAPI.pluginUnlock.stop();
        showToast(t("pluginUnlock.stopped") || "解锁服务已停止");
        setTimeout(refreshPluginUnlockStatus, 500);
      } catch (e) { showToast(e.message); }
    });
    $("[data-action=plugin-unlock-reinject]")?.addEventListener("click", async () => {
      try {
        await CCAPI.pluginUnlock.reinject();
        showToast(t("pluginUnlock.reinjecting") || "正在重新注入...");
        setTimeout(refreshPluginUnlockStatus, 1500);
      } catch (e) { showToast(e.message); }
    });
    $("#exposeAllProviderModels").addEventListener("change", saveSettingsFromForm);
    $("#restoreCodexOnExit")?.addEventListener("change", saveSettingsFromForm);
    $("#configImportFile")?.addEventListener("change", (event) => {
      importConfigFile(event.target.files?.[0]);
    });
    $("#restartReminderLater")?.addEventListener("click", dismissRestartReminderLater);
    $("#restartReminderNow")?.addEventListener("click", restartCodexAppNow);

    $("#confirmDelete").addEventListener("click", async () => {
      if (!pendingDeleteId) return;
      try {
        await CCApi.deleteProvider(pendingDeleteId);
        pendingDeleteId = null;
        deleteModal.hide();
        if (routeFromHash() === "dashboard") {
          await renderDashboard();
        } else {
          await renderProviders();
        }
        showToast(t("toast.providerDeleted"));
      } catch (error) {
        console.error(error);
        showToast(error.message || t("toast.requestFailed"));
      }
    });
  }

  document.addEventListener("DOMContentLoaded", async () => {
    // **M3 i18n apply 时序修**:第一时间用 localStorage cache + navigator.language
    // 同步 apply 一次,消除 DOMContentLoaded → getSettings (async) 之间空窗内
    // EN 用户看 zh 默认占位 / zh 用户看 EN 的乱码体验。Settings 拿到后再 apply
    // 第二次(idempotent,只在 language 不同时真改 DOM)
    CCI18n.apply(CCI18n.cachedLanguage());

    deleteModal = new bootstrap.Modal($("#deleteModal"));
    restartReminderModal = new bootstrap.Modal($("#restartReminderModal"), {
      backdrop: "static",
      keyboard: false,
    });
    toast = new bootstrap.Toast($("#appToast"), { delay: 2200 });
    bindEvents();
    bindFeedbackEvents();
    const settings = await CCApi.getSettings();
    const finalLang = settings.language || "zh";
    if (finalLang !== CCI18n.language) {
      // backend settings 跟 cache/navigator 不一致才再 apply,正常路径无 op
      CCI18n.apply(finalLang);
    }
    applyTheme(settings.theme || "default");
    if (!window.location.hash) window.location.hash = "dashboard";
    await renderRoute(routeFromHash());
  });
})();
