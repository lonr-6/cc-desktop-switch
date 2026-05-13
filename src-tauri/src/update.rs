use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::release_gate::{validate_update_bundle, validate_update_metadata, ReleaseGateIssue};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

const DEFAULT_RELEASE_PUBLIC_KEY_FILE: &str = "CC-Desktop-Switch-release-public.pem";
const PINNED_RELEASE_PUBLIC_KEY_SHA256: &str =
    "3649e9dffa2dd4929a954aa33aca5e3b74b6c9e71eb14e0bdee94df64a41b8af";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub available: bool,
    pub platform: String,
    pub asset: Option<UpdateAssetInfo>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssetInfo {
    pub name: String,
    pub url: String,
    pub sha256: Option<String>,
    pub signature: Option<String>,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDownloadResult {
    pub check: UpdateCheckResult,
    pub asset_path: String,
    pub staging_dir: String,
    pub bytes: u64,
    pub sha256_verified: bool,
    pub signature_verified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstallResult {
    pub launched: bool,
    pub installer_path: String,
    pub installer_type: String,
    pub launch_method: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("update.url_invalid: {0}")]
    UrlInvalid(String),
    #[error("update.request_failed: {0}")]
    RequestFailed(String),
    #[error("update.manifest_invalid: {0}")]
    ManifestInvalid(String),
    #[error("update.platform_unsupported: {0}")]
    PlatformUnsupported(String),
    #[error("update.asset_missing: {0}")]
    AssetMissing(String),
    #[error("update.download_failed: {0}")]
    DownloadFailed(String),
    #[error("update.verify_failed: {0}")]
    VerifyFailed(String),
    #[error("update.install_failed: {0}")]
    InstallFailed(String),
    #[error("update.path_invalid: {0}")]
    PathInvalid(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateManifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    platforms: std::collections::BTreeMap<String, UpdatePlatformManifest>,
    #[serde(default)]
    signature: Option<UpdateSignatureManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UpdatePlatformManifest {
    #[serde(default)]
    assets: Vec<UpdateAssetManifest>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UpdateAssetManifest {
    name: String,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    signature: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct UpdateSignatureManifest {
    #[serde(default)]
    public_key: Option<String>,
}

pub async fn check_update(
    update_url: &str,
    current_version: &str,
) -> Result<UpdateCheckResult, UpdateError> {
    let latest_url = parse_update_url(update_url)?;
    let latest_json = download_bytes(latest_url.clone()).await?;
    let manifest: UpdateManifest = serde_json::from_slice(&latest_json)
        .map_err(|error| UpdateError::ManifestInvalid(error.to_string()))?;
    let temp_staging_dir = std::env::temp_dir().join(format!(
        "ccds-update-check-{}",
        unique_update_staging_dir_name(&manifest.version)
    ));
    fs::create_dir_all(&temp_staging_dir)
        .map_err(|error| UpdateError::DownloadFailed(error.to_string()))?;
    let metadata_result =
        stage_and_verify_update_metadata(&latest_url, &latest_json, &manifest, &temp_staging_dir)
            .await;
    let cleanup_result = fs::remove_dir_all(&temp_staging_dir);
    metadata_result?;
    if let Err(error) = cleanup_result {
        return Err(UpdateError::DownloadFailed(error.to_string()));
    }
    check_from_manifest(&manifest, current_version)
}

pub async fn download_update(
    update_url: &str,
    current_version: &str,
    staging_root: &Path,
) -> Result<UpdateDownloadResult, UpdateError> {
    let latest_url = parse_update_url(update_url)?;
    let latest_json = download_bytes(latest_url.clone()).await?;
    let manifest: UpdateManifest = serde_json::from_slice(&latest_json)
        .map_err(|error| UpdateError::ManifestInvalid(error.to_string()))?;
    let check = check_from_manifest(&manifest, current_version)?;
    let asset = check.asset.clone().ok_or_else(|| {
        UpdateError::AssetMissing("no installer asset for current platform".to_owned())
    })?;

    fs::create_dir_all(staging_root)
        .map_err(|error| UpdateError::DownloadFailed(error.to_string()))?;
    let temp_staging_dir = staging_root.join(unique_update_staging_dir_name(&manifest.version));
    fs::create_dir_all(&temp_staging_dir)
        .map_err(|error| UpdateError::DownloadFailed(error.to_string()))?;

    let prepared = prepare_verified_update_download(
        &latest_url,
        &latest_json,
        &manifest,
        check,
        asset,
        &temp_staging_dir,
    )
    .await;
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = fs::remove_dir_all(&temp_staging_dir);
            return Err(error);
        }
    };

    let final_staging_dir = staging_root.join(sanitize_file_stem(&manifest.version));
    if let Err(error) = replace_staging_dir(&temp_staging_dir, &final_staging_dir) {
        let _ = fs::remove_dir_all(&temp_staging_dir);
        return Err(error);
    }

    let asset_path = final_staging_dir.join(&prepared.asset_name);
    Ok(UpdateDownloadResult {
        check: prepared.check,
        asset_path: asset_path.display().to_string(),
        staging_dir: final_staging_dir.display().to_string(),
        bytes: prepared.bytes,
        sha256_verified: true,
        signature_verified: true,
    })
}

struct PreparedUpdateDownload {
    check: UpdateCheckResult,
    asset_name: String,
    bytes: u64,
}

async fn prepare_verified_update_download(
    latest_url: &Url,
    latest_json: &[u8],
    manifest: &UpdateManifest,
    check: UpdateCheckResult,
    asset: UpdateAssetInfo,
    staging_dir: &Path,
) -> Result<PreparedUpdateDownload, UpdateError> {
    stage_and_verify_update_metadata(latest_url, latest_json, manifest, staging_dir).await?;

    let asset_name = safe_manifest_file_name(&asset.name, "asset")?;
    let asset_url = parse_update_url(&asset.url)?;
    let asset_path = staging_dir.join(&asset_name);
    let asset_bytes = download_bytes(asset_url.clone()).await?;
    write_file(&asset_path, &asset_bytes)?;
    download_to_file(
        sibling_url(&asset_url, &format!("{}.sha256", asset_name))?,
        &staging_dir.join(format!("{}.sha256", asset_name)),
    )
    .await?;
    let signature_name = asset
        .signature
        .as_deref()
        .filter(|signature| !signature.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}.sig", asset_name));
    let signature_name = safe_manifest_file_name(&signature_name, "asset signature")?;
    download_to_file(
        sibling_url(&asset_url, &signature_name)?,
        &staging_dir.join(&signature_name),
    )
    .await?;

    if let Some(expected) = asset.sha256.as_deref() {
        let actual = sha256_hex(&asset_bytes);
        if expected.trim().to_ascii_lowercase() != actual {
            return Err(UpdateError::VerifyFailed(format!(
                "update.asset_sha256_mismatch: expected {}, got {actual}",
                expected.trim()
            )));
        }
    }

    let report = validate_update_bundle(staging_dir, &asset_name);
    if !report.passed {
        return Err(UpdateError::VerifyFailed(format_issues(&report.issues)));
    }

    Ok(PreparedUpdateDownload {
        check,
        asset_name,
        bytes: asset_bytes.len() as u64,
    })
}

async fn stage_and_verify_update_metadata(
    latest_url: &Url,
    latest_json: &[u8],
    manifest: &UpdateManifest,
    staging_dir: &Path,
) -> Result<(), UpdateError> {
    write_file(&staging_dir.join("latest.json"), latest_json)?;
    download_to_file(
        sibling_url(latest_url, "latest.json.sha256")?,
        &staging_dir.join("latest.json.sha256"),
    )
    .await?;
    download_to_file(
        sibling_url(latest_url, "latest.json.sig")?,
        &staging_dir.join("latest.json.sig"),
    )
    .await?;

    let public_key = safe_manifest_file_name(manifest_public_key_file(manifest), "public key")?;
    download_to_file(
        sibling_url(latest_url, &public_key)?,
        &staging_dir.join(&public_key),
    )
    .await?;
    verify_release_public_key_trust(&staging_dir.join(public_key))?;

    let metadata_report = validate_update_metadata(staging_dir);
    if !metadata_report.passed {
        return Err(UpdateError::VerifyFailed(format_issues(
            &metadata_report.issues,
        )));
    }
    Ok(())
}

pub fn install_update(
    installer_path: &Path,
    staging_root: &Path,
) -> Result<UpdateInstallResult, UpdateError> {
    let verified = verified_update_installer(installer_path, staging_root)?;
    let mut command = install_command(&verified.installer_path);
    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|error| UpdateError::InstallFailed(error.to_string()))?;
    Ok(UpdateInstallResult {
        launched: true,
        installer_path: verified.installer_path.display().to_string(),
        installer_type: verified.installer_type,
        launch_method: verified.launch_method,
    })
}

#[derive(Debug)]
struct VerifiedUpdateInstaller {
    installer_path: PathBuf,
    installer_type: String,
    launch_method: String,
}

fn verified_update_installer(
    installer_path: &Path,
    staging_root: &Path,
) -> Result<VerifiedUpdateInstaller, UpdateError> {
    let asset_name = installer_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| UpdateError::PathInvalid("installer path has no file name".to_owned()))
        .and_then(|name| safe_manifest_file_name(name, "installer"))?;
    let installer_path = installer_path
        .canonicalize()
        .map_err(|error| UpdateError::InstallFailed(error.to_string()))?;
    if !installer_path.is_file() {
        return Err(UpdateError::InstallFailed(format!(
            "installer does not exist: {}",
            installer_path.display()
        )));
    }
    let staging_root = staging_root
        .canonicalize()
        .map_err(|error| UpdateError::InstallFailed(error.to_string()))?;
    let staging_dir = installer_path
        .parent()
        .ok_or_else(|| UpdateError::PathInvalid("installer path has no parent".to_owned()))?;
    if staging_dir.parent() != Some(staging_root.as_path()) {
        return Err(UpdateError::InstallFailed(format!(
            "installer is not in an update staging directory: {}",
            installer_path.display()
        )));
    }
    let report = validate_update_bundle(staging_dir, &asset_name);
    if !report.passed {
        return Err(UpdateError::VerifyFailed(format_issues(&report.issues)));
    }
    verify_release_public_key_trust(&staged_public_key_path(staging_dir)?)?;
    Ok(VerifiedUpdateInstaller {
        installer_type: installer_type(&installer_path),
        launch_method: launch_method(&installer_path),
        installer_path,
    })
}

#[cfg(target_os = "windows")]
fn install_command(installer_path: &Path) -> Command {
    if has_extension(installer_path, "msi") {
        let mut command = Command::new("msiexec.exe");
        command.arg("/i").arg(installer_path);
        command
    } else {
        Command::new(installer_path)
    }
}

fn staged_public_key_path(staging_dir: &Path) -> Result<PathBuf, UpdateError> {
    let latest_json = fs::read(staging_dir.join("latest.json")).map_err(|error| {
        UpdateError::VerifyFailed(format!("update.latest_json_missing: {error}"))
    })?;
    let manifest: UpdateManifest = serde_json::from_slice(&latest_json)
        .map_err(|error| UpdateError::ManifestInvalid(error.to_string()))?;
    let public_key = safe_manifest_file_name(manifest_public_key_file(&manifest), "public key")?;
    Ok(staging_dir.join(public_key))
}

fn verify_release_public_key_trust(public_key_path: &Path) -> Result<(), UpdateError> {
    let expected = release_public_key_trust_anchor();
    let expected = expected.to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(UpdateError::VerifyFailed(
            "update.public_key_trust_anchor_invalid: test override must be a 64-character sha256 hex digest"
                .to_owned(),
        ));
    }
    let bytes = fs::read(public_key_path).map_err(|error| {
        UpdateError::VerifyFailed(format!("update.public_key_missing: {error}"))
    })?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(UpdateError::VerifyFailed(format!(
            "update.public_key_trust_anchor_mismatch: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

#[cfg(test)]
fn release_public_key_trust_anchor() -> &'static str {
    option_env!("CCDS_RELEASE_PUBLIC_KEY_SHA256")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(PINNED_RELEASE_PUBLIC_KEY_SHA256)
}

#[cfg(not(test))]
fn release_public_key_trust_anchor() -> &'static str {
    PINNED_RELEASE_PUBLIC_KEY_SHA256
}

#[cfg(target_os = "macos")]
fn install_command(installer_path: &Path) -> Command {
    let mut command = Command::new("open");
    command.arg(installer_path);
    command
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn install_command(installer_path: &Path) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(installer_path);
    command
}

#[cfg(target_os = "windows")]
fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn installer_type(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn launch_method(path: &Path) -> String {
    #[cfg(target_os = "windows")]
    {
        if has_extension(path, "msi") {
            "msiexec".to_owned()
        } else {
            "direct".to_owned()
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = path;
        "open".to_owned()
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = path;
        "xdg-open".to_owned()
    }
}

fn check_from_manifest(
    manifest: &UpdateManifest,
    current_version: &str,
) -> Result<UpdateCheckResult, UpdateError> {
    let platform = current_platform_key()?;
    let platform_manifest = manifest
        .platforms
        .get(&platform)
        .ok_or_else(|| UpdateError::PlatformUnsupported(platform.clone()))?;
    let asset =
        select_installer_asset(&platform, &platform_manifest.assets).map(|asset| UpdateAssetInfo {
            name: asset.name.clone(),
            url: asset.url.clone(),
            sha256: asset.sha256.clone(),
            signature: asset.signature.clone(),
            size: asset.size,
        });
    Ok(UpdateCheckResult {
        current_version: current_version.to_owned(),
        latest_version: manifest.version.clone(),
        available: normalize_version(&manifest.version) != normalize_version(current_version),
        platform,
        asset,
        notes: manifest.notes.clone(),
    })
}

fn current_platform_key() -> Result<String, UpdateError> {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Ok("windows-x64".to_owned());
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Ok("macos-arm64".to_owned());
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Ok("macos-x64".to_owned());
    }
    #[allow(unreachable_code)]
    Err(UpdateError::PlatformUnsupported(format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )))
}

fn select_installer_asset<'a>(
    platform: &str,
    assets: &'a [UpdateAssetManifest],
) -> Option<&'a UpdateAssetManifest> {
    if platform == "windows-x64" {
        assets
            .iter()
            .find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.ends_with(".exe") && name.contains("setup")
            })
            .or_else(|| {
                assets
                    .iter()
                    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".exe"))
            })
    } else {
        assets
            .iter()
            .find(|asset| asset.name.to_ascii_lowercase().ends_with(".pkg"))
            .or_else(|| {
                assets
                    .iter()
                    .find(|asset| asset.name.to_ascii_lowercase().ends_with(".dmg"))
            })
    }
}

fn parse_update_url(url: &str) -> Result<Url, UpdateError> {
    Url::parse(url.trim()).map_err(|error| UpdateError::UrlInvalid(error.to_string()))
}

fn sibling_url(base: &Url, file_name: &str) -> Result<Url, UpdateError> {
    let file_name = safe_manifest_file_name(file_name, "sibling file")?;
    let mut url = base.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| UpdateError::UrlInvalid("cannot build sibling URL".to_owned()))?;
        segments.pop();
        segments.push(&file_name);
    }
    Ok(url)
}

fn safe_manifest_file_name(value: &str, label: &str) -> Result<String, UpdateError> {
    let trimmed = value.trim();
    let is_plain = !trimmed.is_empty()
        && !trimmed.contains('/')
        && !trimmed.contains('\\')
        && !trimmed.contains(':')
        && !Path::new(trimmed).is_absolute()
        && Path::new(trimmed)
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if is_plain {
        Ok(trimmed.to_owned())
    } else {
        Err(UpdateError::PathInvalid(format!(
            "{label} must be a plain file name"
        )))
    }
}

async fn download_bytes(url: Url) -> Result<Vec<u8>, UpdateError> {
    let response = reqwest::get(url.clone())
        .await
        .map_err(|error| UpdateError::RequestFailed(error.to_string()))?
        .error_for_status()
        .map_err(|error| UpdateError::RequestFailed(error.to_string()))?;
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| UpdateError::RequestFailed(error.to_string()))
}

async fn download_to_file(url: Url, path: &Path) -> Result<(), UpdateError> {
    let bytes = download_bytes(url).await?;
    write_file(path, &bytes)
}

fn replace_staging_dir(
    temp_staging_dir: &Path,
    final_staging_dir: &Path,
) -> Result<(), UpdateError> {
    if !final_staging_dir.exists() {
        return fs::rename(temp_staging_dir, final_staging_dir)
            .map_err(|error| UpdateError::DownloadFailed(error.to_string()));
    }
    let backup_staging_dir = rollback_staging_dir(final_staging_dir);
    fs::rename(final_staging_dir, &backup_staging_dir)
        .map_err(|error| UpdateError::DownloadFailed(error.to_string()))?;
    match fs::rename(temp_staging_dir, final_staging_dir) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&backup_staging_dir);
            Ok(())
        }
        Err(error) => {
            if !final_staging_dir.exists() && backup_staging_dir.exists() {
                let _ = fs::rename(&backup_staging_dir, final_staging_dir);
            }
            Err(UpdateError::DownloadFailed(error.to_string()))
        }
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    fs::write(path, bytes).map_err(|error| UpdateError::DownloadFailed(error.to_string()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn normalize_version(version: &str) -> String {
    version.trim().trim_start_matches('v').to_ascii_lowercase()
}

fn sanitize_file_stem(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches(['.', '-', '_']).is_empty() {
        "unknown-version".to_owned()
    } else {
        sanitized
    }
}

fn unique_update_staging_dir_name(version: &str) -> String {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!(".download-{}-{nonce}", sanitize_file_stem(version))
}

fn rollback_staging_dir(final_staging_dir: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let name = final_staging_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("update-staging");
    final_staging_dir.with_file_name(format!(".rollback-{name}-{nonce}"))
}

fn manifest_public_key_file(manifest: &UpdateManifest) -> &str {
    manifest
        .signature
        .as_ref()
        .and_then(|signature| signature.public_key.as_deref())
        .filter(|public_key| !public_key.trim().is_empty())
        .unwrap_or(DEFAULT_RELEASE_PUBLIC_KEY_FILE)
}

fn format_issues(issues: &[ReleaseGateIssue]) -> String {
    issues
        .iter()
        .map(|issue| format!("{}: {}", issue.code, issue.detail))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::net::TcpListener;

    fn temp_update_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ccds-update-{name}-{unique}"));
        fs::create_dir_all(&path).expect("update temp dir should be created");
        path
    }

    fn manifest(version: &str, asset_name: &str) -> UpdateManifest {
        let asset = UpdateAssetManifest {
            name: asset_name.to_owned(),
            url: format!("https://example.com/download/{asset_name}"),
            sha256: Some("a".repeat(64)),
            signature: Some(format!("{asset_name}.sig")),
            size: Some(12),
        };
        UpdateManifest {
            version: version.to_owned(),
            notes: Some("notes".to_owned()),
            platforms: [(
                current_platform_key().unwrap(),
                UpdatePlatformManifest {
                    assets: vec![asset],
                },
            )]
            .into_iter()
            .collect(),
            signature: Some(UpdateSignatureManifest {
                public_key: Some("CC-Desktop-Switch-release-public.pem".to_owned()),
            }),
        }
    }

    fn current_platform_asset_name(version: &str) -> String {
        if current_platform_key().unwrap() == "windows-x64" {
            format!("CC-Desktop-Switch-v{version}-Windows-Setup.exe")
        } else if current_platform_key().unwrap() == "macos-arm64" {
            format!("CC-Desktop-Switch-v{version}-macOS-arm64.pkg")
        } else {
            format!("CC-Desktop-Switch-v{version}-macOS-x64.pkg")
        }
    }

    async fn spawn_latest_json_server(body: String) -> String {
        async fn latest(State(body): State<Arc<String>>) -> impl IntoResponse {
            body.as_ref().clone()
        }

        let app = Router::new()
            .route("/latest.json", get(latest))
            .fallback(|| async { StatusCode::NOT_FOUND })
            .with_state(Arc::new(body));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test update server should bind");
        let addr = listener
            .local_addr()
            .expect("test update server should expose addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("test update server should serve");
        });
        format!("http://{addr}/latest.json")
    }

    #[test]
    fn check_from_manifest_selects_current_platform_installer() {
        let asset_name = if current_platform_key().unwrap() == "windows-x64" {
            "CC-Desktop-Switch-v1.2.0-Windows-Setup.exe"
        } else {
            "CC-Desktop-Switch-v1.2.0-macOS-arm64.pkg"
        };
        let result = check_from_manifest(&manifest("v1.2.0", asset_name), "1.1.0").unwrap();

        assert!(result.available);
        assert_eq!(result.asset.unwrap().name, asset_name);
    }

    #[test]
    fn check_from_manifest_marks_same_version_unavailable() {
        let asset_name = if current_platform_key().unwrap() == "windows-x64" {
            "CC-Desktop-Switch-v1.1.0-Windows-Setup.exe"
        } else {
            "CC-Desktop-Switch-v1.1.0-macOS-arm64.pkg"
        };
        let result = check_from_manifest(&manifest("v1.1.0", asset_name), "1.1.0").unwrap();

        assert!(!result.available);
    }

    #[tokio::test]
    async fn check_update_rejects_unsigned_metadata_preview() {
        let asset_name = current_platform_asset_name("1.2.0");
        let latest_json = serde_json::to_string(&manifest("1.2.0", &asset_name))
            .expect("manifest fixture should serialize");
        let latest_url = spawn_latest_json_server(latest_json).await;

        let error = check_update(&latest_url, "1.1.0").await.unwrap_err();

        assert!(error.to_string().contains("update.request_failed"));
    }

    #[test]
    fn manifest_public_key_file_defaults_when_missing() {
        let asset_name = if current_platform_key().unwrap() == "windows-x64" {
            "CC-Desktop-Switch-v1.2.0-Windows-Setup.exe"
        } else {
            "CC-Desktop-Switch-v1.2.0-macOS-arm64.pkg"
        };
        let mut manifest = manifest("v1.2.0", asset_name);
        manifest.signature = None;

        assert_eq!(
            manifest_public_key_file(&manifest),
            DEFAULT_RELEASE_PUBLIC_KEY_FILE
        );
    }

    #[test]
    fn update_manifest_file_names_must_be_plain_file_names() {
        assert!(safe_manifest_file_name("CC-Desktop-Switch-Windows-Setup.exe", "asset").is_ok());
        assert!(safe_manifest_file_name("../evil.exe", "asset").is_err());
        assert!(safe_manifest_file_name("nested/evil.exe", "asset").is_err());
        assert!(safe_manifest_file_name(r"nested\evil.exe", "asset").is_err());
        assert!(safe_manifest_file_name("C:evil.exe", "asset").is_err());
    }

    #[test]
    fn install_update_rejects_arbitrary_file_outside_update_staging() {
        let staging_root = temp_update_dir("staging-root");
        let outside = temp_update_dir("outside").join("CC-Desktop-Switch-Windows-Setup.exe");
        fs::write(&outside, "fixture installer").expect("outside installer should be written");

        let error = verified_update_installer(&outside, &staging_root).unwrap_err();

        fs::remove_dir_all(&staging_root).expect("staging root should be removed");
        fs::remove_dir_all(outside.parent().unwrap()).expect("outside dir should be removed");
        assert!(error.to_string().contains("update.install_failed"));
    }

    #[test]
    fn runtime_update_rejects_public_key_not_matching_trust_anchor() {
        let dir = temp_update_dir("trust-anchor");
        let public_key = dir.join(DEFAULT_RELEASE_PUBLIC_KEY_FILE);
        fs::write(&public_key, "fixture public key").expect("public key fixture should be written");

        let error = verify_release_public_key_trust(&public_key).unwrap_err();

        fs::remove_dir_all(&dir).expect("trust anchor temp dir should be removed");
        assert!(error.to_string().contains("update.public_key_trust_anchor"));
    }

    #[tokio::test]
    async fn download_update_keeps_existing_staging_when_metadata_verification_fails() {
        let staging_root = temp_update_dir("metadata-fails");
        let final_dir = staging_root.join("1.2.0");
        fs::create_dir_all(&final_dir).expect("existing staging dir should be created");
        let marker = final_dir.join("existing-marker.txt");
        fs::write(&marker, "keep me").expect("marker should be written");

        let asset_name = current_platform_asset_name("1.2.0");
        let mut manifest = manifest("1.2.0", &asset_name);
        manifest
            .platforms
            .get_mut(&current_platform_key().unwrap())
            .unwrap()
            .assets[0]
            .url = "http://127.0.0.1:9/download/installer.exe".to_owned();
        let latest_json =
            serde_json::to_string(&manifest).expect("manifest fixture should serialize");
        let latest_url = spawn_latest_json_server(latest_json).await;

        let error = download_update(&latest_url, "1.1.0", &staging_root)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("update.request_failed"));
        assert_eq!(
            fs::read_to_string(&marker).expect("existing marker should remain"),
            "keep me"
        );
        fs::remove_dir_all(&staging_root).expect("metadata fail temp dir should be removed");
    }
}
