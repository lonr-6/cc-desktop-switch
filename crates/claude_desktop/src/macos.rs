//! macOS Claude Desktop 配置写入 ——
//!
//! 跟踪 `lonr-6/cc-desktop-switch` v1.0.24 `backend/registry.py` 的三源
//! macOS 写入(plist + JSON + configLibrary)。Claude Desktop ≥ 1.7196 用
//! `~/Library/Application Support/Claude-3p/` 子目录(注意:**不是** `Claude/`)。
//!
//! 三个配置源(对照 `registry.py:363/617/662` 三个 `_mac_apply_*`):
//!
//! 1. `~/Library/Preferences/com.anthropic.claudefordesktop.plist`
//!    user defaults plist(数组字段用 JSON string 包装)
//! 2. `~/Library/Application Support/Claude-3p/claude_desktop_config.json`
//!    顶层 `deploymentMode="3p"` + `enterpriseConfig` 段(JSON 真实类型)
//! 3. `~/Library/Application Support/Claude-3p/configLibrary/<uuid>.json`
//!    单独一份 enterprise 配置 + 通过 `_meta.json.appliedId` 指向激活的 entry
//!
//! 三个源**同时写**。Claude Desktop 1.7196+ 优先级是 configLibrary >
//! claude_desktop_config.json > plist;只有 deploymentMode="3p" 才会真正
//! 切换到 inference gateway。restore 时把 deploymentMode 改回 "clear"。
//!
//! 字段集对齐 [`crate::helpers::DESKTOP_CONFIG`](7 字段)+
//! [`crate::helpers::CCDS_MARKER`] + `coworkEgressAllowedHosts: ["*"]`
//! (1.7196 新加,见 `registry.py:547`)。

use std::collections::BTreeMap;
use std::path::PathBuf;

use plist::Value as PlistValue;
#[allow(unused_imports)]
use serde_json::{json, Value as JsonValue};

use crate::helpers::{
    managed_policy_names, serialize_gateway_headers, serialize_inference_models, CCDS_MARKER,
    DESKTOP_CONFIG,
};
use crate::schema::Provider;
use crate::ClaudeDesktopError;

/// `~/Library/Application Support/Claude-3p/claude_desktop_config.json`
/// (Claude Desktop 1.7196+ 用 `Claude-3p/` 子目录,不是 `Claude/`。
/// 对照 `cc-desktop-switch/backend/registry.py:340 MAC_3P_CONFIG`)
pub fn config_json_path() -> Result<PathBuf, ClaudeDesktopError> {
    let home = dirs::home_dir().ok_or_else(|| {
        ClaudeDesktopError::SchemaCorrupt("无法解析 home 目录".to_owned())
    })?;
    Ok(home
        .join("Library")
        .join("Application Support")
        .join("Claude-3p")
        .join("claude_desktop_config.json"))
}

/// `~/Library/Application Support/Claude-3p/configLibrary` 目录
/// (对照 `registry.py:341 MAC_3P_CONFIG_LIBRARY` + `427 _mac_config_library_dir_path`)
pub fn config_library_dir_path() -> Result<PathBuf, ClaudeDesktopError> {
    let cfg = config_json_path()?;
    Ok(cfg
        .parent()
        .ok_or_else(|| ClaudeDesktopError::SchemaCorrupt("config.json 无 parent".to_owned()))?
        .join("configLibrary"))
}

/// `configLibrary/_meta.json`(对照 `registry.py:431`)
pub fn config_library_meta_path() -> Result<PathBuf, ClaudeDesktopError> {
    Ok(config_library_dir_path()?.join("_meta.json"))
}

/// `configLibrary/<entry_id>.json`(对照 `registry.py:435`)
pub fn config_library_entry_path(entry_id: &str) -> Result<PathBuf, ClaudeDesktopError> {
    Ok(config_library_dir_path()?.join(format!("{entry_id}.json")))
}

/// COWORK_EGRESS_ALLOWED_HOSTS 默认值(对照 `registry.py:16`)。
const COWORK_EGRESS_ALLOWED_HOSTS_DEFAULT: &str = "*";

/// `~/Library/Preferences/com.anthropic.claudefordesktop.plist`
pub fn plist_path() -> Result<PathBuf, ClaudeDesktopError> {
    let home = dirs::home_dir().ok_or_else(|| {
        ClaudeDesktopError::SchemaCorrupt("无法解析 home 目录".to_owned())
    })?;
    Ok(home
        .join("Library")
        .join("Preferences")
        .join("com.anthropic.claudefordesktop.plist"))
}

/// 单次 apply 的参数 —— 完整封装 7 字段所需输入。
#[derive(Debug, Clone)]
pub struct ApplyInput<'a> {
    pub provider: &'a Provider,
    pub all_providers: &'a [Provider],
    pub gateway_api_key: &'a str,
    /// 若为 true,inferenceModels 字段写所有 provider 模型(`expose_all` 设置)。
    pub expose_all_models: bool,
    /// 若 Some,覆盖 default base_url(`http://127.0.0.1:18080`)。
    pub gateway_base_url: Option<&'a str>,
}

/// 根据 [`ApplyInput`] 算出最终 7 字段 string-form 值(适合 plist;
/// config.json 还需要再解开数组字段)。
pub fn compute_field_values(input: &ApplyInput<'_>) -> BTreeMap<&'static str, String> {
    let mut out: BTreeMap<&'static str, String> = BTreeMap::new();
    out.insert("inferenceProvider", "gateway".to_owned());
    out.insert(
        "inferenceGatewayApiKey",
        input.gateway_api_key.to_owned(),
    );
    out.insert(
        "inferenceGatewayAuthScheme",
        input.provider.auth_scheme.clone(),
    );
    out.insert(
        "inferenceGatewayHeaders",
        serialize_gateway_headers(input.provider.extra_headers.iter(), input.gateway_api_key),
    );
    out.insert(
        "inferenceModels",
        serialize_inference_models(
            Some(input.provider),
            input.all_providers,
            input.expose_all_models,
        ),
    );
    out.insert(
        "inferenceGatewayBaseUrl",
        input
            .gateway_base_url
            .unwrap_or("http://127.0.0.1:18080")
            .to_owned(),
    );
    out.insert("isClaudeCodeForDesktopEnabled", "1".to_owned());
    out
}

// ──────────────────────────── plist ────────────────────────────

/// 写 user defaults plist。数组字段(headers / models)用 JSON string 包装,
/// 跟 Windows REG_SZ 保持一致;integer 字段(isClaudeCodeForDesktopEnabled)
/// 用 `PlistValue::Integer`。
pub fn write_plist(input: &ApplyInput<'_>) -> Result<(), ClaudeDesktopError> {
    let path = plist_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 读现有 plist(若存在);允许文件不存在则起空 dict
    let mut root: BTreeMap<String, PlistValue> = if path.exists() {
        match PlistValue::from_file(&path) {
            Ok(PlistValue::Dictionary(d)) => d.into_iter().collect(),
            Ok(_) => BTreeMap::new(),
            Err(e) => return Err(ClaudeDesktopError::SchemaCorrupt(format!("plist 解析失败: {e}"))),
        }
    } else {
        BTreeMap::new()
    };

    let values = compute_field_values(input);
    for field in DESKTOP_CONFIG {
        let value = values
            .get(field.name)
            .cloned()
            .unwrap_or_default();
        if field.name == "isClaudeCodeForDesktopEnabled" {
            // integer 字段
            let v: i64 = value.parse().unwrap_or(1);
            root.insert(field.name.to_owned(), PlistValue::Integer(v.into()));
        } else {
            root.insert(field.name.to_owned(), PlistValue::String(value));
        }
    }
    root.insert(CCDS_MARKER.to_owned(), PlistValue::String("true".to_owned()));

    // 写回(plist crate 默认输出 XML,Claude Desktop 都能读)
    let dict: plist::Dictionary = root.into_iter().collect();
    PlistValue::Dictionary(dict)
        .to_file_xml(&path)
        .map_err(|e| ClaudeDesktopError::SchemaCorrupt(format!("plist 写入失败: {e}")))?;
    Ok(())
}

/// 清除 plist 中由本工具管理的字段(对齐 `_managed_policy_names`)。
pub fn clear_plist() -> Result<(), ClaudeDesktopError> {
    let path = plist_path()?;
    if !path.exists() {
        return Ok(());
    }
    let mut root: BTreeMap<String, PlistValue> = match PlistValue::from_file(&path) {
        Ok(PlistValue::Dictionary(d)) => d.into_iter().collect(),
        Ok(_) => return Ok(()),
        Err(e) => return Err(ClaudeDesktopError::SchemaCorrupt(format!("plist 解析失败: {e}"))),
    };
    let existing_names: Vec<String> = root.keys().cloned().collect();
    let managed = managed_policy_names(&existing_names);
    for key in managed {
        root.remove(key);
    }
    let dict: plist::Dictionary = root.into_iter().collect();
    PlistValue::Dictionary(dict)
        .to_file_xml(&path)
        .map_err(|e| ClaudeDesktopError::SchemaCorrupt(format!("plist 写入失败: {e}")))?;
    Ok(())
}

// ────────────────────── claude_desktop_config.json ──────────────────────

/// 写 `~/Library/Application Support/Claude/claude_desktop_config.json` 的
/// `enterpriseConfig` 段(JSON 真实类型 — 数组字段用真实 array)。
pub fn write_config_json(input: &ApplyInput<'_>) -> Result<(), ClaudeDesktopError> {
    let path = config_json_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut root: JsonValue = if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        if text.trim().is_empty() {
            json!({})
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
        }
    } else {
        json!({})
    };
    let obj = root.as_object_mut().ok_or_else(|| {
        ClaudeDesktopError::SchemaCorrupt("config.json 顶层必须是 object".to_owned())
    })?;
    let enterprise = obj
        .entry("enterpriseConfig".to_owned())
        .or_insert_with(|| json!({}));
    let enterprise_obj = enterprise.as_object_mut().ok_or_else(|| {
        ClaudeDesktopError::SchemaCorrupt("enterpriseConfig 必须是 object".to_owned())
    })?;

    enterprise_obj.insert("inferenceProvider".to_owned(), JsonValue::String("gateway".to_owned()));
    enterprise_obj.insert(
        "inferenceGatewayApiKey".to_owned(),
        JsonValue::String(input.gateway_api_key.to_owned()),
    );
    enterprise_obj.insert(
        "inferenceGatewayAuthScheme".to_owned(),
        JsonValue::String(input.provider.auth_scheme.clone()),
    );
    // headers / models 用真实 array(跟 plist 用 string-wrapped 不同)
    let headers_str =
        serialize_gateway_headers(input.provider.extra_headers.iter(), input.gateway_api_key);
    let headers_array: JsonValue = if headers_str.is_empty() {
        json!([])
    } else {
        serde_json::from_str(&headers_str).unwrap_or(json!([]))
    };
    enterprise_obj.insert("inferenceGatewayHeaders".to_owned(), headers_array);
    let models_str = serialize_inference_models(
        Some(input.provider),
        input.all_providers,
        input.expose_all_models,
    );
    let models_array: JsonValue = serde_json::from_str(&models_str).unwrap_or(json!([]));
    enterprise_obj.insert("inferenceModels".to_owned(), models_array);
    enterprise_obj.insert(
        "inferenceGatewayBaseUrl".to_owned(),
        JsonValue::String(
            input
                .gateway_base_url
                .unwrap_or("http://127.0.0.1:18080")
                .to_owned(),
        ),
    );
    enterprise_obj.insert("isClaudeCodeForDesktopEnabled".to_owned(), JsonValue::Bool(true));
    // `coworkEgressAllowedHosts` policy(`registry.py:547`)—— 1.7196+ 必填,
    // 控制 cowork 出站白名单。默认 `["*"]` 全放行(跟上游一致)。
    enterprise_obj.insert(
        "coworkEgressAllowedHosts".to_owned(),
        JsonValue::Array(vec![JsonValue::String(
            COWORK_EGRESS_ALLOWED_HOSTS_DEFAULT.to_owned(),
        )]),
    );
    // marker(用 string "true" 对齐 plist;后续 restore 时清掉)
    enterprise_obj.insert(CCDS_MARKER.to_owned(), JsonValue::String("true".to_owned()));

    // **关键 sentinel**:顶层 `deploymentMode="3p"` 才让 Claude Desktop 切到
    // 第三方部署模式。这是 1.7196 加的开关(对照 `registry.py:721`)。
    obj.insert("deploymentMode".to_owned(), JsonValue::String("3p".to_owned()));

    let serialized = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, serialized)?;
    Ok(())
}

/// 算 enterpriseConfig dict(共用给 claude_desktop_config.json 和 configLibrary
/// entry。对照 `registry.py:532 _mac_json_enterprise_config`)。
fn compute_enterprise_config(input: &ApplyInput<'_>) -> JsonValue {
    let headers_str =
        serialize_gateway_headers(input.provider.extra_headers.iter(), input.gateway_api_key);
    let headers_array: JsonValue = if headers_str.is_empty() {
        json!([])
    } else {
        serde_json::from_str(&headers_str).unwrap_or(json!([]))
    };
    let models_str = serialize_inference_models(
        Some(input.provider),
        input.all_providers,
        input.expose_all_models,
    );
    let models_array: JsonValue = serde_json::from_str(&models_str).unwrap_or(json!([]));
    json!({
        "inferenceProvider": "gateway",
        "inferenceGatewayBaseUrl": input.gateway_base_url.unwrap_or("http://127.0.0.1:18080"),
        "inferenceGatewayApiKey": input.gateway_api_key,
        "inferenceGatewayAuthScheme": input.provider.auth_scheme,
        "inferenceGatewayHeaders": headers_array,
        "inferenceModels": models_array,
        "isClaudeCodeForDesktopEnabled": true,
        "coworkEgressAllowedHosts": [COWORK_EGRESS_ALLOWED_HOSTS_DEFAULT],
    })
}

/// 简单 UUID v4 风格生成器(用纳秒 + 进程 id 凑伪 UUID;Claude Desktop 只看
/// 字符串唯一性,不需要 RFC 4122 严格 UUID)。
fn make_entry_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mix = nanos ^ (pid << 32);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (mix >> 96) as u32,
        ((mix >> 80) & 0xffff) as u16,
        ((mix >> 64) & 0xfff) as u16,
        ((mix >> 48) & 0xffff) as u16,
        (mix & 0xffffffffffff) as u64
    )
}

/// 写 configLibrary —— 第三个配置源(`registry.py:649 _mac_apply_library_config`)。
/// 读 `_meta.json` 取 `appliedId`;若没 _meta 或 appliedId 缺,自建 entry 并更新 _meta。
pub fn write_config_library(input: &ApplyInput<'_>) -> Result<(), ClaudeDesktopError> {
    let dir = config_library_dir_path()?;
    std::fs::create_dir_all(&dir)?;
    let meta_path = config_library_meta_path()?;

    let mut meta: JsonValue = if meta_path.exists() {
        match std::fs::read_to_string(&meta_path) {
            Ok(t) if !t.trim().is_empty() => serde_json::from_str(&t).unwrap_or(json!({})),
            _ => json!({}),
        }
    } else {
        json!({})
    };
    let meta_obj = meta.as_object_mut().ok_or_else(|| {
        ClaudeDesktopError::SchemaCorrupt("_meta.json 顶层必须是 object".to_owned())
    })?;

    // 取已有 appliedId,若 entry 文件不存在或 appliedId 缺则建新
    let active_id: String = meta_obj
        .get("appliedId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .filter(|id| !id.is_empty())
        .unwrap_or_else(make_entry_id);

    let entry_path = config_library_entry_path(&active_id)?;
    let mut entry: JsonValue = if entry_path.exists() {
        match std::fs::read_to_string(&entry_path) {
            Ok(t) if !t.trim().is_empty() => serde_json::from_str(&t).unwrap_or(json!({})),
            _ => json!({}),
        }
    } else {
        json!({})
    };
    let entry_obj = entry.as_object_mut().ok_or_else(|| {
        ClaudeDesktopError::SchemaCorrupt("configLibrary entry 必须是 object".to_owned())
    })?;
    // 用 compute_enterprise_config 算 expected fields 合并(保留 entry 已有但
    // 非 managed 的字段)
    if let JsonValue::Object(ent) = compute_enterprise_config(input) {
        for (k, v) in ent {
            entry_obj.insert(k, v);
        }
    }
    entry_obj.insert(CCDS_MARKER.to_owned(), JsonValue::String("true".to_owned()));

    let entry_serialized = serde_json::to_string_pretty(&entry)?;
    std::fs::write(&entry_path, entry_serialized)?;

    // 更新 _meta.json(appliedId + entries 中至少有这一条)
    meta_obj.insert("appliedId".to_owned(), JsonValue::String(active_id.clone()));
    let entries = meta_obj
        .entry("entries".to_owned())
        .or_insert_with(|| json!([]));
    if let Some(arr) = entries.as_array_mut() {
        let has = arr.iter().any(|e| {
            e.get("id").and_then(|x| x.as_str()) == Some(active_id.as_str())
        });
        if !has {
            arr.push(json!({"id": active_id, "name": "Default"}));
        }
    }
    let meta_serialized = serde_json::to_string_pretty(&meta)?;
    std::fs::write(&meta_path, meta_serialized)?;
    Ok(())
}

/// 清 configLibrary 中本工具 managed 字段(对照 `registry.py:_mac_clear_library_config`,
/// 但更保守 —— 只清字段不删 entry;deploymentMode 切换由 config.json 顶层负责)。
pub fn clear_config_library() -> Result<(), ClaudeDesktopError> {
    let dir = match config_library_dir_path() {
        Ok(d) if d.exists() => d,
        _ => return Ok(()),
    };
    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(_) => return Ok(()),
    };
    for de in entries.flatten() {
        let p = de.path();
        let name = match p.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };
        if name == "_meta.json" || !name.ends_with(".json") {
            continue;
        }
        let text = match std::fs::read_to_string(&p) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        let mut v: JsonValue = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(o) = v.as_object_mut() {
            let names: Vec<String> = o.keys().cloned().collect();
            let managed = managed_policy_names(&names);
            for k in managed.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>() {
                o.remove(&k);
            }
        }
        if let Ok(s) = serde_json::to_string_pretty(&v) {
            let _ = std::fs::write(&p, s);
        }
    }
    Ok(())
}

/// 清除 config.json 中本工具管理的字段(只删 enterpriseConfig 段里 managed
/// 字段,不动 user 自加字段)。如果 enterpriseConfig 段清完为空,删该段。
pub fn clear_config_json() -> Result<(), ClaudeDesktopError> {
    let path = config_json_path()?;
    if !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(());
    }
    let mut root: JsonValue = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Ok(()), // 损坏 JSON 不动
    };
    let obj = match root.as_object_mut() {
        Some(o) => o,
        None => return Ok(()),
    };
    let Some(enterprise) = obj.get_mut("enterpriseConfig") else {
        return Ok(());
    };
    let enterprise_obj = match enterprise.as_object_mut() {
        Some(o) => o,
        None => return Ok(()),
    };
    let existing_names: Vec<String> = enterprise_obj.keys().cloned().collect();
    let managed = managed_policy_names(&existing_names);
    let managed_owned: Vec<String> = managed.iter().map(|s| (*s).to_owned()).collect();
    for key in managed_owned {
        enterprise_obj.remove(&key);
    }
    // 也清掉新加的 coworkEgressAllowedHosts(不在 managed 里因为 DESKTOP_CONFIG
    // 7 字段不含它,这里硬编码清)
    enterprise_obj.remove("coworkEgressAllowedHosts");
    if enterprise_obj.is_empty() {
        obj.remove("enterpriseConfig");
    }
    // 顶层 `deploymentMode` 改回 `"clear"`(对照 `registry.py:824 _mac_clear_json_config`)。
    // 这是让 Claude Desktop 切回 1p / 官方账号的关键 sentinel。
    obj.insert("deploymentMode".to_owned(), JsonValue::String("clear".to_owned()));
    let serialized = serde_json::to_string_pretty(&root)?;
    std::fs::write(&path, serialized)?;
    Ok(())
}

// ────────────────────── 高层 apply / clear / status ──────────────────────

/// 单次 apply —— 同时写 plist + config.json + configLibrary 三处。
pub fn apply(input: &ApplyInput<'_>) -> Result<(), ClaudeDesktopError> {
    write_plist(input)?;
    write_config_json(input)?;
    write_config_library(input)?;
    Ok(())
}

/// 单次 clear —— 同时清 plist + config.json + configLibrary 中 managed 字段,
/// 且把 claude_desktop_config.json 顶层 `deploymentMode` 改回 `"clear"`,让
/// Claude Desktop 重新走 1p / 官方账号路径(对照 `registry.py:824`)。
pub fn clear() -> Result<(), ClaudeDesktopError> {
    clear_plist()?;
    clear_config_json()?;
    clear_config_library()?;
    Ok(())
}

/// 状态查询 —— 返回 plist + config.json 中我们写过的字段(masked)。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    pub plist_exists: bool,
    pub config_json_exists: bool,
    pub managed_by_us: bool,
    /// 实际 base_url(可能跟我们配的不一致 —— user 手动改过 plist)。
    pub current_base_url: Option<String>,
    /// 实际 inferenceProvider 字段值。
    pub current_inference_provider: Option<String>,
}

pub fn get_status() -> Result<DesktopStatus, ClaudeDesktopError> {
    let plist_path = plist_path()?;
    let config_path = config_json_path()?;
    let plist_exists = plist_path.exists();
    let config_json_exists = config_path.exists();

    let (mut managed_by_us, mut current_base_url, mut current_inference_provider) =
        (false, None, None);

    if plist_exists {
        if let Ok(PlistValue::Dictionary(d)) = PlistValue::from_file(&plist_path) {
            if let Some(PlistValue::String(s)) = d.get(CCDS_MARKER) {
                if s == "true" {
                    managed_by_us = true;
                }
            }
            if let Some(PlistValue::String(s)) = d.get("inferenceGatewayBaseUrl") {
                current_base_url = Some(s.clone());
            }
            if let Some(PlistValue::String(s)) = d.get("inferenceProvider") {
                current_inference_provider = Some(s.clone());
            }
        }
    }
    if !managed_by_us && config_json_exists {
        if let Ok(text) = std::fs::read_to_string(&config_path) {
            if let Ok(v) = serde_json::from_str::<JsonValue>(&text) {
                if let Some(ent) = v.get("enterpriseConfig") {
                    if ent.get(CCDS_MARKER).and_then(|x| x.as_str()) == Some("true") {
                        managed_by_us = true;
                    }
                    if current_base_url.is_none() {
                        current_base_url = ent
                            .get("inferenceGatewayBaseUrl")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_owned());
                    }
                    if current_inference_provider.is_none() {
                        current_inference_provider = ent
                            .get("inferenceProvider")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_owned());
                    }
                }
            }
        }
    }
    Ok(DesktopStatus {
        plist_exists,
        config_json_exists,
        managed_by_us,
        current_base_url,
        current_inference_provider,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ModelMappings;
    use indexmap::IndexMap;
    use tempfile::TempDir;

    fn sample_provider() -> Provider {
        let mut models = ModelMappings::new();
        // legacy "sonnet" → 标准化到 sonnet_4_6 slot,生成 claude-sonnet-4-6 路由
        models.insert("sonnet".to_owned(), "deepseek-v4-pro".to_owned());
        let mut headers = IndexMap::new();
        headers.insert("x-api-key".to_owned(), "{apiKey}".to_owned());
        Provider {
            id: "deepseek".to_owned(),
            name: "DeepSeek".to_owned(),
            base_url: "https://api.deepseek.com/anthropic".to_owned(),
            auth_scheme: "bearer".to_owned(),
            api_format: "anthropic".to_owned(),
            api_key: "sk-test".to_owned(),
            models,
            base_url_options: Vec::new(),
            base_url_hint: String::new(),
            model_options: IndexMap::new(),
            model_capabilities: IndexMap::new(),
            request_options: IndexMap::new(),
            request_option_presets: IndexMap::new(),
            extra_headers: headers,
            is_builtin: false,
            sort_index: 0,
            extra: IndexMap::new(),
        }
    }

    #[test]
    fn compute_field_values_uses_inputs() {
        let p = sample_provider();
        let input = ApplyInput {
            provider: &p,
            all_providers: &[],
            gateway_api_key: "sk-cas",
            expose_all_models: false,
            gateway_base_url: Some("http://127.0.0.1:18080"),
        };
        let v = compute_field_values(&input);
        assert_eq!(v.get("inferenceProvider").map(String::as_str), Some("gateway"));
        assert_eq!(v.get("inferenceGatewayApiKey").map(String::as_str), Some("sk-cas"));
        assert_eq!(v.get("inferenceGatewayAuthScheme").map(String::as_str), Some("bearer"));
        assert_eq!(
            v.get("inferenceGatewayBaseUrl").map(String::as_str),
            Some("http://127.0.0.1:18080")
        );
        assert_eq!(v.get("isClaudeCodeForDesktopEnabled").map(String::as_str), Some("1"));
        // headers serialize {apiKey} → sk-cas
        assert!(v
            .get("inferenceGatewayHeaders")
            .unwrap()
            .contains("sk-cas"));
        // models serialize 走 desktop_model_entries:name = claude-sonnet-4-6
        // (Claude 白名单),不再含 deepseek-v4-pro 这种上游真实 ID。
        let models_json = v.get("inferenceModels").unwrap();
        assert!(
            models_json.contains(r#""name":"claude-sonnet-4-6""#),
            "expected name=claude-sonnet-4-6 in {models_json}"
        );
        // sourceModel 必须包含上游真实 ID,客户端发请求时用它作 body.model
        assert!(
            models_json.contains(r#""sourceModel":"deepseek-v4-pro""#),
            "expected sourceModel=deepseek-v4-pro in {models_json}"
        );
        // name 字段不能是上游真实 ID(只能是 claude-* 白名单)
        assert!(
            !models_json.contains(r#""name":"deepseek-v4-pro""#),
            "raw upstream ID must NOT appear in name field: {models_json}"
        );
    }

    #[test]
    fn config_json_write_then_clear_isolates_to_enterprise_config() {
        let td = TempDir::new().unwrap();
        let path = td.path().join("claude_desktop_config.json");
        // 起点:user 已有 preferences 段(不属我们管),不能被动
        std::fs::write(
            &path,
            r#"{"preferences": {"sidebarMode": "task"}}"#,
        )
        .unwrap();

        // 直接调内部函数(暴露给测试用 helper:实现里读 path 用 home dir,这里
        // 我们手动模拟同样的逻辑写 + clear,验 enterpriseConfig 段操作正确)。
        let mut root: JsonValue = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        {
            let obj = root.as_object_mut().unwrap();
            let enterprise = obj.entry("enterpriseConfig".to_owned()).or_insert(json!({}));
            let e = enterprise.as_object_mut().unwrap();
            for f in DESKTOP_CONFIG {
                e.insert(f.name.to_owned(), JsonValue::String("test".to_owned()));
            }
            e.insert(CCDS_MARKER.to_owned(), JsonValue::String("true".to_owned()));
        }
        std::fs::write(&path, serde_json::to_string(&root).unwrap()).unwrap();

        // clear:删除 enterprise managed keys
        let mut root2: JsonValue =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        {
            let obj = root2.as_object_mut().unwrap();
            let enterprise = obj.get_mut("enterpriseConfig").unwrap();
            let e = enterprise.as_object_mut().unwrap();
            let keys: Vec<String> = e.keys().cloned().collect();
            for k in managed_policy_names(&keys) {
                let owned = k.to_owned();
                e.remove(&owned);
            }
            if e.is_empty() {
                obj.remove("enterpriseConfig");
            }
        }
        std::fs::write(&path, serde_json::to_string(&root2).unwrap()).unwrap();

        // 验证:preferences 仍在,enterpriseConfig 已清(空 → 删段)
        let final_v: JsonValue =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(final_v.get("preferences").is_some(), "user 字段不动");
        assert!(final_v.get("enterpriseConfig").is_none(), "全 managed 字段后该段清空 → 删");
    }

    #[test]
    fn paths_under_home_dir() {
        // 仅 sanity:路径含 Library/Application Support / Library/Preferences
        let cfg = config_json_path().unwrap();
        let plist = plist_path().unwrap();
        assert!(cfg.to_string_lossy().contains("Library/Application Support/Claude-3p/"));
        assert!(plist.to_string_lossy().contains("Library/Preferences/com.anthropic.claudefordesktop"));
    }
}
