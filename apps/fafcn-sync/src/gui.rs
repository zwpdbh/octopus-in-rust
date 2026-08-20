//! eframe GUI for non-technical players.
//!
//! 同步 tab (the default): pick the gamedata folder (or let auto-detect find
//! it), click one button, done. 上传 tab: for VPN-having uploaders to publish
//! a new patch set with the group token.

use std::{
    path::PathBuf,
    sync::mpsc::{channel, Receiver},
    thread,
};

use anyhow::Result;
use eframe::egui;

use crate::{
    config::ClientConfig,
    progress::{format_bytes, format_speed},
    sync::{self, SyncProgress, SyncSummary},
    upload::{self, UploadProgress, UploadSummary},
    version,
};

/// Launch the GUI. Blocks until the window closes.
pub fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([560.0, 520.0])
            .with_min_inner_size([480.0, 440.0]),
        ..Default::default()
    };
    let title = format!("fafcn-sync · {}", crate::BUILD_TAG);
    eframe::run_native(
        &title,
        options,
        Box::new(|cc| {
            // Force dark theme: egui defaults to following the OS theme
            // (light on most Windows), which makes the status colors
            // unreadable and clashes with the fafcn-web UI.
            cc.egui_ctx.set_theme(egui::ThemePreference::Dark);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            // Log lines and status text must be copyable (bug reports!).
            cc.egui_ctx
                .all_styles_mut(|s| s.interaction.selectable_labels = true);
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

/// The app tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Sync,
    UploadPatch,
    UploadClient,
    UploadMaps,
}

/// What the background worker is doing (or did last).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionState {
    Idle,
    Running,
    Succeeded,
    Failed,
}

/// Translatable GUI strings.
#[derive(Debug, Clone, Copy)]
enum Txt {
    TabSync,
    TabUploadPatch,
    TabUploadClient,
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
    TokenLabel,
    PatchVersionLabel,
    UploaderLabel,
    UploadNow,
    Uploading,
    UploadHint,
    MissingPrefix,
    FieldServer,
    FieldDir,
    FieldToken,
    FieldPatchVersion,
    FieldUploader,
    VersionAuto,
    VersionUndetected,
    GeneratorVersion,
    GeneratorMissing,
    ChannelGamedata,
    ChannelMapGenerator,
    ChannelFafClient,
    UploadClientNow,
    VersionLabel,
    FieldClientFile,
    TabUploadMaps,
    ChannelMaps,
    MapsDirLabel,
    MapsHint,
    FieldMapsDir,
    FafClientDirLabel,
    FafClientFound,
    FafClientMissing,
    CopyLog,
}

fn tr(lang: GuiLang, txt: Txt) -> &'static str {
    match (txt, lang) {
        (Txt::TabSync, GuiLang::Zh) => "同步",
        (Txt::TabSync, GuiLang::En) => "Sync",
        (Txt::TabUploadPatch, GuiLang::Zh) => "上传补丁",
        (Txt::TabUploadPatch, GuiLang::En) => "Upload patch",
        (Txt::TabUploadClient, GuiLang::Zh) => "上传客户端",
        (Txt::TabUploadClient, GuiLang::En) => "Upload client",
        (Txt::ServerLabel, GuiLang::Zh) => "镜像地址",
        (Txt::ServerLabel, GuiLang::En) => "Mirror address",
        (Txt::DirLabel, GuiLang::Zh) => "FAForever 目录",
        (Txt::DirLabel, GuiLang::En) => "FAForever folder",
        (Txt::Browse, GuiLang::Zh) => "浏览…",
        (Txt::Browse, GuiLang::En) => "Browse…",
        (Txt::Detect, GuiLang::Zh) => "自动检测",
        (Txt::Detect, GuiLang::En) => "Auto-detect",
        (Txt::DirValid, GuiLang::Zh) => "有效的 FAForever 目录",
        (Txt::DirValid, GuiLang::En) => "Valid FAForever folder",
        (Txt::DirSuspicious, GuiLang::Zh) => {
            "注意:该目录看起来不像 FAForever 目录(应包含 gamedata 子目录及 .nx2 文件)"
        }
        (Txt::DirSuspicious, GuiLang::En) => {
            "Warning: this doesn't look like the FAForever folder (expected a gamedata subfolder containing .nx2 files)"
        }
        (Txt::DirMissing, GuiLang::Zh) => "错误:目录不存在",
        (Txt::DirMissing, GuiLang::En) => "Error: folder does not exist",
        (Txt::SyncNow, GuiLang::Zh) => "开始同步",
        (Txt::SyncNow, GuiLang::En) => "Sync now",
        (Txt::Syncing, GuiLang::Zh) => "正在同步…",
        (Txt::Syncing, GuiLang::En) => "Syncing…",
        (Txt::IdleHint, GuiLang::Zh) => "确认镜像地址和目录后,点击“开始同步”。",
        (Txt::IdleHint, GuiLang::En) => "Check the mirror address and folder, then click \"Sync now\".",
        (Txt::TokenLabel, GuiLang::Zh) => "上传令牌",
        (Txt::TokenLabel, GuiLang::En) => "Upload token",
        (Txt::PatchVersionLabel, GuiLang::Zh) => "补丁版本",
        (Txt::PatchVersionLabel, GuiLang::En) => "Patch version",
        (Txt::UploaderLabel, GuiLang::Zh) => "玩家",
        (Txt::UploaderLabel, GuiLang::En) => "Player",
        (Txt::UploadNow, GuiLang::Zh) => "开始上传",
        (Txt::UploadNow, GuiLang::En) => "Start upload",
        (Txt::Uploading, GuiLang::Zh) => "正在上传…",
        (Txt::Uploading, GuiLang::En) => "Uploading…",
        (Txt::UploadHint, GuiLang::Zh) => {
            "仅供有 VPN 的上传者使用:先从官方渠道下载最新补丁到 gamedata 目录,再在此处上传。令牌请向服务器部署者索取。"
        }
        (Txt::UploadHint, GuiLang::En) => {
            "For VPN-having uploaders only: download the latest patch into your gamedata folder via the official channel first, then upload it here. Ask the server admin for the token."
        }
        (Txt::MissingPrefix, GuiLang::Zh) => "还需要填写:",
        (Txt::MissingPrefix, GuiLang::En) => "Still needed: ",
        (Txt::FieldServer, GuiLang::Zh) => "镜像地址",
        (Txt::FieldServer, GuiLang::En) => "mirror address",
        (Txt::FieldDir, GuiLang::Zh) => "FAForever 目录",
        (Txt::FieldDir, GuiLang::En) => "FAForever folder",
        (Txt::FieldToken, GuiLang::Zh) => "上传令牌",
        (Txt::FieldToken, GuiLang::En) => "upload token",
        (Txt::FieldPatchVersion, GuiLang::Zh) => "补丁版本",
        (Txt::FieldPatchVersion, GuiLang::En) => "patch version",
        (Txt::FieldUploader, GuiLang::Zh) => "玩家",
        (Txt::FieldUploader, GuiLang::En) => "player",
        (Txt::VersionAuto, GuiLang::Zh) => "(从 lua.nx2 自动检测)",
        (Txt::VersionAuto, GuiLang::En) => "(auto-detected from lua.nx2)",
        (Txt::VersionUndetected, GuiLang::Zh) => "无法自动检测,请手动填写",
        (Txt::VersionUndetected, GuiLang::En) => "could not auto-detect; enter manually",
        (Txt::GeneratorVersion, GuiLang::Zh) => "地图生成器",
        (Txt::GeneratorVersion, GuiLang::En) => "map generator",
        (Txt::GeneratorMissing, GuiLang::Zh) => "未安装,上传时跳过",
        (Txt::GeneratorMissing, GuiLang::En) => "not installed; skipped on upload",
        (Txt::ChannelGamedata, GuiLang::Zh) => "游戏数据",
        (Txt::ChannelGamedata, GuiLang::En) => "gamedata",
        (Txt::ChannelMapGenerator, GuiLang::Zh) => "地图生成器",
        (Txt::ChannelMapGenerator, GuiLang::En) => "map-generator",
        (Txt::ChannelFafClient, GuiLang::Zh) => "FAF 客户端",
        (Txt::ChannelFafClient, GuiLang::En) => "FAF client",
        (Txt::UploadClientNow, GuiLang::Zh) => "上传安装包",
        (Txt::UploadClientNow, GuiLang::En) => "Upload installer",
        (Txt::VersionLabel, GuiLang::Zh) => "版本",
        (Txt::VersionLabel, GuiLang::En) => "Version",
        (Txt::FieldClientFile, GuiLang::Zh) => "安装包文件",
        (Txt::FieldClientFile, GuiLang::En) => "installer file",
        (Txt::TabUploadMaps, GuiLang::Zh) => "上传地图",
        (Txt::TabUploadMaps, GuiLang::En) => "Upload maps",
        (Txt::ChannelMaps, GuiLang::Zh) => "地图",
        (Txt::ChannelMaps, GuiLang::En) => "maps",
        (Txt::MapsDirLabel, GuiLang::Zh) => "地图文件夹",
        (Txt::MapsDirLabel, GuiLang::En) => "Maps folder",
        (Txt::MapsHint, GuiLang::Zh) => {
            "选择包含地图的文件夹:其中的所有地图(包括子文件夹内的文件)都会上传到镜像。上传是合并式的:同名地图的旧版本会被你上传的版本替换,其他人上传的地图不受影响。"
        }
        (Txt::MapsHint, GuiLang::En) => {
            "Pick a folder of maps: every map inside (including files in subfolders) is uploaded to the mirror. Uploads are merged: your versions replace older versions of the same maps; maps uploaded by others are kept."
        }
        (Txt::FieldMapsDir, GuiLang::Zh) => "地图文件夹",
        (Txt::FieldMapsDir, GuiLang::En) => "maps folder",
        (Txt::FafClientDirLabel, GuiLang::Zh) => "FAF Client 目录(含 faf-client.exe)",
        (Txt::FafClientDirLabel, GuiLang::En) => "FAF Client folder (contains faf-client.exe)",
        (Txt::FafClientFound, GuiLang::Zh) => "已找到 faf-client.exe,地图将同步到 maps_and_mods/maps",
        (Txt::FafClientFound, GuiLang::En) => {
            "faf-client.exe found — maps sync into maps_and_mods/maps"
        }
        (Txt::FafClientMissing, GuiLang::Zh) => "未找到 faf-client.exe,将跳过地图同步",
        (Txt::FafClientMissing, GuiLang::En) => "faf-client.exe not found — maps sync will be skipped",
        (Txt::CopyLog, GuiLang::Zh) => "复制日志",
        (Txt::CopyLog, GuiLang::En) => "Copy log",
    }
}

// --- Interpolated log lines ---

/// A folder picker that opens at `current` when it is a valid directory.
fn folder_picker(current: &str) -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new();
    let current = PathBuf::from(current.trim());
    if current.is_dir() {
        dialog.set_directory(current)
    } else {
        dialog
    }
}

/// Localized display name for a channel id.
fn channel_name(lang: GuiLang, channel: &str) -> &'static str {
    match channel {
        fafcn_gamedata::CHANNEL_MAP_GENERATOR => tr(lang, Txt::ChannelMapGenerator),
        fafcn_gamedata::CHANNEL_FAF_CLIENT => tr(lang, Txt::ChannelFafClient),
        fafcn_gamedata::CHANNEL_MAPS => tr(lang, Txt::ChannelMaps),
        _ => tr(lang, Txt::ChannelGamedata),
    }
}

fn log_channel_started(lang: GuiLang, channel: &str) -> String {
    format!("—— {} ——", channel_name(lang, channel))
}

fn log_channel_empty(lang: GuiLang, channel: &str) -> String {
    match lang {
        GuiLang::Zh => format!("镜像还没有{},请上传者先发布", channel_name(lang, channel)),
        GuiLang::En => format!(
            "mirror has no {} yet — ask an uploader to publish it",
            channel_name(lang, channel)
        ),
    }
}

fn log_manifest(
    lang: GuiLang,
    channel: &str,
    patch: &str,
    files: usize,
    mb: f64,
    uploader: &str,
) -> String {
    match lang {
        GuiLang::Zh => format!(
            "{}版本 {patch}({files} 个文件,{mb:.1} MB),由 {uploader} 上传",
            channel_name(lang, channel)
        ),
        GuiLang::En => format!(
            "{} {patch} ({files} files, {mb:.1} MB), uploaded by {uploader}",
            channel_name(lang, channel)
        ),
    }
}

fn log_plan(lang: GuiLang, downloads: usize, mb: f64) -> String {
    match (lang, downloads) {
        (_, 0) => match lang {
            GuiLang::Zh => "已是最新,无需下载。".to_string(),
            GuiLang::En => "up to date — nothing to download.".to_string(),
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

fn log_pruned(lang: GuiLang, path: &str) -> String {
    match lang {
        GuiLang::Zh => format!("已清理旧版本:{path}"),
        GuiLang::En => format!("pruned old version: {path}"),
    }
}

fn log_sync_done(lang: GuiLang, files: usize) -> String {
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

fn log_scanned(lang: GuiLang, files: usize, mb: f64) -> String {
    match lang {
        GuiLang::Zh => format!("本地共 {files} 个文件,{mb:.1} MB"),
        GuiLang::En => format!("Found {files} file(s), {mb:.1} MB total"),
    }
}

fn log_needed(lang: GuiLang, needed: usize) -> String {
    match (lang, needed) {
        (_, 0) => match lang {
            GuiLang::Zh => "服务器已有全部文件,无需上传。".to_string(),
            GuiLang::En => "Server already has every file — nothing to upload.".to_string(),
        },
        (GuiLang::Zh, _) => format!("服务器需要 {needed} 个文件,开始上传…"),
        (GuiLang::En, _) => format!("Server needs {needed} file(s), uploading…"),
    }
}

fn log_uploaded_file(lang: GuiLang, index: usize, count: usize, path: &str) -> String {
    match lang {
        GuiLang::Zh => format!("[{index}/{count}] 已上传 {path}"),
        GuiLang::En => format!("[{index}/{count}] uploaded {path}"),
    }
}

fn log_committed(lang: GuiLang, channel: &str, files: usize) -> String {
    match lang {
        GuiLang::Zh => format!("{}清单已提交({files} 个文件)", channel_name(lang, channel)),
        GuiLang::En => {
            format!(
                "{} manifest committed ({files} files)",
                channel_name(lang, channel)
            )
        }
    }
}

fn log_channel_skipped(lang: GuiLang, channel: &str, reason: &str) -> String {
    match lang {
        GuiLang::Zh => format!("跳过{}:{reason}", channel_name(lang, channel)),
        GuiLang::En => format!("skipping {}: {reason}", channel_name(lang, channel)),
    }
}

fn log_upload_done(lang: GuiLang, published: &[crate::upload::PublishedChannel]) -> String {
    // The faf-client installer is downloaded from the website, not synced —
    // point players there instead of at the sync tool.
    if published
        .iter()
        .all(|p| p.channel == fafcn_gamedata::CHANNEL_FAF_CLIENT)
    {
        let version = published.first().map(|p| p.version.as_str()).unwrap_or("");
        return match lang {
            GuiLang::Zh => format!(
                "上传完成,FAF 客户端 {version} 已发布!玩家们现在可以在网站的补丁同步页下载它。"
            ),
            GuiLang::En => format!(
                "Upload complete — FAF client {version} is live! Players can now download it from the sync page."
            ),
        };
    }
    let list = published
        .iter()
        .map(|p| format!("{} {}", p.channel, p.version))
        .collect::<Vec<_>>()
        .join(", ");
    match lang {
        GuiLang::Zh => format!("上传完成,已发布:{list},所有人现在都可以同步了!"),
        GuiLang::En => {
            format!("Upload complete — published: {list}. It's live for everyone to sync!")
        }
    }
}

fn log_maps_skipped(lang: GuiLang) -> String {
    match lang {
        GuiLang::Zh => "未设置有效的 FAF Client 目录,跳过地图同步。".to_string(),
        GuiLang::En => "FAF Client folder not set — skipping maps sync.".to_string(),
    }
}

fn log_failed(lang: GuiLang, err: &str) -> String {
    match lang {
        GuiLang::Zh => format!("操作失败:{err}"),
        GuiLang::En => format!("Failed: {err}"),
    }
}

fn txt_server_newer(lang: GuiLang, server: &str, local: &str) -> String {
    match lang {
        GuiLang::Zh => format!("服务器已有更新的补丁 {server}(你的是 {local}),无需上传"),
        GuiLang::En => {
            format!("Server already has newer patch {server} (yours is {local}); nothing to upload")
        }
    }
}

/// Messages from a background worker to the UI.
enum WorkerMsg {
    Sync(SyncProgress),
    Upload(UploadProgress),
    /// The sync ran without a FAF Client folder; maps were skipped.
    MapsSkipped,
    SyncDone(Result<SyncSummary, String>),
    UploadDone(Result<UploadSummary, String>),
}

struct SyncApp {
    lang: GuiLang,
    tab: Tab,
    /// Tab shown on the previous frame (for tab-switch actions).
    last_tab: Tab,
    // Shared fields (same values for both tabs).
    server: String,
    dir: String,
    // Upload-only fields.
    token: String,
    patch_version: String,
    uploader: String,
    // Installer-upload fields (faf-client channel).
    client_file: String,
    client_version: String,
    // Maps-upload source folder (UploadMaps tab).
    maps_dir: String,
    // FAF Client install folder for maps sync (Sync tab).
    faf_client_dir: String,
    // Patch version auto-detected from lua.nx2 (recomputed when dir changes).
    detected_version: Option<String>,
    detected_generator: Option<String>,
    version_dir: String,
    // Server's current patch version (fetched while on the upload tab).
    server_version: Option<String>,
    status_rx: Option<Receiver<Option<String>>>,
    last_status_check: String,
    // Per-tab action state.
    sync_state: ActionState,
    upload_state: ActionState,
    worker: Option<Receiver<WorkerMsg>>,
    /// (done_bytes, total_bytes) for the progress bar of the running action.
    progress: (u64, u64),
    /// Smoothed transfer speed (bytes/sec) of the running action.
    speed: f64,
    log: Vec<String>,
}

impl SyncApp {
    fn new() -> Self {
        let cfg = ClientConfig::load().with_embedded_defaults();
        let dir = cfg
            .gamedata_dir
            .clone()
            .map(sync::normalize_faf_dir)
            .or_else(sync::autodetect_faf_dir)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            lang: GuiLang::from_config(cfg.lang.as_deref()),
            tab: Tab::Sync,
            last_tab: Tab::Sync,
            server: cfg.server.unwrap_or_default(),
            dir,
            token: cfg.upload_token.unwrap_or_default(),
            patch_version: String::new(),
            uploader: cfg.uploader.unwrap_or_default(),
            client_file: String::new(),
            client_version: String::new(),
            maps_dir: String::new(),
            faf_client_dir: cfg
                .faf_client_dir
                .clone()
                .or_else(sync::autodetect_faf_client_dir)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            detected_version: None,
            detected_generator: None,
            version_dir: String::new(),
            server_version: None,
            status_rx: None,
            last_status_check: String::new(),
            sync_state: ActionState::Idle,
            upload_state: ActionState::Idle,
            worker: None,
            progress: (0, 0),
            speed: 0.0,
            log: Vec::new(),
        }
    }

    fn busy(&self) -> bool {
        self.worker.is_some()
    }

    /// The FAForever root from the dir field (tolerates a gamedata subpath).
    fn faf_root(&self) -> PathBuf {
        sync::normalize_faf_dir(PathBuf::from(self.dir.trim()))
    }

    /// Default the maps-upload source folder to the player's own maps folder
    /// (`<FAF Client>/maps_and_mods/maps`) when we know the FAF Client root.
    fn prefill_maps_dir(&mut self) {
        if !self.maps_dir.trim().is_empty() {
            return;
        }
        let root = PathBuf::from(self.faf_client_dir.trim());
        if sync::is_valid_faf_client_dir(&root) {
            let maps = sync::maps_dir(&root);
            if maps.is_dir() {
                self.maps_dir = maps.to_string_lossy().into_owned();
            }
        }
    }

    fn start_sync(&mut self) {
        let server = self.server.trim().trim_end_matches('/').to_string();
        let dir = self.faf_root();
        let faf_client = PathBuf::from(self.faf_client_dir.trim());
        let faf_client = sync::is_valid_faf_client_dir(&faf_client).then_some(faf_client);
        let (tx, rx) = channel();
        self.worker = Some(rx);
        self.progress = (0, 0);
        self.speed = 0.0;
        self.log.clear();
        self.sync_state = ActionState::Running;
        let cfg = self.persisted_config();

        thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(async {
                    let mut forward = |event| {
                        let _ = tx.send(WorkerMsg::Sync(event));
                    };
                    let summary = sync::sync_gamedata(&server, &dir, &mut forward).await?;
                    // Maps live below the FAF Client folder, not FAForever.
                    match &faf_client {
                        Some(root) => {
                            sync::sync_maps(&server, root, &mut forward).await?;
                        }
                        None => {
                            let _ = tx.send(WorkerMsg::MapsSkipped);
                        }
                    }
                    Ok(summary)
                })
                .map_err(|e: anyhow::Error| format!("{e:#}"));
            let _ = tx.send(WorkerMsg::SyncDone(result));
            let _ = cfg.save();
        });
    }

    fn start_upload_maps(&mut self) {
        let server = self.server.trim().trim_end_matches('/').to_string();
        let token = self.token.trim().to_string();
        let folder = PathBuf::from(self.maps_dir.trim());
        let uploader = self.uploader.trim().to_string();
        let (tx, rx) = channel();
        self.worker = Some(rx);
        self.progress = (0, 0);
        self.speed = 0.0;
        self.log.clear();
        self.upload_state = ActionState::Running;
        let cfg = self.persisted_config();

        thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(upload::upload_maps(
                    &server,
                    &token,
                    &folder,
                    &uploader,
                    &mut |event| {
                        let _ = tx.send(WorkerMsg::Upload(event));
                    },
                ))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(WorkerMsg::UploadDone(result));
            let _ = cfg.save();
        });
    }

    fn start_upload(&mut self) {
        let server = self.server.trim().trim_end_matches('/').to_string();
        let dir = self.faf_root();
        let token = self.token.trim().to_string();
        let patch_version = self.patch_version.trim().to_string();
        let uploader = self.uploader.trim().to_string();
        let (tx, rx) = channel();
        self.worker = Some(rx);
        self.progress = (0, 0);
        self.speed = 0.0;
        self.log.clear();
        self.upload_state = ActionState::Running;
        let cfg = self.persisted_config();

        thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(upload::upload_gamedata(
                    &server,
                    &token,
                    &dir,
                    if patch_version.is_empty() {
                        None
                    } else {
                        Some(patch_version.as_str())
                    },
                    &uploader,
                    &mut |event| {
                        let _ = tx.send(WorkerMsg::Upload(event));
                    },
                ))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(WorkerMsg::UploadDone(result));
            let _ = cfg.save();
        });
    }

    fn start_upload_client(&mut self) {
        let server = self.server.trim().trim_end_matches('/').to_string();
        let token = self.token.trim().to_string();
        let file = PathBuf::from(self.client_file.trim());
        let version = self.client_version.trim().to_string();
        let uploader = self.uploader.trim().to_string();
        let (tx, rx) = channel();
        self.worker = Some(rx);
        self.progress = (0, 0);
        self.speed = 0.0;
        self.log.clear();
        self.upload_state = ActionState::Running;
        let cfg = self.persisted_config();

        thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(upload::upload_faf_client(
                    &server,
                    &token,
                    &file,
                    &version,
                    &uploader,
                    &mut |event| {
                        let _ = tx.send(WorkerMsg::Upload(event));
                    },
                ))
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(WorkerMsg::UploadDone(result));
            let _ = cfg.save();
        });
    }

    /// Current field values as they should be remembered on disk.
    fn persisted_config(&self) -> ClientConfig {
        let mut cfg = ClientConfig::load();
        cfg.server = Some(self.server.trim().trim_end_matches('/').to_string());
        cfg.gamedata_dir = Some(self.faf_root());
        cfg.lang = Some(self.lang.code().to_string());
        if !self.token.trim().is_empty() {
            cfg.upload_token = Some(self.token.trim().to_string());
        }
        if !self.uploader.trim().is_empty() {
            cfg.uploader = Some(self.uploader.trim().to_string());
        }
        let faf_client = PathBuf::from(self.faf_client_dir.trim());
        if sync::is_valid_faf_client_dir(&faf_client) {
            cfg.faf_client_dir = Some(faf_client);
        }
        cfg
    }

    fn drain_worker(&mut self, ctx: &egui::Context) {
        let mut finished = false;
        if let Some(rx) = &self.worker {
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMsg::Sync(SyncProgress::ChannelStarted { channel }) => {
                        self.log.push(log_channel_started(self.lang, &channel));
                    }
                    WorkerMsg::Sync(SyncProgress::ChannelEmpty { channel }) => {
                        self.log.push(log_channel_empty(self.lang, &channel));
                    }
                    WorkerMsg::Sync(SyncProgress::ManifestLoaded {
                        channel,
                        patch_version,
                        uploader,
                        file_count,
                        total_bytes,
                    }) => {
                        self.log.push(log_manifest(
                            self.lang,
                            &channel,
                            &patch_version,
                            file_count,
                            total_bytes as f64 / 1e6,
                            &uploader,
                        ));
                    }
                    WorkerMsg::Sync(SyncProgress::PlanReady {
                        downloads,
                        total_bytes,
                        ..
                    }) => {
                        self.progress = (0, total_bytes);
                        self.speed = 0.0;
                        self.log
                            .push(log_plan(self.lang, downloads, total_bytes as f64 / 1e6));
                    }
                    WorkerMsg::Sync(SyncProgress::Bytes(update)) => {
                        self.progress = (update.done_bytes, update.total_bytes);
                        self.speed = update.bytes_per_sec;
                    }
                    WorkerMsg::Sync(SyncProgress::FileInstalled {
                        path, index, count, ..
                    }) => {
                        self.log.push(log_file(self.lang, index, count, &path));
                    }
                    WorkerMsg::Sync(SyncProgress::Pruned { path, .. }) => {
                        self.log.push(log_pruned(self.lang, &path));
                    }
                    WorkerMsg::MapsSkipped => {
                        self.log.push(log_maps_skipped(self.lang));
                    }
                    WorkerMsg::SyncDone(Ok(summary)) => {
                        self.log
                            .push(log_sync_done(self.lang, summary.downloaded_files));
                        self.sync_state = ActionState::Succeeded;
                        finished = true;
                    }
                    WorkerMsg::SyncDone(Err(err)) => {
                        self.log.push(log_failed(self.lang, &err));
                        self.sync_state = ActionState::Failed;
                        finished = true;
                    }
                    WorkerMsg::Upload(UploadProgress::ChannelStarted { channel }) => {
                        self.log.push(log_channel_started(self.lang, &channel));
                    }
                    WorkerMsg::Upload(UploadProgress::ChannelSkipped { channel, reason }) => {
                        self.log
                            .push(log_channel_skipped(self.lang, &channel, &reason));
                    }
                    WorkerMsg::Upload(UploadProgress::Scanned {
                        files, total_bytes, ..
                    }) => {
                        self.log
                            .push(log_scanned(self.lang, files, total_bytes as f64 / 1e6));
                    }
                    WorkerMsg::Upload(UploadProgress::Needed {
                        needed,
                        total_bytes,
                        ..
                    }) => {
                        // No bar when there is nothing to upload.
                        self.progress = (0, if needed == 0 { 0 } else { total_bytes });
                        self.speed = 0.0;
                        self.log.push(log_needed(self.lang, needed));
                    }
                    WorkerMsg::Upload(UploadProgress::Bytes(update)) => {
                        self.progress = (update.done_bytes, update.total_bytes);
                        self.speed = update.bytes_per_sec;
                    }
                    WorkerMsg::Upload(UploadProgress::FileUploaded {
                        path, index, count, ..
                    }) => {
                        self.log
                            .push(log_uploaded_file(self.lang, index, count, &path));
                    }
                    WorkerMsg::Upload(UploadProgress::Committed { channel, files, .. }) => {
                        self.log.push(log_committed(self.lang, &channel, files));
                    }
                    WorkerMsg::UploadDone(Ok(summary)) => {
                        self.log
                            .push(log_upload_done(self.lang, &summary.published));
                        self.upload_state = ActionState::Succeeded;
                        finished = true;
                    }
                    WorkerMsg::UploadDone(Err(err)) => {
                        self.log.push(log_failed(self.lang, &err));
                        self.upload_state = ActionState::Failed;
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
        let path = self.faf_root();
        if sync::is_valid_faf_dir(&path) {
            Some((egui::Color32::LIGHT_GREEN, tr(self.lang, Txt::DirValid)))
        } else if path.is_dir() {
            Some((egui::Color32::YELLOW, tr(self.lang, Txt::DirSuspicious)))
        } else {
            Some((egui::Color32::LIGHT_RED, tr(self.lang, Txt::DirMissing)))
        }
    }

    fn shared_fields(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        ui.label(tr(self.lang, Txt::ServerLabel));
        ui.add_enabled_ui(!busy, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.server)
                    .hint_text("https://your-mirror-address")
                    .desired_width(f32::INFINITY),
            );
        });
        ui.add_space(4.0);

        // The FAForever folder is only needed for sync and patch upload.
        if matches!(self.tab, Tab::Sync | Tab::UploadPatch) {
            ui.label(tr(self.lang, Txt::DirLabel));
            ui.add_enabled_ui(!busy, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.dir)
                        .hint_text(r"C:\ProgramData\FAForever")
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    if ui.button(tr(self.lang, Txt::Browse)).clicked() {
                        if let Some(path) = folder_picker(&self.dir).pick_folder() {
                            self.dir = path.to_string_lossy().into_owned();
                        }
                    }
                    if ui.button(tr(self.lang, Txt::Detect)).clicked() {
                        if let Some(path) = sync::autodetect_faf_dir() {
                            self.dir = path.to_string_lossy().into_owned();
                        }
                    }
                });
            });
            if let Some((color, text)) = self.folder_status() {
                ui.colored_label(color, text);
            }
        }

        // Token + player name are needed by both upload tabs.
        if self.tab != Tab::Sync {
            ui.label(tr(self.lang, Txt::TokenLabel));
            ui.add_enabled_ui(!busy, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.token)
                        .password(true)
                        .desired_width(f32::INFINITY),
                );
            });
            ui.horizontal(|ui| {
                ui.label(tr(self.lang, Txt::UploaderLabel));
                ui.add_enabled_ui(!busy, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.uploader).desired_width(140.0));
                });
            });
        }
    }

    /// Patch version detected from `lua.nx2` (cached, refreshed on dir change).
    fn detected_patch_version(&mut self) -> Option<String> {
        if self.version_dir != self.dir {
            self.version_dir = self.dir.clone();
            let root = PathBuf::from(self.dir.trim());
            self.detected_version = version::detect_patch_version(&root.join("gamedata"));
            self.detected_generator = version::detect_generator_version(&root);
        }
        self.detected_version.clone()
    }

    /// The version that would be uploaded: detected wins, manual is fallback.
    fn effective_patch_version(&mut self) -> Option<String> {
        self.detected_patch_version().or_else(|| {
            let manual = self.patch_version.trim();
            if manual.is_empty() {
                None
            } else {
                Some(manual.to_string())
            }
        })
    }

    /// Kick off a one-shot fetch of the server's patch version when the
    /// patch upload tab is showing and the server address changed.
    fn maybe_refresh_server_version(&mut self) {
        if self.tab != Tab::UploadPatch {
            return;
        }
        let server = self.server.trim().trim_end_matches('/').to_string();
        if server.is_empty() || self.last_status_check == server {
            return;
        }
        self.last_status_check = server.clone();
        let (tx, rx) = channel();
        self.status_rx = Some(rx);
        thread::spawn(move || {
            let version = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio runtime")
                .block_on(async move {
                    let url = crate::api::api_url(&server, "channels/gamedata/manifest.json");
                    let resp = reqwest::get(url).await.ok()?;
                    if !resp.status().is_success() {
                        return None;
                    }
                    resp.json::<fafcn_gamedata::Manifest>()
                        .await
                        .ok()?
                        .patch_version
                        .into()
                });
            let _ = tx.send(version);
        });
    }

    /// Some(true) when the server already has a strictly newer patch.
    fn server_is_newer(&mut self) -> bool {
        let local = self.effective_patch_version();
        match (self.server_version.as_deref(), local.as_deref()) {
            (Some(server), Some(local)) => {
                fafcn_gamedata::compare_version_strings(server, local)
                    == Some(std::cmp::Ordering::Greater)
            }
            _ => false,
        }
    }

    /// Fields still missing for the installer upload, as localized names.
    fn client_missing_fields(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if self.server.trim().is_empty() {
            missing.push(tr(self.lang, Txt::FieldServer));
        }
        if self.token.trim().is_empty() {
            missing.push(tr(self.lang, Txt::FieldToken));
        }
        if !PathBuf::from(self.client_file.trim()).is_file() {
            missing.push(tr(self.lang, Txt::FieldClientFile));
        }
        if self.client_version.trim().is_empty() {
            missing.push(tr(self.lang, Txt::VersionLabel));
        }
        if self.uploader.trim().is_empty() {
            missing.push(tr(self.lang, Txt::FieldUploader));
        }
        missing
    }

    /// Fields still missing for the current tab's action, as localized names.
    fn missing_fields(&mut self) -> Vec<&'static str> {
        if self.tab == Tab::UploadClient {
            return self.client_missing_fields();
        }
        let mut missing = Vec::new();
        if self.server.trim().is_empty() {
            missing.push(tr(self.lang, Txt::FieldServer));
        }
        // The FAForever folder is only needed for sync and patch upload.
        if matches!(self.tab, Tab::Sync | Tab::UploadPatch) && !self.faf_root().is_dir() {
            missing.push(tr(self.lang, Txt::FieldDir));
        }
        if self.tab == Tab::UploadPatch {
            if self.token.trim().is_empty() {
                missing.push(tr(self.lang, Txt::FieldToken));
            }
            if self.effective_patch_version().is_none() {
                missing.push(tr(self.lang, Txt::FieldPatchVersion));
            }
            if self.uploader.trim().is_empty() {
                missing.push(tr(self.lang, Txt::FieldUploader));
            }
        }
        if self.tab == Tab::UploadMaps {
            if self.token.trim().is_empty() {
                missing.push(tr(self.lang, Txt::FieldToken));
            }
            if !PathBuf::from(self.maps_dir.trim()).is_dir() {
                missing.push(tr(self.lang, Txt::FieldMapsDir));
            }
            if self.uploader.trim().is_empty() {
                missing.push(tr(self.lang, Txt::FieldUploader));
            }
        }
        missing
    }

    fn action_button(&mut self, ui: &mut egui::Ui) {
        let busy = self.busy();
        let missing = self.missing_fields();
        let server_newer = self.tab == Tab::UploadPatch && self.server_is_newer();
        let can_run = !busy && missing.is_empty() && !server_newer;
        let running = match self.tab {
            Tab::Sync => self.sync_state == ActionState::Running,
            Tab::UploadPatch | Tab::UploadClient | Tab::UploadMaps => {
                self.upload_state == ActionState::Running
            }
        };
        let label = match (self.tab, running) {
            (Tab::Sync, true) => tr(self.lang, Txt::Syncing),
            (Tab::Sync, false) => tr(self.lang, Txt::SyncNow),
            (Tab::UploadPatch, true) => tr(self.lang, Txt::Uploading),
            (Tab::UploadPatch, false) => tr(self.lang, Txt::UploadNow),
            (Tab::UploadClient, true) => tr(self.lang, Txt::Uploading),
            (Tab::UploadClient, false) => tr(self.lang, Txt::UploadClientNow),
            (Tab::UploadMaps, true) => tr(self.lang, Txt::Uploading),
            (Tab::UploadMaps, false) => tr(self.lang, Txt::UploadNow),
        };
        if ui
            .add_enabled(
                can_run,
                egui::Button::new(label).min_size(egui::vec2(f32::INFINITY, 40.0)),
            )
            .clicked()
        {
            match self.tab {
                Tab::Sync => self.start_sync(),
                Tab::UploadPatch => self.start_upload(),
                Tab::UploadClient => self.start_upload_client(),
                Tab::UploadMaps => self.start_upload_maps(),
            }
        }
        // Explain exactly why the button is disabled.
        if !busy && server_newer {
            let server_v = self.server_version.clone().unwrap_or_default();
            let local_v = self.effective_patch_version().unwrap_or_default();
            ui.colored_label(
                egui::Color32::LIGHT_GREEN,
                txt_server_newer(self.lang, &server_v, &local_v),
            );
        } else if !busy && !missing.is_empty() {
            ui.colored_label(
                egui::Color32::YELLOW,
                format!(
                    "{}{}",
                    tr(self.lang, Txt::MissingPrefix),
                    missing.join(", ")
                ),
            );
        } else if self.tab == Tab::Sync && self.sync_state == ActionState::Idle {
            ui.label(tr(self.lang, Txt::IdleHint));
        }
    }
}

impl eframe::App for SyncApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_worker(ui.ctx());
        // Tab-switch actions: entering the maps upload tab prefills the
        // source folder from the located FAF Client install.
        if self.tab != self.last_tab {
            self.last_tab = self.tab;
            if self.tab == Tab::UploadMaps {
                self.prefill_maps_dir();
            }
        }
        if let Some(rx) = &self.status_rx {
            if let Ok(version) = rx.try_recv() {
                self.server_version = version;
                self.status_rx = None;
            }
        }
        self.maybe_refresh_server_version();
        let busy = self.busy();

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("fafcn-sync");
                ui.label(egui::RichText::new(crate::BUILD_TAG).small().weak());
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

            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Sync, tr(self.lang, Txt::TabSync));
                ui.selectable_value(
                    &mut self.tab,
                    Tab::UploadPatch,
                    tr(self.lang, Txt::TabUploadPatch),
                );
                ui.selectable_value(
                    &mut self.tab,
                    Tab::UploadClient,
                    tr(self.lang, Txt::TabUploadClient),
                );
                ui.selectable_value(
                    &mut self.tab,
                    Tab::UploadMaps,
                    tr(self.lang, Txt::TabUploadMaps),
                );
            });
            ui.separator();

            self.shared_fields(ui);

            if self.tab == Tab::UploadPatch {
                ui.add_space(4.0);
                let detected = self.detected_patch_version();
                ui.horizontal(|ui| {
                    ui.label(tr(self.lang, Txt::PatchVersionLabel));
                    match &detected {
                        Some(v) => {
                            ui.strong(v);
                            ui.label(
                                egui::RichText::new(tr(self.lang, Txt::VersionAuto))
                                    .small()
                                    .weak(),
                            );
                        }
                        None => {
                            ui.add_enabled_ui(!busy, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.patch_version)
                                        .hint_text("3825")
                                        .desired_width(120.0),
                                );
                            });
                            ui.label(
                                egui::RichText::new(tr(self.lang, Txt::VersionUndetected))
                                    .small()
                                    .weak(),
                            );
                        }
                    }
                });
                // Map generator version (auto-detected; informational only).
                ui.horizontal(|ui| {
                    ui.label(tr(self.lang, Txt::GeneratorVersion));
                    match &self.detected_generator {
                        Some(v) => {
                            ui.strong(v);
                        }
                        None => {
                            ui.label(
                                egui::RichText::new(tr(self.lang, Txt::GeneratorMissing))
                                    .small()
                                    .weak(),
                            );
                        }
                    }
                });
                ui.label(
                    egui::RichText::new(tr(self.lang, Txt::UploadHint))
                        .small()
                        .weak(),
                );
            }

            if self.tab == Tab::UploadClient {
                ui.add_space(4.0);
                ui.label(tr(self.lang, Txt::FieldClientFile));
                ui.add_enabled_ui(!busy, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.client_file)
                            .hint_text(r"D:\Downloads\dfc_windows_1_6_3.exe")
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(tr(self.lang, Txt::Browse)).clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                if let Some(v) = fafcn_gamedata::detect_version_from_filename(&name)
                                {
                                    self.client_version = v;
                                }
                                self.client_file = path.to_string_lossy().into_owned();
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label(tr(self.lang, Txt::VersionLabel));
                    ui.add_enabled_ui(!busy, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.client_version)
                                .hint_text("1.6.3")
                                .desired_width(120.0),
                        );
                    });
                });
            }
            if self.tab == Tab::Sync {
                ui.add_space(4.0);
                ui.label(tr(self.lang, Txt::FafClientDirLabel));
                ui.add_enabled_ui(!busy, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.faf_client_dir)
                            .hint_text(r"E:\FAF Client")
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(tr(self.lang, Txt::Browse)).clicked() {
                            if let Some(path) = folder_picker(&self.faf_client_dir).pick_folder() {
                                self.faf_client_dir = path.to_string_lossy().into_owned();
                            }
                        }
                        if ui.button(tr(self.lang, Txt::Detect)).clicked() {
                            if let Some(path) = sync::autodetect_faf_client_dir() {
                                self.faf_client_dir = path.to_string_lossy().into_owned();
                            }
                        }
                    });
                });
                if sync::is_valid_faf_client_dir(&PathBuf::from(self.faf_client_dir.trim())) {
                    ui.colored_label(
                        egui::Color32::LIGHT_GREEN,
                        tr(self.lang, Txt::FafClientFound),
                    );
                } else {
                    ui.colored_label(egui::Color32::YELLOW, tr(self.lang, Txt::FafClientMissing));
                }
            }

            if self.tab == Tab::UploadMaps {
                ui.add_space(4.0);
                ui.label(tr(self.lang, Txt::MapsDirLabel));
                ui.add_enabled_ui(!busy, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.maps_dir)
                            .hint_text(r"D:\MyMaps")
                            .desired_width(f32::INFINITY),
                    );
                    ui.horizontal(|ui| {
                        if ui.button(tr(self.lang, Txt::Browse)).clicked() {
                            if let Some(path) = folder_picker(&self.maps_dir).pick_folder() {
                                self.maps_dir = path.to_string_lossy().into_owned();
                            }
                        }
                    });
                });
                ui.label(
                    egui::RichText::new(tr(self.lang, Txt::MapsHint))
                        .small()
                        .weak(),
                );
            }
            ui.add_space(12.0);

            self.action_button(ui);

            let (done, total) = self.progress;
            if total > 0 {
                let fraction = done as f32 / total as f32;
                ui.add(egui::ProgressBar::new(fraction).text(format!(
                    "{:.0}%  ·  {} / {}  ·  {}",
                    fraction * 100.0,
                    format_bytes(done),
                    format_bytes(total),
                    format_speed(self.speed),
                )));
            }
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui
                    .button(egui::RichText::new(tr(self.lang, Txt::CopyLog)).small())
                    .on_hover_text("fafcn-sync-crash.log")
                    .clicked()
                {
                    ui.ctx().copy_text(self.log.join("\n"));
                }
            });
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
