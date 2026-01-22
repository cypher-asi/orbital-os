//! Development Server for ZERO OS
//!
//! Serves static files with the required COOP/COEP headers for
//! SharedArrayBuffer and Web Workers.

use axum::{
    body::Body,
    http::{header, HeaderValue, Request, StatusCode},
    response::Response,
    routing::get_service,
    Router,
};
use std::net::SocketAddr;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // Serve static files from the web directory
    let serve_dir = ServeDir::new("web").precompressed_gzip().precompressed_br();

    let app = Router::new()
        .fallback_service(get_service(serve_dir).handle_error(|_| async {
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
        }))
        .layer(axum::middleware::from_fn(add_headers));

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║             ZERO OS Development Server            ║");
    println!("╠═══════════════════════════════════════════════════╣");
    println!("║  URL: http://localhost:{}                       ║", port);
    println!("║  Press Ctrl+C to stop                             ║");
    println!("╚═══════════════════════════════════════════════════╝");
    println!();

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Add security headers and fix MIME types
async fn add_headers(request: Request<Body>, next: axum::middleware::Next) -> Response<Body> {
    // Get the request path for MIME type detection
    let path = request.uri().path().to_string();

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // Required for SharedArrayBuffer (used by Web Workers)
    headers.insert(
        "Cross-Origin-Opener-Policy",
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "Cross-Origin-Embedder-Policy",
        HeaderValue::from_static("require-corp"),
    );

    // Fix MIME types for module scripts
    if path.ends_with(".js") || path.ends_with(".mjs") {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/javascript; charset=utf-8"),
        );
    } else if path.ends_with(".wasm") {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/wasm"),
        );
    } else if path.ends_with(".css") {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/css; charset=utf-8"),
        );
    } else if path.ends_with(".html") {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
    } else if path.ends_with(".json") {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        );
    }

    response
}
