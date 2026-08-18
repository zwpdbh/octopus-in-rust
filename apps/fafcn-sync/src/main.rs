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
mod sync;
mod upload;

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
    #[arg(long)]
    pub patch_version: String,

    /// Your display name, shown on the status page.
    #[arg(long)]
    pub uploader: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None | Some(Command::Gui) => gui::run(),
        Some(Command::Sync(args)) => run_cli(sync::run(args)),
        Some(Command::Upload(args)) => run_cli(upload::run(args)),
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
