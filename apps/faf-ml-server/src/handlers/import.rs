//! `POST /api/import/datagen` — import a faf-datagen output directory.
//!
//! A datagen output dir contains `classes.txt` (line no. = class id),
//! `images/*.png`, and `labels/<stem>.txt` with YOLO lines
//! (`<class_id> <cx> <cy> <w> <h>`, normalized). Import copies every image in
//! as a new screenshot, converts its YOLO labels to absolute-pixel JSON
//! labels, and merges the source class list into the store's `classes.txt`.

use std::path::PathBuf;

use axum::{extract::State, Json};
use faf_ml_core::LabeledBox;
use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    state::AppState,
};

/// Request body for `POST /api/import/datagen`.
#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    /// Path to the faf-datagen output directory (on the server's filesystem).
    pub dir: String,
}

/// Response body: what the import did.
#[derive(Debug, Serialize)]
pub struct ImportSummary {
    /// Number of screenshots imported.
    pub imported: usize,
    /// Number of new class names appended to `classes.txt`.
    pub classes_added: usize,
}

/// Parse the class list of a datagen output dir.
fn read_source_classes(dir: &std::path::Path) -> Result<Vec<String>> {
    let raw = std::fs::read_to_string(dir.join("classes.txt"))
        .map_err(|e| Error::BadRequest(format!("cannot read {}: {e}", dir.display())))?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}

/// Merge `source` class names into the store's `classes.txt`, returning the
/// count of newly appended names.
fn merge_classes(state: &AppState, source: &[String]) -> Result<usize> {
    let mut classes = super::classes::read_classes(state)?;
    let before = classes.len();
    for name in source {
        if !classes.contains(name) {
            classes.push(name.clone());
        }
    }
    let added = classes.len() - before;
    if added > 0 {
        let mut raw = classes.join("\n");
        raw.push('\n');
        std::fs::write(state.classes_path(), raw)?;
    }
    Ok(added)
}

/// `POST /api/import/datagen` — see module docs.
pub async fn import_datagen(
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> Result<Json<ImportSummary>> {
    let dir = PathBuf::from(&req.dir);
    if !dir.is_dir() {
        return Err(Error::BadRequest(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let source_classes = read_source_classes(&dir)?;
    let classes_added = merge_classes(&state, &source_classes)?;

    let images_dir = dir.join("images");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&images_dir)
        .map_err(|e| Error::BadRequest(format!("cannot read {}: {e}", images_dir.display())))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "png"))
        .collect();
    entries.sort();

    let mut imported = 0;
    for image_path in entries {
        let bytes = std::fs::read(&image_path)?;
        let filename = image_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.png");
        let meta = super::screenshots::store_screenshot(&state, filename, &bytes)?;

        // Convert the matching YOLO label file (same stem) to JSON labels.
        let label_path = dir.join("labels").join(format!(
            "{}.txt",
            image_path.file_stem().unwrap().to_string_lossy()
        ));
        let labels: Vec<LabeledBox> = match std::fs::read_to_string(&label_path) {
            Ok(raw) => raw
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(|line| {
                    LabeledBox::from_yolo(line, &source_classes, meta.width, meta.height)
                        .map_err(|e| Error::BadRequest(format!("{}: {e}", label_path.display())))
                })
                .collect::<Result<_>>()?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        if !labels.is_empty() {
            std::fs::write(
                state.labels_path(meta.id),
                serde_json::to_string_pretty(&labels)?,
            )?;
        }
        imported += 1;
    }

    Ok(Json(ImportSummary {
        imported,
        classes_added,
    }))
}
