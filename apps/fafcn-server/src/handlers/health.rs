//! Composite health check: `GET /api/health`.
//!
//! Combines a standard service health check (local resources the server
//! needs to answer requests) with the Q&A LLM round-trip check, so callers
//! can distinguish "the service is up" from "the LLM provider works":
//!
//! ```json
//! {
//!   "status": "ok",
//!   "service": { "status": "ok", "units_loaded": 507, ... },
//!   "qa": { "status": "ok", "model": "...", "reply": "pong" }
//! }
//! ```
//!
//! HTTP semantics: `200 OK` whenever the service itself is healthy — a
//! failing LLM provider yields `"status": "degraded"` with `qa.status =
//! "error"` (the site still works, only the Q&A page is affected). `503` is
//! reserved for missing local resources (broken deployment).
//!
//! Note: every call performs a real (tiny) LLM request, which costs a few
//! tokens — do not poll this endpoint aggressively. Use `/api/health/qa` for
//! the LLM-only variant.

use axum::{extract::State, http::StatusCode, Json};
use serde::Serialize;

use crate::{handlers::qa, state::AppState};

/// Response body for `GET /api/health`.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// `"ok"` when every component is healthy, `"degraded"` otherwise.
    pub status: &'static str,
    pub service: ServiceHealth,
    pub qa: QaComponentHealth,
}

/// Standard service health: local resources required to serve requests.
#[derive(Debug, Serialize)]
pub struct ServiceHealth {
    pub status: &'static str,
    /// Number of unit blueprints loaded from the units file.
    pub units_loaded: usize,
    /// The built web SPA (index.html) is present.
    pub web_dist_present: bool,
    /// The unit portraits directory is present.
    pub portraits_dir_present: bool,
    /// The gamedata mirror directory layout is present.
    pub gamedata_dir_present: bool,
}

/// Q&A LLM provider health (a real ping/pong round-trip).
#[derive(Debug, Serialize)]
pub struct QaComponentHealth {
    pub status: &'static str,
    pub provider_type: String,
    pub base_url: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Axum handler for `GET /api/health`.
pub async fn health_handler(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let units_loaded = state.blueprints.all_units().len();
    let web_dist_present = state.assets_dir.join("index.html").is_file();
    let portraits_dir_present = state.portraits_dir.is_dir();
    let gamedata_dir_present = state
        .gamedata
        .files_dir(fafcn_gamedata::CHANNEL_GAMEDATA)
        .is_dir();

    let service_ok =
        units_loaded > 0 && web_dist_present && portraits_dir_present && gamedata_dir_present;
    let service = ServiceHealth {
        status: if service_ok { "ok" } else { "error" },
        units_loaded,
        web_dist_present,
        portraits_dir_present,
        gamedata_dir_present,
    };

    let config = &state.qa_config;
    let qa = match qa::verify_provider_auth(config).await {
        Ok(reply) => QaComponentHealth {
            status: "ok",
            provider_type: config.provider_type.to_string(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            reply: Some(reply),
            error: None,
        },
        Err(e) => QaComponentHealth {
            status: "error",
            provider_type: config.provider_type.to_string(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            reply: None,
            error: Some(e.to_string()),
        },
    };

    let degraded = !service_ok || qa.status != "ok";
    let code = if service_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        code,
        Json(HealthResponse {
            status: if degraded { "degraded" } else { "ok" },
            service,
            qa,
        }),
    )
}
