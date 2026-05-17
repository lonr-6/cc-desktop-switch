//! 静态文件 fallback —— frontend/ 整目录用 `include_dir!` 编进二进制,
//! 在自定义 URI scheme handler 里直接吐字节,不走文件系统.

use axum::{
    body::Body,
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use include_dir::{include_dir, Dir};

/// frontend/ 目录在编译期被嵌入。路径相对 src-tauri/Cargo.toml 所在目录。
static FRONTEND: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../frontend");

pub async fn serve_static(req: Request) -> Response {
    let path = req.uri().path();
    let trimmed = path.trim_start_matches('/');
    let lookup_path = if trimmed.is_empty() || trimmed == "/" {
        "index.html"
    } else {
        trimmed
    };

    if let Some(file) = FRONTEND.get_file(lookup_path) {
        return file_response(lookup_path, file.contents());
    }
    // SPA fallback: 任何非 /api/* 请求,如果命中不到具体文件,回 index.html
    // 让前端 client-side 路由处理(v1.4 原本不需要,但 Tauri 自定义 scheme
    // 下任何 path 都会进 fallback,留个稳健兜底)
    if !path.starts_with("/api/") {
        if let Some(index) = FRONTEND.get_file("index.html") {
            return file_response("index.html", index.contents());
        }
    }
    (StatusCode::NOT_FOUND, format!("404: {path}")).into_response()
}

fn file_response(path: &str, bytes: &'static [u8]) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.essence_str())
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "build response").into_response())
}
