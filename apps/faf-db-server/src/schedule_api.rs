//! Scheduling endpoint: compute a build order for an eco or unit target.
//!
//! This mirrors the `faf-sim schedule eco|unit` CLI command over HTTP. The
//! request uses `UnitKind` JSON directly (rather than the CLI's string
//! grammar) because the web frontend already speaks the typed protocol.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use faf_blueprints::UnitKind;
use faf_build_scheduler::{
    EcoScheduleRequest, EcoTarget, Schedule, ScheduleError, SchedulerConfig, SearchOptions,
    UnitScheduleRequest,
};
use faf_sim::quantities::MassRate;
use faf_sim::runtime::EcoSnapshot;
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Web scheduling request. Tagged by `mode` ("eco" or "unit").
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum ScheduleApiRequest {
    Eco {
        initial_eco: EcoSnapshot,
        #[serde(default = "default_inventory")]
        initial_inventory: Vec<UnitKind>,
        target_mass_production: f64,
        #[serde(default = "default_tolerance")]
        tolerance: f64,
        #[serde(default)]
        options: SearchOptions,
        #[serde(default = "default_max_mex_count")]
        max_mex_count: u32,
    },
    Unit {
        initial_eco: EcoSnapshot,
        #[serde(default = "default_inventory")]
        initial_inventory: Vec<UnitKind>,
        target: UnitKind,
        #[serde(default)]
        options: SearchOptions,
        #[serde(default = "default_max_mex_count")]
        max_mex_count: u32,
    },
}

fn default_inventory() -> Vec<UnitKind> {
    vec![UnitKind::Commander]
}

fn default_tolerance() -> f64 {
    1.0
}

fn default_max_mex_count() -> u32 {
    10
}

/// Error envelope returned when scheduling fails.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleApiError {
    pub error: String,
}

type ApiResult = Result<Json<Schedule>, (StatusCode, Json<ScheduleApiError>)>;

fn api_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<ScheduleApiError>) {
    (
        status,
        Json(ScheduleApiError {
            error: message.into(),
        }),
    )
}

pub async fn schedule(
    State(state): State<AppState>,
    Json(request): Json<ScheduleApiRequest>,
) -> ApiResult {
    // The greedy algorithm is still a `todo!()`; catch its panic so the API
    // returns a clean JSON error instead of crashing the connection.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &request {
        ScheduleApiRequest::Eco {
            initial_eco,
            initial_inventory,
            target_mass_production,
            tolerance,
            options,
            max_mex_count,
        } => {
            let target = EcoTarget {
                mass_production: MassRate::from_raw(*target_mass_production),
                tolerance: *tolerance,
            };
            state.scheduler.schedule_eco(&EcoScheduleRequest {
                initial_eco: *initial_eco,
                initial_inventory: initial_inventory.clone(),
                target,
                options: options.clone(),
                config: SchedulerConfig {
                    max_mex_count: *max_mex_count,
                },
            })
        }
        ScheduleApiRequest::Unit {
            initial_eco,
            initial_inventory,
            target,
            options,
            max_mex_count,
        } => state.scheduler.schedule_unit(&UnitScheduleRequest {
            initial_eco: *initial_eco,
            initial_inventory: initial_inventory.clone(),
            target: target.clone(),
            options: options.clone(),
            config: SchedulerConfig {
                max_mex_count: *max_mex_count,
            },
        }),
    }));

    match result {
        Ok(Ok(schedule)) => Ok(Json(schedule)),
        Ok(Err(err)) => Err(api_error(status_for(&err), err.to_string())),
        Err(_) => Err(api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "scheduling algorithm is not implemented yet",
        )),
    }
}

fn status_for(err: &ScheduleError) -> StatusCode {
    match err {
        ScheduleError::NoLegalBuilder { .. }
        | ScheduleError::GoalUnreachable
        | ScheduleError::SimulationStalled
        | ScheduleError::SearchTimeout => StatusCode::UNPROCESSABLE_ENTITY,
        ScheduleError::AlgorithmNotImplemented(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
