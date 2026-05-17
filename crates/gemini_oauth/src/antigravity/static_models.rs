//! Antigravity 模型清单**静态种子**(fallback)。
//!
//! 上游 `:fetchAvailableModels` 失败(网络 / token expire / Google 改 API 等)时
//! 退到这份种子,UI"获取模型"按钮还能拿到 sane default。
//!
//! 来源:**1:1 抄自** CLIProxyAPI `internal/registry/models/models.json`
//! 第 1876-2067 行(antigravity slice,10 条),编译期 `include_str!` 进二进制。
//!
//! ⚠️ 上游可能新增 / 改动模型 — 这份种子定期(eg release 前)从 CLIProxyAPI
//! `cmd/fetch_antigravity_models` 跑一次刷新,或者直接从 antigravity IDE 的
//! `:fetchAvailableModels` 抓最新。

use std::sync::OnceLock;

use serde_json::Value;

use super::models::AntigravityModelEntry;

const SEED_JSON: &str = include_str!("../../static_data/antigravity_models.json");

fn seed_models() -> &'static Vec<AntigravityModelEntry> {
    static CELL: OnceLock<Vec<AntigravityModelEntry>> = OnceLock::new();
    CELL.get_or_init(|| {
        let raw: Vec<Value> = serde_json::from_str(SEED_JSON)
            .expect("antigravity_models.json static seed parse failed");
        raw.into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect()
    })
}

/// 返静态种子 model 列表(clone)。fetch 失败时调用方退到这里
pub fn antigravity_static_models() -> Vec<AntigravityModelEntry> {
    seed_models().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 锚定 seed 数量 = 10(对齐 CLIProxyAPI models.json 当前 antigravity slice)
    #[test]
    fn seed_count_matches_cliproxyapi() {
        assert_eq!(antigravity_static_models().len(), 10);
    }

    /// 锚定关键 model id 存在(防 seed 被意外清空 / 改名)
    #[test]
    fn seed_contains_canonical_models() {
        let models = antigravity_static_models();
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"gemini-3-pro-low"), "缺 gemini-3-pro-low");
        assert!(ids.contains(&"gemini-3-pro-high"), "缺 gemini-3-pro-high");
        assert!(
            ids.contains(&"claude-opus-4-6-thinking"),
            "缺 claude-opus-4-6-thinking"
        );
        assert!(ids.contains(&"gemini-3.1-pro-low"), "缺 gemini-3.1-pro-low");
        assert!(
            ids.contains(&"gpt-oss-120b-medium"),
            "缺 gpt-oss-120b-medium"
        );
    }

    /// 锚定 owned_by/type 全部 "antigravity"(OpenAI /v1/models 客户端按 owned_by 区分)
    #[test]
    fn seed_all_owned_by_antigravity() {
        for m in antigravity_static_models() {
            assert_eq!(m.owned_by, "antigravity");
            assert_eq!(m.kind, "antigravity");
            assert_eq!(m.object, "model");
        }
    }
}
