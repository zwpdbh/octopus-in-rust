//! Shared application state for Axum handlers.

use std::{path::PathBuf, sync::Arc};

use faf_blueprints::FafBlueprints;

/// State shared across all HTTP/WebSocket handlers.
#[derive(Clone)]
pub struct AppState {
    pub blueprints: Arc<FafBlueprints>,
    pub portraits_dir: Arc<PathBuf>,
    pub assets_dir: Arc<PathBuf>,
    pub qa_config: Arc<crate::handlers::qa::QaConfig>,
}
