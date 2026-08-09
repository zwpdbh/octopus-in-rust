//! Backend server for the fafcn-web construction simulator.
//!
//! Serves the Dioxus web build as static files and exposes:
//!
//! - `GET /api/units` — list all unit summaries.
//! - `GET /api/units/:id` — single unit blueprint.
//! - `GET /api/portraits/:id` — unit portrait image.
//! - `GET /ws/simulate` — WebSocket to run a simulation and stream events.
//! - `POST /api/ask` — ask the FAF Q&A agent.

mod config;
mod env;
mod error;
mod handlers;
mod llm_factory;
mod routes;
mod state;

use std::sync::Arc;

use anyhow::Context;
use axum::http::{header, Method};
use tower_http::cors::{Any, CorsLayer};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::{config::ServerConfig, state::AppState};

/// Initialize tracing so logs go to both stdout and `data/logs/fafcn-server.log`.
///
/// The returned guard must be kept alive for the lifetime of the process so the
/// non-blocking file writer flushes before shutdown.
fn init_tracing() -> anyhow::Result<tracing_appender::non_blocking::WorkerGuard> {
    let log_dir = std::path::PathBuf::from("data/logs");
    std::fs::create_dir_all(&log_dir)
        .with_context(|| format!("failed to create log directory {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::never(&log_dir, "fafcn-server.log");
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
async fn main() -> anyhow::Result<()> {
    env::load();
    let _tracing_guard = init_tracing()?;

    let server_config = ServerConfig::from_env()?;
    let blueprints = Arc::new(faf_blueprints::FafBlueprints::new()?);
    let qa_config = Arc::new(crate::handlers::qa::QaConfig::from_env()?);

    tracing::info!(
        provider_type = %qa_config.provider_type,
        base_url = %qa_config.base_url,
        model = %qa_config.model,
        plugins_dir = %qa_config.plugins_dir.display(),
        "Q&A module loaded"
    );
    tracing::info!("loaded {} units", blueprints.all_units().len());
    tracing::info!(
        "serving static assets from {}",
        server_config.assets_dir.display()
    );

    let state = AppState {
        blueprints,
        portraits_dir: Arc::new(server_config.portraits_dir),
        assets_dir: Arc::new(server_config.assets_dir.clone()),
        qa_config,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
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
