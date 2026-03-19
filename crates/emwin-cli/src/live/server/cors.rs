use axum::http::HeaderValue;
use axum::http::header::AUTHORIZATION;
use tower_http::cors::{Any, CorsLayer};

pub(crate) fn build_cors_layer(cors_origin: Option<String>) -> crate::error::CliResult<CorsLayer> {
    if let Some(origin) = cors_origin {
        if origin == "*" {
            return Ok(CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers([AUTHORIZATION]));
        }

        let header_value = HeaderValue::from_str(&origin).map_err(|err| {
            crate::error::CliError::invalid_argument(format!(
                "invalid --cors-origin value {origin}: {err}"
            ))
        })?;
        return Ok(CorsLayer::new()
            .allow_origin(header_value)
            .allow_methods(Any)
            .allow_headers([AUTHORIZATION]));
    }

    Ok(CorsLayer::new()
        .allow_methods(Any)
        .allow_headers([AUTHORIZATION]))
}
