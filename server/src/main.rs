use futures::StreamExt;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::body::Body;
use axum::extract::Path;
use axum::http::{header, Request};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use bytes::Bytes;
use futures::stream;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::new("info"))
        .init();

    let app = Router::new()
        .route("/:page_number", get(page_handler))
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http()
                .on_request(|request: &Request<_>, span: &tracing::Span| {
                    // Create a span for each request
                    tracing::info!(
                        parent: span,
                        request = ?request,
                        "REQUEST RECEIVED",
                    )
                })
                .on_response(
                    |response: &Response<_>, latency: std::time::Duration, span: &tracing::Span| {
                        // Log when a response is sent
                        span.record("status", tracing::field::display(response.status()));
                        tracing::info!(
                            parent: span,
                            status = %response.status(),
                            latency = ?latency,
                            "RESPONSE SENT"
                        );
                    },
                )
                .on_failure(
                    |error: ServerErrorsFailureClass,
                     latency: std::time::Duration,
                     span: &tracing::Span| {
                        // Log any errors that occur
                        tracing::error!(
                            parent: span,
                            error = %error,
                            latency = ?latency,
                            "RESPONSE ERRORED"
                        );
                    },
                ),
        );

    let config = RustlsConfig::from_pem_file(
        PathBuf::from("server/resources/certs/cert.pem"),
        PathBuf::from("server/resources/certs/key.pem"),
    )
    .await
    .unwrap();

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {}", addr);
    axum_server::bind_rustls(addr, config)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn page_handler(Path(page_number): Path<u32>) -> impl IntoResponse {
    // Stream the large HTML content
    let stream = stream::once(async move {
        Ok::<Bytes, std::io::Error>(Bytes::from(format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<title>Page {}</title>\n</head>\n<body>\n",
            page_number
        )))
    })
    .chain(stream::iter(0..100000).map(move |i| {
        Ok::<Bytes, std::io::Error>(Bytes::from(format!(
            "<h2>Page {}</h2>\n<p>This is some sample content for page {} on iter {}.</p>\n",
            page_number, page_number, i
        )))
    }))
    .chain(stream::once(async {
        Ok::<Bytes, std::io::Error>(Bytes::from("</body>\n</html>"))
    }));

    // Convert the stream into a body
    let body = Body::from_stream(stream);

    // Return the body with appropriate headers
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body)
}
