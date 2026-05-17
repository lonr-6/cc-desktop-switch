use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::Local;
use codex_app_transfer_registry::config_dir;
use serde_json::{json, Value};

const MAX_STORED_BUNDLES: usize = 50;
const MAX_STORED_BODY_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct UpstreamErrorBundleInput {
    pub method: String,
    pub client_path: String,
    pub upstream_url: String,
    pub status_code: u16,
    pub provider_id: String,
    pub provider_name: String,
    pub original_model: Option<String>,
    pub resolved_model: Option<String>,
    pub upstream_model: Option<String>,
    pub outbound_headers_redacted: String,
    pub request_body: Vec<u8>,
    pub response_body: Vec<u8>,
}

pub fn feedback_bundle_dir() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("feedback-bundles"))
}

pub fn write_upstream_error_bundle(input: &UpstreamErrorBundleInput) -> Option<PathBuf> {
    let dir = feedback_bundle_dir()?;
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }
    trim_old_bundles(&dir, MAX_STORED_BUNDLES);
    let now = Local::now();
    let bundle = json!({
        "kind": "upstream_error_bundle",
        "captured_at": now.to_rfc3339(),
        "proxy_version": env!("CARGO_PKG_VERSION"),
        "request": {
            "method": input.method,
            "client_path": input.client_path,
            "upstream_url": input.upstream_url,
            "status_code": input.status_code,
            "provider": {
                "id": input.provider_id,
                "name": input.provider_name,
            },
            "models": {
                "original": input.original_model,
                "resolved": input.resolved_model,
                "upstream": input.upstream_model,
            },
            "outbound_headers_redacted": input.outbound_headers_redacted,
            "body": bytes_payload(&input.request_body, MAX_STORED_BODY_BYTES),
        },
        "response": {
            "body": bytes_payload(&input.response_body, MAX_STORED_BODY_BYTES),
        },
    });
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let filename = format!(
        "bundle-{}-{}-{}.json",
        now.format("%Y%m%d-%H%M%S"),
        std::process::id(),
        ts
    );
    let path = dir.join(filename);
    let encoded = serde_json::to_vec_pretty(&bundle).ok()?;
    fs::write(&path, encoded).ok()?;
    Some(path)
}

pub fn recent_feedback_bundles(limit: usize) -> Vec<PathBuf> {
    let Some(dir) = feedback_bundle_dir() else {
        return Vec::new();
    };
    list_recent_json_files(&dir, limit)
}

fn list_recent_json_files(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    for item in rd.flatten() {
        let path = item.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = item.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        entries.push((modified, path));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    entries
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect()
}

fn bytes_payload(bytes: &[u8], max_bytes: usize) -> Value {
    let (slice, truncated_bytes) = if bytes.len() > max_bytes {
        (&bytes[..max_bytes], bytes.len() - max_bytes)
    } else {
        (bytes, 0usize)
    };
    match std::str::from_utf8(slice) {
        Ok(text) => json!({
            "encoding": "utf8",
            "bytes": bytes.len(),
            "truncated_bytes": truncated_bytes,
            "content": text,
        }),
        Err(_) => json!({
            "encoding": "base64",
            "bytes": bytes.len(),
            "truncated_bytes": truncated_bytes,
            "content": STANDARD.encode(slice),
        }),
    }
}

fn trim_old_bundles(dir: &Path, keep: usize) {
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for item in rd.flatten() {
        let path = item.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = item.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        files.push((modified, path));
    }
    if files.len() <= keep {
        return;
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_payload_preserves_utf8_and_binary() {
        let text = bytes_payload(br#"{"ok":true}"#, 1024);
        assert_eq!(text["encoding"], "utf8");
        assert_eq!(text["content"], r#"{"ok":true}"#);

        let bin = bytes_payload(&[0xff, 0xfe, 0xfd], 1024);
        assert_eq!(bin["encoding"], "base64");
        assert!(bin["content"].as_str().unwrap_or("").len() >= 4);
    }

    #[test]
    fn bytes_payload_truncates_large_content() {
        let long = "a".repeat(20);
        let v = bytes_payload(long.as_bytes(), 8);
        assert_eq!(v["bytes"], 20);
        assert_eq!(v["truncated_bytes"], 12);
        assert_eq!(v["content"], "aaaaaaaa");
    }
}
