//! Version-status panel on the sync tab and the upstream half of the
//! 检查更新 button: fetches `GET /api/gamedata/status` (at startup, when the
//! mirror address changes, and after each manual check) and renders one
//! freshness row per updatable component.

use std::{
    cmp::Ordering,
    sync::mpsc::channel,
    thread,
    time::{Duration, Instant},
};

use eframe::egui;
use fafcn_gamedata::{
    compare_version_strings, StatusResponse, UpdaterComponent, UpdaterState, CHANNEL_FAF_CLIENT,
    CHANNEL_GAMEDATA, CHANNEL_MAP_GENERATOR,
};

use crate::{sync, update};

use super::{app::SyncApp, strings::*};

/// How often the manual check polls the mirror status.
const CHECK_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The manual check polls at most this long — version discovery is fast;
/// we never wait for downloads.
const CHECK_POLL_TIMEOUT: Duration = Duration::from_secs(15);

/// When the server is downloading, keep watching this long so the conclusion
/// reflects the FINAL state (e.g. "已是最新" once the download commits).
const CHECK_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Poll interval while the server is downloading.
const DOWNLOAD_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Messages from a panel worker to the UI.
pub(super) enum PanelMsg {
    /// Silent status fetch (startup / mirror change) finished.
    Status(Result<StatusResponse, String>),
    /// Manual 检查更新 upstream refresh finished.
    Checked(Result<StatusResponse, String>),
}

/// Whether the mirror's version of a component matches upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MirrorFreshness {
    /// Mirror matches (or exceeds) the upstream version.
    Current,
    /// Mirror is behind; the server should already be downloading.
    Behind {
        /// Latest version seen upstream.
        latest: String,
    },
    /// Upstream version unknown (server never checked / is too old).
    Unknown,
}

/// Compare the mirror's version of a component with the latest upstream
/// version. Pure, so the panel and the manual-check conclusions agree.
pub(super) fn mirror_freshness(mirror: Option<&str>, upstream: Option<&str>) -> MirrorFreshness {
    let Some(latest) = upstream else {
        return MirrorFreshness::Unknown;
    };
    match mirror {
        Some(m) if m == latest => MirrorFreshness::Current,
        Some(m) => match compare_version_strings(latest, m) {
            Some(Ordering::Greater) => MirrorFreshness::Behind {
                latest: latest.to_string(),
            },
            Some(_) => MirrorFreshness::Current,
            None => MirrorFreshness::Unknown,
        },
        None => MirrorFreshness::Behind {
            latest: latest.to_string(),
        },
    }
}

/// Run `future` on a fresh current-thread tokio runtime (worker threads have
/// none). Same pattern as the other GUI workers.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

impl SyncApp {
    /// Silent one-shot status fetch for the version panel: fires once per
    /// mirror address (startup and whenever it changes).
    pub(super) fn maybe_fetch_panel_status(&mut self) {
        if self.panel_rx.is_some() {
            return;
        }
        let server = self.server.trim().trim_end_matches('/').to_string();
        if server.is_empty() || server == self.panel_checked_server {
            return;
        }
        self.panel_checked_server = server.clone();
        let (tx, rx) = channel();
        self.panel_rx = Some(rx);
        thread::spawn(move || {
            let result = block_on(async move {
                let http = reqwest::Client::new();
                sync::fetch_status(&http, &server).await
            })
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(PanelMsg::Status(result));
        });
    }

    /// The upstream half of 检查更新: POST the (server-side debounced)
    /// refresh endpoint, then poll the status until the check finished —
    /// stop when `last_check_at` changed or the updater left `Checking`.
    pub(super) fn start_manual_upstream_check(&mut self) {
        if self.check_rx.is_some() {
            return;
        }
        let server = self.server.trim().trim_end_matches('/').to_string();
        if server.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        self.check_rx = Some(rx);
        let tx_done = tx.clone();
        thread::spawn(move || {
            let result = block_on(async move {
                let http = reqwest::Client::new();
                let before = sync::fetch_status(&http, &server).await.ok();
                let before_check_at = before
                    .as_ref()
                    .and_then(|s| s.updater.as_ref())
                    .and_then(|u| u.last_check_at);
                // Fire-and-forget: the server debounces concurrent triggers.
                let _ = sync::fetch_upstream_refresh(&http, &server).await;
                let deadline = Instant::now() + CHECK_POLL_TIMEOUT;
                let mut status = loop {
                    let status = sync::fetch_status(&http, &server).await?;
                    let updater = status.updater.as_ref();
                    let changed = updater.and_then(|u| u.last_check_at) != before_check_at;
                    let checking =
                        matches!(updater.map(|u| &u.state), Some(UpdaterState::Checking));
                    if changed || !checking || Instant::now() >= deadline {
                        break status;
                    }
                    tokio::time::sleep(CHECK_POLL_INTERVAL).await;
                };
                // The version is known, but a download may still be running:
                // keep watching until the updater finishes so the conclusion
                // reflects the final state (matched → "已是最新"). The panel
                // gets live snapshots so the blue downloading row shows up.
                let deadline = Instant::now() + CHECK_DOWNLOAD_TIMEOUT;
                while matches!(
                    status.updater.as_ref().map(|u| &u.state),
                    Some(UpdaterState::Downloading { .. })
                ) && Instant::now() < deadline
                {
                    let _ = tx.send(PanelMsg::Status(Ok(status.clone())));
                    tokio::time::sleep(DOWNLOAD_POLL_INTERVAL).await;
                    status = sync::fetch_status(&http, &server).await?;
                }
                anyhow::Ok(status)
            })
            .map_err(|e: anyhow::Error| format!("{e:#}"));
            let _ = tx_done.send(PanelMsg::Checked(result));
        });
    }

    /// Apply messages from the panel/check workers to the UI state.
    pub(super) fn drain_panel(&mut self, ctx: &egui::Context) {
        use std::sync::mpsc::TryRecvError;
        // Collect first: the receiver borrow must end before mutating state.
        // The silent panel fetch sends exactly one message; the manual-check
        // worker streams live Status snapshots while the server downloads and
        // is retired only by its terminal Checked message (or a disconnect).
        let mut msgs = Vec::new();
        let mut finished = [false; 2];
        for (i, rx) in [&self.panel_rx, &self.check_rx].into_iter().enumerate() {
            let Some(rx) = rx else { continue };
            loop {
                match rx.try_recv() {
                    Ok(msg) => {
                        let terminal = i == 0 || matches!(msg, PanelMsg::Checked(_));
                        msgs.push(msg);
                        if terminal {
                            finished[i] = true;
                            break;
                        }
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished[i] = true;
                        break;
                    }
                }
            }
        }
        if finished[0] {
            self.panel_rx = None;
        }
        if finished[1] {
            self.check_rx = None;
        }
        for msg in msgs {
            match msg {
                PanelMsg::Status(Ok(status)) => self.panel_status = Some(status),
                // Silent fetch: errors only matter for the manual check.
                PanelMsg::Status(Err(_)) => {}
                PanelMsg::Checked(Ok(status)) => {
                    self.log_upstream_conclusions(&status);
                    self.panel_status = Some(status);
                }
                PanelMsg::Checked(Err(err)) => {
                    self.log.push(log_failed(self.lang, &err));
                }
            }
        }
        if self.panel_rx.is_some() || self.check_rx.is_some() {
            // Keep repainting while a fetch/check is in flight.
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// Log the 检查更新 conclusions, one line each for gamedata and the
    /// FAF client (the client self-update conclusion is logged separately).
    fn log_upstream_conclusions(&mut self, status: &StatusResponse) {
        let mirror = |channel: &str| {
            status
                .channels
                .iter()
                .find(|c| c.name == channel)
                .and_then(|c| c.manifest.as_ref())
                .map(|m| m.patch_version.as_str())
        };
        let Some(updater) = &status.updater else {
            let reason = tr(self.lang, Txt::UpstreamStatusUnknown);
            self.log.push(log_upstream_skipped(self.lang, reason));
            return;
        };
        match mirror_freshness(
            mirror(CHANNEL_GAMEDATA),
            updater.latest_official_version.as_deref(),
        ) {
            MirrorFreshness::Current => {
                let version = updater
                    .latest_official_version
                    .as_deref()
                    .or_else(|| mirror(CHANNEL_GAMEDATA))
                    .unwrap_or("?");
                self.log.push(log_gamedata_current(self.lang, version));
            }
            MirrorFreshness::Behind { latest } => {
                self.log.push(log_upstream_downloading(self.lang, &latest));
            }
            MirrorFreshness::Unknown => {
                let reason = tr(self.lang, Txt::UpstreamStatusUnknown);
                self.log.push(log_upstream_skipped(self.lang, reason));
            }
        }
        match mirror_freshness(
            mirror(CHANNEL_FAF_CLIENT),
            updater.latest_client_version.as_deref(),
        ) {
            MirrorFreshness::Current => {
                let version = updater
                    .latest_client_version
                    .as_deref()
                    .or_else(|| mirror(CHANNEL_FAF_CLIENT))
                    .unwrap_or("?");
                self.log.push(log_client_current(self.lang, version));
            }
            MirrorFreshness::Behind { latest } => {
                self.log.push(log_client_new(self.lang, &latest));
            }
            MirrorFreshness::Unknown => {
                let reason = tr(self.lang, Txt::UpstreamStatusUnknown);
                self.log.push(log_upstream_skipped(self.lang, reason));
            }
        }
    }

    /// The version panel below the update row: one freshness row per
    /// updatable component (green aligned / yellow behind / blue
    /// downloading). Rendered only once a status snapshot arrived.
    pub(super) fn version_panel(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.panel_status.clone() else {
            return;
        };
        ui.label(egui::RichText::new(tr(self.lang, Txt::PanelTitle)).weak());
        let mirror = |channel: &str| {
            status
                .channels
                .iter()
                .find(|c| c.name == channel)
                .and_then(|c| c.manifest.as_ref())
                .map(|m| m.patch_version.as_str())
        };
        let downloading = match status.updater.as_ref().map(|u| &u.state) {
            Some(UpdaterState::Downloading { component, version }) => {
                Some((*component, version.as_str()))
            }
            _ => None,
        };

        // fafcn-sync client: local BUILD_TAG vs the mirror's client build.
        let client_text = match &status.client_tag {
            Some(tag) if update::is_newer_build(tag, crate::BUILD_TAG) => {
                (egui::Color32::YELLOW, txt_update_available(self.lang, tag))
            }
            _ => (
                egui::Color32::LIGHT_GREEN,
                txt_current_version(self.lang, crate::BUILD_TAG),
            ),
        };
        self.panel_row(
            ui,
            tr(self.lang, Txt::PanelSyncClient),
            None,
            None,
            client_text,
        );

        // gamedata patch and FAF client: mirror channel vs upstream version.
        let updater = status.updater.as_ref();
        for (label, component, channel, upstream) in [
            (
                tr(self.lang, Txt::PanelGamedata),
                UpdaterComponent::Gamedata,
                CHANNEL_GAMEDATA,
                updater.and_then(|u| u.latest_official_version.as_deref()),
            ),
            (
                tr(self.lang, Txt::ChannelFafClient),
                UpdaterComponent::FafClient,
                CHANNEL_FAF_CLIENT,
                updater.and_then(|u| u.latest_client_version.as_deref()),
            ),
        ] {
            let text = self.freshness_text(component, mirror(channel), upstream, downloading);
            self.panel_row(ui, label, mirror(channel), upstream, text);
        }

        // Map generator: no trustworthy upstream endpoint — display only.
        let version = mirror(CHANNEL_MAP_GENERATOR)
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| tr(self.lang, Txt::NotPublished).to_string());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr(self.lang, Txt::ChannelMapGenerator)).weak());
            ui.label(egui::RichText::new(version).weak());
        });
    }

    /// One panel row: label, mirror version vs upstream version (weak),
    /// colored status text.
    fn panel_row(
        &self,
        ui: &mut egui::Ui,
        label: &str,
        mirror: Option<&str>,
        upstream: Option<&str>,
        (color, text): (egui::Color32, String),
    ) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).weak());
            if let Some(version) = mirror {
                ui.label(
                    egui::RichText::new(format!("{} v{version}", tr(self.lang, Txt::PanelMirror)))
                        .weak(),
                );
            }
            if let Some(version) = upstream {
                ui.label(
                    egui::RichText::new(format!(
                        "{} v{version}",
                        tr(self.lang, Txt::PanelOfficial)
                    ))
                    .weak(),
                );
            }
            ui.label(egui::RichText::new(text).color(color));
        });
    }

    /// Colored status text for one channel row (green aligned / yellow
    /// behind / blue when the updater is downloading a newer version of
    /// THIS component).
    fn freshness_text(
        &self,
        component: UpdaterComponent,
        mirror: Option<&str>,
        upstream: Option<&str>,
        downloading: Option<(UpdaterComponent, &str)>,
    ) -> (egui::Color32, String) {
        if let Some((dl_component, version)) = downloading {
            if dl_component == component && mirror != Some(version) {
                return (
                    egui::Color32::LIGHT_BLUE,
                    format!("{} v{version}…", tr(self.lang, Txt::ServerDownloading)),
                );
            }
        }
        match mirror_freshness(mirror, upstream) {
            MirrorFreshness::Current => {
                let version = mirror.or(upstream).unwrap_or("?");
                (
                    egui::Color32::LIGHT_GREEN,
                    txt_current_version(self.lang, version),
                )
            }
            MirrorFreshness::Behind { latest } => {
                (egui::Color32::YELLOW, txt_new_version(self.lang, &latest))
            }
            MirrorFreshness::Unknown => (
                egui::Color32::GRAY,
                tr(self.lang, Txt::VersionUnknown).to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_current_when_versions_match_or_mirror_ahead() {
        assert_eq!(
            mirror_freshness(Some("3838"), Some("3838")),
            MirrorFreshness::Current
        );
        assert_eq!(
            mirror_freshness(Some("1.6.3"), Some("1.6.3")),
            MirrorFreshness::Current
        );
        // Manual upload ahead of upstream is not "behind".
        assert_eq!(
            mirror_freshness(Some("3839"), Some("3838")),
            MirrorFreshness::Current
        );
    }

    #[test]
    fn freshness_behind_when_upstream_newer_or_mirror_empty() {
        assert_eq!(
            mirror_freshness(Some("1.6.3"), Some("2026.7.1")),
            MirrorFreshness::Behind {
                latest: "2026.7.1".to_string()
            }
        );
        assert_eq!(
            mirror_freshness(None, Some("3838")),
            MirrorFreshness::Behind {
                latest: "3838".to_string()
            }
        );
    }

    #[test]
    fn freshness_unknown_without_upstream_version() {
        assert_eq!(
            mirror_freshness(Some("3838"), None),
            MirrorFreshness::Unknown
        );
        assert_eq!(mirror_freshness(None, None), MirrorFreshness::Unknown);
        // Unparseable versions cannot be ordered.
        assert_eq!(
            mirror_freshness(Some("abc"), Some("xyz")),
            MirrorFreshness::Unknown
        );
    }
}
