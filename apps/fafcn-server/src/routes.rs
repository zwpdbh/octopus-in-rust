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
use fafcn_gamedata::{CHANNEL_FAF_CLIENT, CHANNEL_GAMEDATA, CHANNEL_MAPS, CHANNEL_MAP_GENERATOR};
use tower_http::services::ServeDir;

use crate::{handlers, state::AppState};

/// Build the Axum router for `fafcn-server`.
///
/// `gamedata_root` is the mirror storage root; each channel's `files/` dir is
/// mounted as a static file service (with range support) for sync downloads.
pub fn router(gamedata_root: &Path) -> Router<AppState> {
    let channels = gamedata_root.join("channels");
    Router::new()
        .route("/api/units", get(handlers::units::list_units))
        .route("/api/units/meta", get(handlers::units::units_meta))
        .route("/api/units/{id}", get(handlers::units::get_unit))
        .route(
            "/api/portraits/{id}",
            get(handlers::portraits::get_portrait),
        )
        .route("/ws/simulate", get(handlers::simulate::simulate_ws_handler))
        .route("/api/ask", post(handlers::qa::ask_handler))
        .route("/api/ask/stream", post(handlers::qa::ask_stream_handler))
        .route("/api/health", get(handlers::health::health_handler))
        .route("/api/health/qa", get(handlers::qa::health_handler))
        // Gamedata mirror: JSON API (per channel).
        .route(
            "/api/gamedata/channels/{channel}/manifest.json",
            get(handlers::gamedata::get_manifest),
        )
        .route("/api/gamedata/status", get(handlers::gamedata::get_status))
        .route(
            "/api/gamedata/upstream/refresh",
            post(handlers::gamedata::upstream_refresh),
        )
        .route(
            "/api/gamedata/channels/{channel}/upload/check",
            post(handlers::gamedata::upload_check),
        )
        .route(
            "/api/gamedata/channels/{channel}/upload/file",
            post(handlers::gamedata::upload_file).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/gamedata/channels/{channel}/upload/commit",
            post(handlers::gamedata::upload_commit),
        )
        // Gamedata mirror: static downloads (per channel) + patched client binaries.
        .nest_service(
            "/api/gamedata/channels/gamedata/files",
            ServeDir::new(channels.join(CHANNEL_GAMEDATA).join("files")),
        )
        .nest_service(
            "/api/gamedata/channels/map-generator/files",
            ServeDir::new(channels.join(CHANNEL_MAP_GENERATOR).join("files")),
        )
        .nest_service(
            "/api/gamedata/channels/faf-client/files",
            ServeDir::new(channels.join(CHANNEL_FAF_CLIENT).join("files")),
        )
        .nest_service(
            "/api/gamedata/channels/maps/files",
            ServeDir::new(channels.join(CHANNEL_MAPS).join("files")),
        )
        .route(
            "/api/gamedata/client/{filename}",
            get(handlers::gamedata::download_client),
        )
}
