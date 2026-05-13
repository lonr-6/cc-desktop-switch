use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::apply_flow::DesktopApplyResult;
use crate::config::{
    AppConfig, ConfigBackupSummary, ConfigSettings, ModelMappingDraft, ModelMappingSummary,
    ProviderExportPackage, ProviderImportApplyResult, ProviderImportPreview, ProviderPreset,
};
use crate::desktop::{build_apply_dry_run, ApplyStep};
use crate::desktop_writer::{
    clear_local_config_library, probe_current_desktop_config,
    restart_claude_desktop as restart_claude_desktop_impl, DesktopClearResult, DesktopConfigProbe,
    DesktopRestartResult,
};
use crate::diagnostics::{
    readiness_snapshot, DiagnosticsIssueDraft, DiagnosticsLogEntry, DiagnosticsPackage,
    ReadinessSnapshot, SmokeCheckResult,
};
use crate::gateway::GatewayHealth;
use crate::model_catalog::DesktopModel;
use crate::provider::{ProviderDraft, ProviderSummary};
use crate::state::{AppState, ProxyStatsSnapshot, StateError};
use crate::update::{
    check_update as check_update_impl, download_update as download_update_impl,
    install_update as install_update_impl, UpdateCheckResult, UpdateDownloadResult,
    UpdateInstallResult,
};

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub schema_version: u32,
    pub version: String,
    pub active_provider: Option<String>,
    pub gateway_api_key_present: bool,
    pub providers: Vec<ProviderSummary>,
    pub settings: ConfigSettings,
}

impl From<AppConfig> for ConfigSnapshot {
    fn from(config: AppConfig) -> Self {
        Self {
            schema_version: config.schema_version,
            version: config.version,
            active_provider: config.active_provider,
            gateway_api_key_present: config
                .gateway_api_key
                .as_deref()
                .is_some_and(|key| !key.trim().is_empty()),
            providers: config
                .providers
                .iter()
                .map(crate::config::ConfigProvider::summary)
                .collect(),
            settings: config.settings,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStats {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    pub today: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub stats: ProxyStats,
}

impl From<ProxyStatsSnapshot> for ProxyStats {
    fn from(stats: ProxyStatsSnapshot) -> Self {
        Self {
            total: stats.total,
            success: stats.success,
            failed: stats.failed,
            today: stats.today,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportRequest {
    pub raw_json: String,
    pub replace_existing: bool,
    #[serde(default)]
    pub skip_existing: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMappingUpdateRequest {
    pub provider_id: String,
    pub mappings: Vec<ModelMappingDraft>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresetImportRequest {
    pub preset_id: String,
    pub api_key: String,
    pub replace_existing: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyDryRunView {
    pub mode: String,
    pub success: bool,
    pub expected_base_url: String,
    pub expected_models: Vec<DesktopModel>,
    pub plan_error: Option<String>,
    pub steps: Vec<ApplyStep>,
}

impl From<crate::desktop::ApplyDryRun> for ApplyDryRunView {
    fn from(value: crate::desktop::ApplyDryRun) -> Self {
        Self {
            mode: value.mode,
            success: value.success,
            expected_base_url: value.expected_base_url,
            expected_models: value.expected_models,
            plan_error: value.plan_error,
            steps: value.steps,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("{0}")]
    InvalidProvider(String),
    #[error("state lock poisoned")]
    StateLock,
    #[error("{0}")]
    State(String),
}

impl serde::Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        let payload = CommandErrorPayload {
            error_type: "command_error",
            code: self.code(),
            message: self.to_string(),
        };
        payload.serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandErrorPayload {
    #[serde(rename = "type")]
    error_type: &'static str,
    code: String,
    message: String,
}

impl CommandError {
    fn code(&self) -> String {
        match self {
            CommandError::InvalidProvider(message) => {
                issue_code_from_message(message).unwrap_or_else(|| "provider.invalid".to_owned())
            }
            CommandError::StateLock => "state.lock_poisoned".to_owned(),
            CommandError::State(message) => {
                issue_code_from_message(message).unwrap_or_else(|| "command.state_error".to_owned())
            }
        }
    }
}

fn issue_code_from_message(message: &str) -> Option<String> {
    let (prefix, _) = message.split_once(':')?;
    let prefix = prefix.trim();
    if prefix.contains('.')
        && prefix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-')
    {
        Some(prefix.to_owned())
    } else {
        None
    }
}

#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderSummary>, CommandError> {
    state.list_providers().map_err(CommandError::from)
}

#[tauri::command]
pub fn save_provider(
    request: ProviderDraft,
    state: State<'_, AppState>,
) -> Result<ProviderSummary, CommandError> {
    state.save_provider(request).map_err(CommandError::from)
}

#[tauri::command]
pub fn set_active_provider(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state
        .set_active_provider_and_refresh_gateway(&provider_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn delete_provider(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state
        .delete_provider(&provider_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn reorder_providers(
    provider_ids: Vec<String>,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state
        .reorder_providers(provider_ids)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn export_providers(state: State<'_, AppState>) -> Result<ProviderExportPackage, CommandError> {
    state
        .export_provider_package_redacted()
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn save_provider_export_as(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    let Some(path) = pick_save_json_path(&app, "Save Provider export", "ccds-providers.json")?
    else {
        return Ok(None);
    };
    let package = state
        .export_provider_package()
        .map_err(CommandError::from)?;
    let body = serde_json::to_string_pretty(&package)
        .map_err(|error| CommandError::State(format!("provider.export_format_failed: {error}")))?;
    fs::write(&path, body)
        .map_err(|error| CommandError::State(format!("provider.export_save_failed: {error}")))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub fn preview_provider_import(
    request: ProviderImportRequest,
    state: State<'_, AppState>,
) -> Result<ProviderImportPreview, CommandError> {
    state
        .preview_provider_import_with_merge(
            &request.raw_json,
            request.replace_existing,
            request.skip_existing,
        )
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn import_providers(
    request: ProviderImportRequest,
    state: State<'_, AppState>,
) -> Result<ProviderImportApplyResult, CommandError> {
    state
        .import_providers_with_merge(
            &request.raw_json,
            request.replace_existing,
            request.skip_existing,
        )
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn list_provider_presets(state: State<'_, AppState>) -> Vec<ProviderPreset> {
    state.list_provider_presets()
}

#[tauri::command]
pub fn preview_provider_preset_import(
    request: ProviderPresetImportRequest,
    state: State<'_, AppState>,
) -> Result<ProviderImportPreview, CommandError> {
    state
        .preview_provider_preset_import(&request.preset_id, request.replace_existing)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn import_provider_preset(
    request: ProviderPresetImportRequest,
    state: State<'_, AppState>,
) -> Result<ProviderImportApplyResult, CommandError> {
    state
        .import_provider_preset(
            &request.preset_id,
            request.api_key,
            request.replace_existing,
        )
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn list_model_mappings(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ModelMappingSummary>, CommandError> {
    state
        .list_model_mappings(&provider_id)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn update_model_mappings(
    request: ModelMappingUpdateRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ModelMappingSummary>, CommandError> {
    state
        .update_model_mappings(&request.provider_id, request.mappings)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn list_config_backups(
    state: State<'_, AppState>,
) -> Result<Vec<ConfigBackupSummary>, CommandError> {
    state.list_config_backups().map_err(CommandError::from)
}

#[tauri::command]
pub fn read_config_backup(
    file_name: String,
    state: State<'_, AppState>,
) -> Result<String, CommandError> {
    state
        .read_config_backup(&file_name)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn create_config_backup(
    state: State<'_, AppState>,
) -> Result<Option<ConfigBackupSummary>, CommandError> {
    state.create_config_backup().map_err(CommandError::from)
}

#[tauri::command]
pub fn get_config_snapshot(state: State<'_, AppState>) -> Result<ConfigSnapshot, CommandError> {
    state
        .load_config()
        .map(ConfigSnapshot::from)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<ConfigSettings, CommandError> {
    state.settings().map_err(CommandError::from)
}

#[tauri::command]
pub fn update_settings(
    settings: ConfigSettings,
    state: State<'_, AppState>,
) -> Result<ConfigSettings, CommandError> {
    state.update_settings(settings).map_err(CommandError::from)
}

#[tauri::command]
pub fn health(state: State<'_, AppState>) -> Result<ReadinessSnapshot, CommandError> {
    let snapshot = state.snapshot().map_err(CommandError::from)?;
    let gateway = state.gateway_status().map_err(CommandError::from)?;
    let gateway_issue_code = state.gateway_issue_code().map_err(CommandError::from)?;
    Ok(readiness_snapshot(
        snapshot.active_provider.as_ref(),
        gateway,
        gateway_issue_code.as_deref(),
    ))
}

#[tauri::command]
pub fn gateway_status(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.gateway_status().map_err(CommandError::from)
}

#[tauri::command]
pub fn get_proxy_status(state: State<'_, AppState>) -> Result<ProxyStatus, CommandError> {
    let health = state.gateway_status().map_err(CommandError::from)?;
    let port = health
        .base_url
        .rsplit(':')
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(18080);
    Ok(ProxyStatus {
        running: health.running,
        port,
        base_url: health.base_url,
        stats: state.proxy_stats().map_err(CommandError::from)?.into(),
    })
}

#[tauri::command]
pub fn start_gateway(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.start_gateway().map_err(CommandError::from)
}

#[tauri::command]
pub fn start_proxy_listener(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.start_gateway().map_err(CommandError::from)
}

#[tauri::command]
pub fn stop_gateway(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.stop_gateway().map_err(CommandError::from)
}

#[tauri::command]
pub fn stop_proxy_listener(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.stop_gateway().map_err(CommandError::from)
}

#[tauri::command]
pub fn get_proxy_logs(
    state: State<'_, AppState>,
) -> Result<Vec<DiagnosticsLogEntry>, CommandError> {
    state.runtime_logs().map_err(CommandError::from)
}

#[tauri::command]
pub fn clear_proxy_logs(state: State<'_, AppState>) -> Result<bool, CommandError> {
    state.clear_runtime_logs().map_err(CommandError::from)
}

#[tauri::command]
pub fn desktop_config_probe() -> Result<DesktopConfigProbe, CommandError> {
    probe_current_desktop_config().map_err(|error| CommandError::State(error.to_string()))
}

#[tauri::command]
pub fn clear_desktop_config() -> Result<DesktopClearResult, CommandError> {
    let probe =
        probe_current_desktop_config().map_err(|error| CommandError::State(error.to_string()))?;
    if probe.managed_detected {
        let config_path = probe.local_config_library.join(format!(
            "{}.json",
            crate::desktop_writer::CCDS_LOCAL_CONFIG_ID
        ));
        let meta_path = probe.local_config_library.join("_meta.json");
        let mut issue_codes = probe.issue_codes;
        if !issue_codes
            .iter()
            .any(|code| code == "desktop.clear_blocked_by_managed_config")
        {
            issue_codes.push("desktop.clear_blocked_by_managed_config".to_owned());
        }
        return Ok(DesktopClearResult {
            success: false,
            config_id: crate::desktop_writer::CCDS_LOCAL_CONFIG_ID.to_owned(),
            local_config_library: probe.local_config_library,
            config_path,
            meta_path,
            removed_config: false,
            cleared_active_config: false,
            preserved_meta: false,
            readback_cleared: false,
            issue_codes,
        });
    }
    let result = clear_local_config_library(&probe.local_config_library)
        .map_err(|error| CommandError::State(error.to_string()))?;
    Ok(result)
}

#[tauri::command]
pub fn restart_claude_desktop() -> Result<DesktopRestartResult, CommandError> {
    restart_claude_desktop_impl().map_err(|error| CommandError::State(error.to_string()))
}

#[tauri::command]
pub fn export_diagnostics_package(
    state: State<'_, AppState>,
) -> Result<DiagnosticsPackage, CommandError> {
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    state
        .diagnostics_package(desktop_probe, desktop_error)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn copy_diagnostics_summary(state: State<'_, AppState>) -> Result<String, CommandError> {
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    state
        .diagnostics_summary(desktop_probe, desktop_error)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn copy_diagnostics_summary_to_clipboard(
    state: State<'_, AppState>,
) -> Result<String, CommandError> {
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    let summary = state
        .diagnostics_summary(desktop_probe, desktop_error)
        .map_err(CommandError::from)?;
    copy_text_to_clipboard(&summary).map_err(CommandError::State)?;
    Ok(summary)
}

#[tauri::command]
pub fn copy_text_to_clipboard_command(text: String) -> Result<bool, CommandError> {
    copy_text_to_clipboard(&text).map_err(CommandError::State)?;
    Ok(true)
}

#[tauri::command]
pub fn save_diagnostics_package(state: State<'_, AppState>) -> Result<String, CommandError> {
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    state
        .save_diagnostics_package(desktop_probe, desktop_error)
        .map(|path| path.display().to_string())
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn save_diagnostics_package_as(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    let Some(path) = pick_save_json_path(&app, "Save diagnostics package", "diagnostics.json")?
    else {
        return Ok(None);
    };
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    let package = state
        .diagnostics_package(desktop_probe, desktop_error)
        .map_err(CommandError::from)?;
    let body = serde_json::to_string_pretty(&package).map_err(|error| {
        CommandError::State(format!("diagnostics.package_format_failed: {error}"))
    })?;
    fs::write(&path, body).map_err(|error| {
        CommandError::State(format!("diagnostics.package_save_failed: {error}"))
    })?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command]
pub fn diagnostics_issue_draft(
    state: State<'_, AppState>,
) -> Result<DiagnosticsIssueDraft, CommandError> {
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    state
        .diagnostics_issue_draft(desktop_probe, desktop_error)
        .map_err(CommandError::from)
}

#[tauri::command]
pub fn open_diagnostics_issue(
    state: State<'_, AppState>,
) -> Result<DiagnosticsIssueDraft, CommandError> {
    let (desktop_probe, desktop_error) = desktop_probe_for_diagnostics();
    let draft = state
        .diagnostics_issue_draft(desktop_probe, desktop_error)
        .map_err(CommandError::from)?;
    open_url(&draft.url).map_err(CommandError::State)?;
    Ok(draft)
}

#[tauri::command]
pub fn provider_static_smoke(state: State<'_, AppState>) -> Result<SmokeCheckResult, CommandError> {
    state.provider_static_smoke().map_err(CommandError::from)
}

#[tauri::command]
pub fn gateway_smoke(state: State<'_, AppState>) -> Result<SmokeCheckResult, CommandError> {
    state.gateway_smoke().map_err(CommandError::from)
}

#[tauri::command]
pub fn provider_real_smoke(state: State<'_, AppState>) -> Result<SmokeCheckResult, CommandError> {
    state.provider_real_smoke().map_err(CommandError::from)
}

#[tauri::command]
pub async fn check_update(state: State<'_, AppState>) -> Result<UpdateCheckResult, CommandError> {
    let settings = state.settings().map_err(CommandError::from)?;
    check_update_impl(&settings.update_url, env!("CARGO_PKG_VERSION"))
        .await
        .map_err(|error| CommandError::State(error.to_string()))
}

#[tauri::command]
pub async fn download_update(
    state: State<'_, AppState>,
) -> Result<UpdateDownloadResult, CommandError> {
    let settings = state.settings().map_err(CommandError::from)?;
    let updates_dir = state
        .config_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("updates");
    download_update_impl(
        &settings.update_url,
        env!("CARGO_PKG_VERSION"),
        &updates_dir,
    )
    .await
    .map_err(|error| CommandError::State(error.to_string()))
}

#[tauri::command]
pub fn install_update(
    installer_path: PathBuf,
    state: State<'_, AppState>,
) -> Result<UpdateInstallResult, CommandError> {
    let updates_dir = state
        .config_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("updates");
    install_update_impl(&installer_path, &updates_dir)
        .map_err(|error| CommandError::State(error.to_string()))
}

#[tauri::command]
pub fn apply_dry_run(state: State<'_, AppState>) -> Result<ApplyDryRunView, CommandError> {
    let snapshot = state.snapshot().map_err(CommandError::from)?;
    Ok(build_apply_dry_run(
        snapshot.active_provider.as_ref(),
        &snapshot.active_model_mappings,
        snapshot.proxy_port,
        "ccds_dry_run_key",
    )
    .into())
}

#[tauri::command]
pub fn apply_detected_local_config(
    state: State<'_, AppState>,
) -> Result<DesktopApplyResult, CommandError> {
    let probe =
        probe_current_desktop_config().map_err(|error| CommandError::State(error.to_string()))?;
    Ok(state.apply_to_desktop_config_probe(&probe))
}

#[tauri::command]
pub fn configure_desktop(state: State<'_, AppState>) -> Result<DesktopApplyResult, CommandError> {
    apply_detected_local_config(state)
}

impl From<StateError> for CommandError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::StateLock => CommandError::StateLock,
            StateError::InvalidProvider(message) => CommandError::InvalidProvider(message),
            error => CommandError::State(error.to_string()),
        }
    }
}

fn desktop_probe_for_diagnostics() -> (Option<DesktopConfigProbe>, Option<String>) {
    match probe_current_desktop_config() {
        Ok(probe) => (Some(probe), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

fn pick_save_json_path(
    app: &AppHandle,
    title: &str,
    file_name: &str,
) -> Result<Option<PathBuf>, CommandError> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title(title)
        .set_file_name(file_name)
        .add_filter("JSON", &["json"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    path.into_path()
        .map(Some)
        .map_err(|error| CommandError::State(format!("dialog.invalid_save_path: {error}")))
}

fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("powershell");
        command.creation_flags(0x08000000);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = Command::new("pbcopy");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xclip");

    #[cfg(target_os = "windows")]
    command.args(["-NoProfile", "-Command", "Set-Clipboard"]);
    #[cfg(all(unix, not(target_os = "macos")))]
    command.args(["-selection", "clipboard"]);

    let mut child = command
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("diagnostics.clipboard_failed: {error}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("diagnostics.clipboard_failed: {error}"))?;
    }
    let status = child
        .wait()
        .map_err(|error| format!("diagnostics.clipboard_failed: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("diagnostics.clipboard_failed: {status}"))
    }
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32");
        command.creation_flags(0x08000000);
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    let status = command
        .status()
        .map_err(|error| format!("diagnostics.open_issue_failed: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("diagnostics.open_issue_failed: {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_error_serializes_structured_issue_code() {
        let value = serde_json::to_value(CommandError::State(
            "update.install_failed: installer launch failed".to_owned(),
        ))
        .unwrap();

        assert_eq!(value["type"], "command_error");
        assert_eq!(value["code"], "update.install_failed");
        assert_eq!(
            value["message"],
            "update.install_failed: installer launch failed"
        );
    }

    #[test]
    fn command_error_uses_generic_code_without_issue_prefix() {
        let value = serde_json::to_value(CommandError::State("plain failure".to_owned())).unwrap();

        assert_eq!(value["type"], "command_error");
        assert_eq!(value["code"], "command.state_error");
        assert_eq!(value["message"], "plain failure");
    }
}
