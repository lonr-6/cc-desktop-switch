(function () {
  const routes = ["dashboard", "providers/add", "providers", "models", "desktop", "proxy", "settings", "guide"];
  const modelMeta = [
    { key: "sonnet", title: "Sonnet", icon: "bi-stars", source: "claude-sonnet-4-6" },
    { key: "haiku", title: "Haiku", icon: "bi-leaf", source: "claude-haiku-3-5" },
    { key: "opus", title: "Opus", icon: "bi-box", source: "claude-opus-4-7" },
  ];
  let pendingDeleteId = null;
  let selectedPreset = null;
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

  function providerPayloadFromForm(includeModels = true) {
    const apiFormat = $("[name='apiFormat']:checked")?.value || "Anthropic";
    const payload = {
      name: $("#providerName").value.trim(),
      baseUrl: $("#providerBaseUrl").value.trim(),
      apiKey: $("#providerApiKey").value.trim(),
      authScheme: $("#providerAuth").value,
      apiFormat,
      extraHeaders: selectedPreset?.extraHeaders || {},
    };
    if (includeModels) {
      payload.models = selectedPreset?.models || {
        sonnet: "",
        haiku: "",
        opus: "",
        default: "",
      };
    }
    return payload;
  }

  function providerCardMarkup(provider) {
    const mapping = [provider.mappings.sonnet, provider.mappings.haiku, provider.mappings.opus]
      .filter(Boolean)
      .slice(0, 2)
      .join(" / ");
    return `
      <article class="provider-switch-card ${provider.default ? "active" : ""}">
        <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
        <span class="provider-logo">${iconMarkup(provider)}</span>
        <span class="provider-main">
          <strong>${provider.name}</strong>
          <a class="truncate" href="${provider.baseUrl}" target="_blank" rel="noreferrer">${provider.baseUrl}</a>
        </span>
        <span class="provider-meta truncate">${mapping || provider.apiFormat}</span>
        <span class="provider-actions">
          <button class="btn btn-primary compact-enable" type="button" data-action="set-default" data-id="${provider.id}" ${provider.default ? "disabled" : ""}>
            <i class="bi bi-play-fill"></i><span>${provider.default ? t("status.default") : t("providers.enable")}</span>
          </button>
          <button class="icon-action" type="button" data-action="test-provider" data-id="${provider.id}" title="${t("providers.testSpeed")}" aria-label="${t("providers.testSpeed")}"><i class="bi bi-lightning-charge"></i></button>
          <button class="icon-action" type="button" data-action="edit-provider" data-id="${provider.id}" title="${t("common.edit")}" aria-label="${t("common.edit")}"><i class="bi bi-pencil-square"></i></button>
          <button class="icon-action" type="button" data-action="copy-url" data-url="${provider.baseUrl}" title="${t("common.copy")}" aria-label="${t("common.copy")}"><i class="bi bi-copy"></i></button>
          <a class="icon-action" href="#models" title="${t("nav.models")}" aria-label="${t("nav.models")}"><i class="bi bi-diagram-3"></i></a>
          <a class="icon-action" href="#proxy" title="${t("nav.proxy")}" aria-label="${t("nav.proxy")}"><i class="bi bi-terminal"></i></a>
          <button class="icon-action danger" type="button" data-action="delete-provider" data-id="${provider.id}" title="${t("common.delete")}" aria-label="${t("common.delete")}"><i class="bi bi-trash"></i></button>
        </span>
        <span class="speed-result inline" data-speed-for="${provider.id}"></span>
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
        <button class="provider-switch-card preset-card" type="button" data-action="new-from-preset" data-preset="${preset.id}">
          <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
          <span class="provider-logo">${iconMarkup(preset)}</span>
          <span class="provider-main"><strong>${preset.name}</strong><span class="truncate">${preset.baseUrl}</span></span>
          <span class="provider-meta">${preset.apiFormat}</span>
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
      `<div class="activity-row"><time>${item.time}</time><span>${item.text}</span></div>`
    )).join("");
  }

  async function renderPresets() {
    const presets = await CCApi.getPresets();
    $("#presetList").innerHTML = presets.map((preset) => `
      <button class="preset-item" type="button" data-preset="${preset.id}">
        <span class="preset-logo">${iconMarkup(preset)}</span>
        <span><strong>${preset.name}</strong><span>${preset.baseUrl}</span></span>
        <i class="bi bi-chevron-right"></i>
      </button>
    `).join("");
  }

  function setProviderFormMode(titleKey) {
    const title = $("#page-providers-add .page-title h1");
    if (title) title.textContent = t(titleKey);
    const submit = $("#providerForm button[type='submit']");
    if (submit) submit.textContent = t("common.save");
    const result = $("#formSpeedResult");
    if (result) {
      result.textContent = "";
      result.className = "speed-result";
    }
  }

  function resetProviderForm() {
    editingProviderId = null;
    selectedPreset = null;
    setProviderFormMode("providersAdd.title");
    $("#providerName").value = "";
    $("#providerBaseUrl").value = "";
    $("#providerApiKey").value = "";
    $("#providerAuth").value = "bearer";
    $all("[name='apiFormat']").forEach((input) => {
      input.checked = input.value === "Anthropic";
    });
  }

  function applyPresetToForm(preset, notify = true) {
    $("#providerName").value = preset.name;
    $("#providerBaseUrl").value = preset.baseUrl;
    $("#providerAuth").value = preset.authScheme;
    selectedPreset = preset;
    $all("[name='apiFormat']").forEach((input) => {
      input.checked = input.value === preset.apiFormat;
    });
    if (notify) showToast(`${preset.name} ${t("toast.presetFilled")}`);
  }

  async function fillProviderForEdit(providerId) {
    const providers = await CCApi.getProviders();
    const provider = providers.find((item) => item.id === providerId);
    if (!provider) return;
    editingProviderId = provider.id;
    selectedPreset = { models: provider.mappings, extraHeaders: {} };
    setProviderFormMode("providersAdd.editTitle");
    $("#providerName").value = provider.name;
    $("#providerBaseUrl").value = provider.baseUrl;
    $("#providerApiKey").value = "";
    $("#providerAuth").value = provider.authScheme;
    $all("[name='apiFormat']").forEach((input) => {
      input.checked = input.value.toLowerCase() === provider.apiFormat.toLowerCase();
    });
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
    select.innerHTML = providers.map((provider) => `<option value="${provider.id}">${provider.name}</option>`).join("");
    const active = providers.find((provider) => provider.default) || providers[0];
    if (active) select.value = active.id;
    renderMappingCards();
  }

  async function renderMappingCards() {
    const providers = await CCApi.getProviders();
    const provider = providers.find((item) => item.id === $("#modelProvider").value) || providers[0];
    if (!provider) return;
    $("#mappingStack").innerHTML = modelMeta.map((model) => `
      <article class="mapping-card">
        <div class="mapping-title">
          <span class="mapping-icon ${model.key}"><i class="bi ${model.icon}"></i></span>
          <strong>${model.title}</strong>
          <span class="alias-pill">${model.title}</span>
        </div>
        <input class="form-control form-control-lg" data-model-input="${model.key}" value="${provider.mappings[model.key] || ""}">
        <span class="source-model"><i class="bi bi-arrow-left"></i>${model.source}</span>
      </article>
    `).join("");
  }

  async function renderDesktop() {
    const desktop = await CCApi.getDesktopStatus();
    const entries = Object.entries(desktop.config);
    $("#desktopConfiguredText").textContent = desktop.configured ? t("status.configured") : t("status.notConfigured");
    $("#desktopConfigList").innerHTML = entries.map(([key, value]) => `
      <div class="config-row"><i class="bi bi-check-circle-fill"></i><span>${key}:</span><code>${Array.isArray(value) ? JSON.stringify(value) : value}</code></div>
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
      <div class="log-line"><span>${line.at}</span><span class="log-level ${line.level}">${line.level.toUpperCase()}</span><span>${line.message}</span></div>
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
    if (route === "models") await renderModelSelectors();
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

      if (action === "test-provider-form") {
        const resultEl = $("#formSpeedResult");
        actionEl.disabled = true;
        resultEl.textContent = t("providers.testing");
        resultEl.classList.remove("bad");
        try {
          const result = await CCApi.testProviderPayload(providerPayloadFromForm(false));
          resultEl.textContent = result.message || `${result.latencyMs} ms`;
          resultEl.classList.toggle("bad", result.ok === false);
          showToast(result.message || t("providers.testDone"));
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
        await CCApi.saveModelMappings($("#modelProvider").value, mappings);
        showToast(t("toast.modelsSaved"));
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
    } catch (error) {
      console.error(error);
      showToast(error.message || t("toast.requestFailed"));
    }
  }

  async function fillPreset(presetId) {
    const presets = await CCApi.getPresets();
    const preset = presets.find((item) => item.id === presetId);
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

    document.addEventListener("click", (event) => {
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
      if (presetButton && presetButton.closest("#presetList")) fillPreset(presetButton.dataset.preset);
      handleAction(event.target);
    });

    $("#providerForm").addEventListener("submit", async (event) => {
      event.preventDefault();
      try {
        const payload = providerPayloadFromForm(true);
        if (editingProviderId) {
          await CCApi.updateProvider(editingProviderId, payload);
          showToast(t("toast.providerUpdated"));
        } else {
          await CCApi.addProvider(payload);
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

    $("#modelProvider").addEventListener("change", renderMappingCards);
    $("#settingsProxyPort").addEventListener("change", saveSettingsFromForm);
    $("#settingsAdminPort").addEventListener("change", saveSettingsFromForm);
    $("#settingsUpdateUrl").addEventListener("change", saveSettingsFromForm);
    $("#autoStart").addEventListener("change", saveSettingsFromForm);

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
