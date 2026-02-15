use std::fs;
use std::path::{Path, PathBuf};

use tauri::http::{header, Response, StatusCode};

use crate::updates;

fn mime_for_path(p: &Path) -> String {
    mime_guess::from_path(p)
        .first_or_octet_stream()
        .essence_str()
        .to_string()
}

fn read_file_bytes(p: &Path) -> std::io::Result<Vec<u8>> {
    fs::read(p)
}

fn sanitize_rel_path(p: &str) -> Option<PathBuf> {
    // Drop query/fragment if present (some webviews include them).
    let p = p
        .split('?')
        .next()
        .unwrap_or(p)
        .split('#')
        .next()
        .unwrap_or(p);
    let p = p.trim_start_matches('/');
    if p.is_empty() {
        return Some(PathBuf::from("index.html"));
    }
    let rel = PathBuf::from(p);
    // Basic traversal guard.
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(rel)
}

pub fn handle(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let app = ctx.app_handle();
    let path = request.uri().path();

    let rel = match sanitize_rel_path(path) {
        Some(r) => r,
        None => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(b"bad path".to_vec())
                .unwrap();
        }
    };

    let ui_root = match updates::current_ui_dir(&app) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("ui root error: {e}");
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_TYPE, "text/plain")
                .body(msg.into_bytes())
                .unwrap();
        }
    };

    let mut file_path = ui_root.join(&rel);
    if file_path.is_dir() {
        file_path = file_path.join("index.html");
    }

    let bytes = match read_file_bytes(&file_path) {
        Ok(b) => b,
        Err(_) => {
            // SPA fallback: for non-asset paths, serve index.html.
            let ext = file_path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext.is_empty() || ext == "html" {
                let idx = ui_root.join("index.html");
                match read_file_bytes(&idx) {
                    Ok(b) => b,
                    Err(_) => {
                        return Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .header(header::CONTENT_TYPE, "text/plain")
                            .body(b"ui not found".to_vec())
                            .unwrap();
                    }
                }
            } else {
                return Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .header(header::CONTENT_TYPE, "text/plain")
                    .body(b"not found".to_vec())
                    .unwrap();
            }
        }
    };

    let ct = mime_for_path(&file_path);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(bytes)
        .unwrap()
}
