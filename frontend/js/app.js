(function () {
  const routes = ["dashboard", "providers/add", "providers", "desktop", "proxy", "settings", "guide"];
  const modelMeta = [
    { key: "sonnet", title: "Sonnet", icon: "bi-stars", source: "claude-sonnet-4-6" },
    { key: "haiku", title: "Haiku", icon: "bi-leaf", source: "claude-haiku-3-5" },
    { key: "opus", title: "Opus", icon: "bi-box", source: "claude-opus-4-7" },
  ];
  let pendingDeleteId = null;
  let selectedPreset = null;
  let presetCache = [];
  let formApiFormat = "Anthropic";
  let editingProviderId = null;
  let deleteModal = null;
  let toast = null;

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

  function t(key) {
    return CCI18n.t(key);
  }

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

  function emptyMappings() {
    return {
      sonnet: "",
      haiku: "",
      opus: "",
      default: "",
    };
  }

  function normalizeMappings(mappings = {}) {
    const normalized = { ...emptyMappings(), ...mappings };
    normalized.default = normalized.default || normalized.sonnet || normalized.haiku || normalized.opus || "";
    return normalized;
  }

  function defaultKeyFromMappings(mappings = {}) {
    const normalized = normalizeMappings(mappings);
    return modelMeta.find((model) => normalized[model.key] === normalized.default)?.key || "sonnet";
  }

  function formMappingMarkup(mappings = {}) {
    const normalized = normalizeMappings(mappings);
    return modelMeta.map((model) => `
      <article class="form-mapping-card">
        <div class="mapping-title">
          <span class="mapping-icon ${model.key}"><i class="bi ${model.icon}"></i></span>
          <div>
            <strong>${model.title}</strong>
            <span>${model.source}</span>
          </div>
        </div>
        <input class="form-control" data-provider-model-input="${model.key}" value="${escapeHtml(normalized[model.key] || "")}" placeholder="${escapeHtml(model.source)}">
      </article>
    `).join("");
  }

  function setProviderMappings(mappings = {}) {
    const stack = $("#providerMappingStack");
    if (!stack) return;
    const normalized = normalizeMappings(mappings);
    stack.innerHTML = formMappingMarkup(normalized);
    const defaultSelect = $("#providerDefaultModel");
    if (defaultSelect) defaultSelect.value = defaultKeyFromMappings(normalized);
    const result = $("#providerModelFetchResult");
    if (result) result.textContent = "";
  }

  function renderPresetOptions(preset = null) {
    const container = $("#providerPresetOptions");
    if (!container) return;
    const options = preset?.modelOptions && typeof preset.modelOptions === "object"
      ? Object.entries(preset.modelOptions)
      : [];
    if (!options.length) {
      container.hidden = true;
      container.innerHTML = "";
      return;
    }
    container.hidden = false;
    container.innerHTML = options.map(([id, option]) => `
      <label class="preset-option-item">
        <input class="form-check-input" type="checkbox" data-preset-model-option="${escapeHtml(id)}">
        <span>
          <strong>${escapeHtml(option.label || id)}</strong>
          <small>${escapeHtml(option.description || "")}</small>
        </span>
      </label>
    `).join("");
  }

  function applyPresetModelOption(optionId, enabled) {
    const option = selectedPreset?.modelOptions?.[optionId];
    if (!option) return;
    setProviderMappings(enabled ? option.models : selectedPreset.models || emptyMappings());
    showToast(`${option.label || optionId} ${t("providersAdd.optionApplied")}`);
  }

  function collectProviderMappings() {
    const mappings = emptyMappings();
    $all("[data-provider-model-input]").forEach((input) => {
      mappings[input.dataset.providerModelInput] = input.value.trim();
    });
    const defaultKey = $("#providerDefaultModel")?.value || "sonnet";
    mappings.default = mappings[defaultKey] || mappings.sonnet || mappings.haiku || mappings.opus || "";
    return mappings;
  }

  function providerPayloadFromForm(includeModels = true) {
    const apiKey = $("#providerApiKey").value.trim();
    const payload = {
      name: $("#providerName").value.trim(),
      baseUrl: $("#providerBaseUrl").value.trim(),
      authScheme: $("#providerAuth").value,
      apiFormat: formApiFormat,
      extraHeaders: selectedPreset?.extraHeaders || {},
    };
    if (apiKey) {
      payload.apiKey = apiKey;
    }
    if (includeModels) {
      payload.models = collectProviderMappings();
    }
    return payload;
  }

  function providerCardMarkup(provider) {
    const mapping = [provider.mappings.sonnet, provider.mappings.haiku, provider.mappings.opus]
      .filter(Boolean)
      .slice(0, 2)
      .join(" / ");
    const providerId = escapeHtml(provider.id);
    const providerName = escapeHtml(provider.name);
    const providerUrl = escapeHtml(provider.baseUrl);
    const providerHref = escapeHtml(safeHttpUrl(provider.baseUrl));
    const mappingText = escapeHtml(mapping || provider.apiFormat);
    return `
      <article class="provider-switch-card ${provider.default ? "active" : ""}">
        <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
        <span class="provider-logo">${iconMarkup(provider)}</span>
        <span class="provider-main">
          <strong>${providerName}</strong>
          <a class="truncate" href="${providerHref}" target="_blank" rel="noreferrer">${providerUrl}</a>
        </span>
        <span class="provider-meta truncate">${mappingText}</span>
        <span class="provider-actions">
          <button class="btn btn-primary compact-enable" type="button" data-action="set-default" data-id="${providerId}" ${provider.default ? "disabled" : ""}>
            <i class="bi bi-play-fill"></i><span>${provider.default ? t("status.default") : t("providers.enable")}</span>
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

  async function renderProviderCards(targetSelector) {
    const target = $(targetSelector);
    if (!target) return;
    const providers = await CCApi.getProviders();
    if (!providers.length) {
      const presets = await CCApi.getPresets();
      target.innerHTML = presets.map((preset) => `
        <button class="provider-switch-card preset-card" type="button" data-action="new-from-preset" data-preset="${escapeHtml(preset.id)}">
          <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
          <span class="provider-logo">${iconMarkup(preset)}</span>
          <span class="provider-main"><strong>${escapeHtml(preset.name)}</strong><span class="truncate">${escapeHtml(preset.baseUrl)}</span></span>
          <span class="provider-meta">${escapeHtml(preset.apiFormat)}</span>
          <span class="provider-actions"><span class="compact-enable ghost"><i class="bi bi-plus-lg"></i><span>${t("providers.add")}</span></span></span>
        </button>
      `).join("");
      return;
    }
    target.innerHTML = providers.map(providerCardMarkup).join("");
  }

  async function renderDashboard() {
    const status = await CCApi.getStatus();
    const activities = await CCApi.getActivities();
    await renderProviderCards("#dashboardProviderCards");
    const desktopIcon = $("#dashboardDesktopIcon");
    desktopIcon.classList.toggle("muted", !status.desktopConfigured);
    desktopIcon.innerHTML = `<i class="bi ${status.desktopConfigured ? "bi-check-lg" : "bi-exclamation-lg"}"></i>`;
    const desktopStatus = $("#dashboardDesktopStatus");
    desktopStatus.classList.toggle("muted-text", !status.desktopConfigured);
    desktopStatus.textContent = status.desktopConfigured ? t("status.configured") : t("status.notConfigured");
    $("#dashboardProxyStatus").textContent = status.proxyRunning ? `${t("status.running")} :${status.proxyPort}` : t("status.stopped");
    $("#dashboardProviderName").textContent = status.activeProvider.name;
    $("#activityList").innerHTML = activities.map((item) => (
      `<div class="activity-row"><time>${escapeHtml(item.time)}</time><span>${escapeHtml(item.text)}</span></div>`
    )).join("");
  }

  async function renderPresets() {
    presetCache = await CCApi.getPresets();
    $("#presetList").innerHTML = presetCache.map((preset) => `
      <button class="preset-item" type="button" data-preset="${escapeHtml(preset.id)}">
        <span class="preset-logo">${iconMarkup(preset)}</span>
        <span><strong>${escapeHtml(preset.name)}</strong><span>${escapeHtml(preset.baseUrl)}</span></span>
        <i class="bi bi-chevron-right"></i>
      </button>
    `).join("");
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

  function resetProviderForm() {
    editingProviderId = null;
    selectedPreset = null;
    renderPresetOptions(null);
    setProviderFormMode("providersAdd.title");
    $("#providerName").value = "";
    $("#providerBaseUrl").value = "";
    setApiKeyInputState(false);
    $("#providerAuth").value = "bearer";
    formApiFormat = "Anthropic";
    setProviderMappings(emptyMappings());
  }

  function applyPresetToForm(preset, notify = true) {
    $("#providerName").value = preset.name;
    $("#providerBaseUrl").value = preset.baseUrl;
    $("#providerAuth").value = preset.authScheme;
    setApiKeyInputState(false);
    selectedPreset = preset;
    formApiFormat = preset.apiFormat === "OpenAI" ? "OpenAI" : "Anthropic";
    setProviderMappings(preset.models || emptyMappings());
    renderPresetOptions(preset);
    if (notify) showToast(`${preset.name} ${t("toast.presetFilled")}`);
  }

  async function fillProviderForEdit(providerId) {
    const providers = await CCApi.getProviders();
    const provider = providers.find((item) => item.id === providerId);
    if (!provider) return;
    editingProviderId = provider.id;
    selectedPreset = { models: provider.mappings, extraHeaders: provider.extraHeaders || {} };
    renderPresetOptions(null);
    setProviderFormMode("providersAdd.editTitle");
    $("#providerName").value = provider.name;
    $("#providerBaseUrl").value = provider.baseUrl;
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
    $("#providerAuth").value = provider.authScheme;
    formApiFormat = provider.apiFormat === "openai" ? "OpenAI" : "Anthropic";
    setProviderMappings(provider.mappings || emptyMappings());
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
    await renderProviderCards("#providerRows");
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
      const defaultValue = provider.mappings.default || provider.mappings.sonnet || "";
      const defaultKey = modelMeta.find((model) => provider.mappings[model.key] === defaultValue)?.key || "sonnet";
      defaultSelect.value = defaultKey;
    }
    const result = $("#modelFetchResult");
    if (result) result.textContent = "";
    $("#mappingStack").innerHTML = modelMeta.map((model) => `
      <article class="mapping-card">
        <div class="mapping-title">
          <span class="mapping-icon ${model.key}"><i class="bi ${model.icon}"></i></span>
          <strong>${model.title}</strong>
          <span class="alias-pill">${model.title}</span>
        </div>
        <input class="form-control form-control-lg" data-model-input="${model.key}" value="${escapeHtml(provider.mappings[model.key] || "")}">
        <span class="source-model"><i class="bi bi-arrow-left"></i>${model.source}</span>
      </article>
    `).join("");
  }

  async function renderDesktop() {
    const desktop = await CCApi.getDesktopStatus();
    const entries = Object.entries(desktop.config);
    $("#desktopConfiguredText").textContent = desktop.configured ? t("status.configured") : t("status.notConfigured");
    $("#desktopConfigList").innerHTML = entries.map(([key, value]) => `
      <div class="config-row"><i class="bi bi-check-circle-fill"></i><span>${escapeHtml(key)}:</span><code>${escapeHtml(Array.isArray(value) ? JSON.stringify(value) : value)}</code></div>
    `).join("");
    $("#desktopJson").textContent = JSON.stringify(desktop.config, null, 2);
  }

  async function renderProxy() {
    const status = await CCApi.getStatus();
    const proxyStatus = await CCApi.getProxyStatus();
    const logs = await CCApi.getProxyLogs();
    $("#proxyPort").value = status.proxyPort;
    $("#settingsProxyPort").value = status.proxyPort;
    $("#proxyStateText").textContent = status.proxyRunning ? t("status.running") : t("status.stopped");
    const logEl = $("#proxyLog");
    logEl.innerHTML = logs.map((line) => `
      <div class="log-line"><span>${escapeHtml(line.at)}</span><span class="log-level ${escapeHtml(line.level)}">${escapeHtml(line.level.toUpperCase())}</span><span>${escapeHtml(line.message)}</span></div>
    `).join("");
    if ($("#autoScroll").checked) logEl.scrollTop = logEl.scrollHeight;
    const stats = [
      { label: t("proxy.stats.total"), value: proxyStatus.stats.total, icon: "bi-list-ul" },
      { label: t("proxy.stats.success"), value: proxyStatus.stats.success, icon: "bi-check-circle" },
      { label: t("proxy.stats.failed"), value: proxyStatus.stats.failed, icon: "bi-x-circle", danger: true },
      { label: t("proxy.stats.today"), value: proxyStatus.stats.today, icon: "bi-calendar3" },
    ];
    $("#proxyStats").innerHTML = stats.map((stat) => `
      <article class="stat-card ${stat.danger ? "danger" : ""}"><i class="bi ${stat.icon}"></i><div><span>${stat.label}</span><strong>${stat.value}</strong></div></article>
    `).join("");
  }

  async function renderSettings() {
    const settings = await CCApi.getSettings();
    $("#settingsProxyPort").value = settings.proxyPort;
    $("#settingsAdminPort").value = settings.adminPort;
    $("#autoStart").checked = settings.autoStart;
    $("#settingsUpdateUrl").value = settings.updateUrl || "";
    await refreshBackupList();
  }

  async function renderRoute(route) {
    $all(".page").forEach((page) => page.classList.toggle("active", page.dataset.page === route));
    $all(".route-tab").forEach((tab) => {
      const key = route.startsWith("providers") ? "providers" : route;
      tab.classList.toggle("active", tab.dataset.nav === key);
    });
    if (route === "dashboard") await renderDashboard();
    if (route === "providers/add") await renderProviderForm();
    if (route === "providers") await renderProviders();
    if (route === "desktop") await renderDesktop();
    if (route === "proxy") await renderProxy();
    if (route === "settings") await renderSettings();
  }

  let currentTheme = "light";

  function applyTheme(theme) {
    if (theme === "toggle") {
      theme = currentTheme === "dark" ? "light" : "dark";
    }
    currentTheme = theme;
    const resolved = theme === "auto" && window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : theme === "auto" ? "light" : theme;
    document.documentElement.setAttribute("data-bs-theme", resolved);
    const icon = $(".theme-btn i");
    if (icon) icon.className = resolved === "dark" ? "bi bi-sun-fill" : "bi bi-moon-stars-fill";
  }

  async function saveSettingsFromForm() {
    const settings = {
      proxyPort: Number($("#settingsProxyPort").value),
      adminPort: Number($("#settingsAdminPort").value),
      autoStart: $("#autoStart").checked,
      updateUrl: $("#settingsUpdateUrl").value.trim(),
    };
    await CCApi.saveSettings(settings);
    $("#proxyPort").value = settings.proxyPort;
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

  async function saveProviderFromForm() {
    const payload = providerPayloadFromForm(true);
    if (editingProviderId) {
      const provider = await CCApi.updateProvider(editingProviderId, payload);
      editingProviderId = provider.id || editingProviderId;
      return provider;
    }
    const provider = await CCApi.addProvider(payload);
    editingProviderId = provider.id;
    return provider;
  }

  async function applyProviderToDesktop(actionEl) {
    const form = $("#providerForm");
    if (form && !form.reportValidity()) return;
    if (!window.confirm(t("confirm.providerApplyDesktop"))) return;

    actionEl.disabled = true;
    try {
      const provider = await saveProviderFromForm();
      await CCApi.setDefaultProvider(provider.id);
      await CCApi.configureDesktop();
      await CCApi.startProxy();
      editingProviderId = null;
      selectedPreset = null;
      window.location.hash = "dashboard";
      showToast(t("toast.providerAppliedDesktop"));
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
        await CCApi.setDefaultProvider(actionEl.dataset.id);
        await renderProviderCards("#dashboardProviderCards");
        await renderProviders();
        await renderDashboard();
        showToast(t("toast.defaultUpdated"));
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

      if (action === "test-provider") {
        const resultEl = $(`[data-speed-for="${actionEl.dataset.id}"]`);
        actionEl.disabled = true;
        if (resultEl) {
          resultEl.textContent = t("providers.testing");
          resultEl.classList.remove("bad");
        }
        try {
          const result = await CCApi.testProvider(actionEl.dataset.id);
          if (resultEl) {
            resultEl.textContent = result.message || `${result.latencyMs} ms`;
            resultEl.classList.toggle("bad", result.ok === false);
          }
          showToast(result.message || t("providers.testDone"));
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
          const hasTypedKey = !!$("#providerApiKey").value.trim();
          const result = editingProviderId && !hasTypedKey
            ? await CCApi.testProvider(editingProviderId)
            : await CCApi.testProviderPayload(providerPayloadFromForm(false));
          resultEl.textContent = result.message || `${result.latencyMs} ms`;
          resultEl.classList.toggle("bad", result.ok === false);
          showToast(result.message || t("providers.testDone"));
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "fetch-form-models") {
        const resultEl = $("#providerModelFetchResult");
        actionEl.disabled = true;
        if (resultEl) resultEl.textContent = t("models.fetching");
        try {
          const hasTypedKey = !!$("#providerApiKey").value.trim();
          const result = editingProviderId && !hasTypedKey
            ? await CCApi.autofillProviderModels(editingProviderId)
            : await CCApi.fetchProviderModelsPayload(providerPayloadFromForm(false));
          setProviderMappings(result.suggested || emptyMappings());
          if (resultEl) resultEl.textContent = `${t("models.fetched")} ${(result.models || []).length}`;
          showToast(t("toast.modelsAutofilled"));
        } finally {
          actionEl.disabled = false;
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
        const defaultKey = $("#defaultModel")?.value || "sonnet";
        mappings.default = mappings[defaultKey] || mappings.sonnet || mappings.haiku || mappings.opus || "";
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
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "reset-models") {
        await renderMappingCards();
        showToast(t("toast.modelsReset"));
      }

      if (action === "apply-desktop") {
        if (!window.confirm(t("confirm.desktopApply"))) return;
        await CCApi.configureDesktop();
        await renderDesktop();
        showToast(t("toast.desktopApplied"));
      }

      if (action === "clear-desktop") {
        if (!window.confirm(t("confirm.desktopClear"))) return;
        await CCApi.clearDesktop();
        await renderDesktop();
        showToast(t("toast.desktopCleared"));
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

      if (action === "clear-logs") {
        await CCApi.clearLogs();
        await renderProxy();
        showToast(t("toast.logsCleared"));
      }

      if (action === "view-logs") {
        window.location.hash = "proxy";
      }

      if (action === "check-update") {
        const result = await CCApi.checkUpdate($("#settingsUpdateUrl").value.trim());
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

      if (action === "backup-config") {
        await CCApi.createBackup();
        await refreshBackupList();
        showToast(t("toast.configBackedUp"));
      }

      if (action === "export-config") {
        const data = await CCApi.exportConfig();
        const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
        downloadJson(`cc-desktop-switch-config-${stamp}.json`, data);
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

  function bindEvents() {
    window.addEventListener("hashchange", () => renderRoute(routeFromHash()));
    window.addEventListener("cc:i18n", () => renderRoute(routeFromHash()));
    window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
      const activeTheme = $(".theme-segment .btn.active")?.dataset.themeAction || "light";
      if (activeTheme === "auto") applyTheme("auto");
    });

    document.addEventListener("click", async (event) => {
      const langButton = event.target.closest("[data-lang]");
      if (langButton) CCI18n.apply(langButton.dataset.lang);
      const addLink = event.target.closest("a[href='#providers/add']");
      if (addLink) {
        editingProviderId = null;
        selectedPreset = null;
      }
      const themeButton = event.target.closest("[data-theme-action]");
      if (themeButton) applyTheme(themeButton.dataset.themeAction);
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
    $("#autoStart").addEventListener("change", saveSettingsFromForm);
    $("#configImportFile")?.addEventListener("change", (event) => {
      importConfigFile(event.target.files?.[0]);
    });

    $("#confirmDelete").addEventListener("click", async () => {
      if (!pendingDeleteId) return;
      try {
        await CCApi.deleteProvider(pendingDeleteId);
        pendingDeleteId = null;
        deleteModal.hide();
        await renderProviders();
        showToast(t("toast.providerDeleted"));
      } catch (error) {
        console.error(error);
        showToast(error.message || t("toast.requestFailed"));
      }
    });
  }

  document.addEventListener("DOMContentLoaded", async () => {
    deleteModal = new bootstrap.Modal($("#deleteModal"));
    toast = new bootstrap.Toast($("#appToast"), { delay: 2200 });
    bindEvents();
    CCI18n.apply("zh");
    if (!window.location.hash) window.location.hash = "dashboard";
    await renderRoute(routeFromHash());
  });
})();
