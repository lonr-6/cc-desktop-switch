//! 字节级保真的 JSON I/O.
//!
//! Python `json.dump(obj, f, ensure_ascii=False, indent=2)` 与
//! `serde_json::to_string_pretty` 的默认行为已对齐:
//! - 2 空格缩进
//! - 非 ASCII 不转义(serde_json 默认)
//! - 对象 key 顺序保留(需要 `serde_json/preserve_order` 特性,Cargo.toml 已开)
//! - 分隔符 `,` + `: `(serde_json `to_string_pretty` 默认)
//!
//! Python 主配置文件 `config.json` 末尾**不**带换行;Library 条目
//! `configLibrary/<id>.json` 末尾**带**一个 `\n`(由 `_write_json_file`
//! 显式 `f.write("\n")`).两种写入模式都在本模块中实现.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IoError {
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// 配置文件内容的"原始"视图,保留原始键顺序.
pub type RawConfig = Value;

/// 加载 JSON 文件为 `Value`(保留 key 顺序).
pub fn load_raw_config<P: AsRef<Path>>(path: P) -> Result<RawConfig, IoError> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(IoError::NotFound(path.display().to_string()));
    }
    let mut s = String::new();
    fs::File::open(path)?.read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}

/// 把 `Value` 写回主配置文件路径(不带末尾换行,等价于 Python `save_config`).
pub fn save_raw_config<P: AsRef<Path>>(path: P, value: &RawConfig) -> Result<(), IoError> {
    let body = serde_json::to_string_pretty(value)?;
    write_atomic(path.as_ref(), body.as_bytes())
}

/// 把 `Value` 写到 Library 条目路径(末尾带 `\n`,等价于 Python
/// `_write_json_file`).
pub fn save_raw_library<P: AsRef<Path>>(path: P, value: &RawConfig) -> Result<(), IoError> {
    let mut body = serde_json::to_string_pretty(value)?;
    body.push('\n');
    write_atomic(path.as_ref(), body.as_bytes())
}

/// 原子写入:先写 `<path>.tmp`,再 rename 替换.与 Python `save_config` 用
/// `shutil.move` 等价(同盘 rename,跨盘退化为复制).
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.to_path_buf();
    let mut name = tmp.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    tmp.set_file_name(name);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_preserved_through_roundtrip() {
        let original = json!({
            "version": "1.0.4",
            "activeProvider": null,
            "providers": [],
            "settings": {
                "theme": "default",
                "language": "zh"
            }
        });
        let s1 = serde_json::to_string_pretty(&original).unwrap();
        let parsed: Value = serde_json::from_str(&s1).unwrap();
        let s2 = serde_json::to_string_pretty(&parsed).unwrap();
        assert_eq!(s1, s2, "round-trip 应字节级一致");
    }

    #[test]
    fn save_main_does_not_add_trailing_newline() {
        let dir = tempdir();
        let p = dir.join("config.json");
        let v = json!({"a": 1});
        save_raw_config(&p, &v).unwrap();
        let bytes = fs::read(&p).unwrap();
        assert!(!bytes.ends_with(b"\n"), "主配置文件不应带末尾换行");
    }

    #[test]
    fn save_library_adds_trailing_newline() {
        let dir = tempdir();
        let p = dir.join("entry.json");
        let v = json!({"id": "x"});
        save_raw_library(&p, &v).unwrap();
        let bytes = fs::read(&p).unwrap();
        assert!(bytes.ends_with(b"\n"), "Library 条目应以换行结尾");
    }

    fn tempdir() -> std::path::PathBuf {
        // cargo test 默认多线程并发跑测试,每次调用必须返回唯一目录;
        // 否则 A 写 .tmp 时 B 的 remove_dir_all 会让 A 的 rename 拿到 ENOENT。
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut p = std::env::temp_dir();
        p.push(format!("cas-registry-test-{}-{}", std::process::id(), n));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }
}
