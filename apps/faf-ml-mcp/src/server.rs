//! The faf-ml MCP server: workflow-level tools over the faf-ml-server API.
//!
//! Tool surface is intentionally coarse (one tool per workflow action, not
//! per REST endpoint) so an LLM can drive the whole unit-detection pipeline
//! — collect → triage → generate → snapshot → train — without knowing HTTP
//! details. The REST API stays canonical; everything here is a thin wrapper.

use faf_ml_core::{DatagenConfig, DatagenJob, ScreenshotMeta, TrainingCommand, TrainingConfig};
use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};
use serde::Deserialize;

use crate::{api::Api, training::TrainingManager};

#[derive(Clone)]
pub struct FafMl {
    api: Api,
    training: TrainingManager,
}

impl FafMl {
    pub fn new() -> Self {
        Self {
            api: Api::from_env(),
            training: TrainingManager::default(),
        }
    }
}

// ── parameter structs ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    /// Filter by pool: unclassified | battle | background | synthetic
    /// (default: all pools).
    kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UploadParams {
    /// Absolute paths of PNG screenshot files on this machine.
    paths: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TriageParams {
    /// Screenshot id (uuid).
    id: String,
    /// New pool: battle (real units, held-out test) | background (empty
    /// terrain, datagen canvas) | unclassified (back to triage queue).
    kind: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct IdParams {
    /// Screenshot or datagen-job id (uuid).
    id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DatagenStartParams {
    /// Samples to generate (default 200).
    count: Option<usize>,
    /// Square crop side in px (default 640).
    size: Option<u32>,
    /// Max icons pasted per sample (default 25).
    max_units: Option<usize>,
    /// Sprite scale range vs the 36×40 source (defaults 0.35–0.65;
    /// 0.35 ≈ 13 px on screen — match real screenshots).
    scale_min: Option<f32>,
    scale_max: Option<f32>,
    /// RNG seed (default 42; same config + seed = same set).
    seed: Option<u64>,
    /// Icon classes to exclude (default: all 193 included).
    exclude_classes: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DatasetCreateParams {
    /// Snapshot name (immutable while it exists; [A-Za-z0-9._-]).
    name: String,
    /// Pools to freeze in, default ["synthetic"]. For detector training,
    /// synthetic alone is usually right; battle stays held out unless its
    /// labels were corrected.
    kinds: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NameParams {
    /// Dataset snapshot name.
    name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrainingStartParams {
    /// Epochs (default 50).
    epochs: Option<usize>,
    /// Batch size (default 4 — the real detector's GPU cap).
    batch_size: Option<usize>,
    /// Learning rate (default 0.001).
    lr: Option<f64>,
    /// Tick rate in batches/sec for the dummy pipeline (default 10).
    speed: Option<f64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrainingStatusParams {
    /// Run handle from training_start; omit for the most recent run.
    handle: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TrainingCommandParams {
    /// Run handle from training_start.
    handle: String,
    /// pause | resume | stop | set_speed
    command: String,
    /// Required when command = set_speed.
    batches_per_sec: Option<f64>,
}

// ── tools ───────────────────────────────────────────────────────────────────

#[tool_router(server_handler)]
impl FafMl {
    #[tool(
        description = "Platform orientation: API reachability, screenshot counts per \
                          pool (unclassified/battle/background/synthetic), datasets. Call \
                          this first to understand the current data state."
    )]
    async fn faf_ml_status(&self) -> String {
        match self.run_status().await {
            Ok(text) => text,
            Err(e) => format!(
                "error: {e:#} (is faf-ml-server running on {}?)",
                self.api.base()
            ),
        }
    }

    #[tool(
        description = "List screenshots (id, filename, kind, dimensions, upload time), \
                          optionally filtered by pool."
    )]
    async fn faf_ml_screenshots_list(&self, Parameters(p): Parameters<ListParams>) -> String {
        match self
            .api
            .get::<Vec<ScreenshotMeta>>("/api/screenshots")
            .await
        {
            Ok(metas) => {
                let metas: Vec<_> = metas
                    .into_iter()
                    .filter(|m| p.kind.as_deref().is_none_or(|k| m.kind.as_str() == k))
                    .collect();
                if metas.is_empty() {
                    return "no screenshots match".to_string();
                }
                let mut out = format!("{} screenshot(s):\n", metas.len());
                for m in &metas {
                    out.push_str(&format!(
                        "- {} · {} · {}×{} · {}\n",
                        m.id, m.kind, m.width, m.height, m.filename
                    ));
                }
                out
            }
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Upload PNG screenshots from local file paths (they land in the \
                          unclassified pool — triage them afterwards)."
    )]
    async fn faf_ml_screenshots_upload(&self, Parameters(p): Parameters<UploadParams>) -> String {
        match self.api.upload::<Vec<ScreenshotMeta>>(&p.paths).await {
            Ok(metas) => format!(
                "uploaded {} screenshot(s): {}",
                metas.len(),
                metas
                    .iter()
                    .map(|m| format!("{} ({})", m.filename, m.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Triage a screenshot: mark it battle (held-out test) or background \
                          (datagen canvas). Datagen reads ONLY background shots; real units \
                          in the background pool poison the synthetic labels."
    )]
    async fn faf_ml_screenshot_triage(&self, Parameters(p): Parameters<TriageParams>) -> String {
        #[derive(serde::Serialize)]
        struct KindUpdate {
            kind: String,
        }
        match self
            .api
            .patch_json::<_, ScreenshotMeta>(
                &format!("/api/screenshots/{}", p.id),
                &KindUpdate { kind: p.kind },
            )
            .await
        {
            Ok(meta) => format!("{} is now marked {}", meta.id, meta.kind),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(description = "Delete one screenshot (image + labels).")]
    async fn faf_ml_screenshot_delete(&self, Parameters(p): Parameters<IdParams>) -> String {
        match self.api.delete(&format!("/api/screenshots/{}", p.id)).await {
            Ok(_) => format!("deleted screenshot {}", p.id),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Start a synthetic-data generation job: pastes strategic-icon \
                          sprites onto crops of background-marked screenshots, producing \
                          perfectly labeled training samples. Poll progress with \
                          faf_ml_datagen_jobs."
    )]
    async fn faf_ml_datagen_start(&self, Parameters(p): Parameters<DatagenStartParams>) -> String {
        let mut config = DatagenConfig::default();
        if let Some(v) = p.count {
            config.count = v;
        }
        if let Some(v) = p.size {
            config.size = v;
        }
        if let Some(v) = p.max_units {
            config.max_units = v;
        }
        if let Some(v) = p.scale_min {
            config.scale_min = v;
        }
        if let Some(v) = p.scale_max {
            config.scale_max = v;
        }
        if let Some(v) = p.seed {
            config.seed = v;
        }
        if let Some(v) = p.exclude_classes {
            config.exclude_classes = v;
        }
        match self
            .api
            .post_json::<_, DatagenJob>("/api/datagen", &config)
            .await
        {
            Ok(job) => format!(
                "datagen job {} started ({} samples, seed {}) — poll faf_ml_datagen_jobs",
                job.id, job.config.count, job.config.seed
            ),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(description = "List datagen jobs with progress (running x/y, done, failed).")]
    async fn faf_ml_datagen_jobs(&self) -> String {
        match self.api.get::<Vec<DatagenJob>>("/api/datagen/jobs").await {
            Ok(jobs) if jobs.is_empty() => "no datagen jobs".to_string(),
            Ok(jobs) => {
                let mut out = String::new();
                for j in jobs {
                    let status = match &j.status {
                        faf_ml_core::DatagenStatus::Running { done, total } => {
                            format!("running {done}/{total}")
                        }
                        faf_ml_core::DatagenStatus::Done { generated } => {
                            format!("done — {generated} samples")
                        }
                        faf_ml_core::DatagenStatus::Failed { error } => {
                            format!("failed: {error}")
                        }
                    };
                    out.push_str(&format!(
                        "- {} · {} · count {} · scale {:.2}-{:.2} · seed {}\n",
                        j.id,
                        status,
                        j.config.count,
                        j.config.scale_min,
                        j.config.scale_max,
                        j.config.seed
                    ));
                }
                out
            }
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Delete a finished datagen job AND the whole sample set it \
                          generated (the way to scrap a bad generation run)."
    )]
    async fn faf_ml_datagen_job_delete(&self, Parameters(p): Parameters<IdParams>) -> String {
        match self
            .api
            .delete(&format!("/api/datagen/jobs/{}", p.id))
            .await
        {
            Ok(msg) => msg,
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Delete ALL synthetic samples (and drop finished datagen jobs). \
                          Use before regenerating a full set with new parameters."
    )]
    async fn faf_ml_synthetic_clear(&self) -> String {
        match self.api.delete("/api/screenshots?kind=synthetic").await {
            Ok(msg) => msg,
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Create an immutable dataset snapshot: freezes the chosen pools' \
                          images + current labels so a training run is reproducible. \
                          Default is synthetic-only (the training set)."
    )]
    async fn faf_ml_dataset_create(
        &self,
        Parameters(p): Parameters<DatasetCreateParams>,
    ) -> String {
        #[derive(serde::Serialize)]
        struct Create {
            name: String,
            kinds: Vec<String>,
        }
        let kinds = p.kinds.unwrap_or_else(|| vec!["synthetic".to_string()]);
        match self
            .api
            .post_json::<_, faf_ml_core::DatasetManifest>(
                "/api/datasets",
                &Create {
                    name: p.name,
                    kinds,
                },
            )
            .await
        {
            Ok(manifest) => {
                let boxes: usize = manifest.entries.iter().map(|e| e.labels.len()).sum();
                format!(
                    "snapshot {:?} created: {} images · {} boxes",
                    manifest.name,
                    manifest.entries.len(),
                    boxes
                )
            }
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(description = "List dataset snapshots (name, image/box counts, created time).")]
    async fn faf_ml_datasets_list(&self) -> String {
        match self
            .api
            .get::<Vec<faf_ml_core::DatasetManifest>>("/api/datasets")
            .await
        {
            Ok(list) if list.is_empty() => "no dataset snapshots".to_string(),
            Ok(list) => {
                let mut out = String::new();
                for ds in list {
                    let boxes: usize = ds.entries.iter().map(|e| e.labels.len()).sum();
                    out.push_str(&format!(
                        "- {} · {} images · {} boxes · {}\n",
                        ds.name,
                        ds.entries.len(),
                        boxes,
                        ds.created_at.format("%Y-%m-%d %H:%M UTC")
                    ));
                }
                out
            }
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Delete a dataset snapshot file (immutable = never mutated, \
                          not undeletable)."
    )]
    async fn faf_ml_dataset_delete(&self, Parameters(p): Parameters<NameParams>) -> String {
        match self.api.delete(&format!("/api/datasets/{}", p.name)).await {
            Ok(_) => format!("deleted snapshot {:?}", p.name),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Start a training run (currently a dummy pipeline generating \
                          realistic loss/mAP curves over the same WebSocket path the real \
                          burn training will use). Returns a run handle for \
                          faf_ml_training_status / faf_ml_training_command."
    )]
    async fn faf_ml_training_start(
        &self,
        Parameters(p): Parameters<TrainingStartParams>,
    ) -> String {
        let mut config = TrainingConfig::default();
        if let Some(v) = p.epochs {
            config.epochs = v;
        }
        if let Some(v) = p.batch_size {
            config.batch_size = v;
        }
        if let Some(v) = p.lr {
            config.lr = v;
        }
        match self
            .training
            .start(self.api.base(), config, p.speed.unwrap_or(10.0))
            .await
        {
            Ok(handle) => format!(
                "training started, handle {handle} — poll faf_ml_training_status for \
                 epoch/batch progress and live losses"
            ),
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(
        description = "Training progress: status, metrics points received, latest \
                          epoch/batch and train/cls/bbox/valid losses + mAP."
    )]
    async fn faf_ml_training_status(
        &self,
        Parameters(p): Parameters<TrainingStatusParams>,
    ) -> String {
        match self.training.status(p.handle.as_deref()).await {
            Ok(state) => {
                let mut out = format!(
                    "status: {} · {} metrics points{}",
                    if state.status.is_empty() {
                        "connecting"
                    } else {
                        &state.status
                    },
                    state.points,
                    if state.detail.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", state.detail)
                    }
                );
                if let Some(l) = state.latest {
                    out.push_str(&format!(
                        "\nepoch {} · batch {}/{} · train {:.4} · cls {:.4} · bbox {:.4}",
                        l.epoch, l.batch, l.total_batches, l.train_loss, l.cls_loss, l.bbox_loss
                    ));
                    if let Some(v) = l.valid_loss {
                        out.push_str(&format!(" · valid {v:.4}"));
                    }
                    if let Some(m) = l.map {
                        out.push_str(&format!(" · mAP {m:.3}"));
                    }
                }
                out
            }
            Err(e) => format!("error: {e:#}"),
        }
    }

    #[tool(description = "Control a training run: pause / resume / stop / set_speed.")]
    async fn faf_ml_training_command(
        &self,
        Parameters(p): Parameters<TrainingCommandParams>,
    ) -> String {
        let cmd = match p.command.as_str() {
            "pause" => TrainingCommand::Pause,
            "resume" => TrainingCommand::Resume,
            "stop" => TrainingCommand::Stop,
            "set_speed" => match p.batches_per_sec {
                Some(v) => TrainingCommand::SetSpeed { batches_per_sec: v },
                None => return "error: set_speed requires batches_per_sec".to_string(),
            },
            other => return format!("error: unknown command {other:?}"),
        };
        match self.training.command(&p.handle, cmd).await {
            Ok(()) => format!("sent {} to {}", p.command, p.handle),
            Err(e) => format!("error: {e:#}"),
        }
    }
}

// ── helpers ─────────────────────────────────────────────────────────────────

impl FafMl {
    async fn run_status(&self) -> anyhow::Result<String> {
        let metas = self
            .api
            .get::<Vec<ScreenshotMeta>>("/api/screenshots")
            .await?;
        let mut counts = std::collections::HashMap::new();
        for m in &metas {
            *counts.entry(m.kind.as_str()).or_insert(0usize) += 1;
        }
        let datasets = self
            .api
            .get::<Vec<faf_ml_core::DatasetManifest>>("/api/datasets")
            .await?;
        Ok(format!(
            "api: {} reachable\nscreenshots: {} (unclassified {}, battle {}, background {}, synthetic {})\ndataset snapshots: {}",
            self.api.base(),
            metas.len(),
            counts.get("unclassified").unwrap_or(&0),
            counts.get("battle").unwrap_or(&0),
            counts.get("background").unwrap_or(&0),
            counts.get("synthetic").unwrap_or(&0),
            datasets.len(),
        ))
    }
}
