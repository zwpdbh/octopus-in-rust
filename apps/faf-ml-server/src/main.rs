//! Backend server for the faf-ml data platform (phase 0: collect → review →
//! snapshot).
//!
//! Serves the Dioxus web build as static files and exposes:
//!
//! - `POST /api/screenshots` — multipart PNG upload.
//! - `GET /api/screenshots` — list screenshot metadata.
//! - `GET /api/screenshots/:id/image` — serve the PNG.
//! - `GET/PUT /api/screenshots/:id/labels` — read/replace bounding boxes.
//! - `DELETE /api/screenshots/:id` — remove image + labels.
//! - `GET /api/classes` — class list.
//! - `POST /api/import/datagen` — import a faf-datagen output directory.
//! - `GET/POST /api/datasets` — list / create immutable dataset snapshots.

mod config;
mod env;
mod error;
mod handlers;
mod routes;
mod state;

use anyhow::Context;
use axum::http::{header, Method};
use error::Result;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::{config::ServerConfig, state::AppState};

/// Initialize tracing so logs go to both stdout and `data/logs/faf-ml-server.log`.
///
/// The returned guard must be kept alive for the lifetime of the process so
/// the non-blocking file writer flushes before shutdown.
fn init_tracing() -> Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = config::workspace_root().join("data/logs");
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create log directory {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::never(&log_dir, "faf-ml-server.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false),
        )
        .init();

    Ok(guard)
}

#[tokio::main]
async fn main() -> Result<()> {
    env::load();
    let _tracing_guard = init_tracing()?;

    let server_config = ServerConfig::from_env()?;
    let state = AppState::new(
        server_config.data_dir.clone(),
        server_config.assets_dir.clone(),
    )?;

    tracing::info!(data_dir = %state.data_dir.display(), "data store ready");
    tracing::info!(
        "serving static assets from {}",
        server_config.assets_dir.display()
    );

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE]);

    let app = routes::router()
        .fallback_service(
            ServeDir::new(state.assets_dir.as_ref())
                .fallback(ServeFile::new(state.assets_dir.join("index.html"))),
        )
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", server_config.port)).await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}
