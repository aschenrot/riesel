//! Reject cross-origin browser access and DNS rebinding before routing.
use super::{routes::ApiError, HttpState};
use axum::{
    extract::{Request, State},
    http::{header, Method},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub(super) async fn gate(
    State(state): State<HttpState>,
    request: Request,
    next: Next,
) -> Response {
    let headers = request.headers();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if host != Some(state.authority.as_ref()) {
        return ApiError::Forbidden.into_response();
    }

    let unsafe_method = matches!(
        request.method(),
        &Method::POST | &Method::PATCH | &Method::DELETE
    );
    if unsafe_method {
        let expected = format!("http://{}", state.authority);
        let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
        if origin != Some(expected.as_str()) {
            return ApiError::Forbidden.into_response();
        }
        if let Some(fetch_site) = headers.get("sec-fetch-site") {
            if !matches!(fetch_site.to_str(), Ok("same-origin") | Ok("none")) {
                return ApiError::Forbidden.into_response();
            }
        }
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|v| v.eq_ignore_ascii_case("application/json")) {
            return ApiError::UnsupportedMedia.into_response();
        }
    }
    next.run(request).await
}
