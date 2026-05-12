pub mod apply_flow;
pub mod commands;
pub mod config;
pub mod desktop;
pub mod desktop_writer;
pub mod diagnostics;
pub mod gateway;
pub mod gateway_adapter;
pub mod model_catalog;
pub mod provider;
pub mod release_gate;
pub mod state;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, RunEvent, WindowEvent,
};

#[derive(Clone, serde::Serialize)]
struct SingleInstancePayload {
    args: Vec<String>,
    cwd: String,
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn install_app_shell(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show CC Desktop Switch", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut tray = TrayIconBuilder::with_id("cc-desktop-switch")
        .tooltip("CC Desktop Switch")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.build(app)?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            let _ = app.emit("single-instance", SingleInstancePayload { args, cwd });
            show_main_window(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(state::AppState::default())
        .setup(|app| {
            install_app_shell(app)?;

            let state = app.state::<state::AppState>();
            if let Err(error) = state.start_gateway() {
                if !error.to_string().contains("gateway.no_active_provider") {
                    eprintln!("gateway startup failed: {error}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::save_provider,
            commands::set_active_provider,
            commands::delete_provider,
            commands::reorder_providers,
            commands::export_providers,
            commands::save_provider_export_as,
            commands::preview_provider_import,
            commands::import_providers,
            commands::list_provider_presets,
            commands::preview_provider_preset_import,
            commands::import_provider_preset,
            commands::list_model_mappings,
            commands::update_model_mappings,
            commands::list_config_backups,
            commands::read_config_backup,
            commands::health,
            commands::gateway_status,
            commands::start_gateway,
            commands::stop_gateway,
            commands::desktop_config_probe,
            commands::export_diagnostics_package,
            commands::copy_diagnostics_summary,
            commands::copy_diagnostics_summary_to_clipboard,
            commands::save_diagnostics_package,
            commands::save_diagnostics_package_as,
            commands::diagnostics_issue_draft,
            commands::open_diagnostics_issue,
            commands::provider_static_smoke,
            commands::gateway_smoke,
            commands::provider_real_smoke,
            commands::apply_dry_run,
            commands::apply_local_config,
            commands::apply_detected_local_config
        ])
        .build(tauri::generate_context!())
        .expect("failed to build CC Desktop Switch")
        .run(|app, event| {
            if let RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } = event
            {
                if label == "main" {
                    api.prevent_close();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
            }
        });
}
