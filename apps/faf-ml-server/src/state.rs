//! Shared application state for Axum handlers.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use faf_blueprints::FafBlueprints;
use faf_ml_core::DatagenJob;
use uuid::Uuid;

use crate::error::{Error, Result};

/// State shared across all HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    pub data_dir: Arc<PathBuf>,
    pub assets_dir: Arc<PathBuf>,
    /// Strategic-icon sprite directory (`FAF_ML_ICONS_DIR`) for datagen jobs.
    pub icons_dir: Arc<PathBuf>,
    /// Unit portrait directory (`FAF_ML_PORTRAITS_DIR`) for the Units page.
    pub portraits_dir: Arc<PathBuf>,
    /// Unit blueprints backing `/api/units` (shared ETFreeman unit database).
    pub blueprints: Arc<FafBlueprints>,
    /// In-memory datagen job registry (progress is polled, not streamed).
    pub jobs: Arc<Mutex<HashMap<Uuid, DatagenJob>>>,
}

impl AppState {
    pub fn new(
        data_dir: PathBuf,
        assets_dir: PathBuf,
        icons_dir: PathBuf,
        portraits_dir: PathBuf,
    ) -> Result<Self> {
        let state = Self {
            data_dir: Arc::new(data_dir),
            assets_dir: Arc::new(assets_dir),
            icons_dir: Arc::new(icons_dir),
            portraits_dir: Arc::new(portraits_dir),
            blueprints: Arc::new(
                FafBlueprints::new()
                    .map_err(|e| Error::Internal(format!("loading unit blueprints: {e:#}")))?,
            ),
            jobs: Arc::new(Mutex::new(HashMap::new())),
        };
        std::fs::create_dir_all(state.screenshots_dir())?;
        std::fs::create_dir_all(state.labels_dir())?;
        std::fs::create_dir_all(state.datasets_dir())?;
        Ok(state)
    }

    pub fn screenshots_dir(&self) -> PathBuf {
        self.data_dir.join("screenshots")
    }

    pub fn labels_dir(&self) -> PathBuf {
        self.data_dir.join("labels")
    }

    pub fn datasets_dir(&self) -> PathBuf {
        self.data_dir.join("datasets")
    }

    pub fn index_path(&self) -> PathBuf {
        self.screenshots_dir().join("index.json")
    }

    pub fn classes_path(&self) -> PathBuf {
        self.data_dir.join("classes.txt")
    }

    pub fn image_path(&self, id: Uuid) -> PathBuf {
        self.screenshots_dir().join(format!("{id}.png"))
    }

    pub fn labels_path(&self, id: Uuid) -> PathBuf {
        self.labels_dir().join(format!("{id}.json"))
    }

    pub fn dataset_path(&self, name: &str) -> PathBuf {
        self.datasets_dir().join(format!("{name}.json"))
    }
}

/// Validate a dataset name so it is a safe file name (`<name>.json`).
pub fn valid_dataset_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if ok {
        Ok(())
    } else {
        Err(Error::BadRequest(format!(
            "invalid dataset name {name:?}: only [A-Za-z0-9._-] allowed"
        )))
    }
}
