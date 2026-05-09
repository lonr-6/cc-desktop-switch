use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::apply_flow::{DesktopApplyResult, DesktopApplyStepStatus};
use crate::config::{
    backup_then_save_config, list_config_backups, provider_presets, read_config_backup, AppConfig,
    BackupMeta, ConfigBackupSummary, ConfigError, ConfigProvider, ModelMappingDraft,
    ModelMappingSummary, ProviderExportPackage, ProviderImportApplyResult, ProviderImportPreview,
    ProviderPreset,
};
use crate::desktop::build_desktop_plan;
use crate::desktop_writer::{write_local_config_library, DesktopConfigProbe};
use crate::diagnostics::{
    build_diagnostics_package, build_github_issue_draft, format_diagnostics_summary,
    provider_static_smoke, redact_diagnostics_text, smoke_fail, smoke_pass, DiagnosticsIssueDraft,
    DiagnosticsLogEntry, DiagnosticsPackage, SmokeCheckResult,
};
use crate::gateway::{
    gateway_base_url, gateway_router_with_provider, planned_gateway_health_for_port,
    serve_gateway_router, GatewayHealth, GatewayMode,
};
use crate::gateway_adapter::{build_messages_upstream_request, forward_upstream_request};
use crate::model_catalog::ModelCatalog;
use crate::model_catalog::ModelMapping;
use crate::provider::{Provider, ProviderDraft, ProviderSummary};

pub struct AppState {
    config_path: PathBuf,
    lock: Mutex<()>,
    gateway: Mutex<GatewayRuntime>,
    logs: Mutex<Vec<DiagnosticsLogEntry>>,
}

#[derive(Default)]
struct GatewayRuntime {
    port: Option<u16>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
    last_error_code: Option<String>,
    config_fingerprint: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct AppSnapshot {
    pub active_provider: Option<Provider>,
    pub active_model_mappings: Vec<ModelMapping>,
    pub gateway_api_key: Option<String>,
    pub proxy_port: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("state lock poisoned")]
    StateLock,
    #[error("{0}")]
    InvalidProvider(String),
    #[error("{0}")]
    Gateway(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

impl AppState {
    pub fn with_config_path(path: impl Into<PathBuf>) -> Self {
        Self {
            config_path: path.into(),
            lock: Mutex::new(()),
            gateway: Mutex::new(GatewayRuntime::default()),
            logs: Mutex::new(Vec::new()),
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderSummary>, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let config = self.load_config_unlocked()?;
        Ok(config
            .providers
            .iter()
            .map(ConfigProvider::summary)
            .collect())
    }

    pub fn save_provider(&self, request: ProviderDraft) -> Result<ProviderSummary, StateError> {
        let provider = request
            .into_provider()
            .map_err(StateError::InvalidProvider)?;
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let sort_index = config.providers.len() as u32;
        let provider = ConfigProvider::from_provider(provider, sort_index);
        let summary = provider.summary();
        config.upsert_provider(provider);
        self.save_config_unlocked(&config)?;
        Ok(summary)
    }

    pub fn set_active_provider(&self, provider_id: &str) -> Result<bool, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let changed = config.set_active_provider(provider_id);
        if changed {
            self.save_config_unlocked(&config)?;
        }
        Ok(changed)
    }

    pub fn delete_provider(&self, provider_id: &str) -> Result<bool, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let changed = config.delete_provider(provider_id);
        if changed {
            self.save_config_unlocked(&config)?;
        }
        Ok(changed)
    }

    pub fn reorder_providers(&self, provider_ids: Vec<String>) -> Result<bool, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let changed = config
            .reorder_providers(&provider_ids)
            .map_err(StateError::InvalidProvider)?;
        if changed {
            self.save_config_unlocked(&config)?;
        }
        Ok(changed)
    }

    pub fn export_provider_package(&self) -> Result<ProviderExportPackage, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let config = self.load_config_unlocked()?;
        Ok(config.export_provider_package())
    }

    pub fn preview_provider_import(
        &self,
        raw_json: &str,
        replace_existing: bool,
    ) -> Result<ProviderImportPreview, StateError> {
        self.preview_provider_import_with_merge(raw_json, replace_existing, false)
    }

    pub fn preview_provider_import_with_merge(
        &self,
        raw_json: &str,
        replace_existing: bool,
        skip_existing: bool,
    ) -> Result<ProviderImportPreview, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let config = self.load_config_unlocked()?;
        Ok(config.preview_provider_import_with_merge(raw_json, replace_existing, skip_existing)?)
    }

    pub fn import_providers(
        &self,
        raw_json: &str,
        replace_existing: bool,
    ) -> Result<ProviderImportApplyResult, StateError> {
        self.import_providers_with_merge(raw_json, replace_existing, false)
    }

    pub fn import_providers_with_merge(
        &self,
        raw_json: &str,
        replace_existing: bool,
        skip_existing: bool,
    ) -> Result<ProviderImportApplyResult, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let result =
            config.import_providers_with_merge(raw_json, replace_existing, skip_existing)?;
        if result.changed {
            self.save_config_unlocked(&config)?;
        }
        Ok(result)
    }

    pub fn list_provider_presets(&self) -> Vec<ProviderPreset> {
        provider_presets()
    }

    pub fn preview_provider_preset_import(
        &self,
        preset_id: &str,
        replace_existing: bool,
    ) -> Result<ProviderImportPreview, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let config = self.load_config_unlocked()?;
        Ok(config.preview_provider_preset_import(preset_id, replace_existing)?)
    }

    pub fn import_provider_preset(
        &self,
        preset_id: &str,
        api_key: String,
        replace_existing: bool,
    ) -> Result<ProviderImportApplyResult, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let result = config.import_provider_preset(preset_id, api_key, replace_existing)?;
        if result.changed {
            self.save_config_unlocked(&config)?;
        }
        Ok(result)
    }

    pub fn list_model_mappings(
        &self,
        provider_id: &str,
    ) -> Result<Vec<ModelMappingSummary>, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let config = self.load_config_unlocked()?;
        config
            .model_mapping_summaries(provider_id)
            .map_err(StateError::InvalidProvider)
    }

    pub fn update_model_mappings(
        &self,
        provider_id: &str,
        mappings: Vec<ModelMappingDraft>,
    ) -> Result<Vec<ModelMappingSummary>, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let mut config = self.load_config_unlocked()?;
        let changed = config
            .update_provider_model_mappings(provider_id, mappings)
            .map_err(StateError::InvalidProvider)?;
        if changed {
            self.save_config_unlocked(&config)?;
        }
        config
            .model_mapping_summaries(provider_id)
            .map_err(StateError::InvalidProvider)
    }

    pub fn list_config_backups(&self) -> Result<Vec<ConfigBackupSummary>, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        Ok(list_config_backups(&self.config_path)?)
    }

    pub fn read_config_backup(&self, file_name: &str) -> Result<String, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let raw = read_config_backup(&self.config_path, file_name)?;
        Ok(redact_diagnostics_text(&raw))
    }

    pub fn snapshot(&self) -> Result<AppSnapshot, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        let config = self.load_config_unlocked()?;
        let active_provider = config.active_provider();
        Ok(AppSnapshot {
            active_provider: active_provider.map(ConfigProvider::as_provider),
            active_model_mappings: active_provider
                .map(|provider| provider.model_mappings.clone())
                .unwrap_or_default(),
            gateway_api_key: config.gateway_api_key.clone(),
            proxy_port: config.settings.proxy_port,
        })
    }

    pub fn gateway_status(&self) -> Result<GatewayHealth, StateError> {
        let snapshot = self.snapshot()?;
        self.gateway_health(snapshot.proxy_port)
    }

    pub fn gateway_issue_code(&self) -> Result<Option<String>, StateError> {
        let runtime = self.gateway.lock().map_err(map_lock)?;
        Ok(runtime.last_error_code.clone())
    }

    pub fn runtime_logs(&self) -> Result<Vec<DiagnosticsLogEntry>, StateError> {
        let logs = self.logs.lock().map_err(map_lock)?;
        Ok(logs.clone())
    }

    pub fn diagnostics_package(
        &self,
        desktop_probe: Option<DesktopConfigProbe>,
        desktop_error: Option<String>,
    ) -> Result<DiagnosticsPackage, StateError> {
        let config = self.load_config()?;
        let gateway = self.gateway_status()?;
        let gateway_issue_code = self.gateway_issue_code()?;
        let runtime_logs = self.runtime_logs()?;
        Ok(build_diagnostics_package(
            &self.config_path,
            &config,
            gateway,
            gateway_issue_code.as_deref(),
            desktop_probe,
            desktop_error,
            runtime_logs,
        ))
    }

    pub fn diagnostics_summary(
        &self,
        desktop_probe: Option<DesktopConfigProbe>,
        desktop_error: Option<String>,
    ) -> Result<String, StateError> {
        let package = self.diagnostics_package(desktop_probe, desktop_error)?;
        Ok(format_diagnostics_summary(&package))
    }

    pub fn diagnostics_issue_draft(
        &self,
        desktop_probe: Option<DesktopConfigProbe>,
        desktop_error: Option<String>,
    ) -> Result<DiagnosticsIssueDraft, StateError> {
        let package = self.diagnostics_package(desktop_probe, desktop_error)?;
        Ok(build_github_issue_draft(&package))
    }

    pub fn save_diagnostics_package(
        &self,
        desktop_probe: Option<DesktopConfigProbe>,
        desktop_error: Option<String>,
    ) -> Result<PathBuf, StateError> {
        let package = self.diagnostics_package(desktop_probe, desktop_error)?;
        let root = self
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("diagnostics");
        fs::create_dir_all(&root).map_err(ConfigError::from)?;
        let path = root.join(format!("diagnostics-{}.json", now_millis()));
        let body = serde_json::to_string_pretty(&package).map_err(ConfigError::from)?;
        fs::write(&path, body).map_err(ConfigError::from)?;
        Ok(path)
    }

    pub fn provider_static_smoke(&self) -> Result<SmokeCheckResult, StateError> {
        let snapshot = self.snapshot()?;
        Ok(provider_static_smoke(snapshot.active_provider.as_ref()))
    }

    pub fn gateway_smoke(&self) -> Result<SmokeCheckResult, StateError> {
        let health = match self.start_gateway() {
            Ok(health) => health,
            Err(error) => {
                return Ok(smoke_fail(
                    "gateway.smoke",
                    "gateway.start_failed",
                    &error.to_string(),
                ));
            }
        };
        let url = format!("{}/v1/models", health.base_url);
        let client = reqwest::Client::new();
        let result = block_on_async(async {
            let response = client.get(&url).send().await.map_err(|error| {
                smoke_fail("gateway.smoke", "gateway.smoke_failed", &error.to_string())
            })?;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(smoke_fail(
                    "gateway.smoke",
                    "gateway.smoke_failed",
                    &format!("gateway /v1/models returned {status}: {body}"),
                ));
            }
            let value = serde_json::from_str::<serde_json::Value>(&body).map_err(|error| {
                smoke_fail(
                    "gateway.smoke",
                    "gateway.smoke_failed",
                    &format!("gateway /v1/models returned invalid JSON: {error}"),
                )
            })?;
            let count = value
                .get("data")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len)
                .unwrap_or_default();
            if count == 0 {
                return Err(smoke_fail(
                    "gateway.smoke",
                    "model_catalog.no_visible_routes",
                    "gateway /v1/models returned no visible routes",
                ));
            }
            Ok(smoke_pass(
                "gateway.smoke",
                &format!("gateway /v1/models returned {count} route(s)"),
            ))
        });
        Ok(result.unwrap_or_else(|result| result))
    }

    pub fn provider_real_smoke(&self) -> Result<SmokeCheckResult, StateError> {
        let snapshot = self.snapshot()?;
        let Some(provider) = snapshot.active_provider.as_ref() else {
            return Ok(smoke_fail(
                "provider.real",
                "provider.not_configured",
                "active provider is not configured",
            ));
        };
        let static_result = provider_static_smoke(Some(provider));
        if !static_result.passed {
            return Ok(SmokeCheckResult {
                layer: "provider.real".to_owned(),
                ..static_result
            });
        }
        let catalog = match ModelCatalog::from_mappings(provider, snapshot.active_model_mappings) {
            Ok(catalog) => catalog,
            Err(error) => {
                return Ok(smoke_fail("provider.real", &error.code, &error.message));
            }
        };
        let Some(route) = catalog.desktop_models().first().cloned() else {
            return Ok(smoke_fail(
                "provider.real",
                "model_catalog.no_visible_routes",
                "active provider has no visible routes",
            ));
        };
        let resolution = match catalog.resolve_route(&route.id) {
            Ok(resolution) => resolution,
            Err(error) => {
                return Ok(smoke_fail("provider.real", &error.code, &error.message));
            }
        };
        let body = serde_json::json!({
            "model": route.id,
            "messages": [{"role": "user", "content": "ping"}],
            "max_tokens": 1
        });
        let request = match build_messages_upstream_request(provider, &resolution, body) {
            Ok(request) => request,
            Err(error) => {
                return Ok(smoke_fail("provider.real", &error.code, &error.message));
            }
        };
        let client = reqwest::Client::new();
        let result = block_on_async(async {
            forward_upstream_request(&client, request, &resolution)
                .await
                .map(|response| {
                    smoke_pass(
                        "provider.real",
                        &format!("upstream provider returned status {}", response.status),
                    )
                })
                .map_err(|error| {
                    smoke_fail(
                        "provider.real",
                        &error.code,
                        &format!("{} {}", error.message, error.redacted_preview),
                    )
                })
        });
        Ok(result.unwrap_or_else(|result| result))
    }

    pub fn start_gateway(&self) -> Result<GatewayHealth, StateError> {
        let snapshot = self.snapshot()?;
        let provider = match snapshot.active_provider {
            Some(provider) => provider,
            None => {
                self.remember_gateway_error("gateway.no_active_provider");
                self.record_runtime_log(
                    "error",
                    "gateway.no_active_provider",
                    "active provider is required before starting local gateway",
                );
                return Err(StateError::Gateway(
                    "gateway.no_active_provider: active provider is required".to_owned(),
                ));
            }
        };
        let config_fingerprint =
            gateway_config_fingerprint(&provider, &snapshot.active_model_mappings);

        {
            let mut runtime = self.gateway.lock().map_err(map_lock)?;
            cleanup_gateway_unlocked(&mut runtime);
            if let Some(port) = runtime.port {
                if runtime.config_fingerprint.as_deref() == Some(config_fingerprint.as_str()) {
                    return Ok(GatewayHealth {
                        mode: GatewayMode::LocalGateway,
                        running: true,
                        base_url: gateway_base_url(port),
                    });
                }
                stop_gateway_unlocked(&mut runtime);
            }
        }

        let catalog = match ModelCatalog::from_mappings(&provider, snapshot.active_model_mappings) {
            Ok(catalog) => catalog,
            Err(error) => {
                self.remember_gateway_error(&error.code);
                self.record_runtime_log("error", &error.code, &error.message);
                return Err(StateError::Gateway(format!(
                    "{}: {}",
                    error.code, error.message
                )));
            }
        };
        if catalog.desktop_models().is_empty() {
            self.remember_gateway_error("model_catalog.no_visible_routes");
            self.record_runtime_log(
                "error",
                "model_catalog.no_visible_routes",
                "active provider has no mapped routes",
            );
            return Err(StateError::Gateway(
                "model_catalog.no_visible_routes: active provider has no mapped routes".to_owned(),
            ));
        }

        let addr = SocketAddr::from(([127, 0, 0, 1], snapshot.proxy_port));
        let listener = match TcpListener::bind(addr) {
            Ok(listener) => listener,
            Err(error) => {
                self.remember_gateway_error("gateway.port_in_use");
                self.record_runtime_log(
                    "error",
                    "gateway.port_in_use",
                    &format!("failed to bind local gateway on {addr}: {error}"),
                );
                return Err(StateError::Gateway(format!(
                    "gateway.port_in_use: failed to bind local gateway on {addr}: {error}"
                )));
            }
        };
        let port = match listener.local_addr() {
            Ok(addr) => addr.port(),
            Err(error) => {
                self.remember_gateway_error("gateway.bind_failed");
                self.record_runtime_log("error", "gateway.bind_failed", &error.to_string());
                return Err(StateError::Gateway(format!("gateway.bind_failed: {error}")));
            }
        };
        let router = gateway_router_with_provider(catalog, provider);
        let (shutdown, shutdown_rx) = tokio::sync::oneshot::channel();
        let thread = match std::thread::Builder::new()
            .name("ccds-local-gateway".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("gateway runtime should initialize");
                let _ = runtime.block_on(serve_gateway_router(listener, router, async move {
                    let _ = shutdown_rx.await;
                }));
            }) {
            Ok(thread) => thread,
            Err(error) => {
                self.remember_gateway_error("gateway.start_failed");
                self.record_runtime_log("error", "gateway.start_failed", &error.to_string());
                return Err(StateError::Gateway(format!(
                    "gateway.start_failed: {error}"
                )));
            }
        };

        let mut runtime = self.gateway.lock().map_err(map_lock)?;
        cleanup_gateway_unlocked(&mut runtime);
        runtime.port = Some(port);
        runtime.shutdown = Some(shutdown);
        runtime.thread = Some(thread);
        runtime.last_error_code = None;
        runtime.config_fingerprint = Some(config_fingerprint);
        self.record_runtime_log(
            "info",
            "gateway.started",
            &format!("local gateway started on {}", gateway_base_url(port)),
        );

        Ok(GatewayHealth {
            mode: GatewayMode::LocalGateway,
            running: true,
            base_url: gateway_base_url(port),
        })
    }

    pub fn stop_gateway(&self) -> Result<GatewayHealth, StateError> {
        let mut runtime = self.gateway.lock().map_err(map_lock)?;
        let port = runtime.port;
        stop_gateway_unlocked(&mut runtime);
        runtime.last_error_code = None;
        let fallback_port = self
            .snapshot()
            .map(|snapshot| snapshot.proxy_port)
            .unwrap_or(18080);
        self.record_runtime_log(
            "info",
            "gateway.stopped",
            &format!(
                "local gateway stopped on {}",
                gateway_base_url(port.unwrap_or(fallback_port))
            ),
        );

        Ok(GatewayHealth {
            mode: GatewayMode::LocalGateway,
            running: false,
            base_url: gateway_base_url(port.unwrap_or(fallback_port)),
        })
    }

    pub fn apply_to_local_config_library(&self, root: &Path) -> DesktopApplyResult {
        let mut result = DesktopApplyResult::new();
        self.apply_to_local_config_library_with_result(root, &mut result);
        result
    }

    pub fn apply_to_desktop_config_probe(&self, probe: &DesktopConfigProbe) -> DesktopApplyResult {
        let mut result = DesktopApplyResult::new();
        result.desktop_config = Some(probe.clone());
        if probe.managed_detected {
            result.fail_step(
                "desktop.managed_config",
                "Check managed Claude Desktop config",
                "desktop.managed_config_detected".to_owned(),
            );
            result.push_step(
                "gateway.ensure_running",
                "Ensure local gateway",
                DesktopApplyStepStatus::Skipped,
                None,
            );
            result.push_step(
                "desktop.write",
                "Write Claude Desktop local config",
                DesktopApplyStepStatus::Skipped,
                None,
            );
            return result;
        }

        result.push_step(
            "desktop.config_probe",
            "Resolve Claude Desktop local config path",
            DesktopApplyStepStatus::Passed,
            None,
        );
        self.apply_to_local_config_library_with_result(&probe.local_config_library, &mut result);
        result
    }

    fn apply_to_local_config_library_with_result(
        &self,
        root: &Path,
        result: &mut DesktopApplyResult,
    ) {
        let snapshot = match self.snapshot() {
            Ok(snapshot) => {
                result.push_step(
                    "provider.snapshot",
                    "Read active provider snapshot",
                    DesktopApplyStepStatus::Passed,
                    None,
                );
                snapshot
            }
            Err(error) => {
                result.fail_step(
                    "provider.snapshot",
                    "Read active provider snapshot",
                    error.to_string(),
                );
                return;
            }
        };

        let Some(provider) = snapshot.active_provider.as_ref() else {
            result.fail_step(
                "provider.active",
                "Require active provider",
                "provider.not_configured".to_owned(),
            );
            return;
        };

        let gateway = match self.start_gateway() {
            Ok(gateway) => {
                result.gateway = Some(gateway.clone());
                result.push_step(
                    "gateway.ensure_running",
                    "Ensure local gateway",
                    DesktopApplyStepStatus::Passed,
                    None,
                );
                gateway
            }
            Err(error) => {
                result.fail_step(
                    "gateway.ensure_running",
                    "Ensure local gateway",
                    error.to_string(),
                );
                result.push_step(
                    "desktop.write",
                    "Write Claude Desktop local config",
                    DesktopApplyStepStatus::Skipped,
                    None,
                );
                return;
            }
        };

        let gateway_port =
            port_from_gateway_base_url(&gateway.base_url).unwrap_or(snapshot.proxy_port);
        let gateway_api_key = snapshot
            .gateway_api_key
            .as_deref()
            .unwrap_or("ccds_local_gateway_key");
        let plan = match build_desktop_plan(
            provider,
            &snapshot.active_model_mappings,
            gateway_api_key,
            gateway_port,
        ) {
            Ok(plan) => {
                result.push_step(
                    "model_catalog.build",
                    "Build Claude-safe Desktop plan",
                    DesktopApplyStepStatus::Passed,
                    None,
                );
                plan
            }
            Err(error) => {
                result.fail_step(
                    "model_catalog.build",
                    "Build Claude-safe Desktop plan",
                    format!("{}: {}", error.code, error.message),
                );
                return;
            }
        };

        let write = match write_local_config_library(root, &plan) {
            Ok(write) => {
                result.push_step(
                    "desktop.write",
                    "Write Claude Desktop local config",
                    DesktopApplyStepStatus::Passed,
                    None,
                );
                write
            }
            Err(error) => {
                result.fail_step(
                    "desktop.write",
                    "Write Claude Desktop local config",
                    error.to_string(),
                );
                return;
            }
        };

        if write.health.passed {
            result.push_step(
                "desktop.readback",
                "Read back and compare",
                DesktopApplyStepStatus::Passed,
                None,
            );
            result.success = true;
        } else {
            result.fail_step(
                "desktop.readback",
                "Read back and compare",
                "desktop.readback_failed".to_owned(),
            );
        }
        result.write = Some(write);
    }

    pub fn load_config(&self) -> Result<AppConfig, StateError> {
        let _lock = self.lock.lock().map_err(map_lock)?;
        self.load_config_unlocked()
    }

    fn load_config_unlocked(&self) -> Result<AppConfig, StateError> {
        if !self.config_path.exists() {
            return Ok(AppConfig::empty());
        }

        let body = fs::read_to_string(&self.config_path).map_err(ConfigError::from)?;
        Ok(AppConfig::load_json(&body)?.config)
    }

    fn save_config_unlocked(&self, config: &AppConfig) -> Result<Option<BackupMeta>, StateError> {
        Ok(backup_then_save_config(&self.config_path, config)?)
    }

    fn gateway_health(&self, fallback_port: u16) -> Result<GatewayHealth, StateError> {
        let mut runtime = self.gateway.lock().map_err(map_lock)?;
        cleanup_gateway_unlocked(&mut runtime);
        if let Some(port) = runtime.port {
            return Ok(GatewayHealth {
                mode: GatewayMode::LocalGateway,
                running: true,
                base_url: gateway_base_url(port),
            });
        }

        Ok(planned_gateway_health_for_port(fallback_port))
    }

    fn remember_gateway_error(&self, code: &str) {
        if let Ok(mut runtime) = self.gateway.lock() {
            runtime.last_error_code = Some(code.to_owned());
        }
    }

    fn record_runtime_log(&self, level: &str, code: &str, message: &str) {
        if let Ok(mut logs) = self.logs.lock() {
            logs.push(DiagnosticsLogEntry {
                timestamp_unix_ms: now_millis(),
                level: level.to_owned(),
                code: code.to_owned(),
                message: redact_diagnostics_text(message),
            });
            let overflow = logs.len().saturating_sub(200);
            if overflow > 0 {
                logs.drain(0..overflow);
            }
        }
    }
}

fn cleanup_gateway_unlocked(runtime: &mut GatewayRuntime) {
    let finished = runtime
        .thread
        .as_ref()
        .map(JoinHandle::is_finished)
        .unwrap_or(false);
    if finished {
        if let Some(thread) = runtime.thread.take() {
            let _ = thread.join();
        }
        runtime.shutdown = None;
        runtime.port = None;
        runtime.config_fingerprint = None;
    }
}

fn stop_gateway_unlocked(runtime: &mut GatewayRuntime) {
    if let Some(shutdown) = runtime.shutdown.take() {
        let _ = shutdown.send(());
    }
    if let Some(thread) = runtime.thread.take() {
        let _ = thread.join();
    }
    runtime.port = None;
    runtime.config_fingerprint = None;
}

fn default_config_path() -> PathBuf {
    if let Ok(path) = std::env::var("CCDS_CONFIG_FILE") {
        return PathBuf::from(path);
    }

    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".cc-desktop-switch").join("config.json")
}

fn map_lock<T>(_error: PoisonError<T>) -> StateError {
    StateError::StateLock
}

fn port_from_gateway_base_url(base_url: &str) -> Option<u16> {
    base_url.rsplit(':').next()?.parse().ok()
}

fn gateway_config_fingerprint(provider: &Provider, mappings: &[ModelMapping]) -> String {
    let raw = serde_json::to_string(&(provider, mappings))
        .unwrap_or_else(|_| format!("{}:{}", provider.provider_id, provider.base_url));
    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn block_on_async<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("smoke runtime should initialize")
        .block_on(future)
}

impl Default for AppState {
    fn default() -> Self {
        Self::with_config_path(default_config_path())
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpStream;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::desktop_writer::{
        probe_current_desktop_config, DesktopPlatform, ManagedConfigEvidence,
    };
    use crate::model_catalog::ModelSlot;
    use crate::provider::ApiFormat;

    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir()
            .join(format!("ccds-state-{name}-{millis}"))
            .join("config.json")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("ccds-state-{name}-{millis}"))
    }

    fn provider(display_name: &str) -> ProviderDraft {
        ProviderDraft {
            provider_id: None,
            display_name: display_name.to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            api_key: "sk-secret".to_owned(),
            api_format: ApiFormat::Anthropic,
        }
    }

    fn state_with_provider(name: &str, proxy_port: u16) -> (PathBuf, AppState) {
        let path = temp_config_path(name);
        let state = AppState::with_config_path(&path);
        state.save_provider(provider("DeepSeek")).unwrap();
        let mut config = state.load_config().unwrap();
        config.settings.proxy_port = proxy_port;
        backup_then_save_config(&path, &config).unwrap();
        (path, state)
    }

    fn port_from_base_url(base_url: &str) -> u16 {
        base_url.rsplit(':').next().unwrap().parse().unwrap()
    }

    fn wait_for_tcp(port: u16) {
        for _ in 0..20 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("gateway did not accept TCP connections on port {port}");
    }

    const REAL_DESKTOP_SMOKE_FILES: [&str; 2] =
        ["_meta.json", "cc-desktop-switch-local-gateway.json"];

    fn smoke_check(condition: bool, message: impl Into<String>) -> Result<(), String> {
        if condition {
            Ok(())
        } else {
            Err(message.into())
        }
    }

    fn backup_real_desktop_smoke_files(
        root: &Path,
        backup_root: &Path,
        file_names: &[&str],
    ) -> Result<(), String> {
        fs::create_dir_all(backup_root)
            .map_err(|error| format!("failed to create backup dir: {error}"))?;
        for file_name in file_names {
            let source = root.join(file_name);
            if source.exists() {
                fs::copy(&source, backup_root.join(file_name))
                    .map_err(|error| format!("failed to backup {}: {error}", source.display()))?;
            }
        }
        Ok(())
    }

    fn restore_real_desktop_smoke_files(
        root: &Path,
        backup_root: &Path,
        file_names: &[&str],
        root_existed: bool,
    ) -> Result<(), String> {
        for file_name in file_names {
            let target = root.join(file_name);
            let backup = backup_root.join(file_name);
            if backup.exists() {
                fs::create_dir_all(root)
                    .map_err(|error| format!("failed to recreate desktop root: {error}"))?;
                fs::copy(&backup, &target)
                    .map_err(|error| format!("failed to restore {}: {error}", target.display()))?;
            } else if target.exists() {
                fs::remove_file(&target)
                    .map_err(|error| format!("failed to remove {}: {error}", target.display()))?;
            }
        }

        if !root_existed && root.exists() {
            fs::remove_dir(root).map_err(|error| {
                format!(
                    "created desktop root is not empty after smoke restore: {} ({error})",
                    root.display()
                )
            })?;
        }

        Ok(())
    }

    fn verify_real_desktop_smoke_restore(
        root: &Path,
        backup_root: &Path,
        file_names: &[&str],
        root_existed: bool,
    ) -> Result<(), String> {
        for file_name in file_names {
            let target = root.join(file_name);
            let backup = backup_root.join(file_name);
            if backup.exists() {
                let target_body = fs::read(&target)
                    .map_err(|error| format!("failed to read restored {}: {error}", file_name))?;
                let backup_body = fs::read(&backup)
                    .map_err(|error| format!("failed to read backup {}: {error}", file_name))?;
                smoke_check(
                    target_body == backup_body,
                    format!("restored {file_name} differs from backup"),
                )?;
            } else {
                smoke_check(
                    !target.exists(),
                    format!("{file_name} still exists after restore"),
                )?;
            }
        }

        if !root_existed {
            smoke_check(
                !root.exists(),
                format!(
                    "desktop root still exists after restore: {}",
                    root.display()
                ),
            )?;
        }

        Ok(())
    }

    fn run_real_desktop_local_config_smoke(
        expected_platform: DesktopPlatform,
        state_name: &str,
        backup_name: &str,
    ) {
        if std::env::var("CCDS_ALLOW_REAL_DESKTOP_WRITE")
            .ok()
            .as_deref()
            != Some("1")
        {
            eprintln!("skipped: set CCDS_ALLOW_REAL_DESKTOP_WRITE=1 to write real Desktop config");
            return;
        }

        let (path, state) = state_with_provider(state_name, 0);
        let probe = probe_current_desktop_config().unwrap();
        let desktop_root = probe.local_config_library.clone();
        let desktop_root_existed = desktop_root.exists();
        let backup_root = temp_dir(backup_name);

        backup_real_desktop_smoke_files(&desktop_root, &backup_root, &REAL_DESKTOP_SMOKE_FILES)
            .unwrap();

        let smoke_result = (|| -> Result<(), String> {
            smoke_check(
                probe.platform == expected_platform,
                format!(
                    "expected {:?} probe, got {:?}",
                    expected_platform, probe.platform
                ),
            )?;
            smoke_check(
                !probe.managed_detected,
                format!(
                    "managed Desktop config detected; evidence: {:?}",
                    probe.managed_evidence
                ),
            )?;

            let apply = state.apply_to_desktop_config_probe(&probe);
            smoke_check(
                apply.success,
                format!(
                    "real Desktop apply failed: error={:?}, steps={:?}",
                    apply.error, apply.steps
                ),
            )?;
            let gateway = apply
                .gateway
                .as_ref()
                .ok_or_else(|| "apply result is missing gateway health".to_owned())?;
            smoke_check(gateway.running, "gateway was not running after apply")?;
            let write = apply
                .write
                .as_ref()
                .ok_or_else(|| "apply result is missing Desktop write result".to_owned())?;
            smoke_check(
                write.health.passed,
                format!("Desktop readback health failed: {:?}", write.health),
            )?;
            smoke_check(
                write.config_path == desktop_root.join("cc-desktop-switch-local-gateway.json"),
                format!(
                    "unexpected config path: expected {}, got {}",
                    desktop_root
                        .join("cc-desktop-switch-local-gateway.json")
                        .display(),
                    write.config_path.display()
                ),
            )?;
            smoke_check(
                write.meta_path == desktop_root.join("_meta.json"),
                format!(
                    "unexpected meta path: expected {}, got {}",
                    desktop_root.join("_meta.json").display(),
                    write.meta_path.display()
                ),
            )?;
            smoke_check(
                write.readback.base_url.as_deref() == Some(gateway.base_url.as_str()),
                format!(
                    "Desktop readback base URL mismatch: expected {}, got {:?}",
                    gateway.base_url, write.readback.base_url
                ),
            )?;
            smoke_check(
                write
                    .readback
                    .inference_models
                    .iter()
                    .all(|model| model.id.starts_with("claude-")),
                format!(
                    "Desktop readback contains non-safe routes: {:?}",
                    write.readback.inference_models
                ),
            )?;
            smoke_check(
                write
                    .readback
                    .inference_models
                    .iter()
                    .all(|model| model.id != "Default"),
                "Desktop readback still exposes Default".to_owned(),
            )?;

            let config_body = fs::read_to_string(&write.config_path)
                .map_err(|error| format!("failed to read written Desktop config: {error}"))?;
            smoke_check(
                config_body.contains("claude-deepseek-v4-pro"),
                "written Desktop config does not contain the expected safe route",
            )?;
            smoke_check(
                !config_body.contains("\"name\":\"deepseek-v4-pro\"")
                    && !config_body.contains("\"name\": \"deepseek-v4-pro\""),
                "written Desktop config contains a raw upstream model route",
            )?;

            let gateway_smoke = state
                .gateway_smoke()
                .map_err(|error| format!("gateway smoke command failed: {error}"))?;
            smoke_check(
                gateway_smoke.passed,
                format!("gateway smoke did not pass: {:?}", gateway_smoke),
            )?;

            Ok(())
        })();

        let stop_result = state
            .stop_gateway()
            .map(|_| ())
            .map_err(|error| error.to_string());
        let restore_result = restore_real_desktop_smoke_files(
            &desktop_root,
            &backup_root,
            &REAL_DESKTOP_SMOKE_FILES,
            desktop_root_existed,
        );
        let restore_check_result = verify_real_desktop_smoke_restore(
            &desktop_root,
            &backup_root,
            &REAL_DESKTOP_SMOKE_FILES,
            desktop_root_existed,
        );
        let config_cleanup_result =
            fs::remove_dir_all(path.parent().unwrap()).map_err(|error| error.to_string());
        let backup_cleanup_result =
            fs::remove_dir_all(&backup_root).map_err(|error| error.to_string());

        let errors = [
            ("smoke", smoke_result),
            ("stop_gateway", stop_result),
            ("restore", restore_result),
            ("restore_check", restore_check_result),
            ("config_cleanup", config_cleanup_result),
            ("backup_cleanup", backup_cleanup_result),
        ]
        .into_iter()
        .filter_map(|(label, result)| result.err().map(|error| format!("{label}: {error}")))
        .collect::<Vec<_>>();

        assert!(errors.is_empty(), "{}", errors.join("; "));
    }

    #[test]
    fn save_provider_persists_config_and_lists_summary_without_secret() {
        let path = temp_config_path("save");
        let state = AppState::with_config_path(&path);

        let summary = state.save_provider(provider("DeepSeek")).unwrap();
        let providers = state.list_providers().unwrap();

        assert_eq!(summary.provider_id, "provider-deepseek");
        assert_eq!(providers.len(), 1);
        assert!(providers[0].has_api_key);
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"schemaVersion\""));
        assert!(saved.contains("sk-secret"));
        assert!(!serde_json::to_string(&providers)
            .unwrap()
            .contains("sk-secret"));

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn set_active_provider_updates_snapshot_model_mappings() {
        let path = temp_config_path("active");
        let state = AppState::with_config_path(&path);

        let first = state.save_provider(provider("DeepSeek")).unwrap();
        let second = state.save_provider(provider("Kimi")).unwrap();

        assert!(state.set_active_provider(&second.provider_id).unwrap());
        let snapshot = state.snapshot().unwrap();
        assert_eq!(
            snapshot.active_provider.as_ref().unwrap().provider_id,
            second.provider_id
        );
        assert_eq!(snapshot.active_model_mappings.len(), 1);
        assert_eq!(snapshot.proxy_port, 18080);
        assert!(snapshot.active_model_mappings[0]
            .route_id
            .as_deref()
            .unwrap()
            .starts_with("claude-kimi"));
        assert!(state.set_active_provider(&first.provider_id).unwrap());

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_parity_edit_delete_reorder_roundtrip() {
        let path = temp_config_path("provider-parity");
        let state = AppState::with_config_path(&path);

        let deepseek = state.save_provider(provider("DeepSeek")).unwrap();
        let kimi = state.save_provider(provider("Kimi")).unwrap();
        let qwen = state.save_provider(provider("Qwen")).unwrap();

        let edited = state
            .save_provider(ProviderDraft {
                provider_id: Some(kimi.provider_id.clone()),
                display_name: "Kimi Updated".to_owned(),
                base_url: "https://api.moonshot.cn/anthropic/".to_owned(),
                api_key: "sk-new-secret".to_owned(),
                api_format: ApiFormat::Anthropic,
            })
            .unwrap();
        assert_eq!(edited.provider_id, kimi.provider_id);

        let providers = state.list_providers().unwrap();
        assert_eq!(providers.len(), 3);
        assert_eq!(providers[1].provider_id, kimi.provider_id);
        assert_eq!(providers[1].display_name, "Kimi Updated");
        assert_eq!(providers[1].base_url, "https://api.moonshot.cn/anthropic");
        assert!(!serde_json::to_string(&providers)
            .unwrap()
            .contains("sk-new-secret"));

        assert!(state.set_active_provider(&qwen.provider_id).unwrap());
        assert!(state
            .reorder_providers(vec![
                qwen.provider_id.clone(),
                deepseek.provider_id.clone(),
                kimi.provider_id.clone(),
            ])
            .unwrap());

        let providers = state.list_providers().unwrap();
        assert_eq!(providers[0].provider_id, qwen.provider_id);
        assert_eq!(providers[1].provider_id, deepseek.provider_id);
        assert_eq!(providers[2].provider_id, kimi.provider_id);

        assert!(state.delete_provider(&qwen.provider_id).unwrap());
        let snapshot = state.snapshot().unwrap();
        assert_eq!(
            snapshot.active_provider.as_ref().unwrap().provider_id,
            deepseek.provider_id
        );
        assert!(!state.delete_provider("provider-missing").unwrap());

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_parity_rejects_invalid_reorder_set() {
        let path = temp_config_path("provider-parity-reorder-invalid");
        let state = AppState::with_config_path(&path);
        let deepseek = state.save_provider(provider("DeepSeek")).unwrap();
        let _kimi = state.save_provider(provider("Kimi")).unwrap();

        let error = state
            .reorder_providers(vec![deepseek.provider_id.clone()])
            .unwrap_err();

        assert!(error.to_string().contains("provider.reorder_invalid_set"));

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_import_preview_does_not_write_config() {
        let path = temp_config_path("provider-import-preview");
        let state = AppState::with_config_path(&path);
        let mut source = AppConfig::empty();
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            api_format: ApiFormat::Anthropic,
            api_key: "sk-secret".to_owned(),
        };
        source.upsert_provider(ConfigProvider::from_provider(provider, 0));
        let raw = serde_json::to_string(&source.export_provider_package()).unwrap();

        let preview = state.preview_provider_import(&raw, false).unwrap();

        assert!(preview.would_write);
        assert_eq!(preview.incoming_provider_count, 1);
        assert!(!path.exists());
    }

    #[test]
    fn provider_import_apply_writes_config_and_exports_package() {
        let path = temp_config_path("provider-import-apply");
        let state = AppState::with_config_path(&path);
        let mut source = AppConfig::empty();
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            api_format: ApiFormat::Anthropic,
            api_key: "sk-secret".to_owned(),
        };
        source.upsert_provider(ConfigProvider::from_provider(provider, 0));
        let raw = serde_json::to_string(&source.export_provider_package()).unwrap();

        let result = state.import_providers(&raw, false).unwrap();
        let exported = state.export_provider_package().unwrap();

        assert!(result.changed);
        assert!(path.exists());
        assert_eq!(exported.providers.len(), 1);
        assert_eq!(exported.providers[0].provider_id, "provider-deepseek");

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_preset_import_writes_config_and_hides_secret_in_summary() {
        let path = temp_config_path("provider-preset-import");
        let state = AppState::with_config_path(&path);

        let presets = state.list_provider_presets();
        let preview = state
            .preview_provider_preset_import("deepseek", false)
            .unwrap();
        let result = state
            .import_provider_preset("deepseek", "sk-preset-secret".to_owned(), false)
            .unwrap();
        let summaries = state.list_providers().unwrap();

        assert!(presets.iter().any(|preset| preset.preset_id == "deepseek"));
        assert!(preview.would_write);
        assert!(result.changed);
        assert_eq!(summaries[0].provider_id, "provider-deepseek");
        assert!(summaries[0].has_api_key);
        assert!(!serde_json::to_string(&summaries)
            .unwrap()
            .contains("sk-preset-secret"));

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn model_mapping_update_persists_safe_routes() {
        let (path, state) = state_with_provider("model-mapping-update", 0);
        let provider_id = state.list_providers().unwrap()[0].provider_id.clone();

        let mappings = state
            .update_model_mappings(
                &provider_id,
                vec![
                    ModelMappingDraft {
                        slot: ModelSlot::Sonnet,
                        upstream_model: "deepseek-v4-pro".to_owned(),
                        route_id: Some("claude-deepseek-v4-pro".to_owned()),
                        supports_1m: true,
                        supports_max: false,
                    },
                    ModelMappingDraft {
                        slot: ModelSlot::Opus,
                        upstream_model: "deepseek-reasoner".to_owned(),
                        route_id: None,
                        supports_1m: false,
                        supports_max: true,
                    },
                    ModelMappingDraft {
                        slot: ModelSlot::Default,
                        upstream_model: "deepseek-v4-pro".to_owned(),
                        route_id: Some("Default".to_owned()),
                        supports_1m: true,
                        supports_max: true,
                    },
                ],
            )
            .unwrap();

        assert_eq!(mappings.len(), 3);
        assert!(mappings
            .iter()
            .any(|mapping| mapping.route_id.as_deref() == Some("claude-deepseek-reasoner")));
        assert!(mappings
            .iter()
            .any(|mapping| mapping.slot == ModelSlot::Default && mapping.route_id.is_none()));
        let snapshot = state.snapshot().unwrap();
        assert_eq!(snapshot.active_model_mappings.len(), 3);

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn config_backup_list_and_readback_are_redacted() {
        let path = temp_config_path("config-backup-redacted");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy = serde_json::json!({
            "version": "1.0.20",
            "activeProvider": "deepseek",
            "providers": [{
                "id": "deepseek",
                "name": "DeepSeek",
                "baseUrl": "https://api.deepseek.com/anthropic",
                "apiFormat": "anthropic",
                "apiKey": "sk-backup-secret",
                "models": {"sonnet": "deepseek-v4-pro"}
            }]
        })
        .to_string();
        fs::write(&path, legacy).unwrap();
        let state = AppState::with_config_path(&path);

        state.save_provider(provider("Kimi")).unwrap();
        let backups = state.list_config_backups().unwrap();
        let redacted = state.read_config_backup(&backups[0].file_name).unwrap();

        assert_eq!(backups.len(), 1);
        assert!(redacted.contains("[REDACTED:key]"));
        assert!(!redacted.contains("sk-backup-secret"));
        assert!(state.read_config_backup("../config.json").is_err());

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn diagnostics_package_can_be_saved_and_issue_draft_built() {
        let (path, state) = state_with_provider("diagnostics-save", 0);

        let saved = state.save_diagnostics_package(None, None).unwrap();
        let draft = state.diagnostics_issue_draft(None, None).unwrap();
        let body = fs::read_to_string(&saved).unwrap();

        assert!(saved.exists());
        assert!(saved.parent().unwrap().ends_with(Path::new("diagnostics")));
        assert!(body.contains("\"schemaVersion\""));
        assert!(draft
            .url
            .contains("https://github.com/lonr-6/cc-desktop-switch/issues/new"));
        assert!(!body.contains("sk-secret"));
        assert!(!draft.url.contains("sk-secret"));

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn smoke_checks_cover_static_and_gateway_models() {
        let (path, state) = state_with_provider("smoke-gateway", 0);

        let static_smoke = state.provider_static_smoke().unwrap();
        let gateway_smoke = state.gateway_smoke().unwrap();

        assert!(static_smoke.passed);
        assert!(gateway_smoke.passed);
        assert_eq!(gateway_smoke.layer, "gateway.smoke");

        state.stop_gateway().unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_real_smoke_stops_before_network_when_key_is_missing() {
        let path = temp_config_path("smoke-provider-missing-key");
        let state = AppState::with_config_path(&path);
        state
            .save_provider(ProviderDraft {
                provider_id: None,
                display_name: "DeepSeek".to_owned(),
                base_url: "https://api.deepseek.com/anthropic".to_owned(),
                api_key: String::new(),
                api_format: ApiFormat::Anthropic,
            })
            .unwrap();

        let smoke = state.provider_real_smoke().unwrap();

        assert!(!smoke.passed);
        assert_eq!(smoke.layer, "provider.real");
        assert_eq!(
            smoke.issue_code.as_deref(),
            Some("provider.api_key_missing")
        );

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn gateway_lifecycle_requires_active_provider() {
        let path = temp_config_path("gateway-no-provider");
        let state = AppState::with_config_path(&path);

        let error = state.start_gateway().unwrap_err();

        assert!(error.to_string().contains("gateway.no_active_provider"));
        assert_eq!(
            state.gateway_issue_code().unwrap().as_deref(),
            Some("gateway.no_active_provider")
        );
        let package = state.diagnostics_package(None, None).unwrap();
        assert!(package
            .runtime_logs
            .iter()
            .any(|entry| entry.code == "gateway.no_active_provider"));
    }

    #[test]
    fn gateway_lifecycle_start_stop_updates_status() {
        let (path, state) = state_with_provider("gateway-lifecycle", 0);

        let started = state.start_gateway().unwrap();
        let port = port_from_base_url(&started.base_url);
        wait_for_tcp(port);
        let running = state.gateway_status().unwrap();

        assert!(started.running);
        assert!(running.running);
        assert_eq!(running.base_url, started.base_url);

        let stopped = state.stop_gateway().unwrap();
        let status = state.gateway_status().unwrap();
        let logs = state.runtime_logs().unwrap();

        assert!(!stopped.running);
        assert!(!status.running);
        assert!(logs.iter().any(|entry| entry.code == "gateway.started"));
        assert!(logs.iter().any(|entry| entry.code == "gateway.stopped"));

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn provider_parity_gateway_restarts_when_active_provider_changes() {
        let (path, state) = state_with_provider("gateway-fingerprint", 0);

        let started = state.start_gateway().unwrap();
        let first_port = port_from_base_url(&started.base_url);
        wait_for_tcp(first_port);
        let first_fingerprint = {
            let runtime = state.gateway.lock().unwrap();
            runtime.config_fingerprint.clone().unwrap()
        };

        state
            .save_provider(ProviderDraft {
                provider_id: Some("provider-deepseek".to_owned()),
                display_name: "DeepSeek Updated".to_owned(),
                base_url: "https://api.deepseek.com/anthropic".to_owned(),
                api_key: "sk-new-secret".to_owned(),
                api_format: ApiFormat::Anthropic,
            })
            .unwrap();

        let restarted = state.start_gateway().unwrap();
        let second_port = port_from_base_url(&restarted.base_url);
        wait_for_tcp(second_port);
        let second_fingerprint = {
            let runtime = state.gateway.lock().unwrap();
            runtime.config_fingerprint.clone().unwrap()
        };

        assert!(restarted.running);
        assert_ne!(first_fingerprint, second_fingerprint);

        state.stop_gateway().unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn gateway_lifecycle_reports_port_in_use() {
        let blocker = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = blocker.local_addr().unwrap().port();
        let (path, state) = state_with_provider("gateway-port-in-use", port);

        let error = state.start_gateway().unwrap_err();

        assert!(error.to_string().contains("gateway.port_in_use"));
        assert_eq!(
            state.gateway_issue_code().unwrap().as_deref(),
            Some("gateway.port_in_use")
        );

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn apply_flow_fixture_succeeds_after_gateway_write_and_readback() {
        let (path, state) = state_with_provider("apply-flow-success", 0);
        let desktop_root = temp_dir("apply-flow-desktop");

        let result = state.apply_to_local_config_library(&desktop_root);

        assert!(result.success);
        assert!(result.gateway.as_ref().unwrap().running);
        assert!(result.write.as_ref().unwrap().health.passed);
        assert!(result.steps.iter().any(|step| {
            step.id == "desktop.readback" && step.status == DesktopApplyStepStatus::Passed
        }));

        state.stop_gateway().unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        fs::remove_dir_all(desktop_root).unwrap();
    }

    #[test]
    fn apply_flow_fixture_blocks_missing_provider_without_write() {
        let path = temp_config_path("apply-flow-missing-provider");
        let state = AppState::with_config_path(&path);
        let desktop_root = temp_dir("apply-flow-no-provider");

        let result = state.apply_to_local_config_library(&desktop_root);

        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("provider.not_configured"));
        assert!(result.write.is_none());
        assert!(!desktop_root.exists());
    }

    #[test]
    fn apply_flow_fixture_blocks_port_conflict_before_write() {
        let blocker = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = blocker.local_addr().unwrap().port();
        let (path, state) = state_with_provider("apply-flow-port-conflict", port);
        let desktop_root = temp_dir("apply-flow-port-conflict");

        let result = state.apply_to_local_config_library(&desktop_root);

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("gateway.port_in_use"));
        assert!(result.write.is_none());
        assert!(!desktop_root.exists());

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn apply_flow_probe_succeeds_with_unmanaged_detected_path() {
        let (path, state) = state_with_provider("apply-flow-probe-success", 0);
        let desktop_root = temp_dir("apply-flow-probe-desktop");
        let probe = DesktopConfigProbe {
            platform: DesktopPlatform::Windows,
            local_config_library: desktop_root.clone(),
            managed_detected: false,
            managed_evidence: Vec::new(),
            issue_codes: vec!["desktop.local_config_available".to_owned()],
        };

        let result = state.apply_to_desktop_config_probe(&probe);

        assert!(result.success);
        assert_eq!(
            result.desktop_config.as_ref().unwrap().local_config_library,
            desktop_root
        );
        assert!(result.steps.iter().any(|step| {
            step.id == "desktop.config_probe" && step.status == DesktopApplyStepStatus::Passed
        }));

        state.stop_gateway().unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        fs::remove_dir_all(desktop_root).unwrap();
    }

    #[test]
    fn apply_flow_probe_blocks_managed_config_before_gateway_or_write() {
        let (path, state) = state_with_provider("apply-flow-probe-managed", 0);
        let desktop_root = temp_dir("apply-flow-probe-managed-desktop");
        let probe = DesktopConfigProbe {
            platform: DesktopPlatform::Windows,
            local_config_library: desktop_root.clone(),
            managed_detected: true,
            managed_evidence: vec![ManagedConfigEvidence {
                code: "desktop.managed_config_detected".to_owned(),
                location: "HKCU\\SOFTWARE\\Policies\\Claude".to_owned(),
                detail: "Windows registry policy exists".to_owned(),
            }],
            issue_codes: vec!["desktop.managed_config_detected".to_owned()],
        };

        let result = state.apply_to_desktop_config_probe(&probe);
        let gateway = state.gateway_status().unwrap();

        assert!(!result.success);
        assert_eq!(
            result.error.as_deref(),
            Some("desktop.managed_config_detected")
        );
        assert!(!gateway.running);
        assert!(result.write.is_none());
        assert!(!desktop_root.exists());

        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    #[ignore = "writes the current user's Claude Desktop local configLibrary; run with CCDS_ALLOW_REAL_DESKTOP_WRITE=1"]
    fn windows_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores() {
        if !cfg!(target_os = "windows") {
            eprintln!("skipped: Windows-only real Desktop smoke");
            return;
        }

        run_real_desktop_local_config_smoke(
            DesktopPlatform::Windows,
            "windows-real-desktop-smoke",
            "windows-real-desktop-backup",
        );
    }

    #[test]
    #[ignore = "writes the current user's Claude Desktop local configLibrary; run with CCDS_ALLOW_REAL_DESKTOP_WRITE=1"]
    fn macos_real_desktop_local_config_smoke_writes_readbacks_gateway_and_restores() {
        if !cfg!(target_os = "macos") {
            eprintln!("skipped: macOS-only real Desktop smoke");
            return;
        }

        run_real_desktop_local_config_smoke(
            DesktopPlatform::Macos,
            "macos-real-desktop-smoke",
            "macos-real-desktop-backup",
        );
    }
}
