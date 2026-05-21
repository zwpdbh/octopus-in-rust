use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum UiMode {
    Shell,
    Print,
    Acp,
    Wire,
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

#[derive(Debug, Clone, ValueEnum)]
pub enum AgentChoice {
    Default,
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

    #[arg(long, help = "Run in print mode (non-interactive).")]
    pub print: bool,

    #[arg(long, help = "Run as ACP server.")]
    pub acp: bool,

    #[arg(long, help = "Run as Wire server.")]
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

    #[arg(long, help = "Builtin agent specification to use.")]
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
    Export,
    /// Show info about the current session
    Info,
    /// Manage plugins
    Plugin,
    /// Run Toad TUI
    Toad,
    /// Run web interface
    Web,
    /// Run visualizer interface
    Vis,
}

pub fn parse_cli() -> Cli {
    Cli::parse()
}
