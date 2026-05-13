use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model_catalog::{route_for_model, ModelMapping, ModelSlot, RouteCapabilities};
use crate::provider::{ApiFormat, AuthScheme, Provider, ProviderSummary};

pub const RUST_SCHEMA_VERSION: u32 = 1;
pub const PROVIDER_EXPORT_KIND: &str = "ccds.providerExport";
pub const PROVIDER_TEMPLATE_KIND: &str = "ccds.providerTemplate";
pub const PROVIDER_MARKETPLACE_KIND: &str = "ccds.providerMarketplace";
const FORBIDDEN_TEMPLATE_SECRET_FIELDS: &[&str] = &[
    "apikey",
    "gatewayapikey",
    "inferencegatewayapikey",
    "authorization",
    "cookie",
    "headers",
    "secret",
    "token",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub version: String,
    pub active_provider: Option<String>,
    pub gateway_api_key: Option<String>,
    pub providers: Vec<ConfigProvider>,
    pub settings: ConfigSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigProvider {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub api_format: ApiFormat,
    pub api_key: String,
    pub model_mappings: Vec<ModelMapping>,
    pub sort_index: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSettings {
    pub theme: String,
    pub language: String,
    pub proxy_port: u16,
    pub update_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReport {
    pub source_schema: SourceSchema,
    pub provider_count: usize,
    pub active_provider: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceSchema {
    PythonStable,
    Rust,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LoadedConfig {
    pub config: AppConfig,
    pub report: MigrationReport,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupMeta {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExportPackage {
    pub schema_version: u32,
    pub kind: String,
    pub active_provider: Option<String>,
    pub providers: Vec<ConfigProvider>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplatePackage {
    pub schema_version: u32,
    pub kind: String,
    pub templates: Vec<ProviderTemplate>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTemplate {
    pub template_id: String,
    pub display_name: String,
    pub base_url: String,
    #[serde(
        default = "default_api_format",
        deserialize_with = "deserialize_api_format_alias"
    )]
    pub api_format: ApiFormat,
    #[serde(default)]
    pub model_mappings: Vec<ModelMappingDraft>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMarketplacePackage {
    pub schema_version: u32,
    pub kind: String,
    pub source: ProviderMarketplaceSource,
    pub template_sha256: String,
    pub template_package: ProviderTemplatePackage,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMarketplaceSource {
    pub source_id: String,
    pub display_name: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderImportSource {
    ProviderExport,
    RustConfig,
    CcSwitchLegacy,
    Preset,
    ProviderTemplate,
    ProviderMarketplace,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportConflict {
    pub provider_id: String,
    pub existing_display_name: String,
    pub incoming_display_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportPreview {
    pub source_schema: ProviderImportSource,
    pub incoming_provider_count: usize,
    pub importable_provider_count: usize,
    pub conflict_count: usize,
    pub unresolved_conflict_count: usize,
    pub skipped_conflict_count: usize,
    pub replaced_conflict_count: usize,
    pub providers: Vec<ProviderSummary>,
    pub conflicts: Vec<ProviderImportConflict>,
    pub issue_codes: Vec<String>,
    pub would_write: bool,
    pub replace_existing: bool,
    pub skip_existing: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportApplyResult {
    pub preview: ProviderImportPreview,
    pub changed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelMappingDraft {
    pub slot: ModelSlot,
    pub upstream_model: String,
    pub route_id: Option<String>,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelMappingSummary {
    pub slot: ModelSlot,
    pub upstream_model: String,
    pub route_id: Option<String>,
    pub desktop_visible: bool,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupSummary {
    pub file_name: String,
    pub path: String,
    pub size: u64,
    pub modified_unix_ms: Option<u128>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPreset {
    pub preset_id: String,
    pub display_name: String,
    pub base_url: String,
    pub api_format: ApiFormat,
    pub model_mappings: Vec<ModelMappingSummary>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config must be a JSON object")]
    NotObject,
    #[error("invalid config JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider import package is invalid: {0}")]
    InvalidProviderImport(String),
    #[error("config backup is invalid: {0}")]
    InvalidConfigBackup(String),
}

impl AppConfig {
    pub fn empty() -> Self {
        Self {
            schema_version: RUST_SCHEMA_VERSION,
            version: "1.1.0-rc1".to_owned(),
            active_provider: None,
            gateway_api_key: None,
            providers: Vec::new(),
            settings: ConfigSettings::default(),
        }
    }

    pub fn load_json(raw: &str) -> Result<LoadedConfig, ConfigError> {
        let value: Value = serde_json::from_str(raw)?;
        let object = value.as_object().ok_or(ConfigError::NotObject)?;

        if object.get("schemaVersion").is_some() {
            let mut config: AppConfig = serde_json::from_value(Value::Object(object.clone()))?;
            normalize_rust_config(&mut config);
            return Ok(LoadedConfig {
                report: MigrationReport {
                    source_schema: SourceSchema::Rust,
                    provider_count: config.providers.len(),
                    active_provider: config.active_provider.clone(),
                },
                config,
            });
        }

        let config = migrate_python_config(&value)?;
        Ok(LoadedConfig {
            report: MigrationReport {
                source_schema: SourceSchema::PythonStable,
                provider_count: config.providers.len(),
                active_provider: config.active_provider.clone(),
            },
            config,
        })
    }

    pub fn active_provider(&self) -> Option<&ConfigProvider> {
        let active_provider = self.active_provider.as_deref()?;
        self.providers
            .iter()
            .find(|provider| provider.provider_id == active_provider)
    }

    pub fn upsert_provider(&mut self, provider: ConfigProvider) {
        if let Some(existing) = self
            .providers
            .iter_mut()
            .find(|existing| existing.provider_id == provider.provider_id)
        {
            let sort_index = existing.sort_index;
            let model_mappings = existing.model_mappings.clone();
            let api_key = existing.api_key.clone();
            *existing = provider;
            existing.sort_index = sort_index;
            if existing.api_key.is_empty() {
                existing.api_key = api_key;
            }
            if !model_mappings.is_empty() {
                existing.model_mappings = model_mappings;
            }
        } else {
            let mut provider = provider;
            provider.sort_index = self.providers.len() as u32;
            self.providers.push(provider);
        }

        if self.active_provider.is_none() {
            self.active_provider = self
                .providers
                .first()
                .map(|provider| provider.provider_id.clone());
        }
    }

    pub fn set_active_provider(&mut self, provider_id: &str) -> bool {
        let provider_id = stable_id(provider_id);
        if self
            .providers
            .iter()
            .any(|provider| provider.provider_id == provider_id)
        {
            self.active_provider = Some(provider_id);
            true
        } else {
            false
        }
    }

    pub fn delete_provider(&mut self, provider_id: &str) -> bool {
        let provider_id = stable_id(provider_id);
        let before_len = self.providers.len();
        self.providers
            .retain(|provider| provider.provider_id != provider_id);
        if self.providers.len() == before_len {
            return false;
        }

        normalize_provider_sort(&mut self.providers);
        if self.active_provider.as_deref() == Some(provider_id.as_str()) {
            self.active_provider = self
                .providers
                .first()
                .map(|provider| provider.provider_id.clone());
        }
        true
    }

    pub fn reorder_providers(&mut self, provider_ids: &[String]) -> Result<bool, String> {
        let provider_ids = provider_ids
            .iter()
            .map(|id| stable_id(id))
            .collect::<Vec<_>>();
        let mut requested = provider_ids.clone();
        requested.sort();
        requested.dedup();
        if requested.len() != provider_ids.len() || provider_ids.len() != self.providers.len() {
            return Err(
                "provider.reorder_invalid_set: providerIds must contain every provider exactly once"
                    .to_owned(),
            );
        }

        let mut existing = self
            .providers
            .iter()
            .map(|provider| provider.provider_id.clone())
            .collect::<Vec<_>>();
        existing.sort();
        if requested != existing {
            return Err("provider.reorder_invalid_set: providerIds must contain every provider exactly once".to_owned());
        }

        let before = self
            .providers
            .iter()
            .map(|provider| provider.provider_id.clone())
            .collect::<Vec<_>>();
        let providers = std::mem::take(&mut self.providers);
        self.providers = provider_ids
            .iter()
            .filter_map(|id| {
                providers
                    .iter()
                    .find(|provider| provider.provider_id == *id)
                    .cloned()
            })
            .collect();
        normalize_provider_sort(&mut self.providers);

        Ok(before != provider_ids)
    }

    pub fn export_provider_package(&self) -> ProviderExportPackage {
        ProviderExportPackage {
            schema_version: RUST_SCHEMA_VERSION,
            kind: PROVIDER_EXPORT_KIND.to_owned(),
            active_provider: self.active_provider.clone(),
            providers: self.providers.clone(),
        }
    }

    pub fn export_provider_package_redacted(&self) -> ProviderExportPackage {
        let mut package = self.export_provider_package();
        for provider in &mut package.providers {
            provider.api_key.clear();
        }
        package
    }

    pub fn preview_provider_import(
        &self,
        raw_json: &str,
        replace_existing: bool,
    ) -> Result<ProviderImportPreview, ConfigError> {
        let (source_schema, incoming) = parse_provider_import(raw_json)?;
        Ok(build_provider_import_preview(
            self,
            &incoming,
            source_schema,
            replace_existing,
            false,
        ))
    }

    pub fn preview_provider_import_with_merge(
        &self,
        raw_json: &str,
        replace_existing: bool,
        skip_existing: bool,
    ) -> Result<ProviderImportPreview, ConfigError> {
        let (source_schema, incoming) = parse_provider_import(raw_json)?;
        Ok(build_provider_import_preview(
            self,
            &incoming,
            source_schema,
            replace_existing,
            skip_existing,
        ))
    }

    pub fn import_providers(
        &mut self,
        raw_json: &str,
        replace_existing: bool,
    ) -> Result<ProviderImportApplyResult, ConfigError> {
        self.import_providers_with_merge(raw_json, replace_existing, false)
    }

    pub fn import_providers_with_merge(
        &mut self,
        raw_json: &str,
        replace_existing: bool,
        skip_existing: bool,
    ) -> Result<ProviderImportApplyResult, ConfigError> {
        let (source_schema, incoming) = parse_provider_import(raw_json)?;
        let preview = build_provider_import_preview(
            self,
            &incoming,
            source_schema,
            replace_existing,
            skip_existing,
        );
        if !preview.would_write {
            return Ok(ProviderImportApplyResult {
                preview,
                changed: false,
            });
        }

        let incoming_active_provider = incoming.active_provider.clone();
        let mut changed = false;
        for mut incoming_provider in incoming.providers {
            if let Some(existing) = self
                .providers
                .iter_mut()
                .find(|provider| provider.provider_id == incoming_provider.provider_id)
            {
                if skip_existing && !replace_existing {
                    continue;
                }
                incoming_provider.sort_index = existing.sort_index;
                if *existing != incoming_provider {
                    *existing = incoming_provider;
                    changed = true;
                }
            } else {
                incoming_provider.sort_index = self.providers.len() as u32;
                self.providers.push(incoming_provider);
                changed = true;
            }
        }

        normalize_provider_sort(&mut self.providers);
        if self.active_provider().is_none() {
            self.active_provider = incoming_active_provider
                .filter(|provider_id| {
                    self.providers
                        .iter()
                        .any(|provider| &provider.provider_id == provider_id)
                })
                .or_else(|| {
                    self.providers
                        .first()
                        .map(|provider| provider.provider_id.clone())
                });
        }

        Ok(ProviderImportApplyResult { preview, changed })
    }

    pub fn preview_provider_preset_import(
        &self,
        preset_id: &str,
        replace_existing: bool,
    ) -> Result<ProviderImportPreview, ConfigError> {
        let incoming = config_from_provider_preset(preset_id, String::new())?;
        Ok(build_provider_import_preview(
            self,
            &incoming,
            ProviderImportSource::Preset,
            replace_existing,
            false,
        ))
    }

    pub fn import_provider_preset(
        &mut self,
        preset_id: &str,
        api_key: String,
        replace_existing: bool,
    ) -> Result<ProviderImportApplyResult, ConfigError> {
        let incoming = config_from_provider_preset(preset_id, api_key)?;
        let preview = build_provider_import_preview(
            self,
            &incoming,
            ProviderImportSource::Preset,
            replace_existing,
            false,
        );
        if !preview.would_write {
            return Ok(ProviderImportApplyResult {
                preview,
                changed: false,
            });
        }

        let mut changed = false;
        for incoming_provider in incoming.providers {
            if let Some(existing) = self
                .providers
                .iter_mut()
                .find(|provider| provider.provider_id == incoming_provider.provider_id)
            {
                let mut next_provider = incoming_provider;
                next_provider.sort_index = existing.sort_index;
                if next_provider.api_key.is_empty() {
                    next_provider.api_key = existing.api_key.clone();
                }
                if *existing != next_provider {
                    *existing = next_provider;
                    changed = true;
                }
            } else {
                let mut next_provider = incoming_provider;
                next_provider.sort_index = self.providers.len() as u32;
                self.providers.push(next_provider);
                changed = true;
            }
        }

        normalize_provider_sort(&mut self.providers);
        if self.active_provider().is_none() {
            self.active_provider = self
                .providers
                .first()
                .map(|provider| provider.provider_id.clone());
        }

        Ok(ProviderImportApplyResult { preview, changed })
    }

    pub fn model_mapping_summaries(
        &self,
        provider_id: &str,
    ) -> Result<Vec<ModelMappingSummary>, String> {
        let provider_id = stable_id(provider_id);
        let provider = self
            .providers
            .iter()
            .find(|provider| provider.provider_id == provider_id)
            .ok_or_else(|| format!("provider.not_found: {provider_id}"))?;
        Ok(provider.model_mapping_summaries())
    }

    pub fn update_provider_model_mappings(
        &mut self,
        provider_id: &str,
        drafts: Vec<ModelMappingDraft>,
    ) -> Result<bool, String> {
        let provider_id = stable_id(provider_id);
        let index = self
            .providers
            .iter()
            .position(|provider| provider.provider_id == provider_id)
            .ok_or_else(|| format!("provider.not_found: {provider_id}"))?;
        let provider_for_routes = self.providers[index].as_provider();
        let next_mappings = normalize_model_mapping_drafts(&provider_for_routes, drafts)?;
        let changed = self.providers[index].model_mappings != next_mappings;
        self.providers[index].model_mappings = next_mappings;
        Ok(changed)
    }
}

impl ConfigProvider {
    pub fn as_provider(&self) -> Provider {
        Provider {
            provider_id: self.provider_id.clone(),
            display_name: self.display_name.clone(),
            base_url: self.base_url.clone(),
            auth_scheme: self.auth_scheme.clone(),
            api_format: self.api_format.clone(),
            api_key: self.api_key.clone(),
        }
    }

    pub fn from_provider(provider: Provider, sort_index: u32) -> Self {
        let model_mappings = default_model_mappings(&provider);
        Self {
            provider_id: provider.provider_id.clone(),
            display_name: provider.display_name.clone(),
            base_url: provider.base_url.clone(),
            auth_scheme: provider.auth_scheme.clone(),
            api_format: provider.api_format.clone(),
            api_key: provider.api_key.clone(),
            model_mappings,
            sort_index,
        }
    }

    pub fn summary(&self) -> ProviderSummary {
        self.as_provider().summary()
    }

    pub fn model_mapping_summaries(&self) -> Vec<ModelMappingSummary> {
        let provider_for_routes = self.as_provider();
        self.model_mappings
            .iter()
            .map(|mapping| {
                let upstream_model = mapping.upstream_model.trim().to_owned();
                let desktop_visible =
                    mapping.slot != ModelSlot::Default && !upstream_model.is_empty();
                let route_id = if desktop_visible {
                    mapping
                        .route_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|route_id| !route_id.is_empty())
                        .map(ToOwned::to_owned)
                        .or_else(|| Some(route_for_model(&provider_for_routes, &upstream_model)))
                } else {
                    None
                };
                ModelMappingSummary {
                    slot: mapping.slot.clone(),
                    upstream_model,
                    route_id,
                    desktop_visible,
                    supports_1m: mapping.capabilities.supports_1m,
                    supports_max: mapping.capabilities.supports_max,
                }
            })
            .collect()
    }
}

pub fn provider_presets() -> Vec<ProviderPreset> {
    built_in_provider_presets()
        .into_iter()
        .map(|provider| ProviderPreset {
            preset_id: provider_preset_id(&provider.provider_id),
            display_name: provider.display_name.clone(),
            base_url: provider.base_url.clone(),
            api_format: provider.api_format.clone(),
            model_mappings: provider.model_mapping_summaries(),
        })
        .collect()
}

fn config_from_provider_preset(preset_id: &str, api_key: String) -> Result<AppConfig, ConfigError> {
    let mut provider = built_in_provider_presets()
        .into_iter()
        .find(|provider| provider_preset_id(&provider.provider_id) == stable_id(preset_id))
        .ok_or_else(|| {
            ConfigError::InvalidProviderImport("provider.preset_not_found".to_owned())
        })?;
    provider.api_key = api_key.trim().to_owned();
    let mut config = AppConfig::empty();
    config.active_provider = Some(provider.provider_id.clone());
    config.providers.push(provider);
    Ok(config)
}

fn config_from_provider_template_package(
    package: ProviderTemplatePackage,
) -> Result<AppConfig, ConfigError> {
    if package.schema_version != RUST_SCHEMA_VERSION {
        return Err(ConfigError::InvalidProviderImport(
            "provider.template_unsupported_schema_version".to_owned(),
        ));
    }
    if package.kind != PROVIDER_TEMPLATE_KIND {
        return Err(ConfigError::InvalidProviderImport(
            "provider.template_invalid_kind".to_owned(),
        ));
    }

    let mut config = AppConfig::empty();
    for (index, template) in package.templates.into_iter().enumerate() {
        let display_name = template.display_name.trim().to_owned();
        if display_name.is_empty() {
            return Err(ConfigError::InvalidProviderImport(
                "provider.template_missing_display_name".to_owned(),
            ));
        }
        let base_url = template.base_url.trim().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            return Err(ConfigError::InvalidProviderImport(
                "provider.template_missing_base_url".to_owned(),
            ));
        }
        if !is_http_base_url(&base_url) {
            return Err(ConfigError::InvalidProviderImport(
                "provider.template_invalid_base_url".to_owned(),
            ));
        }
        let provider = Provider {
            provider_id: provider_id_from_template(&template.template_id, &display_name),
            display_name,
            base_url,
            auth_scheme: AuthScheme::Bearer,
            api_format: template.api_format,
            api_key: String::new(),
        };
        let model_mappings = normalize_model_mapping_drafts(&provider, template.model_mappings)
            .map_err(ConfigError::InvalidProviderImport)?;
        config.providers.push(ConfigProvider {
            provider_id: provider.provider_id,
            display_name: provider.display_name,
            base_url: provider.base_url,
            auth_scheme: AuthScheme::Bearer,
            api_format: provider.api_format,
            api_key: String::new(),
            model_mappings,
            sort_index: index as u32,
        });
    }

    validate_unique_provider_ids(&config.providers)?;
    config.active_provider = config
        .providers
        .first()
        .map(|provider| provider.provider_id.clone());
    Ok(config)
}

fn config_from_provider_marketplace_package(
    package: ProviderMarketplacePackage,
) -> Result<AppConfig, ConfigError> {
    if package.schema_version != RUST_SCHEMA_VERSION {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_unsupported_schema_version".to_owned(),
        ));
    }
    if package.kind != PROVIDER_MARKETPLACE_KIND {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_invalid_kind".to_owned(),
        ));
    }
    validate_marketplace_source(&package.source)?;

    let expected = normalize_template_sha256(&package.template_sha256)?;
    let actual = provider_template_package_sha256(&package.template_package)?;
    if expected != actual {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_template_hash_mismatch".to_owned(),
        ));
    }

    config_from_provider_template_package(package.template_package)
}

fn built_in_provider_presets() -> Vec<ConfigProvider> {
    vec![
        ConfigProvider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: String::new(),
            model_mappings: vec![
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
                ModelMapping {
                    slot: ModelSlot::Default,
                    upstream_model: "deepseek-v4-pro".to_owned(),
                    route_id: None,
                    capabilities: RouteCapabilities::default(),
                },
            ],
            sort_index: 0,
        },
        ConfigProvider {
            provider_id: "provider-kimi".to_owned(),
            display_name: "Kimi".to_owned(),
            base_url: "https://api.moonshot.cn/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: String::new(),
            model_mappings: vec![
                ModelMapping {
                    slot: ModelSlot::Sonnet,
                    upstream_model: "kimi-k2.6".to_owned(),
                    route_id: Some("claude-kimi-k2-6".to_owned()),
                    capabilities: RouteCapabilities {
                        supports_1m: false,
                        supports_max: false,
                    },
                },
                ModelMapping {
                    slot: ModelSlot::Default,
                    upstream_model: "kimi-k2.6".to_owned(),
                    route_id: None,
                    capabilities: RouteCapabilities::default(),
                },
            ],
            sort_index: 0,
        },
    ]
}

fn provider_preset_id(provider_id: &str) -> String {
    provider_id
        .trim_start_matches("provider-")
        .trim_matches('-')
        .to_owned()
}

fn provider_id_from_template(template_id: &str, display_name: &str) -> String {
    let id = stable_id(template_id);
    if id.starts_with("provider-") {
        id
    } else if !id.is_empty() {
        format!("provider-{id}")
    } else {
        stable_provider_id(Some(display_name))
    }
}

fn parse_provider_import(raw_json: &str) -> Result<(ProviderImportSource, AppConfig), ConfigError> {
    let value: Value = serde_json::from_str(raw_json)?;
    let object = value.as_object().ok_or(ConfigError::NotObject)?;

    if object.get("kind").and_then(Value::as_str) == Some(PROVIDER_EXPORT_KIND) {
        let package: ProviderExportPackage = serde_json::from_value(value)?;
        if package.schema_version != RUST_SCHEMA_VERSION {
            return Err(ConfigError::InvalidProviderImport(
                "provider.import_unsupported_schema_version".to_owned(),
            ));
        }
        let mut config = AppConfig {
            schema_version: RUST_SCHEMA_VERSION,
            version: "1.1.0-rc1".to_owned(),
            active_provider: package.active_provider,
            gateway_api_key: None,
            providers: package.providers,
            settings: ConfigSettings::default(),
        };
        normalize_rust_config(&mut config);
        normalize_imported_providers(&mut config.providers)?;
        validate_unique_provider_ids(&config.providers)?;
        config.active_provider = active_provider(
            config
                .active_provider
                .as_ref()
                .map(|provider| Value::String(provider.clone()))
                .as_ref(),
            &config.providers,
        );
        return Ok((ProviderImportSource::ProviderExport, config));
    }

    if object.get("kind").and_then(Value::as_str) == Some(PROVIDER_TEMPLATE_KIND) {
        if let Some(field) = find_forbidden_template_secret_field(&value) {
            return Err(ConfigError::InvalidProviderImport(format!(
                "provider.template_secret_field_not_allowed:{field}"
            )));
        }
        let package: ProviderTemplatePackage = serde_json::from_value(value)?;
        let config = config_from_provider_template_package(package)?;
        return Ok((ProviderImportSource::ProviderTemplate, config));
    }

    if object.get("kind").and_then(Value::as_str) == Some(PROVIDER_MARKETPLACE_KIND) {
        if let Some(field) = find_forbidden_template_secret_field(&value) {
            return Err(ConfigError::InvalidProviderImport(format!(
                "provider.template_secret_field_not_allowed:{field}"
            )));
        }
        let package: ProviderMarketplacePackage = serde_json::from_value(value)?;
        let config = config_from_provider_marketplace_package(package)?;
        return Ok((ProviderImportSource::ProviderMarketplace, config));
    }

    let loaded = AppConfig::load_json(raw_json)?;
    let source = match loaded.report.source_schema {
        SourceSchema::Rust => ProviderImportSource::RustConfig,
        SourceSchema::PythonStable => ProviderImportSource::CcSwitchLegacy,
    };
    let mut config = loaded.config;
    normalize_imported_providers(&mut config.providers)?;
    validate_unique_provider_ids(&config.providers)?;
    Ok((source, config))
}

fn build_provider_import_preview(
    current: &AppConfig,
    incoming: &AppConfig,
    source_schema: ProviderImportSource,
    replace_existing: bool,
    skip_existing: bool,
) -> ProviderImportPreview {
    let conflicts = incoming
        .providers
        .iter()
        .filter_map(|incoming_provider| {
            current
                .providers
                .iter()
                .find(|provider| provider.provider_id == incoming_provider.provider_id)
                .map(|existing_provider| ProviderImportConflict {
                    provider_id: incoming_provider.provider_id.clone(),
                    existing_display_name: existing_provider.display_name.clone(),
                    incoming_display_name: incoming_provider.display_name.clone(),
                })
        })
        .collect::<Vec<_>>();
    let conflict_ids = conflicts
        .iter()
        .map(|conflict| conflict.provider_id.clone())
        .collect::<HashSet<_>>();
    let unresolved_conflict_count = if replace_existing || skip_existing {
        0
    } else {
        conflicts.len()
    };
    let skipped_conflict_count = if skip_existing && !replace_existing {
        conflicts.len()
    } else {
        0
    };
    let replaced_conflict_count = if replace_existing { conflicts.len() } else { 0 };

    let mut issue_codes = Vec::new();
    if !conflicts.is_empty() {
        issue_codes.push("provider.import_conflict".to_owned());
    }
    if replace_existing && !conflicts.is_empty() {
        issue_codes.push("provider.import_replace_existing".to_owned());
    }
    if skip_existing && !replace_existing && !conflicts.is_empty() {
        issue_codes.push("provider.import_skip_existing".to_owned());
    }
    if incoming.providers.is_empty() {
        issue_codes.push("provider.import_empty".to_owned());
    }

    let importable_provider_count = if incoming.providers.is_empty()
        || unresolved_conflict_count > 0
    {
        0
    } else {
        incoming
            .providers
            .iter()
            .filter(|provider| replace_existing || !conflict_ids.contains(&provider.provider_id))
            .count()
    };
    if skip_existing && !incoming.providers.is_empty() && importable_provider_count == 0 {
        issue_codes.push("provider.import_noop".to_owned());
    }
    let would_write = importable_provider_count > 0;

    ProviderImportPreview {
        source_schema,
        incoming_provider_count: incoming.providers.len(),
        importable_provider_count,
        conflict_count: conflicts.len(),
        unresolved_conflict_count,
        skipped_conflict_count,
        replaced_conflict_count,
        providers: incoming
            .providers
            .iter()
            .map(ConfigProvider::summary)
            .collect(),
        conflicts,
        issue_codes,
        would_write,
        replace_existing,
        skip_existing,
    }
}

fn normalize_model_mapping_drafts(
    provider: &Provider,
    drafts: Vec<ModelMappingDraft>,
) -> Result<Vec<ModelMapping>, String> {
    let mut mappings = Vec::new();
    let mut route_ids = HashSet::new();

    for draft in drafts {
        let upstream_model = draft.upstream_model.trim().to_owned();
        if upstream_model.is_empty() {
            if draft.slot == ModelSlot::Default {
                continue;
            }
            return Err(
                "model_mapping.missing_upstream_model: upstreamModel is required".to_owned(),
            );
        }

        if draft.slot == ModelSlot::Default {
            mappings.push(ModelMapping {
                slot: draft.slot,
                upstream_model,
                route_id: None,
                capabilities: RouteCapabilities {
                    supports_1m: draft.supports_1m,
                    supports_max: draft.supports_max,
                },
            });
            continue;
        }

        let route_id = draft
            .route_id
            .as_deref()
            .map(str::trim)
            .filter(|route_id| !route_id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| route_for_model(provider, &upstream_model));
        validate_desktop_route_id(&route_id, &upstream_model)?;
        if !route_ids.insert(route_id.clone()) {
            return Err(format!(
                "model_mapping.duplicate_route_id: route '{route_id}' is already mapped"
            ));
        }

        mappings.push(ModelMapping {
            slot: draft.slot,
            upstream_model,
            route_id: Some(route_id),
            capabilities: RouteCapabilities {
                supports_1m: draft.supports_1m,
                supports_max: draft.supports_max,
            },
        });
    }

    if !mappings
        .iter()
        .any(|mapping| mapping.slot != ModelSlot::Default)
    {
        return Err(
            "model_mapping.no_visible_routes: at least one non-Default route is required"
                .to_owned(),
        );
    }

    Ok(mappings)
}

fn validate_desktop_route_id(route_id: &str, upstream_model: &str) -> Result<(), String> {
    if !route_id.starts_with("claude-") {
        return Err(format!(
            "desktop.raw_model_names_detected: route '{route_id}' must be a claude-* alias"
        ));
    }
    if route_id == upstream_model.trim() {
        return Err(format!(
            "desktop.raw_model_names_detected: route '{route_id}' equals the raw upstream model name"
        ));
    }
    if route_id.to_ascii_lowercase().contains("default") {
        return Err(
            "gateway.unmapped_model_route: Default is not a Desktop-visible route".to_owned(),
        );
    }
    Ok(())
}

fn is_http_base_url(base_url: &str) -> bool {
    base_url.starts_with("http://") || base_url.starts_with("https://")
}

fn validate_marketplace_source(source: &ProviderMarketplaceSource) -> Result<(), ConfigError> {
    if stable_id(&source.source_id).is_empty() {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_missing_source_id".to_owned(),
        ));
    }
    if source.display_name.trim().is_empty() {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_missing_display_name".to_owned(),
        ));
    }
    let url = source.url.trim();
    if !url.starts_with("https://") {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_source_url_not_https".to_owned(),
        ));
    }
    let without_scheme = url.trim_start_matches("https://");
    let host = without_scheme.split('/').next().unwrap_or_default();
    if host.is_empty() || host.contains('@') || url.contains('?') || url.contains('#') {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_source_url_not_plain".to_owned(),
        ));
    }
    Ok(())
}

fn normalize_template_sha256(raw: &str) -> Result<String, ConfigError> {
    let normalized = raw
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or_else(|| raw.trim())
        .to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(ConfigError::InvalidProviderImport(
            "provider.marketplace_template_hash_invalid".to_owned(),
        ));
    }
    Ok(normalized)
}

fn provider_template_package_sha256(
    package: &ProviderTemplatePackage,
) -> Result<String, ConfigError> {
    let bytes = serde_json::to_vec(package)?;
    Ok(sha256_hex(&bytes))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

fn find_forbidden_template_secret_field(value: &Value) -> Option<String> {
    match value {
        Value::Object(object) => object.iter().find_map(|(key, child)| {
            let normalized = normalize_secret_field_name(key);
            if FORBIDDEN_TEMPLATE_SECRET_FIELDS.contains(&normalized.as_str()) {
                Some(key.clone())
            } else {
                find_forbidden_template_secret_field(child)
            }
        }),
        Value::Array(items) => items.iter().find_map(find_forbidden_template_secret_field),
        _ => None,
    }
}

fn normalize_secret_field_name(key: &str) -> String {
    key.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn normalize_imported_providers(providers: &mut [ConfigProvider]) -> Result<(), ConfigError> {
    for (index, provider) in providers.iter_mut().enumerate() {
        provider.provider_id = stable_id(&provider.provider_id);
        if provider.provider_id.is_empty() {
            provider.provider_id = stable_provider_id(Some(&provider.display_name));
        }
        provider.display_name = provider.display_name.trim().to_owned();
        if provider.display_name.is_empty() {
            return Err(ConfigError::InvalidProviderImport(
                "provider.import_missing_display_name".to_owned(),
            ));
        }
        provider.base_url = provider.base_url.trim().trim_end_matches('/').to_owned();
        if provider.base_url.is_empty() {
            return Err(ConfigError::InvalidProviderImport(
                "provider.import_missing_base_url".to_owned(),
            ));
        }
        provider.sort_index = index as u32;
        normalize_imported_model_mappings(provider)?;
    }
    Ok(())
}

fn normalize_imported_model_mappings(provider: &mut ConfigProvider) -> Result<(), ConfigError> {
    let provider_for_routes = provider.as_provider();
    let mut route_ids = HashSet::new();
    for mapping in &mut provider.model_mappings {
        mapping.upstream_model = mapping.upstream_model.trim().to_owned();
        if mapping.upstream_model.is_empty() {
            return Err(ConfigError::InvalidProviderImport(
                "provider.import_missing_upstream_model".to_owned(),
            ));
        }

        if mapping.slot == ModelSlot::Default {
            mapping.route_id = None;
            continue;
        }

        let route_id = mapping
            .route_id
            .as_deref()
            .map(str::trim)
            .filter(|route_id| !route_id.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| route_for_model(&provider_for_routes, &mapping.upstream_model));
        if let Err(error) = validate_desktop_route_id(&route_id, &mapping.upstream_model) {
            return Err(ConfigError::InvalidProviderImport(format!(
                "provider.import_raw_route_id: {error}"
            )));
        }
        if !route_ids.insert(route_id.clone()) {
            return Err(ConfigError::InvalidProviderImport(
                "provider.import_duplicate_route_id".to_owned(),
            ));
        }
        mapping.route_id = Some(route_id);
    }
    Ok(())
}

fn validate_unique_provider_ids(providers: &[ConfigProvider]) -> Result<(), ConfigError> {
    let mut seen = HashSet::new();
    for provider in providers {
        if !seen.insert(provider.provider_id.clone()) {
            return Err(ConfigError::InvalidProviderImport(
                "provider.import_duplicate_provider_id".to_owned(),
            ));
        }
    }
    Ok(())
}

impl Default for ConfigSettings {
    fn default() -> Self {
        Self {
            theme: "default".to_owned(),
            language: "zh".to_owned(),
            proxy_port: 18080,
            update_url:
                "https://github.com/lonr-6/cc-desktop-switch/releases/latest/download/latest.json"
                    .to_owned(),
        }
    }
}

pub fn backup_then_save_config(
    path: &Path,
    config: &AppConfig,
) -> Result<Option<BackupMeta>, ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let backup = if path.exists() {
        Some(create_backup(path)?)
    } else {
        None
    };

    let tmp_path = path.with_extension("json.tmp");
    let body = serde_json::to_string_pretty(config)?;
    fs::write(&tmp_path, body)?;
    fs::rename(tmp_path, path)?;

    Ok(backup)
}

pub fn create_config_backup(path: &Path) -> Result<Option<ConfigBackupSummary>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }

    let backup = create_backup(path)?;
    let metadata = fs::metadata(&backup.path)?;
    let modified_unix_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    Ok(Some(ConfigBackupSummary {
        file_name: backup
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("config-backup.json")
            .to_owned(),
        path: backup.path.display().to_string(),
        size: metadata.len(),
        modified_unix_ms,
    }))
}

pub fn list_config_backups(path: &Path) -> Result<Vec<ConfigBackupSummary>, ConfigError> {
    let backup_dir = backup_dir_for_config(path);
    if !backup_dir.exists() {
        return Ok(Vec::new());
    }

    let mut backups = Vec::new();
    for entry in fs::read_dir(&backup_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let modified_unix_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());
        backups.push(ConfigBackupSummary {
            file_name: file_name.to_owned(),
            path: path.display().to_string(),
            size: metadata.len(),
            modified_unix_ms,
        });
    }

    backups.sort_by(|left, right| {
        right
            .modified_unix_ms
            .cmp(&left.modified_unix_ms)
            .then_with(|| right.file_name.cmp(&left.file_name))
    });
    Ok(backups)
}

pub fn read_config_backup(path: &Path, file_name: &str) -> Result<String, ConfigError> {
    let file_name = file_name.trim();
    let candidate = Path::new(file_name);
    if candidate.components().count() != 1
        || candidate.file_name().and_then(|value| value.to_str()) != Some(file_name)
    {
        return Err(ConfigError::InvalidConfigBackup(
            "config_backup.invalid_file_name".to_owned(),
        ));
    }

    let backup_dir = backup_dir_for_config(path);
    let backup_dir = fs::canonicalize(&backup_dir)?;
    let backup_path = fs::canonicalize(backup_dir.join(file_name))?;
    if !backup_path.starts_with(&backup_dir) {
        return Err(ConfigError::InvalidConfigBackup(
            "config_backup.outside_backup_dir".to_owned(),
        ));
    }

    Ok(fs::read_to_string(backup_path)?)
}

fn create_backup(path: &Path) -> Result<BackupMeta, ConfigError> {
    let backup_dir = backup_dir_for_config(path);
    fs::create_dir_all(&backup_dir)?;

    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    let backup_path = backup_dir.join(format!(
        "{stem}-{}-before-rust-migration.json",
        now_millis()
    ));
    fs::copy(path, &backup_path)?;
    let size = fs::metadata(&backup_path)?.len();

    Ok(BackupMeta {
        path: backup_path,
        size,
    })
}

fn backup_dir_for_config(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

fn migrate_python_config(value: &Value) -> Result<AppConfig, ConfigError> {
    let source = value
        .get("config")
        .filter(|nested| nested.is_object())
        .unwrap_or(value);
    let source_object = source.as_object().ok_or(ConfigError::NotObject)?;

    let providers = source_object
        .get("providers")
        .and_then(Value::as_array)
        .map(|providers| {
            providers
                .iter()
                .enumerate()
                .filter_map(|(index, provider)| migrate_python_provider(provider, index as u32))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let active_provider = active_provider(source_object.get("activeProvider"), &providers);

    Ok(AppConfig {
        schema_version: RUST_SCHEMA_VERSION,
        version: "1.1.0-rc1".to_owned(),
        active_provider,
        gateway_api_key: source_object
            .get("gatewayApiKey")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        providers,
        settings: migrate_settings(source_object.get("settings")),
    })
}

fn migrate_python_provider(value: &Value, sort_index: u32) -> Option<ConfigProvider> {
    let object = value.as_object()?;
    let provider_id = object
        .get("id")
        .and_then(Value::as_str)
        .map(stable_id)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| stable_provider_id(object.get("name").and_then(Value::as_str)));
    let display_name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Unnamed Provider")
        .trim()
        .to_owned();
    let base_url = object
        .get("baseUrl")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .trim_end_matches('/')
        .to_owned();
    let api_format = parse_api_format(object.get("apiFormat").and_then(Value::as_str));
    let auth_scheme = parse_auth_scheme(object.get("authScheme").and_then(Value::as_str));
    let api_key = object
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    let provider = Provider {
        provider_id: provider_id.clone(),
        display_name: display_name.clone(),
        base_url: base_url.clone(),
        auth_scheme: auth_scheme.clone(),
        api_format: api_format.clone(),
        api_key: api_key.clone(),
    };

    Some(ConfigProvider {
        provider_id,
        display_name,
        base_url,
        auth_scheme,
        api_format,
        api_key,
        model_mappings: migrate_model_mappings(
            &provider,
            object.get("models"),
            object.get("modelCapabilities"),
        ),
        sort_index,
    })
}

fn migrate_model_mappings(
    provider: &Provider,
    models: Option<&Value>,
    model_capabilities: Option<&Value>,
) -> Vec<ModelMapping> {
    let Some(models) = models.and_then(Value::as_object) else {
        return Vec::new();
    };

    [
        (ModelSlot::Sonnet, "sonnet"),
        (ModelSlot::Opus, "opus"),
        (ModelSlot::Haiku, "haiku"),
        (ModelSlot::Default, "default"),
    ]
    .into_iter()
    .filter_map(|(slot, key)| {
        let upstream_model = models.get(key).and_then(Value::as_str)?.trim().to_owned();
        if upstream_model.is_empty() {
            return None;
        }

        let capabilities = model_capabilities_for(&upstream_model, model_capabilities);
        let route_id = if slot == ModelSlot::Default {
            None
        } else {
            Some(route_for_model(provider, &upstream_model))
        };

        Some(ModelMapping {
            slot,
            upstream_model,
            route_id,
            capabilities,
        })
    })
    .collect()
}

fn default_model_mappings(provider: &Provider) -> Vec<ModelMapping> {
    let upstream_model = if provider.provider_id.contains("kimi") {
        "kimi-k2.6"
    } else {
        "deepseek-v4-pro"
    };

    vec![ModelMapping {
        slot: ModelSlot::Sonnet,
        upstream_model: upstream_model.to_owned(),
        route_id: Some(route_for_model(provider, upstream_model)),
        capabilities: RouteCapabilities {
            supports_1m: provider.provider_id.contains("deepseek"),
            supports_max: false,
        },
    }]
}

fn model_capabilities_for(
    upstream_model: &str,
    model_capabilities: Option<&Value>,
) -> RouteCapabilities {
    let Some(capability_object) = model_capabilities.and_then(Value::as_object) else {
        return RouteCapabilities::default();
    };
    let Some(model_capability) = capability_object
        .get(upstream_model)
        .and_then(Value::as_object)
    else {
        return RouteCapabilities::default();
    };

    RouteCapabilities {
        supports_1m: model_capability
            .get("supports1m")
            .or_else(|| model_capability.get("supports1M"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        supports_max: model_capability
            .get("supportsMax")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn migrate_settings(settings: Option<&Value>) -> ConfigSettings {
    let settings = settings.and_then(Value::as_object);
    ConfigSettings {
        theme: string_setting(settings, "theme", "default"),
        language: string_setting(settings, "language", "zh"),
        proxy_port: settings
            .and_then(|settings| settings.get("proxyPort"))
            .and_then(Value::as_u64)
            .filter(|port| *port <= u16::MAX as u64)
            .unwrap_or(18080) as u16,
        update_url: string_setting(
            settings,
            "updateUrl",
            "https://github.com/lonr-6/cc-desktop-switch/releases/latest/download/latest.json",
        ),
    }
}

fn string_setting(
    settings: Option<&serde_json::Map<String, Value>>,
    key: &str,
    fallback: &str,
) -> String {
    settings
        .and_then(|settings| settings.get(key))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn active_provider(
    active_provider: Option<&Value>,
    providers: &[ConfigProvider],
) -> Option<String> {
    let active_provider = active_provider.and_then(Value::as_str).map(stable_id);
    if let Some(active_provider) = active_provider {
        if providers
            .iter()
            .any(|provider| provider.provider_id == active_provider)
        {
            return Some(active_provider);
        }
    }

    providers
        .first()
        .map(|provider| provider.provider_id.clone())
}

fn normalize_rust_config(config: &mut AppConfig) {
    config.schema_version = RUST_SCHEMA_VERSION;
    for (index, provider) in config.providers.iter_mut().enumerate() {
        provider.provider_id = stable_id(&provider.provider_id);
        provider.base_url = provider.base_url.trim().trim_end_matches('/').to_owned();
        provider.sort_index = index as u32;
    }
    config.active_provider = active_provider(
        config
            .active_provider
            .as_ref()
            .map(|provider| Value::String(provider.clone()))
            .as_ref(),
        &config.providers,
    );
}

fn normalize_provider_sort(providers: &mut [ConfigProvider]) {
    for (index, provider) in providers.iter_mut().enumerate() {
        provider.sort_index = index as u32;
    }
}

fn parse_api_format(value: Option<&str>) -> ApiFormat {
    match value.unwrap_or("anthropic") {
        "openai" | "openai_chat" | "open_ai_chat" => ApiFormat::OpenAiChat,
        _ => ApiFormat::Anthropic,
    }
}

fn default_api_format() -> ApiFormat {
    ApiFormat::Anthropic
}

fn deserialize_api_format_alias<'de, D>(deserializer: D) -> Result<ApiFormat, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(parse_api_format(value.as_deref()))
}

fn parse_auth_scheme(value: Option<&str>) -> AuthScheme {
    match value.unwrap_or("bearer").to_ascii_lowercase().as_str() {
        "x-api-key" | "x_api_key" => AuthScheme::XApiKey,
        "none" => AuthScheme::None,
        _ => AuthScheme::Bearer,
    }
}

fn stable_provider_id(display_name: Option<&str>) -> String {
    let slug = display_name.map(stable_id).filter(|id| !id.is_empty());
    slug.map(|id| format!("provider-{id}"))
        .unwrap_or_else(|| "provider-custom".to_owned())
}

fn stable_id(value: &str) -> String {
    let mut id = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            id.push(ch);
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    id.trim_matches('-').chars().take(64).collect()
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::model_catalog::ModelCatalog;

    use super::*;

    fn legacy_config() -> String {
        serde_json::json!({
            "version": "1.0.20",
            "activeProvider": "deepseek",
            "gatewayApiKey": "ccds_gateway",
            "settings": {
                "theme": "dark",
                "language": "en",
                "proxyPort": 18088,
                "adminPort": 18081,
                "exposeAllProviderModels": true
            },
            "providers": [
                {
                    "id": "deepseek",
                    "name": "DeepSeek",
                    "baseUrl": "https://api.deepseek.com/anthropic/",
                    "authScheme": "bearer",
                    "apiFormat": "anthropic",
                    "apiKey": "sk-secret",
                    "models": {
                        "sonnet": "deepseek-v4-pro[1m]",
                        "haiku": "deepseek-v4-flash",
                        "opus": "deepseek-reasoner",
                        "default": "deepseek-v4-pro[1m]"
                    },
                    "modelCapabilities": {
                        "deepseek-v4-pro[1m]": {"supports1m": true},
                        "deepseek-reasoner": {"supportsMax": true}
                    }
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn migrates_python_config_without_schema_version() {
        let loaded = AppConfig::load_json(&legacy_config()).unwrap();

        assert_eq!(loaded.report.source_schema, SourceSchema::PythonStable);
        assert_eq!(loaded.config.schema_version, RUST_SCHEMA_VERSION);
        assert_eq!(loaded.config.active_provider.as_deref(), Some("deepseek"));
        assert_eq!(
            loaded.config.gateway_api_key.as_deref(),
            Some("ccds_gateway")
        );
        assert_eq!(loaded.config.settings.proxy_port, 18088);

        let provider = &loaded.config.providers[0];
        assert_eq!(provider.provider_id, "deepseek");
        assert_eq!(provider.display_name, "DeepSeek");
        assert_eq!(provider.base_url, "https://api.deepseek.com/anthropic");
        assert_eq!(provider.api_key, "sk-secret");
    }

    #[test]
    fn migrated_model_mappings_preserve_route_identity_and_default_boundary() {
        let loaded = AppConfig::load_json(&legacy_config()).unwrap();
        let provider = &loaded.config.providers[0];
        let catalog =
            ModelCatalog::from_mappings(&provider.as_provider(), provider.model_mappings.clone())
                .unwrap();

        let models = catalog.desktop_models();
        let model_json = serde_json::to_string(&models).unwrap();
        assert!(model_json.contains("claude-deepseek-v4-pro-1m"));
        assert!(model_json.contains("claude-deepseek-v4-flash"));
        assert!(model_json.contains("claude-deepseek-reasoner"));
        assert!(!model_json.contains("Default"));
        assert!(!model_json.contains("\"id\":\"deepseek-v4-pro[1m]\""));

        let sonnet = catalog.resolve_route("claude-deepseek-v4-pro-1m").unwrap();
        let opus = catalog.resolve_route("claude-deepseek-reasoner").unwrap();
        assert!(sonnet.supports_1m);
        assert!(opus.supports_max);
        assert_eq!(
            catalog.resolve_route("Default").unwrap_err().code,
            "gateway.unmapped_model_route"
        );
    }

    #[test]
    fn rust_config_load_preserves_existing_route_ids_after_provider_rename() {
        let raw = serde_json::json!({
            "schemaVersion": 1,
            "version": "1.1.0-rc1",
            "activeProvider": "deepseek-stable",
            "gatewayApiKey": null,
            "settings": {
                "theme": "default",
                "language": "zh",
                "proxyPort": 18080,
                "updateUrl": "https://example.com/latest.json"
            },
            "providers": [{
                "providerId": "deepseek-stable",
                "displayName": "Renamed DeepSeek",
                "baseUrl": "https://api.deepseek.com/anthropic",
                "authScheme": "bearer",
                "apiFormat": "anthropic",
                "apiKey": "sk-secret",
                "sortIndex": 0,
                "modelMappings": [{
                    "slot": "sonnet",
                    "upstreamModel": "deepseek-v4-pro",
                    "routeId": "claude-deepseek-v4-pro",
                    "capabilities": {"supports1m": false, "supportsMax": false}
                }]
            }]
        })
        .to_string();

        let loaded = AppConfig::load_json(&raw).unwrap();
        let provider = &loaded.config.providers[0];
        assert_eq!(provider.display_name, "Renamed DeepSeek");
        assert_eq!(
            provider.model_mappings[0].route_id.as_deref(),
            Some("claude-deepseek-v4-pro")
        );
    }

    #[test]
    fn upsert_provider_preserves_existing_model_mappings_on_metadata_update() {
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-old".to_owned(),
        };
        let mut config = AppConfig::empty();
        config.upsert_provider(ConfigProvider::from_provider(provider.clone(), 0));
        config.providers[0].model_mappings[0].route_id =
            Some("claude-custom-preserved-route".to_owned());

        let updated_provider = Provider {
            display_name: "DeepSeek Renamed".to_owned(),
            api_key: "sk-new".to_owned(),
            ..provider
        };
        config.upsert_provider(ConfigProvider::from_provider(updated_provider, 99));

        let saved = &config.providers[0];
        assert_eq!(saved.display_name, "DeepSeek Renamed");
        assert_eq!(saved.api_key, "sk-new");
        assert_eq!(saved.sort_index, 0);
        assert_eq!(
            saved.model_mappings[0].route_id.as_deref(),
            Some("claude-custom-preserved-route")
        );
    }

    #[test]
    fn upsert_provider_preserves_existing_api_key_when_update_secret_is_blank() {
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-old".to_owned(),
        };
        let mut config = AppConfig::empty();
        config.upsert_provider(ConfigProvider::from_provider(provider.clone(), 0));

        let updated_provider = Provider {
            display_name: "DeepSeek Renamed".to_owned(),
            api_key: String::new(),
            ..provider
        };
        config.upsert_provider(ConfigProvider::from_provider(updated_provider, 0));

        assert_eq!(config.providers[0].display_name, "DeepSeek Renamed");
        assert_eq!(config.providers[0].api_key, "sk-old");
    }

    #[test]
    fn delete_provider_updates_active_provider_and_sort_order() {
        let mut config = AppConfig::empty();
        for name in ["DeepSeek", "Kimi", "Qwen"] {
            let provider = Provider {
                provider_id: stable_provider_id(Some(name)),
                display_name: name.to_owned(),
                base_url: "https://api.example.test".to_owned(),
                auth_scheme: AuthScheme::Bearer,
                api_format: ApiFormat::Anthropic,
                api_key: "sk-secret".to_owned(),
            };
            config.upsert_provider(ConfigProvider::from_provider(
                provider,
                config.providers.len() as u32,
            ));
        }
        config.active_provider = Some("provider-kimi".to_owned());

        assert!(config.delete_provider("provider-kimi"));

        assert_eq!(config.providers.len(), 2);
        assert_eq!(config.active_provider.as_deref(), Some("provider-deepseek"));
        assert_eq!(config.providers[0].sort_index, 0);
        assert_eq!(config.providers[1].sort_index, 1);
        assert!(!config.delete_provider("provider-missing"));
    }

    #[test]
    fn reorder_providers_requires_exact_set_and_preserves_active_provider() {
        let mut config = AppConfig::empty();
        for name in ["DeepSeek", "Kimi", "Qwen"] {
            let provider = Provider {
                provider_id: stable_provider_id(Some(name)),
                display_name: name.to_owned(),
                base_url: "https://api.example.test".to_owned(),
                auth_scheme: AuthScheme::Bearer,
                api_format: ApiFormat::Anthropic,
                api_key: "sk-secret".to_owned(),
            };
            config.upsert_provider(ConfigProvider::from_provider(
                provider,
                config.providers.len() as u32,
            ));
        }
        config.active_provider = Some("provider-kimi".to_owned());

        assert!(config
            .reorder_providers(&[
                "provider-qwen".to_owned(),
                "provider-deepseek".to_owned(),
                "provider-kimi".to_owned(),
            ])
            .unwrap());

        assert_eq!(config.providers[0].provider_id, "provider-qwen");
        assert_eq!(config.providers[1].provider_id, "provider-deepseek");
        assert_eq!(config.providers[2].provider_id, "provider-kimi");
        assert_eq!(config.providers[0].sort_index, 0);
        assert_eq!(config.providers[1].sort_index, 1);
        assert_eq!(config.providers[2].sort_index, 2);
        assert_eq!(config.active_provider.as_deref(), Some("provider-kimi"));
        assert!(config
            .reorder_providers(&["provider-qwen".to_owned()])
            .unwrap_err()
            .contains("provider.reorder_invalid_set"));
    }

    #[test]
    fn provider_import_export_roundtrips_and_keeps_default_non_runtime() {
        let loaded = AppConfig::load_json(&legacy_config()).unwrap();
        let raw = serde_json::to_string(&loaded.config.export_provider_package()).unwrap();
        let mut imported = AppConfig::empty();

        let preview = imported.preview_provider_import(&raw, false).unwrap();
        assert_eq!(preview.source_schema, ProviderImportSource::ProviderExport);
        assert_eq!(preview.incoming_provider_count, 1);
        assert!(preview.would_write);

        let result = imported.import_providers(&raw, false).unwrap();

        assert!(result.changed);
        assert_eq!(imported.active_provider.as_deref(), Some("deepseek"));
        let provider = &imported.providers[0];
        assert!(provider
            .model_mappings
            .iter()
            .any(|mapping| mapping.slot == ModelSlot::Default && mapping.route_id.is_none()));
        let catalog =
            ModelCatalog::from_mappings(&provider.as_provider(), provider.model_mappings.clone())
                .unwrap();
        assert_eq!(
            catalog.resolve_route("Default").unwrap_err().code,
            "gateway.unmapped_model_route"
        );
    }

    #[test]
    fn provider_export_redacted_package_omits_api_keys_for_ui() {
        let loaded = AppConfig::load_json(&legacy_config()).unwrap();

        let full = loaded.config.export_provider_package();
        let redacted = loaded.config.export_provider_package_redacted();

        assert!(full
            .providers
            .iter()
            .any(|provider| !provider.api_key.is_empty()));
        assert!(redacted
            .providers
            .iter()
            .all(|provider| provider.api_key.is_empty()));
        assert_eq!(redacted.active_provider, full.active_provider);
        assert_eq!(
            redacted.providers[0].provider_id,
            full.providers[0].provider_id
        );
        assert_eq!(
            redacted.providers[0].model_mappings,
            full.providers[0].model_mappings
        );
    }

    #[test]
    fn provider_import_preview_blocks_conflict_until_replace_is_requested() {
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-old".to_owned(),
        };
        let mut current = AppConfig::empty();
        current.upsert_provider(ConfigProvider::from_provider(provider.clone(), 0));

        let incoming_provider = Provider {
            display_name: "DeepSeek Imported".to_owned(),
            api_key: "sk-new".to_owned(),
            ..provider
        };
        let mut incoming = AppConfig::empty();
        incoming.upsert_provider(ConfigProvider::from_provider(incoming_provider, 0));
        let raw = serde_json::to_string(&incoming.export_provider_package()).unwrap();

        let preview = current.preview_provider_import(&raw, false).unwrap();
        assert_eq!(preview.conflict_count, 1);
        assert!(!preview.would_write);
        assert!(preview
            .issue_codes
            .contains(&"provider.import_conflict".to_owned()));

        let blocked = current.import_providers(&raw, false).unwrap();
        assert!(!blocked.changed);
        assert_eq!(current.providers[0].display_name, "DeepSeek");

        let replaced = current.import_providers(&raw, true).unwrap();
        assert!(replaced.changed);
        assert_eq!(current.providers[0].display_name, "DeepSeek Imported");
        assert_eq!(current.providers[0].api_key, "sk-new");
    }

    #[test]
    fn provider_import_skip_existing_imports_new_providers_without_replacing_conflicts() {
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-old".to_owned(),
        };
        let mut current = AppConfig::empty();
        current.upsert_provider(ConfigProvider::from_provider(provider.clone(), 0));

        let incoming_provider = Provider {
            display_name: "DeepSeek Imported".to_owned(),
            api_key: "sk-new".to_owned(),
            ..provider
        };
        let kimi_provider = Provider {
            provider_id: "provider-kimi".to_owned(),
            display_name: "Kimi".to_owned(),
            base_url: "https://api.moonshot.cn/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-kimi".to_owned(),
        };
        let mut incoming = AppConfig::empty();
        incoming.upsert_provider(ConfigProvider::from_provider(incoming_provider, 0));
        incoming.upsert_provider(ConfigProvider::from_provider(kimi_provider, 1));
        let raw = serde_json::to_string(&incoming.export_provider_package()).unwrap();

        let blocked = current
            .preview_provider_import_with_merge(&raw, false, false)
            .unwrap();
        assert_eq!(blocked.conflict_count, 1);
        assert_eq!(blocked.unresolved_conflict_count, 1);
        assert_eq!(blocked.importable_provider_count, 0);
        assert!(!blocked.would_write);

        let preview = current
            .preview_provider_import_with_merge(&raw, false, true)
            .unwrap();
        assert_eq!(preview.conflict_count, 1);
        assert_eq!(preview.skipped_conflict_count, 1);
        assert_eq!(preview.importable_provider_count, 1);
        assert!(preview.would_write);
        assert!(preview
            .issue_codes
            .contains(&"provider.import_skip_existing".to_owned()));

        let result = current
            .import_providers_with_merge(&raw, false, true)
            .unwrap();
        assert!(result.changed);
        assert_eq!(current.providers.len(), 2);
        assert_eq!(current.providers[0].display_name, "DeepSeek");
        assert_eq!(current.providers[0].api_key, "sk-old");
        assert!(current
            .providers
            .iter()
            .any(|provider| provider.provider_id == "provider-kimi"));
    }

    #[test]
    fn provider_import_accepts_cc_switch_legacy_shape() {
        let mut config = AppConfig::empty();

        let preview = config
            .preview_provider_import(&legacy_config(), false)
            .unwrap();
        assert_eq!(preview.source_schema, ProviderImportSource::CcSwitchLegacy);
        assert!(preview.would_write);

        let result = config.import_providers(&legacy_config(), false).unwrap();

        assert!(result.changed);
        assert_eq!(config.active_provider.as_deref(), Some("deepseek"));
        assert_eq!(config.providers[0].display_name, "DeepSeek");
        assert!(config.providers[0]
            .model_mappings
            .iter()
            .any(|mapping| mapping.slot == ModelSlot::Default && mapping.route_id.is_none()));
    }

    #[test]
    fn provider_import_rejects_duplicate_ids_and_raw_route_ids() {
        let loaded = AppConfig::load_json(&legacy_config()).unwrap();
        let mut duplicate_package = loaded.config.export_provider_package();
        duplicate_package
            .providers
            .push(duplicate_package.providers[0].clone());
        let duplicate_raw = serde_json::to_string(&duplicate_package).unwrap();

        let duplicate_error = AppConfig::empty()
            .preview_provider_import(&duplicate_raw, false)
            .unwrap_err();
        assert!(duplicate_error
            .to_string()
            .contains("provider.import_duplicate_provider_id"));

        let mut raw_route_package = loaded.config.export_provider_package();
        raw_route_package.providers[0].model_mappings[0].route_id =
            Some("deepseek-v4-pro".to_owned());
        let raw_route = serde_json::to_string(&raw_route_package).unwrap();

        let raw_route_error = AppConfig::empty()
            .preview_provider_import(&raw_route, false)
            .unwrap_err();
        assert!(raw_route_error
            .to_string()
            .contains("provider.import_raw_route_id"));

        let mut default_route_package = loaded.config.export_provider_package();
        default_route_package.providers[0].model_mappings[0].route_id =
            Some("claude-default".to_owned());
        let default_route = serde_json::to_string(&default_route_package).unwrap();
        let default_route_error = AppConfig::empty()
            .preview_provider_import(&default_route, false)
            .unwrap_err();
        assert!(default_route_error
            .to_string()
            .contains("provider.import_raw_route_id"));

        let mut duplicate_route_package = loaded.config.export_provider_package();
        let duplicate_mapping = duplicate_route_package.providers[0].model_mappings[0].clone();
        duplicate_route_package.providers[0]
            .model_mappings
            .push(duplicate_mapping);
        let duplicate_route = serde_json::to_string(&duplicate_route_package).unwrap();
        let duplicate_route_error = AppConfig::empty()
            .preview_provider_import(&duplicate_route, false)
            .unwrap_err();
        assert!(duplicate_route_error
            .to_string()
            .contains("provider.import_duplicate_route_id"));
    }

    #[test]
    fn provider_import_template_imports_secretless_safe_routes() {
        let raw = serde_json::json!({
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
                        "routeId": "Default",
                        "supports1m": false,
                        "supportsMax": false
                    }
                ]
            }]
        })
        .to_string();
        let mut config = AppConfig::empty();

        let preview = config.preview_provider_import(&raw, false).unwrap();
        assert_eq!(
            preview.source_schema,
            ProviderImportSource::ProviderTemplate
        );
        assert_eq!(preview.incoming_provider_count, 1);
        assert_eq!(preview.providers[0].provider_id, "provider-openrouter");
        assert!(!preview.providers[0].has_api_key);
        assert!(preview.would_write);

        let result = config.import_providers(&raw, false).unwrap();

        assert!(result.changed);
        let provider = &config.providers[0];
        assert_eq!(provider.provider_id, "provider-openrouter");
        assert_eq!(provider.api_key, "");
        assert_eq!(provider.api_format, ApiFormat::OpenAiChat);
        assert!(provider
            .model_mappings
            .iter()
            .any(|mapping| mapping.route_id.as_deref() == Some("claude-openrouter-sonnet-4-5")));
        assert!(provider
            .model_mappings
            .iter()
            .any(|mapping| mapping.slot == ModelSlot::Default && mapping.route_id.is_none()));
    }

    #[test]
    fn provider_import_template_rejects_raw_routes_and_duplicate_ids() {
        let raw_route = serde_json::json!({
            "schemaVersion": 1,
            "kind": "ccds.providerTemplate",
            "templates": [{
                "templateId": "raw",
                "displayName": "Raw Route",
                "baseUrl": "https://relay.example.test/v1",
                "modelMappings": [{
                    "slot": "sonnet",
                    "upstreamModel": "relay-sonnet",
                    "routeId": "relay-sonnet",
                    "supports1m": false,
                    "supportsMax": false
                }]
            }]
        })
        .to_string();
        let raw_error = AppConfig::empty()
            .preview_provider_import(&raw_route, false)
            .unwrap_err();
        assert!(raw_error
            .to_string()
            .contains("desktop.raw_model_names_detected"));

        let duplicate = serde_json::json!({
            "schemaVersion": 1,
            "kind": "ccds.providerTemplate",
            "templates": [
                {
                    "templateId": "duplicate",
                    "displayName": "Duplicate A",
                    "baseUrl": "https://a.example.test",
                    "modelMappings": [{
                        "slot": "sonnet",
                        "upstreamModel": "a-sonnet",
                        "routeId": "claude-a-sonnet",
                        "supports1m": false,
                        "supportsMax": false
                    }]
                },
                {
                    "templateId": "duplicate",
                    "displayName": "Duplicate B",
                    "baseUrl": "https://b.example.test",
                    "modelMappings": [{
                        "slot": "sonnet",
                        "upstreamModel": "b-sonnet",
                        "routeId": "claude-b-sonnet",
                        "supports1m": false,
                        "supportsMax": false
                    }]
                }
            ]
        })
        .to_string();
        let duplicate_error = AppConfig::empty()
            .preview_provider_import(&duplicate, false)
            .unwrap_err();
        assert!(duplicate_error
            .to_string()
            .contains("provider.import_duplicate_provider_id"));
    }

    #[test]
    fn provider_import_template_rejects_secret_fields_and_invalid_base_url() {
        let with_secret = serde_json::json!({
            "schemaVersion": 1,
            "kind": "ccds.providerTemplate",
            "templates": [{
                "templateId": "secret",
                "displayName": "Secret Template",
                "baseUrl": "https://secret.example.test/v1",
                "apiKey": "template-secret",
                "modelMappings": [{
                    "slot": "sonnet",
                    "upstreamModel": "secret-sonnet",
                    "routeId": "claude-secret-sonnet",
                    "supports1m": false,
                    "supportsMax": false
                }]
            }]
        })
        .to_string();
        let secret_error = AppConfig::empty()
            .preview_provider_import(&with_secret, false)
            .unwrap_err();
        assert!(secret_error
            .to_string()
            .contains("provider.template_secret_field_not_allowed"));

        let with_header_secret = serde_json::json!({
            "schemaVersion": 1,
            "kind": "ccds.providerTemplate",
            "templates": [{
                "templateId": "headers",
                "displayName": "Header Template",
                "baseUrl": "https://headers.example.test/v1",
                "headers": { "Authorization": "Bearer secret" },
                "modelMappings": [{
                    "slot": "sonnet",
                    "upstreamModel": "headers-sonnet",
                    "routeId": "claude-headers-sonnet",
                    "supports1m": false,
                    "supportsMax": false
                }]
            }]
        })
        .to_string();
        let header_error = AppConfig::empty()
            .preview_provider_import(&with_header_secret, false)
            .unwrap_err();
        assert!(header_error
            .to_string()
            .contains("provider.template_secret_field_not_allowed"));

        let invalid_base_url = serde_json::json!({
            "schemaVersion": 1,
            "kind": "ccds.providerTemplate",
            "templates": [{
                "templateId": "invalid-url",
                "displayName": "Invalid URL",
                "baseUrl": "file:///tmp/provider",
                "modelMappings": [{
                    "slot": "sonnet",
                    "upstreamModel": "invalid-sonnet",
                    "routeId": "claude-invalid-sonnet",
                    "supports1m": false,
                    "supportsMax": false
                }]
            }]
        })
        .to_string();
        let base_url_error = AppConfig::empty()
            .preview_provider_import(&invalid_base_url, false)
            .unwrap_err();
        assert!(base_url_error
            .to_string()
            .contains("provider.template_invalid_base_url"));
    }

    fn marketplace_template_package() -> ProviderTemplatePackage {
        ProviderTemplatePackage {
            schema_version: RUST_SCHEMA_VERSION,
            kind: PROVIDER_TEMPLATE_KIND.to_owned(),
            templates: vec![ProviderTemplate {
                template_id: "marketplace-openrouter".to_owned(),
                display_name: "Marketplace OpenRouter".to_owned(),
                base_url: "https://openrouter.ai/api/v1".to_owned(),
                api_format: ApiFormat::OpenAiChat,
                model_mappings: vec![
                    ModelMappingDraft {
                        slot: ModelSlot::Sonnet,
                        upstream_model: "anthropic/claude-sonnet-4.5".to_owned(),
                        route_id: Some("claude-openrouter-sonnet-4-5".to_owned()),
                        supports_1m: false,
                        supports_max: false,
                    },
                    ModelMappingDraft {
                        slot: ModelSlot::Default,
                        upstream_model: "anthropic/claude-sonnet-4.5".to_owned(),
                        route_id: None,
                        supports_1m: false,
                        supports_max: false,
                    },
                ],
            }],
        }
    }

    fn marketplace_raw(
        template_package: ProviderTemplatePackage,
        template_sha256: String,
        source_url: &str,
    ) -> String {
        serde_json::to_string(&ProviderMarketplacePackage {
            schema_version: RUST_SCHEMA_VERSION,
            kind: PROVIDER_MARKETPLACE_KIND.to_owned(),
            source: ProviderMarketplaceSource {
                source_id: "official".to_owned(),
                display_name: "Official Templates".to_owned(),
                url: source_url.to_owned(),
            },
            template_sha256,
            template_package,
        })
        .expect("marketplace package should serialize")
    }

    #[test]
    fn provider_import_marketplace_requires_https_source_and_matching_template_hash() {
        let template_package = marketplace_template_package();
        let template_sha256 = provider_template_package_sha256(&template_package).unwrap();
        let raw = marketplace_raw(
            template_package,
            format!("sha256:{template_sha256}"),
            "https://templates.example.test/ccds-marketplace.json",
        );

        let mut config = AppConfig::empty();
        let preview = config.preview_provider_import(&raw, false).unwrap();
        assert_eq!(
            preview.source_schema,
            ProviderImportSource::ProviderMarketplace
        );
        assert_eq!(preview.incoming_provider_count, 1);
        assert!(preview.would_write);

        let result = config.import_providers(&raw, false).unwrap();

        assert!(result.changed);
        assert_eq!(
            config.providers[0].provider_id,
            "provider-marketplace-openrouter"
        );
        assert_eq!(config.providers[0].api_key, "");
        assert!(config.providers[0]
            .model_mappings
            .iter()
            .any(|mapping| mapping.slot == ModelSlot::Default && mapping.route_id.is_none()));
    }

    #[test]
    fn provider_import_marketplace_rejects_untrusted_source_and_hash_mismatch() {
        let template_package = marketplace_template_package();
        let template_sha256 = provider_template_package_sha256(&template_package).unwrap();
        let http_raw = marketplace_raw(
            template_package.clone(),
            template_sha256.clone(),
            "http://templates.example.test/ccds-marketplace.json",
        );
        let http_error = AppConfig::empty()
            .preview_provider_import(&http_raw, false)
            .unwrap_err();
        assert!(http_error
            .to_string()
            .contains("provider.marketplace_source_url_not_https"));

        let query_raw = marketplace_raw(
            template_package.clone(),
            template_sha256.clone(),
            "https://templates.example.test/ccds-marketplace.json?debug=1",
        );
        let query_error = AppConfig::empty()
            .preview_provider_import(&query_raw, false)
            .unwrap_err();
        assert!(query_error
            .to_string()
            .contains("provider.marketplace_source_url_not_plain"));

        let mismatch_raw = marketplace_raw(
            template_package,
            "0000000000000000000000000000000000000000000000000000000000000000".to_owned(),
            "https://templates.example.test/ccds-marketplace.json",
        );
        let mismatch_error = AppConfig::empty()
            .preview_provider_import(&mismatch_raw, false)
            .unwrap_err();
        assert!(mismatch_error
            .to_string()
            .contains("provider.marketplace_template_hash_mismatch"));
    }

    #[test]
    fn provider_import_marketplace_reuses_template_secret_rejection() {
        let package = serde_json::json!({
            "schemaVersion": 1,
            "kind": "ccds.providerMarketplace",
            "source": {
                "sourceId": "official",
                "displayName": "Official Templates",
                "url": "https://templates.example.test/ccds-marketplace.json"
            },
            "templateSha256": "0000000000000000000000000000000000000000000000000000000000000000",
            "templatePackage": {
                "schemaVersion": 1,
                "kind": "ccds.providerTemplate",
                "templates": [{
                    "templateId": "secret",
                    "displayName": "Secret Template",
                    "baseUrl": "https://secret.example.test/v1",
                    "headers": { "Authorization": "Bearer secret" },
                    "modelMappings": [{
                        "slot": "sonnet",
                        "upstreamModel": "secret-sonnet",
                        "routeId": "claude-secret-sonnet",
                        "supports1m": false,
                        "supportsMax": false
                    }]
                }]
            }
        })
        .to_string();

        let error = AppConfig::empty()
            .preview_provider_import(&package, false)
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("provider.template_secret_field_not_allowed"));
    }

    #[test]
    fn provider_preset_import_uses_safe_routes_and_blocks_conflicts_until_replace() {
        let presets = provider_presets();
        assert!(presets.iter().any(|preset| preset.preset_id == "deepseek"));
        assert!(presets
            .iter()
            .flat_map(|preset| preset.model_mappings.iter())
            .filter_map(|mapping| mapping.route_id.as_ref())
            .all(|route_id| route_id.starts_with("claude-")));

        let mut config = AppConfig::empty();
        let first = config
            .import_provider_preset("deepseek", "sk-preset-secret".to_owned(), false)
            .unwrap();
        assert!(first.changed);
        assert_eq!(first.preview.source_schema, ProviderImportSource::Preset);
        assert_eq!(config.providers[0].provider_id, "provider-deepseek");
        assert_eq!(config.providers[0].api_key, "sk-preset-secret");
        assert!(config.providers[0]
            .model_mappings
            .iter()
            .any(|mapping| mapping.slot == ModelSlot::Default && mapping.route_id.is_none()));

        let blocked = config
            .preview_provider_preset_import("deepseek", false)
            .unwrap();
        assert!(!blocked.would_write);
        assert_eq!(blocked.conflict_count, 1);

        let replaced = config
            .import_provider_preset("deepseek", String::new(), true)
            .unwrap();
        assert!(!replaced.changed);
        assert_eq!(config.providers[0].api_key, "sk-preset-secret");
    }

    #[test]
    fn model_mapping_edit_generates_safe_routes_and_keeps_default_non_runtime() {
        let provider = Provider {
            provider_id: "provider-kimi".to_owned(),
            display_name: "Kimi".to_owned(),
            base_url: "https://api.moonshot.cn/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-secret".to_owned(),
        };
        let mut config = AppConfig::empty();
        config.upsert_provider(ConfigProvider::from_provider(provider, 0));

        let changed = config
            .update_provider_model_mappings(
                "provider-kimi",
                vec![
                    ModelMappingDraft {
                        slot: ModelSlot::Sonnet,
                        upstream_model: " kimi-k2.6 ".to_owned(),
                        route_id: None,
                        supports_1m: true,
                        supports_max: false,
                    },
                    ModelMappingDraft {
                        slot: ModelSlot::Default,
                        upstream_model: "kimi-k2.6".to_owned(),
                        route_id: Some("Default".to_owned()),
                        supports_1m: true,
                        supports_max: true,
                    },
                ],
            )
            .unwrap();

        assert!(changed);
        let summaries = config.model_mapping_summaries("provider-kimi").unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].route_id.as_deref(), Some("claude-kimi-k2-6"));
        assert!(summaries[0].desktop_visible);
        assert_eq!(summaries[1].slot, ModelSlot::Default);
        assert!(summaries[1].route_id.is_none());
        assert!(!summaries[1].desktop_visible);

        let saved = &config.providers[0];
        let catalog =
            ModelCatalog::from_mappings(&saved.as_provider(), saved.model_mappings.clone())
                .unwrap();
        let desktop_json = serde_json::to_string(&catalog.desktop_models()).unwrap();
        assert!(desktop_json.contains("claude-kimi-k2-6"));
        assert!(!desktop_json.contains("Default"));
        assert!(!desktop_json.contains("upstreamModel"));
    }

    #[test]
    fn model_mapping_edit_rejects_raw_and_duplicate_routes() {
        let provider = Provider {
            provider_id: "provider-kimi".to_owned(),
            display_name: "Kimi".to_owned(),
            base_url: "https://api.moonshot.cn/anthropic".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_format: ApiFormat::Anthropic,
            api_key: "sk-secret".to_owned(),
        };
        let mut config = AppConfig::empty();
        config.upsert_provider(ConfigProvider::from_provider(provider, 0));

        let raw_error = config
            .update_provider_model_mappings(
                "provider-kimi",
                vec![ModelMappingDraft {
                    slot: ModelSlot::Sonnet,
                    upstream_model: "kimi-k2.6".to_owned(),
                    route_id: Some("kimi-k2.6".to_owned()),
                    supports_1m: false,
                    supports_max: false,
                }],
            )
            .unwrap_err();
        assert!(raw_error.contains("desktop.raw_model_names_detected"));

        let duplicate_error = config
            .update_provider_model_mappings(
                "provider-kimi",
                vec![
                    ModelMappingDraft {
                        slot: ModelSlot::Sonnet,
                        upstream_model: "kimi-k2.6".to_owned(),
                        route_id: Some("claude-kimi-k2-6".to_owned()),
                        supports_1m: false,
                        supports_max: false,
                    },
                    ModelMappingDraft {
                        slot: ModelSlot::Opus,
                        upstream_model: "moonshot-v1".to_owned(),
                        route_id: Some("claude-kimi-k2-6".to_owned()),
                        supports_1m: false,
                        supports_max: true,
                    },
                ],
            )
            .unwrap_err();
        assert!(duplicate_error.contains("model_mapping.duplicate_route_id"));

        let default_alias_error = config
            .update_provider_model_mappings(
                "provider-kimi",
                vec![ModelMappingDraft {
                    slot: ModelSlot::Sonnet,
                    upstream_model: "kimi-k2.6".to_owned(),
                    route_id: Some("claude-default".to_owned()),
                    supports_1m: false,
                    supports_max: false,
                }],
            )
            .unwrap_err();
        assert!(default_alias_error.contains("gateway.unmapped_model_route"));
    }

    #[test]
    fn backup_then_save_copies_existing_file_before_replacing_it() {
        let dir = env::temp_dir().join(format!("ccds-config-test-{}", now_millis()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, legacy_config()).unwrap();

        let loaded = AppConfig::load_json(&legacy_config()).unwrap();
        let backup = backup_then_save_config(&path, &loaded.config)
            .unwrap()
            .expect("existing config should be backed up");

        assert!(backup.path.exists());
        assert!(backup.size > 0);
        let backup_text = fs::read_to_string(&backup.path).unwrap();
        assert!(
            backup_text.contains("\"version\":\"1.0.20\"")
                || backup_text.contains("\"version\": \"1.0.20\"")
        );

        let saved_text = fs::read_to_string(&path).unwrap();
        assert!(saved_text.contains("\"schemaVersion\""));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn config_backups_can_be_listed_and_read_by_file_name_only() {
        let dir = env::temp_dir().join(format!("ccds-config-backups-{}", now_millis()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        fs::write(&path, legacy_config()).unwrap();

        let loaded = AppConfig::load_json(&legacy_config()).unwrap();
        backup_then_save_config(&path, &loaded.config).unwrap();
        let backups = list_config_backups(&path).unwrap();

        assert_eq!(backups.len(), 1);
        assert!(backups[0]
            .file_name
            .ends_with("-before-rust-migration.json"));
        let backup_text = read_config_backup(&path, &backups[0].file_name).unwrap();
        assert!(backup_text.contains("\"version\":\"1.0.20\""));
        let invalid = read_config_backup(&path, "../config.json").unwrap_err();
        assert!(invalid
            .to_string()
            .contains("config_backup.invalid_file_name"));

        fs::remove_dir_all(dir).unwrap();
    }
}
