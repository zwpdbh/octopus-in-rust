//! Application route table.
//!
//! This module is the single place where every endpoint is mounted. Feature
//! handlers live in `crate::handlers`; this file only wires them to paths.

use axum::{
    routing::{get, post},
    Router,
};

use crate::{handlers, state::AppState};

/// Build the Axum router for `fafcn-server`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/units", get(handlers::units::list_units))
        .route("/api/units/:id", get(handlers::units::get_unit))
        .route("/api/portraits/:id", get(handlers::portraits::get_portrait))
        .route("/ws/simulate", get(handlers::simulate::simulate_ws_handler))
        .route("/api/ask", post(handlers::qa::ask_handler))
}
