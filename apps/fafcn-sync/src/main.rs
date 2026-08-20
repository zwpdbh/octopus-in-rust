//! fafcn-sync — sync FAF `gamedata` patch files from a `fafcn-server` mirror.
//!
//! - Double-click (no arguments) opens the GUI for non-technical players.
//! - `fafcn-sync sync` — terminal version of the same sync.
//! - `fafcn-sync upload` — publish a new patch set to the mirror
//!   (VPN-having uploaders only; requires the group token).

// Double-clicking the release exe on Windows opens only the GUI, without a
// console window. CLI subcommands re-attach to the parent console instead
// (see `attach_parent_console`).
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod api;
mod config;
mod gui;
mod progress;
mod sync;
mod upload;
mod version;

/// Build tag stamped by `xtask fafcn file-sync` (shown in the GUI title and
/// on the /sync page so users can tell builds apart).
pub const BUILD_TAG: &str = match option_env!("FAFCN_SYNC_BUILD_TAG") {
    Some(tag) => tag,
    None => "dev",
};

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};

/// Sync FAF gamedata patch files from a fafcn mirror.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open the graphical interface (the default when run without arguments).
    Gui,
    /// Download missing/changed gamedata files from the mirror (terminal).
    Sync(SyncArgs),
    /// Upload a new gamedata patch set to the mirror (VPN/downloaders only).
    Upload(UploadArgs),
    /// Upload a FAF client installer to the mirror (VPN/downloaders only).
    UploadClient(UploadClientArgs),
}

/// Arguments for `fafcn-sync sync`.
#[derive(Debug, Args)]
pub struct SyncArgs {
    /// Mirror base URL, e.g. https://fafcn.example.com. Remembered after first use.
    #[arg(long)]
    pub server: Option<String>,

    /// Path to the FAF `gamedata` directory. Auto-detected / remembered after first use.
    #[arg(long)]
    pub dir: Option<PathBuf>,
}

/// Arguments for `fafcn-sync upload`.
#[derive(Debug, Args)]
pub struct UploadArgs {
    /// Mirror base URL, e.g. https://fafcn.example.com. Remembered after first use.
    #[arg(long)]
    pub server: Option<String>,

    /// Group upload token (ask the person who deployed the server).
    #[arg(long)]
    pub token: String,

    /// Directory containing the complete, up-to-date gamedata files.
    #[arg(long)]
    pub dir: PathBuf,

    /// FAF patch version these files correspond to (e.g. "3825").
    /// Auto-detected from lua.nx2 when omitted.
    #[arg(long)]
    pub patch_version: Option<String>,

    /// Your display name, shown on the status page.
    #[arg(long)]
    pub uploader: Option<String>,
}

/// Arguments for `fafcn-sync upload-client`.
#[derive(Debug, Args)]
pub struct UploadClientArgs {
    /// Mirror base URL, e.g. https://fafcn.example.com. Remembered after first use.
    #[arg(long)]
    pub server: Option<String>,

    /// Group upload token (ask the person who deployed the server).
    #[arg(long)]
    pub token: String,

    /// Path to the downloaded installer (e.g. dfc_windows_1_6_3.exe).
    #[arg(long)]
    pub file: PathBuf,

    /// Client version (auto-detected from the file name when omitted).
    #[arg(long)]
    pub version: Option<String>,

    /// Your display name, shown on the status page.
    #[arg(long)]
    pub uploader: Option<String>,
}

fn main() -> Result<()> {
    install_crash_log();
    let cli = Cli::parse();
    match cli.command {
        None | Some(Command::Gui) => run_gui(),
        Some(Command::Sync(args)) => run_cli(sync::run(args)),
        Some(Command::Upload(args)) => run_cli(upload::run(args)),
        Some(Command::UploadClient(args)) => run_cli(upload::run_client(args)),
    }
}

/// GUI release builds have no console: a panic or a windowing error just
/// makes the window vanish. Record panics and how the GUI exited to a crash
/// log next to the config file so such reports can be diagnosed.
fn install_crash_log() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        append_crash_log(&format!("PANIC: {info}"));
        default_hook(info);
    }));
}

/// Run the GUI, recording whether it exited normally, with an error, or
/// (via the panic hook) crashed.
fn run_gui() -> Result<()> {
    match gui::run() {
        Ok(()) => {
            append_crash_log("GUI exited normally");
            Ok(())
        }
        Err(e) => {
            append_crash_log(&format!("GUI exited with error: {e:#}"));
            Err(e)
        }
    }
}

/// Append one timestamped line to the crash log (best-effort, never fails).
fn append_crash_log(line: &str) {
    use std::io::Write;
    let path = config::crash_log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "[{secs}] {line}");
    }
}

/// Run a CLI subcommand on a tokio runtime.
fn run_cli(future: impl std::future::Future<Output = Result<()>>) -> Result<()> {
    attach_parent_console();
    tokio::runtime::Runtime::new()?.block_on(future)
}

/// Windows release builds are GUI-subsystem apps, so stdout from CLI
/// subcommands would be lost; attach to the parent console first.
#[cfg(all(target_os = "windows", not(debug_assertions)))]
fn attach_parent_console() {
    // ATTACH_PARENT_PROCESS = (DWORD)-1
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
    }
}

#[cfg(not(all(target_os = "windows", not(debug_assertions))))]
fn attach_parent_console() {}
