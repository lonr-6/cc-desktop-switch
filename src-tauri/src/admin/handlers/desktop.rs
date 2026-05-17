//! `/api/desktop/*` + Codex.app 进程管理 + apply / restore 桌面状态.
//!
//! - 把 `~/.codex/{config.toml,auth.json}` 应用 / 还原
//! - Codex App 进程退出 / 重启(macOS / Windows / Linux)
//! - 桌面健康检查 + active provider 同步
//!
//! 借鉴 codex-account-switch (`src-tauri/{mac,win}/runtime/process.rs`):旧版用
//! `osascript ... quit; sleep 0.5; open -a Codex` 的 sh 一行式管道,gentle
//! quit 在 Codex 卡住 / 多窗口未保存时会被忽略,sleep 0.5 也太短;表面上
//! spawn 成功代码视为 OK,实则 app 没动.改成三步:
//! 1. pgrep / tasklist 探活
//! 2. SIGTERM / taskkill 普通退出 + 最长 4s 轮询
//! 3. 仍存活 → SIGKILL / taskkill /F + 最长 2s 轮询
//! 4. 解析 .app 路径(macOS:/Applications + ~/Applications)再 open;
//!    Windows 直接 explorer.exe shell:AppsFolder\<APP_ID>.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use axum::{http::StatusCode, response::IntoResponse, Json};
use codex_app_transfer_codex_integration::{
    apply_provider, catalog_models_for_provider, get_snapshot_status, has_snapshot, list_snapshots,
    read_auth, restore_codex_snapshot, restore_codex_state, ApplyConfig, CodexPaths,
};
use codex_app_transfer_proxy::proxy_telemetry;
use codex_app_transfer_registry::RawConfig;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::proxy_runner::ProxyManager;

#[cfg(test)]
use super::super::registry_io::save_for_test as save_registry;
use super::super::registry_io::{load as load_registry, with_config_write, ConfigMutation};
use super::super::state::AdminState;
use super::common::{active_provider_name, err, read_setting_bool, APP_VERSION};
use super::providers::{
    active_provider, provider_api_key, provider_default_model, provider_display_name,
    provider_index, provider_model_capabilities, provider_model_mappings, provider_supports_1m,
};
use super::proxy::{ensure_gateway_key, read_gateway_key, read_proxy_port, start_proxy_if_needed};
use codex_app_transfer_registry::DEFAULT_UPDATE_URL;

const ONE_M_CONTEXT_WINDOW: u64 = 1_000_000;

const MACOS_APP_NAME: &str = "Codex";
const WINDOWS_PROCESS_NAME: &str = "Codex.exe";
/// OpenAI 官方 Windows Store 包 ID,与 codex-account-switch 保持一致;
/// 用户若装的是非 Store 版本,resolve 失败时 explorer.exe 会报错,前端会
/// 看到 INTERNAL_SERVER_ERROR,比静默假成功好。
const WINDOWS_STORE_APP_ID: &str = "OpenAI.Codex_2p2nqsd0c76g0!App";
const LINUX_BIN_NAME: &str = "codex";

const QUIT_TERM_POLL_ITERS: u32 = 20; // 20 × 200ms = 4s
const QUIT_KILL_POLL_ITERS: u32 = 10; // 10 × 200ms = 2s
const QUIT_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// 退出确认后,等 launchd reap 完旧进程的 grace 窗口。低于 ~250ms 时
/// `open -a` 仍可能误命中"已在运行"缓存。
const POST_QUIT_LAUNCHD_GRACE: Duration = Duration::from_millis(400);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRestoreRequest {
    #[serde(default)]
    snapshot_id: Option<String>,
    #[serde(default)]
    cleanup_all: bool,
}

/// 平台检测命令(可纯函数测试).返回 (program, args).第一个元素总是命令名。
fn running_check_command(platform: &str) -> Vec<String> {
    match platform {
        "macos" => vec!["pgrep".into(), "-x".into(), MACOS_APP_NAME.into()],
        "windows" => vec![
            "tasklist".into(),
            "/FI".into(),
            format!("IMAGENAME eq {WINDOWS_PROCESS_NAME}"),
            "/FO".into(),
            "CSV".into(),
            "/NH".into(),
        ],
        _ => vec!["pgrep".into(), "-x".into(), LINUX_BIN_NAME.into()],
    }
}

/// 退出命令(`force=false` 普通退出, `force=true` 强杀).
fn quit_command(platform: &str, force: bool) -> Vec<String> {
    match (platform, force) {
        ("macos", false) => vec![
            "pkill".into(),
            "-TERM".into(),
            "-x".into(),
            MACOS_APP_NAME.into(),
        ],
        ("macos", true) => vec![
            "pkill".into(),
            "-KILL".into(),
            "-x".into(),
            MACOS_APP_NAME.into(),
        ],
        ("windows", false) => vec!["taskkill".into(), "/IM".into(), WINDOWS_PROCESS_NAME.into()],
        ("windows", true) => vec![
            "taskkill".into(),
            "/F".into(),
            "/IM".into(),
            WINDOWS_PROCESS_NAME.into(),
        ],
        (_, false) => vec![
            "pkill".into(),
            "-TERM".into(),
            "-x".into(),
            LINUX_BIN_NAME.into(),
        ],
        (_, true) => vec![
            "pkill".into(),
            "-KILL".into(),
            "-x".into(),
            LINUX_BIN_NAME.into(),
        ],
    }
}

/// 启动命令.macOS 优先用解析后的 .app 路径,fallback 到 `open -a Codex`
/// 让 LaunchServices 自己找。
///
/// `extra_args`: 附加给 Codex Desktop 本身的参数(如 `--remote-debugging-port=9222`)。
/// macOS 通过 `open` 的 `--args` 传递;Linux 直接追加到命令;Windows Store
/// 应用暂不支持命令行参数(忽略)。
fn open_command(
    platform: &str,
    resolved_macos_app: Option<&str>,
    extra_args: &[String],
) -> Vec<String> {
    match platform {
        // `-n`:即使 LaunchServices 缓存还以为 Codex 在运行,也强制启动一个新
        // 实例。我们刚 SIGTERM 杀过主进程,launchd 偶尔会在 reap 完成前误把
        // `open -a` 解读为 activate 已有实例 → 啥也不发生。`-n` 绕过这条。
        "macos" => {
            let mut cmd = vec![
                "open".into(),
                "-n".into(),
                "-a".into(),
                resolved_macos_app.unwrap_or(MACOS_APP_NAME).into(),
            ];
            if !extra_args.is_empty() {
                cmd.push("--args".into());
                cmd.extend(extra_args.iter().cloned());
            }
            cmd
        }
        "windows" => {
            // Windows Store 应用不支持通过 explorer.exe 传递命令行参数。
            // 如需调试端口，需用户手动修改快捷方式或使用其他启动方式。
            vec![
                "explorer.exe".into(),
                format!("shell:AppsFolder\\{WINDOWS_STORE_APP_ID}"),
            ]
        }
        _ => {
            let args_str = if extra_args.is_empty() {
                String::new()
            } else {
                format!(" {}", extra_args.join(" "))
            };
            vec![
                "sh".into(),
                "-c".into(),
                format!("{LINUX_BIN_NAME}{args_str} >/dev/null 2>&1 &"),
            ]
        }
    }
}

fn resolve_macos_app_path() -> Option<String> {
    let mut candidates = vec![PathBuf::from("/Applications/Codex.app")];
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join("Applications").join("Codex.app"));
    }
    candidates
        .into_iter()
        .find(|p| p.is_dir())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Windows 上给 Command 加 `CREATE_NO_WINDOW`(0x08000000)flag,避免每次
/// 调 `tasklist` / `taskkill` 都 flash 一个 console 黑框。其他平台 no-op。
/// 借鉴 codex-account-switch `src-tauri/win/runtime/process.rs::hide_console_window`。
#[cfg(target_os = "windows")]
fn hide_console_window(command: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
fn hide_console_window(command: &mut Command) -> &mut Command {
    command
}

fn is_codex_app_running(platform: &str) -> bool {
    let cmd = running_check_command(platform);
    let Some((program, args)) = cmd.split_first() else {
        return false;
    };
    if platform == "windows" {
        // tasklist 即使没匹配也 exit 0,要看 stdout 里有没有 process 名
        let mut command = Command::new(program);
        command.args(args);
        match hide_console_window(&mut command).output() {
            Ok(out) => String::from_utf8_lossy(&out.stdout)
                .to_ascii_lowercase()
                .contains(&WINDOWS_PROCESS_NAME.to_ascii_lowercase()),
            Err(_) => false,
        }
    } else {
        // pgrep:有进程 exit 0,没进程 exit 1
        Command::new(program)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

fn run_quit_command(platform: &str, force: bool) {
    let cmd = quit_command(platform, force);
    let Some((program, args)) = cmd.split_first() else {
        return;
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = hide_console_window(&mut command).status();
}

fn quit_codex_app_with_retries(platform: &str) -> Result<(), String> {
    if !is_codex_app_running(platform) {
        return Ok(());
    }
    run_quit_command(platform, false);
    for _ in 0..QUIT_TERM_POLL_ITERS {
        if !is_codex_app_running(platform) {
            return Ok(());
        }
        std::thread::sleep(QUIT_POLL_INTERVAL);
    }
    run_quit_command(platform, true);
    for _ in 0..QUIT_KILL_POLL_ITERS {
        if !is_codex_app_running(platform) {
            return Ok(());
        }
        std::thread::sleep(QUIT_POLL_INTERVAL);
    }
    Err("Codex 未能正常退出,请手动关闭后重试".to_owned())
}

/// 如果设置中开启了 autoWakeCodexPet，在启动 Codex 前将其全局状态中的
/// electron-avatar-overlay-open 设为 true，使宠物自动唤醒。
fn maybe_wake_codex_pet() {
    let cfg = match crate::admin::registry_io::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    let enabled = cfg
        .get("settings")
        .and_then(|s| s.get("autoWakeCodexPet"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    if !enabled {
        return;
    }
    let home = match codex_app_transfer_registry::paths::resolve_home() {
        Some(h) => h,
        None => return,
    };
    let path = home.join(".codex").join(".codex-global-state.json");
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut state: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };
    if let Some(obj) = state.as_object_mut() {
        obj.insert(
            "electron-avatar-overlay-open".to_string(),
            Value::Bool(true),
        );
    }
    let _ = fs::write(
        &path,
        serde_json::to_string_pretty(&state).unwrap_or_default(),
    );
}

/// 读取设置判断是否应附加调试端口参数
fn should_attach_debug_port() -> Vec<String> {
    match crate::admin::registry_io::load() {
        Ok(cfg) => {
            let auto_unlock = cfg
                .get("settings")
                .and_then(|s| s.get("autoUnlockCodexPlugins"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if auto_unlock {
                vec!["--remote-debugging-port=9222".into()]
            } else {
                vec![]
            }
        }
        Err(_) => vec![],
    }
}

fn open_codex_app(platform: &str) -> Result<(), String> {
    maybe_wake_codex_pet();
    let resolved = if platform == "macos" {
        resolve_macos_app_path()
    } else {
        None
    };
    let extra_args = should_attach_debug_port();
    let cmd = open_command(platform, resolved.as_deref(), &extra_args);
    let Some((program, args)) = cmd.split_first() else {
        return Err("open command is empty".to_owned());
    };
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console_window(&mut command)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot launch Codex App: {e}"))
}

fn launch_codex_app_restart(platform: &str) -> Result<(), String> {
    let was_running = is_codex_app_running(platform);
    quit_codex_app_with_retries(platform)?;
    // 退出确认后给 launchd 一段 grace 让它 reap 完旧进程,LaunchServices 才会
    // 把"Codex 在运行"的缓存清掉。否则紧跟的 `open -a` 会被当成 activate
    // 一个不存在的实例,啥也不发生(2026-05-06 现场实测)。
    // 跳过条件:本来就没在运行,根本不需要等。
    if was_running {
        std::thread::sleep(POST_QUIT_LAUNCHD_GRACE);
    }
    open_codex_app(platform)
}

pub(super) struct DesktopConfigTarget {
    pub(super) base_url: String,
    pub(super) api_key: String,
    pub(super) supports_1m: bool,
    pub(super) provider_name: String,
    pub(super) default_model: String,
    pub(super) model_mappings: Value,
    pub(super) model_capabilities: Value,
    pub(super) requires_proxy: bool,
    pub(super) mode: &'static str,
    pub(super) proxy_port: u16,
}

fn desktop_config_target_for_provider(
    cfg: &mut RawConfig,
    provider: &Value,
    proxy_port_override: Option<u16>,
) -> DesktopConfigTarget {
    let proxy_port = proxy_port_override.unwrap_or_else(|| read_proxy_port(cfg));

    // **bypass_proxy 模式**(2026-05-10):用户在「自定义第三方」preset 显式选
    // `apiFormat=responses` 协议,且填了 baseUrl + apiKey → Codex.app 直连上游,
    // 代理不参与转发(借鉴 codex-account-switch 的纯配置写入模式)。
    //
    // 适用范围:OpenAI 官方 / 任何原生实现 OpenAI Responses API 的反代或自建服务。
    // 触发条件:
    //   - apiFormat 严格等于 `responses` / `openai_responses`(anthropic /
    //     anthropic_messages / claude / messages 是 Anthropic Messages 路径
    //     → 继续走代理做本地协议转换)
    //   - baseUrl 与 apiKey 都非空(空了 direct 没法 work,fallback 到 local_proxy)
    //   - healing 命中 builtin preset 时强制覆盖 apiFormat=openai_chat,**builtin
    //     用户行为不变**(MiMo / Kimi / DeepSeek / MiniMax / 智谱 / 百炼 / Kimi Code 等)
    //
    // 历史教训(2026-05-08 MiMo Token Plan 404):v1.x 用 apiFormat=responses 当
    // "上游原生透传"隐式信号 → 用户配 MiMo 也被路由到 direct_provider → MiMo 上游
    // 没有 /responses 端点 → 必 404。此次设计的关键差异:**healing 已经把所有
    // builtin preset 强制覆盖回 openai_chat**,bypass 只可能命中显式自定义的
    // 第三方 provider —— 用户对此场景做出 informed choice。
    let api_format_lower = provider
        .get("apiFormat")
        .and_then(|v| v.as_str())
        .unwrap_or("openai_chat")
        .trim()
        .to_ascii_lowercase();
    let provider_base_url = provider
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_owned();
    let direct_api_key = provider_api_key(provider);
    let bypass_proxy = matches!(api_format_lower.as_str(), "responses" | "openai_responses")
        && !provider_base_url.is_empty()
        && !direct_api_key.is_empty();

    if bypass_proxy {
        return DesktopConfigTarget {
            base_url: provider_base_url,
            api_key: direct_api_key,
            supports_1m: provider_supports_1m(provider),
            provider_name: provider_display_name(provider),
            default_model: provider_default_model(provider),
            model_mappings: provider_model_mappings(provider),
            model_capabilities: provider_model_capabilities(provider),
            requires_proxy: false,
            mode: "direct",
            proxy_port,
        };
    }

    // 默认 local_proxy 模式:Codex.app → 127.0.0.1:18080 → 本地代理(协议转换 +
    // extras 注入 + model 改写 + vision 剥离 + namespace MCP 展平等)→ 上游。
    // 本项目核心价值在协议转换层,默认所有 provider 走代理,需要透传必须显式选。
    let base_url = format!("http://127.0.0.1:{proxy_port}");
    let api_key = ensure_gateway_key(cfg);
    DesktopConfigTarget {
        base_url,
        api_key,
        supports_1m: provider_supports_1m(provider),
        provider_name: provider_display_name(provider),
        default_model: provider_default_model(provider),
        model_mappings: provider_model_mappings(provider),
        model_capabilities: provider_model_capabilities(provider),
        requires_proxy: true,
        mode: "local_proxy",
        proxy_port,
    }
}

pub(super) fn desktop_target_for_active_provider(cfg: &RawConfig) -> Option<DesktopConfigTarget> {
    let provider = active_provider(cfg)?;
    let mut snapshot = cfg.clone();
    Some(desktop_config_target_for_provider(
        &mut snapshot,
        &provider,
        None,
    ))
}

fn desktop_expected_model_items(target: &DesktopConfigTarget) -> Vec<Value> {
    catalog_models_for_provider(
        &target.provider_name,
        &target.default_model,
        target.supports_1m,
        Some(&target.model_mappings),
        Some(&target.model_capabilities),
    )
    .into_iter()
    .map(|model| {
        let mut item = json!({
            "name": model.slug,
            "displayName": model.display_name,
        });
        if model.context_window >= ONE_M_CONTEXT_WINDOW {
            item["supports1m"] = Value::Bool(true);
        }
        item
    })
    .collect()
}

fn desktop_inference_models_json(target: Option<&DesktopConfigTarget>) -> String {
    let Some(target) = target else {
        return "[]".to_owned();
    };
    serde_json::to_string(&desktop_expected_model_items(target)).unwrap_or_else(|_| "[]".to_owned())
}

pub(super) fn read_codex_toml_root_string(paths: &CodexPaths, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(&paths.config_toml).ok()?;
    for line in content.lines() {
        let stripped = line.trim_start();
        if stripped.starts_with('[') {
            break;
        }
        if !stripped.starts_with(key) {
            continue;
        }
        let after = &stripped[key.len()..];
        let mut rest = after.trim_start();
        if !rest.starts_with('=') {
            continue;
        }
        rest = rest[1..].trim();
        if let Some(idx) = rest.find('#') {
            rest = rest[..idx].trim_end();
        }
        let trimmed = rest.trim_matches(|c: char| c == '"' || c == '\'');
        return Some(trimmed.to_owned());
    }
    None
}

pub(super) fn codex_openai_api_key_present(paths: &CodexPaths) -> bool {
    read_auth(&paths.auth_json)
        .ok()
        .and_then(|auth| {
            auth.get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .map(|s| !s.trim().is_empty())
        })
        .unwrap_or(false)
}

fn one_million_catalog_ready(paths: &CodexPaths, target: &DesktopConfigTarget) -> bool {
    let one_million_names: Vec<String> = desktop_expected_model_items(target)
        .into_iter()
        .filter_map(|item| {
            if item.get("supports1m").and_then(|v| v.as_bool()) == Some(true) {
                item.get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_owned())
            } else {
                None
            }
        })
        .collect();
    if one_million_names.is_empty() {
        return true;
    }

    let Some(catalog_path) = read_codex_toml_root_string(paths, "model_catalog_json") else {
        return false;
    };
    let catalog_path = PathBuf::from(catalog_path);
    // M3 (silent-failure-hunter review):catalog 读不到 / JSON 解析失败 → 通过
    // proxy_telemetry().logs 写日志面板可见(原 `.ok().and_then().unwrap_or_else(|| json!({}))`
    // 把"文件不存在"和"JSON 损坏"混吃成空对象 → 用户看到"未就绪"但分不清是配置
    // 缺失还是文件损坏)。
    //
    // **必须用 proxy_telemetry.logs 而非 `tracing::warn!`**:整个 workspace 没
    // `tracing_subscriber::*::init()`,Tauri 桌面用户不会从终端跑 binary,tracing
    // event 默认 drop,等于"假修复"。proxy_telemetry.logs 通道写 ~/.codex-app-transfer/
    // logs/proxy-*.log,设置面板 logs viewer 也能直接读。
    let telemetry = proxy_telemetry();
    let catalog: Value = match fs::read_to_string(&catalog_path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(v) => v,
            Err(e) => {
                telemetry.logs.add(
                    "WARN",
                    format!(
                        "one_million_catalog_ready: model_catalog JSON 解析失败 ({}): {e}",
                        catalog_path.display(),
                    ),
                );
                return false;
            }
        },
        Err(e) => {
            telemetry.logs.add(
                "WARN",
                format!(
                    "one_million_catalog_ready: model_catalog 文件读取失败 ({}): {e}",
                    catalog_path.display(),
                ),
            );
            return false;
        }
    };
    let Some(models) = catalog.get("models").and_then(|v| v.as_array()) else {
        return false;
    };
    models.iter().any(|item| {
        let slug = item
            .get("slug")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !one_million_names.iter().any(|name| name == slug) {
            return false;
        }
        let context_window = item
            .get("context_window")
            .or_else(|| item.get("max_context_window"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        context_window >= ONE_M_CONTEXT_WINDOW
    })
}

pub(super) fn desktop_health(
    paths: Option<&CodexPaths>,
    configured: bool,
    actual_base_url: Option<&str>,
    actual_api_key_present: bool,
    target: Option<&DesktopConfigTarget>,
) -> Value {
    let expected_base_url = target
        .map(|target| target.base_url.trim_end_matches('/').to_owned())
        .unwrap_or_default();
    let actual_base_url = actual_base_url
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_owned();
    let mut issues = Vec::new();

    if !configured {
        if !actual_base_url.is_empty() || actual_api_key_present {
            issues.push(json!({
                "code": "not_managed_by_cas",
                "message": "Current Codex CLI config was not written by the latest version of this tool.",
            }));
        } else {
            issues.push(json!({
                "code": "codex_snapshot_missing",
                "message": "Codex CLI config has not been applied by this tool — re-generate the config from the dashboard.",
            }));
        }
    }

    if !actual_base_url.is_empty()
        && !expected_base_url.is_empty()
        && actual_base_url != expected_base_url
    {
        issues.push(json!({
            "code": "gateway_base_url_mismatch",
            "message": "Codex CLI 仍指向旧地址，请重新一键生成 Codex CLI 配置。",
        }));
    }

    let one_million_ready = match (paths, target) {
        (Some(paths), Some(target)) => one_million_catalog_ready(paths, target),
        _ => true,
    };
    if !one_million_ready {
        issues.push(json!({
            "code": "one_million_not_written",
            "message": "1M 上下文模型尚未写入 Codex CLI 配置，请重新一键生成配置并重启终端。",
        }));
    }

    json!({
        "needsApply": !configured || !issues.is_empty(),
        "oneMillionReady": one_million_ready,
        "expectedBaseUrl": expected_base_url,
        "actualBaseUrl": actual_base_url,
        "mode": target.map(|target| target.mode),
        "requiresProxy": target.map(|target| target.requires_proxy).unwrap_or(false),
        "issues": issues,
    })
}

fn apply_desktop_target(target: &DesktopConfigTarget) -> Result<Value, String> {
    let paths = CodexPaths::from_home_env().map_err(|e| e.to_string())?;
    let result = apply_provider(
        &paths,
        &ApplyConfig {
            base_url: &target.base_url,
            gateway_api_key: &target.api_key,
            supports_1m: target.supports_1m,
            provider_name: &target.provider_name,
            default_model: &target.default_model,
            model_mappings: Some(&target.model_mappings),
            model_capabilities: Some(&target.model_capabilities),
            app_version: APP_VERSION,
        },
    )
    .map_err(|e| format!("apply 失败: {e}"))?;
    serde_json::to_value(result).map_err(|e| format!("apply 结果序列化失败: {e}"))
}

async fn sync_desktop_for_active_provider(state: &AdminState) -> Value {
    // RMW atomic — load + mutate (desktop_config_target_for_provider 修改 cfg)
    // + save 必须在同 lock 内,防与并发 form save / OAuth sync 互相覆盖
    let target_result = with_config_write(|cfg| {
        let Some(provider) = active_provider(cfg) else {
            return Err("no default provider".into());
        };
        let target = desktop_config_target_for_provider(cfg, &provider, None);
        Ok(ConfigMutation::Modified(target))
    });
    let target = match target_result {
        Ok(t) => t,
        Err(e) if e == "no default provider" => {
            return json!({
                "attempted": false,
                "success": false,
                "message": e,
            });
        }
        Err(e) => return json!({"attempted": true, "success": false, "message": e}),
    };

    let mut proxy_started = false;
    if target.requires_proxy {
        match start_proxy_if_needed(&state.proxy_manager, target.proxy_port).await {
            Ok(started) => proxy_started = started,
            Err(e) => {
                return json!({"attempted": true, "success": false, "mode": target.mode, "requiresProxy": target.requires_proxy, "message": e});
            }
        }
    } else {
        state.proxy_manager.stop_silent();
    }

    match apply_desktop_target(&target) {
        Ok(mut result) => {
            if let Some(obj) = result.as_object_mut() {
                obj.insert("attempted".into(), Value::Bool(true));
                obj.insert("success".into(), Value::Bool(true));
                obj.insert("mode".into(), Value::String(target.mode.to_owned()));
                obj.insert("requiresProxy".into(), Value::Bool(target.requires_proxy));
                obj.insert("proxyStarted".into(), Value::Bool(proxy_started));
            }
            result
        }
        Err(e) => {
            json!({"attempted": true, "success": false, "mode": target.mode, "requiresProxy": target.requires_proxy, "proxyStarted": proxy_started, "message": e})
        }
    }
}

pub async fn auto_apply_on_startup_if_enabled(proxy_manager: Arc<ProxyManager>) -> Value {
    let cfg = match load_registry() {
        Ok(cfg) => cfg,
        Err(e) => {
            return json!({"applied": false, "requiresProxy": false, "proxyStarted": false, "message": format!("failed: {e}")})
        }
    };
    if !read_setting_bool(&cfg, "autoApplyOnStart", true) {
        return json!({"applied": false, "requiresProxy": false, "proxyStarted": false, "message": "disabled by settings"});
    }
    if active_provider(&cfg).is_none() {
        return json!({"applied": false, "requiresProxy": false, "proxyStarted": false, "message": "no active provider; skip"});
    }
    let state = AdminState { proxy_manager };
    let result = sync_desktop_for_active_provider(&state).await;
    if result.get("success").and_then(|v| v.as_bool()) == Some(true) {
        return json!({
            "applied": true,
            "requiresProxy": result.get("requiresProxy").and_then(|v| v.as_bool()).unwrap_or(false),
            "proxyStarted": result.get("proxyStarted").and_then(|v| v.as_bool()).unwrap_or(false),
            "message": format!("applied {}", active_provider_name(&cfg)),
        });
    }
    json!({
        "applied": false,
        "requiresProxy": result.get("requiresProxy").and_then(|v| v.as_bool()).unwrap_or(false),
        "proxyStarted": result.get("proxyStarted").and_then(|v| v.as_bool()).unwrap_or(false),
        "message": format!("failed: {}", result.get("message").and_then(|v| v.as_str()).unwrap_or("unknown")),
    })
}

pub fn restore_codex_if_enabled(reason: &str) -> Value {
    let cfg = match load_registry() {
        Ok(cfg) => cfg,
        Err(e) => {
            return json!({"attempted": true, "restored": false, "success": false, "reason": reason, "message": e})
        }
    };
    if !read_setting_bool(&cfg, "restoreCodexOnExit", true) {
        return json!({"attempted": false, "restored": false, "success": true, "reason": reason, "message": "disabled by settings"});
    }
    let paths = match CodexPaths::from_home_env() {
        Ok(paths) => paths,
        Err(e) => {
            return json!({"attempted": true, "restored": false, "success": false, "reason": reason, "message": e.to_string()})
        }
    };
    if !has_snapshot(&paths) {
        return json!({"attempted": false, "restored": false, "success": true, "reason": reason, "message": "no snapshot; skip"});
    }
    match restore_codex_state(&paths) {
        Ok(restored) => {
            json!({"attempted": true, "restored": restored, "success": true, "reason": reason})
        }
        Err(e) => {
            json!({"attempted": true, "restored": false, "success": false, "reason": reason, "message": e.to_string()})
        }
    }
}

pub async fn switch_provider_and_sync(
    proxy_manager: Arc<ProxyManager>,
    provider_id: String,
) -> Value {
    let result = with_config_write(|cfg| {
        if provider_index(cfg, &provider_id).is_none() {
            return Err("provider not found".into());
        }
        cfg.as_object_mut()
            .unwrap()
            .insert("activeProvider".into(), Value::String(provider_id.clone()));
        Ok(ConfigMutation::Modified(()))
    });
    if let Err(e) = result {
        return json!({"success": false, "message": e});
    }
    let state = AdminState { proxy_manager };
    let desktop_sync = sync_desktop_for_active_provider(&state).await;
    json!({
        "success": true,
        "message": "default provider updated",
        "desktopSync": desktop_sync,
    })
}

// ── /api/desktop/* ───────────────────────────────────────────────────

pub async fn desktop_status() -> impl IntoResponse {
    let paths = match CodexPaths::from_home_env() {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let configured = has_snapshot(&paths);
    let cfg = load_registry().unwrap_or_else(|_| json!({}));
    let proxy_port = read_proxy_port(&cfg);
    let actual_base_url = read_codex_toml_root_string(&paths, "openai_base_url");
    let actual_api_key_present = codex_openai_api_key_present(&paths);
    let desktop_target = desktop_target_for_active_provider(&cfg);
    let fallback_base_url = desktop_target
        .as_ref()
        .map(|target| target.base_url.clone())
        .unwrap_or_else(|| format!("http://127.0.0.1:{proxy_port}"));
    let api_key_present = actual_api_key_present
        || desktop_target
            .as_ref()
            .map(|target| !target.api_key.is_empty())
            .unwrap_or_else(|| !read_gateway_key(&cfg).is_empty());
    let health = desktop_health(
        Some(&paths),
        configured,
        actual_base_url.as_deref(),
        actual_api_key_present,
        desktop_target.as_ref(),
    );
    Json(json!({
        "configured": configured,
        "health": health,
        "keys": {
            "inferenceProvider": "gateway",
            "inferenceGatewayBaseUrl": actual_base_url.unwrap_or(fallback_base_url),
            "inferenceGatewayApiKey": if api_key_present { "******" } else { "" },
            "inferenceGatewayAuthScheme": "bearer",
            "inferenceModels": desktop_inference_models_json(desktop_target.as_ref()),
        },
    }))
    .into_response()
}

pub async fn desktop_configure() -> impl IntoResponse {
    let target_result = with_config_write(|cfg| {
        let Some(active) = active_provider(cfg) else {
            return Err("add a provider first".into());
        };
        let target = desktop_config_target_for_provider(cfg, &active, None);
        Ok(ConfigMutation::Modified(target))
    });
    let target = match target_result {
        Ok(t) => t,
        Err(e) if e == "add a provider first" => {
            return err(StatusCode::BAD_REQUEST, e).into_response();
        }
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    };
    match apply_desktop_target(&target) {
        Ok(mut result) => {
            if let Some(obj) = result.as_object_mut() {
                obj.insert("success".into(), Value::Bool(true));
                obj.insert("mode".into(), Value::String(target.mode.to_owned()));
                obj.insert("requiresProxy".into(), Value::Bool(target.requires_proxy));
            }
            Json(result).into_response()
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

pub async fn desktop_clear() -> impl IntoResponse {
    let paths = match CodexPaths::from_home_env() {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match restore_codex_state(&paths) {
        Ok(restored) => Json(json!({"success": true, "restored": restored})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn desktop_snapshots() -> impl IntoResponse {
    let paths = match CodexPaths::from_home_env() {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    Json(json!({
        "snapshots": list_snapshots(&paths),
    }))
    .into_response()
}

pub async fn desktop_restore(Json(payload): Json<DesktopRestoreRequest>) -> impl IntoResponse {
    let paths = match CodexPaths::from_home_env() {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let snapshot_id = payload.snapshot_id.unwrap_or_default();
    match restore_codex_snapshot(&paths, &snapshot_id, payload.cleanup_all) {
        Ok(restored) => Json(json!({
            "success": true,
            "restored": restored,
            "snapshotId": if snapshot_id.is_empty() { Value::Null } else { Value::String(snapshot_id) },
            "cleanupAll": payload.cleanup_all,
        }))
        .into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn desktop_snapshot_status() -> impl IntoResponse {
    let paths = match CodexPaths::from_home_env() {
        Ok(p) => p,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let status = get_snapshot_status(&paths);
    Json(json!({
        "hasSnapshot": status.has_snapshot,
        "snapshotAt": status.snapshot_at,
        "configExisted": status.config_existed,
        "authExisted": status.auth_existed,
        "appVersion": status.app_version,
        "restorableCount": status.restorable_count,
        "recoveryCount": status.recovery_count,
    }))
    .into_response()
}

pub async fn restart_codex_app() -> impl IntoResponse {
    match launch_codex_app_restart(std::env::consts::OS) {
        Ok(_) => Json(json!({"success": true})).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::super::common::test_support::with_isolated_home;

    fn config_with_secret() -> Value {
        json!({
            "version": APP_VERSION,
            "activeProvider": "p1",
            "gatewayApiKey": "cas_existing",
            "providers": [{
                "id": "p1",
                "name": "Provider One",
                "baseUrl": "https://api.example.com/v1",
                "authScheme": "bearer",
                "apiFormat": "openai_chat",
                "apiKey": "sk-existing",
                "extraHeaders": {"x-extra-secret": "secret-header"},
                "models": {"default": "model-one"},
                "sortIndex": 0
            }],
            "settings": {
                "theme": "default",
                "language": "zh",
                "proxyPort": 18080,
                "adminPort": 18081,
                "autoStart": false,
                "autoApplyOnStart": true,
                "exposeAllProviderModels": false,
                "restoreCodexOnExit": true,
                "updateUrl": DEFAULT_UPDATE_URL
            }
        })
    }

    fn agent_debug_log(hypothesis_id: &str, location: &str, message: &str, data: Value) {
        let payload = json!({
            "sessionId": "bf3f9f",
            "runId": "pre-fix",
            "hypothesisId": hypothesis_id,
            "location": location,
            "message": message,
            "data": data,
            "timestamp": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/Users/alysechen/alysechen/github/codex-app-transfer/.cursor/debug-bf3f9f.log")
        {
            let _ = writeln!(f, "{}", payload);
        }
    }

    #[test]
    fn running_check_command_is_platform_specific() {
        assert_eq!(running_check_command("macos"), vec!["pgrep", "-x", "Codex"]);
        let windows = running_check_command("windows");
        assert_eq!(windows[0], "tasklist");
        assert!(windows.iter().any(|a| a == "IMAGENAME eq Codex.exe"));
        assert_eq!(running_check_command("linux"), vec!["pgrep", "-x", "codex"]);
    }

    #[test]
    fn quit_command_uses_term_then_kill() {
        // graceful = SIGTERM / 普通 taskkill;force = SIGKILL / taskkill /F
        assert_eq!(
            quit_command("macos", false),
            vec!["pkill", "-TERM", "-x", "Codex"]
        );
        assert_eq!(
            quit_command("macos", true),
            vec!["pkill", "-KILL", "-x", "Codex"]
        );
        assert_eq!(
            quit_command("windows", false),
            vec!["taskkill", "/IM", "Codex.exe"]
        );
        assert_eq!(
            quit_command("windows", true),
            vec!["taskkill", "/F", "/IM", "Codex.exe"]
        );
        assert_eq!(
            quit_command("linux", false),
            vec!["pkill", "-TERM", "-x", "codex"]
        );
        assert_eq!(
            quit_command("linux", true),
            vec!["pkill", "-KILL", "-x", "codex"]
        );
    }

    #[test]
    fn open_command_uses_resolved_path_when_available() {
        // macOS 必带 `-n` 强制新实例(重启场景下 LaunchServices 缓存仍以为
        // 旧 Codex 在运行,不加 -n 会让 `open -a` 静默 no-op)。
        assert_eq!(
            open_command("macos", Some("/Applications/Codex.app"), &[]),
            vec!["open", "-n", "-a", "/Applications/Codex.app"]
        );
        // 落空时回到裸 app 名,让 LaunchServices 找
        assert_eq!(
            open_command("macos", None, &[]),
            vec!["open", "-n", "-a", "Codex"]
        );
        // 带调试参数时通过 --args 传递
        assert_eq!(
            open_command("macos", None, &["--remote-debugging-port=9222".into()]),
            vec![
                "open",
                "-n",
                "-a",
                "Codex",
                "--args",
                "--remote-debugging-port=9222"
            ]
        );
        let windows = open_command("windows", None, &[]);
        assert_eq!(windows[0], "explorer.exe");
        assert!(windows[1].starts_with("shell:AppsFolder\\"));
        assert!(windows[1].contains("OpenAI.Codex"));
        let linux = open_command("linux", None, &[]);
        assert_eq!(linux[0], "sh");
        assert_eq!(linux[1], "-c");
        assert!(linux[2].contains("codex"));
    }

    #[test]
    fn desktop_config_target_matches_legacy_proxy_and_direct_modes() {
        let mut proxy_cfg = config_with_secret();
        proxy_cfg["gatewayApiKey"] = Value::Null;
        let proxy_provider = active_provider(&proxy_cfg).unwrap();
        let proxy_target =
            desktop_config_target_for_provider(&mut proxy_cfg, &proxy_provider, Some(19090));
        assert_eq!(proxy_target.mode, "local_proxy");
        assert!(proxy_target.requires_proxy);
        assert_eq!(proxy_target.base_url, "http://127.0.0.1:19090");
        assert!(proxy_target.api_key.starts_with("cas_"));
        assert_eq!(
            proxy_cfg
                .get("gatewayApiKey")
                .and_then(|v| v.as_str())
                .unwrap(),
            proxy_target.api_key
        );

        // 关键设计(2026-05-10):自定义第三方 provider 显式选 apiFormat=responses
        // + 填 baseUrl + apiKey → 走 direct mode,Codex.app 直连上游不经代理。
        // 历史 v1.x 用 apiFormat=responses 隐式信号导致 MiMo 404 那次教训仍然
        // 生效:healing 强制把 builtin preset 的 apiFormat 覆盖回 openai_chat
        // (Kimi / DeepSeek / MiMo / MiniMax 等),所以 direct 只可能命中**显式
        // 自定义 + 用户主动选择透传**的第三方场景。
        let mut direct_cfg = config_with_secret();
        direct_cfg["gatewayApiKey"] = Value::Null;
        let direct_provider = json!({
            "id": "custom-third-party-instance",
            "name": "Custom Third-Party (Direct)",
            "baseUrl": "https://direct.example.com/v1/",
            "authScheme": "bearer",
            "apiFormat": "responses",  // 显式选透传 → bypass proxy
            "apiKey": "sk-direct",
            "models": {"default": "direct-model"},
        });
        let target =
            desktop_config_target_for_provider(&mut direct_cfg, &direct_provider, Some(19090));
        assert_eq!(
            target.mode, "direct",
            "apiFormat=responses + 自定义第三方 + 填齐 baseUrl/apiKey → direct 透传"
        );
        assert!(!target.requires_proxy, "direct 模式不启动本地代理");
        assert_eq!(
            target.base_url, "https://direct.example.com/v1/",
            "config.toml 直接指向用户填的上游 baseUrl"
        );
        assert_eq!(
            target.api_key, "sk-direct",
            "auth.json 直接写用户填的 apiKey"
        );
        // bypass 时不调 ensure_gateway_key,gatewayApiKey 应保持 null
        assert!(direct_cfg
            .get("gatewayApiKey")
            .map(|v| v.is_null())
            .unwrap_or(false));
    }

    #[test]
    fn responses_format_without_apikey_falls_back_to_local_proxy() {
        // 防御性回归:apiFormat=responses 但 apiKey 为空 → 不进 direct 分支
        // (没 key 直连必失败,fallback 到 local_proxy 让用户看清是 key 缺失)
        let mut cfg = config_with_secret();
        let provider = json!({
            "id": "incomplete-direct",
            "name": "Incomplete Direct",
            "baseUrl": "https://direct.example.com/v1/",
            "authScheme": "bearer",
            "apiFormat": "responses",
            "apiKey": "",  // ← 空 key
            "models": {"default": "x"},
        });
        let target = desktop_config_target_for_provider(&mut cfg, &provider, Some(19090));
        assert_eq!(target.mode, "local_proxy", "apiKey 空时 fallback");
        assert!(target.requires_proxy);
    }

    #[test]
    fn responses_format_without_baseurl_falls_back_to_local_proxy() {
        // 同上:baseUrl 为空 → fallback 到 local_proxy
        let mut cfg = config_with_secret();
        let provider = json!({
            "id": "incomplete-direct-2",
            "name": "Incomplete Direct 2",
            "baseUrl": "",  // ← 空 baseUrl
            "authScheme": "bearer",
            "apiFormat": "responses",
            "apiKey": "sk-x",
            "models": {"default": "x"},
        });
        let target = desktop_config_target_for_provider(&mut cfg, &provider, Some(19090));
        assert_eq!(target.mode, "local_proxy");
        assert!(target.requires_proxy);
    }

    #[test]
    fn anthropic_aliases_never_bypass_proxy() {
        // 防回归:`anthropic` / `claude` / `messages` 是 Python 历史兼容值,
        // canonical `anthropic_messages` 也必须继续走 local_proxy 做本地协议转换;
        // direct 分支只放行 `responses` / `openai_responses`。如未来误把
        // anthropic 加进 bypass match
        // → 复活 v1.x MiMo 404 类回归(Codex.app 直连第三方上游 /responses → 必 404)。
        for fmt in [
            "anthropic",
            "anthropic_messages",
            "claude",
            "messages",
            "claude_messages",
        ] {
            let mut cfg = config_with_secret();
            let provider = json!({
                "id": "anthropic-aliased",
                "name": "Anthropic Aliased",
                "baseUrl": "https://anthropic-style.example.com/v1/",
                "authScheme": "bearer",
                "apiFormat": fmt,
                "apiKey": "sk-x",
                "models": {"default": "claude-sonnet"},
            });
            let target = desktop_config_target_for_provider(&mut cfg, &provider, Some(19090));
            assert_eq!(
                target.mode, "local_proxy",
                "{fmt} 必须走代理协议转换,不能进 bypass"
            );
            assert!(target.requires_proxy, "{fmt} 必须 requires_proxy=true");
        }
    }

    #[test]
    fn openai_responses_alias_triggers_direct_mode() {
        // 防回归:registry.rs::lookup 已支持 `openai_responses` 别名,
        // desktop bypass 分支必须同样支持(独立 match 分支,registry 测试不传递)。
        let mut cfg = config_with_secret();
        let provider = json!({
            "id": "alias-direct",
            "name": "Alias Direct",
            "baseUrl": "https://api.openai.com/v1/",
            "authScheme": "bearer",
            "apiFormat": "openai_responses",
            "apiKey": "sk-direct",
            "models": {"default": "gpt-5"},
        });
        let target = desktop_config_target_for_provider(&mut cfg, &provider, Some(19090));
        assert_eq!(
            target.mode, "direct",
            "openai_responses 别名必须跟 responses 同样进 bypass"
        );
        assert!(!target.requires_proxy);
        assert_eq!(target.base_url, "https://api.openai.com/v1/");
        assert_eq!(target.api_key, "sk-direct");
    }

    #[test]
    fn switch_back_to_builtin_restarts_proxy_and_repoints_config() {
        // 防回归:用户工作流 "加 OpenAI direct 试用 → 切回 Kimi"。反向切换
        // 必须:① config.toml 重新指向 127.0.0.1:proxy_port;② auth.json 写
        // gateway key(不再是 sk-direct);③ 代理 manager 重新 start。
        // 若反向切换 config 没重写 / 代理没重启 → Codex.app 仍连 OpenAI baseUrl
        // 但带 Kimi key → 全爆。
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        with_isolated_home(|home| {
            runtime.block_on(async {
                let mut cfg = config_with_secret();
                cfg["settings"]["proxyPort"] = json!(0);
                cfg["providers"] = json!([
                    cfg["providers"][0].clone(),  // p1: builtin (openai_chat)
                    {
                        "id": "p2",
                        "name": "Custom Direct",
                        "baseUrl": "https://direct.example.com/v1/",
                        "authScheme": "bearer",
                        "apiFormat": "responses",
                        "apiKey": "sk-direct",
                        "models": {"default": "direct-model"},
                        "sortIndex": 1
                    }
                ]);
                save_registry(&cfg).unwrap();
                fs::create_dir_all(home.join(".codex")).unwrap();

                let manager = Arc::new(ProxyManager::new());

                // Step 1: 切到 p2 (direct mode) — config 应指向 direct.example.com
                let r1 = switch_provider_and_sync(Arc::clone(&manager), "p2".to_owned()).await;
                assert_eq!(r1["desktopSync"]["mode"], json!("direct"));
                assert!(!manager.status().running, "direct 模式不启动代理");
                let toml1 = fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
                assert!(toml1.contains("direct.example.com"));

                // Step 2: 切回 p1 (builtin local_proxy) — config 应重新指向 127.0.0.1
                let p1_id = cfg["providers"][0]["id"].as_str().unwrap().to_owned();
                let r2 = switch_provider_and_sync(Arc::clone(&manager), p1_id).await;
                assert_eq!(r2["desktopSync"]["mode"], json!("local_proxy"));
                assert!(manager.status().running, "切回 builtin 必须重启代理");
                let toml2 = fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
                assert!(
                    toml2.contains("openai_base_url = \"http://127.0.0.1:"),
                    "config.toml 必须重新指向 127.0.0.1,实际:\n{toml2}"
                );
                assert!(
                    !toml2.contains("direct.example.com"),
                    "禁止残留 direct 上游 URL:\n{toml2}"
                );
                let auth_json: Value = serde_json::from_str(
                    &fs::read_to_string(home.join(".codex").join("auth.json")).unwrap(),
                )
                .unwrap();
                let api_key = auth_json["OPENAI_API_KEY"].as_str().unwrap_or_default();
                assert_ne!(
                    api_key, "sk-direct",
                    "auth.json 不能残留 direct 时的 provider apiKey"
                );
                assert!(
                    !api_key.is_empty(),
                    "auth.json 必须有 gateway key(local_proxy 模式)"
                );

                manager.stop_silent();
            });
        });
    }

    #[test]
    fn startup_auto_apply_starts_proxy_and_exit_restore_uses_snapshot() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        with_isolated_home(|home| {
            runtime.block_on(async {
                let mut cfg = config_with_secret();
                cfg["gatewayApiKey"] = Value::Null;
                cfg["settings"]["proxyPort"] = json!(0);
                save_registry(&cfg).unwrap();

                let codex_dir = home.join(".codex");
                fs::create_dir_all(&codex_dir).unwrap();
                let config_toml = codex_dir.join("config.toml");
                fs::write(&config_toml, "approval_policy = \"on-request\"\n").unwrap();

                let manager = Arc::new(ProxyManager::new());
                let result = auto_apply_on_startup_if_enabled(Arc::clone(&manager)).await;
                assert_eq!(result["applied"], json!(true));
                assert_eq!(result["requiresProxy"], json!(true));
                assert_eq!(result["proxyStarted"], json!(true));
                assert!(manager.status().running);

                let saved = load_registry().unwrap();
                assert!(saved
                    .get("gatewayApiKey")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .starts_with("cas_"));
                let paths = CodexPaths::from_home_env().unwrap();
                assert!(has_snapshot(&paths));
                let applied_config = fs::read_to_string(&config_toml).unwrap();
                assert!(applied_config.contains("approval_policy = \"on-request\""));
                assert!(applied_config.contains("openai_base_url = \"http://127.0.0.1:0\""));

                let restored = restore_codex_if_enabled("test-exit");
                assert_eq!(restored["success"], json!(true));
                assert_eq!(restored["attempted"], json!(true));
                assert!(!has_snapshot(&paths));
                let restored_config = fs::read_to_string(&config_toml).unwrap();
                assert!(restored_config.contains("approval_policy = \"on-request\""));
                assert!(!restored_config.contains("openai_base_url"));
                manager.stop_silent();
            });
        });
    }

    #[test]
    fn startup_auto_apply_respects_disabled_setting() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        with_isolated_home(|_| {
            runtime.block_on(async {
                let mut cfg = config_with_secret();
                cfg["settings"]["autoApplyOnStart"] = json!(false);
                save_registry(&cfg).unwrap();

                let manager = Arc::new(ProxyManager::new());
                let result = auto_apply_on_startup_if_enabled(Arc::clone(&manager)).await;
                assert_eq!(result["applied"], json!(false));
                assert_eq!(result["message"], json!("disabled by settings"));
                assert!(!manager.status().running);
            });
        });
    }

    #[test]
    fn provider_switch_syncs_desktop_via_direct_when_apiformat_is_responses() {
        // 关键设计(2026-05-10):自定义第三方 provider 显式选 apiFormat=responses
        // + 填齐 baseUrl + apiKey 的场景下,切换默认 provider + 同步 desktop 应:
        // 1. config.toml 写**用户填的 baseUrl**(不是 127.0.0.1:18080)
        // 2. auth.json 写**用户填的 apiKey**(不是 gateway key)
        // 3. **不启动本地代理**(requires_proxy=false → stop_silent)
        //
        // 这是 codex-account-switch 风格的纯配置切换,Codex.app 直连上游。适用
        // OpenAI 官方 / 任何原生实现 Responses API 的反代。
        //
        // 历史教训仍生效:builtin preset(MiMo / Kimi / DeepSeek / MiniMax 等)
        // 即使用户改 config.json apiFormat=responses,healing 启动时强制覆盖回
        // openai_chat → 它们仍走 local_proxy(本测试 case 是显式自定义 provider,
        // baseUrl=direct.example.com 不命中任何 builtin)。
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        with_isolated_home(|home| {
            runtime.block_on(async {
                let mut cfg = config_with_secret();
                // 让 OS 自选空闲端口,避免与并发其它测试用 18080 冲突
                cfg["settings"]["proxyPort"] = json!(0);
                cfg["providers"] = json!([
                    cfg["providers"][0].clone(),
                    {
                        "id": "p2",
                        "name": "Custom Third-Party (Direct)",
                        "baseUrl": "https://direct.example.com/v1/",
                        "authScheme": "bearer",
                        // apiFormat=responses + 填齐 baseUrl/apiKey → direct 透传
                        "apiFormat": "responses",
                        "apiKey": "sk-direct",
                        "models": {"default": "direct-model"},
                        "sortIndex": 1
                    }
                ]);
                save_registry(&cfg).unwrap();
                fs::create_dir_all(home.join(".codex")).unwrap();

                let manager = Arc::new(ProxyManager::new());
                let result = switch_provider_and_sync(Arc::clone(&manager), "p2".to_owned()).await;
                assert_eq!(result["success"], json!(true));
                assert_eq!(result["desktopSync"]["success"], json!(true));
                assert_eq!(result["desktopSync"]["mode"], json!("direct"));
                assert_eq!(result["desktopSync"]["requiresProxy"], json!(false));
                // direct 模式不启动本地代理
                assert!(
                    !manager.status().running,
                    "direct 模式必须不启动代理(stop_silent)"
                );

                let saved = load_registry().unwrap();
                assert_eq!(saved["activeProvider"], json!("p2"));
                // Codex CLI 直接指向用户填的上游 baseUrl
                let config_toml =
                    fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
                assert!(
                    config_toml.contains("openai_base_url = \"https://direct.example.com/v1/\""),
                    "config.toml 必须指向用户填的上游 baseUrl,实际:\n{config_toml}"
                );
                assert!(
                    !config_toml.contains("127.0.0.1"),
                    "direct 模式禁止指向 127.0.0.1:\n{config_toml}"
                );
                // auth.json 写用户填的 apiKey
                let auth_json: Value = serde_json::from_str(
                    &fs::read_to_string(home.join(".codex").join("auth.json")).unwrap(),
                )
                .unwrap();
                let api_key = auth_json["OPENAI_API_KEY"].as_str().unwrap_or_default();
                assert_eq!(
                    api_key, "sk-direct",
                    "auth.json 必须写用户填的 provider apiKey"
                );
            });
        });
    }

    #[test]
    fn desktop_inference_models_use_current_codex_catalog_slots() {
        let mut cfg = config_with_secret();
        cfg["providers"][0]["models"] = json!({
            "default": "deepseek-v4-pro[1m]",
            "gpt_5_5": "kimi-k2",
            "gpt_5_4": "glm-4.6",
        });
        let provider = active_provider(&cfg).unwrap();
        let target = desktop_config_target_for_provider(&mut cfg, &provider, Some(19090));
        let raw = desktop_inference_models_json(Some(&target));

        assert!(!raw.contains("sonnet"));
        assert!(!raw.contains("haiku"));
        assert!(!raw.contains("opus"));

        let models: Vec<Value> = serde_json::from_str(&raw).unwrap();
        let names: Vec<&str> = models
            .iter()
            .filter_map(|item| item.get("name").and_then(|v| v.as_str()))
            .collect();
        assert!(names.contains(&"gpt-5.5"));
        assert!(names.contains(&"gpt-5.4"));
        assert!(names.contains(&"gpt-5.4-mini"));
        assert!(names.contains(&"deepseek-v4-pro"));
        assert!(models
            .iter()
            .any(|item| item.get("supports1m").and_then(|v| v.as_bool()) == Some(true)));
    }

    #[test]
    fn desktop_status_reports_current_models_and_health_issues() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        with_isolated_home(|home| {
            runtime.block_on(async {
                let mut cfg = config_with_secret();
                cfg["providers"][0]["models"] = json!({
                    "default": "deepseek-v4-pro[1m]",
                    "gpt_5_5": "kimi-k2",
                });
                save_registry(&cfg).unwrap();

                let codex_dir = home.join(".codex");
                fs::create_dir_all(&codex_dir).unwrap();
                fs::write(
                    codex_dir.join("config.toml"),
                    "openai_base_url = \"http://127.0.0.1:18080\"\n",
                )
                .unwrap();
                fs::write(
                    codex_dir.join("auth.json"),
                    "{\"OPENAI_API_KEY\":\"cas_existing\"}\n",
                )
                .unwrap();

                let response = desktop_status().await.into_response();
                assert_eq!(response.status(), StatusCode::OK);
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let payload: Value = serde_json::from_slice(&body).unwrap();

                let models_raw = payload["keys"]["inferenceModels"].as_str().unwrap();
                assert!(!models_raw.contains("sonnet"));
                assert!(models_raw.contains("gpt-5.5"));
                assert!(models_raw.contains("deepseek-v4-pro"));
                assert_eq!(payload["configured"], json!(false));
                assert_eq!(payload["health"]["needsApply"], json!(true));
                assert_eq!(payload["health"]["oneMillionReady"], json!(false));

                let codes: Vec<&str> = payload["health"]["issues"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|issue| issue.get("code").and_then(|v| v.as_str()))
                    .collect();
                assert!(codes.contains(&"not_managed_by_cas"));
                assert!(codes.contains(&"one_million_not_written"));
            });
        });
    }

    #[test]
    fn desktop_health_reports_base_url_mismatch() {
        with_isolated_home(|home| {
            let cfg = config_with_secret();
            let provider = active_provider(&cfg).unwrap();
            let mut target_cfg = cfg.clone();
            let target =
                desktop_config_target_for_provider(&mut target_cfg, &provider, Some(19090));

            let codex_dir = home.join(".codex");
            fs::create_dir_all(&codex_dir).unwrap();
            fs::write(
                codex_dir.join("config.toml"),
                "openai_base_url = \"http://127.0.0.1:18080\"\n",
            )
            .unwrap();
            fs::write(
                codex_dir.join("auth.json"),
                "{\"OPENAI_API_KEY\":\"cas_old\"}\n",
            )
            .unwrap();

            let paths = CodexPaths::from_home_env().unwrap();
            let actual_base_url = read_codex_toml_root_string(&paths, "openai_base_url");
            let health = desktop_health(
                Some(&paths),
                false,
                actual_base_url.as_deref(),
                true,
                Some(&target),
            );

            assert_eq!(health["needsApply"], json!(true));
            assert_eq!(health["expectedBaseUrl"], json!("http://127.0.0.1:19090"));
            assert_eq!(health["actualBaseUrl"], json!("http://127.0.0.1:18080"));
            let codes: Vec<&str> = health["issues"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|issue| issue.get("code").and_then(|v| v.as_str()))
                .collect();
            assert!(codes.contains(&"not_managed_by_cas"));
            assert!(codes.contains(&"gateway_base_url_mismatch"));
        });
    }

    #[test]
    fn qwen_local_proxy_apply_writes_codex_auth_and_base_url() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        with_isolated_home(|home| {
            runtime.block_on(async {
                let mut cfg = config_with_secret();
                cfg["activeProvider"] = json!("qwen");
                cfg["gatewayApiKey"] = Value::Null;
                cfg["settings"]["proxyPort"] = json!(19091);
                cfg["providers"] = json!([{
                    "id": "qwen",
                    "name": "Qwen",
                    "baseUrl": "https://dashscope.aliyuncs.com/compatible-mode/v1",
                    "authScheme": "bearer",
                    "apiFormat": "openai_chat",
                    "apiKey": "sk-qwen-upstream",
                    "models": {"default": "qwen3.6-plus"},
                    "sortIndex": 0
                }]);
                save_registry(&cfg).unwrap();
                fs::create_dir_all(home.join(".codex")).unwrap();

                // #region agent log
                agent_debug_log(
                    "H1",
                    "src-tauri/src/admin/handlers/desktop.rs:qwen_local_proxy_apply_writes_codex_auth_and_base_url:before_sync",
                    "prepared qwen provider config",
                    json!({
                        "activeProvider": "qwen",
                        "proxyPort": 19091,
                        "providerModel": "qwen3.6-plus",
                        "gatewayApiKeyNull": true
                    }),
                );
                // #endregion

                let manager = Arc::new(ProxyManager::new());
                let result = switch_provider_and_sync(Arc::clone(&manager), "qwen".to_owned()).await;

                // #region agent log
                agent_debug_log(
                    "H1",
                    "src-tauri/src/admin/handlers/desktop.rs:qwen_local_proxy_apply_writes_codex_auth_and_base_url:after_sync",
                    "desktop sync result for qwen",
                    json!({
                        "success": result["success"],
                        "desktopSyncSuccess": result["desktopSync"]["success"],
                        "desktopMode": result["desktopSync"]["mode"],
                        "requiresProxy": result["desktopSync"]["requiresProxy"],
                    }),
                );
                // #endregion

                assert_eq!(result["success"], json!(true));
                assert_eq!(result["desktopSync"]["success"], json!(true));
                assert_eq!(result["desktopSync"]["mode"], json!("local_proxy"));
                assert_eq!(result["desktopSync"]["requiresProxy"], json!(true));

                let toml = fs::read_to_string(home.join(".codex").join("config.toml")).unwrap();
                let auth_raw = fs::read_to_string(home.join(".codex").join("auth.json")).unwrap();
                let auth_json: Value = serde_json::from_str(&auth_raw).unwrap();
                let injected_key = auth_json["OPENAI_API_KEY"].as_str().unwrap_or_default();

                // #region agent log
                agent_debug_log(
                    "H2",
                    "src-tauri/src/admin/handlers/desktop.rs:qwen_local_proxy_apply_writes_codex_auth_and_base_url:codex_files",
                    "verified codex files after qwen apply",
                    json!({
                        "tomlHasProxyBaseUrl": toml.contains("openai_base_url = \"http://127.0.0.1:19091\""),
                        "tomlHas1m": toml.contains("model_context_window = 1000000"),
                        "authMode": auth_json["auth_mode"],
                        "injectedKeyPrefix": injected_key.get(0..4).unwrap_or(""),
                        "injectedKeyLen": injected_key.len(),
                    }),
                );
                // #endregion

                assert!(toml.contains("openai_base_url = \"http://127.0.0.1:19091\""));
                assert!(
                    toml.contains("model_context_window = 1000000"),
                    "qwen3.6-* should be treated as 1M-capable"
                );
                assert_eq!(auth_json["auth_mode"], json!("apikey"));
                assert!(
                    injected_key.starts_with("cas_") && !injected_key.is_empty(),
                    "qwen local_proxy should inject gateway key into auth.json"
                );
                manager.stop_silent();
            });
        });
    }
}
