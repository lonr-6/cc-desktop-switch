use leptos::html;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::commands::{
    self, ApiFormat, AuthScheme, ConfigSettings, ModelMappingDraft, ModelSlot, ProviderDraft,
    ProviderPreset, ProviderSummary,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppPage {
    Dashboard,
    ProvidersAdd,
    Providers,
    Desktop,
    Proxy,
    Settings,
    Guide,
}

#[derive(Clone, Debug)]
struct ProxyLogRow {
    key: String,
    timestamp: String,
    level: String,
    code: String,
    message: String,
}

#[component]
pub fn App() -> impl IntoView {
    let (active_page, set_active_page) = signal(AppPage::Dashboard);
    let (language, set_language) = signal("zh".to_owned());
    let (theme, set_theme) = signal("light".to_owned());
    let (providers, set_providers) = signal(Vec::<ProviderSummary>::new());
    let (selected_provider_id, set_selected_provider_id) = signal(None::<String>);
    let (editing_provider_id, set_editing_provider_id) = signal(None::<String>);
    let (active_provider_id, set_active_provider_id) = signal(None::<String>);
    let (provider_name, set_provider_name) = signal(String::new());
    let (base_url, set_base_url) = signal(String::new());
    let (api_key, set_api_key) = signal(String::new());
    let (show_api_key, set_show_api_key) = signal(false);
    let (api_format, set_api_format) = signal("anthropic".to_owned());
    let (auth_scheme, set_auth_scheme) = signal("bearer".to_owned());
    let (import_json, set_import_json) = signal(String::new());
    let (import_preview_text, set_import_preview_text) =
        signal("Provider import preview has not run.".to_owned());
    let (provider_presets, set_provider_presets) = signal(Vec::<ProviderPreset>::new());
    let (selected_preset_id, set_selected_preset_id) = signal(String::new());
    let (preset_api_key, set_preset_api_key) = signal(String::new());
    let (model_mapping_json, set_model_mapping_json) = signal(default_model_mapping_json());
    let (mapping_default, set_mapping_default) = signal("deepseek-v4-pro".to_owned());
    let (mapping_opus, set_mapping_opus) = signal("deepseek-v4-pro".to_owned());
    let (mapping_sonnet, set_mapping_sonnet) = signal("deepseek-v4-pro".to_owned());
    let (mapping_haiku, set_mapping_haiku) = signal("deepseek-v4-flash".to_owned());
    let (backup_file_name, set_backup_file_name) = signal(String::new());
    let (proxy_port, set_proxy_port) = signal("18080".to_owned());
    let (update_url, set_update_url) = signal(String::new());
    let (proxy_logs_text, set_proxy_logs_text) = signal("尚未读取日志。".to_owned());
    let (proxy_logs, set_proxy_logs) = signal(Vec::<commands::DiagnosticsLogEntry>::new());
    let (proxy_status, set_proxy_status) = signal(None::<commands::ProxyStatus>);
    let (update_download_path, set_update_download_path) = signal(None::<String>);
    let (auto_scroll_logs, set_auto_scroll_logs) = signal(true);
    let proxy_log_body_ref = NodeRef::<html::Div>::new();
    let (result, set_result) = signal("尚未执行 command。".to_owned());
    let (diagnostics_text, set_diagnostics_text) = signal("尚未生成 diagnostics。".to_owned());
    let (provider_saved, set_provider_saved) = signal(false);
    let (_gateway_status_text, set_gateway_status_text) = signal(String::new());
    let (readiness_snapshot, set_readiness_snapshot) = signal(None::<commands::ReadinessSnapshot>);
    let (restart_required, set_restart_required) = signal(false);

    let copy = move |key: &'static str| text(&language.get(), key);

    Effect::new(move |_| {
        let _ = proxy_logs.get().len();
        if auto_scroll_logs.get() {
            if let Some(node) = proxy_log_body_ref.get() {
                node.set_scroll_top(node.scroll_height());
            }
        }
    });

    let refresh_providers = move |_| {
        set_result.set("刷新 Provider 列表中...".to_owned());
        spawn_local(async move {
            match commands::get_config_snapshot().await {
                Ok(snapshot) => {
                    set_active_provider_id.set(snapshot.active_provider.clone());
                    let next_providers = snapshot.providers;
                    if selected_provider_id.get_untracked().is_none() {
                        if let Some(provider) = next_providers.first() {
                            set_selected_provider_id.set(Some(provider.provider_id.clone()));
                        }
                    }
                    set_result.set(format_provider_list(&next_providers));
                    set_providers.set(next_providers);
                }
                Err(error) => set_result.set(format!("get_config_snapshot failed: {error}")),
            }
        });
    };

    let save_provider = move |_| {
        let request = ProviderDraft {
            provider_id: editing_provider_id.get_untracked(),
            display_name: provider_name.get_untracked(),
            base_url: base_url.get_untracked(),
            auth_scheme: auth_scheme_from_value(&auth_scheme.get_untracked()),
            api_key: api_key.get_untracked(),
            api_format: if api_format.get_untracked() == "openai_chat" {
                ApiFormat::OpenAiChat
            } else {
                ApiFormat::Anthropic
            },
        };
        set_result.set("保存 Provider 中...".to_owned());
        spawn_local(async move {
            match commands::save_provider(request).await {
                Ok(summary) => {
                    set_provider_saved.set(true);
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    set_selected_provider_id.set(Some(summary.provider_id.clone()));
                    set_editing_provider_id.set(Some(summary.provider_id.clone()));
                    set_api_key.set(String::new());
                    match commands::get_config_snapshot().await {
                        Ok(snapshot) => {
                            set_active_provider_id.set(snapshot.active_provider.clone());
                            let next_providers = snapshot.providers;
                            set_providers.set(next_providers.clone());
                            set_result.set(format!(
                                "save_provider ok\n{}\n{}",
                                summary.provider_id,
                                format_provider_list(&next_providers)
                            ));
                        }
                        Err(error) => set_result.set(format!(
                            "save_provider ok\nget_config_snapshot failed: {error}"
                        )),
                    }
                }
                Err(error) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_provider_state_from_backend(
                        set_active_provider_id,
                        set_selected_provider_id,
                        set_providers,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "save_provider",
                        &error,
                        refresh_note,
                    ));
                }
            }
        });
    };

    let export_providers = move |_| {
        set_result.set("生成 Provider export 预览中...".to_owned());
        spawn_local(async move {
            match commands::export_providers().await {
                Ok(package) => match serde_json::to_string_pretty(&package) {
                    Ok(raw) => {
                        set_import_json.set(raw.clone());
                        set_result.set(format!(
                            "Provider export preview (API keys redacted). Use Save as to write a full export file.\n{raw}"
                        ));
                    }
                    Err(error) => set_result.set(format!("format export failed: {error}")),
                },
                Err(error) => set_result.set(format!("export_providers failed: {error}")),
            }
        });
    };

    let save_provider_export_as = move |_| {
        set_result.set("选择 Provider export 保存位置中...".to_owned());
        spawn_local(async move {
            match commands::save_provider_export_as().await {
                Ok(Some(path)) => set_result.set(format!("provider export saved:\n{path}")),
                Ok(None) => set_result.set("save_provider_export_as canceled".to_owned()),
                Err(error) => set_result.set(format!("save_provider_export_as failed: {error}")),
            }
        });
    };

    let load_provider_template_example = move |_| {
        let raw = default_provider_template_json();
        set_import_json.set(raw.clone());
        set_import_preview_text
            .set("Template example loaded. Preview before importing.".to_owned());
        set_result.set(raw);
    };

    let preview_import = move |replace_existing: bool, skip_existing: bool| {
        let raw = import_json.get_untracked();
        set_result.set("预览 Provider import 中...".to_owned());
        spawn_local(async move {
            match commands::preview_provider_import(raw, replace_existing, skip_existing).await {
                Ok(preview) => match serde_json::to_string_pretty(&preview) {
                    Ok(raw) => {
                        let summary = format_provider_import_value(&preview);
                        set_import_preview_text.set(summary.clone());
                        set_result.set(format!("{summary}\n\n{raw}"));
                    }
                    Err(error) => set_result.set(format!("format preview failed: {error}")),
                },
                Err(error) => set_result.set(format!("preview_provider_import failed: {error}")),
            }
        });
    };

    let apply_import = move |replace_existing: bool, skip_existing: bool| {
        let raw = import_json.get_untracked();
        set_result.set("导入 Provider 中...".to_owned());
        spawn_local(async move {
            match commands::import_providers(raw, replace_existing, skip_existing).await {
                Ok(import_result) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    if let Ok(snapshot) = commands::get_config_snapshot().await {
                        set_active_provider_id.set(snapshot.active_provider.clone());
                        if selected_provider_id.get_untracked().is_none() {
                            set_selected_provider_id.set(snapshot.active_provider.clone().or_else(
                                || {
                                    snapshot
                                        .providers
                                        .first()
                                        .map(|provider| provider.provider_id.clone())
                                },
                            ));
                        }
                        set_providers.set(snapshot.providers);
                    }
                    match serde_json::to_string_pretty(&import_result) {
                        Ok(raw) => {
                            let summary = format_provider_import_result(&import_result);
                            set_import_preview_text.set(summary.clone());
                            set_result.set(format!("{summary}\n\n{raw}"));
                        }
                        Err(error) => set_result.set(format!("format import failed: {error}")),
                    }
                }
                Err(error) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_provider_state_from_backend(
                        set_active_provider_id,
                        set_selected_provider_id,
                        set_providers,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "import_providers",
                        &error,
                        refresh_note,
                    ));
                }
            }
        });
    };

    let load_provider_presets = move |_| {
        set_result.set("读取 Provider presets 中...".to_owned());
        spawn_local(async move {
            match commands::list_provider_presets().await {
                Ok(presets) => {
                    if let Some(first) = presets.first() {
                        set_selected_preset_id.set(first.preset_id.clone());
                    }
                    set_result.set(format_provider_presets(&presets));
                    set_provider_presets.set(presets);
                }
                Err(error) => set_result.set(format!("list_provider_presets failed: {error}")),
            }
        });
    };

    let preview_preset_import = move |_| {
        let preset_id = selected_preset_id.get_untracked();
        set_result.set(format!("预览 Provider preset 中: {preset_id}"));
        spawn_local(async move {
            match commands::preview_provider_preset_import(preset_id, false).await {
                Ok(preview) => match serde_json::to_string_pretty(&preview) {
                    Ok(raw) => set_result.set(raw),
                    Err(error) => set_result.set(format!("format preset preview failed: {error}")),
                },
                Err(error) => {
                    set_result.set(format!("preview_provider_preset_import failed: {error}"))
                }
            }
        });
    };

    let import_preset = move |replace_existing: bool| {
        let preset_id = selected_preset_id.get_untracked();
        let api_key = preset_api_key.get_untracked();
        set_result.set(format!("导入 Provider preset 中: {preset_id}"));
        spawn_local(async move {
            match commands::import_provider_preset(preset_id, api_key, replace_existing).await {
                Ok(import_result) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    if let Ok(snapshot) = commands::get_config_snapshot().await {
                        set_active_provider_id.set(snapshot.active_provider.clone());
                        set_editing_provider_id.set(None);
                        set_selected_provider_id.set(snapshot.active_provider.clone().or_else(
                            || {
                                snapshot
                                    .providers
                                    .first()
                                    .map(|provider| provider.provider_id.clone())
                            },
                        ));
                        set_providers.set(snapshot.providers);
                    }
                    set_preset_api_key.set(String::new());
                    match serde_json::to_string_pretty(&import_result) {
                        Ok(raw) => set_result.set(raw),
                        Err(error) => {
                            set_result.set(format!("format preset import failed: {error}"))
                        }
                    }
                }
                Err(error) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_provider_state_from_backend(
                        set_active_provider_id,
                        set_selected_provider_id,
                        set_providers,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "import_provider_preset",
                        &error,
                        refresh_note,
                    ));
                }
            }
        });
    };

    let load_model_mappings = move |_| {
        let Some(provider_id) = model_mapping_target_provider_id(
            editing_provider_id.get_untracked(),
            selected_provider_id.get_untracked(),
        ) else {
            set_result.set("list_model_mappings skipped: no provider selected".to_owned());
            return;
        };
        set_result.set(format!("读取模型映射中: {provider_id}"));
        spawn_local(async move {
            load_model_mappings_for_provider(
                provider_id,
                set_mapping_default,
                set_mapping_opus,
                set_mapping_sonnet,
                set_mapping_haiku,
                set_model_mapping_json,
                set_result,
            )
            .await;
        });
    };

    let load_editing_model_mappings = move |_| {
        let Some(provider_id) = editing_provider_id.get_untracked() else {
            set_result.set(
                "list_model_mappings skipped: save or edit an existing provider first".to_owned(),
            );
            return;
        };
        set_result.set(format!("读取模型映射中: {provider_id}"));
        spawn_local(async move {
            load_model_mappings_for_provider(
                provider_id,
                set_mapping_default,
                set_mapping_opus,
                set_mapping_sonnet,
                set_mapping_haiku,
                set_model_mapping_json,
                set_result,
            )
            .await;
        });
    };

    let save_model_mappings = move |_| {
        let Some(provider_id) = model_mapping_target_provider_id(
            editing_provider_id.get_untracked(),
            selected_provider_id.get_untracked(),
        ) else {
            set_result.set("update_model_mappings skipped: no provider selected".to_owned());
            return;
        };
        let provider_id_for_refresh = provider_id.clone();
        let mappings = visible_model_mapping_drafts(
            mapping_default.get_untracked(),
            mapping_opus.get_untracked(),
            mapping_sonnet.get_untracked(),
            mapping_haiku.get_untracked(),
        );
        set_result.set(format!("保存模型映射中: {provider_id}"));
        spawn_local(async move {
            save_model_mappings_for_provider(
                provider_id,
                provider_id_for_refresh,
                mappings,
                set_active_provider_id,
                set_selected_provider_id,
                set_providers,
                set_mapping_default,
                set_mapping_opus,
                set_mapping_sonnet,
                set_mapping_haiku,
                set_model_mapping_json,
                set_readiness_snapshot,
                set_restart_required,
                set_result,
            )
            .await;
        });
    };

    let save_editing_model_mappings = move |_| {
        let Some(provider_id) = editing_provider_id.get_untracked() else {
            set_result.set(
                "update_model_mappings skipped: save or edit an existing provider first".to_owned(),
            );
            return;
        };
        let provider_id_for_refresh = provider_id.clone();
        let mappings = visible_model_mapping_drafts(
            mapping_default.get_untracked(),
            mapping_opus.get_untracked(),
            mapping_sonnet.get_untracked(),
            mapping_haiku.get_untracked(),
        );
        set_result.set(format!("保存模型映射中: {provider_id}"));
        spawn_local(async move {
            save_model_mappings_for_provider(
                provider_id,
                provider_id_for_refresh,
                mappings,
                set_active_provider_id,
                set_selected_provider_id,
                set_providers,
                set_mapping_default,
                set_mapping_opus,
                set_mapping_sonnet,
                set_mapping_haiku,
                set_model_mapping_json,
                set_readiness_snapshot,
                set_restart_required,
                set_result,
            )
            .await;
        });
    };

    let list_config_backups = move |_| {
        set_result.set("读取 config backups 中...".to_owned());
        spawn_local(async move {
            match commands::list_config_backups().await {
                Ok(backups) => {
                    if let Some(first) = backups.first() {
                        set_backup_file_name.set(first.file_name.clone());
                    }
                    set_result.set(format_config_backups(&backups));
                }
                Err(error) => set_result.set(format!("list_config_backups failed: {error}")),
            }
        });
    };

    let read_selected_backup = move |_| {
        let file_name = backup_file_name.get_untracked();
        if file_name.trim().is_empty() {
            set_result.set("read_config_backup skipped: no backup file selected".to_owned());
            return;
        }
        set_result.set(format!("读取 config backup 中: {file_name}"));
        spawn_local(async move {
            match commands::read_config_backup(file_name).await {
                Ok(redacted) => set_result.set(redacted),
                Err(error) => set_result.set(format!("read_config_backup failed: {error}")),
            }
        });
    };

    let create_backup_now = move |_| {
        set_result.set("创建 config backup 中...".to_owned());
        spawn_local(async move {
            match commands::create_config_backup().await {
                Ok(Some(backup)) => {
                    set_backup_file_name.set(backup.file_name.clone());
                    set_result.set(format!(
                        "create_config_backup ok\n{} | {} bytes",
                        backup.file_name, backup.size
                    ));
                }
                Ok(None) => {
                    set_result.set("create_config_backup skipped: config file not found".to_owned())
                }
                Err(error) => set_result.set(format!("create_config_backup failed: {error}")),
            }
        });
    };

    let save_settings = move |_| {
        let proxy_port_value = match parse_proxy_port(&proxy_port.get_untracked()) {
            Ok(port) => port,
            Err(error) => {
                set_result.set(error);
                return;
            }
        };
        let settings = ConfigSettings {
            theme: theme.get_untracked(),
            language: language.get_untracked(),
            proxy_port: proxy_port_value,
            update_url: update_url_or_default(&update_url.get_untracked()),
        };
        set_result.set("保存 settings 中...".to_owned());
        spawn_local(async move {
            match commands::update_settings(settings).await {
                Ok(saved) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    set_proxy_port.set(saved.proxy_port.to_string());
                    set_update_url.set(saved.update_url.clone());
                    if let Ok(status) = commands::get_proxy_status().await {
                        set_proxy_status.set(Some(status));
                    }
                    set_result.set(format!(
                        "update_settings ok\ntheme={}\nlanguage={}\nproxyPort={}\nupdateUrl={}",
                        saved.theme, saved.language, saved.proxy_port, saved.update_url
                    ));
                }
                Err(error) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_settings_state_from_backend(
                        set_language,
                        set_theme,
                        set_proxy_port,
                        set_update_url,
                        set_proxy_status,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "update_settings",
                        &error,
                        refresh_note,
                    ));
                }
            }
        });
    };

    let check_update_now = move |_| {
        set_update_download_path.set(None);
        let proxy_port_value = match parse_proxy_port(&proxy_port.get_untracked()) {
            Ok(port) => port,
            Err(error) => {
                set_result.set(error);
                return;
            }
        };
        let settings = ConfigSettings {
            theme: theme.get_untracked(),
            language: language.get_untracked(),
            proxy_port: proxy_port_value,
            update_url: update_url_or_default(&update_url.get_untracked()),
        };
        set_result.set("检查更新中...".to_owned());
        spawn_local(async move {
            match commands::update_settings(settings).await {
                Ok(saved) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    set_update_url.set(saved.update_url.clone());
                    match commands::check_update().await {
                        Ok(check) => {
                            if !check.available {
                                set_update_download_path.set(None);
                            }
                            set_result.set(format_update_check(&check));
                        }
                        Err(error) => {
                            set_update_download_path.set(None);
                            set_result.set(format!("check_update failed: {error}"));
                        }
                    }
                }
                Err(error) => {
                    set_update_download_path.set(None);
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_settings_state_from_backend(
                        set_language,
                        set_theme,
                        set_proxy_port,
                        set_update_url,
                        set_proxy_status,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "update_settings",
                        &error,
                        refresh_note,
                    ));
                }
            }
        });
    };

    let download_update_now = move |_| {
        set_update_download_path.set(None);
        let proxy_port_value = match parse_proxy_port(&proxy_port.get_untracked()) {
            Ok(port) => port,
            Err(error) => {
                set_result.set(error);
                return;
            }
        };
        let settings = ConfigSettings {
            theme: theme.get_untracked(),
            language: language.get_untracked(),
            proxy_port: proxy_port_value,
            update_url: update_url_or_default(&update_url.get_untracked()),
        };
        set_result.set("下载并验证更新中...".to_owned());
        spawn_local(async move {
            match commands::update_settings(settings).await {
                Ok(saved) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    set_update_url.set(saved.update_url.clone());
                    match commands::download_update().await {
                        Ok(download) => {
                            set_update_download_path.set(Some(download.asset_path.clone()));
                            set_result.set(format_update_download(&download));
                        }
                        Err(error) => {
                            set_update_download_path.set(None);
                            set_result.set(format!("download_update failed: {error}"));
                        }
                    }
                }
                Err(error) => {
                    set_update_download_path.set(None);
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_settings_state_from_backend(
                        set_language,
                        set_theme,
                        set_proxy_port,
                        set_update_url,
                        set_proxy_status,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "update_settings",
                        &error,
                        refresh_note,
                    ));
                }
            }
        });
    };

    let install_update_now = move |_| {
        let Some(path) = update_download_path.get_untracked() else {
            set_result.set(
                "install_update skipped: no verified installer has been downloaded".to_owned(),
            );
            return;
        };
        set_result.set("启动已验证安装器中...".to_owned());
        spawn_local(async move {
            match commands::install_update(path).await {
                Ok(result) => set_result.set(format!(
                    "install_update ok\nlaunched={}\ninstaller={}\ninstallerType={}\nlaunchMethod={}",
                    result.launched,
                    result.installer_path,
                    result.installer_type,
                    result.launch_method
                )),
                Err(error) => set_result.set(format!("install_update failed: {error}")),
            }
        });
    };

    let read_proxy_logs = move |_| {
        set_proxy_logs_text.set("读取 gateway logs 中...".to_owned());
        spawn_local(async move {
            match commands::get_proxy_logs().await {
                Ok(logs) => {
                    let text = format_proxy_logs(&logs);
                    set_proxy_logs.set(logs);
                    set_proxy_logs_text.set(text.clone());
                    set_result.set(text);
                    if let Ok(status) = commands::get_proxy_status().await {
                        set_proxy_port.set(status.port.to_string());
                        set_proxy_status.set(Some(status));
                    }
                }
                Err(error) => set_proxy_logs_text.set(format!("get_proxy_logs failed: {error}")),
            }
        });
    };

    let clear_proxy_logs = move |_| {
        set_proxy_logs_text.set("清除 gateway logs 中...".to_owned());
        spawn_local(async move {
            match commands::clear_proxy_logs().await {
                Ok(changed) => {
                    let text = format!("clear_proxy_logs changed={changed}");
                    set_proxy_logs.set(Vec::new());
                    set_proxy_logs_text.set(text.clone());
                    set_result.set(text);
                    if let Ok(status) = commands::get_proxy_status().await {
                        set_proxy_port.set(status.port.to_string());
                        set_proxy_status.set(Some(status));
                    }
                }
                Err(error) => set_proxy_logs_text.set(format!("clear_proxy_logs failed: {error}")),
            }
        });
    };

    let run_health_check = move || {
        set_result.set("读取 health 中...".to_owned());
        spawn_local(async move {
            match commands::health().await {
                Ok(snapshot) => {
                    set_readiness_snapshot.set(Some(snapshot.clone()));
                    set_gateway_status_text.set(format_gateway_health(&snapshot.gateway));
                    if let Ok(status) = commands::get_proxy_status().await {
                        set_proxy_port.set(status.port.to_string());
                        set_proxy_status.set(Some(status));
                    }
                    set_result.set(format!(
                        "providerConfigured: {}\ndesktopReadbackPassed: {}\nproviderSmokePassed: {}\ngatewaySmokePassed: {}\ngateway: {} running={}\nissues: {}",
                        snapshot.provider_configured,
                        snapshot.desktop_readback_passed,
                        snapshot.provider_smoke_passed,
                        snapshot.gateway_smoke_passed,
                        snapshot.gateway.base_url,
                        snapshot.gateway.running,
                        snapshot.issue_codes.join(", ")
                    ));
                }
                Err(error) => set_result.set(format!("health failed: {error}")),
            }
        });
    };
    let check_health = move |_| run_health_check();

    let start_gateway = move |_| {
        let proxy_port_value = match parse_proxy_port(&proxy_port.get_untracked()) {
            Ok(port) => port,
            Err(error) => {
                set_result.set(error);
                return;
            }
        };
        let settings = ConfigSettings {
            theme: theme.get_untracked(),
            language: language.get_untracked(),
            proxy_port: proxy_port_value,
            update_url: update_url_or_default(&update_url.get_untracked()),
        };
        set_result.set("启动 gateway 中...".to_owned());
        spawn_local(async move {
            if let Err(error) = commands::update_settings(settings).await {
                mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                let refresh_note = refresh_settings_state_from_backend(
                    set_language,
                    set_theme,
                    set_proxy_port,
                    set_update_url,
                    set_proxy_status,
                )
                .await;
                set_result.set(format!(
                    "{}\nstart_gateway skipped",
                    format_backend_mutation_error("update_settings", &error, refresh_note)
                ));
                return;
            }
            mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
            match commands::start_gateway().await {
                Ok(health) => {
                    let formatted = format_gateway_health(&health);
                    let status_note = match commands::get_proxy_status().await {
                        Ok(status) => {
                            set_proxy_port.set(status.port.to_string());
                            set_proxy_status.set(Some(status));
                            String::new()
                        }
                        Err(error) => format!("\nget_proxy_status failed: {error}"),
                    };
                    set_gateway_status_text.set(formatted.clone());
                    set_result.set(format!("gateway started: {formatted}{status_note}"));
                }
                Err(error) => set_result.set(format!("start_gateway failed: {error}")),
            }
        });
    };

    let stop_gateway = move |_| {
        set_result.set("停止 gateway 中...".to_owned());
        spawn_local(async move {
            match commands::stop_gateway().await {
                Ok(health) => {
                    let formatted = format_gateway_health(&health);
                    let status_note = match commands::get_proxy_status().await {
                        Ok(status) => {
                            set_proxy_port.set(status.port.to_string());
                            set_proxy_status.set(Some(status));
                            String::new()
                        }
                        Err(error) => format!("\nget_proxy_status failed: {error}"),
                    };
                    set_gateway_status_text.set(formatted.clone());
                    set_result.set(format!("gateway stopped: {formatted}{status_note}"));
                }
                Err(error) => set_result.set(format!("stop_gateway failed: {error}")),
            }
        });
    };

    let probe_desktop_config = move |_| {
        set_result.set("探测 Claude Desktop 配置路径中...".to_owned());
        spawn_local(async move {
            match commands::desktop_config_probe().await {
                Ok(probe) => {
                    set_result.set(format!(
                        "platform: {:?}\nlocalConfigLibrary: {}\nmanagedDetected: {}\nissues: {}\nmanagedEvidence: {}{}",
                        probe.platform,
                        probe.local_config_library,
                        probe.managed_detected,
                        probe.issue_codes.join(", "),
                        probe.managed_evidence
                            .iter()
                            .map(|evidence| format!("{} ({})", evidence.location, evidence.detail))
                            .collect::<Vec<_>>()
                            .join("; "),
                        desktop_managed_policy_note(probe.managed_detected)
                    ));
                }
                Err(error) => set_result.set(format!("desktop_config_probe failed: {error}")),
            }
        });
    };

    let clear_desktop_config = move |_| {
        if !confirm_action("这会清除本工具写入 Claude Desktop 的本机 gateway 配置，不会删除本工具保存的 Provider 和 API Key。继续？") {
            return;
        }
        set_result.set("清除 Claude Desktop 配置中...".to_owned());
        mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
        spawn_local(async move {
            match commands::clear_desktop_config().await {
                Ok(clear_result) => {
                    set_restart_required.set(clear_result.success);
                    set_result.set(format_desktop_clear_result(&clear_result));
                    if let Ok(snapshot) = commands::health().await {
                        set_readiness_snapshot.set(Some(snapshot));
                    }
                }
                Err(error) => set_result.set(format!("clear_desktop_config failed: {error}")),
            }
        });
    };

    let restart_claude_desktop = move |_| {
        if !confirm_action(
            "这会尝试关闭并重新打开 Claude Desktop。未保存的 Claude Desktop 内容可能受影响。继续？",
        ) {
            return;
        }
        set_result.set("重启 Claude Desktop 中...".to_owned());
        spawn_local(async move {
            match commands::restart_claude_desktop().await {
                Ok(restart_result) => {
                    if restart_result.launched {
                        set_restart_required.set(false);
                    }
                    set_result.set(format_desktop_restart_result(&restart_result));
                }
                Err(error) => set_result.set(format!("restart_claude_desktop failed: {error}")),
            }
        });
    };

    let run_copy_diagnostics_summary = move || {
        set_diagnostics_text.set("生成 diagnostics summary 中...".to_owned());
        spawn_local(async move {
            match commands::copy_diagnostics_summary().await {
                Ok(summary) => {
                    set_diagnostics_text.set(summary.clone());
                    set_result.set(summary);
                }
                Err(error) => {
                    set_diagnostics_text.set(format!("copy_diagnostics_summary failed: {error}"));
                }
            }
        });
    };
    let copy_diagnostics_summary = move |_: leptos::ev::MouseEvent| {
        run_copy_diagnostics_summary();
    };

    let export_diagnostics_package = move |_| {
        set_diagnostics_text.set("生成 diagnostics package 中...".to_owned());
        spawn_local(async move {
            match commands::export_diagnostics_package().await {
                Ok(package) => match serde_json::to_string_pretty(&package) {
                    Ok(raw) => set_diagnostics_text.set(raw),
                    Err(error) => {
                        set_diagnostics_text.set(format!("format diagnostics failed: {error}"))
                    }
                },
                Err(error) => {
                    set_diagnostics_text.set(format!("export_diagnostics_package failed: {error}"));
                }
            }
        });
    };

    let copy_diagnostics_to_clipboard = move |_| {
        set_diagnostics_text.set("复制 diagnostics summary 中...".to_owned());
        spawn_local(async move {
            match commands::copy_diagnostics_summary_to_clipboard().await {
                Ok(summary) => set_diagnostics_text.set(summary),
                Err(error) => set_diagnostics_text.set(format!(
                    "copy_diagnostics_summary_to_clipboard failed: {error}"
                )),
            }
        });
    };

    let save_diagnostics_package = move |_| {
        set_diagnostics_text.set("保存 diagnostics package 中...".to_owned());
        spawn_local(async move {
            match commands::save_diagnostics_package().await {
                Ok(path) => set_diagnostics_text.set(format!("diagnostics package saved:\n{path}")),
                Err(error) => {
                    set_diagnostics_text.set(format!("save_diagnostics_package failed: {error}"))
                }
            }
        });
    };

    let save_diagnostics_package_as = move |_: leptos::ev::MouseEvent| {
        set_diagnostics_text.set("选择 diagnostics package 保存位置中...".to_owned());
        spawn_local(async move {
            match commands::save_diagnostics_package_as().await {
                Ok(Some(path)) => {
                    set_diagnostics_text.set(format!("diagnostics package saved:\n{path}"))
                }
                Ok(None) => {
                    set_diagnostics_text.set("save_diagnostics_package_as canceled".to_owned())
                }
                Err(error) => {
                    set_diagnostics_text.set(format!("save_diagnostics_package_as failed: {error}"))
                }
            }
        });
    };

    let preview_issue_draft = move |_: leptos::ev::MouseEvent| {
        set_diagnostics_text.set("生成 GitHub Issue draft 中...".to_owned());
        spawn_local(async move {
            match commands::diagnostics_issue_draft().await {
                Ok(draft) => set_diagnostics_text.set(format!(
                    "title: {}\nurl: {}\n\n{}",
                    draft.title, draft.url, draft.body
                )),
                Err(error) => {
                    set_diagnostics_text.set(format!("diagnostics_issue_draft failed: {error}"))
                }
            }
        });
    };

    let open_issue = move |_: leptos::ev::MouseEvent| {
        set_diagnostics_text.set("打开 GitHub Issue 中...".to_owned());
        spawn_local(async move {
            match commands::open_diagnostics_issue().await {
                Ok(draft) => set_diagnostics_text.set(format!(
                    "opened GitHub Issue draft:\n{}\n{}",
                    draft.title, draft.url
                )),
                Err(error) => {
                    set_diagnostics_text.set(format!("open_diagnostics_issue failed: {error}"))
                }
            }
        });
    };

    let run_provider_static_smoke = move |_| {
        set_result.set("检查当前已启用 Provider 的静态配置中...".to_owned());
        spawn_local(async move {
            match commands::provider_static_smoke().await {
                Ok(smoke) => set_result.set(format_smoke_result(&smoke)),
                Err(error) => set_result.set(format!("provider_static_smoke failed: {error}")),
            }
        });
    };

    let run_gateway_smoke = move |_| {
        set_result.set("运行 gateway smoke 中...".to_owned());
        spawn_local(async move {
            match commands::gateway_smoke().await {
                Ok(smoke) => set_result.set(format_smoke_result(&smoke)),
                Err(error) => set_result.set(format!("gateway_smoke failed: {error}")),
            }
        });
    };

    let run_provider_real_smoke = move |_| {
        set_result.set("运行 provider real smoke 中...".to_owned());
        spawn_local(async move {
            match commands::provider_real_smoke().await {
                Ok(smoke) => set_result.set(format_smoke_result(&smoke)),
                Err(error) => set_result.set(format!("provider_real_smoke failed: {error}")),
            }
        });
    };

    let dry_run = move |_: leptos::ev::MouseEvent| {
        set_result.set("生成 apply dry-run 中...".to_owned());
        spawn_local(async move {
            match commands::apply_dry_run().await {
                Ok(plan) => {
                    let routes = plan
                        .expected_models
                        .iter()
                        .map(|model| model.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let steps = plan
                        .steps
                        .iter()
                        .map(|step| format!("- {}: wouldRun={}", step.id, step.would_run))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let plan_error = plan
                        .plan_error
                        .as_deref()
                        .map(|error| format!("\nplanError: {error}"))
                        .unwrap_or_default();
                    set_result.set(format!(
                        "success: {}\nmode: {}\nexpectedBaseUrl: {}\nexpectedRoutes: {}{}\n{}",
                        plan.success, plan.mode, plan.expected_base_url, routes, plan_error, steps
                    ));
                }
                Err(error) => set_result.set(format!("apply_dry_run failed: {error}")),
            }
        });
    };

    let run_apply = move || {
        set_result.set("执行 apply 中...".to_owned());
        mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
        spawn_local(async move {
            match commands::apply_detected_local_config().await {
                Ok(result) => {
                    set_restart_required.set(result.success);
                    if result.success {
                        if let Ok(snapshot) = commands::health().await {
                            set_gateway_status_text.set(format_gateway_health(&snapshot.gateway));
                            set_readiness_snapshot.set(Some(snapshot));
                        }
                    }
                    if let Ok(status) = commands::get_proxy_status().await {
                        set_proxy_port.set(status.port.to_string());
                        set_proxy_status.set(Some(status));
                    }
                    let gateway = result
                        .gateway
                        .as_ref()
                        .map(format_gateway_health)
                        .unwrap_or_else(|| "not started".to_owned());
                    let desktop_path = result
                        .desktop_config
                        .as_ref()
                        .map(|probe| probe.local_config_library.clone())
                        .unwrap_or_else(|| "not resolved".to_owned());
                    let steps = result
                        .steps
                        .iter()
                        .map(|step| {
                            format!(
                                "- {}: {:?}{}",
                                step.id,
                                step.status,
                                step.error
                                    .as_ref()
                                    .map(|error| format!(" ({error})"))
                                    .unwrap_or_default()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    set_result.set(format!(
                        "success: {}\nmode: {}\ngateway: {}\ndesktopConfig: {}\nerror: {}\n{}{}",
                        result.success,
                        result.mode,
                        gateway,
                        desktop_path,
                        result.error.as_deref().unwrap_or_default(),
                        steps,
                        desktop_managed_policy_note(
                            result.error.as_deref() == Some("desktop.managed_config_detected")
                        )
                    ));
                }
                Err(error) => {
                    set_result.set(format!("apply_detected_local_config failed: {error}"))
                }
            }
        });
    };
    let apply = move |_| run_apply();

    let apply_provider_to_desktop = move |_| {
        let request = ProviderDraft {
            provider_id: editing_provider_id.get_untracked(),
            display_name: provider_name.get_untracked(),
            base_url: base_url.get_untracked(),
            auth_scheme: auth_scheme_from_value(&auth_scheme.get_untracked()),
            api_key: api_key.get_untracked(),
            api_format: if api_format.get_untracked() == "openai_chat" {
                ApiFormat::OpenAiChat
            } else {
                ApiFormat::Anthropic
            },
        };
        let mapping_drafts = visible_model_mapping_drafts(
            mapping_default.get_untracked(),
            mapping_opus.get_untracked(),
            mapping_sonnet.get_untracked(),
            mapping_haiku.get_untracked(),
        );
        set_result.set("保存 Provider 并应用到 Claude Desktop 中...".to_owned());
        spawn_local(async move {
            let summary = match commands::save_provider(request).await {
                Ok(summary) => summary,
                Err(error) => {
                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                    let refresh_note = refresh_provider_state_from_backend(
                        set_active_provider_id,
                        set_selected_provider_id,
                        set_providers,
                    )
                    .await;
                    set_result.set(format_backend_mutation_error(
                        "save_provider",
                        &error,
                        refresh_note,
                    ));
                    return;
                }
            };
            mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
            set_provider_saved.set(true);
            set_selected_provider_id.set(Some(summary.provider_id.clone()));
            set_editing_provider_id.set(Some(summary.provider_id.clone()));
            set_api_key.set(String::new());

            if let Err(error) =
                commands::update_model_mappings(summary.provider_id.clone(), mapping_drafts).await
            {
                let provider_refresh = refresh_provider_state_from_backend_preserving_selection(
                    set_active_provider_id,
                    set_selected_provider_id,
                    set_providers,
                    Some(summary.provider_id.clone()),
                )
                .await;
                let mapping_refresh = refresh_model_mappings_from_backend(
                    summary.provider_id.clone(),
                    set_mapping_default,
                    set_mapping_opus,
                    set_mapping_sonnet,
                    set_mapping_haiku,
                    set_model_mapping_json,
                )
                .await;
                let refresh_note =
                    merge_refresh_results(provider_refresh, mapping_refresh, "model mappings");
                set_result.set(format!(
                    "save_provider ok\n{}",
                    format_backend_mutation_error("update_model_mappings", &error, refresh_note)
                ));
                return;
            }

            if let Err(error) = set_active_provider_and_sync(
                summary.provider_id.clone(),
                set_active_provider_id,
                set_selected_provider_id,
                set_providers,
                set_readiness_snapshot,
                set_restart_required,
            )
            .await
            {
                set_result.set(format!("save_provider ok\n{error}"));
                return;
            }

            match commands::apply_detected_local_config().await {
                Ok(apply_result) => {
                    set_restart_required.set(apply_result.success);
                    if let Ok(next_providers) = commands::list_providers().await {
                        set_providers.set(next_providers);
                    }
                    if apply_result.success {
                        if let Ok(snapshot) = commands::health().await {
                            set_gateway_status_text.set(format_gateway_health(&snapshot.gateway));
                            set_readiness_snapshot.set(Some(snapshot));
                        }
                    }
                    if let Ok(status) = commands::get_proxy_status().await {
                        set_proxy_port.set(status.port.to_string());
                        set_proxy_status.set(Some(status));
                    }
                    let steps = apply_result
                        .steps
                        .iter()
                        .map(|step| {
                            format!(
                                "- {}: {:?}{}",
                                step.id,
                                step.status,
                                step.error
                                    .as_ref()
                                    .map(|error| format!(" ({error})"))
                                    .unwrap_or_default()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    set_result.set(format!(
                        "success: {}\nprovider: {}\nmode: {}\nerror: {}\n{}{}",
                        apply_result.success,
                        summary.provider_id,
                        apply_result.mode,
                        apply_result.error.as_deref().unwrap_or_default(),
                        steps,
                        desktop_managed_policy_note(
                            apply_result.error.as_deref()
                                == Some("desktop.managed_config_detected")
                        )
                    ));
                }
                Err(error) => {
                    set_result.set(format!("apply_detected_local_config failed: {error}"))
                }
            }
        });
    };

    spawn_local(async move {
        if let Ok(snapshot) = commands::get_config_snapshot().await {
            if snapshot.settings.language == "zh" || snapshot.settings.language == "en" {
                set_language.set(snapshot.settings.language.clone());
            }
            if is_supported_theme(&snapshot.settings.theme) {
                set_theme.set(snapshot.settings.theme.clone());
            }
            set_proxy_port.set(snapshot.settings.proxy_port.to_string());
            set_update_url.set(snapshot.settings.update_url.clone());
            set_active_provider_id.set(snapshot.active_provider.clone());
            set_selected_provider_id.set(snapshot.active_provider.clone().or_else(|| {
                snapshot
                    .providers
                    .first()
                    .map(|provider| provider.provider_id.clone())
            }));
            set_providers.set(snapshot.providers);
        }
        if let Ok(proxy) = commands::get_proxy_status().await {
            set_proxy_port.set(proxy.port.to_string());
            set_proxy_status.set(Some(proxy));
        }
        if let Ok(settings) = commands::get_settings().await {
            if settings.language == "zh" || settings.language == "en" {
                set_language.set(settings.language.clone());
            }
            if is_supported_theme(&settings.theme) {
                set_theme.set(settings.theme.clone());
            }
            set_proxy_port.set(settings.proxy_port.to_string());
            set_update_url.set(settings.update_url);
        }
        if let Ok(next_providers) = commands::list_providers().await {
            if selected_provider_id.get_untracked().is_none() {
                if let Some(provider) = next_providers.first() {
                    set_selected_provider_id.set(Some(provider.provider_id.clone()));
                }
            }
            set_providers.set(next_providers);
        }
        if let Ok(presets) = commands::list_provider_presets().await {
            set_provider_presets.set(presets);
        }
        if let Ok(snapshot) = commands::health().await {
            set_gateway_status_text.set(format_gateway_health(&snapshot.gateway));
            set_readiness_snapshot.set(Some(snapshot));
        }
    });

    view! {
        <div class="app-shell" data-theme=move || theme.get()>
            <header class="app-header">
                <div class="header-actions-left">
                    <button class="dashboard-import-button" type="button" on:click=move |_| set_active_page.set(AppPage::Settings)>
                        <span class="button-icon icon-svg icon-import" aria-hidden="true"></span><span>"导入 CC Switch 配置"</span>
                    </button>
                    <button class="dashboard-clear-button" type="button" on:click=clear_desktop_config>
                        <span class="button-icon icon-svg icon-trash" aria-hidden="true"></span><span>"清除桌面版配置"</span>
                    </button>
                </div>

                <nav class="route-tabs" aria-label="Primary navigation">
                    <button title="Dashboard" class=move || route_tab_class(active_page.get() == AppPage::Dashboard) type="button" on:click=move |_| set_active_page.set(AppPage::Dashboard)>
                        <span class="tab-icon icon-svg icon-dashboard" aria-hidden="true"></span>
                        <span>{move || copy("nav_dashboard")}</span>
                    </button>
                    <button title="Providers" class=move || route_tab_class(primary_route_active(active_page.get(), AppPage::Providers)) type="button" on:click=move |_| set_active_page.set(AppPage::Providers)>
                        <span class="tab-icon icon-svg icon-plug" aria-hidden="true"></span>
                        <span>{move || copy("nav_provider")}</span>
                    </button>
                    <button title="Forwarding" class=move || route_tab_class(active_page.get() == AppPage::Proxy) type="button" on:click=move |_| set_active_page.set(AppPage::Proxy)>
                        <span class="tab-icon icon-svg icon-broadcast" aria-hidden="true"></span>
                        <span>{move || copy("nav_proxy")}</span>
                    </button>
                    <button title="Settings" class=move || route_tab_class(active_page.get() == AppPage::Settings) type="button" on:click=move |_| set_active_page.set(AppPage::Settings)>
                        <span class="tab-icon icon-svg icon-gear" aria-hidden="true"></span>
                        <span>{move || copy("nav_settings")}</span>
                    </button>
                    <button title="Guide" class=move || route_tab_class(active_page.get() == AppPage::Guide) type="button" on:click=move |_| set_active_page.set(AppPage::Guide)>
                        <span class="tab-icon icon-svg icon-book" aria-hidden="true"></span>
                        <span>{move || copy("nav_guide")}</span>
                    </button>
                </nav>

                <div class="header-actions">
                    <button class="update-badge" type="button" hidden=true>"有新版本"</button>
                    <button class="theme-btn" type="button" aria-label="Settings" on:click=move |_| set_active_page.set(AppPage::Settings)>
                        <span class="icon-svg icon-gear" aria-hidden="true"></span>
                    </button>
                    <button class="language-toggle" type="button" on:click=move |_| {
                        set_language.update(|value| {
                            *value = if value == "zh" {
                                "en".to_owned()
                            } else if value == "en" {
                                "ja".to_owned()
                            } else {
                                "zh".to_owned()
                            };
                        });
                    }>{move || language_switch_label(&language.get())}</button>
                    <button class="theme-btn" type="button" aria-label="Toggle theme" on:click=move |_| {
                        set_theme.update(|value| {
                            *value = if value == "dark" { "light".to_owned() } else { "dark".to_owned() };
                        });
                    }>
                        <span class=move || if theme.get() == "dark" { "icon-svg icon-sun" } else { "icon-svg icon-moon" } aria-hidden="true"></span>
                    </button>
                    <button class="round-add" type="button" aria-label="Add provider" on:click=move |_| {
                        reset_provider_form(
                            set_editing_provider_id,
                            set_selected_preset_id,
                            set_provider_name,
                            set_base_url,
                            set_api_key,
                            set_api_format,
                            set_auth_scheme,
                        );
                        set_active_page.set(AppPage::ProvidersAdd);
                    }>
                        <span class="icon-svg icon-plus" aria-hidden="true"></span>
                    </button>
                </div>
            </header>

            <main class="app-main">
                <section class=move || page_title_class(active_page.get())>
                    <h1>{move || page_title(active_page.get(), &language.get())}</h1>
                    <p>{move || page_subtitle(active_page.get(), &language.get())}</p>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::Dashboard)>
                    <div class=move || desktop_warning_class(readiness_snapshot.get().as_ref())>
                        <span>"!"</span>
                        <strong>{move || readiness_issue_text(readiness_snapshot.get().as_ref())}</strong>
                    </div>

                    <div class="switch-board">
                        <div class="provider-card-list">
                            <div class=move || configured_provider_list_class(!providers.get().is_empty())>
                                <For
                                    each=move || providers.get()
                                    key=|provider| provider.provider_id.clone()
                                    children=move |provider| {
                                        let provider_id_for_card_class = provider.provider_id.clone();
                                        let provider_id_for_button_class = provider.provider_id.clone();
                                        let provider_id_for_button_label = provider.provider_id.clone();
                                        let provider_id_for_disabled = provider.provider_id.clone();
                                        let provider_id_for_apply = provider.provider_id.clone();
                                        let provider_id_for_test = provider.provider_id.clone();
                                        let provider_id_for_test_disabled = provider.provider_id.clone();
                                        let provider_id_for_copy = provider.provider_id.clone();
                                        let provider_url_for_copy = provider.base_url.clone();
                                        let provider_id_for_delete = provider.provider_id.clone();
                                        let edit_provider = provider.clone();
                                        let logo_src = provider_logo_src(&provider.display_name);
                                        view! {
                                            <article class=move || provider_switch_card_class(active_provider_id.get().as_deref() == Some(provider_id_for_card_class.as_str()))>
                                                <span class="drag-handle"><span class="icon-svg icon-grip" aria-hidden="true"></span></span>
                                                <span class="provider-logo"><img src=logo_src alt="" /></span>
                                                <span class="provider-main">
                                                    <strong>{provider.display_name.clone()}</strong>
                                                    <span>{provider.base_url.clone()}</span>
                                                </span>
                                                <span class="provider-actions">
                                                    <button
                                                        class=move || compact_enable_class(active_provider_id.get().as_deref() == Some(provider_id_for_button_class.as_str()))
                                                        type="button"
                                                        disabled=move || active_provider_id.get().as_deref() == Some(provider_id_for_disabled.as_str())
                                                        on:click=move |_| {
                                                            let provider_id = provider_id_for_apply.clone();
                                                            set_selected_provider_id.set(Some(provider_id.clone()));
                                                            set_result.set(format!("设置 active Provider 中: {provider_id}"));
                                                            spawn_local(async move {
                                                                match set_active_provider_and_sync(
                                                                    provider_id,
                                                                    set_active_provider_id,
                                                                    set_selected_provider_id,
                                                                    set_providers,
                                                                    set_readiness_snapshot,
                                                                    set_restart_required,
                                                                ).await {
                                                                    Ok(message) => set_result.set(message),
                                                                    Err(message) => set_result.set(message),
                                                                }
                                                            });
                                                        }
                                                    >
                                                        <span class="icon-svg icon-play" aria-hidden="true"></span>
                                                        <span>{move || if active_provider_id.get().as_deref() == Some(provider_id_for_button_label.as_str()) { "默认" } else { "启用" }}</span>
                                                    </button>
                                                    <button
                                                        class="icon-action"
                                                        type="button"
                                                        title="Test speed"
                                                        disabled=move || active_provider_id.get().as_deref() != Some(provider_id_for_test_disabled.as_str())
                                                        on:click=move |_| {
                                                        let provider_id = provider_id_for_test.clone();
                                                        set_result.set(format!("运行 provider real smoke: {provider_id}"));
                                                        spawn_local(async move {
                                                            match commands::provider_real_smoke().await {
                                                                Ok(smoke) => set_result.set(format_smoke_result(&smoke)),
                                                                Err(error) => set_result.set(format!("provider_real_smoke failed: {error}")),
                                                            }
                                                        });
                                                    }>
                                                        <span class="icon-svg icon-lightning" aria-hidden="true"></span>
                                                    </button>
                                                    <button class="icon-action" type="button" title="Copy URL" on:click=move |_| {
                                                        let provider_id = provider_id_for_copy.clone();
                                                        let url = provider_url_for_copy.clone();
                                                        set_result.set(format!("copy provider url: {provider_id}"));
                                                        spawn_local(async move {
                                                            match commands::copy_text_to_clipboard(url).await {
                                                                Ok(_) => set_result.set(format!("copied provider url: {provider_id}")),
                                                                Err(error) => set_result.set(format!("copy provider url failed: {error}")),
                                                            }
                                                        });
                                                    }>
                                                        <span class="icon-svg icon-copy" aria-hidden="true"></span>
                                                    </button>
                                                    <button class="icon-action" type="button" title="Edit" on:click=move |_| {
                                                        set_selected_provider_id.set(Some(edit_provider.provider_id.clone()));
                                                        set_editing_provider_id.set(Some(edit_provider.provider_id.clone()));
                                                        set_provider_name.set(edit_provider.display_name.clone());
                                                        set_base_url.set(edit_provider.base_url.clone());
                                                        set_api_format.set(api_format_value(&edit_provider.api_format));
                                                        set_auth_scheme.set(auth_scheme_value(&edit_provider.auth_scheme));
                                                        set_api_key.set(String::new());
                                                        set_selected_preset_id.set(String::new());
                                                        set_active_page.set(AppPage::ProvidersAdd);
                                                    }>
                                                        <span class="icon-svg icon-edit" aria-hidden="true"></span>
                                                    </button>
                                                    <button class="icon-action" type="button" title="Forwarding" on:click=move |_| set_active_page.set(AppPage::Proxy)>
                                                        <span class="icon-svg icon-terminal" aria-hidden="true"></span>
                                                    </button>
                                                    <button class="icon-action danger-icon" type="button" title="Delete" on:click=move |_| {
                                                        let provider_id = provider_id_for_delete.clone();
                                                        set_result.set(format!("删除 Provider 中: {provider_id}"));
                                                        spawn_local(async move {
                                                            match commands::delete_provider(provider_id.clone()).await {
                                                                Ok(changed) => match commands::get_config_snapshot().await {
                                                                    Ok(snapshot) => {
                                                                        mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                                                                        set_active_provider_id.set(snapshot.active_provider.clone());
                                                                        set_selected_provider_id.set(snapshot.active_provider.clone().or_else(|| snapshot.providers.first().map(|provider| provider.provider_id.clone())));
                                                                        let next_providers = snapshot.providers;
                                                                        set_providers.set(next_providers.clone());
                                                                        set_result.set(format!("delete_provider changed={changed}\n{}", format_provider_list(&next_providers)));
                                                                    }
                                                                    Err(error) => {
                                                                        mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                                                                        set_result.set(format!("delete_provider changed={changed}\nget_config_snapshot failed: {error}"));
                                                                    }
                                                                },
                                                                Err(error) => {
                                                                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                                                                    let refresh_note = refresh_provider_state_from_backend(
                                                                        set_active_provider_id,
                                                                        set_selected_provider_id,
                                                                        set_providers,
                                                                    )
                                                                    .await;
                                                                    set_result.set(format_backend_mutation_error(
                                                                        "delete_provider",
                                                                        &error,
                                                                        refresh_note,
                                                                    ));
                                                                }
                                                            }
                                                        });
                                                    }>
                                                        <span class="icon-svg icon-trash" aria-hidden="true"></span>
                                                    </button>
                                                </span>
                                            </article>
                                        }
                                    }
                                />
                            </div>

                            <div class=move || dashboard_empty_preset_grid_class(providers.get().is_empty())>
                                <div class="cc-switch-empty-state">
                                    <span class="empty-state-icon"><span class="icon-svg icon-users" aria-hidden="true"></span></span>
                                    <h2>"还没有添加任何供应商"</h2>
                                    <p>"如果你已有配置，请点击“导入当前配置”；所有数据将安全保存在 default 供应商中。"</p>
                                    <div class="empty-state-actions">
                                        <button class="primary-button" type="button" on:click=move |_| set_active_page.set(AppPage::Settings)>
                                            <span class="icon-svg icon-import" aria-hidden="true"></span><span>"导入当前配置"</span>
                                        </button>
                                        <button class="secondary-button" type="button" on:click=move |_| {
                                            reset_provider_form(
                                                set_editing_provider_id,
                                                set_selected_preset_id,
                                                set_provider_name,
                                                set_base_url,
                                                set_api_key,
                                                set_api_format,
                                                set_auth_scheme,
                                            );
                                            set_active_page.set(AppPage::ProvidersAdd);
                                        }>
                                            <span>"添加供应商"</span>
                                        </button>
                                    </div>
                                </div>
                                <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                    set_editing_provider_id.set(None);
                                    set_selected_preset_id.set("deepseek".to_owned());
                                    set_provider_name.set("DeepSeek".to_owned());
                                    set_base_url.set("https://api.deepseek.com/anthropic".to_owned());
                                    set_api_format.set("anthropic".to_owned());
                                    set_auth_scheme.set("bearer".to_owned());
                                    set_api_key.set(String::new());
                                    set_active_page.set(AppPage::ProvidersAdd);
                                }>
                                    <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                    <span class="provider-logo"><img src="deepseek.ico" alt="" /></span>
                                    <span class="provider-main"><strong>"DeepSeek"</strong><span>"https://api.deepseek.com/anthropic"</span></span>
                                    <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                </button>
                                <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                    set_editing_provider_id.set(None);
                                    set_selected_preset_id.set("kimi".to_owned());
                                    set_provider_name.set("Kimi（月之暗面）".to_owned());
                                    set_base_url.set("https://api.moonshot.cn/v1".to_owned());
                                    set_api_format.set("openai_chat".to_owned());
                                    set_auth_scheme.set("bearer".to_owned());
                                    set_api_key.set(String::new());
                                    set_active_page.set(AppPage::ProvidersAdd);
                                }>
                                    <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                    <span class="provider-logo"><img src="kimi.ico" alt="" /></span>
                                    <span class="provider-main"><strong>"Kimi（月之暗面）"</strong><span>"https://api.moonshot.cn/v1"</span></span>
                                    <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                </button>
                                <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                    set_editing_provider_id.set(None);
                                    set_selected_preset_id.set("qiniu".to_owned());
                                    set_provider_name.set("七牛云 AI".to_owned());
                                    set_base_url.set("https://api.qnaigc.com/v1".to_owned());
                                    set_api_format.set("openai_chat".to_owned());
                                    set_auth_scheme.set("bearer".to_owned());
                                    set_api_key.set(String::new());
                                    set_active_page.set(AppPage::ProvidersAdd);
                                }>
                                    <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                    <span class="provider-logo"><img src="qiniu.ico" alt="" /></span>
                                    <span class="provider-main"><strong>"七牛云 AI"</strong><span>"https://api.qnaigc.com/v1"</span></span>
                                    <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                </button>
                                <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                    set_editing_provider_id.set(None);
                                    set_selected_preset_id.set("zhipu".to_owned());
                                    set_provider_name.set("智谱 GLM".to_owned());
                                    set_base_url.set("https://open.bigmodel.cn/api/paas/v4/".to_owned());
                                    set_api_format.set("openai_chat".to_owned());
                                    set_auth_scheme.set("bearer".to_owned());
                                    set_api_key.set(String::new());
                                    set_active_page.set(AppPage::ProvidersAdd);
                                }>
                                    <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                    <span class="provider-logo"><img src="zhipu.png" alt="" /></span>
                                    <span class="provider-main"><strong>"智谱 GLM"</strong><span>"https://open.bigmodel.cn/api/paas/v4/"</span></span>
                                    <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                </button>
                            </div>

                            <section class=move || dashboard_preset_section_class(!providers.get().is_empty()) aria-label="继续添加提供商">
                                <div class="section-title-row compact">
                                    <div>
                                        <h2>"继续添加提供商"</h2>
                                        <p>"这里会一直保留还没添加的厂商，点一个就能带着预设进入添加页。"</p>
                                    </div>
                                </div>
                                <div class="provider-preset-grid">
                                    <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                        set_editing_provider_id.set(None);
                                        set_selected_preset_id.set("deepseek".to_owned());
                                        set_provider_name.set("DeepSeek".to_owned());
                                        set_base_url.set("https://api.deepseek.com/anthropic".to_owned());
                                        set_api_format.set("anthropic".to_owned());
                                        set_auth_scheme.set("bearer".to_owned());
                                        set_api_key.set(String::new());
                                        set_active_page.set(AppPage::ProvidersAdd);
                                    }>
                                        <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                        <span class="provider-logo"><img src="deepseek.ico" alt="" /></span>
                                        <span class="provider-main"><strong>"DeepSeek"</strong><span>"https://api.deepseek.com/anthropic"</span></span>
                                        <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                    </button>
                                    <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                        set_editing_provider_id.set(None);
                                        set_selected_preset_id.set("kimi".to_owned());
                                        set_provider_name.set("Kimi（月之暗面）".to_owned());
                                        set_base_url.set("https://api.moonshot.cn/v1".to_owned());
                                        set_api_format.set("openai_chat".to_owned());
                                        set_auth_scheme.set("bearer".to_owned());
                                        set_api_key.set(String::new());
                                        set_active_page.set(AppPage::ProvidersAdd);
                                    }>
                                        <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                        <span class="provider-logo"><img src="kimi.ico" alt="" /></span>
                                        <span class="provider-main"><strong>"Kimi（月之暗面）"</strong><span>"https://api.moonshot.cn/v1"</span></span>
                                        <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                    </button>
                                    <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                        set_editing_provider_id.set(None);
                                        set_selected_preset_id.set("qiniu".to_owned());
                                        set_provider_name.set("七牛云 AI".to_owned());
                                        set_base_url.set("https://api.qnaigc.com/v1".to_owned());
                                        set_api_format.set("openai_chat".to_owned());
                                        set_auth_scheme.set("bearer".to_owned());
                                        set_api_key.set(String::new());
                                        set_active_page.set(AppPage::ProvidersAdd);
                                    }>
                                        <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                        <span class="provider-logo"><img src="qiniu.ico" alt="" /></span>
                                        <span class="provider-main"><strong>"七牛云 AI"</strong><span>"https://api.qnaigc.com/v1"</span></span>
                                        <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                    </button>
                                    <button class="provider-switch-card preset-card" type="button" on:click=move |_| {
                                        set_editing_provider_id.set(None);
                                        set_selected_preset_id.set("zhipu".to_owned());
                                        set_provider_name.set("智谱 GLM".to_owned());
                                        set_base_url.set("https://open.bigmodel.cn/api/paas/v4/".to_owned());
                                        set_api_format.set("openai_chat".to_owned());
                                        set_auth_scheme.set("bearer".to_owned());
                                        set_api_key.set(String::new());
                                        set_active_page.set(AppPage::ProvidersAdd);
                                    }>
                                        <span class="drag-handle preset-plus"><span class="icon-svg icon-plus" aria-hidden="true"></span></span>
                                        <span class="provider-logo"><img src="zhipu.png" alt="" /></span>
                                        <span class="provider-main"><strong>"智谱 GLM"</strong><span>"https://open.bigmodel.cn/api/paas/v4/"</span></span>
                                        <span class="provider-actions"><span class="compact-enable ghost"><span class="icon-svg icon-plus" aria-hidden="true"></span><span>"添加提供商"</span></span></span>
                                    </button>
                                </div>
                            </section>
                        </div>
                    </div>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::ProvidersAdd)>
                    <div class="two-column provider-add-layout">
                    <article class="panel form-panel">
                        <div class="form-grid">
                            <label class="field">
                                <span>{move || copy("provider_name")}</span>
                                <input
                                    value=move || provider_name.get()
                                    on:input=move |event| set_provider_name.set(event_target_value(&event))
                                />
                            </label>
                            <label class="field">
                                <span class="label-line">
                                    <span>"API Base URL"</span>
                                    <button class="link-action" type="button" on:click=run_provider_static_smoke>
                                        <span class="icon-svg icon-lightning" aria-hidden="true"></span><span>"检查已启用 Provider"</span>
                                    </button>
                                </span>
                                <input
                                    value=move || base_url.get()
                                    on:input=move |event| set_base_url.set(event_target_value(&event))
                                />
                                <small class="speed-result"></small>
                            </label>
                            <div class="field">
                                <span>"API Key"</span>
                                <div class="input-with-button">
                                    <input
                                        type=move || if show_api_key.get() { "text" } else { "password" }
                                        placeholder="sk-..."
                                        value=move || api_key.get()
                                        on:input=move |event| set_api_key.set(event_target_value(&event))
                                    />
                                    <button class="input-icon" type="button" aria-label="Toggle API key" on:click=move |_| set_show_api_key.update(|value| *value = !*value)>
                                        <span class=move || if show_api_key.get() { "icon-svg icon-eye-off" } else { "icon-svg icon-eye" } aria-hidden="true"></span>
                                    </button>
                                </div>
                            </div>
                            <label class="field">
                                <span>"Auth Scheme *"</span>
                                <select
                                    class="form-select auth-scheme-trigger"
                                    on:change=move |event| set_auth_scheme.set(event_target_value(&event))
                                >
                                    <option value="bearer" selected=move || auth_scheme.get() == "bearer">"bearer"</option>
                                    <option value="x_api_key" selected=move || auth_scheme.get() == "x_api_key">"x-api-key"</option>
                                    <option value="none" selected=move || auth_scheme.get() == "none">"none"</option>
                                </select>
                            </label>

                            <details class="advanced-provider-options" open=true>
                                <summary><span class="icon-svg icon-sliders" aria-hidden="true"></span><span>"高级：第三方兼容接口"</span><span class="compat-chevron icon-svg icon-chevron-down" aria-hidden="true"></span></summary>
                                <p>"默认使用 Anthropic 兼容接口。OpenAI Chat 属于实验适配，建议确认工具调用可用后再用于 Claude Code。"</p>
                                <div class="format-choice" role="group" aria-label="API format">
                                    <button class=move || format_choice_class(api_format.get() == "anthropic") type="button" on:click=move |_| set_api_format.set("anthropic".to_owned())>
                                        <strong>"Anthropic 兼容"</strong>
                                        <span>"推荐，稳定主流程"</span>
                                    </button>
                                    <button class=move || format_choice_class(api_format.get() == "openai_chat") type="button" on:click=move |_| set_api_format.set("openai_chat".to_owned())>
                                        <strong>"OpenAI Chat 实验"</strong>
                                        <span>"用于 new-api / CPA / OpenCode Go 等兼容端点"</span>
                                    </button>
                                </div>
                                <div class="detect-format-row">
                                    <button class="secondary-button compact" type="button" on:click=run_provider_static_smoke>
                                        <span class="icon-svg icon-search" aria-hidden="true"></span><span>"检查已启用 Provider"</span>
                                    </button>
                                    <span class="detect-format-status">"此检查读取当前已启用 Provider；新增草稿需先保存并启用。"</span>
                                </div>
                            </details>

                            <section class="provider-mapping-section">
                                <div class="section-title-row">
                                    <div>
                                        <h2>{move || copy("model_mapping")}</h2>
                                        <p>"把 Claude 的 Sonnet / Haiku / Opus 映射到这个供应商真实支持的模型。"</p>
                                    </div>
                                    <button
                                        class="provider-mapping-fetch"
                                        type="button"
                                        disabled=move || editing_provider_id.get().is_none()
                                        title="Edit an existing provider before loading mappings"
                                        on:click=load_editing_model_mappings
                                    >
                                        <span class="icon-svg icon-download" aria-hidden="true"></span><span>"读取映射"</span>
                                    </button>
                                </div>
                                <div class="provider-mapping-card">
                                    <div class="provider-mapping-list">
                                        <article class="form-mapping-row">
                                            <div class="form-mapping-left">
                                                <div class="mapping-select-wrap">
                                                    <span class="mapping-icon">"D"</span>
                                                    <button class="form-select mapping-slot-trigger" type="button" disabled=true><span>"Default"</span><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-right">
                                                <div class="provider-model-input-wrap">
                                                    <input class="provider-model-input" prop:value=move || mapping_default.get() on:input=move |event| set_mapping_default.set(event_target_value(&event)) />
                                                    <button class="provider-model-trigger" type="button" disabled=true><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-actions"><span class="mapping-check-placeholder" aria-hidden="true"></span></div>
                                        </article>
                                        <article class="form-mapping-row">
                                            <div class="form-mapping-left">
                                                <div class="mapping-select-wrap">
                                                    <span class="mapping-icon">"O"</span>
                                                    <button class="form-select mapping-slot-trigger" type="button" disabled=true><span>"Opus 4.7"</span><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-right">
                                                <div class="provider-model-input-wrap">
                                                    <input class="provider-model-input" prop:value=move || mapping_opus.get() on:input=move |event| set_mapping_opus.set(event_target_value(&event)) />
                                                    <button class="provider-model-trigger" type="button" disabled=true><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-actions"><button class="secondary-button compact" type="button" on:click=run_provider_static_smoke>"检查已启用"</button></div>
                                        </article>
                                        <article class="form-mapping-row">
                                            <div class="form-mapping-left">
                                                <div class="mapping-select-wrap">
                                                    <span class="mapping-icon">"S"</span>
                                                    <button class="form-select mapping-slot-trigger" type="button" disabled=true><span>"Sonnet 4.6"</span><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-right">
                                                <div class="provider-model-input-wrap">
                                                    <input class="provider-model-input" prop:value=move || mapping_sonnet.get() on:input=move |event| set_mapping_sonnet.set(event_target_value(&event)) />
                                                    <button class="provider-model-trigger" type="button" disabled=true><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-actions"><button class="secondary-button compact" type="button" on:click=run_provider_static_smoke>"检查已启用"</button></div>
                                        </article>
                                        <article class="form-mapping-row">
                                            <div class="form-mapping-left">
                                                <div class="mapping-select-wrap">
                                                    <span class="mapping-icon">"H"</span>
                                                    <button class="form-select mapping-slot-trigger" type="button" disabled=true><span>"Haiku 4.5"</span><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-right">
                                                <div class="provider-model-input-wrap">
                                                    <input class="provider-model-input" prop:value=move || mapping_haiku.get() on:input=move |event| set_mapping_haiku.set(event_target_value(&event)) />
                                                    <button class="provider-model-trigger" type="button" disabled=true><span class="icon-svg icon-chevron-down" aria-hidden="true"></span></button>
                                                </div>
                                            </div>
                                            <div class="form-mapping-actions"><button class="secondary-button compact" type="button" on:click=run_provider_static_smoke>"检查已启用"</button></div>
                                        </article>
                                    </div>
                                    <div class="provider-mapping-footer">
                                        <button
                                            class="secondary-button compact"
                                            type="button"
                                            disabled=move || editing_provider_id.get().is_none()
                                            title="Edit an existing provider before saving mappings"
                                            on:click=save_editing_model_mappings
                                        >
                                            <span class="icon-svg icon-save" aria-hidden="true"></span><span>"保存映射"</span>
                                        </button>
                                    </div>
                                </div>
                                <div class="apply-explain">
                                    <span class="icon-svg icon-info" aria-hidden="true"></span>
                                    <p>"一键应用会保存供应商和模型映射，把它设为默认，并让 Claude 桌面版连接到本机 gateway。"</p>
                                </div>
                            </section>

                            <div class="button-row provider-form-actions">
                                <button class="primary-button form-action" type="button" on:click=apply_provider_to_desktop><span class="icon-svg icon-magic" aria-hidden="true"></span><span>"一键应用到 Claude 桌面版"</span></button>
                                <button class="secondary-button form-action" type="button" on:click=save_provider><span class="icon-svg icon-save" aria-hidden="true"></span><span>"仅保存"</span></button>
                                <button class="secondary-button form-action" type="button" on:click=move |_| {
                                    reset_provider_form(
                                        set_editing_provider_id,
                                        set_selected_preset_id,
                                        set_provider_name,
                                        set_base_url,
                                        set_api_key,
                                        set_api_format,
                                        set_auth_scheme,
                                    );
                                    set_active_page.set(AppPage::Providers);
                                }>"取消"</button>
                            </div>
                        </div>

                        <div class="provider-list">
                            <div class="provider-list-toolbar">
                                <button class="secondary-button compact" type="button" on:click=refresh_providers>"刷新列表"</button>
                            </div>
                            <For
                                each=move || providers.get()
                                key=|provider| provider.provider_id.clone()
                                children=move |provider| {
                                    let edit_provider = provider.clone();
                                    let active_provider = provider.clone();
                                    view! {
                                        <div class="provider-row">
                                            <div class="provider-meta">
                                                <strong>{provider.display_name.clone()}</strong>
                                                <span>{provider.provider_id.clone()}</span>
                                                <span>{provider.base_url.clone()}</span>
                                            </div>
                                            <div class="provider-actions">
                                                <button class="secondary-button compact" type="button" on:click=move |_| {
                                                    set_selected_provider_id.set(Some(edit_provider.provider_id.clone()));
                                                    set_editing_provider_id.set(Some(edit_provider.provider_id.clone()));
                                                    set_provider_name.set(edit_provider.display_name.clone());
                                                    set_base_url.set(edit_provider.base_url.clone());
                                                    set_api_format.set(api_format_value(&edit_provider.api_format));
                                                    set_auth_scheme.set(auth_scheme_value(&edit_provider.auth_scheme));
                                                    set_api_key.set(String::new());
                                                }>"Edit"</button>
                                                <button class="secondary-button compact" type="button" on:click=move |_| {
                                                    let provider_id = active_provider.provider_id.clone();
                                                    set_selected_provider_id.set(Some(provider_id.clone()));
                                                    set_result.set(format!("设置 active Provider 中: {provider_id}"));
                                                    spawn_local(async move {
                                                        match set_active_provider_and_sync(
                                                            provider_id,
                                                            set_active_provider_id,
                                                            set_selected_provider_id,
                                                            set_providers,
                                                            set_readiness_snapshot,
                                                            set_restart_required,
                                                        ).await {
                                                            Ok(message) => set_result.set(message),
                                                            Err(message) => set_result.set(message),
                                                        }
                                                    });
                                                }>"设为默认"</button>
                                            </div>
                                        </div>
                                    }
                                }
                            />
                        </div>
                    </article>

                    <aside class="panel preset-panel quick-preset-panel">
                        <h2>"快捷预设"</h2>
                        <p class="preset-help">"选择后会自动填入 API 地址和推荐模型，API Key 仍由你自己填写。"</p>
                        <div class="preset-list">
                            <button class=move || preset_item_class(selected_preset_id.get() == "deepseek") type="button" on:click=move |_| {
                                set_editing_provider_id.set(None);
                                set_selected_preset_id.set("deepseek".to_owned());
                                set_provider_name.set("DeepSeek".to_owned());
                                set_base_url.set("https://api.deepseek.com/anthropic".to_owned());
                                set_api_format.set("anthropic".to_owned());
                                set_auth_scheme.set("bearer".to_owned());
                                set_api_key.set(String::new());
                            }>
                                <span class="preset-logo"><img src="deepseek.ico" alt="" /></span>
                                <span><strong>"DeepSeek"</strong><span>"https://api.deepseek.com/anthropic"</span></span>
                                <span class=move || if selected_preset_id.get() == "deepseek" { "icon-svg icon-check" } else { "icon-svg icon-chevron-right" } aria-hidden="true"></span>
                            </button>
                            <button class=move || preset_item_class(selected_preset_id.get() == "kimi") type="button" on:click=move |_| {
                                set_editing_provider_id.set(None);
                                set_selected_preset_id.set("kimi".to_owned());
                                set_provider_name.set("Kimi（月之暗面）".to_owned());
                                set_base_url.set("https://api.moonshot.cn/v1".to_owned());
                                set_api_format.set("openai_chat".to_owned());
                                set_auth_scheme.set("bearer".to_owned());
                                set_api_key.set(String::new());
                            }>
                                <span class="preset-logo"><img src="kimi.ico" alt="" /></span>
                                <span><strong>"Kimi（月之暗面）"</strong><span>"https://api.moonshot.cn/v1"</span></span>
                                <span class=move || if selected_preset_id.get() == "kimi" { "icon-svg icon-check" } else { "icon-svg icon-chevron-right" } aria-hidden="true"></span>
                            </button>
                            <button class=move || preset_item_class(selected_preset_id.get() == "qiniu") type="button" on:click=move |_| {
                                set_editing_provider_id.set(None);
                                set_selected_preset_id.set("qiniu".to_owned());
                                set_provider_name.set("七牛云 AI".to_owned());
                                set_base_url.set("https://api.qnaigc.com/v1".to_owned());
                                set_api_format.set("openai_chat".to_owned());
                                set_auth_scheme.set("bearer".to_owned());
                                set_api_key.set(String::new());
                            }>
                                <span class="preset-logo"><img src="qiniu.ico" alt="" /></span>
                                <span><strong>"七牛云 AI"</strong><span>"https://api.qnaigc.com/v1"</span></span>
                                <span class=move || if selected_preset_id.get() == "qiniu" { "icon-svg icon-check" } else { "icon-svg icon-chevron-right" } aria-hidden="true"></span>
                            </button>
                            <button class=move || preset_item_class(selected_preset_id.get() == "zhipu") type="button" on:click=move |_| {
                                set_editing_provider_id.set(None);
                                set_selected_preset_id.set("zhipu".to_owned());
                                set_provider_name.set("智谱 GLM".to_owned());
                                set_base_url.set("https://open.bigmodel.cn/api/paas/v4/".to_owned());
                                set_api_format.set("openai_chat".to_owned());
                                set_auth_scheme.set("bearer".to_owned());
                                set_api_key.set(String::new());
                            }>
                                <span class="preset-logo"><img src="zhipu.png" alt="" /></span>
                                <span><strong>"智谱 GLM"</strong><span>"https://open.bigmodel.cn/api/paas/v4/"</span></span>
                                <span class=move || if selected_preset_id.get() == "zhipu" { "icon-svg icon-check" } else { "icon-svg icon-chevron-right" } aria-hidden="true"></span>
                            </button>
                        </div>
                        <div class="preset-import-box hidden" aria-hidden="true">
                            <p class="preset-count">{move || format!("Loaded presets: {}", provider_presets.get().len())}</p>
                            <label class="field">
                                <span>"内置 preset API Key"</span>
                                <input
                                    type="password"
                                    placeholder="preset API key"
                                    value=move || preset_api_key.get()
                                    on:input=move |event| set_preset_api_key.set(event_target_value(&event))
                                />
                            </label>
                            <div class="button-row">
                                <button class="secondary-button compact" type="button" on:click=load_provider_presets>"Load presets"</button>
                                <button class="secondary-button compact" type="button" on:click=preview_preset_import>"Preview"</button>
                                <button class="primary-button compact" type="button" on:click=move |_| import_preset(false)>"Import"</button>
                            </div>
                        </div>
                    </aside>
                    </div>

                    <div class="advanced-provider-grid">
                    <article class="panel">
                        <h2>"Provider Import / Export"</h2>
                        <div class="button-row">
                            <button class="secondary-button" type="button" on:click=export_providers>"Export"</button>
                            <button class="secondary-button" type="button" on:click=save_provider_export_as>"Save as"</button>
                            <button class="secondary-button" type="button" on:click=load_provider_template_example>"Template example"</button>
                            <button class="secondary-button" type="button" on:click=move |_| preview_import(false, false)>"Preview"</button>
                            <button class="secondary-button" type="button" on:click=move |_| preview_import(false, true)>"Preview new only"</button>
                            <button class="secondary-button" type="button" on:click=move |_| apply_import(false, false)>"Import"</button>
                            <button class="secondary-button" type="button" on:click=move |_| apply_import(false, true)>"Import new only"</button>
                            <button class="secondary-button" type="button" on:click=move |_| apply_import(true, false)>"Replace"</button>
                        </div>
                        <div class="merge-box" aria-live="polite">{move || import_preview_text.get()}</div>
                        <textarea
                            class="json-input"
                            prop:value=move || import_json.get()
                            on:input=move |event| set_import_json.set(event_target_value(&event))
                        ></textarea>
                    </article>

                    <article class="panel">
                        <h2>{move || copy("model_mapping")}</h2>
                        <div class="button-row">
                            <button
                                class="secondary-button"
                                type="button"
                                disabled=move || editing_provider_id.get().is_none()
                                on:click=load_model_mappings
                            >"Load mappings"</button>
                            <button
                                class="primary-button"
                                type="button"
                                disabled=move || editing_provider_id.get().is_none()
                                on:click=save_model_mappings
                            >"Save mappings"</button>
                        </div>
                        <textarea
                            class="json-input mapping-input"
                            prop:value=move || model_mapping_json.get()
                            on:input=move |event| set_model_mapping_json.set(event_target_value(&event))
                        ></textarea>
                    </article>

                    <article class="panel">
                        <h2>"Config Backups"</h2>
                        <div class="backup-controls">
                            <input
                                placeholder="backup file name"
                                value=move || backup_file_name.get()
                                on:input=move |event| set_backup_file_name.set(event_target_value(&event))
                            />
                            <button class="secondary-button" type="button" on:click=list_config_backups>"List backups"</button>
                            <button class="secondary-button" type="button" on:click=read_selected_backup>"Read redacted"</button>
                        </div>
                    </article>
                    </div>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::Providers)>
                    <div class="page-title with-action">
                        <div>
                            <h1>"提供商"</h1>
                            <p>"管理已配置的 API 提供商。"</p>
                        </div>
                        <button class="primary-button" type="button" on:click=move |_| {
                            reset_provider_form(
                                set_editing_provider_id,
                                set_selected_preset_id,
                                set_provider_name,
                                set_base_url,
                                set_api_key,
                                set_api_format,
                                set_auth_scheme,
                            );
                            set_active_page.set(AppPage::ProvidersAdd);
                        }>"添加提供商"</button>
                    </div>
                    <article class="panel table-panel">
                        <div class="provider-table-header">
                            <span></span>
                            <span>"提供商名称"</span>
                            <span>"API Base URL"</span>
                            <span>"模型映射"</span>
                            <span>"状态"</span>
                            <span>"操作"</span>
                        </div>
                        <div class="provider-card-list">
                            <For
                                each=move || providers.get()
                                key=|provider| provider.provider_id.clone()
                                children=move |provider| {
                                    let edit_provider = provider.clone();
                                    let select_provider = provider.clone();
                                    let active_provider = provider.clone();
                                    let copy_provider = provider.clone();
                                    let delete_provider = provider.clone();
                                    let provider_id_for_class = provider.provider_id.clone();
                                    let provider_id_for_button_class = provider.provider_id.clone();
                                    let provider_id_for_button_label = provider.provider_id.clone();
                                    let provider_id_for_disabled = provider.provider_id.clone();
                                    let logo_src = provider_logo_src(&provider.display_name);
                                    view! {
                                        <article class=move || provider_switch_card_class(active_provider_id.get().as_deref() == Some(provider_id_for_class.as_str()))>
                                            <span class="drag-handle"><span class="icon-svg icon-grip" aria-hidden="true"></span></span>
                                            <span class="provider-logo"><img src=logo_src alt="" /></span>
                                            <span class="provider-main">
                                                <strong>{provider.display_name.clone()}</strong>
                                                <span>{provider.base_url.clone()}</span>
                                            </span>
                                            <span class="provider-feedback">
                                                <span class="speed-result inline">{format!("key={}", provider.has_api_key)}</span>
                                            </span>
                                            <span class="provider-actions">
                                                <button
                                                    class=move || compact_enable_class(active_provider_id.get().as_deref() == Some(provider_id_for_button_class.as_str()))
                                                    type="button"
                                                    disabled=move || active_provider_id.get().as_deref() == Some(provider_id_for_disabled.as_str())
                                                    on:click=move |_| {
                                                        let provider_id = select_provider.provider_id.clone();
                                                        set_selected_provider_id.set(Some(provider_id.clone()));
                                                        set_result.set(format!("设置 active Provider 中: {provider_id}"));
                                                        spawn_local(async move {
                                                            match set_active_provider_and_sync(
                                                                provider_id,
                                                                set_active_provider_id,
                                                                set_selected_provider_id,
                                                                set_providers,
                                                                set_readiness_snapshot,
                                                                set_restart_required,
                                                            ).await {
                                                                Ok(message) => set_result.set(message),
                                                                Err(message) => set_result.set(message),
                                                            }
                                                        });
                                                    }
                                                >
                                                    <span class="icon-svg icon-play" aria-hidden="true"></span>
                                                    <span>{move || if active_provider_id.get().as_deref() == Some(provider_id_for_button_label.as_str()) { "默认" } else { "启用" }}</span>
                                                </button>
                                                <button class="icon-action" type="button" title="Set active" on:click=move |_| {
                                                    let provider_id = active_provider.provider_id.clone();
                                                    set_selected_provider_id.set(Some(provider_id.clone()));
                                                    set_result.set(format!("设置 active Provider 中: {provider_id}"));
                                                    spawn_local(async move {
                                                        match set_active_provider_and_sync(
                                                            provider_id,
                                                            set_active_provider_id,
                                                            set_selected_provider_id,
                                                            set_providers,
                                                            set_readiness_snapshot,
                                                            set_restart_required,
                                                        ).await {
                                                            Ok(message) => set_result.set(message),
                                                            Err(message) => set_result.set(message),
                                                        }
                                                    });
                                                }>
                                                    <span class="icon-svg icon-lightning" aria-hidden="true"></span>
                                                </button>
                                                <button class="icon-action" type="button" title="Edit" on:click=move |_| {
                                                    set_selected_provider_id.set(Some(edit_provider.provider_id.clone()));
                                                    set_editing_provider_id.set(Some(edit_provider.provider_id.clone()));
                                                    set_provider_name.set(edit_provider.display_name.clone());
                                                    set_base_url.set(edit_provider.base_url.clone());
                                                    set_api_format.set(api_format_value(&edit_provider.api_format));
                                                    set_auth_scheme.set(auth_scheme_value(&edit_provider.auth_scheme));
                                                    set_api_key.set(String::new());
                                                    set_active_page.set(AppPage::ProvidersAdd);
                                                }>
                                                    <span class="icon-svg icon-edit" aria-hidden="true"></span>
                                                </button>
                                                <button class="icon-action" type="button" title="Copy URL" on:click=move |_| {
                                                    let provider_id = copy_provider.provider_id.clone();
                                                    let url = copy_provider.base_url.clone();
                                                    set_result.set(format!("copy provider url: {provider_id}"));
                                                    spawn_local(async move {
                                                        match commands::copy_text_to_clipboard(url).await {
                                                            Ok(_) => set_result.set(format!("copied provider url: {provider_id}")),
                                                            Err(error) => set_result.set(format!("copy provider url failed: {error}")),
                                                        }
                                                    });
                                                }>
                                                    <span class="icon-svg icon-copy" aria-hidden="true"></span>
                                                </button>
                                                <button class="icon-action" type="button" title="Forwarding" on:click=move |_| set_active_page.set(AppPage::Proxy)>
                                                    <span class="icon-svg icon-terminal" aria-hidden="true"></span>
                                                </button>
                                                <button class="icon-action danger-icon" type="button" title="Delete" on:click=move |_| {
                                                    let provider_id = delete_provider.provider_id.clone();
                                                    set_result.set(format!("删除 Provider 中: {provider_id}"));
                                                    spawn_local(async move {
                                                        match commands::delete_provider(provider_id.clone()).await {
                                                            Ok(changed) => match commands::get_config_snapshot().await {
                                                                Ok(snapshot) => {
                                                                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                                                                    set_active_provider_id.set(snapshot.active_provider.clone());
                                                                    set_selected_provider_id.set(snapshot.active_provider.clone().or_else(|| snapshot.providers.first().map(|provider| provider.provider_id.clone())));
                                                                    let next_providers = snapshot.providers;
                                                                    set_providers.set(next_providers.clone());
                                                                    set_result.set(format!("delete_provider changed={changed}\n{}", format_provider_list(&next_providers)));
                                                                }
                                                                Err(error) => {
                                                                    mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                                                                    set_result.set(format!("delete_provider changed={changed}\nget_config_snapshot failed: {error}"));
                                                                }
                                                            },
                                                            Err(error) => {
                                                                mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                                                                let refresh_note = refresh_provider_state_from_backend(
                                                                    set_active_provider_id,
                                                                    set_selected_provider_id,
                                                                    set_providers,
                                                                )
                                                                .await;
                                                                set_result.set(format_backend_mutation_error(
                                                                    "delete_provider",
                                                                    &error,
                                                                    refresh_note,
                                                                ));
                                                            }
                                                        }
                                                    });
                                                }>
                                                    <span class="icon-svg icon-trash" aria-hidden="true"></span>
                                                </button>
                                            </span>
                                        </article>
                                    }
                                }
                            />
                        </div>
                        <div class="button-row provider-list-toolbar">
                            <button class="secondary-button compact" type="button" on:click=refresh_providers><span class="icon-svg icon-refresh" aria-hidden="true"></span><span>"刷新"</span></button>
                        </div>
                    </article>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::Desktop)>
                    <div class="page-title title-with-back">
                        <button class="square-button" type="button" aria-label="back" on:click=move |_| set_active_page.set(AppPage::Dashboard)>"←"</button>
                        <div>
                            <h1>"Claude 桌面版"</h1>
                            <p>"一键让 Claude 桌面版使用当前供应商。"</p>
                        </div>
                    </div>
                    <div class="two-column desktop-layout">
                        <div>
                            <article class="panel desktop-card">
                                <h2>"Claude 桌面版配置"</h2>
                                <div class=move || configured_row_class(readiness_snapshot.get().as_ref(), provider_saved.get())>
                                    <span class="circle-check">{move || desktop_status_mark(readiness_snapshot.get().as_ref(), provider_saved.get())}</span>
                                    <strong>{move || desktop_status_value(readiness_snapshot.get().as_ref(), provider_saved.get())}</strong>
                                </div>
                                <div class="apply-explain desktop-explain">
                                    <span>"i"</span>
                                    <p>"Claude Desktop 先连接本机 gateway；本工具再把 claude-* safe route 翻译成当前供应商模型。"</p>
                                </div>
                                <button class="primary-button wide-button" type="button" on:click=apply>"应用到 Claude 桌面版"</button>
                                <button class="secondary-button wide-button" type="button" on:click=probe_desktop_config>"读取配置详情"</button>
                                <button class="secondary-button wide-button" type="button" on:click=restart_claude_desktop>"重启 Claude Desktop"</button>
                                <button class="danger-button wide-button" type="button" on:click=clear_desktop_config>"清除桌面版配置"</button>
                                <div class=move || restart_reminder_class(restart_required.get())>
                                    <strong>"需要重启 Claude Desktop"</strong>
                                    <p>"Claude Desktop 配置已变更。重启后才会读取最新 claude-* 模型列表、gateway 地址或清理结果。"</p>
                                </div>
                            </article>
                            <article class="panel details-panel">
                                <div class="panel-header">
                                    <h2>"配置详情"</h2>
                                </div>
                                <pre class="code-block">{move || result.get()}</pre>
                            </article>
                        </div>

                        <article class="panel guide-panel">
                            <h2>"快速引导"</h2>
                            <div class="mini-steps">
                                <div class="mini-step"><span>"1"</span><strong>"点击应用"</strong><p>"把 Claude Desktop 连接到本工具。"</p></div>
                                <div class="mini-step"><span>"2"</span><strong>"重启 Claude Desktop"</strong><p>"关闭并重新打开桌面版。"</p></div>
                                <div class="mini-step"><span>"3"</span><strong>"开始使用"</strong><p>"在 Claude Desktop 里正常提问。"</p></div>
                            </div>
                        </article>
                    </div>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::Proxy)>
                    <article class="proxy-status-card">
                        <div class="proxy-status-left">
                            <span class=move || proxy_status_dot_class(proxy_status.get().as_ref(), readiness_snapshot.get().as_ref())></span>
                            <strong class=move || proxy_status_text_class(proxy_status.get().as_ref(), readiness_snapshot.get().as_ref())>{move || proxy_status_label(proxy_status.get().as_ref(), readiness_snapshot.get().as_ref())}</strong>
                            <span>"本机监听"</span>
                        </div>
                        <div class="proxy-status-controls">
                            <label>"转发端口"</label>
                            <input prop:value=move || proxy_port.get() on:input=move |event| set_proxy_port.set(event_target_value(&event)) />
                            <button class="secondary-button" type="button" on:click=start_gateway><span class="icon-svg icon-play" aria-hidden="true"></span><span>"启动转发"</span></button>
                            <button class="danger-button" type="button" on:click=stop_gateway><span>"■"</span><span>"停止转发"</span></button>
                        </div>
                    </article>

                    <article class="proxy-log-panel">
                        <div class="proxy-toolbar">
                            <button class="dark-toolbar-button" type="button" on:click=clear_proxy_logs><span class="icon-svg icon-trash" aria-hidden="true"></span><span>"清除日志"</span></button>
                            <button class="dark-toolbar-button" type="button" on:click=read_proxy_logs><span class="icon-svg icon-refresh" aria-hidden="true"></span><span>"刷新日志"</span></button>
                            <button class="dark-toolbar-button" type="button" on:click=check_health><span>"⌁"</span><span>"运行诊断"</span></button>
                            <button class="dark-toolbar-button" type="button" on:click=dry_run><span class="icon-svg icon-magic" aria-hidden="true"></span><span>"Dry-run"</span></button>
                            <button class="dark-toolbar-button" type="button" on:click=export_diagnostics_package><span class="icon-svg icon-info" aria-hidden="true"></span><span>"导出诊断包"</span></button>
                            <label class="auto-scroll-toggle"><span>"自动滚动"</span><input type="checkbox" prop:checked=move || auto_scroll_logs.get() on:change=move |event| set_auto_scroll_logs.set(event_target_checked(&event)) /></label>
                        </div>
                        <div class="proxy-log-body" node_ref=proxy_log_body_ref>
                            <div class=move || log_empty_class(proxy_logs.get().is_empty())>{move || proxy_logs_text.get()}</div>
                            <For
                                each=move || proxy_log_entries(proxy_logs.get())
                                key=|entry| entry.key.clone()
                                children=move |entry| {
                                    let level_class = log_level_class(&entry.level);
                                    view! {
                                        <div class="log-line">
                                            <span class="log-time">{entry.timestamp}</span>
                                            <span class=level_class>{entry.level}</span>
                                            <span class="log-code">{entry.code}</span>
                                            <span class="log-message">{entry.message}</span>
                                        </div>
                                    }
                                }
                            />
                        </div>
                    </article>

                    <div class="proxy-metrics-grid">
                        <article class="metric-card"><span class="metric-icon icon-svg icon-sliders" aria-hidden="true"></span><strong>"总请求"</strong><b>{move || proxy_stat_total(proxy_status.get().as_ref()).to_string()}</b></article>
                        <article class="metric-card"><span class="metric-icon icon-svg icon-check" aria-hidden="true"></span><strong>"成功"</strong><b>{move || proxy_stat_success(proxy_status.get().as_ref()).to_string()}</b></article>
                        <article class="metric-card"><span class="metric-icon">"×"</span><strong>"失败"</strong><b>{move || proxy_stat_failed(proxy_status.get().as_ref()).to_string()}</b></article>
                        <article class="metric-card"><span class="metric-icon icon-svg icon-dashboard" aria-hidden="true"></span><strong>"今日"</strong><b>{move || proxy_stat_today(proxy_status.get().as_ref()).to_string()}</b></article>
                    </div>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::Settings)>
                    <article class="settings-panel">
                        <div class="settings-row">
                            <label>"主题"</label>
                            <div class="theme-swatches">
                                <button class=move || theme_swatch_class(theme.get() == "light", "blue") type="button" on:click=move |_| set_theme.set("light".to_owned())><span></span></button>
                                <button class=move || theme_swatch_class(theme.get() == "green", "green") type="button" on:click=move |_| set_theme.set("green".to_owned())><span></span></button>
                                <button class=move || theme_swatch_class(theme.get() == "orange", "orange") type="button" on:click=move |_| set_theme.set("orange".to_owned())><span></span></button>
                                <button class=move || theme_swatch_class(theme.get() == "slate", "slate") type="button" on:click=move |_| set_theme.set("slate".to_owned())><span></span></button>
                                <button class=move || theme_swatch_class(theme.get() == "dark", "dark") type="button" on:click=move |_| set_theme.set("dark".to_owned())><span></span></button>
                                <button class=move || theme_swatch_class(theme.get() == "white", "white") type="button" on:click=move |_| set_theme.set("white".to_owned())><span></span></button>
                            </div>
                        </div>
                        <div class="settings-row">
                            <label>"语言"</label>
                            <div class="language-segmented">
                                <button class=move || segmented_button_class(language.get() == "zh") type="button" on:click=move |_| set_language.set("zh".to_owned())>"中文"</button>
                                <button class=move || segmented_button_class(language.get() == "en") type="button" on:click=move |_| set_language.set("en".to_owned())>"EN"</button>
                            </div>
                        </div>
                        <div class="settings-row">
                            <label>"转发端口"</label>
                            <input prop:value=move || proxy_port.get() on:input=move |event| set_proxy_port.set(event_target_value(&event)) />
                        </div>
                        <div class="settings-row">
                            <label>"更新地址"</label>
                            <div>
                                <input prop:value=move || update_url.get() on:input=move |event| {
                                    set_update_url.set(event_target_value(&event));
                                    set_update_download_path.set(None);
                                } />
                                <div class="button-row compact-row">
                                    <button class="secondary-button compact" type="button" on:click=check_update_now><span class="icon-svg icon-search" aria-hidden="true"></span><span>"检查更新"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=download_update_now><span class="icon-svg icon-download" aria-hidden="true"></span><span>"下载并验证"</span></button>
                                    <button class="secondary-button compact" type="button" prop:disabled=move || update_download_path.get().is_none() on:click=install_update_now><span>"▣"</span><span>"启动安装器"</span></button>
                                </div>
                            </div>
                        </div>
                        <div class="settings-row">
                            <label>"第三方兼容"</label>
                            <div>
                                <button class="secondary-button compact" type="button" on:click=run_gateway_smoke><span class="icon-svg icon-info" aria-hidden="true"></span><span>"检查兼容性"</span></button>
                                <p>"Anthropic 兼容接口是稳定主线；OpenAI Chat/new-api 会做工具调用转换，具体服务仍需用诊断验证。"</p>
                            </div>
                        </div>
                        <div class="settings-row">
                            <label>"诊断与支持"</label>
                            <div>
                                <div class="button-row">
                                    <button class="secondary-button compact" type="button" on:click=run_provider_real_smoke><span>"⌁"</span><span>"运行诊断"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=copy_diagnostics_summary><span>"▣"</span><span>"诊断摘要"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=save_diagnostics_package><span class="icon-svg icon-info" aria-hidden="true"></span><span>"导出诊断包"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=save_diagnostics_package_as><span class="icon-svg icon-download" aria-hidden="true"></span><span>"另存诊断包"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=copy_diagnostics_to_clipboard><span>"▣"</span><span>"复制诊断摘要"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=preview_issue_draft><span class="icon-svg icon-edit" aria-hidden="true"></span><span>"Issue 草稿"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=open_issue><span class="icon-svg icon-terminal" aria-hidden="true"></span><span>"打开 Issue"</span></button>
                                </div>
                                <p>"诊断包会自动脱敏 API Key、gateway key、Authorization 和 token，可用于提交 issue。"</p>
                                <pre class="settings-result">{move || diagnostics_text.get()}</pre>
                            </div>
                        </div>
                        <div class="settings-row">
                            <label>"配置备份"</label>
                            <div>
                                <div class="button-row">
                                    <button class="secondary-button compact" type="button" on:click=create_backup_now><span class="icon-svg icon-info" aria-hidden="true"></span><span>"立即备份"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=list_config_backups><span class="icon-svg icon-refresh" aria-hidden="true"></span><span>"备份列表"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=save_provider_export_as><span class="icon-svg icon-download" aria-hidden="true"></span><span>"导出配置"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=move |_| preview_import(false, true)><span class="icon-svg icon-import" aria-hidden="true"></span><span>"预览导入"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=move |_| apply_import(false, true)><span class="icon-svg icon-import" aria-hidden="true"></span><span>"导入新增项"</span></button>
                                    <button class="secondary-button compact" type="button" on:click=save_settings><span class="icon-svg icon-save" aria-hidden="true"></span><span>"保存设置"</span></button>
                                </div>
                                <textarea class="json-input settings-import-json" placeholder="粘贴 CC Switch / CC Desktop Switch 配置 JSON" prop:value=move || import_json.get() on:input=move |event| set_import_json.set(event_target_value(&event))></textarea>
                                <pre class="settings-result">{move || import_preview_text.get()}</pre>
                            </div>
                        </div>
                    </article>
                </section>

                <section class=move || page_section_class(active_page.get(), AppPage::Guide)>
                    <div class="guide-hero">
                        <img src="app-icon.png" alt="CC Desktop Switch icon" />
                        <div>
                            <h1>"使用引导"</h1>
                            <p>"添加供应商后，按下面 3 步完成接入。"</p>
                        </div>
                    </div>
                    <div class="timeline">
                        <article class="timeline-card"><span>"1"</span><strong>"添加提供商"</strong><p>"选择预设，填入 API Key，必要时调整模型映射。"</p></article>
                        <article class="timeline-card"><span>"2"</span><strong>"一键应用"</strong><p>"保存 Provider、设为默认、启动 gateway 并写入 Claude Desktop。"</p></article>
                        <article class="timeline-card"><span>"3"</span><strong>"重启 Claude Desktop"</strong><p>"重启后 Claude Desktop 只看到 claude-* safe route。"</p></article>
                    </div>
                    <button class="primary-button guide-start" type="button" on:click=move |_| {
                        reset_provider_form(
                            set_editing_provider_id,
                            set_selected_preset_id,
                            set_provider_name,
                            set_base_url,
                            set_api_key,
                            set_api_format,
                            set_auth_scheme,
                        );
                        set_active_page.set(AppPage::ProvidersAdd);
                    }>"开始使用"</button>
                </section>
            </main>
        </div>
    }
}

fn api_format_value(api_format: &commands::ApiFormat) -> String {
    match api_format {
        commands::ApiFormat::OpenAiChat => "openai_chat".to_owned(),
        commands::ApiFormat::Anthropic => "anthropic".to_owned(),
    }
}

fn auth_scheme_from_value(value: &str) -> AuthScheme {
    match value {
        "x_api_key" => AuthScheme::XApiKey,
        "none" => AuthScheme::None,
        _ => AuthScheme::Bearer,
    }
}

fn auth_scheme_value(auth_scheme: &commands::AuthScheme) -> String {
    match auth_scheme {
        commands::AuthScheme::XApiKey => "x_api_key".to_owned(),
        commands::AuthScheme::None => "none".to_owned(),
        commands::AuthScheme::Bearer => "bearer".to_owned(),
    }
}

fn format_gateway_health(health: &commands::GatewayHealth) -> String {
    format!(
        "{} running={} auth=required(Bearer/x-api-key)",
        health.base_url, health.running
    )
}

fn is_supported_theme(value: &str) -> bool {
    matches!(
        value,
        "light" | "green" | "orange" | "slate" | "dark" | "white"
    )
}

#[allow(clippy::too_many_arguments)]
fn reset_provider_form(
    set_editing_provider_id: WriteSignal<Option<String>>,
    set_selected_preset_id: WriteSignal<String>,
    set_provider_name: WriteSignal<String>,
    set_base_url: WriteSignal<String>,
    set_api_key: WriteSignal<String>,
    set_api_format: WriteSignal<String>,
    set_auth_scheme: WriteSignal<String>,
) {
    set_editing_provider_id.set(None);
    set_selected_preset_id.set(String::new());
    set_provider_name.set(String::new());
    set_base_url.set(String::new());
    set_api_key.set(String::new());
    set_api_format.set("anthropic".to_owned());
    set_auth_scheme.set("bearer".to_owned());
}

fn mark_desktop_readiness_stale(
    set_readiness_snapshot: WriteSignal<Option<commands::ReadinessSnapshot>>,
    set_restart_required: WriteSignal<bool>,
) {
    set_readiness_snapshot.set(None);
    set_restart_required.set(false);
}

async fn refresh_provider_state_from_backend(
    set_active_provider_id: WriteSignal<Option<String>>,
    set_selected_provider_id: WriteSignal<Option<String>>,
    set_providers: WriteSignal<Vec<ProviderSummary>>,
) -> Result<(), String> {
    refresh_provider_state_from_backend_preserving_selection(
        set_active_provider_id,
        set_selected_provider_id,
        set_providers,
        None,
    )
    .await
}

async fn refresh_provider_state_from_backend_preserving_selection(
    set_active_provider_id: WriteSignal<Option<String>>,
    set_selected_provider_id: WriteSignal<Option<String>>,
    set_providers: WriteSignal<Vec<ProviderSummary>>,
    preferred_selected_provider_id: Option<String>,
) -> Result<(), String> {
    match commands::get_config_snapshot().await {
        Ok(snapshot) => {
            let preferred_exists =
                preferred_selected_provider_id
                    .as_ref()
                    .is_some_and(|provider_id| {
                        snapshot
                            .providers
                            .iter()
                            .any(|provider| provider.provider_id == *provider_id)
                    });
            let selected_provider = if preferred_exists {
                preferred_selected_provider_id
            } else {
                snapshot.active_provider.clone().or_else(|| {
                    snapshot
                        .providers
                        .first()
                        .map(|provider| provider.provider_id.clone())
                })
            };
            set_active_provider_id.set(snapshot.active_provider);
            set_selected_provider_id.set(selected_provider);
            set_providers.set(snapshot.providers);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn set_active_provider_and_sync(
    provider_id: String,
    set_active_provider_id: WriteSignal<Option<String>>,
    set_selected_provider_id: WriteSignal<Option<String>>,
    set_providers: WriteSignal<Vec<ProviderSummary>>,
    set_readiness_snapshot: WriteSignal<Option<commands::ReadinessSnapshot>>,
    set_restart_required: WriteSignal<bool>,
) -> Result<String, String> {
    match commands::set_active_provider(provider_id.clone()).await {
        Ok(true) => {
            mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
            set_active_provider_id.set(Some(provider_id.clone()));
            Ok(format!(
                "set_active_provider changed=true\nactiveProvider={provider_id}"
            ))
        }
        Ok(false) => {
            mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
            let refresh_note = refresh_provider_state_from_backend(
                set_active_provider_id,
                set_selected_provider_id,
                set_providers,
            )
            .await;
            Err(format_backend_mutation_error(
                "set_active_provider",
                &format!("provider not found: {provider_id}"),
                refresh_note,
            ))
        }
        Err(error) => {
            mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
            let refresh_note = refresh_provider_state_from_backend(
                set_active_provider_id,
                set_selected_provider_id,
                set_providers,
            )
            .await;
            Err(format_backend_mutation_error(
                "set_active_provider",
                &error,
                refresh_note,
            ))
        }
    }
}

async fn refresh_settings_state_from_backend(
    set_language: WriteSignal<String>,
    set_theme: WriteSignal<String>,
    set_proxy_port: WriteSignal<String>,
    set_update_url: WriteSignal<String>,
    set_proxy_status: WriteSignal<Option<commands::ProxyStatus>>,
) -> Result<(), String> {
    match commands::get_settings().await {
        Ok(settings) => {
            if settings.language == "zh" || settings.language == "en" {
                set_language.set(settings.language);
            }
            if is_supported_theme(&settings.theme) {
                set_theme.set(settings.theme);
            }
            set_proxy_port.set(settings.proxy_port.to_string());
            set_update_url.set(settings.update_url);
            if let Ok(status) = commands::get_proxy_status().await {
                set_proxy_status.set(Some(status));
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

async fn refresh_model_mappings_from_backend(
    provider_id: String,
    set_mapping_default: WriteSignal<String>,
    set_mapping_opus: WriteSignal<String>,
    set_mapping_sonnet: WriteSignal<String>,
    set_mapping_haiku: WriteSignal<String>,
    set_model_mapping_json: WriteSignal<String>,
) -> Result<(), String> {
    match commands::list_model_mappings(provider_id).await {
        Ok(mappings) => {
            for mapping in &mappings {
                match &mapping.slot {
                    ModelSlot::Default => set_mapping_default.set(mapping.upstream_model.clone()),
                    ModelSlot::Opus => set_mapping_opus.set(mapping.upstream_model.clone()),
                    ModelSlot::Sonnet => set_mapping_sonnet.set(mapping.upstream_model.clone()),
                    ModelSlot::Haiku => set_mapping_haiku.set(mapping.upstream_model.clone()),
                }
            }
            match serde_json::to_string_pretty(&mappings) {
                Ok(raw) => {
                    set_model_mapping_json.set(raw);
                    Ok(())
                }
                Err(error) => Err(format!("format mappings failed: {error}")),
            }
        }
        Err(error) => Err(error),
    }
}

async fn load_model_mappings_for_provider(
    provider_id: String,
    set_mapping_default: WriteSignal<String>,
    set_mapping_opus: WriteSignal<String>,
    set_mapping_sonnet: WriteSignal<String>,
    set_mapping_haiku: WriteSignal<String>,
    set_model_mapping_json: WriteSignal<String>,
    set_result: WriteSignal<String>,
) {
    match commands::list_model_mappings(provider_id).await {
        Ok(mappings) => match serde_json::to_string_pretty(&mappings) {
            Ok(raw) => {
                for mapping in &mappings {
                    match &mapping.slot {
                        ModelSlot::Default => {
                            set_mapping_default.set(mapping.upstream_model.clone())
                        }
                        ModelSlot::Opus => set_mapping_opus.set(mapping.upstream_model.clone()),
                        ModelSlot::Sonnet => set_mapping_sonnet.set(mapping.upstream_model.clone()),
                        ModelSlot::Haiku => set_mapping_haiku.set(mapping.upstream_model.clone()),
                    }
                }
                set_model_mapping_json.set(raw.clone());
                set_result.set(raw);
            }
            Err(error) => set_result.set(format!("format mappings failed: {error}")),
        },
        Err(error) => set_result.set(format!("list_model_mappings failed: {error}")),
    }
}

#[allow(clippy::too_many_arguments)]
async fn save_model_mappings_for_provider(
    provider_id: String,
    provider_id_for_refresh: String,
    mappings: Vec<ModelMappingDraft>,
    set_active_provider_id: WriteSignal<Option<String>>,
    set_selected_provider_id: WriteSignal<Option<String>>,
    set_providers: WriteSignal<Vec<ProviderSummary>>,
    set_mapping_default: WriteSignal<String>,
    set_mapping_opus: WriteSignal<String>,
    set_mapping_sonnet: WriteSignal<String>,
    set_mapping_haiku: WriteSignal<String>,
    set_model_mapping_json: WriteSignal<String>,
    set_readiness_snapshot: WriteSignal<Option<commands::ReadinessSnapshot>>,
    set_restart_required: WriteSignal<bool>,
    set_result: WriteSignal<String>,
) {
    match commands::update_model_mappings(provider_id, mappings).await {
        Ok(saved) => match serde_json::to_string_pretty(&saved) {
            Ok(raw) => {
                mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
                set_model_mapping_json.set(raw.clone());
                set_result.set(format!("update_model_mappings ok\n{raw}"));
            }
            Err(error) => set_result.set(format!("format mappings failed: {error}")),
        },
        Err(error) => {
            mark_desktop_readiness_stale(set_readiness_snapshot, set_restart_required);
            let provider_refresh = refresh_provider_state_from_backend_preserving_selection(
                set_active_provider_id,
                set_selected_provider_id,
                set_providers,
                Some(provider_id_for_refresh.clone()),
            )
            .await;
            let mapping_refresh = refresh_model_mappings_from_backend(
                provider_id_for_refresh,
                set_mapping_default,
                set_mapping_opus,
                set_mapping_sonnet,
                set_mapping_haiku,
                set_model_mapping_json,
            )
            .await;
            let refresh_note =
                merge_refresh_results(provider_refresh, mapping_refresh, "model mappings");
            set_result.set(format_backend_mutation_error(
                "update_model_mappings",
                &error,
                refresh_note,
            ));
        }
    }
}

fn merge_refresh_results(
    primary: Result<(), String>,
    secondary: Result<(), String>,
    secondary_label: &str,
) -> Result<(), String> {
    match (primary, secondary) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(format!("{secondary_label} refresh failed: {error}")),
        (Err(first), Err(second)) => Err(format!(
            "{first}; {secondary_label} refresh failed: {second}"
        )),
    }
}

fn format_backend_mutation_error(
    action: &str,
    error: &str,
    refresh_result: Result<(), String>,
) -> String {
    let refresh_note = match refresh_result {
        Ok(()) => "backend config state refreshed after error".to_owned(),
        Err(refresh_error) => format!("backend config refresh failed after error: {refresh_error}"),
    };
    format!("{action} failed: {error}\n{refresh_note}")
}

fn parse_proxy_port(value: &str) -> Result<u16, String> {
    value
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| "proxy_port_invalid: enter a port from 1 to 65535".to_owned())
}

fn update_url_or_default(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        "https://github.com/lonr-6/cc-desktop-switch/releases/latest/download/latest.json"
            .to_owned()
    } else {
        value.to_owned()
    }
}

fn format_update_check(check: &commands::UpdateCheckResult) -> String {
    let asset = check
        .asset
        .as_ref()
        .map(|asset| format!("asset={}\nurl={}", asset.name, asset.url))
        .unwrap_or_else(|| "asset=<none>".to_owned());
    format!(
        "check_update ok\ncurrent={}\nlatest={}\navailable={}\nplatform={}\n{}",
        check.current_version, check.latest_version, check.available, check.platform, asset
    )
}

fn format_update_download(download: &commands::UpdateDownloadResult) -> String {
    format!(
        "download_update ok\nasset={}\nbytes={}\nsha256Verified={}\nsignatureVerified={}\nstagingDir={}",
        download.asset_path,
        download.bytes,
        download.sha256_verified,
        download.signature_verified,
        download.staging_dir
    )
}

fn format_proxy_logs(logs: &[commands::DiagnosticsLogEntry]) -> String {
    if logs.is_empty() {
        return "gateway logs: none".to_owned();
    }
    logs.iter()
        .rev()
        .take(80)
        .map(|entry| {
            format!(
                "{} [{}] {}: {}",
                entry.timestamp_unix_ms, entry.level, entry.code, entry.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn route_tab_class(active: bool) -> &'static str {
    if active {
        "route-tab active"
    } else {
        "route-tab"
    }
}

fn primary_route_active(active_page: AppPage, route: AppPage) -> bool {
    match route {
        AppPage::Providers => matches!(active_page, AppPage::Providers | AppPage::ProvidersAdd),
        _ => active_page == route,
    }
}

fn page_title_class(page: AppPage) -> &'static str {
    if matches!(
        page,
        AppPage::Dashboard | AppPage::Providers | AppPage::Guide
    ) {
        "page-title page-title-dashboard"
    } else {
        "page-title"
    }
}

fn format_choice_class(active: bool) -> &'static str {
    if active {
        "format-choice-button active"
    } else {
        "format-choice-button"
    }
}

fn preset_item_class(active: bool) -> &'static str {
    if active {
        "preset-item active"
    } else {
        "preset-item"
    }
}

fn provider_switch_card_class(active: bool) -> &'static str {
    if active {
        "provider-switch-card active"
    } else {
        "provider-switch-card"
    }
}

fn configured_provider_list_class(visible: bool) -> &'static str {
    if visible {
        "provider-configured-list"
    } else {
        "provider-configured-list hidden"
    }
}

fn dashboard_empty_preset_grid_class(visible: bool) -> &'static str {
    if visible {
        "provider-preset-grid dashboard-empty-state"
    } else {
        "provider-preset-grid hidden"
    }
}

fn dashboard_preset_section_class(visible: bool) -> &'static str {
    if visible {
        "dashboard-preset-section"
    } else {
        "dashboard-preset-section hidden"
    }
}

fn compact_enable_class(active: bool) -> &'static str {
    if active {
        "primary-button compact-enable"
    } else {
        "compact-enable ghost"
    }
}

fn proxy_status_label(
    status: Option<&commands::ProxyStatus>,
    snapshot: Option<&commands::ReadinessSnapshot>,
) -> &'static str {
    if proxy_is_running(status, snapshot) {
        "运行中"
    } else {
        "已停止"
    }
}

fn proxy_status_dot_class(
    status: Option<&commands::ProxyStatus>,
    snapshot: Option<&commands::ReadinessSnapshot>,
) -> &'static str {
    if proxy_is_running(status, snapshot) {
        "proxy-status-dot running"
    } else {
        "proxy-status-dot"
    }
}

fn proxy_status_text_class(
    status: Option<&commands::ProxyStatus>,
    snapshot: Option<&commands::ReadinessSnapshot>,
) -> &'static str {
    if proxy_is_running(status, snapshot) {
        "proxy-status-text running"
    } else {
        "proxy-status-text stopped"
    }
}

fn proxy_is_running(
    status: Option<&commands::ProxyStatus>,
    snapshot: Option<&commands::ReadinessSnapshot>,
) -> bool {
    status
        .map(|status| status.running)
        .or_else(|| snapshot.map(|snapshot| snapshot.gateway.running))
        .unwrap_or(false)
}

fn proxy_stat_total(status: Option<&commands::ProxyStatus>) -> u64 {
    status.map(|status| status.stats.total).unwrap_or(0)
}

fn proxy_stat_success(status: Option<&commands::ProxyStatus>) -> u64 {
    status.map(|status| status.stats.success).unwrap_or(0)
}

fn proxy_stat_failed(status: Option<&commands::ProxyStatus>) -> u64 {
    status.map(|status| status.stats.failed).unwrap_or(0)
}

fn proxy_stat_today(status: Option<&commands::ProxyStatus>) -> u64 {
    status.map(|status| status.stats.today).unwrap_or(0)
}

fn proxy_log_entries(logs: Vec<commands::DiagnosticsLogEntry>) -> Vec<ProxyLogRow> {
    logs.into_iter()
        .rev()
        .take(80)
        .enumerate()
        .map(|(index, entry)| {
            let key = if entry.id > 0 {
                format!("log-{}", entry.id)
            } else {
                format!(
                    "log-fallback-{index}-{}-{}-{}-{}",
                    entry.timestamp_unix_ms, entry.level, entry.code, entry.message
                )
            };
            ProxyLogRow {
                key,
                timestamp: entry.timestamp_unix_ms.to_string(),
                level: entry.level,
                code: entry.code,
                message: entry.message,
            }
        })
        .collect()
}

fn log_empty_class(visible: bool) -> &'static str {
    if visible {
        "log-empty"
    } else {
        "log-empty hidden"
    }
}

fn log_level_class(level: &str) -> String {
    format!("log-level {}", level.trim().to_ascii_lowercase())
}

fn segmented_button_class(active: bool) -> &'static str {
    if active {
        "segmented-button active"
    } else {
        "segmented-button"
    }
}

fn theme_swatch_class(active: bool, color: &'static str) -> String {
    if active {
        format!("theme-swatch {color} active")
    } else {
        format!("theme-swatch {color}")
    }
}

fn desktop_warning_class(snapshot: Option<&commands::ReadinessSnapshot>) -> &'static str {
    if snapshot
        .map(|snapshot| snapshot.issue_codes.is_empty())
        .unwrap_or(true)
    {
        "desktop-warning hidden"
    } else {
        "desktop-warning"
    }
}

fn provider_logo_src(name: &str) -> &'static str {
    let normalized = name.to_ascii_lowercase();
    if normalized.contains("deepseek") {
        "deepseek.ico"
    } else if normalized.contains("kimi") || name.contains("月之暗面") {
        "kimi.ico"
    } else if normalized.contains("aliyun")
        || normalized.contains("bailian")
        || name.contains("阿里")
        || name.contains("百炼")
    {
        "aliyun.ico"
    } else if normalized.contains("xiaomi") || normalized.contains("mimo") || name.contains("小米")
    {
        "xiaomi-mimo.png"
    } else if normalized.contains("qiniu") || name.contains("七牛") {
        "qiniu.ico"
    } else if normalized.contains("zhipu") || name.contains("智谱") {
        "zhipu.png"
    } else {
        "app-icon.png"
    }
}

fn language_switch_label(language: &str) -> &'static str {
    match language {
        "zh" => "中 / EN",
        "en" => "EN / 日",
        "ja" => "日 / 中",
        _ => "中 / EN",
    }
}

fn page_section_class(active_page: AppPage, page: AppPage) -> &'static str {
    if active_page == page {
        "page-section"
    } else {
        "page-section hidden"
    }
}

fn page_title(page: AppPage, language: &str) -> &'static str {
    match (language, page) {
        ("en", AppPage::Dashboard) => "Dashboard",
        ("en", AppPage::ProvidersAdd) => "Add provider",
        ("en", AppPage::Providers) => "Providers",
        ("en", AppPage::Desktop) => "Claude Desktop",
        ("en", AppPage::Proxy) => "Forwarding",
        ("en", AppPage::Settings) => "Settings",
        ("en", AppPage::Guide) => "Guide",
        ("ja", AppPage::Dashboard) => "ダッシュボード",
        ("ja", AppPage::ProvidersAdd) => "Provider 追加",
        ("ja", AppPage::Providers) => "Provider",
        ("ja", AppPage::Desktop) => "Claude Desktop",
        ("ja", AppPage::Proxy) => "転送",
        ("ja", AppPage::Settings) => "設定",
        ("ja", AppPage::Guide) => "ガイド",
        (_, AppPage::Dashboard) => "仪表盘",
        (_, AppPage::ProvidersAdd) => "添加提供商",
        (_, AppPage::Providers) => "提供商",
        (_, AppPage::Desktop) => "Claude 桌面版",
        (_, AppPage::Proxy) => "转发状态",
        (_, AppPage::Settings) => "设置",
        (_, AppPage::Guide) => "使用引导",
    }
}

fn page_subtitle(page: AppPage, language: &str) -> &'static str {
    match (language, page) {
        ("en", AppPage::Dashboard) => {
            "Save provider, check health, apply to Claude Desktop, and report issues."
        }
        ("en", AppPage::ProvidersAdd) => "Add a new API provider or choose a preset.",
        ("en", AppPage::Providers) => "Manage configured API providers.",
        ("en", AppPage::Desktop) => "Apply the active provider to Claude Desktop.",
        ("en", AppPage::Proxy) => "Gateway, Apply, readiness, and redacted diagnostics.",
        ("en", AppPage::Settings) => "Language, theme, and local gateway defaults.",
        ("en", AppPage::Guide) => "Three steps to connect Claude Desktop.",
        ("ja", AppPage::Dashboard) => "Provider 保存、health、Apply、問題報告。",
        ("ja", AppPage::ProvidersAdd) => "新しい API Provider を追加するか preset を選択します。",
        ("ja", AppPage::Providers) => "設定済み API Provider を管理します。",
        ("ja", AppPage::Desktop) => "active Provider を Claude Desktop に適用します。",
        ("ja", AppPage::Proxy) => "Gateway、Apply、readiness、redacted diagnostics。",
        ("ja", AppPage::Settings) => "言語、テーマ、local gateway default。",
        ("ja", AppPage::Guide) => "Claude Desktop 接続の 3 ステップ。",
        (_, AppPage::Dashboard) => "保存 Provider、健康检查、一键应用、报告问题。",
        (_, AppPage::ProvidersAdd) => "添加新的 API 提供商或选择预设。",
        (_, AppPage::Providers) => "管理已配置的 API 提供商。",
        (_, AppPage::Desktop) => "一键让 Claude 桌面版使用当前供应商。",
        (_, AppPage::Proxy) => "Gateway、Apply、readiness 和脱敏诊断。",
        (_, AppPage::Settings) => "语言、主题和本机 gateway 默认项。",
        (_, AppPage::Guide) => "按 3 步完成接入。",
    }
}

fn desktop_status_value(
    snapshot: Option<&commands::ReadinessSnapshot>,
    provider_saved: bool,
) -> String {
    match snapshot {
        Some(snapshot) if snapshot.desktop_readback_passed => "读回通过".to_owned(),
        Some(snapshot) if snapshot.provider_configured => "已配置".to_owned(),
        Some(_) => "未配置".to_owned(),
        None if provider_saved => "待应用".to_owned(),
        None => "未配置".to_owned(),
    }
}

fn desktop_status_kind(
    snapshot: Option<&commands::ReadinessSnapshot>,
    provider_saved: bool,
) -> &'static str {
    match snapshot {
        Some(snapshot) if snapshot.desktop_readback_passed => "passed",
        Some(snapshot) if snapshot.provider_configured => "pending",
        Some(_) => "failed",
        None if provider_saved => "pending",
        None => "failed",
    }
}

fn configured_row_class(
    snapshot: Option<&commands::ReadinessSnapshot>,
    provider_saved: bool,
) -> &'static str {
    match desktop_status_kind(snapshot, provider_saved) {
        "passed" => "configured-row passed",
        "pending" => "configured-row pending",
        _ => "configured-row failed",
    }
}

fn desktop_status_mark(
    snapshot: Option<&commands::ReadinessSnapshot>,
    provider_saved: bool,
) -> &'static str {
    match desktop_status_kind(snapshot, provider_saved) {
        "passed" => "✓",
        "pending" => "!",
        _ => "×",
    }
}

fn readiness_issue_text(snapshot: Option<&commands::ReadinessSnapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return "Run Health to refresh readiness layers.".to_owned();
    };
    if snapshot.issue_codes.is_empty() {
        "No issue codes in the latest health snapshot.".to_owned()
    } else {
        format!("Issue codes: {}", snapshot.issue_codes.join(", "))
    }
}

fn format_provider_list(providers: &[commands::ProviderSummary]) -> String {
    if providers.is_empty() {
        return "providers: none".to_owned();
    }
    providers
        .iter()
        .map(|provider| {
            format!(
                "{} | {} | key={}",
                provider.provider_id, provider.base_url, provider.has_api_key
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_config_backups(backups: &[commands::ConfigBackupSummary]) -> String {
    if backups.is_empty() {
        return "config backups: none".to_owned();
    }
    backups
        .iter()
        .map(|backup| {
            format!(
                "{} | {} bytes | modified={}",
                backup.file_name,
                backup.size,
                backup
                    .modified_unix_ms
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_owned())
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_provider_import_result(value: &serde_json::Value) -> String {
    let changed = value
        .get("changed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let preview = value.get("preview").unwrap_or(value);
    format!(
        "changed: {changed}\n{}",
        format_provider_import_value(preview)
    )
}

fn format_provider_import_value(value: &serde_json::Value) -> String {
    let conflicts = value
        .get("conflicts")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let provider_id = json_string(item, "providerId");
                    let existing = json_string(item, "existingDisplayName");
                    let incoming = json_string(item, "incomingDisplayName");
                    format!("{provider_id}: {existing} -> {incoming}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "none".to_owned());
    let issue_codes = value
        .get("issueCodes")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "none".to_owned());

    format!(
        "source: {}\nincoming: {}\nimportable: {}\nconflicts: {} unresolved={} skipped={} replaced={}\nwouldWrite: {}\nreplaceExisting: {}\nskipExisting: {}\nissues: {}\nconflict list:\n{}",
        json_string(value, "sourceSchema"),
        json_usize(value, "incomingProviderCount"),
        json_usize(value, "importableProviderCount"),
        json_usize(value, "conflictCount"),
        json_usize(value, "unresolvedConflictCount"),
        json_usize(value, "skippedConflictCount"),
        json_usize(value, "replacedConflictCount"),
        json_bool(value, "wouldWrite"),
        json_bool(value, "replaceExisting"),
        json_bool(value, "skipExisting"),
        issue_codes,
        conflicts
    )
}

fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}

fn json_usize(value: &serde_json::Value, key: &str) -> usize {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

fn json_bool(value: &serde_json::Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn format_provider_presets(presets: &[commands::ProviderPreset]) -> String {
    if presets.is_empty() {
        return "provider presets: none".to_owned();
    }
    presets
        .iter()
        .map(|preset| {
            let routes = preset
                .model_mappings
                .iter()
                .filter_map(|mapping| mapping.route_id.as_deref())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} | {} | {} | routes: {}",
                preset.preset_id, preset.display_name, preset.base_url, routes
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_smoke_result(smoke: &commands::SmokeCheckResult) -> String {
    format!(
        "layer: {}\npassed: {}\nissue: {}\ndetail: {}",
        smoke.layer,
        smoke.passed,
        smoke.issue_code.as_deref().unwrap_or("none"),
        smoke.detail
    )
}

fn desktop_managed_policy_note(managed: bool) -> &'static str {
    if managed {
        "\nnotice: 检测到 Claude Desktop managed policy，Apply 会在写入 local configLibrary 前停止。通常需要先备份并清理旧 policy，或在 unmanaged Windows profile 里重试。"
    } else {
        ""
    }
}

fn confirm_action(message: &str) -> bool {
    web_sys::window()
        .and_then(|window| window.confirm_with_message(message).ok())
        .unwrap_or(false)
}

fn restart_reminder_class(required: bool) -> &'static str {
    if required {
        "restart-reminder"
    } else {
        "restart-reminder hidden"
    }
}

fn format_desktop_clear_result(result: &commands::DesktopClearResult) -> String {
    format!(
        "success: {}\nconfigId: {}\nlocalConfigLibrary: {}\nremovedConfig: {}\nclearedActiveConfig: {}\npreservedMeta: {}\nreadbackCleared: {}\nissues: {}",
        result.success,
        result.config_id,
        result.local_config_library,
        result.removed_config,
        result.cleared_active_config,
        result.preserved_meta,
        result.readback_cleared,
        if result.issue_codes.is_empty() {
            "none".to_owned()
        } else {
            result.issue_codes.join(", ")
        }
    )
}

fn format_desktop_restart_result(result: &commands::DesktopRestartResult) -> String {
    format!(
        "launched: {}\nplatform: {:?}\nstoppedProcesses: {}\nforcedProcesses: {}\nexecutable: {}\nmessage: {}",
        result.launched,
        result.platform,
        result.stopped_processes,
        result.forced_processes,
        result.executable.as_deref().unwrap_or("unknown"),
        result.message
    )
}

fn default_provider_template_json() -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "schemaVersion": 1,
        "kind": "ccds.providerTemplate",
        "templates": [{
            "templateId": "openrouter",
            "displayName": "OpenRouter",
            "baseUrl": "https://openrouter.ai/api/v1",
            "apiFormat": "openai_chat",
            "modelMappings": [
                {
                    "slot": "sonnet",
                    "upstreamModel": "anthropic/claude-sonnet-4.5",
                    "routeId": "claude-openrouter-sonnet-4-5",
                    "supports1m": false,
                    "supportsMax": false
                },
                {
                    "slot": "default",
                    "upstreamModel": "anthropic/claude-sonnet-4.5",
                    "routeId": null,
                    "supports1m": false,
                    "supportsMax": false
                }
            ]
        }]
    }))
    .unwrap_or_else(|_| "{}".to_owned())
}

fn default_model_mapping_json() -> String {
    serde_json::to_string_pretty(&serde_json::json!([
        {
            "slot": "sonnet",
            "upstreamModel": "deepseek-v4-pro",
            "routeId": "claude-deepseek-v4-pro",
            "supports1m": true,
            "supportsMax": false
        },
        {
            "slot": "default",
            "upstreamModel": "deepseek-v4-pro",
            "routeId": null,
            "supports1m": true,
            "supportsMax": false
        }
    ]))
    .unwrap_or_else(|_| "[]".to_owned())
}

fn visible_model_mapping_drafts(
    default_model: String,
    opus_model: String,
    sonnet_model: String,
    haiku_model: String,
) -> Vec<ModelMappingDraft> {
    vec![
        ModelMappingDraft {
            slot: ModelSlot::Default,
            upstream_model: default_model,
            route_id: None,
            supports_1m: false,
            supports_max: false,
        },
        ModelMappingDraft {
            slot: ModelSlot::Opus,
            upstream_model: opus_model,
            route_id: None,
            supports_1m: true,
            supports_max: false,
        },
        ModelMappingDraft {
            slot: ModelSlot::Sonnet,
            upstream_model: sonnet_model,
            route_id: None,
            supports_1m: true,
            supports_max: false,
        },
        ModelMappingDraft {
            slot: ModelSlot::Haiku,
            upstream_model: haiku_model,
            route_id: None,
            supports_1m: false,
            supports_max: false,
        },
    ]
}

fn model_mapping_target_provider_id(
    editing_provider_id: Option<String>,
    _selected_provider_id: Option<String>,
) -> Option<String> {
    editing_provider_id
}

fn text(language: &str, key: &str) -> &'static str {
    match (language, key) {
        ("en", "nav_dashboard") => "Dashboard",
        ("en", "nav_provider") => "Provider",
        ("en", "nav_proxy") => "Forwarding",
        ("en", "nav_diagnostics") => "Diagnostics",
        ("en", "nav_settings") => "Settings",
        ("en", "nav_guide") => "Guide",
        ("en", "dashboard_title") => "Local gateway workbench",
        ("en", "dashboard_subtitle") => {
            "Provider, gateway, apply, import/export, and diagnostics are wired through Rust commands."
        }
        ("en", "desktop_status") => "Claude Desktop",
        ("en", "gateway_status") => "Gateway",
        ("en", "active_provider") => "Current provider",
        ("en", "readback_pending") => "readback pending",
        ("en", "provider_form") => "Provider configuration",
        ("en", "provider_name") => "Provider name *",
        ("en", "save_provider") => "Save",
        ("en", "check_health") => "Check config",
        ("en", "apply_dry_run") => "Apply dry-run",
        ("en", "home_actions") => "Common actions",
        ("en", "report_issue") => "Report issue",
        ("en", "model_mapping") => "Model mapping",
        ("en", "deferred_slot") => "deferred",
        ("en", "readiness_layers") => "Readiness",
        ("ja", "nav_dashboard") => "ダッシュボード",
        ("ja", "nav_provider") => "プロバイダー",
        ("ja", "nav_proxy") => "転送",
        ("ja", "nav_diagnostics") => "診断",
        ("ja", "nav_settings") => "設定",
        ("ja", "nav_guide") => "ガイド",
        ("ja", "dashboard_title") => "ローカル gateway",
        ("ja", "dashboard_subtitle") => {
            "Provider、gateway、Apply、import/export、diagnostics を Rust command で接続しています。"
        }
        ("ja", "desktop_status") => "Claude Desktop",
        ("ja", "gateway_status") => "Gateway",
        ("ja", "active_provider") => "現在の Provider",
        ("ja", "readback_pending") => "readback 未実行",
        ("ja", "provider_form") => "Provider 設定",
        ("ja", "provider_name") => "Provider 名 *",
        ("ja", "save_provider") => "保存",
        ("ja", "check_health") => "設定確認",
        ("ja", "apply_dry_run") => "Dry-run",
        ("ja", "home_actions") => "よく使う操作",
        ("ja", "report_issue") => "問題を報告",
        ("ja", "model_mapping") => "モデル mapping",
        ("ja", "deferred_slot") => "未実装",
        ("ja", "readiness_layers") => "Readiness",
        (_, "nav_dashboard") => "仪表盘",
        (_, "nav_provider") => "提供商",
        (_, "nav_proxy") => "转发",
        (_, "nav_diagnostics") => "诊断",
        (_, "nav_settings") => "设置",
        (_, "nav_guide") => "引导",
        (_, "dashboard_title") => "本机 gateway 工作台",
        (_, "dashboard_subtitle") => {
            "Provider、gateway、Apply、import/export 和 diagnostics 已通过 Rust command 串起。"
        }
        (_, "desktop_status") => "Claude Desktop 状态",
        (_, "gateway_status") => "Gateway 状态",
        (_, "active_provider") => "当前提供商",
        (_, "readback_pending") => "等待读回校验",
        (_, "provider_form") => "提供商配置",
        (_, "provider_name") => "提供商名称 *",
        (_, "save_provider") => "保存",
        (_, "check_health") => "检查配置",
        (_, "apply_dry_run") => "Apply dry-run",
        (_, "home_actions") => "常用操作",
        (_, "report_issue") => "报告问题",
        (_, "model_mapping") => "模型映射",
        (_, "deferred_slot") => "后续实现",
        (_, "readiness_layers") => "分层 readiness",
        _ => "",
    }
}
