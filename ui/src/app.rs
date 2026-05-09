use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::commands::{
    self, ApiFormat, ModelMappingDraft, ProviderDraft, ProviderPreset, ProviderSummary,
};

#[component]
pub fn App() -> impl IntoView {
    let (language, set_language) = signal("zh".to_owned());
    let (theme, set_theme) = signal("light".to_owned());
    let (providers, set_providers) = signal(Vec::<ProviderSummary>::new());
    let (selected_provider_id, set_selected_provider_id) = signal(None::<String>);
    let (provider_name, set_provider_name) = signal("DeepSeek".to_owned());
    let (base_url, set_base_url) = signal("https://api.deepseek.com/anthropic".to_owned());
    let (api_key, set_api_key) = signal(String::new());
    let (api_format, set_api_format) = signal("anthropic".to_owned());
    let (import_json, set_import_json) = signal(String::new());
    let (import_preview_text, set_import_preview_text) =
        signal("Provider import preview has not run.".to_owned());
    let (provider_presets, set_provider_presets) = signal(Vec::<ProviderPreset>::new());
    let (selected_preset_id, set_selected_preset_id) = signal("deepseek".to_owned());
    let (preset_api_key, set_preset_api_key) = signal(String::new());
    let (model_mapping_json, set_model_mapping_json) = signal(default_model_mapping_json());
    let (backup_file_name, set_backup_file_name) = signal(String::new());
    let (result, set_result) = signal("尚未执行 command。".to_owned());
    let (diagnostics_text, set_diagnostics_text) = signal("尚未生成 diagnostics。".to_owned());
    let (provider_saved, set_provider_saved) = signal(false);
    let (gateway_status_text, set_gateway_status_text) = signal("planned :18080".to_owned());
    let (readiness_snapshot, set_readiness_snapshot) = signal(None::<commands::ReadinessSnapshot>);

    let copy = move |key: &'static str| text(&language.get(), key);
    let theme_label = move || if theme.get() == "dark" { "☀" } else { "☾" };

    let refresh_providers = move |_| {
        set_result.set("刷新 Provider 列表中...".to_owned());
        spawn_local(async move {
            match commands::list_providers().await {
                Ok(next_providers) => {
                    if selected_provider_id.get_untracked().is_none() {
                        if let Some(provider) = next_providers.first() {
                            set_selected_provider_id.set(Some(provider.provider_id.clone()));
                        }
                    }
                    set_result.set(format_provider_list(&next_providers));
                    set_providers.set(next_providers);
                }
                Err(error) => set_result.set(format!("list_providers failed: {error}")),
            }
        });
    };

    let save_provider = move |_| {
        let request = ProviderDraft {
            provider_id: selected_provider_id.get_untracked(),
            display_name: provider_name.get_untracked(),
            base_url: base_url.get_untracked(),
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
                    set_selected_provider_id.set(Some(summary.provider_id.clone()));
                    set_api_key.set(String::new());
                    match commands::list_providers().await {
                        Ok(next_providers) => {
                            set_providers.set(next_providers.clone());
                            set_result.set(format!(
                                "save_provider ok\n{}\n{}",
                                summary.provider_id,
                                format_provider_list(&next_providers)
                            ));
                        }
                        Err(error) => set_result
                            .set(format!("save_provider ok\nlist_providers failed: {error}")),
                    }
                }
                Err(error) => set_result.set(format!("save_provider failed: {error}")),
            }
        });
    };

    let delete_selected_provider = move |_| {
        let Some(provider_id) = selected_provider_id.get_untracked() else {
            set_result.set("delete_provider skipped: no provider selected".to_owned());
            return;
        };
        set_result.set(format!("删除 Provider 中: {provider_id}"));
        spawn_local(async move {
            match commands::delete_provider(provider_id).await {
                Ok(changed) => match commands::list_providers().await {
                    Ok(next_providers) => {
                        set_selected_provider_id.set(
                            next_providers
                                .first()
                                .map(|provider| provider.provider_id.clone()),
                        );
                        set_providers.set(next_providers.clone());
                        set_result.set(format!(
                            "delete_provider changed={changed}\n{}",
                            format_provider_list(&next_providers)
                        ));
                    }
                    Err(error) => set_result.set(format!(
                        "delete_provider changed={changed}\nlist_providers failed: {error}"
                    )),
                },
                Err(error) => set_result.set(format!("delete_provider failed: {error}")),
            }
        });
    };

    let move_selected_provider_first = move |_| {
        let Some(provider_id) = selected_provider_id.get_untracked() else {
            set_result.set("reorder_providers skipped: no provider selected".to_owned());
            return;
        };
        set_result.set(format!("调整 Provider 顺序中: {provider_id}"));
        spawn_local(async move {
            let current = match commands::list_providers().await {
                Ok(providers) => providers,
                Err(error) => {
                    set_result.set(format!("list_providers failed: {error}"));
                    return;
                }
            };
            let ids = current
                .iter()
                .map(|provider| provider.provider_id.clone())
                .collect::<Vec<_>>();
            if !ids.iter().any(|id| id == &provider_id) {
                set_result.set(format!(
                    "reorder_providers skipped: {provider_id} is not in provider list"
                ));
                return;
            }
            let mut reordered = vec![provider_id.clone()];
            reordered.extend(ids.into_iter().filter(|id| id != &provider_id));
            match commands::reorder_providers(reordered).await {
                Ok(changed) => match commands::list_providers().await {
                    Ok(next_providers) => {
                        set_providers.set(next_providers.clone());
                        set_result.set(format!(
                            "reorder_providers changed={changed}\n{}",
                            format_provider_list(&next_providers)
                        ));
                    }
                    Err(error) => set_result.set(format!(
                        "reorder_providers changed={changed}\nlist_providers failed: {error}"
                    )),
                },
                Err(error) => set_result.set(format!("reorder_providers failed: {error}")),
            }
        });
    };

    let set_selected_active = move |_| {
        let Some(provider_id) = selected_provider_id.get_untracked() else {
            set_result.set("set_active_provider skipped: no provider selected".to_owned());
            return;
        };
        set_result.set(format!("设置 active Provider 中: {provider_id}"));
        spawn_local(async move {
            match commands::set_active_provider(provider_id.clone()).await {
                Ok(changed) => set_result.set(format!(
                    "set_active_provider changed={changed}\nactiveProvider={provider_id}"
                )),
                Err(error) => set_result.set(format!("set_active_provider failed: {error}")),
            }
        });
    };

    let export_providers = move |_| {
        set_result.set("导出 Provider package 中...".to_owned());
        spawn_local(async move {
            match commands::export_providers().await {
                Ok(package) => match serde_json::to_string_pretty(&package) {
                    Ok(raw) => {
                        set_import_json.set(raw.clone());
                        set_result.set(raw);
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
                    if let Ok(next_providers) = commands::list_providers().await {
                        set_providers.set(next_providers);
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
                Err(error) => set_result.set(format!("import_providers failed: {error}")),
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
                    if let Ok(next_providers) = commands::list_providers().await {
                        set_providers.set(next_providers);
                    }
                    set_preset_api_key.set(String::new());
                    match serde_json::to_string_pretty(&import_result) {
                        Ok(raw) => set_result.set(raw),
                        Err(error) => {
                            set_result.set(format!("format preset import failed: {error}"))
                        }
                    }
                }
                Err(error) => set_result.set(format!("import_provider_preset failed: {error}")),
            }
        });
    };

    let load_model_mappings = move |_| {
        let Some(provider_id) = selected_provider_id.get_untracked() else {
            set_result.set("list_model_mappings skipped: no provider selected".to_owned());
            return;
        };
        set_result.set(format!("读取模型映射中: {provider_id}"));
        spawn_local(async move {
            match commands::list_model_mappings(provider_id).await {
                Ok(mappings) => match serde_json::to_string_pretty(&mappings) {
                    Ok(raw) => {
                        set_model_mapping_json.set(raw.clone());
                        set_result.set(raw);
                    }
                    Err(error) => set_result.set(format!("format mappings failed: {error}")),
                },
                Err(error) => set_result.set(format!("list_model_mappings failed: {error}")),
            }
        });
    };

    let save_model_mappings = move |_| {
        let Some(provider_id) = selected_provider_id.get_untracked() else {
            set_result.set("update_model_mappings skipped: no provider selected".to_owned());
            return;
        };
        let raw = model_mapping_json.get_untracked();
        let mappings = match serde_json::from_str::<Vec<ModelMappingDraft>>(&raw) {
            Ok(mappings) => mappings,
            Err(error) => {
                set_result.set(format!("parse model mappings failed: {error}"));
                return;
            }
        };
        set_result.set(format!("保存模型映射中: {provider_id}"));
        spawn_local(async move {
            match commands::update_model_mappings(provider_id, mappings).await {
                Ok(saved) => match serde_json::to_string_pretty(&saved) {
                    Ok(raw) => {
                        set_model_mapping_json.set(raw.clone());
                        set_result.set(format!("update_model_mappings ok\n{raw}"));
                    }
                    Err(error) => set_result.set(format!("format mappings failed: {error}")),
                },
                Err(error) => set_result.set(format!("update_model_mappings failed: {error}")),
            }
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

    let run_health_check = move || {
        set_result.set("读取 health 中...".to_owned());
        spawn_local(async move {
            match commands::health().await {
                Ok(snapshot) => {
                    set_readiness_snapshot.set(Some(snapshot.clone()));
                    set_gateway_status_text.set(format_gateway_health(&snapshot.gateway));
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

    let check_gateway_status = move |_| {
        set_result.set("读取 gateway status 中...".to_owned());
        spawn_local(async move {
            match commands::gateway_status().await {
                Ok(health) => {
                    let formatted = format_gateway_health(&health);
                    set_gateway_status_text.set(formatted.clone());
                    set_result.set(format!("gateway: {formatted}"));
                }
                Err(error) => set_result.set(format!("gateway_status failed: {error}")),
            }
        });
    };

    let start_gateway = move |_| {
        set_result.set("启动 gateway 中...".to_owned());
        spawn_local(async move {
            match commands::start_gateway().await {
                Ok(health) => {
                    let formatted = format_gateway_health(&health);
                    set_gateway_status_text.set(formatted.clone());
                    set_result.set(format!("gateway started: {formatted}"));
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
                    set_gateway_status_text.set(formatted.clone());
                    set_result.set(format!("gateway stopped: {formatted}"));
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
    let copy_diagnostics_summary = move |_| run_copy_diagnostics_summary();

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

    let save_diagnostics_package_as = move |_| {
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

    let preview_issue_draft = move |_| {
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

    let open_issue = move |_| {
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
        set_result.set("运行 provider static smoke 中...".to_owned());
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

    let dry_run = move |_| {
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
                    set_result.set(format!(
                        "success: {}\nmode: {}\nexpectedBaseUrl: {}\nexpectedRoutes: {}\n{}",
                        plan.success, plan.mode, plan.expected_base_url, routes, steps
                    ));
                }
                Err(error) => set_result.set(format!("apply_dry_run failed: {error}")),
            }
        });
    };

    let run_apply = move || {
        set_result.set("执行 apply 中...".to_owned());
        spawn_local(async move {
            match commands::apply_detected_local_config().await {
                Ok(result) => {
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

    view! {
        <div class="app-shell" data-theme=move || theme.get()>
            <header class="app-header">
                <div class="brand">
                    <span class="brand-mark">"CC"</span>
                    <span class="brand-title">"CC Desktop Switch"</span>
                </div>
                <nav class="route-tabs" aria-label="Primary navigation">
                    <button class="route-tab active" type="button">{move || copy("nav_dashboard")}</button>
                    <button class="route-tab" type="button">{move || copy("nav_provider")}</button>
                    <button class="route-tab" type="button">{move || copy("nav_diagnostics")}</button>
                    <button class="route-tab" type="button">{move || copy("nav_settings")}</button>
                </nav>
                <div class="header-actions">
                    <button class="ghost-button" type="button" on:click=move |_| {
                        set_language.update(|value| {
                            *value = if value == "zh" {
                                "en".to_owned()
                            } else if value == "en" {
                                "ja".to_owned()
                            } else {
                                "zh".to_owned()
                            };
                        });
                    }>{move || language.get().to_uppercase()}</button>
                    <button class="icon-button" type="button" aria-label="Toggle theme" on:click=move |_| {
                        set_theme.update(|value| {
                            *value = if value == "dark" { "light".to_owned() } else { "dark".to_owned() };
                        });
                    }>{theme_label}</button>
                </div>
            </header>

            <main class="app-main">
                <section class="page-title">
                    <h1>{move || copy("dashboard_title")}</h1>
                    <p>{move || copy("dashboard_subtitle")}</p>
                </section>

                <section class="status-grid" aria-label="Status overview">
                    <article class="status-card">
                        <h2>{move || copy("desktop_status")}</h2>
                        <span class="status-value">{move || desktop_status_value(readiness_snapshot.get().as_ref(), provider_saved.get())}</span>
                        <span class=move || status_pill_class(readiness_snapshot.get().map(|snapshot| snapshot.desktop_readback_passed))>
                            {move || readiness_label(readiness_snapshot.get().map(|snapshot| snapshot.desktop_readback_passed), copy("readback_pending"))}
                        </span>
                    </article>
                    <article class="status-card">
                        <h2>{move || copy("gateway_status")}</h2>
                        <span class="status-value">"local gateway"</span>
                        <span class=move || status_pill_class(readiness_snapshot.get().map(|snapshot| snapshot.gateway.running))>
                            {move || gateway_status_text.get()}
                        </span>
                    </article>
                    <article class="status-card">
                        <h2>{move || copy("active_provider")}</h2>
                        <span class="status-value">{move || provider_name.get()}</span>
                        <span class=move || status_pill_class(readiness_snapshot.get().map(|snapshot| snapshot.provider_configured).or_else(|| selected_provider_id.get().map(|_| true)))>
                            {move || selected_provider_id.get().unwrap_or_else(|| "no provider id".to_owned())}
                        </span>
                    </article>
                </section>

                <section class="home-command-bar" aria-label="Dashboard actions">
                    <div class="home-command-copy">
                        <strong>{move || copy("home_actions")}</strong>
                        <span>{move || readiness_summary_text(readiness_snapshot.get().as_ref())}</span>
                    </div>
                    <div class="button-row">
                        <button class="secondary-button" type="button" on:click=move |_| run_health_check()>{move || copy("check_health")}</button>
                        <button class="primary-button" type="button" on:click=move |_| run_apply()>"Apply"</button>
                        <button class="secondary-button" type="button" on:click=move |_| run_copy_diagnostics_summary()>{move || copy("report_issue")}</button>
                    </div>
                </section>

                <section class="work-grid">
                    <article class="panel">
                        <h2>{move || copy("provider_form")}</h2>
                        <div class="form-grid">
                            <label class="field">
                                <span>{move || copy("provider_name")}</span>
                                <input
                                    value=move || provider_name.get()
                                    on:input=move |event| set_provider_name.set(event_target_value(&event))
                                />
                            </label>
                            <label class="field">
                                <span>"API Base URL"</span>
                                <input
                                    value=move || base_url.get()
                                    on:input=move |event| set_base_url.set(event_target_value(&event))
                                />
                            </label>
                            <label class="field">
                                <span>"API Key"</span>
                                <input
                                    type="password"
                                    placeholder="leave blank to keep saved key"
                                    value=move || api_key.get()
                                    on:input=move |event| set_api_key.set(event_target_value(&event))
                                />
                            </label>
                            <label class="field">
                                <span>"API Format"</span>
                                <select on:change=move |event| set_api_format.set(event_target_value(&event))>
                                    <option value="anthropic" selected=move || api_format.get() == "anthropic">"Anthropic compatible"</option>
                                    <option value="openai_chat" selected=move || api_format.get() == "openai_chat">"OpenAI Chat"</option>
                                </select>
                            </label>
                            <div class="button-row">
                                <button class="primary-button" type="button" on:click=save_provider>{move || copy("save_provider")}</button>
                                <button class="secondary-button" type="button" on:click=refresh_providers>"List"</button>
                                <button class="secondary-button" type="button" on:click=set_selected_active>"Set active"</button>
                                <button class="secondary-button" type="button" on:click=delete_selected_provider>"Delete"</button>
                                <button class="secondary-button" type="button" on:click=move_selected_provider_first>"Move first"</button>
                            </div>
                        </div>

                        <div class="provider-list">
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
                                                    set_provider_name.set(edit_provider.display_name.clone());
                                                    set_base_url.set(edit_provider.base_url.clone());
                                                    set_api_format.set(api_format_value(&edit_provider.api_format));
                                                    set_api_key.set(String::new());
                                                }>"Edit"</button>
                                                <button class="secondary-button compact" type="button" on:click=move |_| {
                                                    set_selected_provider_id.set(Some(active_provider.provider_id.clone()));
                                                }>"Select"</button>
                                            </div>
                                        </div>
                                    }
                                }
                            />
                        </div>
                    </article>

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

                        <h2>"Provider Presets"</h2>
                        <div class="preset-controls">
                            <select on:change=move |event| set_selected_preset_id.set(event_target_value(&event))>
                                <For
                                    each=move || provider_presets.get()
                                    key=|preset| preset.preset_id.clone()
                                    children=move |preset| view! {
                                        <option
                                            value=preset.preset_id.clone()
                                            selected=move || selected_preset_id.get() == preset.preset_id
                                        >
                                            {format!("{} ({})", preset.display_name, preset.preset_id)}
                                        </option>
                                    }
                                />
                            </select>
                            <input
                                type="password"
                                placeholder="preset API key"
                                value=move || preset_api_key.get()
                                on:input=move |event| set_preset_api_key.set(event_target_value(&event))
                            />
                        </div>
                        <div class="button-row">
                            <button class="secondary-button" type="button" on:click=load_provider_presets>"Load presets"</button>
                            <button class="secondary-button" type="button" on:click=preview_preset_import>"Preview preset"</button>
                            <button class="secondary-button" type="button" on:click=move |_| import_preset(false)>"Import preset"</button>
                            <button class="secondary-button" type="button" on:click=move |_| import_preset(true)>"Replace preset"</button>
                        </div>

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

                    <article class="panel">
                        <h2>"Gateway / Apply"</h2>
                        <div class="button-row">
                            <button class="secondary-button" type="button" on:click=check_health>{move || copy("check_health")}</button>
                            <button class="secondary-button" type="button" on:click=check_gateway_status>"Gateway"</button>
                            <button class="secondary-button" type="button" on:click=start_gateway>"Start"</button>
                            <button class="secondary-button" type="button" on:click=stop_gateway>"Stop"</button>
                            <button class="secondary-button" type="button" on:click=run_provider_static_smoke>"Static smoke"</button>
                            <button class="secondary-button" type="button" on:click=run_gateway_smoke>"Gateway smoke"</button>
                            <button class="secondary-button" type="button" on:click=run_provider_real_smoke>"Provider smoke"</button>
                            <button class="secondary-button" type="button" on:click=probe_desktop_config>"Config"</button>
                            <button class="secondary-button" type="button" on:click=dry_run>{move || copy("apply_dry_run")}</button>
                            <button class="primary-button" type="button" on:click=apply>"Apply"</button>
                        </div>
                        <div class="result-box" aria-live="polite">{move || result.get()}</div>
                    </article>

                    <article class="panel">
                        <h2>{move || copy("model_mapping")}</h2>
                        <div class="button-row">
                            <button class="secondary-button" type="button" on:click=load_model_mappings>"Load mappings"</button>
                            <button class="primary-button" type="button" on:click=save_model_mappings>"Save mappings"</button>
                        </div>
                        <textarea
                            class="json-input mapping-input"
                            prop:value=move || model_mapping_json.get()
                            on:input=move |event| set_model_mapping_json.set(event_target_value(&event))
                        ></textarea>

                        <h2>{move || copy("readiness_layers")}</h2>
                        <ul class="readiness-list">
                            <li><span>"static config"</span><strong class=move || readiness_badge_class(readiness_snapshot.get().map(|snapshot| snapshot.provider_configured).or(Some(provider_saved.get())))>{move || readiness_badge_label(readiness_snapshot.get().map(|snapshot| snapshot.provider_configured).or(Some(provider_saved.get())))}</strong></li>
                            <li><span>"desktop readback"</span><strong class=move || readiness_badge_class(readiness_snapshot.get().map(|snapshot| snapshot.desktop_readback_passed))>{move || readiness_badge_label(readiness_snapshot.get().map(|snapshot| snapshot.desktop_readback_passed))}</strong></li>
                            <li><span>"provider smoke"</span><strong class=move || readiness_badge_class(readiness_snapshot.get().map(|snapshot| snapshot.provider_smoke_passed))>{move || readiness_badge_label(readiness_snapshot.get().map(|snapshot| snapshot.provider_smoke_passed))}</strong></li>
                            <li><span>"gateway smoke"</span><strong class=move || readiness_badge_class(readiness_snapshot.get().map(|snapshot| snapshot.gateway_smoke_passed))>{move || readiness_badge_label(readiness_snapshot.get().map(|snapshot| snapshot.gateway_smoke_passed))}</strong></li>
                        </ul>
                        <div class="issue-strip">{move || readiness_issue_text(readiness_snapshot.get().as_ref())}</div>
                    </article>

                    <article class="panel diagnostics-panel">
                        <h2>{move || copy("nav_diagnostics")}</h2>
                        <div class="button-row">
                            <button class="secondary-button" type="button" on:click=copy_diagnostics_summary>"Summary"</button>
                            <button class="secondary-button" type="button" on:click=copy_diagnostics_to_clipboard>"Copy"</button>
                            <button class="secondary-button" type="button" on:click=export_diagnostics_package>"Package"</button>
                            <button class="secondary-button" type="button" on:click=save_diagnostics_package>"Save"</button>
                            <button class="secondary-button" type="button" on:click=save_diagnostics_package_as>"Save as"</button>
                            <button class="secondary-button" type="button" on:click=preview_issue_draft>"Issue draft"</button>
                            <button class="secondary-button" type="button" on:click=open_issue>"Open issue"</button>
                        </div>
                        <div class="result-box diagnostics-output" aria-live="polite">{move || diagnostics_text.get()}</div>
                    </article>
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

fn format_gateway_health(health: &commands::GatewayHealth) -> String {
    format!("{} running={}", health.base_url, health.running)
}

fn desktop_status_value(
    snapshot: Option<&commands::ReadinessSnapshot>,
    provider_saved: bool,
) -> String {
    match snapshot {
        Some(snapshot) if snapshot.desktop_readback_passed => "readback passed".to_owned(),
        Some(snapshot) if snapshot.provider_configured => "configured".to_owned(),
        Some(_) => "not configured".to_owned(),
        None if provider_saved => "dry-run ready".to_owned(),
        None => "not configured".to_owned(),
    }
}

fn status_pill_class(passed: Option<bool>) -> String {
    match passed {
        Some(true) => "status-pill success".to_owned(),
        Some(false) => "status-pill danger".to_owned(),
        None => "status-pill warning".to_owned(),
    }
}

fn readiness_label(passed: Option<bool>, pending: &'static str) -> String {
    match passed {
        Some(true) => "pass".to_owned(),
        Some(false) => "needs attention".to_owned(),
        None => pending.to_owned(),
    }
}

fn readiness_badge_class(passed: Option<bool>) -> String {
    match passed {
        Some(true) => "readiness-badge pass".to_owned(),
        Some(false) => "readiness-badge fail".to_owned(),
        None => "readiness-badge pending".to_owned(),
    }
}

fn readiness_badge_label(passed: Option<bool>) -> &'static str {
    match passed {
        Some(true) => "pass",
        Some(false) => "check",
        None => "pending",
    }
}

fn readiness_summary_text(snapshot: Option<&commands::ReadinessSnapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return "Health has not run in this session.".to_owned();
    };
    if snapshot.issue_codes.is_empty() {
        return "No readiness issue codes reported.".to_owned();
    }
    format!("Issues: {}", snapshot.issue_codes.join(", "))
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

fn text(language: &str, key: &str) -> &'static str {
    match (language, key) {
        ("en", "nav_dashboard") => "Dashboard",
        ("en", "nav_provider") => "Provider",
        ("en", "nav_diagnostics") => "Diagnostics",
        ("en", "nav_settings") => "Settings",
        ("en", "dashboard_title") => "Local gateway workbench",
        ("en", "dashboard_subtitle") => {
            "Provider, gateway, apply, import/export, and diagnostics are wired through Rust commands."
        }
        ("en", "desktop_status") => "Claude Desktop",
        ("en", "gateway_status") => "Gateway",
        ("en", "active_provider") => "Selected provider",
        ("en", "readback_pending") => "readback pending",
        ("en", "provider_form") => "Provider",
        ("en", "provider_name") => "Provider name",
        ("en", "save_provider") => "Save",
        ("en", "check_health") => "Health",
        ("en", "apply_dry_run") => "Apply dry-run",
        ("en", "home_actions") => "Common actions",
        ("en", "report_issue") => "Report issue",
        ("en", "model_mapping") => "Model mapping",
        ("en", "deferred_slot") => "deferred",
        ("en", "readiness_layers") => "Readiness",
        ("ja", "nav_dashboard") => "ホーム",
        ("ja", "nav_provider") => "プロバイダー",
        ("ja", "nav_diagnostics") => "診断",
        ("ja", "nav_settings") => "設定",
        ("ja", "dashboard_title") => "ローカル gateway",
        ("ja", "dashboard_subtitle") => {
            "Provider、gateway、Apply、import/export、diagnostics を Rust command で接続しています。"
        }
        ("ja", "desktop_status") => "Claude Desktop",
        ("ja", "gateway_status") => "Gateway",
        ("ja", "active_provider") => "選択中 Provider",
        ("ja", "readback_pending") => "readback 未実行",
        ("ja", "provider_form") => "Provider",
        ("ja", "provider_name") => "Provider 名",
        ("ja", "save_provider") => "保存",
        ("ja", "check_health") => "Health",
        ("ja", "apply_dry_run") => "Dry-run",
        ("ja", "home_actions") => "よく使う操作",
        ("ja", "report_issue") => "問題を報告",
        ("ja", "model_mapping") => "モデル mapping",
        ("ja", "deferred_slot") => "未実装",
        ("ja", "readiness_layers") => "Readiness",
        (_, "nav_dashboard") => "首页",
        (_, "nav_provider") => "Provider",
        (_, "nav_diagnostics") => "诊断",
        (_, "nav_settings") => "设置",
        (_, "dashboard_title") => "本机 gateway 工作台",
        (_, "dashboard_subtitle") => {
            "Provider、gateway、Apply、import/export 和 diagnostics 已通过 Rust command 串起。"
        }
        (_, "desktop_status") => "Claude Desktop 状态",
        (_, "gateway_status") => "Gateway 状态",
        (_, "active_provider") => "选中 Provider",
        (_, "readback_pending") => "等待读回校验",
        (_, "provider_form") => "Provider 表单",
        (_, "provider_name") => "Provider 名称",
        (_, "save_provider") => "保存 Provider",
        (_, "check_health") => "健康检查",
        (_, "apply_dry_run") => "Apply dry-run",
        (_, "home_actions") => "常用操作",
        (_, "report_issue") => "报告问题",
        (_, "model_mapping") => "模型映射",
        (_, "deferred_slot") => "后续实现",
        (_, "readiness_layers") => "分层 readiness",
        _ => "",
    }
}
