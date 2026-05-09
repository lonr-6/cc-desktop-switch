use serde::{Deserialize, Serialize};

use crate::gateway::planned_gateway_health_for_port;
use crate::model_catalog::{DesktopModel, ModelCatalog, ModelMapping, RouteError};
use crate::provider::Provider;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPlan {
    pub base_url: String,
    pub gateway_api_key: String,
    pub auth_scheme: String,
    pub gateway_headers: Vec<String>,
    pub inference_models: Vec<DesktopModel>,
    pub expected_routes: Vec<String>,
    pub expected_capabilities: Vec<ExpectedCapability>,
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExpectedCapability {
    pub route_id: String,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopReadback {
    pub base_url: Option<String>,
    pub inference_models: Vec<DesktopModel>,
    pub mode: Option<String>,
    pub auth_scheme: Option<String>,
    pub gateway_api_key_present: Option<bool>,
    #[serde(default)]
    pub gateway_headers: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopHealth {
    pub passed: bool,
    pub issues: Vec<DesktopHealthIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopHealthIssue {
    pub code: String,
    pub expected: String,
    pub actual: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyDryRun {
    pub mode: String,
    pub success: bool,
    pub expected_base_url: String,
    pub expected_models: Vec<DesktopModel>,
    pub plan: Option<DesktopPlan>,
    pub plan_error: Option<String>,
    pub steps: Vec<ApplyStep>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyStep {
    pub id: String,
    pub label: String,
    pub would_run: bool,
}

pub fn build_apply_dry_run(
    provider: Option<&Provider>,
    mappings: &[ModelMapping],
    gateway_port: u16,
    gateway_api_key: &str,
) -> ApplyDryRun {
    let plan_result = provider
        .map(|provider| build_desktop_plan(provider, mappings, gateway_api_key, gateway_port));
    let (plan, plan_error) = match plan_result {
        Some(Ok(plan)) => (Some(plan), None),
        Some(Err(error)) => (None, Some(format!("{}: {}", error.code, error.message))),
        None => (None, None),
    };
    let expected_base_url = plan
        .as_ref()
        .map(|plan| plan.base_url.clone())
        .unwrap_or_else(|| planned_gateway_health_for_port(gateway_port).base_url);
    let expected_models = plan
        .as_ref()
        .map(|plan| plan.inference_models.clone())
        .unwrap_or_default();

    ApplyDryRun {
        mode: "local_gateway".to_owned(),
        success: false,
        expected_base_url,
        expected_models,
        plan,
        plan_error,
        steps: vec![
            ApplyStep {
                id: "provider.save".to_owned(),
                label: "Save provider".to_owned(),
                would_run: provider.is_some(),
            },
            ApplyStep {
                id: "provider.set_default".to_owned(),
                label: "Set active provider".to_owned(),
                would_run: provider.is_some(),
            },
            ApplyStep {
                id: "model_catalog.build".to_owned(),
                label: "Build Claude-safe model catalog".to_owned(),
                would_run: provider.is_some(),
            },
            ApplyStep {
                id: "gateway.ensure_running".to_owned(),
                label: "Ensure local gateway".to_owned(),
                would_run: false,
            },
            ApplyStep {
                id: "desktop.write".to_owned(),
                label: "Write Claude Desktop config".to_owned(),
                would_run: false,
            },
            ApplyStep {
                id: "desktop.readback".to_owned(),
                label: "Read back and compare".to_owned(),
                would_run: false,
            },
        ],
    }
}

pub fn build_desktop_plan(
    provider: &Provider,
    mappings: &[ModelMapping],
    gateway_api_key: &str,
    gateway_port: u16,
) -> Result<DesktopPlan, RouteError> {
    let catalog = catalog_for_provider(provider, mappings)?;
    let inference_models = catalog.desktop_models();
    if inference_models.is_empty() {
        return Err(RouteError {
            code: "model_catalog.no_visible_routes".to_owned(),
            message: "active provider has no explicit Desktop-visible model mappings".to_owned(),
        });
    }

    let expected_routes = inference_models
        .iter()
        .map(|model| model.id.clone())
        .collect::<Vec<_>>();
    let expected_capabilities = inference_models
        .iter()
        .map(|model| ExpectedCapability {
            route_id: model.id.clone(),
            supports_1m: model.supports_1m,
            supports_max: model.supports_max,
        })
        .collect();

    Ok(DesktopPlan {
        base_url: planned_gateway_health_for_port(gateway_port).base_url,
        gateway_api_key: gateway_api_key.to_owned(),
        auth_scheme: "bearer".to_owned(),
        gateway_headers: Vec::new(),
        inference_models,
        expected_routes,
        expected_capabilities,
        mode: "local_gateway".to_owned(),
    })
}

pub fn compare_desktop_readback(expected: &DesktopPlan, actual: &DesktopReadback) -> DesktopHealth {
    let mut issues = Vec::new();

    if actual.base_url.as_deref() != Some(expected.base_url.as_str()) {
        issues.push(DesktopHealthIssue {
            code: "desktop.stale_base_url".to_owned(),
            expected: expected.base_url.clone(),
            actual: actual.base_url.clone().unwrap_or_default(),
        });
    }

    if actual.mode.as_deref() != Some(expected.mode.as_str()) {
        issues.push(DesktopHealthIssue {
            code: "desktop.mode_mismatch".to_owned(),
            expected: expected.mode.clone(),
            actual: actual.mode.clone().unwrap_or_default(),
        });
    }

    if actual.auth_scheme.as_deref() != Some(expected.auth_scheme.as_str()) {
        issues.push(DesktopHealthIssue {
            code: "desktop.auth_scheme_mismatch".to_owned(),
            expected: expected.auth_scheme.clone(),
            actual: actual.auth_scheme.clone().unwrap_or_default(),
        });
    }

    if actual.gateway_api_key_present != Some(!expected.gateway_api_key.is_empty()) {
        issues.push(DesktopHealthIssue {
            code: "desktop.gateway_key_missing".to_owned(),
            expected: (!expected.gateway_api_key.is_empty()).to_string(),
            actual: actual
                .gateway_api_key_present
                .map(|value| value.to_string())
                .unwrap_or_default(),
        });
    }

    if actual.gateway_headers != expected.gateway_headers {
        issues.push(DesktopHealthIssue {
            code: "desktop.gateway_headers_mismatch".to_owned(),
            expected: expected.gateway_headers.join(","),
            actual: actual.gateway_headers.join(","),
        });
    }

    for route in &expected.expected_routes {
        if !actual
            .inference_models
            .iter()
            .any(|model| &model.id == route)
        {
            issues.push(DesktopHealthIssue {
                code: "desktop.config_readback_mismatch".to_owned(),
                expected: route.clone(),
                actual: "missing route".to_owned(),
            });
        }
    }

    for actual_model in &actual.inference_models {
        if actual_model.id.starts_with("claude-")
            && !expected
                .expected_routes
                .iter()
                .any(|route| route == &actual_model.id)
        {
            issues.push(DesktopHealthIssue {
                code: "desktop.config_readback_mismatch".to_owned(),
                expected: "exact route set".to_owned(),
                actual: format!("unexpected route {}", actual_model.id),
            });
        }
    }

    for expected_model in &expected.inference_models {
        if let Some(actual_model) = actual
            .inference_models
            .iter()
            .find(|model| model.id == expected_model.id)
        {
            if expected_model.supports_1m && !actual_model.supports_1m {
                issues.push(DesktopHealthIssue {
                    code: "desktop.one_million_not_written".to_owned(),
                    expected: format!("{} supports1m=true", expected_model.id),
                    actual: format!("{} supports1m=false", actual_model.id),
                });
            }

            if expected_model.supports_max != actual_model.supports_max {
                issues.push(DesktopHealthIssue {
                    code: "desktop.max_capability_mismatch".to_owned(),
                    expected: format!(
                        "{} supportsMax={}",
                        expected_model.id, expected_model.supports_max
                    ),
                    actual: format!(
                        "{} supportsMax={}",
                        actual_model.id, actual_model.supports_max
                    ),
                });
            }
        }
    }

    for actual_model in &actual.inference_models {
        if !actual_model.id.starts_with("claude-") {
            issues.push(DesktopHealthIssue {
                code: "desktop.raw_model_names_detected".to_owned(),
                expected: "claude-* route".to_owned(),
                actual: actual_model.id.clone(),
            });
        }
    }

    DesktopHealth {
        passed: issues.is_empty(),
        issues,
    }
}

fn catalog_for_provider(
    provider: &Provider,
    mappings: &[ModelMapping],
) -> Result<ModelCatalog, RouteError> {
    ModelCatalog::from_mappings(provider, mappings.to_vec())
}

#[cfg(test)]
mod tests {
    use crate::model_catalog::{ModelMapping, ModelSlot, RouteCapabilities};
    use crate::provider::{ApiFormat, ProviderDraft};

    use super::*;

    fn provider() -> Provider {
        ProviderDraft {
            provider_id: None,
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
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

    #[test]
    fn dry_run_never_reports_applied_success() {
        let provider = provider();

        let plan = build_apply_dry_run(Some(&provider), &mappings(), 18080, "ccds_dry_run_key");

        assert!(!plan.success);
        assert_eq!(plan.mode, "local_gateway");
        assert_eq!(plan.expected_base_url, "http://127.0.0.1:18080");
        assert_eq!(plan.expected_models[0].id, "claude-deepseek-v4-pro");
        assert!(plan.plan.is_some());
        assert!(plan.plan_error.is_none());
        assert!(plan.steps.iter().any(|step| step.id == "desktop.readback"));
    }

    #[test]
    fn desktop_plan_uses_local_gateway_and_catalog_routes() {
        let provider = provider();

        let plan = build_desktop_plan(&provider, &mappings(), "gateway-key", 18080).unwrap();

        assert_eq!(plan.mode, "local_gateway");
        assert_eq!(plan.base_url, "http://127.0.0.1:18080");
        assert_eq!(plan.gateway_api_key, "gateway-key");
        assert_eq!(plan.auth_scheme, "bearer");
        assert!(plan
            .expected_routes
            .contains(&"claude-deepseek-v4-pro".to_owned()));
        assert!(!serde_json::to_string(&plan)
            .unwrap()
            .contains("\"deepseek-v4-pro\""));
    }

    #[test]
    fn desktop_plan_rejects_empty_mappings_without_fixture_fallback() {
        let provider = provider();

        let error = build_desktop_plan(&provider, &[], "gateway-key", 18080).unwrap_err();

        assert_eq!(error.code, "model_catalog.no_visible_routes");
    }

    #[test]
    fn desktop_plan_rejects_invalid_raw_route_without_swallowing_error() {
        let provider = provider();
        let invalid_mappings = vec![ModelMapping {
            slot: ModelSlot::Sonnet,
            upstream_model: "deepseek-v4-pro".to_owned(),
            route_id: Some("deepseek-v4-pro".to_owned()),
            capabilities: RouteCapabilities::default(),
        }];

        let error =
            build_desktop_plan(&provider, &invalid_mappings, "gateway-key", 18080).unwrap_err();

        assert_eq!(error.code, "desktop.raw_model_names_detected");
    }

    #[test]
    fn readback_mismatch_blocks_apply_success() {
        let provider = provider();
        let plan = build_desktop_plan(&provider, &mappings(), "gateway-key", 18080).unwrap();
        let actual = DesktopReadback {
            base_url: Some("https://api.deepseek.com/anthropic".to_owned()),
            inference_models: vec![DesktopModel {
                id: "deepseek-v4-pro".to_owned(),
                display_name: "deepseek-v4-pro".to_owned(),
                supports_1m: false,
                supports_max: false,
            }],
            mode: Some("local_gateway".to_owned()),
            auth_scheme: Some("bearer".to_owned()),
            gateway_api_key_present: Some(true),
            gateway_headers: Vec::new(),
        };

        let health = compare_desktop_readback(&plan, &actual);

        assert!(!health.passed);
        assert!(health
            .issues
            .iter()
            .any(|issue| issue.code == "desktop.stale_base_url"));
        assert!(health
            .issues
            .iter()
            .any(|issue| issue.code == "desktop.raw_model_names_detected"));
    }

    #[test]
    fn readback_rejects_extra_route_and_auth_mismatch() {
        let provider = provider();
        let plan = build_desktop_plan(&provider, &mappings(), "gateway-key", 18080).unwrap();
        let mut actual_models = plan.inference_models.clone();
        actual_models.push(DesktopModel {
            id: "claude-old-route".to_owned(),
            display_name: "claude-old-route".to_owned(),
            supports_1m: false,
            supports_max: false,
        });
        let actual = DesktopReadback {
            base_url: Some(plan.base_url.clone()),
            inference_models: actual_models,
            mode: Some("direct_provider".to_owned()),
            auth_scheme: Some("none".to_owned()),
            gateway_api_key_present: Some(false),
            gateway_headers: Vec::new(),
        };

        let health = compare_desktop_readback(&plan, &actual);

        assert!(!health.passed);
        assert!(health
            .issues
            .iter()
            .any(|issue| issue.code == "desktop.mode_mismatch"));
        assert!(health
            .issues
            .iter()
            .any(|issue| issue.code == "desktop.auth_scheme_mismatch"));
        assert!(health
            .issues
            .iter()
            .any(|issue| issue.code == "desktop.gateway_key_missing"));
        assert!(health.issues.iter().any(|issue| {
            issue.code == "desktop.config_readback_mismatch"
                && issue.actual.contains("claude-old-route")
        }));
    }

    #[test]
    fn desktop_plan_uses_configured_gateway_port() {
        let provider = provider();

        let plan = build_desktop_plan(&provider, &mappings(), "gateway-key", 18088).unwrap();

        assert_eq!(plan.base_url, "http://127.0.0.1:18088");
    }
}
