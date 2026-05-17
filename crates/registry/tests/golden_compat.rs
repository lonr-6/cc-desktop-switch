//! Golden fixture 契约测试 —— Rust 读 commit 在仓库的 fixture, 再写回
//! 必须字节级一致 + typed Config 能消化字段值.
//!
//! Fixture 由 `cargo run -p xtask -- gen-fixtures` 产出 (Phase 3 起,
//! 之前是 `scripts/gen_registry_fixtures.py`, 已删), 目录:
//! `tests/replay/fixtures/registry/`. 权威源是 git: 改 schema/数据 →
//! 重新 gen-fixtures → commit 新 golden, CI 反向 diff (ci.yml) 强制
//! 这一闭环.
//!
//! 命名约定:
//! - `library_*.json` 走 Library 写入路径(末尾保留 `\n`)
//! - 其它走主配置写入路径(末尾**不**带 `\n`)
//!
//! 任何 schema / 序列化器变更只要破坏字节级 round-trip,这套测试都会红.

use std::path::{Path, PathBuf};

use codex_app_transfer_registry::{
    builtin_presets, load_raw_config, raw_io::save_raw_library, save_raw_config,
};
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("..")
        .join("..")
        .join("tests")
        .join("replay")
        .join("fixtures")
        .join("registry")
}

fn read_bytes(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("read {} failed: {e}", p.display()))
}

fn tmp_target(name: &str) -> PathBuf {
    let mut t = std::env::temp_dir();
    t.push(format!(
        "cas-registry-roundtrip-{}-{}",
        std::process::id(),
        name
    ));
    let _ = std::fs::remove_file(&t);
    t
}

/// 检查 round-trip 字节级一致;`is_library` 决定是否走带末尾 `\n` 的写入器.
fn assert_byte_identical_roundtrip(fixture_name: &str, is_library: bool) {
    let src = fixtures_dir().join(fixture_name);
    let original = read_bytes(&src);
    let parsed = load_raw_config(&src).expect("load_raw_config 失败");
    let dst = tmp_target(fixture_name);
    if is_library {
        save_raw_library(&dst, &parsed).expect("save_raw_library 失败");
    } else {
        save_raw_config(&dst, &parsed).expect("save_raw_config 失败");
    }
    let written = read_bytes(&dst);
    let _ = std::fs::remove_file(&dst);
    if original != written {
        let original_s = String::from_utf8_lossy(&original);
        let written_s = String::from_utf8_lossy(&written);
        panic!(
            "[{fixture_name}] round-trip 字节不一致\n--- golden ({} bytes)\n{original_s}\n--- rust ({} bytes)\n{written_s}",
            original.len(),
            written.len()
        );
    }
}

#[test]
fn default_config_roundtrip() {
    assert_byte_identical_roundtrip("default_config.json", false);
}

#[test]
fn with_provider_roundtrip() {
    assert_byte_identical_roundtrip("with_provider.json", false);
}

#[test]
fn builtin_presets_roundtrip() {
    assert_byte_identical_roundtrip("builtin_presets.json", false);
}

#[test]
fn library_entry_roundtrip() {
    assert_byte_identical_roundtrip("library_entry.json", true);
}

/// 验证 Rust embedded 的内置预设与 commit 在仓库的 golden fixture
/// (parse 后) 在 JSON 值层面完全相等. 这是"Rust 数据源 vs commit golden"
/// 的对照, 与上面"golden 序列化 → Rust round-trip"是不同维度的检查.
#[test]
fn rust_embedded_presets_match_committed_fixture() {
    let p = fixtures_dir().join("builtin_presets.json");
    let golden: Value = load_raw_config(&p).expect("read committed presets fixture");
    let golden_arr = golden.as_array().expect("presets fixture must be array");
    let rs = builtin_presets();
    assert_eq!(
        rs.len(),
        golden_arr.len(),
        "presets 数量不一致:rust={}, golden={}",
        rs.len(),
        golden_arr.len()
    );
    for (i, (a, b)) in rs.iter().zip(golden_arr.iter()).enumerate() {
        assert_eq!(
            a,
            b,
            "preset[{i}] 不一致:\n  rust = {}\n  golden = {}",
            serde_json::to_string_pretty(a).unwrap(),
            serde_json::to_string_pretty(b).unwrap()
        );
    }
}

/// 同时也用类型化 schema 反序列化主配置文件,证明 Rust 的 typed view 能消化
/// golden 输出而不丢字段(extra 走 flatten 透传).
#[test]
fn typed_config_can_parse_default() {
    use codex_app_transfer_registry::Config;
    let p = fixtures_dir().join("default_config.json");
    let txt = std::fs::read_to_string(&p).unwrap();
    let cfg: Config = serde_json::from_str(&txt).expect("typed parse failed");
    assert_eq!(cfg.version, "1.0.4");
    assert_eq!(cfg.active_provider, None);
    assert_eq!(cfg.gateway_api_key, None);
    assert!(cfg.providers.is_empty());
    assert_eq!(cfg.settings.proxy_port, 18080);
    assert_eq!(cfg.settings.update_url.contains("Cmochance"), true);
}

#[test]
fn typed_config_can_parse_with_provider() {
    use codex_app_transfer_registry::Config;
    let p = fixtures_dir().join("with_provider.json");
    let txt = std::fs::read_to_string(&p).unwrap();
    let cfg: Config = serde_json::from_str(&txt).expect("typed parse failed");
    assert_eq!(cfg.providers.len(), 1);
    let pr = &cfg.providers[0];
    assert_eq!(pr.id, "fixture-provider");
    assert_eq!(pr.api_format, "openai_chat");
    // 6 个槽位都应存在
    assert_eq!(pr.models.len(), 6);
    assert_eq!(pr.models["default"], "fixture-default");
    assert_eq!(pr.models["gpt_5_5"], "fixture-gpt-5.5");
}
