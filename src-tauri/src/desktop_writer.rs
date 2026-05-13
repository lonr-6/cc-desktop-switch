use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::desktop::{compare_desktop_readback, DesktopHealth, DesktopPlan, DesktopReadback};
use crate::model_catalog::DesktopModel;

pub const CCDS_LOCAL_CONFIG_ID: &str = "cc-desktop-switch-local-gateway";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPlatform {
    Windows,
    Macos,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfigProbe {
    pub platform: DesktopPlatform,
    pub local_config_library: PathBuf,
    pub managed_detected: bool,
    pub managed_evidence: Vec<ManagedConfigEvidence>,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedConfigEvidence {
    pub code: String,
    pub location: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWriteResult {
    pub config_id: String,
    pub config_path: PathBuf,
    pub meta_path: PathBuf,
    pub readback: DesktopReadback,
    pub health: DesktopHealth,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopClearResult {
    pub success: bool,
    pub config_id: String,
    pub local_config_library: PathBuf,
    pub config_path: PathBuf,
    pub meta_path: PathBuf,
    pub removed_config: bool,
    pub cleared_active_config: bool,
    pub preserved_meta: bool,
    pub readback_cleared: bool,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRestartResult {
    pub platform: DesktopPlatform,
    pub stopped_processes: u32,
    pub forced_processes: u32,
    pub launched: bool,
    pub executable: Option<String>,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum DesktopWriterError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Desktop config JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("desktop.config_library_meta_missing: active config id is missing")]
    MissingActiveConfig,
    #[error("desktop.config_library_not_object: config file must be a JSON object")]
    ConfigNotObject,
    #[error("desktop.unsupported_platform: current OS is not supported for Claude Desktop config")]
    UnsupportedPlatform,
    #[error("desktop.home_dir_missing: USERPROFILE or HOME is required")]
    HomeDirMissing,
    #[error("desktop.restart_failed: {0}")]
    RestartFailed(String),
}

pub fn probe_current_desktop_config() -> Result<DesktopConfigProbe, DesktopWriterError> {
    let platform = current_desktop_platform().ok_or(DesktopWriterError::UnsupportedPlatform)?;
    let home_dir = current_home_dir().ok_or(DesktopWriterError::HomeDirMissing)?;
    let local_app_data = std::env::var("LOCALAPPDATA").ok().map(PathBuf::from);
    let managed_policy_paths = default_managed_policy_paths(platform, &home_dir);
    let mut probe = probe_desktop_config(
        platform,
        &home_dir,
        local_app_data.as_deref(),
        &managed_policy_paths,
    );
    probe
        .managed_evidence
        .extend(current_managed_policy_evidence(platform));
    refresh_managed_probe_status(&mut probe);
    Ok(probe)
}

pub fn probe_desktop_config(
    platform: DesktopPlatform,
    home_dir: &Path,
    local_app_data: Option<&Path>,
    managed_policy_paths: &[PathBuf],
) -> DesktopConfigProbe {
    let managed_evidence = managed_policy_paths
        .iter()
        .filter(|path| path.exists())
        .map(|path| ManagedConfigEvidence {
            code: "desktop.managed_config_detected".to_owned(),
            location: path.display().to_string(),
            detail: "managed Claude Desktop config path exists".to_owned(),
        })
        .collect::<Vec<_>>();
    let mut probe = DesktopConfigProbe {
        platform,
        local_config_library: local_config_library_path(platform, home_dir, local_app_data),
        managed_detected: false,
        managed_evidence,
        issue_codes: Vec::new(),
    };
    refresh_managed_probe_status(&mut probe);
    probe
}

pub fn local_config_library_path(
    platform: DesktopPlatform,
    home_dir: &Path,
    local_app_data: Option<&Path>,
) -> PathBuf {
    match platform {
        DesktopPlatform::Windows => local_app_data
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home_dir.join("AppData").join("Local"))
            .join("Claude-3p")
            .join("configLibrary"),
        DesktopPlatform::Macos => home_dir
            .join("Library")
            .join("Application Support")
            .join("Claude-3p")
            .join("configLibrary"),
    }
}

fn refresh_managed_probe_status(probe: &mut DesktopConfigProbe) {
    let evidence_issue_codes = probe
        .managed_evidence
        .iter()
        .map(|evidence| evidence.code.clone())
        .collect::<Vec<_>>();
    probe.managed_detected = !probe.managed_evidence.is_empty();
    probe.issue_codes.retain(|code| {
        code != "desktop.managed_config_detected"
            && code != "desktop.local_config_available"
            && !evidence_issue_codes.iter().any(|evidence| evidence == code)
    });
    if probe.managed_detected {
        probe
            .issue_codes
            .push("desktop.managed_config_detected".to_owned());
        for code in evidence_issue_codes {
            if code != "desktop.managed_config_detected" && !probe.issue_codes.contains(&code) {
                probe.issue_codes.push(code);
            }
        }
    } else {
        probe
            .issue_codes
            .push("desktop.local_config_available".to_owned());
    }
}

fn current_desktop_platform() -> Option<DesktopPlatform> {
    if cfg!(target_os = "windows") {
        Some(DesktopPlatform::Windows)
    } else if cfg!(target_os = "macos") {
        Some(DesktopPlatform::Macos)
    } else {
        None
    }
}

fn current_home_dir() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
}

fn default_managed_policy_paths(platform: DesktopPlatform, home_dir: &Path) -> Vec<PathBuf> {
    match platform {
        DesktopPlatform::Windows => Vec::new(),
        DesktopPlatform::Macos => vec![
            PathBuf::from("/Library/Managed Preferences/com.anthropic.claude.plist"),
            home_dir
                .join("Library")
                .join("Managed Preferences")
                .join("com.anthropic.claude.plist"),
        ],
    }
}

fn current_managed_policy_evidence(platform: DesktopPlatform) -> Vec<ManagedConfigEvidence> {
    match platform {
        DesktopPlatform::Windows => windows_registry_policy_evidence(),
        DesktopPlatform::Macos => Vec::new(),
    }
}

#[cfg(target_os = "windows")]
fn windows_registry_policy_evidence() -> Vec<ManagedConfigEvidence> {
    [
        r"HKCU\SOFTWARE\Policies\Claude",
        r"HKLM\SOFTWARE\Policies\Claude",
    ]
    .into_iter()
    .filter(|key| {
        Command::new("reg")
            .args(["query", key])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    })
    .map(|key| ManagedConfigEvidence {
        code: windows_registry_policy_code(key),
        location: key.to_owned(),
        detail: windows_registry_policy_detail(key),
    })
    .collect()
}

#[cfg(target_os = "windows")]
fn windows_registry_policy_code(key: &str) -> String {
    if windows_registry_value_contains(key, "ccds_managed", "true") {
        "desktop.ccds_managed_policy_detected".to_owned()
    } else {
        "desktop.managed_config_detected".to_owned()
    }
}

#[cfg(target_os = "windows")]
fn windows_registry_policy_detail(key: &str) -> String {
    if windows_registry_value_contains(key, "ccds_managed", "true") {
        "CC Desktop Switch managed registry policy exists".to_owned()
    } else {
        "Windows registry policy exists".to_owned()
    }
}

#[cfg(target_os = "windows")]
fn windows_registry_value_contains(key: &str, value_name: &str, expected: &str) -> bool {
    Command::new("reg")
        .args(["query", key, "/v", value_name])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .to_ascii_lowercase()
                    .contains(&expected.to_ascii_lowercase())
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn windows_registry_policy_evidence() -> Vec<ManagedConfigEvidence> {
    let _ = Command::new("reg");
    Vec::new()
}

pub fn write_local_config_library(
    root: &Path,
    plan: &DesktopPlan,
) -> Result<DesktopWriteResult, DesktopWriterError> {
    fs::create_dir_all(root)?;

    let config_path = root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json"));
    let meta_path = root.join("_meta.json");
    let mut config = read_json_object_if_exists(&config_path)?.unwrap_or_default();
    apply_plan_to_config_object(&mut config, plan);
    let previous_config_bytes = if config_path.exists() {
        Some(fs::read(&config_path)?)
    } else {
        None
    };

    let meta = local_config_meta_for_root(root)?;
    write_json_file(&config_path, &Value::Object(config))?;
    if let Err(error) = write_json_file(&meta_path, &meta) {
        restore_config_file_after_failed_meta_write(&config_path, previous_config_bytes)?;
        return Err(error);
    }

    let readback = read_local_config_library(root)?;
    let health = compare_desktop_readback(plan, &readback);

    Ok(DesktopWriteResult {
        config_id: CCDS_LOCAL_CONFIG_ID.to_owned(),
        config_path,
        meta_path,
        readback,
        health,
    })
}

pub fn read_local_config_library(root: &Path) -> Result<DesktopReadback, DesktopWriterError> {
    let meta_path = root.join("_meta.json");
    let meta = read_json_object(&meta_path)?;
    let config_id = active_config_id(&meta).ok_or(DesktopWriterError::MissingActiveConfig)?;
    let config_path = root.join(format!("{config_id}.json"));
    let config = read_json_object(&config_path)?;

    Ok(read_desktop_config_object(&config))
}

pub fn clear_local_config_library(root: &Path) -> Result<DesktopClearResult, DesktopWriterError> {
    let config_path = root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json"));
    let meta_path = root.join("_meta.json");
    let mut cleared_active_config = false;
    let mut preserved_meta = false;
    let mut remove_meta = false;
    let mut next_meta = None;
    if meta_path.is_file() {
        let mut meta = read_json_object(&meta_path)?;
        if active_config_id(&meta).as_deref() == Some(CCDS_LOCAL_CONFIG_ID) {
            for key in active_config_keys() {
                if string_value(meta.get(*key)).as_deref() == Some(CCDS_LOCAL_CONFIG_ID) {
                    meta.remove(*key);
                }
            }
            cleared_active_config = true;
            if meta.is_empty() {
                remove_meta = true;
            } else {
                next_meta = Some(Value::Object(meta));
                preserved_meta = true;
            }
        } else {
            preserved_meta = true;
        }
    }
    if remove_meta {
        fs::remove_file(&meta_path)?;
    } else if let Some(meta) = next_meta {
        write_json_file(&meta_path, &meta)?;
    }

    let removed_config = if config_path.is_file() {
        fs::remove_file(&config_path)?;
        true
    } else {
        false
    };

    let active_after = if meta_path.is_file() {
        Some(active_config_id(&read_json_object(&meta_path)?))
    } else {
        None
    }
    .flatten();
    let readback_cleared =
        !config_path.exists() && active_after.as_deref() != Some(CCDS_LOCAL_CONFIG_ID);
    let mut issue_codes = Vec::new();
    if !readback_cleared {
        issue_codes.push("desktop.clear_readback_failed".to_owned());
    }

    Ok(DesktopClearResult {
        success: readback_cleared,
        config_id: CCDS_LOCAL_CONFIG_ID.to_owned(),
        local_config_library: root.to_path_buf(),
        config_path,
        meta_path,
        removed_config,
        cleared_active_config,
        preserved_meta,
        readback_cleared,
        issue_codes,
    })
}

pub fn restart_claude_desktop() -> Result<DesktopRestartResult, DesktopWriterError> {
    let platform = current_desktop_platform().ok_or(DesktopWriterError::UnsupportedPlatform)?;
    restart_claude_desktop_for_platform(platform)
}

fn restart_claude_desktop_for_platform(
    platform: DesktopPlatform,
) -> Result<DesktopRestartResult, DesktopWriterError> {
    match platform {
        DesktopPlatform::Windows => restart_claude_desktop_windows(),
        DesktopPlatform::Macos => restart_claude_desktop_macos(),
    }
}

#[cfg(target_os = "windows")]
fn restart_claude_desktop_windows() -> Result<DesktopRestartResult, DesktopWriterError> {
    let stopped_processes = windows_process_count("Claude.exe");
    if stopped_processes > 0 {
        let status = hidden_command("taskkill")
            .args(["/IM", "Claude.exe", "/T"])
            .status()
            .map_err(|error| DesktopWriterError::RestartFailed(error.to_string()))?;
        if !status.success() {
            return Err(DesktopWriterError::RestartFailed(format!(
                "taskkill returned {status}"
            )));
        }
    }

    let executable = windows_claude_executable_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            DesktopWriterError::RestartFailed(
                "Claude.exe was not found in known install locations".to_owned(),
            )
        })?;
    hidden_command(&executable)
        .spawn()
        .map_err(|error| DesktopWriterError::RestartFailed(error.to_string()))?;

    Ok(DesktopRestartResult {
        platform: DesktopPlatform::Windows,
        stopped_processes,
        forced_processes: 0,
        launched: true,
        executable: Some(executable.display().to_string()),
        message: "Claude Desktop restart requested".to_owned(),
    })
}

#[cfg(not(target_os = "windows"))]
fn restart_claude_desktop_windows() -> Result<DesktopRestartResult, DesktopWriterError> {
    Err(DesktopWriterError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
fn restart_claude_desktop_macos() -> Result<DesktopRestartResult, DesktopWriterError> {
    let stopped_processes = macos_process_count("Claude");
    if stopped_processes > 0 {
        let status = Command::new("osascript")
            .args([
                "-e",
                "tell application id \"com.anthropic.claudefordesktop\" to quit",
            ])
            .status()
            .map_err(|error| DesktopWriterError::RestartFailed(error.to_string()))?;
        if !status.success() {
            return Err(DesktopWriterError::RestartFailed(format!(
                "osascript quit returned {status}"
            )));
        }
    }

    let status = Command::new("open")
        .args(["-b", "com.anthropic.claudefordesktop"])
        .status()
        .map_err(|error| DesktopWriterError::RestartFailed(error.to_string()))?;
    if !status.success() {
        return Err(DesktopWriterError::RestartFailed(format!(
            "open returned {status}"
        )));
    }

    Ok(DesktopRestartResult {
        platform: DesktopPlatform::Macos,
        stopped_processes,
        forced_processes: 0,
        launched: true,
        executable: Some("com.anthropic.claudefordesktop".to_owned()),
        message: "Claude Desktop restart requested".to_owned(),
    })
}

#[cfg(not(target_os = "macos"))]
fn restart_claude_desktop_macos() -> Result<DesktopRestartResult, DesktopWriterError> {
    Err(DesktopWriterError::UnsupportedPlatform)
}

fn apply_plan_to_config_object(config: &mut Map<String, Value>, plan: &DesktopPlan) {
    config.insert(
        "name".to_owned(),
        Value::String("CC Desktop Switch".to_owned()),
    );
    config.insert(
        "inferenceProvider".to_owned(),
        Value::String("gateway".to_owned()),
    );
    config.insert(
        "inferenceGatewayBaseUrl".to_owned(),
        Value::String(plan.base_url.clone()),
    );
    config.insert(
        "inferenceGatewayApiKey".to_owned(),
        Value::String(plan.gateway_api_key.clone()),
    );
    config.insert(
        "inferenceGatewayAuthScheme".to_owned(),
        Value::String(plan.auth_scheme.clone()),
    );
    config.insert(
        "inferenceGatewayHeaders".to_owned(),
        Value::Array(
            plan.gateway_headers
                .iter()
                .map(|header| Value::String(header.clone()))
                .collect(),
        ),
    );
    config.insert(
        "inferenceModels".to_owned(),
        Value::Array(
            plan.inference_models
                .iter()
                .map(inference_model_config_value)
                .collect(),
        ),
    );
}

fn read_desktop_config_object(config: &Map<String, Value>) -> DesktopReadback {
    DesktopReadback {
        base_url: string_value(config.get("inferenceGatewayBaseUrl")),
        inference_models: parse_inference_models(config.get("inferenceModels")),
        mode: string_value(config.get("inferenceProvider")).map(|provider| {
            if provider == "gateway" {
                "local_gateway".to_owned()
            } else {
                provider
            }
        }),
        auth_scheme: string_value(config.get("inferenceGatewayAuthScheme")),
        gateway_api_key_present: Some(
            string_value(config.get("inferenceGatewayApiKey"))
                .map(|key| !key.is_empty())
                .unwrap_or(false),
        ),
        gateway_headers: parse_string_array(config.get("inferenceGatewayHeaders")),
    }
}

fn active_config_keys() -> &'static [&'static str] {
    &[
        "activeConfigId",
        "active_config_id",
        "appliedConfigId",
        "selectedConfigId",
    ]
}

fn inference_model_config_value(model: &DesktopModel) -> Value {
    if model.supports_1m || model.supports_max {
        let mut object = Map::new();
        object.insert("name".to_owned(), Value::String(model.id.clone()));
        if model.supports_1m {
            object.insert("supports1m".to_owned(), Value::Bool(true));
        }
        if model.supports_max {
            object.insert("supportsMax".to_owned(), Value::Bool(true));
        }
        Value::Object(object)
    } else {
        Value::String(model.id.clone())
    }
}

fn parse_inference_models(value: Option<&Value>) -> Vec<DesktopModel> {
    parse_array_value(value)
        .into_iter()
        .filter_map(|value| match value {
            Value::String(id) => Some(DesktopModel {
                display_name: id.clone(),
                id,
                supports_1m: false,
                supports_max: false,
            }),
            Value::Object(object) => {
                let id = object.get("name").and_then(Value::as_str)?.to_owned();
                Some(DesktopModel {
                    display_name: id.clone(),
                    id,
                    supports_1m: object
                        .get("supports1m")
                        .or_else(|| object.get("supports1M"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    supports_max: object
                        .get("supportsMax")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            }
            _ => None,
        })
        .collect()
}

fn parse_string_array(value: Option<&Value>) -> Vec<String> {
    parse_array_value(value)
        .into_iter()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect()
}

fn parse_array_value(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(values)) => values.clone(),
        Some(Value::String(value)) => serde_json::from_str::<Vec<Value>>(value).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn local_config_meta_for_root(root: &Path) -> Result<Value, DesktopWriterError> {
    let meta_path = root.join("_meta.json");
    let mut meta = read_json_object_if_exists(&meta_path)?.unwrap_or_default();
    for key in active_config_keys() {
        meta.remove(*key);
    }
    meta.insert(
        "activeConfigId".to_owned(),
        Value::String(CCDS_LOCAL_CONFIG_ID.to_owned()),
    );
    Ok(Value::Object(meta))
}

fn active_config_id(meta: &Map<String, Value>) -> Option<String> {
    active_config_keys()
        .iter()
        .find_map(|key| string_value(meta.get(*key)))
}

#[cfg(target_os = "windows")]
fn hidden_command<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(target_os = "windows"))]
fn hidden_command<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    Command::new(program)
}

#[cfg(target_os = "windows")]
fn windows_process_count(image_name: &str) -> u32 {
    let filter = format!("IMAGENAME eq {image_name}");
    hidden_command("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| line.trim_start().starts_with(&format!("\"{image_name}\"")))
                .count() as u32
        })
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn windows_claude_executable_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let local_app_data = PathBuf::from(local_app_data);
        candidates.push(local_app_data.join("AnthropicClaude").join("Claude.exe"));
        candidates.push(
            local_app_data
                .join("Programs")
                .join("Claude")
                .join("Claude.exe"),
        );
        candidates.push(local_app_data.join("Claude").join("Claude.exe"));
    }
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        candidates.push(
            PathBuf::from(program_files)
                .join("Claude")
                .join("Claude.exe"),
        );
    }
    if let Ok(program_files_x86) = std::env::var("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(program_files_x86)
                .join("Claude")
                .join("Claude.exe"),
        );
    }
    candidates
}

#[cfg(target_os = "macos")]
fn macos_process_count(process_name: &str) -> u32 {
    Command::new("pgrep")
        .args(["-x", process_name])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).lines().count() as u32)
        .unwrap_or(0)
}

fn read_json_object_if_exists(
    path: &Path,
) -> Result<Option<Map<String, Value>>, DesktopWriterError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_json_object(path)?))
}

fn read_json_object(path: &Path) -> Result<Map<String, Value>, DesktopWriterError> {
    let body = fs::read_to_string(path)?;
    match serde_json::from_str::<Value>(&body)? {
        Value::Object(object) => Ok(object),
        _ => Err(DesktopWriterError::ConfigNotObject),
    }
}

fn write_json_file(path: &Path, value: &Value) -> Result<(), DesktopWriterError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, serde_json::to_string_pretty(value)?)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

fn restore_config_file_after_failed_meta_write(
    config_path: &Path,
    previous_config_bytes: Option<Vec<u8>>,
) -> Result<(), DesktopWriterError> {
    match previous_config_bytes {
        Some(bytes) => fs::write(config_path, bytes)?,
        None => {
            if config_path.exists() {
                fs::remove_file(config_path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::desktop::build_desktop_plan;
    use crate::model_catalog::{ModelMapping, ModelSlot, RouteCapabilities};
    use crate::provider::{ApiFormat, AuthScheme, Provider, ProviderDraft};

    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("ccds-desktop-writer-{name}-{millis}"))
    }

    fn provider() -> Provider {
        ProviderDraft {
            provider_id: Some("provider-deepseek".to_owned()),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_key: "sk-test".to_owned(),
            api_format: ApiFormat::Anthropic,
        }
        .into_provider()
        .unwrap()
    }

    fn mappings() -> Vec<ModelMapping> {
        vec![
            ModelMapping {
                slot: ModelSlot::Sonnet,
                upstream_model: "deepseek-v4-pro".to_owned(),
                route_id: Some("claude-deepseek-v4-pro".to_owned()),
                capabilities: RouteCapabilities {
                    supports_1m: true,
                    supports_max: false,
                },
            },
            ModelMapping {
                slot: ModelSlot::Opus,
                upstream_model: "deepseek-reasoner".to_owned(),
                route_id: Some("claude-deepseek-reasoner".to_owned()),
                capabilities: RouteCapabilities {
                    supports_1m: false,
                    supports_max: true,
                },
            },
        ]
    }

    fn plan() -> DesktopPlan {
        build_desktop_plan(&provider(), &mappings(), "ccds_gateway_key", 18080).unwrap()
    }

    #[test]
    fn desktop_writer_writes_local_config_library_and_readback_passes() {
        let root = temp_root("write-readback");
        let plan = plan();

        let result = write_local_config_library(&root, &plan).unwrap();

        assert!(result.health.passed);
        assert_eq!(
            result.readback.base_url.as_deref(),
            Some(plan.base_url.as_str())
        );
        assert_eq!(result.readback.mode.as_deref(), Some("local_gateway"));
        assert_eq!(result.readback.auth_scheme.as_deref(), Some("bearer"));
        assert_eq!(result.readback.gateway_api_key_present, Some(true));
        assert!(result
            .readback
            .inference_models
            .iter()
            .any(|model| model.id == "claude-deepseek-v4-pro" && model.supports_1m));

        let body = fs::read_to_string(&result.config_path).unwrap();
        assert!(body.contains("claude-deepseek-v4-pro"));
        assert!(!body.contains("\"deepseek-v4-pro\""));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_writer_preserves_unrelated_local_config_values() {
        let root = temp_root("preserve");
        fs::create_dir_all(&root).unwrap();
        let config_path = root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json"));
        fs::write(
            &config_path,
            serde_json::json!({
                "coworkEgressAllowedHosts": ["example.com"],
                "unrelated": "keep"
            })
            .to_string(),
        )
        .unwrap();

        write_local_config_library(&root, &plan()).unwrap();
        let saved =
            serde_json::from_str::<Value>(&fs::read_to_string(&config_path).unwrap()).unwrap();

        assert_eq!(saved["unrelated"], "keep");
        assert_eq!(saved["coworkEgressAllowedHosts"][0], "example.com");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_writer_preserves_unrelated_local_config_meta_values() {
        let root = temp_root("preserve-meta");
        fs::create_dir_all(&root).unwrap();
        write_json_file(
            &root.join("_meta.json"),
            &serde_json::json!({
                "activeConfigId": "manual-profile",
                "selectedConfigId": "manual-profile",
                "unrelatedMeta": "keep"
            }),
        )
        .unwrap();

        write_local_config_library(&root, &plan()).unwrap();
        let saved = read_json_object(&root.join("_meta.json")).unwrap();

        assert_eq!(
            string_value(saved.get("activeConfigId")).as_deref(),
            Some(CCDS_LOCAL_CONFIG_ID)
        );
        assert_eq!(saved.get("selectedConfigId"), None);
        assert_eq!(saved["unrelatedMeta"], "keep");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_writer_reads_json_string_models_and_headers() {
        let root = temp_root("json-string");
        fs::create_dir_all(&root).unwrap();
        write_json_file(
            &root.join("_meta.json"),
            &serde_json::json!({
                "activeConfigId": CCDS_LOCAL_CONFIG_ID
            }),
        )
        .unwrap();
        write_json_file(
            &root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json")),
            &serde_json::json!({
                "inferenceProvider": "gateway",
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080",
                "inferenceGatewayApiKey": "ccds_gateway_key",
                "inferenceGatewayAuthScheme": "bearer",
                "inferenceGatewayHeaders": "[\"X-Test: ok\"]",
                "inferenceModels": "[{\"name\":\"claude-deepseek-v4-pro\",\"supports1m\":true}]"
            }),
        )
        .unwrap();

        let readback = read_local_config_library(&root).unwrap();

        assert_eq!(readback.gateway_headers, vec!["X-Test: ok"]);
        assert_eq!(readback.inference_models[0].id, "claude-deepseek-v4-pro");
        assert!(readback.inference_models[0].supports_1m);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_local_config_library_removes_only_ccds_config_and_active_meta() {
        let root = temp_root("clear-ccds");
        fs::create_dir_all(&root).unwrap();
        let ccds_path = root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json"));
        let unrelated_path = root.join("manual-profile.json");
        write_json_file(
            &root.join("_meta.json"),
            &serde_json::json!({
                "activeConfigId": CCDS_LOCAL_CONFIG_ID
            }),
        )
        .unwrap();
        write_json_file(
            &ccds_path,
            &serde_json::json!({
                "inferenceProvider": "gateway",
                "inferenceGatewayBaseUrl": "http://127.0.0.1:18080"
            }),
        )
        .unwrap();
        write_json_file(
            &unrelated_path,
            &serde_json::json!({
                "name": "Manual profile"
            }),
        )
        .unwrap();

        let result = clear_local_config_library(&root).unwrap();

        assert!(result.success);
        assert!(result.removed_config);
        assert!(result.cleared_active_config);
        assert!(result.readback_cleared);
        assert!(!ccds_path.exists());
        assert!(unrelated_path.exists());
        assert!(!root.join("_meta.json").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_local_config_library_preserves_unrelated_active_meta() {
        let root = temp_root("clear-preserve-active");
        fs::create_dir_all(&root).unwrap();
        let ccds_path = root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json"));
        let unrelated_path = root.join("manual-profile.json");
        write_json_file(
            &root.join("_meta.json"),
            &serde_json::json!({
                "activeConfigId": "manual-profile",
                "lastUsed": "keep"
            }),
        )
        .unwrap();
        write_json_file(&ccds_path, &serde_json::json!({"name": "CCDS"})).unwrap();
        write_json_file(&unrelated_path, &serde_json::json!({"name": "Manual"})).unwrap();

        let result = clear_local_config_library(&root).unwrap();
        let meta = read_json_object(&root.join("_meta.json")).unwrap();

        assert!(result.success);
        assert!(result.removed_config);
        assert!(!result.cleared_active_config);
        assert!(result.preserved_meta);
        assert_eq!(
            string_value(meta.get("activeConfigId")).as_deref(),
            Some("manual-profile")
        );
        assert_eq!(string_value(meta.get("lastUsed")).as_deref(), Some("keep"));
        assert!(!ccds_path.exists());
        assert!(unrelated_path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clear_local_config_library_does_not_delete_config_when_meta_parse_fails() {
        let root = temp_root("clear-invalid-meta");
        fs::create_dir_all(&root).unwrap();
        let ccds_path = root.join(format!("{CCDS_LOCAL_CONFIG_ID}.json"));
        fs::write(root.join("_meta.json"), "{not valid json").unwrap();
        write_json_file(&ccds_path, &serde_json::json!({"name": "CCDS"})).unwrap();

        let _error = clear_local_config_library(&root).unwrap_err();

        assert!(ccds_path.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_writer_paths_use_current_user_locations() {
        let windows_path = local_config_library_path(
            DesktopPlatform::Windows,
            Path::new("C:\\Users\\Alice"),
            Some(Path::new("C:\\Users\\Alice\\AppData\\Local")),
        );
        let macos_path =
            local_config_library_path(DesktopPlatform::Macos, Path::new("/Users/alice"), None);

        assert!(windows_path.ends_with(Path::new("Claude-3p").join("configLibrary")));
        assert!(macos_path.ends_with(
            Path::new("Library")
                .join("Application Support")
                .join("Claude-3p")
                .join("configLibrary")
        ));
    }

    #[test]
    fn desktop_config_probe_uses_local_config_path_when_unmanaged() {
        let root = temp_root("probe-unmanaged");
        let probe = probe_desktop_config(DesktopPlatform::Windows, &root, None, &[]);

        assert!(!probe.managed_detected);
        assert!(probe.local_config_library.ends_with(
            Path::new("AppData")
                .join("Local")
                .join("Claude-3p")
                .join("configLibrary")
        ));
        assert_eq!(probe.issue_codes, vec!["desktop.local_config_available"]);
    }

    #[test]
    fn desktop_config_probe_uses_local_app_data_on_windows() {
        let root = temp_root("probe-local-app-data");
        let local_app_data = root.join("LocalAppData");
        let probe =
            probe_desktop_config(DesktopPlatform::Windows, &root, Some(&local_app_data), &[]);

        assert_eq!(
            probe.local_config_library,
            local_app_data.join("Claude-3p").join("configLibrary")
        );
    }

    #[test]
    fn desktop_config_probe_reports_managed_policy_path() {
        let root = temp_root("probe-managed");
        let managed_path = root.join("managed").join("com.anthropic.claude.plist");
        fs::create_dir_all(managed_path.parent().unwrap()).unwrap();
        fs::write(&managed_path, "managed").unwrap();

        let probe = probe_desktop_config(
            DesktopPlatform::Macos,
            &root,
            None,
            std::slice::from_ref(&managed_path),
        );

        assert!(probe.managed_detected);
        assert_eq!(probe.issue_codes, vec!["desktop.managed_config_detected"]);
        assert_eq!(
            probe.managed_evidence[0].location,
            managed_path.display().to_string()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn desktop_config_probe_surfaces_ccds_managed_policy_code() {
        let root = temp_root("probe-ccds-managed");
        let mut probe = DesktopConfigProbe {
            platform: DesktopPlatform::Windows,
            local_config_library: root.join("Claude-3p").join("configLibrary"),
            managed_detected: false,
            managed_evidence: vec![ManagedConfigEvidence {
                code: "desktop.ccds_managed_policy_detected".to_owned(),
                location: "HKCU\\SOFTWARE\\Policies\\Claude".to_owned(),
                detail: "CC Desktop Switch managed registry policy exists".to_owned(),
            }],
            issue_codes: Vec::new(),
        };

        refresh_managed_probe_status(&mut probe);
        refresh_managed_probe_status(&mut probe);

        assert!(probe.managed_detected);
        assert_eq!(
            probe.issue_codes,
            vec![
                "desktop.managed_config_detected",
                "desktop.ccds_managed_policy_detected"
            ]
        );
    }
}
