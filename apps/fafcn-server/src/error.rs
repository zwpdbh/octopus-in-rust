//! Shared error type and Axum response mapping.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub enum AppError {
    /// Requested resource was not found.
    NotFound,

    /// Blueprint lookup or parse failure.
    Blueprint(faf_blueprints::Error),

    /// Agent (Q&A) turn failed.
    Agent(agent_core::BrainError),
}

impl From<faf_blueprints::Error> for AppError {
    fn from(err: faf_blueprints::Error) -> Self {
        AppError::Blueprint(err)
    }
}

impl From<agent_core::BrainError> for AppError {
    fn from(err: agent_core::BrainError) -> Self {
        AppError::Agent(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            AppError::Blueprint(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{err}")).into_response()
            }
            AppError::Agent(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{err}")).into_response()
            }
        }
    }
}
