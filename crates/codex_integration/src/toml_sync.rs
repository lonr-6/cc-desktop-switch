//! `~/.codex/config.toml` 根级别 key-value 同步.
//!
//! **不用真 TOML parser**:Codex CLI 用户可能在文件里写注释 / 多 section,
//! 真 parser round-trip 会丢这些。我们做最小改动:
//! 1. 删除所有现有的 root-level `<key> = ...` 行
//! 2. 在第一个 `[section]` 标题之前插入新行;没有节则追加到末尾
//!
//! 1:1 对齐 Python `_sync_codex_toml_value`(`backend/registry.py:872-903`)。

use std::path::Path;

use crate::CodexError;

/// 同步一个根级别 key:
/// - `Some(raw_value)` → 把 `<key> = <raw_value>` 写到第一个 section 之前(若存在),
///   否则追加到末尾。`raw_value` 必须是已经按 TOML 字面量格式化好的字符串
///   (字符串值要传 `"\"abc\""` 含引号;整数传 `"1000000"`;布尔传 `"true"`)。
/// - `None` → 删除该 key 的所有出现位置。
///
/// 文件不存在时按"空内容"处理。写入用 `write_atomic`(tmp + rename)。
pub fn sync_root_value(
    config_toml_path: &Path,
    key: &str,
    raw_value: Option<&str>,
) -> Result<(), CodexError> {
    let current = read_or_empty(config_toml_path)?;
    let new_content = sync_root_value_in_memory(&current, key, raw_value);
    write_atomic(config_toml_path, &new_content)?;
    Ok(())
}

/// 纯函数:对一段 TOML 文本做同步,返回新文本。**与 IO 解耦,便于单测**。
pub fn sync_root_value_in_memory(current: &str, key: &str, raw_value: Option<&str>) -> String {
    let mut new_lines: Vec<String> = Vec::new();
    let mut inserted = false;

    for line in current.lines() {
        // 删除已有的 root-level `<key> ...` 行
        let stripped = line.trim_start();
        if line_matches_root_key(stripped, key) {
            continue;
        }
        // 插入点:第一个 section header 之前
        if !inserted && raw_value.is_some() && stripped.starts_with('[') {
            if let Some(v) = raw_value {
                new_lines.push(format!("{key} = {v}"));
            }
            inserted = true;
        }
        new_lines.push(line.to_owned());
    }

    if !inserted {
        if let Some(v) = raw_value {
            new_lines.push(format!("{key} = {v}"));
        }
    }

    let mut result = new_lines.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// 把 Rust 字符串转 TOML 双引号字面量;用 serde_json 做转义(TOML basic
/// string 与 JSON string 的转义规则在常见字符上一致)。
pub fn toml_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
}

fn line_matches_root_key(stripped_left: &str, key: &str) -> bool {
    // Python 用 `stripped.startswith(key) and "=" in stripped`,我们对齐。
    // 同时谨慎一点:`key` 之后必须是空白 / `=`,避免 `model_provider` 把
    // `model_provider_id` 也误删。
    if !stripped_left.starts_with(key) {
        return false;
    }
    if !stripped_left.contains('=') {
        return false;
    }
    let after_key = &stripped_left[key.len()..];
    matches!(after_key.chars().next(), Some(c) if c == '=' || c.is_ascii_whitespace())
}

fn read_or_empty(path: &Path) -> Result<String, CodexError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn write_atomic(path: &Path, content: &str) -> Result<(), CodexError> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.to_path_buf();
    let mut name = tmp.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    tmp.set_file_name(name);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_into_empty_appends_with_trailing_newline() {
        let out = sync_root_value_in_memory("", "openai_base_url", Some("\"http://x\""));
        assert_eq!(out, "openai_base_url = \"http://x\"\n");
    }

    #[test]
    fn replace_existing_key() {
        let input = "openai_base_url = \"old\"\n";
        let out = sync_root_value_in_memory(input, "openai_base_url", Some("\"new\""));
        assert_eq!(out, "openai_base_url = \"new\"\n");
    }

    #[test]
    fn insert_before_first_section_header() {
        let input = "# my comment\n[profiles]\napi_key = \"x\"\n";
        let out = sync_root_value_in_memory(input, "openai_base_url", Some("\"http://up\""));
        assert!(out.contains("openai_base_url = \"http://up\""));
        assert!(out.starts_with("# my comment\n"));
        assert!(out.contains("[profiles]"));
        // 确保插在 [profiles] 之前
        let idx_key = out.find("openai_base_url").unwrap();
        let idx_section = out.find("[profiles]").unwrap();
        assert!(idx_key < idx_section, "应插入在 [profiles] 之前");
    }

    #[test]
    fn delete_removes_all_instances() {
        let input = "openai_base_url = \"a\"\nfoo = 1\nopenai_base_url = \"b\"\n[s]\nbar = 2\n";
        let out = sync_root_value_in_memory(input, "openai_base_url", None);
        assert!(!out.contains("openai_base_url"));
        assert!(out.contains("foo = 1"));
        assert!(out.contains("bar = 2"));
        assert!(out.contains("[s]"));
    }

    #[test]
    fn does_not_touch_keys_with_same_prefix() {
        let input = "openai_base_url = \"a\"\nopenai_base_url_alt = \"b\"\n";
        let out = sync_root_value_in_memory(input, "openai_base_url", Some("\"new\""));
        assert!(out.contains("openai_base_url = \"new\""));
        assert!(
            out.contains("openai_base_url_alt = \"b\""),
            "前缀同名的 key 不应被改动"
        );
    }

    #[test]
    fn preserves_user_comments_and_other_keys() {
        let input = "\
# user wrote this
api_key = \"k\"
openai_base_url = \"old\"
# trailing
";
        let out = sync_root_value_in_memory(input, "openai_base_url", Some("\"new\""));
        assert!(out.contains("# user wrote this"));
        assert!(out.contains("api_key = \"k\""));
        assert!(out.contains("openai_base_url = \"new\""));
        assert!(out.contains("# trailing"));
    }

    #[test]
    fn integer_value_no_quotes() {
        let out = sync_root_value_in_memory("", "model_context_window", Some("1000000"));
        assert_eq!(out, "model_context_window = 1000000\n");
    }

    #[test]
    fn toml_string_literal_escapes() {
        assert_eq!(toml_string_literal("hello"), "\"hello\"");
        assert_eq!(toml_string_literal("a\"b"), "\"a\\\"b\"");
        assert_eq!(toml_string_literal("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn delete_when_key_absent_is_noop_but_keeps_trailing_newline() {
        let input = "foo = 1\n";
        let out = sync_root_value_in_memory(input, "missing_key", None);
        assert_eq!(out, "foo = 1\n");
    }
}
