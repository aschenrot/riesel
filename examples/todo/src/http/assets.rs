//! Serve trusted, statically built SvelteKit assets from the Todo API authority.
use super::{HttpState, routes::ApiError};
use axum::{Router, routing::any};
use std::{
    io,
    path::{Path, PathBuf},
};
use tower_http::services::{ServeDir, ServeFile};

pub(super) fn verify(directory: Option<PathBuf>) -> io::Result<Option<PathBuf>> {
    directory
        .map(|directory| {
            let canonical = directory.canonicalize()?;
            if !canonical.is_dir() || !canonical.join("200.html").is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Todo --assets must be a SvelteKit build directory containing 200.html",
                ));
            }
            Ok(canonical)
        })
        .transpose()
}

pub(super) fn mount(router: Router<HttpState>, directory: Option<&Path>) -> Router<HttpState> {
    match directory {
        Some(directory) => {
            // Unknown API paths must remain JSON 404 responses, not SPA HTML.
            router
                .route("/api", any(|| async { ApiError::NotFound }))
                .route("/api/{*rest}", any(|| async { ApiError::NotFound }))
                .fallback_service(
                    ServeDir::new(directory).fallback(ServeFile::new(directory.join("200.html"))),
                )
        }
        None => router.fallback(|| async { ApiError::NotFound }),
    }
}
