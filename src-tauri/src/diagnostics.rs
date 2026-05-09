use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, ConfigProvider};
use crate::desktop_writer::DesktopConfigProbe;
use crate::gateway::GatewayHealth;
use crate::provider::{Provider, ProviderSummary};

pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReadinessSnapshot {
    pub provider_configured: bool,
    pub desktop_readback_passed: bool,
    pub provider_smoke_passed: bool,
    pub gateway_smoke_passed: bool,
    pub gateway: GatewayHealth,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsPackage {
    pub schema_version: u32,
    pub generated_at_unix_ms: u128,
    pub app: DiagnosticsAppSection,
    pub config: DiagnosticsConfigSection,
    pub gateway: GatewayHealth,
    pub desktop: DiagnosticsDesktopSection,
    pub runtime_logs: Vec<DiagnosticsLogEntry>,
    pub readiness: ReadinessSnapshot,
    pub redacted_config_json: Option<String>,
    pub issue_codes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsAppSection {
    pub app_name: String,
    pub app_version: String,
    pub target_version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsConfigSection {
    pub config_path: String,
    pub provider_count: usize,
    pub active_provider_id: Option<String>,
    pub active_provider_display_name: Option<String>,
    pub proxy_port: u16,
    pub gateway_api_key_present: bool,
    pub providers: Vec<ProviderSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsDesktopSection {
    pub probe: Option<DesktopConfigProbe>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsLogEntry {
    pub timestamp_unix_ms: u128,
    pub level: String,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsIssueDraft {
    pub title: String,
    pub body: String,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SmokeCheckResult {
    pub layer: String,
    pub passed: bool,
    pub issue_code: Option<String>,
    pub detail: String,
}

pub fn readiness_snapshot(
    provider: Option<&Provider>,
    gateway: GatewayHealth,
    gateway_issue_code: Option<&str>,
) -> ReadinessSnapshot {
    let mut issue_codes = Vec::new();
    if provider.is_none() {
        issue_codes.push("provider.not_configured".to_owned());
    }
    if !gateway.running {
        issue_codes.push(
            gateway_issue_code
                .unwrap_or("gateway.not_running")
                .to_owned(),
        );
    }
    issue_codes.push("diagnostics.false_green_readiness".to_owned());

    ReadinessSnapshot {
        provider_configured: provider.is_some(),
        desktop_readback_passed: false,
        provider_smoke_passed: false,
        gateway_smoke_passed: gateway.running,
        gateway,
        issue_codes,
    }
}

pub fn provider_static_smoke(provider: Option<&Provider>) -> SmokeCheckResult {
    let Some(provider) = provider else {
        return smoke_fail(
            "provider.static",
            "provider.not_configured",
            "active provider is not configured",
        );
    };
    if provider.base_url.trim().is_empty() {
        return smoke_fail(
            "provider.static",
            "provider.base_url_missing",
            "provider base URL is missing",
        );
    }
    if !(provider.base_url.starts_with("http://") || provider.base_url.starts_with("https://")) {
        return smoke_fail(
            "provider.static",
            "provider.base_url_invalid",
            "provider base URL must start with http:// or https://",
        );
    }
    if provider.api_key.trim().is_empty() {
        return smoke_fail(
            "provider.static",
            "provider.api_key_missing",
            "provider API key is missing",
        );
    }

    SmokeCheckResult {
        layer: "provider.static".to_owned(),
        passed: true,
        issue_code: None,
        detail: format!("{} static config is complete", provider.provider_id),
    }
}

pub fn smoke_fail(layer: &str, issue_code: &str, detail: &str) -> SmokeCheckResult {
    SmokeCheckResult {
        layer: layer.to_owned(),
        passed: false,
        issue_code: Some(issue_code.to_owned()),
        detail: redact_diagnostics_text(detail),
    }
}

pub fn smoke_pass(layer: &str, detail: &str) -> SmokeCheckResult {
    SmokeCheckResult {
        layer: layer.to_owned(),
        passed: true,
        issue_code: None,
        detail: redact_diagnostics_text(detail),
    }
}

pub fn build_diagnostics_package(
    config_path: &Path,
    config: &AppConfig,
    gateway: GatewayHealth,
    gateway_issue_code: Option<&str>,
    desktop_probe: Option<DesktopConfigProbe>,
    desktop_error: Option<String>,
    runtime_logs: Vec<DiagnosticsLogEntry>,
) -> DiagnosticsPackage {
    let active_provider = config.active_provider().map(ConfigProvider::as_provider);
    let readiness = readiness_snapshot(
        active_provider.as_ref(),
        gateway.clone(),
        gateway_issue_code,
    );
    let mut issue_codes = readiness.issue_codes.clone();
    if let Some(probe) = desktop_probe.as_ref() {
        issue_codes.extend(probe.issue_codes.clone());
    }
    if desktop_error.is_some() {
        issue_codes.push("desktop.config_probe_failed".to_owned());
    }
    issue_codes.sort();
    issue_codes.dedup();

    let redacted_config_json = serde_json::to_string_pretty(config)
        .ok()
        .map(|raw| redact_diagnostics_text(&raw));

    DiagnosticsPackage {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        generated_at_unix_ms: now_millis(),
        app: DiagnosticsAppSection {
            app_name: "CC Desktop Switch".to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            target_version: "v1.1.0-rc1".to_owned(),
        },
        config: DiagnosticsConfigSection {
            config_path: config_path.display().to_string(),
            provider_count: config.providers.len(),
            active_provider_id: active_provider
                .as_ref()
                .map(|provider| provider.provider_id.clone()),
            active_provider_display_name: active_provider
                .as_ref()
                .map(|provider| provider.display_name.clone()),
            proxy_port: config.settings.proxy_port,
            gateway_api_key_present: config.gateway_api_key.is_some(),
            providers: config
                .providers
                .iter()
                .map(ConfigProvider::summary)
                .collect(),
        },
        gateway,
        desktop: DiagnosticsDesktopSection {
            probe: desktop_probe,
            error: desktop_error.map(|error| redact_diagnostics_text(&error)),
        },
        runtime_logs: runtime_logs
            .into_iter()
            .map(|entry| DiagnosticsLogEntry {
                timestamp_unix_ms: entry.timestamp_unix_ms,
                level: entry.level,
                code: entry.code,
                message: redact_diagnostics_text(&entry.message),
            })
            .collect(),
        readiness,
        redacted_config_json,
        issue_codes,
    }
}

pub fn format_diagnostics_summary(package: &DiagnosticsPackage) -> String {
    let desktop_issues = package
        .desktop
        .probe
        .as_ref()
        .map(|probe| probe.issue_codes.join(", "))
        .filter(|issues| !issues.is_empty())
        .unwrap_or_else(|| "none".to_owned());
    let summary = format!(
        "CC Desktop Switch diagnostics\nversion: {}\ntarget: {}\nconfigPath: {}\nproviders: {}\nactiveProvider: {}\ngateway: {} running={}\nreadiness: providerConfigured={} desktopReadbackPassed={} providerSmokePassed={} gatewaySmokePassed={}\nissues: {}\ndesktopIssues: {}\nruntimeLogs: {}",
        package.app.app_version,
        package.app.target_version,
        package.config.config_path,
        package.config.provider_count,
        package
            .config
            .active_provider_id
            .as_deref()
            .unwrap_or("none"),
        package.gateway.base_url,
        package.gateway.running,
        package.readiness.provider_configured,
        package.readiness.desktop_readback_passed,
        package.readiness.provider_smoke_passed,
        package.readiness.gateway_smoke_passed,
        package.issue_codes.join(", "),
        desktop_issues,
        package.runtime_logs.len(),
    );
    redact_diagnostics_text(&summary)
}

pub fn build_github_issue_draft(package: &DiagnosticsPackage) -> DiagnosticsIssueDraft {
    let summary = format_diagnostics_summary(package);
    let primary_issue = package
        .issue_codes
        .first()
        .cloned()
        .unwrap_or_else(|| "diagnostics.unknown".to_owned());
    let title = format!("Diagnostics report: {primary_issue}");
    let body = redact_diagnostics_text(&format!(
        "## Summary\n\n```text\n{summary}\n```\n\n## Issue codes\n\n{}\n\n## Notes\n\nAttach the exported diagnostics package if requested. Do not paste API keys or tokens.",
        package
            .issue_codes
            .iter()
            .map(|code| format!("- `{code}`"))
            .collect::<Vec<_>>()
            .join("\n")
    ));
    let url = format!(
        "https://github.com/lonr-6/cc-desktop-switch/issues/new?title={}&body={}",
        percent_encode(&title),
        percent_encode(&body)
    );

    DiagnosticsIssueDraft { title, body, url }
}

pub fn redact_diagnostics_text(input: &str) -> String {
    let mut output = input.to_owned();

    output = regex::Regex::new(r"(?im)^(authorization:\s*)(bearer|basic)\s+[^\r\n]+")
        .expect("authorization redaction regex should compile")
        .replace_all(&output, "${1}${2} [REDACTED:authorization]")
        .into_owned();
    output = regex::Regex::new(r"(?im)^(cookie:\s*)[^\r\n]+")
        .expect("cookie redaction regex should compile")
        .replace_all(&output, "${1}[REDACTED:cookie]")
        .into_owned();
    output = regex::Regex::new(
        r"(?im)^([^:\r\n]*(?:api[-_]?key|gateway[-_]?key|token|secret|auth|cookie)[^:\r\n]*:\s*)[^\r\n]+",
    )
    .expect("secret header redaction regex should compile")
    .replace_all(&output, "${1}[REDACTED:secret-header]")
    .into_owned();
    output = regex::Regex::new(r"(?im)^(authorization:\s*)\[REDACTED:secret-header\]")
        .expect("authorization marker restore regex should compile")
        .replace_all(&output, "${1}Bearer [REDACTED:authorization]")
        .into_owned();
    output = regex::Regex::new(r"(?im)^(cookie:\s*)\[REDACTED:secret-header\]")
        .expect("cookie marker restore regex should compile")
        .replace_all(&output, "${1}[REDACTED:cookie]")
        .into_owned();
    output = regex::Regex::new(
        r#"(?i)((?:api[-_]?key|gateway[-_]?key|token|secret|authorization|cookie)"?\s*[=:]\s*"?)[^",\s&}]+"#,
    )
    .expect("key value redaction regex should compile")
    .replace_all(&output, "${1}[REDACTED:key]")
    .into_owned();
    output = regex::Regex::new(r"(?i)(https?://)[^/@\s]+@")
        .expect("url userinfo redaction regex should compile")
        .replace_all(&output, "${1}[REDACTED:userinfo]@")
        .into_owned();
    output = regex::Regex::new(
        r"(?i)([?&][^=&\s]*(?:api[-_]?key|gateway[-_]?key|token|secret|auth|cookie)[^=&\s]*=)[^&\s]+",
    )
    .expect("query secret redaction regex should compile")
    .replace_all(&output, "${1}[REDACTED:query]")
    .into_owned();
    output =
        regex::Regex::new(r"\b(?:sk|ak|pk|ccds)_[A-Za-z0-9._-]{6,}\b|\bsk-[A-Za-z0-9._-]{6,}\b")
            .expect("token redaction regex should compile")
            .replace_all(&output, "[REDACTED:key]")
            .into_owned();
    output = regex::Regex::new(r"(?im)^(authorization:\s*).+")
        .expect("authorization final marker regex should compile")
        .replace_all(&output, "${1}Bearer [REDACTED:authorization]")
        .into_owned();
    output = regex::Regex::new(r"(?im)^(cookie:\s*).+")
        .expect("cookie final marker regex should compile")
        .replace_all(&output, "${1}[REDACTED:cookie]")
        .into_owned();

    output
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn percent_encode(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char)
            }
            _ => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ConfigProvider};
    use crate::provider::ApiFormat;

    #[test]
    fn readiness_uses_configured_gateway_port() {
        let snapshot = readiness_snapshot(
            None,
            crate::gateway::planned_gateway_health_for_port(18088),
            None,
        );

        assert_eq!(snapshot.gateway.base_url, "http://127.0.0.1:18088");
        assert!(snapshot
            .issue_codes
            .contains(&"gateway.not_running".to_owned()));
        assert!(snapshot
            .issue_codes
            .contains(&"diagnostics.false_green_readiness".to_owned()));
    }

    #[test]
    fn readiness_surfaces_gateway_startup_issue_code() {
        let snapshot = readiness_snapshot(
            None,
            crate::gateway::planned_gateway_health_for_port(18080),
            Some("gateway.port_in_use"),
        );

        assert!(snapshot
            .issue_codes
            .contains(&"gateway.port_in_use".to_owned()));
        assert!(!snapshot
            .issue_codes
            .contains(&"gateway.not_running".to_owned()));
    }

    #[test]
    fn provider_static_smoke_requires_provider_url_and_key() {
        assert_eq!(
            provider_static_smoke(None).issue_code.as_deref(),
            Some("provider.not_configured")
        );

        let missing_key = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            api_format: ApiFormat::Anthropic,
            api_key: String::new(),
        };
        assert_eq!(
            provider_static_smoke(Some(&missing_key))
                .issue_code
                .as_deref(),
            Some("provider.api_key_missing")
        );

        let configured = Provider {
            api_key: "sk-secret".to_owned(),
            ..missing_key
        };
        assert!(provider_static_smoke(Some(&configured)).passed);
    }

    #[test]
    fn redacts_keys_headers_cookies_and_url_tokens() {
        let input = r#"
apiKey: sk-live-provider-secret
gatewayApiKey=ccds_gateway_secret
Authorization: Bearer sk-auth-secret
Cookie: session=secret-cookie
X-Custom-Token: token-secret
https://user:pass@example.com/path?api_key=query-secret&ok=1
upstream body: {"error":"sk-upstream-secret"}
"#;

        let redacted = redact_diagnostics_text(input);

        for secret in [
            "sk-live-provider-secret",
            "ccds_gateway_secret",
            "sk-auth-secret",
            "secret-cookie",
            "token-secret",
            "user:pass",
            "query-secret",
            "sk-upstream-secret",
        ] {
            assert!(
                !redacted.contains(secret),
                "secret leaked after redaction: {secret}"
            );
        }
        assert!(redacted.contains("[REDACTED:key]"));
        assert!(redacted.contains("[REDACTED:authorization]"));
        assert!(redacted.contains("[REDACTED:cookie]"));
        assert!(redacted.contains("[REDACTED:query]"));
    }

    #[test]
    fn diagnostics_package_redacts_config_secrets_and_preserves_issue_codes() {
        let provider = Provider {
            provider_id: "provider-deepseek".to_owned(),
            display_name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            api_format: ApiFormat::Anthropic,
            api_key: "sk-provider-secret".to_owned(),
        };
        let mut config = AppConfig::empty();
        config.gateway_api_key = Some("ccds_gateway_secret".to_owned());
        config.upsert_provider(ConfigProvider::from_provider(provider, 0));
        let gateway = crate::gateway::planned_gateway_health_for_port(18080);

        let package = build_diagnostics_package(
            Path::new("C:/Users/example/.cc-desktop-switch/config.json"),
            &config,
            gateway,
            Some("gateway.port_in_use"),
            None,
            Some("Authorization: Bearer sk-desktop-secret".to_owned()),
            Vec::new(),
        );
        let serialized = serde_json::to_string(&package).unwrap();

        for secret in [
            "sk-provider-secret",
            "ccds_gateway_secret",
            "sk-desktop-secret",
        ] {
            assert!(
                !serialized.contains(secret),
                "secret leaked in diagnostics package: {secret}"
            );
        }
        assert!(serialized.contains("[REDACTED:key]"));
        assert!(serialized.contains("[REDACTED:authorization]"));
        assert!(package
            .issue_codes
            .contains(&"diagnostics.false_green_readiness".to_owned()));
        assert!(package
            .issue_codes
            .contains(&"gateway.port_in_use".to_owned()));
        assert!(package
            .issue_codes
            .contains(&"desktop.config_probe_failed".to_owned()));
        assert!(package.config.gateway_api_key_present);
        assert!(package.config.providers[0].has_api_key);
    }

    #[test]
    fn diagnostics_summary_names_false_green_layers_without_secrets() {
        let config = AppConfig::empty();
        let package = build_diagnostics_package(
            Path::new("D:/ccds/config.json?api_key=sk-path-secret"),
            &config,
            crate::gateway::planned_gateway_health_for_port(18088),
            None,
            None,
            None,
            Vec::new(),
        );

        let summary = format_diagnostics_summary(&package);

        assert!(summary.contains("CC Desktop Switch diagnostics"));
        assert!(summary.contains("diagnostics.false_green_readiness"));
        assert!(summary.contains("providerConfigured=false"));
        assert!(summary.contains("gatewaySmokePassed=false"));
        assert!(!summary.contains("sk-path-secret"));
        assert!(summary.contains("[REDACTED:query]"));
    }

    #[test]
    fn diagnostics_package_redacts_runtime_logs() {
        let config = AppConfig::empty();
        let package = build_diagnostics_package(
            Path::new("D:/ccds/config.json"),
            &config,
            crate::gateway::planned_gateway_health_for_port(18080),
            Some("gateway.start_failed"),
            None,
            None,
            vec![DiagnosticsLogEntry {
                timestamp_unix_ms: 123,
                level: "error".to_owned(),
                code: "gateway.start_failed".to_owned(),
                message: "failed with sk-runtime-secret".to_owned(),
            }],
        );
        let serialized = serde_json::to_string(&package).unwrap();
        let summary = format_diagnostics_summary(&package);

        assert_eq!(package.runtime_logs.len(), 1);
        assert!(!serialized.contains("sk-runtime-secret"));
        assert!(serialized.contains("[REDACTED:key]"));
        assert!(summary.contains("runtimeLogs: 1"));
    }

    #[test]
    fn github_issue_draft_is_redacted_and_url_encoded() {
        let mut config = AppConfig::empty();
        config.gateway_api_key = Some("ccds_gateway_secret".to_owned());
        let package = build_diagnostics_package(
            Path::new("D:/ccds/config.json"),
            &config,
            crate::gateway::planned_gateway_health_for_port(18080),
            Some("gateway.port_in_use"),
            None,
            None,
            Vec::new(),
        );

        let draft = build_github_issue_draft(&package);

        assert!(draft.title.starts_with("Diagnostics report:"));
        assert!(draft
            .url
            .starts_with("https://github.com/lonr-6/cc-desktop-switch/issues/new?"));
        assert!(draft.url.contains("title=Diagnostics%20report"));
        assert!(draft.url.contains("body="));
        assert!(!draft.body.contains("ccds_gateway_secret"));
        assert!(!draft.url.contains("ccds_gateway_secret"));
    }
}
