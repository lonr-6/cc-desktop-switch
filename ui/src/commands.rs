use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke_without_args(cmd: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke)]
    async fn invoke_with_args(cmd: &str, args: JsValue) -> JsValue;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDraft {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    pub display_name: String,
    pub base_url: String,
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
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
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

pub async fn save_provider(request: ProviderDraft) -> Result<ProviderSummary, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "request": request }))
        .map_err(|error| error.to_string())?;
    let value = invoke_with_args("save_provider", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn list_providers() -> Result<Vec<ProviderSummary>, String> {
    let value = invoke_without_args("list_providers").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn set_active_provider(provider_id: String) -> Result<bool, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerId": provider_id }))
        .map_err(|error| error.to_string())?;
    let value = invoke_with_args("set_active_provider", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn delete_provider(provider_id: String) -> Result<bool, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerId": provider_id }))
        .map_err(|error| error.to_string())?;
    let value = invoke_with_args("delete_provider", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn reorder_providers(provider_ids: Vec<String>) -> Result<bool, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerIds": provider_ids }))
        .map_err(|error| error.to_string())?;
    let value = invoke_with_args("reorder_providers", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn export_providers() -> Result<serde_json::Value, String> {
    let value = invoke_without_args("export_providers").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn save_provider_export_as() -> Result<Option<String>, String> {
    let value = invoke_without_args("save_provider_export_as").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
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
    let value = invoke_with_args("preview_provider_import", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
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
    let value = invoke_with_args("import_providers", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn list_provider_presets() -> Result<Vec<ProviderPreset>, String> {
    let value = invoke_without_args("list_provider_presets").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
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
    let value = invoke_with_args("preview_provider_preset_import", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
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
    let value = invoke_with_args("import_provider_preset", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn list_model_mappings(provider_id: String) -> Result<Vec<ModelMappingSummary>, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "providerId": provider_id }))
        .map_err(|error| error.to_string())?;
    let value = invoke_with_args("list_model_mappings", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
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
    let value = invoke_with_args("update_model_mappings", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn list_config_backups() -> Result<Vec<ConfigBackupSummary>, String> {
    let value = invoke_without_args("list_config_backups").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn read_config_backup(file_name: String) -> Result<String, String> {
    let args = serde_wasm_bindgen::to_value(&serde_json::json!({ "fileName": file_name }))
        .map_err(|error| error.to_string())?;
    let value = invoke_with_args("read_config_backup", args).await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn health() -> Result<ReadinessSnapshot, String> {
    let value = invoke_without_args("health").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn gateway_status() -> Result<GatewayHealth, String> {
    let value = invoke_without_args("gateway_status").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn start_gateway() -> Result<GatewayHealth, String> {
    let value = invoke_without_args("start_gateway").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn stop_gateway() -> Result<GatewayHealth, String> {
    let value = invoke_without_args("stop_gateway").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn desktop_config_probe() -> Result<DesktopConfigProbe, String> {
    let value = invoke_without_args("desktop_config_probe").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn apply_dry_run() -> Result<ApplyDryRun, String> {
    let value = invoke_without_args("apply_dry_run").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn apply_detected_local_config() -> Result<DesktopApplyResult, String> {
    let value = invoke_without_args("apply_detected_local_config").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn copy_diagnostics_summary() -> Result<String, String> {
    let value = invoke_without_args("copy_diagnostics_summary").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn copy_diagnostics_summary_to_clipboard() -> Result<String, String> {
    let value = invoke_without_args("copy_diagnostics_summary_to_clipboard").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn export_diagnostics_package() -> Result<serde_json::Value, String> {
    let value = invoke_without_args("export_diagnostics_package").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn save_diagnostics_package() -> Result<String, String> {
    let value = invoke_without_args("save_diagnostics_package").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn save_diagnostics_package_as() -> Result<Option<String>, String> {
    let value = invoke_without_args("save_diagnostics_package_as").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn diagnostics_issue_draft() -> Result<DiagnosticsIssueDraft, String> {
    let value = invoke_without_args("diagnostics_issue_draft").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn open_diagnostics_issue() -> Result<DiagnosticsIssueDraft, String> {
    let value = invoke_without_args("open_diagnostics_issue").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn provider_static_smoke() -> Result<SmokeCheckResult, String> {
    let value = invoke_without_args("provider_static_smoke").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn gateway_smoke() -> Result<SmokeCheckResult, String> {
    let value = invoke_without_args("gateway_smoke").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}

pub async fn provider_real_smoke() -> Result<SmokeCheckResult, String> {
    let value = invoke_without_args("provider_real_smoke").await;
    serde_wasm_bindgen::from_value(value).map_err(|error| error.to_string())
}
