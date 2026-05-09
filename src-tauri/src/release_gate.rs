use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

use base64::{engine::general_purpose, Engine as _};
use rsa::pkcs1v15;
use rsa::signature::Verifier;
use rsa::{BigUint, RsaPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseGateInput {
    pub latest_json: Option<String>,
    pub latest_json_sha256_present: bool,
    pub latest_json_signature_present: bool,
    pub public_key_present: bool,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseAsset {
    pub asset_id: String,
    pub file_name: String,
    pub sha256_present: bool,
    pub signature_present: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseGateReport {
    pub passed: bool,
    pub issues: Vec<ReleaseGateIssue>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseGateIssue {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Deserialize)]
struct LatestJsonManifest {
    #[serde(default)]
    platforms: BTreeMap<String, LatestPlatformManifest>,
    signature: Option<LatestSignatureManifest>,
}

#[derive(Debug, Deserialize)]
struct LatestPlatformManifest {
    #[serde(default)]
    assets: Vec<LatestAssetManifest>,
}

#[derive(Debug, Deserialize)]
struct LatestAssetManifest {
    name: String,
    signature: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LatestSignatureManifest {
    public_key: Option<String>,
    algorithm: Option<String>,
}

pub fn validate_release_gate(input: &ReleaseGateInput) -> ReleaseGateReport {
    let mut issues = Vec::new();

    match input.latest_json.as_deref() {
        Some(raw) => {
            if serde_json::from_str::<serde_json::Value>(raw).is_err() {
                issues.push(issue(
                    "release.latest_json_invalid",
                    "latest.json must be valid JSON",
                ));
            }
        }
        None => issues.push(issue(
            "release.latest_json_missing",
            "latest.json is required",
        )),
    }
    if !input.latest_json_sha256_present {
        issues.push(issue(
            "release.latest_json_sha256_missing",
            "latest.json.sha256 is required",
        ));
    }
    if !input.latest_json_signature_present {
        issues.push(issue(
            "release.latest_json_sig_missing",
            "latest.json.sig is required",
        ));
    }
    if !input.public_key_present {
        issues.push(issue(
            "release.public_key_missing",
            "update signature public key is required",
        ));
    }

    let present_assets = input
        .assets
        .iter()
        .map(|asset| asset.asset_id.as_str())
        .collect::<HashSet<_>>();
    for required in required_release_assets() {
        if !present_assets.contains(required) {
            issues.push(issue(
                "release.asset_missing",
                &format!("required asset '{required}' is missing"),
            ));
        }
    }

    for asset in &input.assets {
        if asset.file_name.trim().is_empty() {
            issues.push(issue(
                "release.asset_file_name_missing",
                &format!("asset '{}' has no file name", asset.asset_id),
            ));
        }
        if !asset.sha256_present {
            issues.push(issue(
                "release.asset_sha256_missing",
                &format!("asset '{}' is missing .sha256", asset.asset_id),
            ));
        }
        if !asset.signature_present {
            issues.push(issue(
                "release.asset_sig_missing",
                &format!("asset '{}' is missing .sig", asset.asset_id),
            ));
        }
    }

    normalize_issues(&mut issues);

    ReleaseGateReport {
        passed: issues.is_empty(),
        issues,
    }
}

pub fn validate_release_directory(staging_dir: impl AsRef<Path>) -> ReleaseGateReport {
    let staging_dir = staging_dir.as_ref();
    let latest_path = staging_dir.join("latest.json");
    let latest_sha256_path = staging_dir.join("latest.json.sha256");
    let latest_json = fs::read_to_string(&latest_path).ok();
    let latest_manifest = latest_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<LatestJsonManifest>(raw).ok());

    let mut input = ReleaseGateInput {
        latest_json,
        latest_json_sha256_present: latest_sha256_path.is_file(),
        latest_json_signature_present: staging_dir.join("latest.json.sig").is_file(),
        public_key_present: false,
        assets: Vec::new(),
    };
    let mut issues = Vec::new();

    validate_sha256_sidecar(
        &mut issues,
        &latest_path,
        &latest_sha256_path,
        "release.latest_json_sha256_invalid",
        "release.latest_json_sha256_mismatch",
        "latest.json",
    );

    if let Some(manifest) = latest_manifest {
        let public_key_file = manifest
            .signature
            .as_ref()
            .and_then(|signature| signature.public_key.as_deref())
            .filter(|public_key| !public_key.trim().is_empty())
            .unwrap_or("CC-Desktop-Switch-release-public.pem");
        let public_key_path = staging_dir.join(public_key_file);
        input.public_key_present = public_key_path.is_file();
        let public_key = validate_public_key(
            &mut issues,
            &public_key_path,
            manifest
                .signature
                .as_ref()
                .and_then(|item| item.algorithm.as_deref()),
        );
        validate_signature_sidecar(
            &mut issues,
            &latest_path,
            &staging_dir.join("latest.json.sig"),
            public_key.as_ref(),
            "release.latest_json_sig_invalid",
            "release.latest_json_sig_mismatch",
            "latest.json",
        );

        for platform in manifest.platforms.values() {
            for manifest_asset in &platform.assets {
                let asset_id = asset_id_for_file_name(&manifest_asset.name)
                    .unwrap_or("unknown-release-asset")
                    .to_owned();
                let asset_path = staging_dir.join(&manifest_asset.name);
                let signature_name = manifest_asset
                    .signature
                    .as_deref()
                    .filter(|signature| !signature.trim().is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{}.sig", manifest_asset.name));

                if !asset_path.is_file() {
                    issues.push(issue(
                        "release.latest_json_asset_missing",
                        &format!(
                            "latest.json references missing asset '{}'",
                            manifest_asset.name
                        ),
                    ));
                }

                let sha256_path = staging_dir.join(format!("{}.sha256", manifest_asset.name));
                validate_sha256_sidecar(
                    &mut issues,
                    &asset_path,
                    &sha256_path,
                    "release.asset_sha256_invalid",
                    "release.asset_sha256_mismatch",
                    &format!("asset '{}'", manifest_asset.name),
                );

                let signature_path = staging_dir.join(&signature_name);
                validate_signature_sidecar(
                    &mut issues,
                    &asset_path,
                    &signature_path,
                    public_key.as_ref(),
                    "release.asset_sig_invalid",
                    "release.asset_sig_mismatch",
                    &format!("asset '{}'", manifest_asset.name),
                );

                input.assets.push(ReleaseAsset {
                    asset_id,
                    file_name: manifest_asset.name.clone(),
                    sha256_present: sha256_path.is_file(),
                    signature_present: signature_path.is_file(),
                });
            }
        }
    }

    issues.extend(validate_release_gate(&input).issues);
    normalize_issues(&mut issues);

    ReleaseGateReport {
        passed: issues.is_empty(),
        issues,
    }
}

pub fn required_release_assets() -> &'static [&'static str] {
    &[
        "windows-setup",
        "windows-portable-zip",
        "windows-x64-exe",
        "macos-arm64-pkg",
        "macos-arm64-dmg",
        "macos-x64-pkg",
        "macos-x64-dmg",
    ]
}

fn asset_id_for_file_name(file_name: &str) -> Option<&'static str> {
    if file_name.ends_with("-Windows-Setup.exe") {
        return Some("windows-setup");
    }
    if file_name.ends_with("-Windows-Portable.zip") {
        return Some("windows-portable-zip");
    }
    if file_name.ends_with("-Windows-x64.exe") {
        return Some("windows-x64-exe");
    }
    if file_name.ends_with("-macOS-arm64.pkg") {
        return Some("macos-arm64-pkg");
    }
    if file_name.ends_with("-macOS-arm64.dmg") {
        return Some("macos-arm64-dmg");
    }
    if file_name.ends_with("-macOS-x64.pkg") {
        return Some("macos-x64-pkg");
    }
    if file_name.ends_with("-macOS-x64.dmg") {
        return Some("macos-x64-dmg");
    }
    None
}

fn validate_public_key(
    issues: &mut Vec<ReleaseGateIssue>,
    public_key_path: &Path,
    algorithm: Option<&str>,
) -> Option<RsaPublicKey> {
    if !public_key_path.is_file() {
        return None;
    }
    if let Some(algorithm) = algorithm {
        if algorithm.trim() != "RSA-CSP-BLOB-SHA256" {
            issues.push(issue(
                "release.signature_algorithm_unsupported",
                &format!("unsupported release signature algorithm '{algorithm}'"),
            ));
            return None;
        }
    }

    let raw = match fs::read_to_string(public_key_path) {
        Ok(raw) => raw,
        Err(error) => {
            issues.push(issue(
                "release.public_key_invalid",
                &format!("release public key could not be read: {error}"),
            ));
            return None;
        }
    };
    match parse_csp_public_key_pem(&raw) {
        Ok(key) => Some(key),
        Err(detail) => {
            issues.push(issue(
                "release.public_key_invalid",
                &format!("release public key is invalid: {detail}"),
            ));
            None
        }
    }
}

fn validate_signature_sidecar(
    issues: &mut Vec<ReleaseGateIssue>,
    file_path: &Path,
    signature_path: &Path,
    public_key: Option<&RsaPublicKey>,
    invalid_code: &str,
    mismatch_code: &str,
    label: &str,
) {
    if !file_path.is_file() || !signature_path.is_file() {
        return;
    }
    let Some(public_key) = public_key else {
        return;
    };

    let signature_raw = match fs::read_to_string(signature_path) {
        Ok(raw) => raw,
        Err(error) => {
            issues.push(issue(
                invalid_code,
                &format!("{label} signature could not be read: {error}"),
            ));
            return;
        }
    };
    let signature_bytes = match decode_base64_compact(&signature_raw) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => {
            issues.push(issue(invalid_code, &format!("{label} signature is empty")));
            return;
        }
        Err(detail) => {
            issues.push(issue(
                invalid_code,
                &format!("{label} signature is not valid base64: {detail}"),
            ));
            return;
        }
    };
    let signature = match pkcs1v15::Signature::try_from(signature_bytes.as_slice()) {
        Ok(signature) => signature,
        Err(error) => {
            issues.push(issue(
                invalid_code,
                &format!("{label} signature has invalid length or shape: {error}"),
            ));
            return;
        }
    };
    let bytes = match fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            issues.push(issue(
                mismatch_code,
                &format!("{label} could not be read for signature verification: {error}"),
            ));
            return;
        }
    };
    let verifying_key = pkcs1v15::VerifyingKey::<Sha256>::new(public_key.clone());
    if verifying_key.verify(&bytes, &signature).is_err() {
        issues.push(issue(
            mismatch_code,
            &format!("{label} signature does not match the file bytes"),
        ));
    }
}

fn validate_sha256_sidecar(
    issues: &mut Vec<ReleaseGateIssue>,
    file_path: &Path,
    sha256_path: &Path,
    invalid_code: &str,
    mismatch_code: &str,
    label: &str,
) {
    if !file_path.is_file() || !sha256_path.is_file() {
        return;
    }

    let expected = match fs::read_to_string(sha256_path) {
        Ok(raw) => match parse_sha256_sidecar(&raw) {
            Some(hash) => hash,
            None => {
                issues.push(issue(
                    invalid_code,
                    &format!("{label} sha256 sidecar is not a 64-character hex digest"),
                ));
                return;
            }
        },
        Err(error) => {
            issues.push(issue(
                invalid_code,
                &format!("{label} sha256 sidecar could not be read: {error}"),
            ));
            return;
        }
    };

    let bytes = match fs::read(file_path) {
        Ok(bytes) => bytes,
        Err(error) => {
            issues.push(issue(
                mismatch_code,
                &format!("{label} could not be read for sha256 verification: {error}"),
            ));
            return;
        }
    };
    let actual = sha256_hex(&bytes);
    if expected != actual {
        issues.push(issue(
            mismatch_code,
            &format!("{label} sha256 mismatch: expected {expected}, got {actual}"),
        ));
    }
}

fn parse_csp_public_key_pem(raw: &str) -> Result<RsaPublicKey, String> {
    if !raw.contains("-----BEGIN RSA PUBLIC KEY BLOB-----")
        || !raw.contains("-----END RSA PUBLIC KEY BLOB-----")
    {
        return Err("expected RSA PUBLIC KEY BLOB PEM wrapper".to_owned());
    }
    let bytes = decode_pem_body(raw)?;
    parse_csp_public_key_blob(&bytes)
}

fn parse_csp_public_key_blob(bytes: &[u8]) -> Result<RsaPublicKey, String> {
    const PUBLICKEYBLOB: u8 = 0x06;
    const CUR_BLOB_VERSION: u8 = 0x02;
    const RSA1_MAGIC: u32 = 0x3141_5352;
    const HEADER_LEN: usize = 8;
    const RSA_PUBKEY_LEN: usize = 12;
    const MIN_LEN: usize = HEADER_LEN + RSA_PUBKEY_LEN;

    if bytes.len() < MIN_LEN {
        return Err("CSP public key blob is too short".to_owned());
    }
    if bytes[0] != PUBLICKEYBLOB {
        return Err("CSP blob is not a PUBLICKEYBLOB".to_owned());
    }
    if bytes[1] != CUR_BLOB_VERSION {
        return Err("unsupported CSP blob version".to_owned());
    }

    let magic = read_u32_le(bytes, 8)?;
    if magic != RSA1_MAGIC {
        return Err("CSP blob does not contain an RSA1 public key".to_owned());
    }
    let bit_len = read_u32_le(bytes, 12)? as usize;
    if bit_len == 0 || !bit_len.is_multiple_of(8) {
        return Err("CSP RSA bit length is invalid".to_owned());
    }
    let exponent = read_u32_le(bytes, 16)?;
    if exponent < 3 {
        return Err("CSP RSA public exponent is invalid".to_owned());
    }

    let modulus_len = bit_len / 8;
    let modulus_start = MIN_LEN;
    let modulus_end = modulus_start + modulus_len;
    if bytes.len() < modulus_end {
        return Err("CSP RSA modulus is truncated".to_owned());
    }
    let modulus = BigUint::from_bytes_le(&bytes[modulus_start..modulus_end]);
    let exponent = BigUint::from(exponent);
    RsaPublicKey::new(modulus, exponent).map_err(|error| error.to_string())
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset + 4;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| "CSP blob has a truncated u32 field".to_owned())?;
    Ok(u32::from_le_bytes(
        slice
            .try_into()
            .expect("slice length was already checked as four bytes"),
    ))
}

fn decode_pem_body(raw: &str) -> Result<Vec<u8>, String> {
    let body = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("-----"))
        .collect::<String>();
    decode_base64_compact(&body)
}

fn decode_base64_compact(raw: &str) -> Result<Vec<u8>, String> {
    let compact = raw.split_whitespace().collect::<String>();
    general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|error| error.to_string())
}

fn parse_sha256_sidecar(raw: &str) -> Option<String> {
    let matches = raw
        .split_whitespace()
        .map(|token| token.trim_matches(|ch| ch == '*' || ch == '(' || ch == ')' || ch == '='))
        .filter(|token| token.len() == 64 && token.chars().all(|ch| ch.is_ascii_hexdigit()))
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [hash] => Some(hash.clone()),
        _ => None,
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn normalize_issues(issues: &mut Vec<ReleaseGateIssue>) {
    issues.sort_by(|left, right| {
        left.code
            .cmp(&right.code)
            .then_with(|| left.detail.cmp(&right.detail))
    });
    issues.dedup();
}

fn issue(code: &str, detail: &str) -> ReleaseGateIssue {
    ReleaseGateIssue {
        code: code.to_owned(),
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose;
    use rand::{rngs::StdRng, SeedableRng};
    use rsa::pkcs1v15::SigningKey;
    use rsa::signature::{SignatureEncoding, Signer};
    use rsa::traits::PublicKeyParts;
    use rsa::{RsaPrivateKey, RsaPublicKey};
    use std::path::PathBuf;
    #[cfg(windows)]
    use std::process::Command;
    use std::sync::OnceLock;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn complete_input() -> ReleaseGateInput {
        ReleaseGateInput {
            latest_json: Some(r#"{"version":"v1.1.0-rc1"}"#.to_owned()),
            latest_json_sha256_present: true,
            latest_json_signature_present: true,
            public_key_present: true,
            assets: required_release_assets()
                .iter()
                .map(|asset_id| ReleaseAsset {
                    asset_id: (*asset_id).to_owned(),
                    file_name: format!("{asset_id}.asset"),
                    sha256_present: true,
                    signature_present: true,
                })
                .collect(),
        }
    }

    fn temp_release_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ccds-release-gate-{name}-{unique}"));
        fs::create_dir_all(&path).expect("release temp dir should be created");
        path
    }

    fn write_file_with_sha256(dir: &Path, file_name: &str, contents: &str) {
        let file_path = dir.join(file_name);
        fs::write(&file_path, contents).expect("fixture file should be written");
        write_sha256_sidecar(&file_path);
        write_signature_sidecar(&file_path, fixture_private_key());
    }

    fn write_sha256_sidecar(file_path: &Path) {
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture file name should be utf-8");
        let bytes = fs::read(file_path).expect("fixture file should be readable");
        let sha256 = sha256_hex(&bytes);
        let sha256_path = file_path
            .parent()
            .expect("fixture file should have a parent")
            .join(format!("{file_name}.sha256"));
        fs::write(sha256_path, format!("{sha256}  {file_name}\n"))
            .expect("fixture sha256 should be written");
    }

    fn write_signature_sidecar(file_path: &Path, private_key: &RsaPrivateKey) {
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("fixture file name should be utf-8");
        let bytes = fs::read(file_path).expect("fixture file should be readable");
        let signing_key = SigningKey::<Sha256>::new(private_key.clone());
        let signature = signing_key.sign(&bytes);
        let signature_path = file_path
            .parent()
            .expect("fixture file should have a parent")
            .join(format!("{file_name}.sig"));
        fs::write(
            signature_path,
            general_purpose::STANDARD.encode(signature.to_bytes()),
        )
        .expect("fixture signature should be written");
    }

    fn fixture_private_key() -> &'static RsaPrivateKey {
        static KEY: OnceLock<RsaPrivateKey> = OnceLock::new();
        KEY.get_or_init(|| {
            let mut rng = StdRng::seed_from_u64(42);
            RsaPrivateKey::new(&mut rng, 2048).expect("fixture RSA key should be generated")
        })
    }

    fn write_fixture_public_key(dir: &Path) {
        let public_key = RsaPublicKey::from(fixture_private_key());
        fs::write(
            dir.join("CC-Desktop-Switch-release-public.pem"),
            csp_public_key_pem(&public_key),
        )
        .expect("public key should be written");
    }

    fn csp_public_key_pem(public_key: &RsaPublicKey) -> String {
        let mut blob = Vec::new();
        blob.extend_from_slice(&[0x06, 0x02, 0x00, 0x00]);
        blob.extend_from_slice(&0x0000_2400u32.to_le_bytes());
        blob.extend_from_slice(&0x3141_5352u32.to_le_bytes());
        let modulus = public_key.n().to_bytes_le();
        blob.extend_from_slice(&((modulus.len() * 8) as u32).to_le_bytes());
        let exponent_bytes = public_key.e().to_bytes_le();
        let mut exponent = [0u8; 4];
        let copy_len = exponent_bytes.len().min(4);
        exponent[..copy_len].copy_from_slice(&exponent_bytes[..copy_len]);
        blob.extend_from_slice(&exponent);
        blob.extend_from_slice(&modulus);

        let body = general_purpose::STANDARD.encode(blob);
        let lines = body
            .as_bytes()
            .chunks(64)
            .map(|chunk| std::str::from_utf8(chunk).expect("base64 chunk should be utf-8"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("-----BEGIN RSA PUBLIC KEY BLOB-----\n{lines}\n-----END RSA PUBLIC KEY BLOB-----\n")
    }

    fn complete_release_dir() -> PathBuf {
        let dir = temp_release_dir("complete");
        let assets = [
            "CC-Desktop-Switch-v1.1.0-rc1-Windows-Setup.exe",
            "CC-Desktop-Switch-v1.1.0-rc1-Windows-Portable.zip",
            "CC-Desktop-Switch-v1.1.0-rc1-Windows-x64.exe",
            "CC-Desktop-Switch-v1.1.0-rc1-macOS-arm64.pkg",
            "CC-Desktop-Switch-v1.1.0-rc1-macOS-arm64.dmg",
            "CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.pkg",
            "CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.dmg",
        ];
        for asset in assets {
            write_file_with_sha256(&dir, asset, "fixture");
        }
        write_fixture_public_key(&dir);
        fs::write(
            dir.join("latest.json"),
            r#"{
                "version": "1.1.0-rc1",
                "signature": {
                    "algorithm": "RSA-CSP-BLOB-SHA256",
                    "public_key": "CC-Desktop-Switch-release-public.pem"
                },
                "platforms": {
                    "windows-x64": {
                        "assets": [
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-Windows-Setup.exe", "signature": "CC-Desktop-Switch-v1.1.0-rc1-Windows-Setup.exe.sig" },
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-Windows-Portable.zip", "signature": "CC-Desktop-Switch-v1.1.0-rc1-Windows-Portable.zip.sig" },
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-Windows-x64.exe", "signature": "CC-Desktop-Switch-v1.1.0-rc1-Windows-x64.exe.sig" }
                        ]
                    },
                    "macos-arm64": {
                        "assets": [
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-macOS-arm64.pkg", "signature": "CC-Desktop-Switch-v1.1.0-rc1-macOS-arm64.pkg.sig" },
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-macOS-arm64.dmg", "signature": "CC-Desktop-Switch-v1.1.0-rc1-macOS-arm64.dmg.sig" }
                        ]
                    },
                    "macos-x64": {
                        "assets": [
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.pkg", "signature": "CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.pkg.sig" },
                            { "name": "CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.dmg", "signature": "CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.dmg.sig" }
                        ]
                    }
                }
            }"#,
        )
        .expect("latest.json should be written");
        write_sha256_sidecar(&dir.join("latest.json"));
        write_signature_sidecar(&dir.join("latest.json"), fixture_private_key());
        dir
    }

    #[test]
    fn complete_release_gate_passes() {
        let report = validate_release_gate(&complete_input());

        assert!(report.passed);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn missing_macos_x64_assets_fail_release_gate() {
        let mut input = complete_input();
        input
            .assets
            .retain(|asset| !asset.asset_id.starts_with("macos-x64"));

        let report = validate_release_gate(&input);

        assert!(!report.passed);
        assert!(report.issues.iter().any(|issue| {
            issue.code == "release.asset_missing" && issue.detail.contains("macos-x64-pkg")
        }));
        assert!(report.issues.iter().any(|issue| {
            issue.code == "release.asset_missing" && issue.detail.contains("macos-x64-dmg")
        }));
    }

    #[test]
    fn missing_hashes_signatures_and_public_key_fail_release_gate() {
        let mut input = complete_input();
        input.latest_json = Some("{not-json}".to_owned());
        input.latest_json_sha256_present = false;
        input.latest_json_signature_present = false;
        input.public_key_present = false;
        input.assets[0].sha256_present = false;
        input.assets[1].signature_present = false;

        let report = validate_release_gate(&input);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_invalid"));
        assert!(codes.contains("release.latest_json_sha256_missing"));
        assert!(codes.contains("release.latest_json_sig_missing"));
        assert!(codes.contains("release.public_key_missing"));
        assert!(codes.contains("release.asset_sha256_missing"));
        assert!(codes.contains("release.asset_sig_missing"));
    }

    #[test]
    fn complete_release_directory_passes() {
        let dir = complete_release_dir();

        let report = validate_release_directory(&dir);

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(report.passed, "{:?}", report.issues);
    }

    #[test]
    fn release_directory_rejects_latest_json_referencing_missing_asset() {
        let dir = complete_release_dir();
        fs::remove_file(dir.join("CC-Desktop-Switch-v1.1.0-rc1-macOS-x64.dmg"))
            .expect("asset should be removable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_asset_missing"));
    }

    #[test]
    fn release_directory_rejects_missing_sidecars_and_public_key() {
        let dir = complete_release_dir();
        fs::remove_file(dir.join("latest.json.sig")).expect("latest sig should be removable");
        fs::remove_file(dir.join("CC-Desktop-Switch-release-public.pem"))
            .expect("public key should be removable");
        fs::remove_file(dir.join("CC-Desktop-Switch-v1.1.0-rc1-Windows-Setup.exe.sha256"))
            .expect("asset hash should be removable");
        fs::remove_file(dir.join("CC-Desktop-Switch-v1.1.0-rc1-Windows-x64.exe.sig"))
            .expect("asset sig should be removable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_sig_missing"));
        assert!(codes.contains("release.public_key_missing"));
        assert!(codes.contains("release.asset_sha256_missing"));
        assert!(codes.contains("release.asset_sig_missing"));
    }

    #[test]
    fn release_directory_rejects_invalid_latest_json() {
        let dir = complete_release_dir();
        fs::write(dir.join("latest.json"), "{not-json}")
            .expect("invalid latest.json should be written");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_invalid"));
    }

    #[test]
    fn release_directory_rejects_latest_json_sha256_mismatch() {
        let dir = complete_release_dir();
        fs::write(
            dir.join("latest.json.sha256"),
            format!("{}  latest.json\n", "0".repeat(64)),
        )
        .expect("latest hash should be replaceable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_sha256_mismatch"));
    }

    #[test]
    fn release_directory_rejects_asset_sha256_mismatch() {
        let dir = complete_release_dir();
        fs::write(
            dir.join("CC-Desktop-Switch-v1.1.0-rc1-Windows-Setup.exe.sha256"),
            format!(
                "{}  CC-Desktop-Switch-v1.1.0-rc1-Windows-Setup.exe\n",
                "0".repeat(64)
            ),
        )
        .expect("asset hash should be replaceable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.asset_sha256_mismatch"));
    }

    #[test]
    fn release_directory_rejects_invalid_sha256_sidecars() {
        let dir = complete_release_dir();
        fs::write(dir.join("latest.json.sha256"), "not-a-sha")
            .expect("latest hash should be replaceable");
        fs::write(
            dir.join("CC-Desktop-Switch-v1.1.0-rc1-Windows-Portable.zip.sha256"),
            "not-a-sha",
        )
        .expect("asset hash should be replaceable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_sha256_invalid"));
        assert!(codes.contains("release.asset_sha256_invalid"));
    }

    #[test]
    fn release_directory_rejects_signature_mismatch_and_invalid_public_key() {
        let dir = complete_release_dir();
        fs::write(dir.join("latest.json.sig"), "not-base64")
            .expect("latest signature should be replaceable");
        fs::write(
            dir.join("CC-Desktop-Switch-v1.1.0-rc1-Windows-x64.exe.sig"),
            general_purpose::STANDARD.encode([0u8; 256]),
        )
        .expect("asset signature should be replaceable");
        fs::write(
            dir.join("CC-Desktop-Switch-release-public.pem"),
            "not-a-public-key",
        )
        .expect("public key should be replaceable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.public_key_invalid"));
    }

    #[test]
    fn release_directory_rejects_signature_mismatch_and_invalid_signature() {
        let dir = complete_release_dir();
        fs::write(dir.join("latest.json.sig"), "not-base64")
            .expect("latest signature should be replaceable");
        fs::write(
            dir.join("CC-Desktop-Switch-v1.1.0-rc1-Windows-x64.exe.sig"),
            general_purpose::STANDARD.encode([0u8; 256]),
        )
        .expect("asset signature should be replaceable");

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.latest_json_sig_invalid"));
        assert!(codes.contains("release.asset_sig_mismatch"));
    }

    #[test]
    fn release_directory_rejects_unsupported_signature_algorithm() {
        let dir = complete_release_dir();
        let latest_json = fs::read_to_string(dir.join("latest.json"))
            .expect("latest.json should be readable")
            .replace("RSA-CSP-BLOB-SHA256", "RSA-UNKNOWN");
        fs::write(dir.join("latest.json"), latest_json).expect("latest.json should be writable");
        write_sha256_sidecar(&dir.join("latest.json"));

        let report = validate_release_directory(&dir);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code.as_str())
            .collect::<HashSet<_>>();

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(!report.passed);
        assert!(codes.contains("release.signature_algorithm_unsupported"));
    }

    #[cfg(windows)]
    #[test]
    fn release_directory_verifies_powershell_manifest_signatures() {
        let dir = temp_release_dir("powershell");
        let key_dir = dir.join("keys");
        let version = "1.1.0-rc1";
        for asset in [
            format!("CC-Desktop-Switch-v{version}-Windows-Setup.exe"),
            format!("CC-Desktop-Switch-v{version}-Windows-Portable.zip"),
            format!("CC-Desktop-Switch-v{version}-Windows-x64.exe"),
            format!("CC-Desktop-Switch-v{version}-macOS-arm64.pkg"),
            format!("CC-Desktop-Switch-v{version}-macOS-arm64.dmg"),
            format!("CC-Desktop-Switch-v{version}-macOS-x64.pkg"),
            format!("CC-Desktop-Switch-v{version}-macOS-x64.dmg"),
        ] {
            fs::write(dir.join(asset), "powershell fixture")
                .expect("PowerShell fixture asset should be written");
        }
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri should have a repo root parent");
        let script = repo_root.join("scripts").join("New-ReleaseManifest.ps1");
        let output = Command::new("powershell")
            .arg("-NoProfile")
            .arg("-ExecutionPolicy")
            .arg("Bypass")
            .arg("-File")
            .arg(&script)
            .arg("-Version")
            .arg(version)
            .arg("-StagingDir")
            .arg(&dir)
            .arg("-Repository")
            .arg("")
            .arg("-KeyDir")
            .arg(&key_dir)
            .output()
            .expect("PowerShell manifest script should run");
        assert!(
            output.status.success(),
            "manifest script failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let report = validate_release_directory(&dir);

        fs::remove_dir_all(&dir).expect("release temp dir should be removed");
        assert!(report.passed, "{:?}", report.issues);
    }
}
