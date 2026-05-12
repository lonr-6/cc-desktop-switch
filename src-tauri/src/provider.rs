use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDraft {
    #[serde(default)]
    pub provider_id: Option<String>,
    pub display_name: String,
    pub base_url: String,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    pub api_key: String,
    pub api_format: ApiFormat,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub api_format: ApiFormat,
    pub api_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    Anthropic,
    OpenAiChat,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    #[default]
    Bearer,
    XApiKey,
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub api_format: ApiFormat,
    pub has_api_key: bool,
}

impl ProviderDraft {
    pub fn into_provider(self) -> Result<Provider, String> {
        let display_name = self.display_name.trim().to_owned();
        let base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        let api_key = self.api_key.trim().to_owned();

        if display_name.is_empty() {
            return Err("provider name is required".to_owned());
        }
        if base_url.is_empty() {
            return Err("base URL is required".to_owned());
        }
        if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err("base URL must start with http:// or https://".to_owned());
        }

        let provider_id = self
            .provider_id
            .as_deref()
            .map(stable_provider_id_from_raw)
            .filter(|provider_id| !provider_id.is_empty())
            .unwrap_or_else(|| stable_provider_id(&display_name));
        Ok(Provider {
            provider_id,
            display_name,
            base_url,
            auth_scheme: self.auth_scheme,
            api_format: self.api_format,
            api_key,
        })
    }
}

impl Provider {
    pub fn summary(&self) -> ProviderSummary {
        ProviderSummary {
            provider_id: self.provider_id.clone(),
            display_name: self.display_name.clone(),
            base_url: self.base_url.clone(),
            auth_scheme: self.auth_scheme.clone(),
            api_format: self.api_format.clone(),
            has_api_key: !self.api_key.is_empty(),
        }
    }
}

fn stable_provider_id(display_name: &str) -> String {
    let slug = stable_provider_id_from_raw(display_name);
    if slug.is_empty() {
        "provider-custom".to_owned()
    } else {
        format!("provider-{slug}")
    }
}

fn stable_provider_id_from_raw(value: &str) -> String {
    let mut slug = String::new();

    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }

    slug.trim_matches('-').chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_does_not_echo_api_key() {
        let provider = ProviderDraft {
            provider_id: None,
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic/".to_owned(),
            auth_scheme: AuthScheme::Bearer,
            api_key: "sk-secret".to_owned(),
            api_format: ApiFormat::Anthropic,
        }
        .into_provider()
        .expect("provider should validate");

        let summary = provider.summary();
        assert_eq!(summary.base_url, "https://api.deepseek.com/anthropic");
        assert!(summary.has_api_key);
        assert!(!serde_json::to_string(&summary)
            .unwrap()
            .contains("sk-secret"));
    }
}
