//! Gemini grounding metadata → OpenAI annotations 映射。
//!
//! 1:1 移植 LiteLLM `vertex_and_google_ai_studio_gemini.py:2110
//! _convert_grounding_metadata_to_annotations`:
//!
//! 公式:对每个 `groundingSupports[i]`:
//! ```text
//! segment = supports[i].segment
//! chunk_idx = supports[i].groundingChunkIndices[0]  # 只用首个
//! annotation = {
//!   type: "url_citation",
//!   url_citation: {
//!     start_index: segment.startIndex,
//!     end_index: segment.endIndex,
//!     url: groundingChunks[chunk_idx].web.uri,
//!     title: groundingChunks[chunk_idx].web.title,
//!   }
//! }
//! ```

use serde_json::{json, Map, Value};

use super::types::GroundingMetadata;

/// 把 Gemini grounding_metadata 转 OpenAI 风格 annotations 数组。
/// 输出形态与 OpenAI Responses `response.output_text.annotation.added` 事件
/// 一致(`type: "url_citation"`)。
pub fn convert_grounding_metadata_to_annotations(gm: &GroundingMetadata) -> Vec<Value> {
    let chunks = match &gm.grounding_chunks {
        Some(c) => c,
        None => return Vec::new(),
    };
    let supports = match &gm.grounding_supports {
        Some(s) => s,
        None => return Vec::new(),
    };

    let mut annotations: Vec<Value> = Vec::new();
    for support in supports {
        if support.grounding_chunk_indices.is_empty() {
            continue;
        }
        let first_idx = support.grounding_chunk_indices[0];
        let Some(chunk) = chunks.get(first_idx) else {
            tracing::debug!(
                chunk_index = first_idx,
                total_chunks = chunks.len(),
                "gemini grounding support references out-of-range chunk index, skipping"
            );
            continue;
        };
        let Some(web) = &chunk.web else {
            // H6 修复:非 web chunk(如 retrievedContext / RAG)目前 MVP 不映射,
            // 用户视角"模型回答有引用但 UI 没显示来源"。debug log 帮 follow-up
            // 加 retrievedContext → annotation 转换时定位需求。
            tracing::debug!(
                chunk_index = first_idx,
                "gemini grounding chunk lacks `web` field (likely retrievedContext or future \
                 chunk type); skipping annotation. TODO: map non-web chunks once Codex.app \
                 UI supports their citation shape."
            );
            continue;
        };
        let mut url_citation = Map::new();
        url_citation.insert("start_index".into(), json!(support.segment.start_index));
        url_citation.insert("end_index".into(), json!(support.segment.end_index));
        url_citation.insert("url".into(), json!(web.uri));
        if let Some(title) = &web.title {
            url_citation.insert("title".into(), json!(title));
        }
        annotations.push(json!({
            "type": "url_citation",
            "url_citation": Value::Object(url_citation),
        }));
    }
    annotations
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_real_grounding_metadata_shape() {
        // 来自 ai.google.dev/api/generate-content GroundingMetadata 文档示例
        let gm: GroundingMetadata = serde_json::from_value(json!({
            "webSearchQueries": ["NYC weather today"],
            "groundingChunks": [
                {"web": {"uri": "https://weather.com/nyc", "title": "Weather - NYC"}},
                {"web": {"uri": "https://accuweather.com/nyc", "title": "AccuWeather"}}
            ],
            "groundingSupports": [
                {
                    "segment": {"startIndex": 0, "endIndex": 25, "text": "NYC today is 25°C sunny"},
                    "groundingChunkIndices": [0, 1],
                    "confidenceScores": [0.95, 0.88]
                },
                {
                    "segment": {"startIndex": 30, "endIndex": 60, "text": "Forecast: light rain tomorrow"},
                    "groundingChunkIndices": [1],
                    "confidenceScores": [0.7]
                }
            ]
        }))
        .unwrap();

        let annotations = convert_grounding_metadata_to_annotations(&gm);
        assert_eq!(annotations.len(), 2);
        // 第 1 条 — 用 chunk[0]
        assert_eq!(annotations[0]["type"], "url_citation");
        assert_eq!(annotations[0]["url_citation"]["start_index"], 0);
        assert_eq!(annotations[0]["url_citation"]["end_index"], 25);
        assert_eq!(
            annotations[0]["url_citation"]["url"],
            "https://weather.com/nyc"
        );
        assert_eq!(annotations[0]["url_citation"]["title"], "Weather - NYC");
        // 第 2 条 — 用 chunk[1]
        assert_eq!(
            annotations[1]["url_citation"]["url"],
            "https://accuweather.com/nyc"
        );
    }

    #[test]
    fn skips_supports_with_empty_chunk_indices() {
        let gm: GroundingMetadata = serde_json::from_value(json!({
            "groundingChunks": [{"web":{"uri":"https://x.com"}}],
            "groundingSupports": [
                {"segment":{"startIndex":0,"endIndex":10}, "groundingChunkIndices":[]}
            ]
        }))
        .unwrap();
        let annotations = convert_grounding_metadata_to_annotations(&gm);
        assert!(
            annotations.is_empty(),
            "groundingChunkIndices 为空必须 skip"
        );
    }

    #[test]
    fn skips_chunks_without_web() {
        // 非 web 类型 chunk(retrieved_context 等)— MVP 不映射
        let gm: GroundingMetadata = serde_json::from_value(json!({
            "groundingChunks": [{"retrievedContext":{"uri":"x://y"}}],
            "groundingSupports": [
                {"segment":{"startIndex":0,"endIndex":10}, "groundingChunkIndices":[0]}
            ]
        }))
        .unwrap();
        let annotations = convert_grounding_metadata_to_annotations(&gm);
        assert!(annotations.is_empty(), "非 web chunk 必须 skip");
    }

    #[test]
    fn handles_missing_title() {
        let gm: GroundingMetadata = serde_json::from_value(json!({
            "groundingChunks": [{"web":{"uri":"https://x.com"}}],
            "groundingSupports": [
                {"segment":{"startIndex":0,"endIndex":10}, "groundingChunkIndices":[0]}
            ]
        }))
        .unwrap();
        let annotations = convert_grounding_metadata_to_annotations(&gm);
        assert_eq!(annotations.len(), 1);
        assert!(
            annotations[0]["url_citation"].get("title").is_none(),
            "title 缺时不写字段"
        );
    }

    #[test]
    fn empty_metadata_returns_empty_annotations() {
        let gm: GroundingMetadata = serde_json::from_value(json!({})).unwrap();
        assert!(convert_grounding_metadata_to_annotations(&gm).is_empty());
    }
}
