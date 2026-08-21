//! Self-update UI: check the mirror for a newer build, download it, and
//! swap it in place (the mechanics live in `crate::update`).

use std::{path::PathBuf, sync::mpsc::channel, thread};

use eframe::egui;

use crate::{progress::format_bytes, update};

use super::{app::SyncApp, strings::*};

/// Self-update lifecycle: checked at startup and re-checked when the mirror
/// address changes; a newer build can be downloaded and swapped in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SelfUpdate {
    /// Fetching the mirror's client build tag.
    Checking,
    /// The running build matches (or is newer than) the mirror's.
    UpToDate,
    /// The mirror serves a newer build.
    Available { tag: String },
    /// Downloading the new exe: (done_bytes, total_bytes).
    Downloading { done: u64, total: u64 },
    /// Downloaded; swapping the exe and relaunching.
    Restarting,
    /// Check or download failed (hover the label for the error).
    Failed(String),
}

/// Messages from a self-update worker to the UI.
pub(super) enum UpdateMsg {
    /// Version check finished: the mirror's client build tag.
    Checked(Result<Option<String>, String>),
    /// Download progress: (done_bytes, total_bytes).
    Progress(u64, u64),
    /// Download finished: path of the `<exe>.new` file.
    Downloaded(Result<PathBuf, String>),
}

impl SyncApp {
    /// Kick off a one-shot check for a newer client build when the mirror
    /// address changed (and no update is already in flight).
    pub(super) fn maybe_start_update_check(&mut self) {
        if self.update_rx.is_some()
            || matches!(
                self.update,
                SelfUpdate::Downloading { .. } | SelfUpdate::Restarting
            )
        {
            return;
        }
        let server = self.server.trim().trim_end_matches('/').to_string();
        if server.is_empty() || server == self.update_checked_server {
            return;
        }
        self.update_checked_server = server.clone();
        self.update = SelfUpdate::Checking;
        let (tx, rx) = channel();
        self.update_rx = Some(rx);
        thread::spawn(move || {
            let tag = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(update::fetch_client_tag(&server))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(UpdateMsg::Checked(tag));
        });
    }

    /// Download the newer build from the mirror, then swap and relaunch.
    pub(super) fn start_self_update(&mut self) {
        let server = self.server.trim().trim_end_matches('/').to_string();
        let dest = match update::new_exe_path() {
            Ok(path) => path,
            Err(e) => {
                self.update = SelfUpdate::Failed(format!("{e:#}"));
                return;
            }
        };
        self.update = SelfUpdate::Downloading { done: 0, total: 0 };
        let (tx, rx) = channel();
        self.update_rx = Some(rx);
        thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(async {
                    update::download_client(&server, &dest, &mut |done, total| {
                        let _ = tx.send(UpdateMsg::Progress(done, total));
                    })
                    .await?;
                    Ok::<PathBuf, anyhow::Error>(dest)
                })
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(UpdateMsg::Downloaded(result));
        });
    }

    /// Apply messages from the self-update worker to the UI state.
    pub(super) fn drain_update(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;
        // Collect first: the receiver borrow must end before mutating state.
        let mut msgs = Vec::new();
        let mut disconnected = false;
        if let Some(rx) = &self.update_rx {
            loop {
                match rx.try_recv() {
                    Ok(msg) => msgs.push(msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let mut restart_with: Option<PathBuf> = None;
        let mut finished = false;
        for msg in msgs {
            match msg {
                UpdateMsg::Checked(Ok(tag)) => {
                    self.update = match tag {
                        Some(tag) if update::is_newer_build(&tag, crate::BUILD_TAG) => {
                            SelfUpdate::Available { tag }
                        }
                        _ => SelfUpdate::UpToDate,
                    };
                    finished = true;
                }
                UpdateMsg::Checked(Err(err)) => {
                    self.update = SelfUpdate::Failed(err);
                    finished = true;
                }
                UpdateMsg::Progress(done, total) => {
                    self.update = SelfUpdate::Downloading { done, total };
                }
                UpdateMsg::Downloaded(Ok(path)) => {
                    self.update = SelfUpdate::Restarting;
                    restart_with = Some(path);
                    finished = true;
                }
                UpdateMsg::Downloaded(Err(err)) => {
                    self.update = SelfUpdate::Failed(err);
                    finished = true;
                }
            }
        }
        if disconnected && !finished {
            self.update = SelfUpdate::Failed("update worker died".to_string());
        }
        if finished || disconnected {
            self.update_rx = None;
        }
        if self.update_rx.is_some() {
            // Keep repainting while the check/download is in flight.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Swapping the exe must happen on the UI thread; on success this
        // relaunches the new build and exits the process.
        if let Some(path) = restart_with {
            if let Err(e) = update::apply_and_restart(&path) {
                let err = format!("{e:#}");
                self.log.push(log_failed(self.lang, &err));
                self.update = SelfUpdate::Failed(err);
            }
        }
    }

    /// The always-visible self-update row below the tab bar: the button is
    /// enabled only when the mirror serves a newer build.
    pub(super) fn update_row(&mut self, ui: &mut egui::Ui) {
        // What to do after the row renders (borrowing `self.update` in the
        // closure prevents calling `&mut self` methods directly).
        enum UpdateAction {
            Start,
            RetryCheck,
        }
        let mut action: Option<UpdateAction> = None;
        ui.horizontal(|ui| match &self.update {
            SelfUpdate::Checking => {
                ui.label(
                    egui::RichText::new(tr(self.lang, Txt::UpdateChecking))
                        .small()
                        .weak(),
                );
                ui.add_enabled(
                    false,
                    egui::Button::new(tr(self.lang, Txt::UpdateNow)).small(),
                );
            }
            SelfUpdate::UpToDate => {
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        tr(self.lang, Txt::UpdateUpToDate),
                        crate::BUILD_TAG
                    ))
                    .small()
                    .weak(),
                );
                ui.add_enabled(
                    false,
                    egui::Button::new(tr(self.lang, Txt::UpdateNow)).small(),
                );
            }
            SelfUpdate::Available { tag } => {
                ui.colored_label(
                    egui::Color32::LIGHT_GREEN,
                    txt_update_available(self.lang, tag),
                );
                if ui.button(tr(self.lang, Txt::UpdateNow)).clicked() {
                    action = Some(UpdateAction::Start);
                }
            }
            SelfUpdate::Downloading { .. } => {
                ui.label(tr(self.lang, Txt::UpdateDownloading));
                ui.add_enabled(
                    false,
                    egui::Button::new(tr(self.lang, Txt::UpdateNow)).small(),
                );
            }
            SelfUpdate::Restarting => {
                ui.colored_label(
                    egui::Color32::LIGHT_GREEN,
                    tr(self.lang, Txt::UpdateRestarting),
                );
            }
            SelfUpdate::Failed(err) => {
                ui.colored_label(egui::Color32::LIGHT_RED, tr(self.lang, Txt::UpdateFailed))
                    .on_hover_text(err);
                if ui.button(tr(self.lang, Txt::UpdateRetry)).clicked() {
                    action = Some(UpdateAction::RetryCheck);
                }
            }
        });
        if let SelfUpdate::Downloading { done, total } = self.update {
            let (fraction, text) = if total > 0 {
                (
                    done as f32 / total as f32,
                    format!("{} / {}", format_bytes(done), format_bytes(total)),
                )
            } else {
                (0.0, format_bytes(done))
            };
            ui.add(egui::ProgressBar::new(fraction).text(text));
        }
        match action {
            Some(UpdateAction::Start) => self.start_self_update(),
            // Retry re-runs the version check against the current mirror.
            Some(UpdateAction::RetryCheck) => {
                self.update_checked_server.clear();
                self.update = SelfUpdate::Checking;
            }
            None => {}
        }
    }
}
