//! Unit portrait image route.

use axum::{
    extract::{Path, State},
    http::header,
    response::IntoResponse,
};

use crate::{error::AppError, state::AppState};

/// Serve a unit portrait PNG.
pub async fn get_portrait(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let path = state
        .portraits_dir
        .join(format!("{}.png", id.to_ascii_uppercase()));
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    Ok(([(header::CONTENT_TYPE, "image/png")], bytes))
}
