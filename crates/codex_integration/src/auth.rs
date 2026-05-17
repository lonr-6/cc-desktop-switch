//! `~/.codex/auth.json` 读写.
//!
//! 文件结构(常见字段,我们只动 `auth_mode` 与 `OPENAI_API_KEY`,其他字段
//! 透传不动):
//! ```json
//! {
//!   "auth_mode": "apikey",
//!   "OPENAI_API_KEY": "cas_xxx",
//!   "tokens": { ... }            // OpenAI 登录后的 OAuth token
//! }
//! ```

use std::path::Path;

use serde_json::Value;

use crate::toml_sync::write_atomic;
use crate::CodexError;

/// 读 `auth.json`,文件不存在时返回 `{}`(便于增量写)。
pub fn read_auth(path: &Path) -> Result<Value, CodexError> {
    if !path.exists() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    let s = std::fs::read_to_string(path)?;
    if s.trim().is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }
    Ok(serde_json::from_str(&s)?)
}

/// 写 `auth.json`(`indent=2` + 末尾换行,与 Python 行为一致;原子写入)。
pub fn write_auth(path: &Path, value: &Value) -> Result<(), CodexError> {
    let mut s = serde_json::to_string_pretty(value)?;
    s.push('\n');
    write_atomic(path, &s)?;
    // POSIX:把 auth.json 设为 0o600,防止其它用户读到 token
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn read_missing_returns_empty_object() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("auth.json");
        let v = read_auth(&p).unwrap();
        assert_eq!(v, json!({}));
    }

    #[test]
    fn round_trip_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("auth.json");
        let original = json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "cas_test",
            "tokens": {"access": "xyz"}
        });
        write_auth(&p, &original).unwrap();
        let read_back = read_auth(&p).unwrap();
        assert_eq!(read_back, original);
        let raw = std::fs::read_to_string(&p).unwrap();
        assert!(raw.ends_with('\n'), "auth.json 应以换行结尾");
    }

    #[test]
    fn empty_file_treated_as_empty_object() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("auth.json");
        std::fs::write(&p, "").unwrap();
        let v = read_auth(&p).unwrap();
        assert_eq!(v, json!({}));
    }

    #[cfg(unix)]
    #[test]
    fn write_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("auth.json");
        write_auth(&p, &json!({"k": "v"})).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "auth.json 必须仅 owner 可读写");
    }
}
