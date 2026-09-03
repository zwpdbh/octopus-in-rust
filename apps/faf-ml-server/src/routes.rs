//! Application route table.
//!
//! This module is the single place where every endpoint is mounted. Feature
//! handlers live in `crate::handlers`; this file only wires them to paths.

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post},
    Router,
};

use crate::{handlers, state::AppState};

/// Build the Axum router for `faf-ml-server`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(handlers::health::health_handler))
        .route(
            "/api/screenshots",
            post(handlers::screenshots::upload_screenshots)
                .get(handlers::screenshots::list_screenshots)
                // Screenshots are a few MB each; lift the 2 MB default.
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route(
            "/api/screenshots/{id}/image",
            get(handlers::screenshots::get_image),
        )
        .route(
            "/api/screenshots/{id}/labels",
            get(handlers::labels::get_labels).put(handlers::labels::put_labels),
        )
        .route(
            "/api/screenshots/{id}",
            delete(handlers::screenshots::delete_screenshot),
        )
        .route("/api/classes", get(handlers::classes::get_classes))
        .route(
            "/api/import/datagen",
            post(handlers::import::import_datagen),
        )
        .route(
            "/api/datasets",
            get(handlers::datasets::list_datasets).post(handlers::datasets::create_dataset),
        )
}
