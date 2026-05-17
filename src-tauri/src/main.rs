// Stage 6:Tauri 自定义 URI scheme `cas://localhost/` → in-process axum,
// frontend/ 整目录 include_dir 进二进制。frontend 零改动(v1.4 Bootstrap 视觉)。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod admin;
mod codex_plugin_unlocker;
mod proxy_runner;
mod telemetry_bridge;

use std::sync::Arc;

use axum::body::Body;
use bytes::Bytes;
use proxy_runner::ProxyManager;
use std::io::Write;

use tauri::menu::{Menu, MenuBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, Runtime, WindowEvent};
use tower::ServiceExt;

use admin::{build_app_router, handlers, AdminState};

fn main() {
    // 必须在所有可能 emit tracing event 的代码之前 init,否则 startup 阶段
    // (registry healing / desktop apply / proxy 拉起)的 tracing event 会被 drop。
    telemetry_bridge::init_global_subscriber();

    let proxy_manager = Arc::new(ProxyManager::new());
    let admin_state = AdminState {
        proxy_manager: proxy_manager.clone(),
    };
    let app_router = Arc::new(build_app_router(admin_state));
    let app_router_for_protocol = app_router.clone();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_main_window(app);
        }))
        .manage(proxy_manager)
        .register_asynchronous_uri_scheme_protocol("cas", move |_app, request, responder| {
            let router = app_router_for_protocol.clone();
            tauri::async_runtime::spawn(async move {
                let response = handle_cas_request(router, request).await;
                responder.respond(response);
            });
        })
        .setup(|app| {
            let startup_proxy_manager = app.state::<Arc<ProxyManager>>().inner().clone();
            let _ = handlers::desktop::restore_codex_if_enabled("startup");
            tauri::async_runtime::spawn(async move {
                let _ = handlers::desktop::auto_apply_on_startup_if_enabled(startup_proxy_manager)
                    .await;
            });

            // ── Plugin Unlock 守护进程自动启动 ──
            // 如果用户开启了 "autoUnlockCodexPlugins" 设置，启动 CDP 注入守护
            tauri::async_runtime::spawn(async move {
                // 延迟 5 秒，等桌面 apply + Codex 启动完成后再检测
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

                // 从 registry 中读取开关状态
                let auto_unlock = match crate::admin::registry_io::load() {
                    Ok(cfg) => cfg
                        .get("settings")
                        .and_then(|s| s.get("autoUnlockCodexPlugins"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    Err(_) => false,
                };

                if auto_unlock {
                    tracing::info!("[PluginUnlock] autoUnlockCodexPlugins=true, starting service");
                    let service = crate::codex_plugin_unlocker::PluginUnlockService::default_new();
                    service.start();
                } else {
                    tracing::debug!(
                        "[PluginUnlock] autoUnlockCodexPlugins=false, skipping auto-start"
                    );
                }
            });

            let menu = build_tray_menu(app)?;
            let _ = TrayIconBuilder::with_id("main")
                .tooltip("Codex App Transfer")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                // macOS 习惯:左键点图标 = 切窗口可见性,右键才弹菜单
                .show_menu_on_left_click(false)
                .on_menu_event(handle_tray_menu)
                .on_tray_icon_event(|tray, event| {
                    log_tray_event(&event);
                    let app = tray.app_handle();
                    // **不要**在每个事件(尤其右键 Click/Move/Enter)里
                    // 调用 `refresh_tray_menu`:Windows 平台正在呈现菜单时
                    // 把菜单引用替换会让选项点不动 / 不显示(2026-05-06
                    // 现场实测)。菜单在 handle_tray_menu 切 provider 之后
                    // 已经会刷新一次,那是真正会变内容的时机;其他事件
                    // (左键开窗 / 悬停 / 双击)菜单不变,没必要重建。
                    match event {
                        // 左键单击(Up)= 显示窗口并 focus
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } => show_main_window(app),
                        // 双击 = 同左键
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        } => show_main_window(app),
                        _ => {}
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                // macOS:用 NSApp.hide:(`app.hide()`)而不是 NSWindow.orderOut:
                // (`window.hide()`)。NSApp.hide/unhide 是 Apple 提供的 app 级
                // 隐藏 API,状态切换比 NSWindow.orderOut 干净;且与 NSStatusItem
                // 组合的官方 menubar-app 模式就是这样写的。
                // 非 macOS 仍用 window.hide()。
                #[cfg(target_os = "macos")]
                {
                    let app_handle = window.app_handle().clone();
                    let _ = window.run_on_main_thread(move || {
                        let _ = app_handle.hide();
                    });
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building Codex App Transfer");

    app.run(|app_handle, event| {
        if matches!(event, RunEvent::Exit) {
            let manager = app_handle.state::<Arc<ProxyManager>>();
            manager.stop_silent();
            // 取消任何 in-flight OAuth login —— 防 user 在 OAuth 5min 等待
            // 期间 Cmd+Q 退出 app,后台 task 残留 5min 后才超时(浪费资源,
            // 而且 callback 还可能触发 token persist 写入磁盘但 user 已经
            // 退出 app,产生 ghost 状态)
            let outcome = handlers::gemini_oauth::cancel_in_flight_login();
            if let (true, Some(target_epoch)) = (outcome.cancelled, outcome.cancelled_epoch) {
                tracing::info!(
                    target_epoch,
                    "app exit: cancelled in-flight OAuth login,等 epoch={target_epoch} task 真退出 (≤2s) 防 partial token persist"
                );
                // **C1 chatgpt-codex P1+P2 修**:wait_for_login_epoch_complete 用
                // watch::channel sticky 状态等 specific epoch 完成。比 notify
                // 强:① guard.drop 在 await 之前发生时仍能"读到" sticky 值
                //   立即返(P2 持久化完成信号);② preemption 场景下不被另一
                //   newer login 完成事件误唤醒(P1 specific epoch wait)。
                // timeout 2s 兜底防 task 异常 hang
                let _ = tauri::async_runtime::block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        handlers::gemini_oauth::wait_for_login_epoch_complete(target_epoch),
                    )
                    .await
                });
                tracing::info!(target_epoch, "app exit: epoch={target_epoch} 已退出或 timeout");
            }
            let _ = handlers::desktop::restore_codex_if_enabled("exit");
        }
    });
}

/// 把 Tauri 协议层的 http::Request<Vec<u8>> 喂进 axum,response 转回 http 类型.
async fn handle_cas_request(
    router: Arc<axum::Router>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    // 1. http::Request<Vec<u8>> → axum::Request<Body>
    let (parts, body_bytes) = request.into_parts();
    let axum_req = axum::http::Request::from_parts(parts, Body::from(Bytes::from(body_bytes)));

    // 2. router.oneshot
    let response = match (*router).clone().oneshot(axum_req).await {
        Ok(r) => r,
        Err(e) => {
            return tauri::http::Response::builder()
                .status(500)
                .body(format!("router error: {e}").into_bytes())
                .unwrap();
        }
    };

    // 3. axum::Response<Body> → http::Response<Vec<u8>>
    let (parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b.to_vec(),
        Err(e) => {
            return tauri::http::Response::builder()
                .status(500)
                .body(format!("body read error: {e}").into_bytes())
                .unwrap();
        }
    };
    tauri::http::Response::from_parts(parts, bytes)
}

fn handle_tray_menu(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref();
    if let Some(provider_id) = id.strip_prefix("provider:") {
        if provider_id != "none" {
            let provider_id = provider_id.to_owned();
            let app_handle = app.clone();
            let proxy_manager = app.state::<Arc<ProxyManager>>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let _ =
                    handlers::desktop::switch_provider_and_sync(proxy_manager, provider_id).await;
                refresh_tray_menu(&app_handle);
            });
        }
        return;
    }

    match id {
        "show" => show_main_window(app),
        "hide" => hide_main_window(app),
        "quit" => {
            let manager = app.state::<Arc<ProxyManager>>();
            manager.stop_silent();
            app.exit(0);
        }
        _ => {}
    }
}

fn build_tray_menu<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Menu<R>> {
    let mut providers = SubmenuBuilder::new(manager, "Switch provider");
    let entries = tray_provider_entries();
    if entries.is_empty() {
        providers = providers.text("provider:none", "No providers");
    } else {
        for entry in entries {
            let label = if entry.active {
                format!("✓ {}", entry.name)
            } else {
                entry.name
            };
            providers = providers.text(format!("provider:{}", entry.id), label);
        }
    }
    let providers = providers.build()?;
    MenuBuilder::new(manager)
        .text("show", "Show window")
        .text("hide", "Hide window")
        .separator()
        .item(&providers)
        .separator()
        .text("quit", "Quit Codex App Transfer")
        .build()
}

fn refresh_tray_menu(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Ok(menu) = build_tray_menu(app) {
        let _ = tray.set_menu(Some(menu));
    }
}

struct TrayProviderEntry {
    id: String,
    name: String,
    active: bool,
}

fn tray_provider_entries() -> Vec<TrayProviderEntry> {
    let Ok(cfg) = admin::registry_io::load() else {
        return Vec::new();
    };
    let active_id = cfg.get("activeProvider").and_then(|v| v.as_str());
    cfg.get("providers")
        .and_then(|v| v.as_array())
        .map(|providers| {
            providers
                .iter()
                .filter_map(|provider| {
                    let id = provider.get("id").and_then(|v| v.as_str())?;
                    let name = provider
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unnamed Provider");
                    Some(TrayProviderEntry {
                        id: id.to_owned(),
                        name: name.to_owned(),
                        active: Some(id) == active_id,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn show_main_window(app: &AppHandle) {
    // macOS:NSApp.unhide:(`app.show()`)反向唤醒 app + 全部窗口,
    // 与关窗时的 NSApp.hide: 配对。
    #[cfg(target_os = "macos")]
    let _ = app.show();

    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }

    // macOS 14+:`NSApplicationActivateIgnoringOtherApps` 已废弃,改用
    // `NSRunningApplication.activate(.activateAllWindows)` 强制带到前台。
    #[cfg(target_os = "macos")]
    activate_macos_app();
}

#[cfg(target_os = "macos")]
fn activate_macos_app() {
    // macOS 14(Sonoma)起 `NSApplicationActivateIgnoringOtherApps` 已被
    // deprecated 且**实际无效** —— 这就是我们之前 Tauri set_focus()
    // 失效的根本原因(Tauri 内部就用这个 flag 走 NSApp.activate)。
    // 改走 NSRunningApplication.activate(.activateAllWindows),Apple 推荐
    // 替代品,在所有 macOS 14+ 版本下强制有效。
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    unsafe {
        let app = NSRunningApplication::currentApplication();
        app.activateWithOptions(NSApplicationActivationOptions::NSApplicationActivateAllWindows);
    }
}

fn hide_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// 写每次 tray 事件到 `~/.codex-app-transfer/tray.log`,便于诊断 click 是否
/// 真的触发 / 被什么字段过滤掉.手测完可以删此函数.
fn log_tray_event(event: &TrayIconEvent) {
    let Ok(home) = std::env::var("HOME") else {
        return;
    };
    let dir = std::path::Path::new(&home).join(".codex-app-transfer");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("tray.log");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {event:?}");
    }
}
