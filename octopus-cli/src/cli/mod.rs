use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

pub mod export;
pub mod info;

/// The mutually exclusive UI modes the CLI can run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum UiMode {
    /// Interactive shell (default).
    #[default]
    Shell,
    /// Non-interactive print mode.
    Print,
    /// ACP server mode.
    Acp,
    /// Wire server mode.
    Wire,
}

impl UiMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            UiMode::Shell => "shell",
            UiMode::Print => "print",
            UiMode::Acp => "acp",
            UiMode::Wire => "wire",
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum InputFormat {
    Text,
    #[value(name = "stream-json")]
    StreamJson,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Text,
    #[value(name = "stream-json")]
    StreamJson,
}

/// Builtin agent personalities.
///
/// Each variant selects a predefined agent configuration that controls
/// behavior, system prompts, and available tools.
#[derive(Debug, Clone, ValueEnum)]
pub enum AgentChoice {
    /// The standard general-purpose agent.
    ///
    /// Suitable for most coding tasks with balanced tool usage
    /// and default system instructions.
    Default,
    /// An agent styled after a mad scientist persona.
    ///
    /// More verbose, exploratory, and opinionated. Good for
    /// brainstorming, deep analysis, or when you want detailed
    /// reasoning with a distinctive voice.
    Okabe,
}

#[derive(Debug, Parser)]
#[command(
    name = "kimi",
    about = "Kimi, your next CLI agent.",
    version = crate::constant::get_version(),
    help_template = "{before-help}{about}\n\n{usage-heading} {usage}\n\n{options}\n{subcommands}{after-help}",
)]
pub struct Cli {
    #[arg(long, help = "Print verbose information.")]
    pub verbose: bool,

    #[arg(long, help = "Log debug information.")]
    pub debug: bool,

    #[arg(short = 'w', long, help = "Working directory for the agent.")]
    pub work_dir: Option<PathBuf>,

    #[arg(long, help = "Add an additional directory to the workspace scope.")]
    pub add_dir: Vec<PathBuf>,

    #[arg(
        short = 'S',
        long = "session",
        visible_alias = "resume",
        visible_short_alias = 'r',
        help = "Resume a session. With ID: resume that session. Without ID: interactively pick a session."
    )]
    pub session: Option<String>,

    #[arg(
        short = 'C',
        long = "continue",
        help = "Continue the previous session."
    )]
    pub continue_: bool,

    #[arg(long, help = "Config TOML/JSON string to load.")]
    pub config: Option<String>,

    #[arg(long, help = "Config TOML/JSON file to load.")]
    pub config_file: Option<PathBuf>,

    #[arg(short = 'm', long, help = "LLM model to use.")]
    pub model: Option<String>,

    #[arg(long, help = "Enable thinking mode.")]
    pub thinking: Option<bool>,

    #[arg(
        short = 'y',
        long,
        visible_alias = "yes",
        visible_alias = "auto-approve",
        help = "Automatically approve all actions."
    )]
    pub yolo: bool,

    #[arg(long, help = "Start in plan mode.")]
    pub plan: bool,

    #[arg(long, help = "Run in afk mode.")]
    pub afk: bool,

    #[arg(
        short = 'p',
        long,
        visible_alias = "command",
        visible_short_alias = 'c',
        help = "User prompt to the agent."
    )]
    pub prompt: Option<String>,

    #[arg(long, group = "ui", help = "Run in print mode (non-interactive).")]
    pub print: bool,

    #[arg(long, group = "ui", help = "Run as ACP server.")]
    pub acp: bool,

    #[arg(long, group = "ui", help = "Run as Wire server.")]
    pub wire: bool,

    #[arg(long, help = "Input format to use.")]
    pub input_format: Option<InputFormat>,

    #[arg(long, help = "Output format to use.")]
    pub output_format: Option<OutputFormat>,

    #[arg(long, help = "Only print the final assistant message.")]
    pub final_message_only: bool,

    #[arg(
        long,
        help = "Alias for --print --output-format text --final-message-only."
    )]
    pub quiet: bool,

    #[arg(long, help = "Builtin agent specification to use (default, okabe).")]
    pub agent: Option<AgentChoice>,

    #[arg(long, help = "Custom agent specification file.")]
    pub agent_file: Option<PathBuf>,

    #[arg(long, help = "MCP config file to load.")]
    pub mcp_config_file: Vec<PathBuf>,

    #[arg(long, help = "MCP config JSON to load.")]
    pub mcp_config: Vec<String>,

    #[arg(long, help = "Custom skills directories.")]
    pub skills_dir: Vec<PathBuf>,

    #[arg(long, help = "Maximum number of steps in one turn.")]
    pub max_steps_per_turn: Option<usize>,

    #[arg(long, help = "Maximum number of retries in one step.")]
    pub max_retries_per_step: Option<usize>,

    #[arg(long, help = "Extra iterations after the first turn in Ralph mode.")]
    pub max_ralph_iterations: Option<i32>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    /// Resolve the effective UI mode from the mutually exclusive flags.
    ///
    /// `--quiet` is treated as an alias for `--print` with additional
    /// output restrictions, so it also resolves to `UiMode::Print`.
    pub fn ui_mode(&self) -> UiMode {
        if self.quiet || self.print {
            UiMode::Print
        } else if self.acp {
            UiMode::Acp
        } else if self.wire {
            UiMode::Wire
        } else {
            UiMode::Shell
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Login to your account
    Login {
        #[arg(long, help = "Emit OAuth events as JSON lines.")]
        json: bool,
    },
    /// Logout from your account
    Logout {
        #[arg(long, help = "Emit OAuth events as JSON lines.")]
        json: bool,
    },
    /// Run Toad TUI backed by Kimi Code CLI ACP server
    Term,
    /// Run Kimi Code CLI ACP server
    Acp,
    /// Run background task worker subprocess (internal)
    #[command(name = "__background-task-worker", hide = true)]
    BackgroundTaskWorker {
        #[arg(long)]
        task_dir: PathBuf,
        #[arg(long, default_value = "5000")]
        heartbeat_interval_ms: u64,
        #[arg(long, default_value = "500")]
        control_poll_interval_ms: u64,
        #[arg(long, default_value = "2000")]
        kill_grace_period_ms: u64,
    },
    /// Run web worker subprocess (internal)
    #[command(name = "__web-worker", hide = true)]
    WebWorker { session_id: String },
    /// Export session data
    Export {
        /// Session ID to export (defaults to previous session)
        session_id: Option<String>,
        /// Output file path
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
        /// Skip confirmation when exporting default session
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Show info about the current session
    Info {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage plugins
    Plugin {
        #[command(subcommand)]
        command: Option<PluginCommands>,
    },
    /// Run Toad TUI
    Toad,
    /// Run web interface
    Web,
    /// Run visualizer interface
    Vis,
}

#[derive(Debug, Subcommand)]
pub enum PluginCommands {
    /// Install a plugin
    Install { target: String },
    /// List installed plugins
    List,
    /// Remove a plugin
    Remove { name: String },
    /// Show plugin info
    Info { name: String },
}

pub fn parse_cli() -> Cli {
    Cli::parse()
}
