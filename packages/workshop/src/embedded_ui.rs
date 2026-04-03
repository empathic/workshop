//! Embedded UI serving when compiled with the `embedded-ui` feature.
//!
//! This module serves the SvelteKit SPA via Axum using assets from the
//! workshop_ui crate.

use axum::{
    Router,
    body::Body,
    http::{Request, Response, StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use workshop_ui::Assets as UiAssets;

/// Create a router that serves the embedded SPA under /spa/
pub fn spa_router() -> Router {
    Router::new()
        // Serve exact /spa/ path as index
        .route("/", get(serve_index))
        // Serve all other paths under /spa/*
        .fallback(serve_spa_asset)
}

/// Serve index.html
async fn serve_index() -> impl IntoResponse {
    serve_file("index.html")
}

/// Serve embedded UI assets, with SPA fallback to index.html
async fn serve_spa_asset(req: Request<Body>) -> impl IntoResponse {
    // Strip leading slash to get the asset path
    let path = req.uri().path().trim_start_matches('/');

    // Try to serve the exact path first
    if !path.is_empty()
        && let Some(response) = try_serve_file(path)
    {
        return response;
    }

    // For SPA routing, serve index.html for non-asset paths
    if path.is_empty() || !path.contains('.') || path.ends_with(".html") {
        return serve_file("index.html");
    }

    // 404 for missing assets
    not_found_response()
}

fn try_serve_file(path: &str) -> Option<Response<Body>> {
    UiAssets::get(path).map(|content| {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.into_owned()))
            .unwrap_or_else(|_| not_found_response())
    })
}

fn serve_file(path: &str) -> Response<Body> {
    if let Some(content) = UiAssets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(content.data.into_owned()))
            .unwrap_or_else(|_| not_found_response())
    } else {
        not_found_response()
    }
}

/// Helper to create a 404 response without unwrap
fn not_found_response() -> Response<Body> {
    let mut response = Response::new(Body::from("Not Found"));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}
