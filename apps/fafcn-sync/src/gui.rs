//! eframe GUI for non-technical players: pick the gamedata folder (or let
//! auto-detect find it), click one button, done.

use std::{
    path::PathBuf,
    sync::mpsc::{channel, Receiver},
    thread,
};

use anyhow::Result;
use eframe::egui;

use crate::{
    config::ClientConfig,
    sync::{self, SyncProgress, SyncSummary},
};

/// Launch the GUI. Blocks until the window closes.
pub fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 480.0])
            .with_min_inner_size([480.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "fafcn-sync",
        options,
        Box::new(|cc| {
            install_cjk_font(cc);
            Ok(Box::new(SyncApp::new()))
        }),
    )
    .map_err(|e| anyhow::anyhow!("gui error: {e}"))
}

/// egui's bundled fonts have no CJK glyphs, so Chinese text renders as
/// boxes. Load the operating system's CJK font (Microsoft YaHei is present
/// on every Chinese Windows install) and register it as a fallback for both
/// font families — Latin text keeps using egui's default font.
fn install_cjk_font(cc: &eframe::CreationContext<'_>) {
    let Some(bytes) = load_system_cjk_font() else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(bytes).into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_owned());
    }
    cc.egui_ctx.set_fonts(fonts);
}

/// Read the first available OS CJK font.
fn load_system_cjk_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        // Windows: Microsoft YaHei / SimHei / SimSun (always present on zh systems).
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        // Linux: Noto CJK / WenQuanYi (for development).
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        // macOS.
        "/System/Library/Fonts/PingFang.ttc",
    ];
    CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
}

/// UI language (target audience is Chinese players, so Zh is the default).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuiLang {
    Zh,
    En,
}

impl GuiLang {
    fn from_config(value: Option<&str>) -> Self {
        match value {
            Some("en") => GuiLang::En,
            _ => GuiLang::Zh,
        }
    }

    fn code(self) -> &'static str {
        match self {
            GuiLang::Zh => "zh",
            GuiLang::En => "en",
        }
    }
}

/// Translatable GUI strings.
#[derive(Debug, Clone, Copy)]
enum Txt {
    ServerLabel,
    DirLabel,
    Browse,
    Detect,
    DirValid,
    DirSuspicious,
    DirMissing,
    SyncNow,
    Syncing,
    IdleHint,
}

fn tr(lang: GuiLang, txt: Txt) -> &'static str {
    match (txt, lang) {
        (Txt::ServerLabel, GuiLang::Zh) => "镜像地址",
        (Txt::ServerLabel, GuiLang::En) => "Mirror address",
        (Txt::DirLabel, GuiLang::Zh) => "gamedata 目录",
        (Txt::DirLabel, GuiLang::En) => "gamedata folder",
        (Txt::Browse, GuiLang::Zh) => "浏览…",
        (Txt::Browse, GuiLang::En) => "Browse…",
        (Txt::Detect, GuiLang::Zh) => "自动检测",
        (Txt::Detect, GuiLang::En) => "Auto-detect",
        (Txt::DirValid, GuiLang::Zh) => "有效的 FAF gamedata 目录",
        (Txt::DirValid, GuiLang::En) => "Valid FAF gamedata folder",
        (Txt::DirSuspicious, GuiLang::Zh) => {
            "注意:该目录看起来不像 FAF gamedata 目录(应为 FAForever\\gamedata 且包含 .nx2 文件)"
        }
        (Txt::DirSuspicious, GuiLang::En) => {
            "Warning: this doesn't look like a FAF gamedata folder (expected FAForever\\gamedata containing .nx2 files)"
        }
        (Txt::DirMissing, GuiLang::Zh) => "错误:目录不存在",
        (Txt::DirMissing, GuiLang::En) => "Error: folder does not exist",
        (Txt::SyncNow, GuiLang::Zh) => "开始同步",
        (Txt::SyncNow, GuiLang::En) => "Sync now",
        (Txt::Syncing, GuiLang::Zh) => "正在同步…",
        (Txt::Syncing, GuiLang::En) => "Syncing…",
        (Txt::IdleHint, GuiLang::Zh) => "确认镜像地址和目录后,点击“开始同步”。",
        (Txt::IdleHint, GuiLang::En) => "Check the mirror address and folder, then click \"Sync now\".",
    }
}

/// Interpolated log lines.
fn log_manifest(lang: GuiLang, patch: &str, files: usize, mb: f64, uploader: &str) -> String {
    match lang {
        GuiLang::Zh => {
            format!("镜像补丁版本 {patch}({files} 个文件,{mb:.1} MB),由 {uploader} 上传")
        }
        GuiLang::En => {
            format!("Mirror patch {patch} ({files} files, {mb:.1} MB), uploaded by {uploader}")
        }
    }
}

fn log_plan(lang: GuiLang, downloads: usize, mb: f64) -> String {
    match (lang, downloads) {
        (_, 0) => match lang {
            GuiLang::Zh => "已是最新,无需下载。".to_string(),
            GuiLang::En => "Everything is up to date — nothing to download.".to_string(),
        },
        (GuiLang::Zh, _) => format!("需要下载 {downloads} 个文件,共 {mb:.1} MB"),
        (GuiLang::En, _) => format!("Downloading {downloads} file(s), {mb:.1} MB total"),
    }
}

fn log_file(lang: GuiLang, index: usize, count: usize, path: &str) -> String {
    match lang {
        GuiLang::Zh => format!("[{index}/{count}] 已安装 {path}"),
        GuiLang::En => format!("[{index}/{count}] installed {path}"),
    }
}

fn log_done(lang: GuiLang, files: usize) -> String {
    match lang {
        GuiLang::Zh => {
            if files == 0 {
                "完成:无需更改。".to_string()
            } else {
                format!("同步完成,共更新 {files} 个文件。可以启动 FAF 客户端了!")
            }
        }
        GuiLang::En => {
            if files == 0 {
                "Done: no changes needed.".to_string()
            } else {
                format!("Sync complete — {files} file(s) updated. You can start FAF now!")
            }
        }
    }
}

fn log_failed(lang: GuiLang, err: &str) -> String {
    match lang {
        GuiLang::Zh => format!("同步失败:{err}"),
        GuiLang::En => format!("Sync failed: {err}"),
    }
}

/// Messages from the background sync worker to the UI.
enum WorkerMsg {
    Progress(SyncProgress),
    Finished(Result<SyncSummary, String>),
}

/// What the sync worker is doing right now.
enum SyncState {
    Idle,
    Running,
    Succeeded,
    Failed,
}

struct SyncApp {
    lang: GuiLang,
    server: String,
    dir: String,
    state: SyncState,
    worker: Option<Receiver<WorkerMsg>>,
    /// (installed, total) files in the current run, for the progress bar.
    progress: (usize, usize),
    log: Vec<String>,
}

impl SyncApp {
    fn new() -> Self {
        let cfg = ClientConfig::load().with_embedded_defaults();
        let dir = cfg
            .gamedata_dir
            .clone()
            .or_else(sync::autodetect_gamedata_dir)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            lang: GuiLang::from_config(cfg.lang.as_deref()),
            server: cfg.server.unwrap_or_default(),
            dir,
            state: SyncState::Idle,
            worker: None,
            progress: (0, 0),
            log: Vec::new(),
        }
    }

    fn start_sync(&mut self) {
        let server = self.server.trim().trim_end_matches('/').to_string();
        let dir = PathBuf::from(self.dir.trim());
        let lang = self.lang;
        let (tx, rx) = channel();
        self.worker = Some(rx);
        self.progress = (0, 0);
        self.log.clear();
        self.state = SyncState::Running;

        thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(sync::sync_gamedata(&server, &dir, &mut |event| {
                    let _ = tx.send(WorkerMsg::Progress(event));
                }))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(WorkerMsg::Finished(result));
            // Persist working settings for next launch.
            let mut cfg = ClientConfig::load();
            cfg.server = Some(server);
            cfg.gamedata_dir = Some(dir);
            cfg.lang = Some(lang.code().to_string());
            let _ = cfg.save();
        });
    }

    fn drain_worker(&mut self, ctx: &egui::Context) {
        let mut finished = false;
        if let Some(rx) = &self.worker {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMsg::Progress(SyncProgress::ManifestLoaded {
                        patch_version,
                        uploader,
                        file_count,
                        total_bytes,
                    }) => {
                        self.log.push(log_manifest(
                            self.lang,
                            &patch_version,
                            file_count,
                            total_bytes as f64 / 1e6,
                            &uploader,
                        ));
                    }
                    WorkerMsg::Progress(SyncProgress::PlanReady {
                        downloads,
                        total_bytes,
                    }) => {
                        self.progress = (0, downloads);
                        self.log
                            .push(log_plan(self.lang, downloads, total_bytes as f64 / 1e6));
                    }
                    WorkerMsg::Progress(SyncProgress::FileInstalled { path, index, count }) => {
                        self.progress = (index, count);
                        self.log.push(log_file(self.lang, index, count, &path));
                    }
                    WorkerMsg::Finished(Ok(summary)) => {
                        self.log.push(log_done(self.lang, summary.downloaded_files));
                        self.state = SyncState::Succeeded;
                        finished = true;
                    }
                    WorkerMsg::Finished(Err(err)) => {
                        self.log.push(log_failed(self.lang, &err));
                        self.state = SyncState::Failed;
                        finished = true;
                    }
                }
            }
        }
        if self.worker.is_some() {
            // Keep repainting while the worker is alive so progress shows up.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if finished {
            self.worker = None;
        }
    }

    fn folder_status(&self) -> Option<(egui::Color32, &'static str)> {
        let dir = self.dir.trim();
        if dir.is_empty() {
            return None;
        }
        let path = PathBuf::from(dir);
        if sync::is_valid_gamedata_dir(&path) {
            Some((egui::Color32::LIGHT_GREEN, tr(self.lang, Txt::DirValid)))
        } else if path.is_dir() {
            Some((egui::Color32::YELLOW, tr(self.lang, Txt::DirSuspicious)))
        } else {
            Some((egui::Color32::LIGHT_RED, tr(self.lang, Txt::DirMissing)))
        }
    }
}

impl eframe::App for SyncApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_worker(ctx);
        let running = matches!(self.state, SyncState::Running);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("fafcn-sync");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = match self.lang {
                        GuiLang::Zh => "EN",
                        GuiLang::En => "中文",
                    };
                    if ui.button(label).clicked() {
                        self.lang = match self.lang {
                            GuiLang::Zh => GuiLang::En,
                            GuiLang::En => GuiLang::Zh,
                        };
                    }
                });
            });
            ui.add_space(8.0);

            ui.label(tr(self.lang, Txt::ServerLabel));
            ui.add_enabled_ui(!running, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.server)
                        .hint_text("https://your-mirror-address")
                        .desired_width(f32::INFINITY),
                );
            });
            ui.add_space(4.0);

            ui.label(tr(self.lang, Txt::DirLabel));
            ui.add_enabled_ui(!running, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.dir)
                        .hint_text(r"C:\ProgramData\FAForever\gamedata")
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    if ui.button(tr(self.lang, Txt::Browse)).clicked() {
                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                            self.dir = path.to_string_lossy().into_owned();
                        }
                    }
                    if ui.button(tr(self.lang, Txt::Detect)).clicked() {
                        if let Some(path) = sync::autodetect_gamedata_dir() {
                            self.dir = path.to_string_lossy().into_owned();
                        }
                    }
                });
            });
            if let Some((color, text)) = self.folder_status() {
                ui.colored_label(color, text);
            }
            ui.add_space(12.0);

            let can_sync = !running
                && !self.server.trim().is_empty()
                && PathBuf::from(self.dir.trim()).is_dir();
            let button_text = if running {
                tr(self.lang, Txt::Syncing)
            } else {
                tr(self.lang, Txt::SyncNow)
            };
            if ui
                .add_enabled(
                    can_sync,
                    egui::Button::new(button_text).min_size(egui::vec2(f32::INFINITY, 40.0)),
                )
                .clicked()
            {
                self.start_sync();
            }
            if matches!(self.state, SyncState::Idle) {
                ui.label(tr(self.lang, Txt::IdleHint));
            }

            let (done, total) = self.progress;
            if total > 0 {
                ui.add(
                    egui::ProgressBar::new(done as f32 / total as f32)
                        .text(format!("{done} / {total}")),
                );
            }
            ui.add_space(8.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.log {
                        ui.label(line);
                    }
                });
        });
    }
}
