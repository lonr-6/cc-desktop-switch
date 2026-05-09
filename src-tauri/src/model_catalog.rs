use serde::{Deserialize, Serialize};

use crate::provider::Provider;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelSlot {
    Sonnet,
    Opus,
    Haiku,
    Default,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteCapabilities {
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelMapping {
    pub slot: ModelSlot,
    pub upstream_model: String,
    pub route_id: Option<String>,
    pub capabilities: RouteCapabilities,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelRoute {
    pub route_id: String,
    pub provider_id: String,
    pub slot: ModelSlot,
    pub upstream_model: String,
    pub supports_1m: bool,
    pub supports_max: bool,
    pub visible_to_desktop: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub routes: Vec<ModelRoute>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopModel {
    pub id: String,
    pub display_name: String,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteResolution {
    pub route_id: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub supports_1m: bool,
    pub supports_max: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestOptions {
    pub use_max: bool,
}

impl ModelCatalog {
    pub fn for_provider(provider: &Provider) -> Self {
        Self::from_mappings(provider, deepseek_fixture_mappings())
            .expect("built-in DeepSeek fixture should be valid")
    }

    pub fn from_mappings(
        provider: &Provider,
        mappings: Vec<ModelMapping>,
    ) -> Result<Self, RouteError> {
        let mut routes = Vec::new();

        for mapping in mappings {
            if mapping.slot == ModelSlot::Default {
                continue;
            }

            let upstream_model = mapping.upstream_model.trim().to_owned();
            if upstream_model.is_empty() {
                continue;
            }

            let route_id = match mapping.route_id {
                Some(route_id) => validate_explicit_route_id(&route_id, &upstream_model)?,
                None => route_for_model(provider, &upstream_model),
            };

            let route_id = disambiguate_route_id(route_id, provider, &routes);

            routes.push(ModelRoute {
                route_id,
                provider_id: provider.provider_id.clone(),
                slot: mapping.slot,
                upstream_model,
                supports_1m: mapping.capabilities.supports_1m,
                supports_max: mapping.capabilities.supports_max,
                visible_to_desktop: true,
            });
        }

        Ok(Self {
            routes: deduplicate_routes(routes),
        })
    }

    pub fn desktop_models(&self) -> Vec<DesktopModel> {
        self.routes
            .iter()
            .filter(|route| route.visible_to_desktop)
            .map(|route| DesktopModel {
                id: route.route_id.clone(),
                display_name: route.route_id.clone(),
                supports_1m: route.supports_1m,
                supports_max: route.supports_max,
            })
            .collect()
    }

    pub fn resolve_route(&self, route_id: &str) -> Result<RouteResolution, RouteError> {
        self.routes
            .iter()
            .find(|route| route.route_id == route_id)
            .map(|route| RouteResolution {
                route_id: route.route_id.clone(),
                provider_id: route.provider_id.clone(),
                upstream_model: route.upstream_model.clone(),
                supports_1m: route.supports_1m,
                supports_max: route.supports_max,
            })
            .ok_or_else(|| RouteError {
                code: "gateway.unmapped_model_route".to_owned(),
                message: format!(
                    "model route '{route_id}' is not mapped; Default is not a fallback"
                ),
            })
    }

    pub fn validate_request_options(
        &self,
        route_id: &str,
        options: RequestOptions,
    ) -> Result<RouteResolution, RouteError> {
        let resolution = self.resolve_route(route_id)?;
        if options.use_max && !resolution.supports_max {
            return Err(RouteError {
                code: "provider.max_not_supported".to_owned(),
                message: format!("model route '{route_id}' does not support Max/thinking"),
            });
        }

        Ok(resolution)
    }
}

pub fn route_for_provider(provider: &Provider) -> String {
    route_for_model(provider, "deepseek-v4-pro")
}

pub fn route_for_model(provider: &Provider, upstream_model: &str) -> String {
    let provider_slug = provider
        .provider_id
        .trim_start_matches("provider-")
        .trim_matches('-');
    let provider_slug = if provider_slug.is_empty() {
        "custom"
    } else {
        provider_slug
    };
    let model_slug = slugify(upstream_model);

    if model_slug == provider_slug || model_slug.starts_with(&format!("{provider_slug}-")) {
        format!("claude-{model_slug}")
    } else {
        format!("claude-{provider_slug}-{model_slug}")
    }
}

fn validate_explicit_route_id(route_id: &str, upstream_model: &str) -> Result<String, RouteError> {
    let trimmed = route_id.trim();
    if !trimmed.starts_with("claude-") {
        return Err(RouteError {
            code: "desktop.raw_model_names_detected".to_owned(),
            message: format!(
                "desktop route '{trimmed}' is not Claude-safe; use a claude-* route alias"
            ),
        });
    }
    if trimmed == upstream_model.trim() {
        return Err(RouteError {
            code: "desktop.raw_model_names_detected".to_owned(),
            message: format!("desktop route '{trimmed}' equals the raw upstream model name"),
        });
    }
    if trimmed.to_ascii_lowercase().contains("default") {
        return Err(RouteError {
            code: "gateway.unmapped_model_route".to_owned(),
            message: "`Default` is not a Desktop-visible route".to_owned(),
        });
    }

    Ok(trimmed.to_owned())
}

fn disambiguate_route_id(
    route_id: String,
    provider: &Provider,
    existing_routes: &[ModelRoute],
) -> String {
    if !existing_routes
        .iter()
        .any(|route| route.route_id == route_id && route.provider_id != provider.provider_id)
    {
        return route_id;
    }

    format!("{route_id}-{}", provider_suffix(&provider.provider_id))
}

fn deduplicate_routes(routes: Vec<ModelRoute>) -> Vec<ModelRoute> {
    routes.into_iter().fold(Vec::new(), |mut unique, route| {
        if !unique.iter().any(|existing: &ModelRoute| {
            existing.route_id == route.route_id && existing.provider_id == route.provider_id
        }) {
            unique.push(route);
        }
        unique
    })
}

fn deepseek_fixture_mappings() -> Vec<ModelMapping> {
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
        ModelMapping {
            slot: ModelSlot::Default,
            upstream_model: "deepseek-v4-pro".to_owned(),
            route_id: Some("Default".to_owned()),
            capabilities: RouteCapabilities::default(),
        },
    ]
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }

    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "model".to_owned()
    } else {
        slug.to_owned()
    }
}

fn provider_suffix(provider_id: &str) -> String {
    let slug = slugify(provider_id);
    slug.chars().take(6).collect::<String>()
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn desktop_models_use_safe_routes_only() {
        let catalog = ModelCatalog::for_provider(&provider());
        let models = catalog.desktop_models();

        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "claude-deepseek-v4-pro");
        assert!(models[0].supports_1m);
        assert!(!models[0].supports_max);
        let desktop_json = serde_json::to_string(&models).unwrap();
        assert!(!desktop_json.contains("\"id\":\"deepseek-v4-pro\""));
        assert!(!desktop_json.contains("\"displayName\":\"deepseek-v4-pro\""));
        assert!(!desktop_json.contains("upstreamModel"));
        assert!(!desktop_json.contains("Default"));
    }

    #[test]
    fn default_mapping_is_not_desktop_visible_or_resolvable() {
        let catalog = ModelCatalog::from_mappings(
            &provider(),
            vec![ModelMapping {
                slot: ModelSlot::Default,
                upstream_model: "deepseek-v4-pro".to_owned(),
                route_id: Some("Default".to_owned()),
                capabilities: RouteCapabilities {
                    supports_1m: true,
                    supports_max: true,
                },
            }],
        )
        .unwrap();

        assert!(catalog.desktop_models().is_empty());
        assert_eq!(
            catalog.resolve_route("Default").unwrap_err().code,
            "gateway.unmapped_model_route"
        );
    }

    #[test]
    fn explicit_raw_route_id_is_rejected() {
        let error = ModelCatalog::from_mappings(
            &provider(),
            vec![ModelMapping {
                slot: ModelSlot::Sonnet,
                upstream_model: "deepseek-v4-pro".to_owned(),
                route_id: Some("deepseek-v4-pro".to_owned()),
                capabilities: RouteCapabilities::default(),
            }],
        )
        .unwrap_err();

        assert_eq!(error.code, "desktop.raw_model_names_detected");
        assert!(error.message.contains("Claude-safe"));
    }

    #[test]
    fn explicit_default_route_alias_is_rejected() {
        let error = ModelCatalog::from_mappings(
            &provider(),
            vec![ModelMapping {
                slot: ModelSlot::Sonnet,
                upstream_model: "deepseek-v4-pro".to_owned(),
                route_id: Some("claude-default".to_owned()),
                capabilities: RouteCapabilities::default(),
            }],
        )
        .unwrap_err();

        assert_eq!(error.code, "gateway.unmapped_model_route");
    }

    #[test]
    fn unmapped_route_is_rejected_without_default_fallback() {
        let catalog = ModelCatalog::for_provider(&provider());
        let error = catalog.resolve_route("claude-missing-route").unwrap_err();

        assert_eq!(error.code, "gateway.unmapped_model_route");
        assert!(error.message.contains("Default is not a fallback"));
    }

    #[test]
    fn supports_1m_and_max_are_attached_to_explicit_routes() {
        let catalog = ModelCatalog::for_provider(&provider());
        let sonnet = catalog.resolve_route("claude-deepseek-v4-pro").unwrap();
        let opus = catalog.resolve_route("claude-deepseek-reasoner").unwrap();

        assert!(sonnet.supports_1m);
        assert!(!sonnet.supports_max);
        assert!(!opus.supports_1m);
        assert!(opus.supports_max);
    }

    #[test]
    fn max_request_is_gated_by_route_capability() {
        let catalog = ModelCatalog::for_provider(&provider());

        let error = catalog
            .validate_request_options("claude-deepseek-v4-pro", RequestOptions { use_max: true })
            .unwrap_err();
        assert_eq!(error.code, "provider.max_not_supported");

        let supported = catalog
            .validate_request_options("claude-deepseek-reasoner", RequestOptions { use_max: true })
            .unwrap();
        assert_eq!(supported.upstream_model, "deepseek-reasoner");
    }

    #[test]
    fn route_generation_uses_provider_and_model_slugs() {
        let custom = ProviderDraft {
            provider_id: None,
            display_name: "Kimi".to_owned(),
            base_url: "https://api.moonshot.cn/anthropic".to_owned(),
            api_key: "sk-test".to_owned(),
            api_format: ApiFormat::Anthropic,
        }
        .into_provider()
        .unwrap();

        assert_eq!(route_for_model(&custom, "kimi-k2.6"), "claude-kimi-k2-6");
    }
}
