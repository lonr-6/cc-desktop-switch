//! `provider.extra_headers` 的合法性校验.
//!
//! 历史背景:`Provider::extra_headers` 是 `IndexMap<String, String>`,
//! schema 层不做 HTTP header 合法性检查;运行时 `crate::resolver` 才在
//! outbound 时尝试 `HeaderName::from_bytes(k)` / `HeaderValue::from_str(v)`,
//! 失败时只 telemetry WARN 然后**静默跳过**该 header。
//!
//! 后果:用户在 UI 配置 Kimi/Provider 时不小心写了非法 header(粘贴时带换行、
//! header 名含空格、value 含 \r\n 等),保存成功 → 运行时 header 默默丢失 →
//! Kimi 上游 403,用户完全不知道是哪个 header 没生效。
//!
//! 这个模块在 admin handler(`add_provider` / `update_provider`)接到 user
//! input 时调用,非法直接拒绝并给具体错误位置,让"运行时能 parse 的 =
//! 配置时能保存的"一致。

use reqwest::header::{HeaderName, HeaderValue};

/// `provider.extra_headers` 单条 (key, value) 的校验失败原因.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderValidationError {
    /// header name 非法(含禁止字符 / 空 / 不符 RFC 7230 token 规则).
    InvalidName { key: String, reason: String },
    /// header value 非法(含控制字符 / 非 visible ASCII / obs-fold 等).
    InvalidValue { key: String, reason: String },
}

impl std::fmt::Display for HeaderValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeaderValidationError::InvalidName { key, reason } => {
                write!(f, "extra_headers 名 {key:?} 非法: {reason}")
            }
            HeaderValidationError::InvalidValue { key, reason } => {
                write!(f, "extra_headers {key:?} 的值非法: {reason}")
            }
        }
    }
}

impl std::error::Error for HeaderValidationError {}

/// 校验整个 `extra_headers` map,返回所有非法条目(空 vec 表示全合法).
///
/// 规则:
/// - **key**:必须能 `HeaderName::from_bytes`,即符合 RFC 7230 token 规则
///   (字母 / 数字 / `!#$%&'*+-.^_\`|~` 子集,无空格 / 控制字符)
/// - **value**:必须能 `HeaderValue::from_str`,即只含 visible ASCII 或
///   普通 UTF-8 文本,**绝不能含 \r 或 \n**(否则 HTTP smuggling 风险)
/// - **`{apiKey}` 模板**:`crate::resolver` 运行时把 `{apiKey}` 替换成
///   `provider.api_key`。这里把模板**替换成空字符串后再校验**,确保运行时
///   无论 api_key 是否为空,合成后的 value 都合法。
pub fn validate_extra_headers<'a, I>(headers: I) -> Vec<HeaderValidationError>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut errs = Vec::new();
    for (k, v) in headers {
        if let Err(e) = HeaderName::from_bytes(k.as_bytes()) {
            errs.push(HeaderValidationError::InvalidName {
                key: k.to_owned(),
                reason: e.to_string(),
            });
            continue;
        }

        let v_for_check = v.replace("{apiKey}", "");
        if let Err(e) = HeaderValue::from_str(&v_for_check) {
            errs.push(HeaderValidationError::InvalidValue {
                key: k.to_owned(),
                reason: e.to_string(),
            });
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn map_from(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn validate_map(m: &IndexMap<String, String>) -> Vec<HeaderValidationError> {
        validate_extra_headers(m.iter().map(|(k, v)| (k.as_str(), v.as_str())))
    }

    #[test]
    fn accepts_common_kimi_headers() {
        let m = map_from(&[
            ("User-Agent", "KimiCLI/1.40.0"),
            ("X-Foo", "bar"),
            ("Authorization", "Bearer sk-test"),
        ]);
        assert!(validate_map(&m).is_empty());
    }

    #[test]
    fn accepts_apikey_template() {
        let m = map_from(&[("X-Api-Key", "Bearer {apiKey}")]);
        assert!(validate_map(&m).is_empty());
    }

    #[test]
    fn rejects_header_name_with_space() {
        let m = map_from(&[("User Agent", "X")]);
        let errs = validate_map(&m);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            HeaderValidationError::InvalidName { key, .. } if key == "User Agent"
        ));
    }

    #[test]
    fn rejects_header_name_with_control_char() {
        let m = map_from(&[("User\nAgent", "X")]);
        assert_eq!(validate_map(&m).len(), 1);
    }

    #[test]
    fn rejects_empty_header_name() {
        let m = map_from(&[("", "X")]);
        assert_eq!(validate_map(&m).len(), 1);
    }

    #[test]
    fn rejects_value_with_lf() {
        let m = map_from(&[("X-Custom", "good\nbad: injected")]);
        let errs = validate_map(&m);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            HeaderValidationError::InvalidValue { key, .. } if key == "X-Custom"
        ));
    }

    #[test]
    fn rejects_value_with_cr() {
        let m = map_from(&[("X-Custom", "good\rbad")]);
        assert_eq!(validate_map(&m).len(), 1);
    }

    #[test]
    fn collects_all_errors_not_just_first() {
        let m = map_from(&[
            ("Bad Name", "ok"),
            ("X-Good", "fine"),
            ("X-Bad-Value", "x\ny"),
            ("Another Bad", "z"),
        ]);
        let errs = validate_map(&m);
        assert_eq!(errs.len(), 3, "应收集 3 个错误,实际: {errs:?}");
    }

    #[test]
    fn empty_map_is_valid() {
        let m: IndexMap<String, String> = IndexMap::new();
        assert!(validate_map(&m).is_empty());
    }

    #[test]
    fn apikey_template_with_invalid_surroundings_still_caught() {
        let m = map_from(&[("X-Bad", "Bearer {apiKey}\ninjected: yes")]);
        let errs = validate_map(&m);
        assert_eq!(errs.len(), 1);
        assert!(matches!(
            &errs[0],
            HeaderValidationError::InvalidValue { .. }
        ));
    }
}
