//! `xtask release-bundle` — 签 release artifact + 出 latest.json.
//!
//! 替代 `scripts/release_assets.py` (Phase 3 Commit D), Rust 版本与 Python
//! 版**输出字节级一致**: PKCS#1 v1.5 是确定性签名 (同 key 同 msg → 同 sig),
//! `serde_json::to_string_pretty(preserve_order)` 与 `json.dumps(indent=2,
//! ensure_ascii=False)` 等价, RSA-3072 PKCS#8 PEM 是 RFC 标准, `cryptography`
//! 与 `rsa = "0.9"` 互通.
//!
//! 算法:
//! - 签名: RSA-3072 PKCS#1 v1.5 + SHA-256 (raw signature, 384 bytes, base64
//!   存为 `<file>.sig`)
//! - sha256: 流式 1 MB chunk
//! - latest.json schema: 与 Python 版同, 包含 platforms 列表 + 签名 metadata

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use clap::Args as ClapArgs;
use indexmap::IndexMap;
use rand::rngs::OsRng;
use regex::Regex;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::{Pkcs1v15Sign, RsaPrivateKey, RsaPublicKey};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const PROJECT_NAME: &str = "Codex App Transfer";
const ASSET_PREFIX: &str = "Codex-App-Transfer";
const PUBLIC_KEY_BASENAME: &str = "Codex-App-Transfer-release-public.pem";

/// Phase 2 后的 PLATFORM_PATTERNS:
/// - macOS:  .pkg 退役, 只保 .dmg
/// - Linux:  .deb (推荐) + .AppImage (兜底)
/// - Windows: -Setup.exe (NSIS) + .msi (WiX)
fn platform_patterns() -> IndexMap<&'static str, Vec<(Regex, &'static str)>> {
    let mut m = IndexMap::new();
    m.insert(
        "windows",
        vec![
            (
                Regex::new(r"-Windows-x64-Setup\.exe$").unwrap(),
                "windows-x64",
            ),
            (Regex::new(r"-Windows-x64\.msi$").unwrap(), "windows-x64"),
        ],
    );
    m.insert(
        "macos",
        vec![
            (Regex::new(r"-macOS-arm64\.dmg$").unwrap(), "macos-arm64"),
            (Regex::new(r"-macOS-x64\.dmg$").unwrap(), "macos-x64"),
        ],
    );
    m.insert(
        "linux",
        vec![
            (Regex::new(r"-Linux-x86_64\.deb$").unwrap(), "linux-x86_64"),
            (
                Regex::new(r"-Linux-x86_64\.AppImage$").unwrap(),
                "linux-x86_64",
            ),
        ],
    );
    m
}

#[derive(ClapArgs)]
pub struct Args {
    /// release 版本号 (如 2.0.1, 不带 v 前缀)
    #[arg(long)]
    version: String,

    /// 处理哪些平台 (默认 macos linux windows 三平台全跑)
    #[arg(long, num_args = 1.., default_values_t = ["macos".to_string(), "linux".to_string(), "windows".to_string()])]
    include: Vec<String>,

    /// 已 rename 的产物输入目录 (release.yml build job 的 download-artifact 落地)
    #[arg(long, default_value = "dist-incoming")]
    incoming_dir: PathBuf,

    /// release/ 输出目录 (sha256 + .sig + latest.json 落地处)
    #[arg(long, default_value = "release")]
    output_dir: PathBuf,

    /// 仓库 owner/repo, latest.json 的 url 字段会变成 GH release URL
    #[arg(long, env = "GITHUB_REPOSITORY")]
    repo: Option<String>,
}

#[derive(Serialize, Clone)]
struct AssetEntry {
    name: String,
    url: String,
    signature: String,
    sha256: String,
    size: u64,
}

pub fn run(args: Args) -> Result<()> {
    let patterns = platform_patterns();
    let include = validate_include(&args.include, &patterns)?;

    let root = repo_root()?;
    let release_dir = root.join(&args.output_dir);
    let incoming_dir = root.join(&args.incoming_dir);
    let key_dir = root.join(".release-signing");

    fs::create_dir_all(&release_dir)
        .with_context(|| format!("创建输出目录: {}", release_dir.display()))?;
    let private_key = get_or_create_key(&key_dir, &release_dir)?;

    let mut platforms: IndexMap<String, Vec<AssetEntry>> = IndexMap::new();

    // 1) --include 平台: 清旧 + cp incoming → release/ + 签名 + 索引
    for (platform_name, pats) in &patterns {
        if !include.contains(*platform_name) {
            continue;
        }
        clean_platform(&release_dir, pats)?;
        let files = collect_from_incoming(&incoming_dir, &release_dir, &args.version, pats)?;
        for f in files {
            let asset = sign_and_index(&f, &private_key, args.repo.as_deref(), &args.version)?;
            for (re, key) in pats {
                if re.is_match(&f.file_name().unwrap().to_string_lossy()) {
                    platforms
                        .entry(key.to_string())
                        .or_default()
                        .push(asset.clone());
                    break;
                }
            }
        }
    }

    // 2) 没 --include 的平台: 从 release/ 上次跑已签的文件读出, 让 latest.json
    //    仍然完整描述全部 3 平台 (incremental release 场景).
    for (platform_name, pats) in &patterns {
        if include.contains(*platform_name) {
            continue;
        }
        for f in existing_assets_for_platform(&release_dir, &args.version, pats)? {
            if let Some(asset) = asset_dict_from_existing(&f, args.repo.as_deref(), &args.version)?
            {
                for (re, key) in pats {
                    if re.is_match(&f.file_name().unwrap().to_string_lossy()) {
                        platforms.entry(key.to_string()).or_default().push(asset);
                        break;
                    }
                }
            }
        }
    }

    // platforms 按 key 排序, assets 按 name 排序, 输出稳定
    let mut sorted: IndexMap<String, Value> = IndexMap::new();
    let mut keys: Vec<&String> = platforms.keys().collect();
    keys.sort();
    for k in keys {
        let mut assets = platforms[k].clone();
        assets.sort_by(|a, b| a.name.cmp(&b.name));
        sorted.insert(k.clone(), json!({ "assets": assets }));
    }

    let latest = json!({
        "name": PROJECT_NAME,
        "version": args.version,
        "pub_date": Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "notes": format!("Release for {} v{}.", PROJECT_NAME, args.version),
        "update_protocol": 1,
        "minimum_supported_version": "1.0.0",
        "platforms": sorted,
        "signature": {
            "algorithm": "RSA-PKCS1-V15-SHA256",
            "public_key": PUBLIC_KEY_BASENAME,
            "format": "base64 raw signature over file bytes",
        },
    });

    let latest_path = release_dir.join("latest.json");
    fs::write(&latest_path, serde_json::to_string_pretty(&latest)?)?;
    write_sha256(&latest_path)?;
    sign_file(&latest_path, &private_key)?;

    println!("\nRelease assets in {}", release_dir.display());
    let mut entries: Vec<_> = fs::read_dir(&release_dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        if e.path().is_file() {
            let size = e.metadata()?.len();
            println!("  {}  ({} bytes)", e.file_name().to_string_lossy(), size);
        }
    }

    if sorted.is_empty() {
        eprintln!("\nWARNING: no platform artifacts found. Build first.");
        std::process::exit(1);
    }
    Ok(())
}

fn validate_include<'a>(
    include: &'a [String],
    patterns: &IndexMap<&'static str, Vec<(Regex, &'static str)>>,
) -> Result<std::collections::HashSet<&'a str>> {
    let mut selected = std::collections::HashSet::new();
    for platform in include {
        let platform = platform.as_str();
        if !patterns.contains_key(platform) {
            let valid = patterns.keys().copied().collect::<Vec<_>>().join(", ");
            bail!("unsupported --include platform '{platform}'. Valid values: {valid}");
        }
        selected.insert(platform);
    }
    Ok(selected)
}

fn get_or_create_key(key_dir: &Path, release_dir: &Path) -> Result<RsaPrivateKey> {
    fs::create_dir_all(key_dir).with_context(|| format!("创建 key 目录: {}", key_dir.display()))?;
    let private_path = key_dir.join("release-private-key.pem");
    let public_path = key_dir.join("release-public-key.pem");

    let private_key = if private_path.exists() {
        let pem = fs::read_to_string(&private_path)
            .with_context(|| format!("读私钥: {}", private_path.display()))?;
        RsaPrivateKey::from_pkcs8_pem(&pem)
            .with_context(|| format!("解析 PKCS#8 PEM: {}", private_path.display()))?
    } else {
        let mut rng = OsRng;
        let key = RsaPrivateKey::new(&mut rng, 3072)?;
        let pem = key.to_pkcs8_pem(LineEnding::LF)?;
        fs::write(&private_path, pem.as_bytes())?;
        let pub_pem = RsaPublicKey::from(&key).to_public_key_pem(LineEnding::LF)?;
        fs::write(&public_path, pub_pem.as_bytes())?;
        eprintln!(
            "Created local release signing key: {}",
            private_path.display()
        );
        key
    };

    // 确保公钥也存在并复制到 release/
    if !public_path.exists() {
        let pub_pem = RsaPublicKey::from(&private_key).to_public_key_pem(LineEnding::LF)?;
        fs::write(&public_path, pub_pem.as_bytes())?;
    }
    fs::copy(&public_path, release_dir.join(PUBLIC_KEY_BASENAME))?;

    Ok(private_key)
}

fn sha256_of(path: &Path) -> Result<String> {
    let f = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::with_capacity(1024 * 1024, f);
    let mut h = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

fn write_sha256(path: &Path) -> Result<PathBuf> {
    let digest = sha256_of(path)?;
    let sha_path = path.parent().unwrap().join(format!(
        "{}.sha256",
        path.file_name().unwrap().to_string_lossy()
    ));
    let name = path.file_name().unwrap().to_string_lossy();
    fs::write(&sha_path, format!("{}  {}\n", digest, name))?;
    Ok(sha_path)
}

fn sign_file(path: &Path, private_key: &RsaPrivateKey) -> Result<PathBuf> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let hashed = Sha256::digest(&bytes);
    let sig = private_key
        .sign(Pkcs1v15Sign::new::<Sha256>(), &hashed)
        .map_err(|e| anyhow::anyhow!("sign failed: {e}"))?;
    let sig_path = path.parent().unwrap().join(format!(
        "{}.sig",
        path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&sig_path, B64.encode(&sig))?;
    Ok(sig_path)
}

fn asset_url(repo: Option<&str>, version: &str, filename: &str) -> String {
    match repo {
        Some(r) => format!(
            "https://github.com/{}/releases/download/v{}/{}",
            r, version, filename
        ),
        None => filename.to_string(),
    }
}

fn sign_and_index(
    path: &Path,
    private_key: &RsaPrivateKey,
    repo: Option<&str>,
    version: &str,
) -> Result<AssetEntry> {
    write_sha256(path)?;
    sign_file(path, private_key)?;
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    Ok(AssetEntry {
        url: asset_url(repo, version, &name),
        signature: format!("{}.sig", &name),
        sha256: sha256_of(path)?,
        size: path.metadata()?.len(),
        name,
    })
}

fn clean_platform(release_dir: &Path, pats: &[(Regex, &'static str)]) -> Result<()> {
    if !release_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(release_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut base = name.clone();
        for trail in [".sha256", ".sig"] {
            if let Some(stripped) = base.strip_suffix(trail) {
                base = stripped.to_string();
                break;
            }
        }
        for (re, _) in pats {
            if re.is_match(&base) {
                fs::remove_file(entry.path())?;
                break;
            }
        }
    }
    Ok(())
}

fn collect_from_incoming(
    incoming_dir: &Path,
    release_dir: &Path,
    version: &str,
    pats: &[(Regex, &'static str)],
) -> Result<Vec<PathBuf>> {
    if !incoming_dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let prefix = format!("{}-v{}-", ASSET_PREFIX, version);
    let mut entries: Vec<_> = fs::read_dir(incoming_dir)?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        for (re, _) in pats {
            if re.is_match(&name) {
                let target = release_dir.join(&name);
                if target.exists() {
                    fs::remove_file(&target)?;
                }
                fs::copy(entry.path(), &target)?;
                out.push(target);
                break;
            }
        }
    }
    Ok(out)
}

fn existing_assets_for_platform(
    release_dir: &Path,
    version: &str,
    pats: &[(Regex, &'static str)],
) -> Result<Vec<PathBuf>> {
    if !release_dir.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    let prefix = format!("{}-v{}-", ASSET_PREFIX, version);
    for entry in fs::read_dir(release_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".sha256") || name.ends_with(".sig") {
            continue;
        }
        if name == PUBLIC_KEY_BASENAME || name.starts_with("latest.json") {
            continue;
        }
        if !name.starts_with(&prefix) {
            continue;
        }
        for (re, _) in pats {
            if re.is_match(&name) {
                found.push(entry.path());
                break;
            }
        }
    }
    Ok(found)
}

fn asset_dict_from_existing(
    path: &Path,
    repo: Option<&str>,
    version: &str,
) -> Result<Option<AssetEntry>> {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let sig_path = path.parent().unwrap().join(format!("{}.sig", &name));
    if !sig_path.exists() {
        return Ok(None);
    }
    Ok(Some(AssetEntry {
        url: asset_url(repo, version, &name),
        signature: format!("{}.sig", &name),
        sha256: sha256_of(path)?,
        size: path.metadata()?.len(),
        name,
    }))
}

fn repo_root() -> Result<PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Ok(PathBuf::from(manifest)
        .parent()
        .context("xtask 的 parent 目录不存在")?
        .to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端单测: Rust 生成 key → sign 一段 bytes → verify OK →
    /// PKCS#8 PEM round-trip 后 sign 同一段 bytes → 签名 byte-for-byte 相同
    /// (PKCS#1 v1.5 是确定性签名, 这是 Python cryptography 兼容性的算法层证据).
    #[test]
    fn rsa_sign_roundtrip_deterministic() {
        let mut rng = OsRng;
        let priv1 = RsaPrivateKey::new(&mut rng, 3072).unwrap();
        let pub1 = RsaPublicKey::from(&priv1);
        let data = b"hello, codex-app-transfer release pipeline";

        let hashed = Sha256::digest(data);
        let sig1 = priv1.sign(Pkcs1v15Sign::new::<Sha256>(), &hashed).unwrap();
        assert_eq!(sig1.len(), 384, "3072-bit RSA signature must be 384 bytes");

        // 验签 OK
        pub1.verify(Pkcs1v15Sign::new::<Sha256>(), &hashed, &sig1)
            .expect("verify must pass");

        // PKCS#8 PEM round-trip → 签名仍 byte-for-byte 一致
        let pem = priv1.to_pkcs8_pem(LineEnding::LF).unwrap();
        let priv2 = RsaPrivateKey::from_pkcs8_pem(&pem).unwrap();
        let sig2 = priv2.sign(Pkcs1v15Sign::new::<Sha256>(), &hashed).unwrap();
        assert_eq!(
            sig1, sig2,
            "PKCS#1 v1.5 deterministic: 同 key 同 msg 必同 sig"
        );
    }

    /// 验签失败路径: 改一个 bit 的 data, verify 必须 Err.
    #[test]
    fn verify_rejects_tampered_data() {
        let mut rng = OsRng;
        let priv1 = RsaPrivateKey::new(&mut rng, 3072).unwrap();
        let pub1 = RsaPublicKey::from(&priv1);
        let data = b"original";
        let hashed = Sha256::digest(data);
        let sig = priv1.sign(Pkcs1v15Sign::new::<Sha256>(), &hashed).unwrap();

        let tampered = Sha256::digest(b"tampered");
        assert!(pub1
            .verify(Pkcs1v15Sign::new::<Sha256>(), &tampered, &sig)
            .is_err());
    }

    #[test]
    fn validate_include_rejects_unknown_platform() {
        let patterns = platform_patterns();
        let include = vec!["macos".to_string(), "freebsd".to_string()];
        let err = validate_include(&include, &patterns)
            .expect_err("unknown platform must be rejected")
            .to_string();
        assert!(err.contains("unsupported --include platform 'freebsd'"));
        assert!(err.contains("macos"));
        assert!(err.contains("linux"));
        assert!(err.contains("windows"));
    }

    #[test]
    fn sha256_known_vector() {
        // 标准 sha256("") 16 bytes 截前 = e3b0c44298fc1c14...
        let dir = std::env::temp_dir().join(format!(
            "xtask-sha256-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("empty.bin");
        fs::write(&p, b"").unwrap();
        assert_eq!(
            sha256_of(&p).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
