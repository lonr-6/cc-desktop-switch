//! `/api/presets` —— 内置 provider presets.

use axum::{response::IntoResponse, Json};
use codex_app_transfer_registry::builtin_presets;
use serde_json::{json, Value};

pub async fn list_presets() -> impl IntoResponse {
    let presets: Vec<Value> = builtin_presets().to_vec();
    Json(json!({"presets": presets})).into_response()
}
