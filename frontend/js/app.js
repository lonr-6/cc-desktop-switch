(function () {
  const routes = ["dashboard", "providers/add", "providers", "desktop", "proxy", "settings", "guide"];
  const restartReminderStorageKey = "ccds.restartReminder.dismissed";
  const modelMeta = [
    { key: "sonnet", title: "Sonnet", icon: "bi-stars", source: "claude-sonnet-4-6" },
    { key: "haiku", title: "Haiku", icon: "bi-leaf", source: "claude-haiku-3-5" },
    { key: "opus", title: "Opus", icon: "bi-box", source: "claude-opus-4-7" },
  ];
  const providerFormModelSlots = [
    { key: "default", label: "Default", icon: "bi-circle-fill", iconClass: "default", source: "仅作为内部后备，不显示在 Claude Desktop 菜单", required: true },
    { key: "opus_4_7", label: "Opus 4.7", icon: "bi-box", iconClass: "opus", source: "claude-opus-4-7" },
    { key: "opus_4_6", label: "Opus 4.6", icon: "bi-box", iconClass: "opus", source: "claude-opus-4-6" },
    { key: "opus_3", label: "Opus 3", icon: "bi-box", iconClass: "opus", source: "claude-3-opus" },
    { key: "sonnet_4_6", label: "Sonnet 4.6", icon: "bi-stars", iconClass: "sonnet", source: "claude-sonnet-4-6" },
    { key: "sonnet_4_5", label: "Sonnet 4.5", icon: "bi-stars", iconClass: "sonnet", source: "claude-sonnet-4-5" },
    { key: "haiku_4_5", label: "Haiku 4.5", icon: "bi-leaf", iconClass: "haiku", source: "claude-haiku-4-5" },
  ];
  const availableThemes = ["default", "green", "orange", "gray", "dark", "white"];
  const providerAuthSchemes = ["bearer", "x-api-key", "none"];
  const providerFormDefaultRows = ["default", "opus_4_7", "sonnet_4_6", "haiku_4_5"];
  let pendingDeleteId = null;
  let selectedPreset = null;
  let presetCache = [];
  let formApiFormat = "Anthropic";
  let formModelCapabilities = {};
  let formRequestOptions = {};
  let providerFormMappings = {};
  let providerFormRows = [...providerFormDefaultRows];
  let providerAvailableModels = [];
  let openProviderSlotMenuIndex = null;
  let openProviderModelMenuKey = null;
  let protocolDetected = false;
  let baseUrlMenuOpen = false;
  let authSchemeMenuOpen = false;
  let editingProviderId = null;
  let deleteModal = null;
  let restartReminderModal = null;
  let toast = null;
  let updateCheckCache = null;
  let updateInstallPhase = "idle";
  let ccSwitchCandidates = [];
  let proxyLogTimer = null;
  let proxyLogInflight = false;

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

  let _mappingAlertTimer = null;
  function showMappingCheckAlert(model, available, message) {
    const wrap = $("#mappingCheckAlertWrap");
    if (!wrap) return;
    const alertClass = available ? "alert-success" : "alert-danger";
    const iconClass = available ? "bi-check-circle-fill" : "bi-x-circle-fill";
    const title = available ? t("toast.modelAvailable") : t("toast.modelUnavailable");
    wrap.innerHTML = `
      <div class="alert ${alertClass} mapping-check-alert alert-dismissible fade show" role="alert">
        <i class="bi ${iconClass}"></i>
        <strong>${escapeHtml(model)}</strong> ${escapeHtml(title)}：${escapeHtml(message)}
        <button type="button" class="btn-close" data-bs-dismiss="alert" aria-label="Close"></button>
      </div>
    `;
    if (_mappingAlertTimer) clearTimeout(_mappingAlertTimer);
    _mappingAlertTimer = setTimeout(() => {
      wrap.innerHTML = "";
    }, 5000);
  }

  function restartReminderDismissed() {
    try {
      return localStorage.getItem(restartReminderStorageKey) === "1";
    } catch (error) {
      return false;
    }
  }

  function showRestartReminder() {
    if (restartReminderDismissed()) return;
    const checkbox = $("#restartReminderDontShow");
    if (checkbox) checkbox.checked = false;
    restartReminderModal?.show();
  }

  function dismissRestartReminder() {
    const checkbox = $("#restartReminderDontShow");
    if (checkbox?.checked) {
      try {
        localStorage.setItem(restartReminderStorageKey, "1");
      } catch (error) {
        console.warn(error);
      }
    }
    restartReminderModal?.hide();
  }

  async function ensureDesktopProxy(result = {}) {
    const next = { ...result };
    if (!next.requiresProxy || next.proxyStarted) return next;
    try {
      const proxy = await CCApi.startProxy(next.proxyPort);
      next.proxyStarted = !!proxy.running;
      next.proxyPort = proxy.port || next.proxyPort;
      if (next.proxyStarted && next.configSuccess !== false) {
        next.success = true;
      } else if (!next.proxyStarted) {
        next.success = false;
        next.message = next.message || t("toast.requestFailed");
      }
    } catch (error) {
      next.success = false;
      next.proxyStarted = false;
      next.message = error.message || next.message || t("toast.requestFailed");
    }
    return next;
  }

  function desktopApplySucceeded(result = {}) {
    if (result.success) return true;
    showToast(result.message || t("toast.requestFailed"));
    return false;
  }

  function t(key) {
    return CCI18n.t(key);
  }

  function formatI18n(key, values = {}) {
    return t(key).replace(/\{(\w+)\}/g, (_, name) => (
      Object.prototype.hasOwnProperty.call(values, name) ? values[name] : `{${name}}`
    ));
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

  function normalizePresetKey(value) {
    return String(value || "").trim().toLowerCase().replace(/\/+$/, "");
  }

  function presetExists(preset, providers) {
    const presetName = normalizePresetKey(preset.name);
    const presetUrl = normalizePresetKey(preset.baseUrl);
    return providers.some((provider) => (
      normalizePresetKey(provider.name) === presetName
      || normalizePresetKey(provider.baseUrl) === presetUrl
    ));
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

  function setFormApiFormat(format) {
    formApiFormat = ["OpenAI", "openai", "openai_chat"].includes(format) ? "OpenAI" : "Anthropic";
    const activeFormat = formApiFormat === "OpenAI" ? "openai_chat" : "anthropic";
    $all("[data-api-format]").forEach((button) => {
      const active = button.dataset.apiFormat === activeFormat;
      button.classList.toggle("active", active);
      button.setAttribute("aria-pressed", active ? "true" : "false");
    });
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

  function focusCcSwitchImportSection() {
    const section = $("#ccSwitchImportSection");
    if (!section) return;
    section.scrollIntoView({ behavior: "smooth", block: "center" });
    section.classList.add("focus-flash");
    window.setTimeout(() => section.classList.remove("focus-flash"), 1400);
  }

  function openCcSwitchImportSettings() {
    if (routeFromHash() !== "settings") {
      window.location.hash = "settings";
      window.setTimeout(focusCcSwitchImportSection, 260);
      return;
    }
    focusCcSwitchImportSection();
  }

  function emptyMappings() {
    return Object.fromEntries(providerFormModelSlots.map((slot) => [slot.key, ""]));
  }

  function normalizeMappings(mappings = {}) {
    const normalized = emptyMappings();
    if (!mappings || typeof mappings !== "object") return normalized;
    normalized.default = String(mappings.default || "").trim();
    normalized.opus_4_7 = String(mappings.opus_4_7 || mappings.opus || "").trim();
    normalized.opus_4_6 = String(mappings.opus_4_6 || "").trim();
    normalized.opus_3 = String(mappings.opus_3 || "").trim();
    normalized.sonnet_4_6 = String(mappings.sonnet_4_6 || mappings.sonnet || "").trim();
    normalized.sonnet_4_5 = String(mappings.sonnet_4_5 || "").trim();
    normalized.haiku_4_5 = String(mappings.haiku_4_5 || mappings.haiku || "").trim();
    return normalized;
  }

  function normalizeCapabilities(capabilities = {}) {
    if (!capabilities || typeof capabilities !== "object") return {};
    return Object.fromEntries(Object.entries(capabilities).filter(([, value]) => (
      value && typeof value === "object" && value.supports1m === true
    )));
  }

  function normalizeRequestOptions(options = {}) {
    if (!options || typeof options !== "object") return {};
    const source = options.anthropic && typeof options.anthropic === "object"
      ? options.anthropic
      : options;
    const normalized = {};
    const thinkingType = source.thinking?.type;
    if (["enabled", "disabled"].includes(thinkingType)) {
      normalized.thinking = { type: thinkingType };
    }
    const effort = source.output_config?.effort;
    if (["low", "medium", "high", "xhigh", "max"].includes(effort)) {
      normalized.output_config = { effort };
    }
    return Object.keys(normalized).length ? { anthropic: normalized } : {};
  }

  function mergeRequestOptions(base = {}, extra = {}) {
    const baseAnthropic = normalizeRequestOptions(base).anthropic || {};
    const extraAnthropic = normalizeRequestOptions(extra).anthropic || {};
    const merged = { ...baseAnthropic };
    if (extraAnthropic.thinking) merged.thinking = { ...extraAnthropic.thinking };
    if (extraAnthropic.output_config) merged.output_config = { ...extraAnthropic.output_config };
    return normalizeRequestOptions({ anthropic: merged });
  }

  function clearRequestOptions(base = {}, option = {}) {
    const baseAnthropic = normalizeRequestOptions(base).anthropic || {};
    const optionAnthropic = normalizeRequestOptions(option).anthropic || {};
    const current = { ...baseAnthropic };
    if (optionAnthropic.thinking) delete current.thinking;
    if (optionAnthropic.output_config) delete current.output_config;
    return normalizeRequestOptions({ anthropic: current });
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

  function formMappingMarkup() {
    return providerFormRows.map((rowKey, index) => {
      const slot = slotByKey(rowKey);
      const isRequired = rowKey === "default";
      const currentProviderModel = providerFormMappings[rowKey] || "";
      const canCheck = !!currentProviderModel && !!editingProviderId;
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
            ${canCheck
              ? `<button class="btn btn-outline-primary btn-sm mapping-check-button" type="button" data-action="check-model" data-row-key="${escapeHtml(rowKey)}" aria-label="${escapeHtml(t("providersAdd.checkModel"))}">${escapeHtml(t("providersAdd.checkModel"))}</button>`
              : '<span class="mapping-check-placeholder" aria-hidden="true"></span>'}
            ${isRequired
              ? '<span class="mapping-remove-placeholder" aria-hidden="true"></span>'
              : `<button class="btn btn-outline-danger btn-sm mapping-remove-button" type="button" data-action="remove-provider-model-row" data-row-index="${index}" aria-label="${escapeHtml(t("providersAdd.removeMapping"))}">${escapeHtml(t("providersAdd.removeMapping"))}</button>`}
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
    const canAddMoreRows = providerFormModelSlots.some((slot) => !providerFormRows.includes(slot.key));
    stack.innerHTML = `
      <div class="provider-mapping-card">
        <div class="provider-mapping-list">
          ${formMappingMarkup()}
        </div>
        <div class="provider-mapping-footer">
          <button class="btn btn-outline-primary btn-sm" type="button" data-action="add-provider-model-row" ${canAddMoreRows ? "" : "disabled"}>
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
    if (!remaining) return;
    providerFormRows = [...providerFormRows, remaining];
    openProviderSlotMenuIndex = null;
    openProviderModelMenuKey = null;
    renderProviderMappings();
  }

  function removeProviderMappingRow(index) {
    const key = providerFormRows[index];
    if (!key || key === "default") return;
    providerFormRows = providerFormRows.filter((_, rowIndex) => rowIndex !== index);
    providerFormMappings[key] = "";
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

  function renderAuthSchemeControl() {
    const input = $("#providerAuth");
    const trigger = $("#providerAuthTrigger");
    const menu = $("#providerAuthMenu");
    const wrap = $("#providerAuthControl");
    if (!input || !trigger || !menu || !wrap) return;
    const value = providerAuthSchemes.includes(input.value) ? input.value : "bearer";
    input.value = value;
    $("span", trigger).textContent = value;
    trigger.setAttribute("aria-expanded", authSchemeMenuOpen ? "true" : "false");
    wrap.classList.toggle("open", authSchemeMenuOpen);
    menu.innerHTML = providerAuthSchemes.map((item) => `
      <button class="auth-scheme-option ${item === value ? "selected" : ""}" type="button" role="option" data-action="select-auth-scheme" data-value="${escapeHtml(item)}" aria-selected="${item === value ? "true" : "false"}">
        <span>${escapeHtml(item)}</span>
        ${item === value ? '<i class="bi bi-check2"></i>' : ""}
      </button>
    `).join("");
  }

  function setAuthSchemeValue(value) {
    const input = $("#providerAuth");
    if (!input) return;
    input.value = providerAuthSchemes.includes(value) ? value : "bearer";
    authSchemeMenuOpen = false;
    renderAuthSchemeControl();
  }

  function toggleAuthSchemeMenu() {
    authSchemeMenuOpen = !authSchemeMenuOpen;
    renderAuthSchemeControl();
  }

  function closeAuthSchemeMenu() {
    if (!authSchemeMenuOpen) return;
    authSchemeMenuOpen = false;
    renderAuthSchemeControl();
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
    if (!options.length) {
      container.hidden = true;
      container.innerHTML = "";
      return;
    }
    const currentMappings = normalizeMappings(mappings || collectProviderMappings());
    container.hidden = false;
    container.innerHTML = options.map(([id, option]) => `
      <label class="preset-option-item">
        <input class="form-check-input" type="checkbox" data-preset-model-option="${escapeHtml(id)}" ${optionEnabled(option, currentMappings) ? "checked" : ""}>
        <span>
          <strong>${escapeHtml(option.label || id)}</strong>
          <small>${escapeHtml(option.description || "")}</small>
        </span>
      </label>
    `).join("");
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
    return normalizeMappings(providerFormMappings);
  }

  function providerPayloadFromForm(includeModels = true) {
    const apiKey = $("#providerApiKey").value.trim();
    const mappings = includeModels ? collectProviderMappings() : null;
    const payload = {
      name: $("#providerName").value.trim(),
      baseUrl: $("#providerBaseUrl").value.trim(),
      authScheme: $("#providerAuth").value,
      apiFormat: formApiFormat,
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
    return payload;
  }

  function providerCardMarkup(provider) {
    const mapping = [
      provider.mappings.default,
      provider.mappings.sonnet_4_6,
      provider.mappings.haiku_4_5,
      provider.mappings.opus_4_7,
    ]
      .filter(Boolean)
      .slice(0, 2)
      .join(" / ");
    const providerId = escapeHtml(provider.id);
    const providerName = escapeHtml(provider.name);
    const providerUrl = escapeHtml(provider.baseUrl);
    const providerHref = escapeHtml(safeHttpUrl(provider.baseUrl));
    const mappingText = escapeHtml(mapping || provider.apiFormat);
    return `
      <article class="provider-switch-card ${provider.default ? "active" : ""}" draggable="true" data-provider-id="${providerId}">
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

  function providerPresetCardMarkup(preset, added = false) {
    const presetId = escapeHtml(preset.id);
    return `
      <button class="provider-switch-card preset-card ${added ? "added" : ""}" type="button" data-action="new-from-preset" data-preset="${presetId}" ${added ? "disabled" : ""}>
        <span class="drag-handle preset-plus"><i class="bi ${added ? "bi-check2" : "bi-plus-lg"}"></i></span>
        <span class="provider-logo">${iconMarkup(preset)}</span>
        <span class="provider-main"><strong>${escapeHtml(preset.name)}</strong><span class="truncate">${escapeHtml(preset.baseUrl)}</span></span>
        <span class="provider-meta">${escapeHtml(preset.apiFormat)}</span>
        <span class="provider-actions"><span class="compact-enable ghost"><i class="bi ${added ? "bi-check2" : "bi-plus-lg"}"></i><span>${added ? t("providers.added") : t("providers.add")}</span></span></span>
      </button>
    `;
  }

  function dashboardPresetSectionMarkup(providers, presets) {
    const available = presets.filter((preset) => !presetExists(preset, providers));
    if (!available.length) return "";
    return `
      <section class="dashboard-preset-section" aria-label="${escapeHtml(t("dashboard.availablePresets"))}">
        <div class="section-title-row compact">
          <div>
            <h2>${escapeHtml(t("dashboard.availablePresets"))}</h2>
            <p>${escapeHtml(t("dashboard.availablePresetsHint"))}</p>
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
    const providerList = providers.length
      ? `<div class="provider-configured-list" data-provider-list>${providers.map(providerCardMarkup).join("")}</div>`
      : "";
    if (!providers.length) {
      const presets = await CCApi.getPresets();
      target.innerHTML = `<div class="provider-preset-grid">${presets.map((preset) => providerPresetCardMarkup(preset)).join("")}</div>`;
      return;
    }
    if (options.includePresets) {
      const presets = await CCApi.getPresets();
      target.innerHTML = `${providerList}${dashboardPresetSectionMarkup(providers, presets)}`;
    } else {
      target.innerHTML = providerList;
    }
    enableProviderReorder($("[data-provider-list]", target));
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
    $("#dashboardProxyStatus").textContent = status.proxyRunning ? `${t("status.running")} :${status.proxyPort}` : t("status.stopped");
    $("#dashboardProviderName").textContent = status.activeProvider.name;
    $("#activityList").innerHTML = activities.map((item) => (
      `<div class="activity-row"><time>${escapeHtml(item.time)}</time><span>${escapeHtml(item.text)}</span></div>`
    )).join("");
    await refreshUpdateBadge();
  }

  async function renderPresets() {
    presetCache = await CCApi.getPresets();
    $("#presetList").innerHTML = presetCache.map((preset) => {
      const active = selectedPreset?.id === preset.id;
      return `
      <button class="preset-item ${active ? "active" : ""}" type="button" data-preset="${escapeHtml(preset.id)}" aria-pressed="${active ? "true" : "false"}">
        <span class="preset-logo">${iconMarkup(preset)}</span>
        <span><strong>${escapeHtml(preset.name)}</strong><span>${escapeHtml(preset.baseUrl)}</span></span>
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

  function updateDetectFormatButton() {
    const btn = $("#detectFormatBtn");
    const status = $("#detectFormatStatus");
    if (!btn) return;
    const baseUrl = $("#providerBaseUrl")?.value.trim();
    const apiKey = $("#providerApiKey")?.value.trim();
    const canDetect = !!(baseUrl && apiKey);
    btn.disabled = !canDetect;
    if (status && !status.textContent) {
      status.className = "detect-format-status";
    }
  }

  function updateApplyButtonState() {
    const applyBtn = $("[data-action='apply-provider-desktop']");
    if (!applyBtn) return;
    const isThirdParty = selectedPreset?.id === "third-party";
    const canApply = !isThirdParty || protocolDetected;
    applyBtn.disabled = !canApply;
  }

  async function handleDetectFormat() {
    const btn = $("#detectFormatBtn");
    const status = $("#detectFormatStatus");
    if (!btn || !status) return;

    const baseUrl = $("#providerBaseUrl")?.value.trim();
    const apiKey = $("#providerApiKey")?.value.trim();
    if (!baseUrl || !apiKey) return;

    btn.disabled = true;
    status.textContent = "探测中...";
    status.className = "detect-format-status";

    try {
      const result = await CCApi.detectApiFormat(baseUrl, apiKey);
      if (result.success) {
        const fmt = result.apiFormat === "openai_responses" ? "openai_chat" : result.apiFormat;
        setFormApiFormat(fmt);
        const fmtLabel = fmt === "anthropic" ? "Anthropic 兼容" : fmt === "openai_chat" ? "OpenAI Chat" : "OpenAI Responses";
        status.textContent = `已识别: ${fmtLabel}`;
        status.className = "detect-format-status success";
        protocolDetected = true;
        updateApplyButtonState();
      } else {
        status.textContent = result.message || "探测失败";
        status.className = "detect-format-status error";
      }
    } catch (error) {
      status.textContent = error.message || "探测失败";
      status.className = "detect-format-status error";
    } finally {
      updateDetectFormatButton();
    }
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
    protocolDetected = false;
    setProviderFormMode("providersAdd.title");
    $("#providerName").value = "";
    $("#providerBaseUrl").value = "";
    renderBaseUrlOptions(null);
    setApiKeyInputState(false);
    $("#providerAuth").value = "bearer";
    renderAuthSchemeControl();
    setFormApiFormat("anthropic");
    setProviderMappings(emptyMappings());
    updateDetectFormatButton();
    updateApplyButtonState();
  }

  function applyPresetToForm(preset, notify = true) {
    $("#providerName").value = preset.name;
    $("#providerBaseUrl").value = preset.baseUrl;
    baseUrlMenuOpen = false;
    renderBaseUrlOptions(preset);
    setAuthSchemeValue(preset.authScheme);
    setApiKeyInputState(false);
    selectedPreset = preset;
    setFormApiFormat(preset.apiFormat === "OpenAI" ? "openai_chat" : "anthropic");
    formModelCapabilities = normalizeCapabilities(preset.modelCapabilities || {});
    formRequestOptions = normalizeRequestOptions(preset.requestOptions || {});
    providerAvailableModels = [];
    setProviderMappings(preset.models || emptyMappings());
    renderPresetOptions(preset, preset.models || emptyMappings());
    updatePresetSelection();
    // 内置预设已知协议，第三方需要探测
    protocolDetected = preset.id !== "third-party";
    updateDetectFormatButton();
    updateApplyButtonState();
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
    $("#providerBaseUrl").value = provider.baseUrl;
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
    setFormApiFormat(["openai", "openai_chat"].includes(provider.apiFormat) ? "openai_chat" : "anthropic");
    providerAvailableModels = [];
    setProviderMappings(provider.mappings || emptyMappings());
    renderPresetOptions(selectedPreset, provider.mappings || emptyMappings());
    updatePresetSelection();
    protocolDetected = true;
    updateDetectFormatButton();
    updateApplyButtonState();
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
      const defaultValue = provider.mappings.default || provider.mappings.sonnet_4_6 || "";
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
    $("#desktopJson").textContent = JSON.stringify(desktop.config, null, 2);
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
      logEl.innerHTML = logs.map((line) => `
        <div class="log-line"><span>${escapeHtml(line.at)}</span><span class="log-level ${escapeHtml(line.level)}">${escapeHtml(line.level.toUpperCase())}</span><span>${escapeHtml(line.message)}</span></div>
      `).join("");
      if ($("#autoScroll")?.checked) logEl.scrollTop = logEl.scrollHeight;
      const stats = [
        { label: t("proxy.stats.total"), value: proxyStatus.stats.total, icon: "bi-list-ul" },
        { label: t("proxy.stats.success"), value: proxyStatus.stats.success, icon: "bi-check-circle" },
        { label: t("proxy.stats.failed"), value: proxyStatus.stats.failed, icon: "bi-x-circle", danger: true },
        { label: t("proxy.stats.today"), value: proxyStatus.stats.today, icon: "bi-calendar3" },
      ];
      $("#proxyStats").innerHTML = stats.map((stat) => `
        <article class="stat-card ${stat.danger ? "danger" : ""}"><i class="bi ${stat.icon}"></i><div><span>${stat.label}</span><strong>${stat.value}</strong></div></article>
      `).join("");
    } catch (error) {
      console.warn(error);
    } finally {
      proxyLogInflight = false;
    }
  }

  function stopProxyLogAutoRefresh() {
    if (proxyLogTimer !== null) {
      clearInterval(proxyLogTimer);
      proxyLogTimer = null;
    }
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
    await refreshProxyLog();
    if (routeFromHash() === "proxy") {
      startProxyLogAutoRefresh();
    } else {
      stopProxyLogAutoRefresh();
    }
  }

  async function renderSettings() {
    const settings = await CCApi.getSettings();
    applyTheme(settings.theme || "default");
    $("#settingsProxyPort").value = settings.proxyPort;
    $("#settingsAdminPort").value = settings.adminPort;
    $("#autoStart").checked = settings.autoStart;
    $("#exposeAllProviderModels").checked = !!settings.exposeAllProviderModels;
    $("#settingsUpdateUrl").value = settings.updateUrl || "";
    $("#settingsUpstreamProxy").value = settings.upstreamProxy || "";
    $("#settingsUpstreamProxyEnabled").checked = settings.upstreamProxyEnabled !== false;
    renderModelMenuModeState(settings);
    await refreshBackupList();
    await refreshCcSwitchImportStatus();
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
    if (route === "proxy") await renderProxy();
    if (route === "settings") await renderSettings();
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
      autoStart: $("#autoStart").checked,
      exposeAllProviderModels: $("#exposeAllProviderModels")?.checked || false,
      updateUrl: $("#settingsUpdateUrl").value.trim(),
      upstreamProxy: $("#settingsUpstreamProxy").value.trim(),
      upstreamProxyEnabled: $("#settingsUpstreamProxyEnabled").checked,
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
    if (!window.confirm(t("confirm.ccswitchImport"))) return;
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

  function ccSwitchStatusMessage(providers = ccSwitchCandidates) {
    const supported = providers.filter((item) => item.supported).length;
    const unsupported = providers.length - supported;
    if (!providers.length) return t("settings.ccswitchNotFound");
    return formatI18n("settings.ccswitchFound", { supported, unsupported });
  }

  function translateCcSwitchReason(reason = "") {
    const normalized = String(reason || "").trim();
    if (!normalized) return "";
    if (normalized === "没有发现 API 地址，可能是官方登录或空配置。") {
      return t("settings.ccswitchReason.noBaseUrl");
    }
    if (normalized === "这是 CC-Switch 本机代理地址，不能作为上游 API 导入。") {
      return t("settings.ccswitchReason.localProxy");
    }
    if (normalized === "没有发现 API Key。") {
      return t("settings.ccswitchReason.noApiKey");
    }
    if (normalized === "OpenAI Chat 格式本轮不自动导入，避免转换兼容风险。") {
      return t("settings.ccswitchReason.openaiChat");
    }
    if (normalized === "OpenAI Responses 格式暂未适配，暂不自动导入。") {
      return t("settings.ccswitchReason.openaiResponses");
    }
    const unsupportedMatch = normalized.match(/^(.+?) 格式暂不支持自动导入。$/);
    if (unsupportedMatch) {
      return formatI18n("settings.ccswitchReason.unsupportedFormat", { format: unsupportedMatch[1] });
    }
    return normalized;
  }

  function renderCcSwitchImportList(providers = ccSwitchCandidates, message = "") {
    const target = $("#ccSwitchImportList");
    if (!target) return;
    if (!providers.length) {
      target.innerHTML = `<p class="ccswitch-import-empty">${escapeHtml(message || t("settings.ccswitchNotFound"))}</p>`;
      return;
    }
    target.innerHTML = `
      <p class="ccswitch-import-summary">${escapeHtml(message || ccSwitchStatusMessage(providers))}</p>
      ${providers.map((provider) => {
        const statusLabel = provider.supported ? t("settings.ccswitchSupported") : t("settings.ccswitchUnsupported");
        const statusIcon = provider.supported ? "bi-check2-circle" : "bi-slash-circle";
        const model = provider.models?.default || provider.models?.sonnet_4_6 || provider.models?.sonnet || "";
        const translatedReason = translateCcSwitchReason(provider.reason);
        return `
          <article class="ccswitch-import-item ${provider.supported ? "supported" : "unsupported"}">
            <div>
              <strong>${escapeHtml(provider.name)}</strong>
              <span class="truncate">${escapeHtml(provider.baseUrl || translatedReason || provider.apiFormat)}</span>
              ${model ? `<small>${escapeHtml(model)}</small>` : ""}
            </div>
            <span class="ccswitch-import-secret">${escapeHtml(provider.hasApiKey ? provider.apiKeyPreview : "")}</span>
            <span class="ccswitch-import-status"><i class="bi ${statusIcon}"></i>${escapeHtml(statusLabel)}</span>
            ${translatedReason ? `<p>${escapeHtml(translatedReason)}</p>` : ""}
          </article>
        `;
      }).join("")}
    `;
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

  function renderDiagnosticsResult(result) {
    const target = $("#diagnosticsResult");
    if (!target) return;
    const checks = result?.checks || [];
    if (!checks.length) {
      target.innerHTML = `<p class="compatibility-empty">${escapeHtml(t("diagnostics.notRun"))}</p>`;
      return;
    }
    target.innerHTML = checks.map((check) => `
      <article class="compatibility-item ${check.ok ? "stable" : "experimental"}">
        <div>
          <strong>${escapeHtml(check.code)}</strong>
          <span>${escapeHtml(check.message || "")}</span>
        </div>
        <em>${escapeHtml(check.ok ? t("diagnostics.ok") : t("diagnostics.warn"))}</em>
      </article>
    `).join("");
  }

  function diagnosticsFilename() {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, "-");
    return `cc-desktop-switch-diagnostics-${stamp}.json`;
  }

  async function refreshCcSwitchImportStatus() {
    const target = $("#ccSwitchImportList");
    if (!target) return;
    try {
      const status = await CCApi.getCcSwitchStatus();
      if (!status.found) {
        ccSwitchCandidates = [];
        renderCcSwitchImportList([], t("settings.ccswitchNotFound"));
        return;
      }
      const result = await CCApi.getCcSwitchProviders();
      ccSwitchCandidates = result.providers || [];
      renderCcSwitchImportList(ccSwitchCandidates, formatI18n("settings.ccswitchFound", {
        supported: result.supportedCount || 0,
        unsupported: result.unsupportedCount || 0,
      }));
    } catch (error) {
      console.error(error);
      ccSwitchCandidates = [];
      renderCcSwitchImportList([], error.message || t("settings.ccswitchNotFound"));
    }
  }

  async function importCcSwitchProviders(actionEl) {
    if (!ccSwitchCandidates.length) {
      await refreshCcSwitchImportStatus();
    }
    const ids = ccSwitchCandidates.filter((item) => item.supported).map((item) => item.id);
    if (!ids.length) {
      showToast(t("settings.ccswitchNoSupported"));
      return;
    }
    if (!window.confirm(t("confirm.configImport"))) return;
    actionEl.disabled = true;
    try {
      const result = await CCApi.importCcSwitchProviders(ids, false);
      await refreshBackupList();
      await refreshCcSwitchImportStatus();
      if (routeFromHash() === "dashboard") await renderDashboard();
      if (routeFromHash() === "providers") await renderProviders();
      showToast(formatI18n("settings.ccswitchImported", { count: result.imported?.length || 0 }));
    } finally {
      actionEl.disabled = false;
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
      const desktopResult = await ensureDesktopProxy(await CCApi.configureDesktop());
      if (!desktopApplySucceeded(desktopResult)) return;
      editingProviderId = null;
      selectedPreset = null;
      window.location.hash = "dashboard";
      showToast(t("toast.providerAppliedDesktop"));
      showRestartReminder();
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
          result.desktopSync = await ensureDesktopProxy(result.desktopSync);
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
          providerAvailableModels = Array.isArray(result.models) ? result.models.slice() : [];
          setProviderMappings(result.suggested || emptyMappings(), { availableModels: providerAvailableModels });
          if (resultEl) resultEl.textContent = t("models.fetchSuccess");
          showToast(t("toast.modelsAutofilled"));
        } catch (error) {
          providerAvailableModels = [];
          renderProviderMappings();
          if (resultEl) resultEl.textContent = t("models.fetchFailedManual");
          showToast(error.message || t("toast.requestFailed"));
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

      if (action === "check-model") {
        const rowKey = actionEl.dataset.rowKey;
        const model = providerFormMappings[rowKey] || "";
        if (!model || !editingProviderId) return;
        actionEl.disabled = true;
        actionEl.innerHTML = `<span class="spinner-border spinner-border-sm" role="status" aria-hidden="true"></span>`;
        try {
          const result = await CCApi.checkModelAvailability(editingProviderId, model);
          showMappingCheckAlert(model, result.available, result.message);
        } catch (error) {
          showMappingCheckAlert(model, false, error.message || t("toast.requestFailed"));
        } finally {
          actionEl.disabled = false;
          actionEl.innerHTML = escapeHtml(t("providersAdd.checkModel"));
        }
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

      if (action === "toggle-auth-scheme-menu") {
        toggleAuthSchemeMenu();
      }

      if (action === "select-auth-scheme") {
        setAuthSchemeValue(actionEl.dataset.value);
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
        mappings.default = mappings[defaultKey]
          || mappings.sonnet_4_6
          || mappings.sonnet_4_5
          || mappings.haiku_4_5
          || mappings.opus_4_7
          || mappings.opus_4_6
          || mappings.opus_3
          || mappings.sonnet
          || mappings.haiku
          || mappings.opus
          || "";
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
        const desktopResult = await ensureDesktopProxy(await CCApi.configureDesktop());
        await renderDesktop();
        if (!desktopApplySucceeded(desktopResult)) return;
        showToast(t("toast.desktopApplied"));
      }

      if (action === "clear-desktop") {
        if (!window.confirm(t("confirm.desktopClear"))) return;
        await CCApi.clearDesktop();
        const route = routeFromHash();
        if (route === "dashboard") {
          await renderDashboard();
        } else if (route === "desktop") {
          await renderDesktop();
        }
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

      if (action === "open-ccswitch-import") {
        openCcSwitchImportSettings();
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

      if (action === "run-diagnostics") {
        actionEl.disabled = true;
        try {
          const result = await CCApi.checkDiagnostics();
          renderDiagnosticsResult(result);
          showToast(result.ok ? t("diagnostics.allGood") : t("diagnostics.hasWarnings"));
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "export-diagnostics") {
        actionEl.disabled = true;
        try {
          const result = await CCApi.exportDiagnostics();
          downloadJson(diagnosticsFilename(), result.diagnostics || result);
          showToast(t("diagnostics.exported"));
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "copy-diagnostics") {
        const result = await CCApi.exportDiagnostics();
        await navigator.clipboard.writeText(JSON.stringify(result.diagnostics || result, null, 2));
        showToast(t("diagnostics.copied"));
      }

      if (action === "detect-proxy") {
        actionEl.disabled = true;
        try {
          const detected = await CCApi.detectLocalProxy();
          if (detected) {
            $("#settingsUpstreamProxy").value = detected;
            await saveSettingsFromForm();
            showToast(t("toast.proxyDetected"));
          } else {
            showToast(t("toast.proxyNotDetected"));
          }
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
        let progressPoll = null;
        setUpdateInstallPhase("downloading");
        if (status) {
          status.textContent = t("toast.updateDownloading");
          status.classList.add("available");
          status.style.setProperty("--progress", "0%");
        }
        const startProgressPoll = () => {
          progressPoll = setInterval(async () => {
            try {
              const progress = await CCApi.getUpdateProgress();
              if (progress.active && status) {
                status.style.setProperty("--progress", `${progress.percent}%`);
                if (progress.message) {
                  status.textContent = progress.message;
                }
              }
            } catch (e) {}
          }, 300);
        };
        const stopProgressPoll = () => {
          if (progressPoll) {
            clearInterval(progressPoll);
            progressPoll = null;
          }
          if (status) {
            status.style.setProperty("--progress", "0%");
          }
        };
        startProgressPoll();
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
          if (status) {
            status.textContent = "下载失败，请重试";
            status.classList.remove("available");
          }
          throw error;
        } finally {
          stopProgressPoll();
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
        downloadJson(`cc-desktop-switch-config-${stamp}.json`, data);
        showToast(t("toast.configExported"));
      }

      if (action === "choose-import-config") {
        $("#configImportFile").click();
      }

      if (action === "detect-ccswitch") {
        actionEl.disabled = true;
        try {
          await refreshCcSwitchImportStatus();
          showToast(ccSwitchStatusMessage());
        } finally {
          actionEl.disabled = false;
        }
      }

      if (action === "import-ccswitch") {
        await importCcSwitchProviders(actionEl);
      }

      if (action === "apply-provider-desktop") {
        await applyProviderToDesktop(actionEl);
      }

      if (action === "detect-format") {
        await handleDetectFormat();
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
      if (!event.target.closest(".auth-scheme-menu-wrap")) {
        closeAuthSchemeMenu();
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
      const formatButton = event.target.closest("[data-api-format]");
      if (formatButton) {
        event.preventDefault();
        setFormApiFormat(formatButton.dataset.apiFormat);
        protocolDetected = true;
        updateApplyButtonState();
        showToast(formatButton.dataset.apiFormat === "openai_chat" ? t("toast.openaiFormatExperimental") : t("toast.anthropicFormatSelected"));
        return;
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
    });

    document.addEventListener("input", (event) => {
      if (event.target.id === "providerBaseUrl") {
        renderBaseUrlOptions();
        protocolDetected = false;
        updateDetectFormatButton();
        updateApplyButtonState();
      }
      if (event.target.id === "providerApiKey") {
        updateDetectFormatButton();
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
        closeAuthSchemeMenu();
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
    $("#settingsUpstreamProxy").addEventListener("change", saveSettingsFromForm);
    $("#autoStart").addEventListener("change", saveSettingsFromForm);
    $("#exposeAllProviderModels").addEventListener("change", saveSettingsFromForm);
    $("#configImportFile")?.addEventListener("change", (event) => {
      importConfigFile(event.target.files?.[0]);
    });
    $("#restartReminderAck")?.addEventListener("click", dismissRestartReminder);

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
    deleteModal = new bootstrap.Modal($("#deleteModal"));
    restartReminderModal = new bootstrap.Modal($("#restartReminderModal"), {
      backdrop: "static",
      keyboard: false,
    });
    toast = new bootstrap.Toast($("#appToast"), { delay: 2200 });
    bindEvents();
    const settings = await CCApi.getSettings();
    CCI18n.apply(settings.language || "zh");
    applyTheme(settings.theme || "default");
    if (!window.location.hash) window.location.hash = "dashboard";
    await renderRoute(routeFromHash());
  });
})();
