use serde::{de::DeserializeOwned, Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn raw_invoke_without_args(cmd: &str) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(catch, js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn raw_invoke_with_args(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
}

async fn invoke_without_args<T>(cmd: &str) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = raw_invoke_without_args(cmd)
        .await
        .map_err(command_error_to_string)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

async fn invoke_with_args<T>(cmd: &str, args: JsValue) -> Result<T, String>
where
    T: DeserializeOwned,
{
    let value = raw_invoke_with_args(cmd, args)
        .await
        .map_err(command_error_to_string)?;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

fn command_error_to_string(error: JsValue) -> String {
    if let Some(text) = error.as_string() {
        return text;
    }
    match serde_wasm_bindgen::from_value::<serde_json::Value>(error.clone()) {
        Ok(value) => format_command_error_value(&value),
        Err(_) => format!("{error:?}"),
    }
}

fn format_command_error_value(value: &serde_json::Value) -> String {
    let code = value.get("code").and_then(serde_json::Value::as_str);
    let message = value.get("message").and_then(serde_json::Value::as_str);
    match (code, message) {
        (Some(code), Some(message)) if !message.starts_with(code) => {
            format!("{code}: {message}")
        }
        (_, Some(message)) => message.to_owned(),
        (Some(code), None) => code.to_owned(),
        _ => value.to_string(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub display_name: String,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub api_key: String,
    pub api_format: ApiFormat,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    Anthropic,
    OpenAiChat,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    Bearer,
    XApiKey,
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub api_format: ApiFormat,
    pub has_api_key: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelSlot {
    Sonnet,
    Opus,
    Haiku,
    Default,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMappingDraft {
    pub slot: ModelSlot,
    pub upstream_model: String,
    pub route_id: Option<String>,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMappingSummary {
    pub slot: ModelSlot,
    pub upstream_model: String,
    pub route_id: Option<String>,
    pub desktop_visible: bool,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupSummary {
    pub file_name: String,
    pub path: String,
    pub size: u64,
    pub modified_unix_ms: Option<u128>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettings {
    pub theme: String,
    pub language: String,
    pub proxy_port: u16,
    pub update_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSnapshot {
    pub schema_version: u32,
    pub version: String,
    pub active_provider: Option<String>,
    pub gateway_api_key_present: bool,
    pub providers: Vec<ProviderSummary>,
    pub settings: ConfigSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub preset_id: String,
    pub display_name: String,
    pub base_url: String,
    pub api_format: ApiFormat,
    pub model_mappings: Vec<ModelMappingSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHealth {
    pub mode: GatewayMode,
    pub running: bool,
    pub base_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStats {
    pub total: u64,
    pub success: u64,
    pub failed: u64,
    pub today: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub running: bool,
    pub port: u16,
    pub base_url: String,
    pub stats: ProxyStats,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssetInfo {
    pub name: String,
    pub url: String,
    pub sha256: Option<String>,
    pub signature: Option<String>,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub available: bool,
    pub platform: String,
    pub asset: Option<UpdateAssetInfo>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadResult {
    pub check: UpdateCheckResult,
    pub asset_path: String,
    pub staging_dir: String,
    pub bytes: u64,
    pub sha256_verified: bool,
    pub signature_verified: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstallResult {
    pub launched: bool,
    pub installer_path: String,
    pub installer_type: String,
    pub launch_method: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayMode {
    LocalGateway,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopConfigProbe {
    pub platform: DesktopPlatform,
    pub local_config_library: String,
    pub managed_detected: bool,
    pub managed_evidence: Vec<ManagedConfigEvidence>,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopClearResult {
    pub success: bool,
    pub config_id: String,
    pub local_config_library: String,
    pub config_path: String,
    pub meta_path: String,
    pub removed_config: bool,
    pub cleared_active_config: bool,
    pub preserved_meta: bool,
    pub readback_cleared: bool,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRestartResult {
    pub platform: DesktopPlatform,
    pub stopped_processes: u32,
    pub forced_processes: u32,
    pub launched: bool,
    pub executable: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPlatform {
    Windows,
    Macos,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedConfigEvidence {
    pub code: String,
    pub location: String,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessSnapshot {
    pub provider_configured: bool,
    pub desktop_readback_passed: bool,
    pub provider_smoke_passed: bool,
    pub gateway_smoke_passed: bool,
    pub gateway: GatewayHealth,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopModel {
    pub id: String,
    pub display_name: String,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyDryRun {
    pub mode: String,
    pub success: bool,
    pub expected_base_url: String,
    pub expected_models: Vec<DesktopModel>,
    pub plan_error: Option<String>,
    pub steps: Vec<ApplyStep>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStep {
    pub id: String,
    pub label: String,
    pub would_run: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApplyResult {
    pub mode: String,
    pub success: bool,
    pub gateway: Option<GatewayHealth>,
    pub desktop_config: Option<DesktopConfigProbe>,
    pub write: Option<DesktopWriteResult>,
    pub steps: Vec<DesktopApplyStep>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopWriteResult {
    pub config_id: String,
    pub config_path: String,
    pub meta_path: String,
    pub readback: ReadinessDesktopReadback,
    pub health: DesktopHealth,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessDesktopReadback {
    pub base_url: Option<String>,
    pub inference_models: Vec<DesktopModel>,
    pub mode: Option<String>,
    pub auth_scheme: Option<String>,
    pub gateway_api_key_present: Option<bool>,
    pub gateway_headers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopHealth {
    pub passed: bool,
    pub issues: Vec<DesktopHealthIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopHealthIssue {
    pub code: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopApplyStep {
    pub id: String,
    pub label: String,
    pub status: DesktopApplyStepStatus,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopApplyStepStatus {
    Pending,
    Passed,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsIssueDraft {
    pub title: String,
    pub body: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmokeCheckResult {
    pub layer: String,
    pub passed: bool,
    pub issue_code: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsLogEntry {
    #[serde(default)]
    pub id: u64,
    pub timestamp_unix_ms: u128,
    pub level: String,
    pub code: String,
    pub message: String,
}

pub async fn save_provider(request: ProviderDraft) -> Result<ProviderSummary, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "request": request }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("save_provider", args).await
}

pub async fn list_providers() -> Result<Vec<ProviderSummary>, String> {
    invoke_without_args("list_providers").await
}

pub async fn set_active_provider(provider_id: String) -> Result<bool, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerId": provider_id }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("set_active_provider", args).await
}

pub async fn delete_provider(provider_id: String) -> Result<bool, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerId": provider_id }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("delete_provider", args).await
}

pub async fn export_providers() -> Result<serde_json::Value, String> {
    invoke_without_args("export_providers").await
}

pub async fn save_provider_export_as() -> Result<Option<String>, String> {
    invoke_without_args("save_provider_export_as").await
}

pub async fn preview_provider_import(
    raw_json: String,
    replace_existing: bool,
    skip_existing: bool,
) -> Result<serde_json::Value, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "request": {
            "rawJson": raw_json,
            "replaceExisting": replace_existing,
            "skipExisting": skip_existing
        }
    }))
    .map_err(|error| error.to_string())?;
    invoke_with_args("preview_provider_import", args).await
}

pub async fn import_providers(
    raw_json: String,
    replace_existing: bool,
    skip_existing: bool,
) -> Result<serde_json::Value, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "request": {
            "rawJson": raw_json,
            "replaceExisting": replace_existing,
            "skipExisting": skip_existing
        }
    }))
    .map_err(|error| error.to_string())?;
    invoke_with_args("import_providers", args).await
}

pub async fn list_provider_presets() -> Result<Vec<ProviderPreset>, String> {
    invoke_without_args("list_provider_presets").await
}

pub async fn preview_provider_preset_import(
    preset_id: String,
    replace_existing: bool,
) -> Result<serde_json::Value, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "request": {
            "presetId": preset_id,
            "apiKey": "",
            "replaceExisting": replace_existing
        }
    }))
    .map_err(|error| error.to_string())?;
    invoke_with_args("preview_provider_preset_import", args).await
}

pub async fn import_provider_preset(
    preset_id: String,
    api_key: String,
    replace_existing: bool,
) -> Result<serde_json::Value, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "request": {
            "presetId": preset_id,
            "apiKey": api_key,
            "replaceExisting": replace_existing
        }
    }))
    .map_err(|error| error.to_string())?;
    invoke_with_args("import_provider_preset", args).await
}

pub async fn list_model_mappings(provider_id: String) -> Result<Vec<ModelMappingSummary>, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerId": provider_id }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("list_model_mappings", args).await
}

pub async fn update_model_mappings(
    provider_id: String,
    mappings: Vec<ModelMappingDraft>,
) -> Result<Vec<ModelMappingSummary>, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "request": {
            "providerId": provider_id,
            "mappings": mappings
        }
    }))
    .map_err(|error| error.to_string())?;
    invoke_with_args("update_model_mappings", args).await
}

pub async fn list_config_backups() -> Result<Vec<ConfigBackupSummary>, String> {
    invoke_without_args("list_config_backups").await
}

pub async fn read_config_backup(file_name: String) -> Result<String, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "fileName": file_name }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("read_config_backup", args).await
}

pub async fn create_config_backup() -> Result<Option<ConfigBackupSummary>, String> {
    invoke_without_args("create_config_backup").await
}

pub async fn get_config_snapshot() -> Result<ConfigSnapshot, String> {
    invoke_without_args("get_config_snapshot").await
}

pub async fn get_settings() -> Result<ConfigSettings, String> {
    invoke_without_args("get_settings").await
}

pub async fn update_settings(settings: ConfigSettings) -> Result<ConfigSettings, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "settings": settings }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("update_settings", args).await
}

pub async fn health() -> Result<ReadinessSnapshot, String> {
    invoke_without_args("health").await
}

pub async fn get_proxy_status() -> Result<ProxyStatus, String> {
    invoke_without_args("get_proxy_status").await
}

pub async fn start_gateway() -> Result<GatewayHealth, String> {
    invoke_without_args("start_gateway").await
}

pub async fn stop_gateway() -> Result<GatewayHealth, String> {
    invoke_without_args("stop_gateway").await
}

pub async fn get_proxy_logs() -> Result<Vec<DiagnosticsLogEntry>, String> {
    invoke_without_args("get_proxy_logs").await
}

pub async fn clear_proxy_logs() -> Result<bool, String> {
    invoke_without_args("clear_proxy_logs").await
}

pub async fn desktop_config_probe() -> Result<DesktopConfigProbe, String> {
    invoke_without_args("desktop_config_probe").await
}

pub async fn clear_desktop_config() -> Result<DesktopClearResult, String> {
    invoke_without_args("clear_desktop_config").await
}

pub async fn restart_claude_desktop() -> Result<DesktopRestartResult, String> {
    invoke_without_args("restart_claude_desktop").await
}

pub async fn apply_dry_run() -> Result<ApplyDryRun, String> {
    invoke_without_args("apply_dry_run").await
}

pub async fn apply_detected_local_config() -> Result<DesktopApplyResult, String> {
    invoke_without_args("apply_detected_local_config").await
}

pub async fn copy_diagnostics_summary() -> Result<String, String> {
    invoke_without_args("copy_diagnostics_summary").await
}

pub async fn copy_diagnostics_summary_to_clipboard() -> Result<String, String> {
    invoke_without_args("copy_diagnostics_summary_to_clipboard").await
}

pub async fn copy_text_to_clipboard(text: String) -> Result<bool, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "text": text }))
        .map_err(|error| error.to_string())?;
    invoke_with_args("copy_text_to_clipboard_command", args).await
}

pub async fn export_diagnostics_package() -> Result<serde_json::Value, String> {
    invoke_without_args("export_diagnostics_package").await
}

pub async fn save_diagnostics_package() -> Result<String, String> {
    invoke_without_args("save_diagnostics_package").await
}

pub async fn save_diagnostics_package_as() -> Result<Option<String>, String> {
    invoke_without_args("save_diagnostics_package_as").await
}

pub async fn diagnostics_issue_draft() -> Result<DiagnosticsIssueDraft, String> {
    invoke_without_args("diagnostics_issue_draft").await
}

pub async fn open_diagnostics_issue() -> Result<DiagnosticsIssueDraft, String> {
    invoke_without_args("open_diagnostics_issue").await
}

pub async fn provider_static_smoke() -> Result<SmokeCheckResult, String> {
    invoke_without_args("provider_static_smoke").await
}

pub async fn gateway_smoke() -> Result<SmokeCheckResult, String> {
    invoke_without_args("gateway_smoke").await
}

pub async fn provider_real_smoke() -> Result<SmokeCheckResult, String> {
    invoke_without_args("provider_real_smoke").await
}

pub async fn check_update() -> Result<UpdateCheckResult, String> {
    invoke_without_args("check_update").await
}

pub async fn download_update() -> Result<UpdateDownloadResult, String> {
    invoke_without_args("download_update").await
}

pub async fn install_update(installer_path: String) -> Result<UpdateInstallResult, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({
        "installerPath": installer_path,
    }))
    .map_err(|error| error.to_string())?;
    invoke_with_args("install_update", args).await
}
