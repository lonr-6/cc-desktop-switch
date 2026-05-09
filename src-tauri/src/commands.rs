use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::apply_flow::{ApplyLocalConfigRequest, DesktopApplyResult};
use crate::config::{
    ConfigBackupSummary, ModelMappingDraft, ModelMappingSummary, ProviderExportPackage,
    ProviderImportApplyResult, ProviderImportPreview, ProviderPreset,
};
use crate::desktop::{build_apply_dry_run, ApplyDryRun};
use crate::desktop_writer::{probe_current_desktop_config, DesktopConfigProbe};
use crate::diagnostics::{
    readiness_snapshot, DiagnosticsIssueDraft, DiagnosticsPackage, ReadinessSnapshot,
    SmokeCheckResult,
};
use crate::gateway::GatewayHealth;
use crate::provider::{ProviderDraft, ProviderSummary};
use crate::state::{AppState, StateError};

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
        serializer.serialize_str(&self.to_string())
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
        .set_active_provider(&provider_id)
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
    state.export_provider_package().map_err(CommandError::from)
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
pub fn start_gateway(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.start_gateway().map_err(CommandError::from)
}

#[tauri::command]
pub fn stop_gateway(state: State<'_, AppState>) -> Result<GatewayHealth, CommandError> {
    state.stop_gateway().map_err(CommandError::from)
}

#[tauri::command]
pub fn desktop_config_probe() -> Result<DesktopConfigProbe, CommandError> {
    probe_current_desktop_config().map_err(|error| CommandError::State(error.to_string()))
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
pub fn apply_dry_run(state: State<'_, AppState>) -> Result<ApplyDryRun, CommandError> {
    let snapshot = state.snapshot().map_err(CommandError::from)?;
    Ok(build_apply_dry_run(
        snapshot.active_provider.as_ref(),
        &snapshot.active_model_mappings,
        snapshot.proxy_port,
        snapshot
            .gateway_api_key
            .as_deref()
            .unwrap_or("ccds_dry_run_key"),
    ))
}

#[tauri::command]
pub fn apply_local_config(
    request: ApplyLocalConfigRequest,
    state: State<'_, AppState>,
) -> Result<DesktopApplyResult, CommandError> {
    Ok(state.apply_to_local_config_library(&request.config_library_root))
}

#[tauri::command]
pub fn apply_detected_local_config(
    state: State<'_, AppState>,
) -> Result<DesktopApplyResult, CommandError> {
    let probe =
        probe_current_desktop_config().map_err(|error| CommandError::State(error.to_string()))?;
    Ok(state.apply_to_desktop_config_probe(&probe))
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
