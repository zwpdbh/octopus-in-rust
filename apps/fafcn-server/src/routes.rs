//! Application route table.
//!
//! This module is the single place where every endpoint is mounted. Feature
//! handlers live in `crate::handlers`; this file only wires them to paths.

use std::path::Path;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use tower_http::services::ServeDir;

use crate::{handlers, state::AppState};

/// Build the Axum router for `fafcn-server`.
///
/// `gamedata_files_dir` is mounted as a static file service (with range
/// support) for sync downloads.
pub fn router(gamedata_files_dir: &Path) -> Router<AppState> {
    Router::new()
        .route("/api/units", get(handlers::units::list_units))
        .route("/api/units/:id", get(handlers::units::get_unit))
        .route("/api/portraits/:id", get(handlers::portraits::get_portrait))
        .route("/ws/simulate", get(handlers::simulate::simulate_ws_handler))
        .route("/api/ask", post(handlers::qa::ask_handler))
        .route("/api/ask/stream", post(handlers::qa::ask_stream_handler))
        .route("/api/health/qa", get(handlers::qa::health_handler))
        // Gamedata mirror: JSON API.
        .route(
            "/api/gamedata/manifest.json",
            get(handlers::gamedata::get_manifest),
        )
        .route("/api/gamedata/status", get(handlers::gamedata::get_status))
        .route(
            "/api/gamedata/upload/check",
            post(handlers::gamedata::upload_check),
        )
        .route(
            "/api/gamedata/upload/file",
            post(handlers::gamedata::upload_file).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/gamedata/upload/commit",
            post(handlers::gamedata::upload_commit),
        )
        // Gamedata mirror: static downloads (mirror files) + patched client binaries.
        .nest_service("/api/gamedata/files", ServeDir::new(gamedata_files_dir))
        .route(
            "/api/gamedata/client/:filename",
            get(handlers::gamedata::download_client),
        )
}
