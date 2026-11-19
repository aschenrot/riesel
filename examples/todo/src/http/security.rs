//! Reject cross-origin browser access and DNS rebinding before routing.
use super::{HttpState, routes::ApiError};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, Method, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// A duplicated or undecodable security header is ambiguous and rejected.
fn unique<'a>(headers: &'a HeaderMap, name: HeaderName) -> Result<Option<&'a str>, ApiError> {
    let mut values = headers.get_all(name).iter();
    let first = values.next();
    if values.next().is_some() {
        return Err(ApiError::Forbidden);
    }
    first
        .map(|value| value.to_str().map_err(|_| ApiError::Forbidden))
        .transpose()
}

pub(super) async fn gate(State(state): State<HttpState>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let host = match unique(headers, header::HOST) {
        Ok(host) => host,
        Err(error) => return error.into_response(),
    };
    if host != Some(state.authority.as_ref()) {
        return ApiError::Forbidden.into_response();
    }

    // Fetch Metadata applies to reads as well as writes: reject attempts
    // from other sites even if a future handler accidentally adds CORS.
    let site = match unique(headers, HeaderName::from_static("sec-fetch-site")) {
        Ok(site) => site,
        Err(error) => return error.into_response(),
    };
    if site.is_some_and(|site| !matches!(site, "same-origin" | "none")) {
        return ApiError::Forbidden.into_response();
    }

    let unsafe_method = matches!(
        request.method(),
        &Method::POST | &Method::PATCH | &Method::DELETE
    );
    if unsafe_method {
        let expected = format!("http://{}", state.authority);
        let origin = match unique(headers, header::ORIGIN) {
            Ok(origin) => origin,
            Err(error) => return error.into_response(),
        };
        if origin != Some(expected.as_str()) {
            return ApiError::Forbidden.into_response();
        }
        let content_type = match unique(headers, header::CONTENT_TYPE) {
            Ok(content_type) => content_type,
            Err(error) => return error.into_response(),
        };
        let content_type = content_type
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|v| v.eq_ignore_ascii_case("application/json")) {
            return ApiError::UnsupportedMedia.into_response();
        }
    }
    next.run(request).await
}
