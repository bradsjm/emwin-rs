use crate::server::types::API_PREFIX;
use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

fn content_type_for_filename(filename: &str) -> &'static str {
    let upper = filename.to_ascii_uppercase();
    if upper.ends_with(".TXT") || upper.ends_with(".WMO") || upper.ends_with(".XML") {
        "text/plain; charset=utf-8"
    } else if upper.ends_with(".JSON") {
        "application/json"
    } else {
        "application/octet-stream"
    }
}

pub(crate) fn sanitize_requested_filename(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start_matches('/').trim();
    if trimmed.is_empty() || trimmed.contains('\0') || trimmed.contains("..") {
        return None;
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return None;
    }
    Some(trimmed.to_string())
}

pub(crate) fn file_download_url(filename: &str) -> String {
    format!("{API_PREFIX}/files/{}", percent_encode(filename))
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

pub(crate) fn build_file_download_response(file: emwin_live::RetainedFile) -> Response {
    let content_type = content_type_for_filename(&file.metadata.filename);
    let disposition = format!("attachment; filename=\"{}\"", file.metadata.filename);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));

    (headers, file.data).into_response()
}

pub(crate) fn build_bytes_download_response(filename: &str, bytes: Vec<u8>) -> Response {
    let content_type = content_type_for_filename(filename);
    let disposition = format!("attachment; filename=\"{}\"", filename);

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(CONTENT_DISPOSITION, value);
    }
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));

    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(bytes))
        .map(|mut response| {
            *response.headers_mut() = headers;
            response
        })
        .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, Vec::new()).into_response())
}

pub(crate) fn filename_request_or_400(raw: &str) -> Result<String, StatusCode> {
    sanitize_requested_filename(raw).ok_or(StatusCode::BAD_REQUEST)
}
