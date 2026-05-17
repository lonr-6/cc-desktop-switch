//! Apply / restore 主入口.

use serde::{Deserialize, Serialize};

use crate::auth::{read_auth, write_auth};
use crate::model_catalog::{
    catalog_models_for_provider, clear_catalog_models, upsert_catalog_models,
    CODEX_MODEL_CATALOG_KEY,
};
use crate::paths::CodexPaths;
use crate::snapshot::{
    drop_all_snapshots, drop_snapshot, drop_snapshot_by_id, has_snapshot, list_snapshots,
    read_snapshot_auth, read_snapshot_auth_by_id, read_snapshot_config, read_snapshot_config_by_id,
    snapshot_codex_state, snapshot_toml_value_literal,
};
use crate::toml_sync::{sync_root_value, toml_string_literal};
use crate::CodexError;

/// 我们 apply 时实际触碰的 auth 字段(restore 时只动这些,其它字段保留)。
const MANAGED_AUTH_KEYS: &[&str] = &["auth_mode", "OPENAI_API_KEY"];

/// 我们 apply 时实际触碰的 config.toml 根级别字段(restore 时只动这些)。
const MANAGED_TOML_KEYS: &[&str] = &[
    "openai_base_url",
    "model_context_window",
    CODEX_MODEL_CATALOG_KEY,
    "model",
    "model_provider",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyConfig<'a> {
    /// 代理 base URL,例如 `http://127.0.0.1:18080`。
    pub base_url: &'a str,
    /// gateway API key(`cas_...`),会写到 auth.json。空字符串表示移除。
    pub gateway_api_key: &'a str,
    /// 当前 active provider 默认模型是否支持 1M 上下文。
    /// 为 `true` 时 config.toml 会被注入 1M 兼容配置。
    pub supports_1m: bool,
    /// 当前 active provider 的展示名,用于生成 Codex model catalog。
    #[serde(default)]
    pub provider_name: &'a str,
    /// 当前 active provider 的默认真实模型 ID,用于生成 Codex model catalog。
    #[serde(default)]
    pub default_model: &'a str,
    /// 当前 active provider 的模型槽位映射,用于让 catalog 与 proxy 路由一致。
    #[serde(skip)]
    pub model_mappings: Option<&'a serde_json::Value>,
    /// 当前 active provider 的模型能力声明,用于按目标模型声明窗口。
    #[serde(skip)]
    pub model_capabilities: Option<&'a serde_json::Value>,
    /// 应用版本(写入快照 manifest,便于诊断)。
    pub app_version: &'a str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApplyResult {
    pub config_toml_path: String,
    pub auth_json_path: String,
    pub snapshot_taken: bool,
    pub model_context_window_set: bool,
    pub model_catalog_json_set: bool,
}

/// 把 active provider 配置写入 `~/.codex/{config.toml,auth.json}`,
/// 首次写入前自动 snapshot。
pub fn apply_provider(paths: &CodexPaths, cfg: &ApplyConfig) -> Result<ApplyResult, CodexError> {
    // 1. snapshot(幂等;已有快照不会覆盖)
    let snapshot_taken_now = !has_snapshot(paths);
    snapshot_codex_state(paths, cfg.app_version, cfg.provider_name)?;

    // 2. config.toml: openai_base_url
    if cfg.base_url.is_empty() {
        sync_root_value(&paths.config_toml, "openai_base_url", None)?;
    } else {
        let literal = toml_string_literal(cfg.base_url);
        sync_root_value(&paths.config_toml, "openai_base_url", Some(&literal))?;
    }

    // 2b. 强制 model_provider = "openai":Codex CLI 只有在 openai provider 下
    // 才会读 openai_base_url。用户旧 config 里可能残留 model_provider = "custom"
    // (历史教程 / 旧版 CLI 自己写的),配合 [model_providers.custom] 段会把流量
    // 旁路到第三方 base_url,导致我们的 proxy 被绕过。Codex CLI 0.126+ 把端点
    // 从 /v1/responses 切到 /responses,在残留路径上直接表现为 404(issue #178)。
    // 快照已在第 1 步拿到用户原值,restore 时能完整退回。
    sync_root_value(&paths.config_toml, "model_provider", Some("\"openai\""))?;

    // 3. config.toml: model_context_window(旧版兼容) + model_catalog_json(Codex 0.128+)
    //
    // catalog 始终写(2026-05-06):之前只在 `supports_1m=true` 时写,导致非 1M
    // provider(如 Kimi `kimi-k2.6` / MiMo `mimo-v2.5-pro`)在 Codex CLI 模型
    // 选择器里 fallback 到内置 GPT 系列名("GPT-5.5"等),用户看不到真实
    // provider/model。现在每条 provider 都通过 catalog 把 display_name 设成
    // "<provider> / <real-model>",`model_context_window` 仍只在 1M 时设。
    let catalog_literal = toml_string_literal(&paths.model_catalog_json.display().to_string());
    sync_root_value(
        &paths.config_toml,
        CODEX_MODEL_CATALOG_KEY,
        Some(&catalog_literal),
    )?;
    let models = catalog_models_for_provider(
        cfg.provider_name,
        cfg.default_model,
        cfg.supports_1m,
        cfg.model_mappings,
        cfg.model_capabilities,
    );
    upsert_catalog_models(&paths.model_catalog_json, &models)?;
    if cfg.supports_1m {
        sync_root_value(&paths.config_toml, "model_context_window", Some("1000000"))?;
    } else {
        sync_root_value(&paths.config_toml, "model_context_window", None)?;
    }

    // 4. auth.json: auth_mode + OPENAI_API_KEY
    let mut auth = read_auth(&paths.auth_json)?;
    let obj = auth.as_object_mut().expect("read_auth 保证返回 Object");
    if cfg.gateway_api_key.is_empty() {
        obj.remove("OPENAI_API_KEY");
    } else {
        obj.insert(
            "auth_mode".into(),
            serde_json::Value::String("apikey".into()),
        );
        obj.insert(
            "OPENAI_API_KEY".into(),
            serde_json::Value::String(cfg.gateway_api_key.to_owned()),
        );
    }
    write_auth(&paths.auth_json, &auth)?;

    Ok(ApplyResult {
        config_toml_path: paths.config_toml.display().to_string(),
        auth_json_path: paths.auth_json.display().to_string(),
        snapshot_taken: snapshot_taken_now,
        model_context_window_set: cfg.supports_1m,
        model_catalog_json_set: true,
    })
}

/// 基于快照精确还原我们改过的 key,不动用户在我们运行期间手加的内容。
/// 还原成功后清掉快照。
pub fn restore_codex_state(paths: &CodexPaths) -> Result<bool, CodexError> {
    if !has_snapshot(paths) {
        // 没快照时退化为旧版"删除我们的 key"逻辑,与 Python 行为对齐
        clear_managed_codex_state(paths)?;
        return Ok(false);
    }

    let snapshot_config = read_snapshot_config(paths).unwrap_or_default();
    let snapshot_auth = read_snapshot_auth(paths);
    restore_from_snapshot_values(paths, &snapshot_config, &snapshot_auth, RestoreMode::Auto)?;

    drop_snapshot(paths)?;
    clear_catalog_models(&paths.model_catalog_json)?;
    Ok(true)
}

/// 区分两种 restore 流程:
/// - `Auto`:stop app 自动 restore。快照里没有 `model` 时保留当前 CLI 写入的活跃
///   选择(避免擦掉用户用 Codex CLI picker 选过的模型)。
/// - `Manual`:UI 手动选某个 snapshot 恢复。语义是"完全回到那个快照的状态",
///   `model` 也必须严格按快照恢复(没有就移除)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreMode {
    Auto,
    Manual,
}

/// 人工恢复指定快照。恢复成功后默认删除该快照;当 `drop_remaining_snapshots`
/// 为 true 时,按 UI 选择恢复语义清理所有剩余 active/recovery/legacy 快照。
pub fn restore_codex_snapshot(
    paths: &CodexPaths,
    snapshot_id: &str,
    drop_remaining_snapshots: bool,
) -> Result<bool, CodexError> {
    if snapshot_id.trim().is_empty() {
        return restore_codex_state(paths);
    }
    if !list_snapshots(paths).iter().any(|s| s.id == snapshot_id) {
        return Err(CodexError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("snapshot not found: {snapshot_id}"),
        )));
    }
    let snapshot_config = read_snapshot_config_by_id(paths, snapshot_id).unwrap_or_default();
    let snapshot_auth = read_snapshot_auth_by_id(paths, snapshot_id);
    restore_from_snapshot_values(paths, &snapshot_config, &snapshot_auth, RestoreMode::Manual)?;
    if drop_remaining_snapshots {
        drop_all_snapshots(paths)?;
    } else {
        drop_snapshot_by_id(paths, snapshot_id)?;
    }
    clear_catalog_models(&paths.model_catalog_json)?;
    Ok(true)
}

fn clear_managed_codex_state(paths: &CodexPaths) -> Result<(), CodexError> {
    for key in MANAGED_TOML_KEYS {
        sync_root_value(&paths.config_toml, key, None)?;
    }
    clear_catalog_models(&paths.model_catalog_json)?;
    if paths.auth_json.exists() {
        let mut auth = read_auth(&paths.auth_json)?;
        if let Some(obj) = auth.as_object_mut() {
            for key in MANAGED_AUTH_KEYS {
                obj.remove(*key);
            }
        }
        write_auth(&paths.auth_json, &auth)?;
    }
    Ok(())
}

fn restore_from_snapshot_values(
    paths: &CodexPaths,
    snapshot_config: &str,
    snapshot_auth: &serde_json::Value,
    mode: RestoreMode,
) -> Result<(), CodexError> {
    // 1. config.toml:对每个 managed key 用快照里的字面量还原;快照里没有就删。
    //
    // `model` 在 `RestoreMode::Auto`(stop app 自动 restore)下是例外:apply 不写
    // 它,但用户在 app 接管期间可能通过 Codex CLI 模型选择器选过模型,CLI 会把
    // 选择 `model = "..."` 写回 config.toml。若快照里没有 `model`,自动 restore
    // 不应擦掉用户的活跃选择,只在快照里有时还原回原值。
    //
    // `RestoreMode::Manual`(UI 手动选某个 snapshot 恢复)的语义是"完全回到那个
    // 快照的状态",所以 `model` 也必须严格按快照恢复 —— 没有就移除,否则用户选
    // 老备份反而沿用了 post-snapshot 的 model 映射。
    for key in MANAGED_TOML_KEYS {
        let literal = snapshot_toml_value_literal(snapshot_config, key);
        match (*key, literal.as_deref(), mode) {
            ("model", None, RestoreMode::Auto) => continue,
            _ => sync_root_value(&paths.config_toml, key, literal.as_deref())?,
        }
    }

    // 2. auth.json:对每个 managed key,快照里有就改回快照值,没有就 remove
    let mut current = read_auth(&paths.auth_json)?;
    if let Some(obj) = current.as_object_mut() {
        for key in MANAGED_AUTH_KEYS {
            match snapshot_auth.get(*key) {
                Some(v) => {
                    obj.insert((*key).to_owned(), v.clone());
                }
                None => {
                    obj.remove(*key);
                }
            }
        }
    }
    write_auth(&paths.auth_json, &current)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, CodexPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = CodexPaths::from_home_dir(tmp.path());
        (tmp, paths)
    }

    fn read_toml(paths: &CodexPaths) -> String {
        std::fs::read_to_string(&paths.config_toml).unwrap()
    }

    fn read_auth_value(paths: &CodexPaths) -> serde_json::Value {
        read_auth(&paths.auth_json).unwrap()
    }

    fn read_app_config(paths: &CodexPaths) -> serde_json::Value {
        codex_app_transfer_registry::load_raw_config(&paths.model_catalog_json).unwrap()
    }

    #[test]
    fn apply_on_empty_writes_both_files_and_takes_snapshot() {
        let (_t, paths) = setup();
        let result = apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://127.0.0.1:18080",
                gateway_api_key: "cas_test",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v2.0.0-stage2.5",
            },
        )
        .unwrap();
        assert!(result.snapshot_taken);
        assert!(!result.model_context_window_set);
        // catalog 现在始终写(让非 1M provider 也能在 Codex CLI 模型选择器
        // 显示"<provider> / <real-model>"而不是 fallback 到 GPT 内置名)
        assert!(result.model_catalog_json_set);

        let toml = read_toml(&paths);
        assert!(toml.contains("openai_base_url = \"http://127.0.0.1:18080\""));
        assert!(!toml.contains("model_context_window"));
        // model_catalog_json 始终在 config.toml 里
        assert!(toml.contains("model_catalog_json"));

        let auth = read_auth_value(&paths);
        assert_eq!(auth["auth_mode"], "apikey");
        assert_eq!(auth["OPENAI_API_KEY"], "cas_test");
    }

    #[test]
    fn apply_with_supports_1m_writes_model_context_window_and_catalog() {
        let (_t, paths) = setup();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://x",
                gateway_api_key: "k",
                supports_1m: true,
                provider_name: "DeepSeek",
                default_model: "deepseek-v4-pro[1m]",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let toml = read_toml(&paths);
        assert!(toml.contains("model_context_window = 1000000"));
        assert!(toml.contains("model_catalog_json = "));
        assert!(toml.contains(".codex-app-transfer"));
        assert!(toml.contains("config.json"));
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&paths.model_catalog_json).unwrap()).unwrap();
        assert_eq!(catalog["models"][0]["context_window"], 1_000_000);
        assert_eq!(catalog["models"][0]["effective_context_window_percent"], 95);
        assert!(catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["slug"] == "deepseek-v4-pro"));
    }

    #[test]
    fn apply_with_supports_1m_uses_provider_slot_mapping() {
        let (_t, paths) = setup();
        let mappings = json!({
            "default": "deepseek-v4-pro",
            "gpt_5_5": "short-context-model",
            "gpt_5_4": "custom-long-model"
        });
        let capabilities = json!({
            "short-context-model": {"supports1m": false},
            "custom-long-model": {"supports1m": true}
        });

        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://x",
                gateway_api_key: "k",
                supports_1m: true,
                provider_name: "Mixed",
                default_model: "deepseek-v4-pro",
                model_mappings: Some(&mappings),
                model_capabilities: Some(&capabilities),
                app_version: "v",
            },
        )
        .unwrap();

        let catalog = read_app_config(&paths);
        let models = catalog["models"].as_array().unwrap();
        let gpt55 = models.iter().find(|m| m["slug"] == "gpt-5.5").unwrap();
        let gpt54 = models.iter().find(|m| m["slug"] == "gpt-5.4").unwrap();
        let mini = models.iter().find(|m| m["slug"] == "gpt-5.4-mini").unwrap();
        assert_eq!(gpt55["display_name"], "Mixed / short-context-model");
        assert_eq!(gpt55["context_window"], 258_400);
        assert_eq!(gpt54["display_name"], "Mixed / custom-long-model");
        assert_eq!(gpt54["context_window"], 1_000_000);
        assert_eq!(
            mini["display_name"], "Mixed / deepseek-v4-pro",
            "empty slots should document their default fallback target"
        );
        assert_eq!(mini["context_window"], 1_000_000);
    }

    #[test]
    fn apply_without_supports_1m_keeps_catalog_drops_only_context_window() {
        let (_t, paths) = setup();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://x",
                gateway_api_key: "k",
                supports_1m: true,
                provider_name: "DeepSeek",
                default_model: "deepseek-v4-pro",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        assert!(read_app_config(&paths).get("models").is_some());

        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://x",
                gateway_api_key: "k",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();

        // 现在 catalog 始终写,即使 supports_1m=false 也保留(2026-05-06):
        // - model_context_window 仍按 supports_1m 切换:这条只在 1M 时设
        // - model_catalog_json 与顶层 "models" 数组不再被清掉,Codex CLI
        //   能继续从 catalog 读到正确的 "<provider> / <real-model>" 显示
        let toml = read_toml(&paths);
        assert!(!toml.contains("model_context_window = "));
        assert!(toml.contains(CODEX_MODEL_CATALOG_KEY));
        let models = read_app_config(&paths)
            .get("models")
            .and_then(|v| v.as_array())
            .cloned()
            .expect("models 数组应保留");
        assert!(
            !models.is_empty(),
            "catalog 始终写,至少包含 default 模型条目"
        );
    }

    #[test]
    fn apply_preserves_user_other_toml_and_auth_fields() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            "# my comment\napi_key = \"k\"\n[profiles]\nfoo = 1\n",
        )
        .unwrap();
        std::fs::write(
            &paths.auth_json,
            "{\"tokens\":{\"access\":\"xyz\"},\"OPENAI_API_KEY\":\"old\"}\n",
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://up",
                gateway_api_key: "cas_new",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let toml = read_toml(&paths);
        assert!(toml.contains("# my comment"));
        assert!(toml.contains("api_key = \"k\""));
        assert!(toml.contains("openai_base_url = \"http://up\""));
        assert!(toml.contains("[profiles]"));
        assert!(toml.contains("foo = 1"));
        let auth = read_auth_value(&paths);
        assert_eq!(auth["OPENAI_API_KEY"], "cas_new");
        assert_eq!(auth["tokens"]["access"], "xyz", "用户 tokens 不应被动");
    }

    #[test]
    fn restore_with_snapshot_brings_back_original_values() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        // 用户原本的状态:有 base_url 和 auth.OPENAI_API_KEY
        std::fs::write(
            &paths.config_toml,
            "openai_base_url = \"https://api.openai.com/v1\"\nmodel = \"gpt-5.5\"\n[profiles]\nfoo = 1\n",
        )
        .unwrap();
        std::fs::write(
            &paths.auth_json,
            "{\"OPENAI_API_KEY\":\"sk-original\",\"tokens\":{\"a\":1}}\n",
        )
        .unwrap();
        // apply 我们的代理配置
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://127.0.0.1:18080",
                gateway_api_key: "cas_proxy",
                supports_1m: true,
                provider_name: "DeepSeek",
                default_model: "deepseek-v4-pro",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        // 模拟 Codex 在接管期间把 UI 模型选择写成第三方映射模型。
        sync_root_value(&paths.config_toml, "model", Some("\"deepseek-v4-pro\"")).unwrap();
        // 还原
        let restored = restore_codex_state(&paths).unwrap();
        assert!(restored, "有快照时 restore 应返回 true");

        let toml = read_toml(&paths);
        assert!(
            toml.contains("openai_base_url = \"https://api.openai.com/v1\""),
            "base_url 应还原为原始 OpenAI 地址"
        );
        assert!(
            !toml.contains("model_context_window"),
            "原状态没有 1M 字段,还原后也不应有"
        );
        assert!(
            toml.contains("model = \"gpt-5.5\""),
            "Codex 模型选择应还原为用户原值"
        );
        assert!(toml.contains("[profiles]"), "用户的 [profiles] 应保留");

        let auth = read_auth_value(&paths);
        assert_eq!(auth["OPENAI_API_KEY"], "sk-original");
        assert_eq!(auth["tokens"]["a"], 1);
        assert!(
            auth.get("auth_mode").is_none(),
            "原状态没有 auth_mode,还原后应不存在"
        );

        assert!(!has_snapshot(&paths), "restore 完成后应清掉快照");
        assert!(
            read_app_config(&paths).get("models").is_none(),
            "restore 应清理本应用写入的顶层 catalog models"
        );
    }

    #[test]
    fn restore_with_snapshot_restores_user_model_catalog_json_key() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            "model_catalog_json = \"/tmp/user-catalog.json\"\n",
        )
        .unwrap();

        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://127.0.0.1:18080",
                gateway_api_key: "cas_proxy",
                supports_1m: true,
                provider_name: "DeepSeek",
                default_model: "deepseek-v4-pro",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        assert!(read_toml(&paths).contains(".codex-app-transfer"));
        assert!(read_app_config(&paths).get("models").is_some());

        restore_codex_state(&paths).unwrap();

        let toml = read_toml(&paths);
        assert!(toml.contains("model_catalog_json = \"/tmp/user-catalog.json\""));
        assert!(read_app_config(&paths).get("models").is_none());
    }

    #[test]
    fn restore_without_snapshot_falls_back_to_remove_managed_keys() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            "openai_base_url = \"http://leftover\"\nmodel_context_window = 1000000\nmodel_catalog_json = \"leftover.json\"\nmodel = \"deepseek-v4-pro\"\nmodel_provider = \"codex-app-transfer\"\nfoo = 1\n",
        )
        .unwrap();
        codex_app_transfer_registry::save_raw_config(
            &paths.model_catalog_json,
            &json!({
                "version": "1.0.4",
                "models": [{"slug": "gpt-5.5"}],
                "settings": {"theme": "default"}
            }),
        )
        .unwrap();
        std::fs::write(
            &paths.auth_json,
            "{\"auth_mode\":\"apikey\",\"OPENAI_API_KEY\":\"leftover\",\"keep\":1}\n",
        )
        .unwrap();
        let restored = restore_codex_state(&paths).unwrap();
        assert!(!restored, "没有快照时返回 false");
        let toml = read_toml(&paths);
        assert!(!toml.contains("openai_base_url"));
        assert!(!toml.contains("model_context_window"));
        assert!(!toml.contains(CODEX_MODEL_CATALOG_KEY));
        assert!(!toml.contains("model = "));
        assert!(!toml.contains("model_provider = "));
        assert!(toml.contains("foo = 1"));
        assert!(read_app_config(&paths).get("models").is_none());
        let auth = read_auth_value(&paths);
        assert!(auth.get("OPENAI_API_KEY").is_none());
        assert!(auth.get("auth_mode").is_none());
        assert_eq!(auth["keep"], 1);
    }

    #[test]
    fn restore_snapshot_by_id_restores_chosen_backup_and_cleans_all_snapshots() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            "openai_base_url = \"active-original\"\nmodel = \"gpt-5.5\"\n",
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://active-managed",
                gateway_api_key: "cas_active",
                supports_1m: false,
                provider_name: "Active",
                default_model: "active-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v-active",
            },
        )
        .unwrap();

        let recovery_dir = paths.recovery_snapshots_dir.join("older-backup");
        std::fs::create_dir_all(&recovery_dir).unwrap();
        std::fs::write(
            recovery_dir.join("config.toml"),
            "openai_base_url = \"older-original\"\nmodel = \"gpt-5.4\"\n",
        )
        .unwrap();
        std::fs::write(recovery_dir.join("auth.json"), "{\"keep\":1}\n").unwrap();
        std::fs::write(
            recovery_dir.join("manifest.json"),
            json!({
                "schema_version": 2,
                "snapshot_id": "older-backup",
                "session_id": "older-session",
                "snapshot_at": "2026-05-15T02:00:00",
                "config_existed": true,
                "auth_existed": true,
                "app_version": "v-old",
                "provider_name": "Older"
            })
            .to_string(),
        )
        .unwrap();

        sync_root_value(
            &paths.config_toml,
            "openai_base_url",
            Some("\"http://managed\""),
        )
        .unwrap();
        sync_root_value(&paths.config_toml, "model", Some("\"deepseek-v4-pro\"")).unwrap();

        let restored = restore_codex_snapshot(&paths, "older-backup", true).unwrap();
        assert!(restored);
        let toml = read_toml(&paths);
        assert!(toml.contains("openai_base_url = \"older-original\""));
        assert!(toml.contains("model = \"gpt-5.4\""));
        assert!(
            crate::snapshot::list_snapshots(&paths).is_empty(),
            "manual restore should clear all remaining backups after success"
        );
    }

    #[test]
    fn apply_then_apply_again_does_not_overwrite_original_snapshot() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(&paths.config_toml, "openai_base_url = \"original\"\n").unwrap();
        // 第一次 apply
        let r1 = apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://first",
                gateway_api_key: "cas_first",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        assert!(r1.snapshot_taken);
        // 第二次 apply
        let r2 = apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://second",
                gateway_api_key: "cas_second",
                supports_1m: true,
                provider_name: "DeepSeek",
                default_model: "deepseek-v4-pro",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        assert!(!r2.snapshot_taken, "第二次不应再 snapshot");
        // restore 应回到 ORIGINAL,不是 first
        restore_codex_state(&paths).unwrap();
        let toml = read_toml(&paths);
        assert!(toml.contains("openai_base_url = \"original\""));
    }

    #[test]
    fn apply_with_empty_gateway_api_key_removes_key() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.auth_json,
            "{\"OPENAI_API_KEY\":\"present\",\"keep\":1}\n",
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://x",
                gateway_api_key: "",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let auth = read_auth_value(&paths);
        assert!(auth.get("OPENAI_API_KEY").is_none());
        assert_eq!(auth["keep"], 1);
    }

    #[test]
    fn apply_with_empty_base_url_removes_key() {
        let (_t, paths) = setup();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "",
                gateway_api_key: "k",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let toml = std::fs::read_to_string(&paths.config_toml).unwrap_or_default();
        assert!(!toml.contains("openai_base_url"));
    }

    /// 防回归:若用户的 config.toml 里某 key 含 `key_alt = ...` 这种前缀同名行,
    /// apply / restore 都不应误改它(已由 toml_sync 单测覆盖,这里再做端到端校验)。
    #[test]
    fn similar_prefixed_keys_are_not_touched() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            "openai_base_url_alt = \"keep\"\nopenai_base_url = \"old\"\n",
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://new",
                gateway_api_key: "k",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let toml = read_toml(&paths);
        assert!(toml.contains("openai_base_url_alt = \"keep\""));
        assert!(toml.contains("openai_base_url = \"http://new\""));
    }

    #[test]
    fn auth_json_unaffected_when_user_has_oauth_tokens() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        let oauth_blob = json!({
            "tokens": {
                "access_token": "ya29.xxx",
                "refresh_token": "1//xxx",
                "expires_at": 9999999999i64
            }
        });
        std::fs::write(
            &paths.auth_json,
            serde_json::to_string_pretty(&oauth_blob).unwrap(),
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://x",
                gateway_api_key: "cas_test",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let auth = read_auth_value(&paths);
        assert_eq!(auth["tokens"]["access_token"], "ya29.xxx");
        assert_eq!(auth["OPENAI_API_KEY"], "cas_test");
        // restore 应把 OAuth 块完整保留,把 OPENAI_API_KEY 删除(原来没有)
        restore_codex_state(&paths).unwrap();
        let auth_after = read_auth_value(&paths);
        assert_eq!(auth_after["tokens"]["access_token"], "ya29.xxx");
        assert!(auth_after.get("OPENAI_API_KEY").is_none());
        assert!(auth_after.get("auth_mode").is_none());
    }

    /// issue #178:用户旧 config 残留 `model_provider = "custom"` + `[model_providers.custom]`
    /// 段时,apply 必须把 `model_provider` 拉到 `"openai"`,否则 Codex CLI 把流量
    /// 旁路到 custom block 的 base_url,绕过 proxy(0.126+ 表现为 /v1/responses 404)。
    #[test]
    fn apply_normalizes_legacy_custom_model_provider() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            concat!(
                "model_provider = \"custom\"\n",
                "openai_base_url = \"https://stale.example.com/v1\"\n",
                "[model_providers.custom]\n",
                "name = \"Custom\"\n",
                "base_url = \"https://stale.example.com/v1\"\n",
            ),
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://127.0.0.1:18080",
                gateway_api_key: "cas_test",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        let toml = read_toml(&paths);
        assert!(
            toml.contains("model_provider = \"openai\""),
            "apply 必须把 model_provider 拉正到 openai,实际 toml:\n{toml}"
        );
        assert!(
            toml.contains("openai_base_url = \"http://127.0.0.1:18080\""),
            "openai_base_url 应指向 app proxy"
        );
        assert!(
            toml.contains("[model_providers.custom]"),
            "[model_providers.custom] 不是我们管的段,保留即可"
        );

        // restore 必须把 model_provider 退回到用户原值 "custom"。
        restore_codex_state(&paths).unwrap();
        let restored = read_toml(&paths);
        assert!(
            restored.contains("model_provider = \"custom\""),
            "restore 应把 model_provider 退回为用户原值,实际 toml:\n{restored}"
        );
        assert!(
            restored.contains("openai_base_url = \"https://stale.example.com/v1\""),
            "openai_base_url 也应退回用户原值"
        );
    }

    /// UI 手动选某个 snapshot 恢复时,语义是"完全回到那个快照的状态"。即使快照里
    /// 没有 `model`,也必须把当前 `model` 移除(否则用户选老备份反而沿用了
    /// post-snapshot 的 model 映射)。RestoreMode::Auto 才保留 CLI 写入的选择,
    /// Manual 不应享受这个例外。
    #[test]
    fn manual_restore_strictly_matches_snapshot_even_for_model_key() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(&paths.config_toml, "openai_base_url = \"original\"\n").unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://127.0.0.1:18080",
                gateway_api_key: "cas_test",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        // 模拟接管期间 CLI picker 写入的活跃 model。
        sync_root_value(&paths.config_toml, "model", Some("\"deepseek-v4-pro\"")).unwrap();

        // 拿到 active snapshot id,走手动恢复路径。
        let snapshots = crate::snapshot::list_snapshots(&paths);
        let snapshot_id = snapshots
            .iter()
            .find(|s| s.kind == "active")
            .expect("apply 应创建 active snapshot")
            .id
            .clone();
        restore_codex_snapshot(&paths, &snapshot_id, false).unwrap();

        let toml = read_toml(&paths);
        assert!(
            !toml.contains("model = "),
            "manual restore 必须严格按快照恢复;快照无 model 时应移除当前值,实际 toml:\n{toml}"
        );
        assert!(
            toml.contains("openai_base_url = \"original\""),
            "openai_base_url 也按快照退回"
        );
    }

    /// 用户首次安装时 config.toml 没有 `model`,apply 也不写 `model`。但用户在
    /// Codex CLI 模型选择器里选过模型后,CLI 会把 `model = "..."` 写回 config.toml。
    /// restore 时快照里没有 `model`,我们不应把 CLI 写入的活跃选择擦掉。
    #[test]
    fn restore_preserves_user_model_picked_via_codex_cli() {
        let (_t, paths) = setup();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        std::fs::write(
            &paths.config_toml,
            "openai_base_url = \"https://api.openai.com/v1\"\n",
        )
        .unwrap();
        apply_provider(
            &paths,
            &ApplyConfig {
                base_url: "http://127.0.0.1:18080",
                gateway_api_key: "cas_proxy",
                supports_1m: false,
                provider_name: "Mock",
                default_model: "mock-model",
                model_mappings: None,
                model_capabilities: None,
                app_version: "v",
            },
        )
        .unwrap();
        // 模拟 Codex CLI picker 在 app 接管期间把 model 写回 config.toml。
        sync_root_value(&paths.config_toml, "model", Some("\"kimi-k2.6\"")).unwrap();

        restore_codex_state(&paths).unwrap();
        let toml = read_toml(&paths);
        assert!(
            toml.contains("model = \"kimi-k2.6\""),
            "快照里没有 model 时,restore 应保留 CLI 写入的活跃选择,实际 toml:\n{toml}"
        );
    }
}
