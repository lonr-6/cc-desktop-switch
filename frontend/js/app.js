(function () {
  const routes = ["dashboard", "providers/add", "providers", "models", "desktop", "proxy", "settings", "guide"];
  const modelMeta = [
    { key: "sonnet", title: "Sonnet", icon: "bi-stars", source: "claude-sonnet-4-6" },
    { key: "haiku", title: "Haiku", icon: "bi-leaf", source: "claude-haiku-3-5" },
    { key: "opus", title: "Opus", icon: "bi-box", source: "claude-opus-4-7" },
  ];
  let pendingDeleteId = null;
  let selectedPreset = null;
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

  async function renderDashboard() {
    const status = await CCApi.getStatus();
    const activities = await CCApi.getActivities();
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

  async function renderProviders() {
    const providers = await CCApi.getProviders();
    $("#providerRows").innerHTML = providers.map((provider) => {
      const mapping = `S:${provider.mappings.sonnet}　H:${provider.mappings.haiku}　P:${provider.mappings.opus}`;
      return `
        <div class="provider-row ${provider.default ? "default" : ""}">
          <span class="drag-handle"><i class="bi bi-grip-vertical"></i></span>
          <span class="provider-name"><span class="provider-logo">${iconMarkup(provider)}</span><span>${provider.name}</span></span>
          <span class="truncate">${provider.baseUrl}</span>
          <span class="truncate">${mapping}</span>
          <span class="status-badge ${provider.default ? "active" : ""}"><i class="bi bi-circle-fill"></i>${provider.default ? t("status.default") : t("status.standby")}</span>
          <span class="row-actions">
            <button class="btn btn-outline-primary" type="button" data-action="set-default" data-id="${provider.id}"><i class="bi bi-star"></i><span>${t("providers.setDefault")}</span></button>
            <a class="btn btn-outline-secondary" href="#providers/add" aria-label="edit ${provider.name}"><i class="bi bi-pencil"></i></a>
            <button class="btn btn-outline-danger" type="button" data-action="delete-provider" data-id="${provider.id}" aria-label="delete ${provider.name}"><i class="bi bi-trash"></i></button>
          </span>
        </div>
      `;
    }).join("");
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
    if (route === "providers/add") await renderPresets();
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
        await renderProviders();
        await renderDashboard();
        showToast(t("toast.defaultUpdated"));
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
    $("#providerName").value = preset.name;
    $("#providerBaseUrl").value = preset.baseUrl;
    $("#providerAuth").value = preset.authScheme;
    selectedPreset = preset;
    $all("[name='apiFormat']").forEach((input) => {
      input.checked = input.value === preset.apiFormat;
    });
    showToast(`${preset.name} ${t("toast.presetFilled")}`);
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
      const themeButton = event.target.closest("[data-theme-action]");
      if (themeButton) applyTheme(themeButton.dataset.themeAction);
      const presetButton = event.target.closest("[data-preset]");
      if (presetButton) fillPreset(presetButton.dataset.preset);
      handleAction(event.target);
    });

    $("#providerForm").addEventListener("submit", async (event) => {
      event.preventDefault();
      const form = new FormData(event.currentTarget);
      try {
        await CCApi.addProvider({
          name: form.get("name"),
          baseUrl: form.get("baseUrl"),
          apiKey: form.get("apiKey"),
          authScheme: form.get("authScheme"),
          apiFormat: form.get("apiFormat"),
          models: selectedPreset?.models || {},
          extraHeaders: selectedPreset?.extraHeaders || {},
        });
        showToast(t("toast.providerSaved"));
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
