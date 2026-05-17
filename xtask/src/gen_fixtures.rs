//! `xtask gen-fixtures` — 生成 4 份 registry fixture JSON.
//!
//! 替代 `scripts/gen_registry_fixtures.py` (Phase 1 删 backend/ 后已死).
//!
//! 输出位置: `tests/replay/fixtures/registry/`
//! - `default_config.json`   `Config::default()`,无末尾换行
//! - `with_provider.json`    default + 1 个合成中文 provider
//! - `builtin_presets.json`  `builtin_presets()` Vec
//! - `library_entry.json`    1 个 library 条目,**末尾带 `\n`**
//!
//! 字节级硬要求: `cargo run -p xtask -- gen-fixtures &&
//! git diff --exit-code -- tests/replay/fixtures/registry/` 必须干净。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use codex_app_transfer_registry::{builtin_presets, empty_model_mappings, Config, Provider};
use indexmap::IndexMap;
use serde_json::Value;

#[derive(ClapArgs)]
pub struct Args {
    /// 输出目录(默认 tests/replay/fixtures/registry, 相对仓库根)
    #[arg(long, default_value = "tests/replay/fixtures/registry")]
    out: PathBuf,
}

pub fn run(args: Args) -> Result<()> {
    let root = repo_root()?;
    let out_dir = root.join(&args.out);
    std::fs::create_dir_all(&out_dir)
        .with_context(|| format!("创建输出目录: {}", out_dir.display()))?;

    write_main_config(
        &out_dir.join("default_config.json"),
        &default_config_value()?,
    )?;
    write_main_config(&out_dir.join("with_provider.json"), &with_provider_value()?)?;
    write_main_config(
        &out_dir.join("builtin_presets.json"),
        &builtin_presets_value()?,
    )?;
    write_library_entry(&out_dir.join("library_entry.json"), &library_entry_value()?)?;

    println!("wrote 4 fixtures into {}", args.out.display());
    Ok(())
}

/// 主配置写入: `serde_json::to_string_pretty` 无末尾换行, 与
/// `crates/registry/src/raw_io.rs::save_raw_config` 行为一致.
fn write_main_config(path: &Path, value: &Value) -> Result<()> {
    let body = serde_json::to_string_pretty(value)?;
    std::fs::write(path, body.as_bytes()).with_context(|| format!("写入: {}", path.display()))?;
    Ok(())
}

/// Library 条目写入: `to_string_pretty` + 末尾追加 `\n`,与
/// `save_raw_library` 行为一致.
fn write_library_entry(path: &Path, value: &Value) -> Result<()> {
    let mut body = serde_json::to_string_pretty(value)?;
    body.push('\n');
    std::fs::write(path, body.as_bytes()).with_context(|| format!("写入: {}", path.display()))?;
    Ok(())
}

fn default_config_value() -> Result<Value> {
    Ok(serde_json::to_value(Config::default())?)
}

fn with_provider_value() -> Result<Value> {
    // 复刻 gen_registry_fixtures.py 的合成 provider:6 槽位中 default +
    // gpt_5_5 填值, 其余空字符串。empty_model_mappings 已按 MODEL_ORDER
    // 顺序填好空槽位, 直接覆盖需要的两个键 (key 在原位, 不会重排)。
    let mut models = empty_model_mappings();
    models.insert("default".to_string(), "fixture-default".to_string());
    models.insert("gpt_5_5".to_string(), "fixture-gpt-5.5".to_string());

    let provider = Provider {
        id: "fixture-provider".to_string(),
        name: "Fixture · 合成 Provider".to_string(),
        base_url: "https://fixture.invalid/v1".to_string(),
        auth_scheme: "bearer".to_string(),
        api_format: "openai_chat".to_string(),
        api_key: "<redacted>".to_string(),
        models,
        extra_headers: IndexMap::new(),
        model_capabilities: IndexMap::new(),
        request_options: IndexMap::new(),
        is_builtin: false,
        sort_index: 0,
        extra: IndexMap::new(),
    };

    let mut cfg = Config::default();
    cfg.active_provider = Some("fixture-provider".to_string());
    cfg.gateway_api_key = Some("cas_<redacted>".to_string());
    cfg.providers = vec![provider];
    Ok(serde_json::to_value(cfg)?)
}

fn builtin_presets_value() -> Result<Value> {
    Ok(serde_json::to_value(builtin_presets())?)
}

fn library_entry_value() -> Result<Value> {
    let mut models = empty_model_mappings();
    models.insert("default".to_string(), "library-default".to_string());
    let entry = Provider {
        id: "library-fixture".to_string(),
        name: "Library Fixture".to_string(),
        base_url: "https://library.invalid/v1".to_string(),
        auth_scheme: "bearer".to_string(),
        api_format: "openai_chat".to_string(),
        api_key: "<redacted>".to_string(),
        models,
        extra_headers: IndexMap::new(),
        model_capabilities: IndexMap::new(),
        request_options: IndexMap::new(),
        is_builtin: false,
        sort_index: 0,
        extra: IndexMap::new(),
    };
    Ok(serde_json::to_value(entry)?)
}

/// 仓库根目录: xtask/Cargo.toml 的 parent 的 parent.
fn repo_root() -> Result<PathBuf> {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Ok(PathBuf::from(manifest)
        .parent()
        .context("xtask 的 parent 目录不存在")?
        .to_path_buf())
}
