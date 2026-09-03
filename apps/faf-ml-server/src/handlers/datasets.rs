//! Dataset snapshot routes (`GET/POST /api/datasets`).
//!
//! Snapshots are immutable: `POST` embeds the current labels of every
//! requested image into `datasets/<name>.json` and refuses to overwrite an
//! existing snapshot.

use axum::{extract::State, Json};
use faf_ml_core::{DatasetEntry, DatasetManifest};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    error::{Error, Result},
    state::{valid_dataset_name, AppState},
};

/// Request body for `POST /api/datasets`.
#[derive(Debug, Deserialize)]
pub struct CreateDatasetRequest {
    pub name: String,
    pub image_ids: Vec<Uuid>,
}

/// `GET /api/datasets` — list all snapshot manifests.
pub async fn list_datasets(State(state): State<AppState>) -> Result<Json<Vec<DatasetManifest>>> {
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(state.datasets_dir())? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let raw = std::fs::read_to_string(&path)?;
            manifests.push(serde_json::from_str::<DatasetManifest>(&raw)?);
        }
    }
    manifests.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(manifests))
}

/// `POST /api/datasets` — create an immutable snapshot embedding the current
/// labels of every requested image.
pub async fn create_dataset(
    State(state): State<AppState>,
    Json(req): Json<CreateDatasetRequest>,
) -> Result<Json<DatasetManifest>> {
    valid_dataset_name(&req.name)?;
    let path = state.dataset_path(&req.name);
    if path.exists() {
        return Err(Error::Conflict(format!(
            "dataset {:?} already exists (snapshots are immutable)",
            req.name
        )));
    }
    let mut entries = Vec::new();
    for image_id in &req.image_ids {
        if !state.image_path(*image_id).is_file() {
            return Err(Error::BadRequest(format!("unknown screenshot {image_id}")));
        }
        entries.push(DatasetEntry {
            image_id: *image_id,
            labels: super::labels::read_labels(&state, *image_id)?,
        });
    }
    let manifest = DatasetManifest {
        name: req.name,
        created_at: chrono::Utc::now(),
        entries,
    };
    std::fs::write(&path, serde_json::to_string_pretty(&manifest)?)?;
    Ok(Json(manifest))
}
