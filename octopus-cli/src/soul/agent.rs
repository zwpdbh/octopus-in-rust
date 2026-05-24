use std::collections::HashMap;
use std::path::PathBuf;

use crate::approval_runtime::ApprovalRuntime;
use crate::auth::OAuthManager;
use crate::background::BackgroundTaskManager;
use crate::config::Config;
use crate::llm::LLM;
use crate::notifications::manager::NotificationManager;
use crate::session::Session;
use crate::skills::Skill;
use crate::soul::approval::Approval;
use crate::soul::toolset::KimiToolset;
use crate::subagents::{LaborMarket, SubagentStore};
use crate::wire::RootWireHub;

#[derive(Debug, Clone)]
pub struct BuiltinSystemPromptArgs {
    pub kimi_now: String,
    pub kimi_work_dir: PathBuf,
    pub kimi_work_dir_ls: String,
    pub kimi_agents_md: String,
    pub kimi_skills: String,
    pub kimi_additional_dirs_info: String,
    pub kimi_os: String,
    pub kimi_shell: String,
}

#[derive(Debug, Clone)]
pub struct DenwaRenji;

impl DenwaRenji {
    pub fn new() -> Self {
        Self
    }

    pub fn set_n_checkpoints(&self, _n: usize) {}

    pub fn fetch_pending_dmail(&self) -> Option<Dmail> {
        None
    }
}

impl Default for DenwaRenji {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct Dmail {
    pub checkpoint_id: usize,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Environment {
    pub os_kind: String,
    pub shell_name: String,
    pub shell_path: String,
}

impl Environment {
    pub async fn detect() -> Self {
        Self {
            os_kind: std::env::consts::OS.to_string(),
            shell_name: "bash".to_string(),
            shell_path: "/bin/bash".to_string(),
        }
    }
}

pub struct Runtime {
    pub config: Config,
    pub oauth: OAuthManager,
    pub llm: Option<LLM>,
    pub session: Session,
    pub builtin_args: BuiltinSystemPromptArgs,
    pub denwa_renji: DenwaRenji,
    pub approval: Approval,
    pub labor_market: LaborMarket,
    pub environment: Environment,
    pub notifications: NotificationManager,
    pub background_tasks: BackgroundTaskManager,
    pub skills: HashMap<String, Skill>,
    pub additional_dirs: Vec<PathBuf>,
    pub skills_dirs: Vec<PathBuf>,
    pub subagent_store: Option<SubagentStore>,
    pub approval_runtime: Option<ApprovalRuntime>,
    pub root_wire_hub: Option<RootWireHub>,
    pub subagent_id: Option<String>,
    pub subagent_type: Option<String>,
    pub role: String,
    pub ui_mode: String,
    pub resumed: bool,
}

impl Runtime {
    pub fn new(
        config: Config,
        session: Session,
        llm: Option<LLM>,
        approval: Approval,
        builtin_args: BuiltinSystemPromptArgs,
    ) -> Self {
        let subagent_store = Some(SubagentStore::new(&session));
        let notification_root = crate::share::get_share_dir()
            .join("notifications")
            .join(&session.id);
        let notifications =
            NotificationManager::new(notification_root, config.notifications.clone());
        Self {
            config,
            oauth: OAuthManager::new(),
            llm,
            session,
            builtin_args,
            denwa_renji: DenwaRenji::new(),
            approval,
            labor_market: LaborMarket::new(),
            environment: Environment::detect_blocking(),
            notifications,
            background_tasks: BackgroundTaskManager::new(),
            skills: HashMap::new(),
            additional_dirs: Vec::new(),
            skills_dirs: Vec::new(),
            subagent_store,
            approval_runtime: Some(ApprovalRuntime::new()),
            root_wire_hub: Some(RootWireHub::new()),
            subagent_id: None,
            subagent_type: None,
            role: "root".to_string(),
            ui_mode: "shell".to_string(),
            resumed: false,
        }
    }
}

impl Environment {
    pub fn detect_blocking() -> Self {
        Self {
            os_kind: std::env::consts::OS.to_string(),
            shell_name: "bash".to_string(),
            shell_path: "/bin/bash".to_string(),
        }
    }
}

pub struct Agent {
    pub name: String,
    pub system_prompt: String,
    pub toolset: KimiToolset,
    pub runtime: Runtime,
}

pub async fn load_agent(
    _agent_file: &std::path::Path,
    runtime: Runtime,
) -> crate::exception::Result<Agent> {
    let toolset = KimiToolset::new();
    Ok(Agent {
        name: "default".to_string(),
        system_prompt: "You are a helpful assistant.".to_string(),
        toolset,
        runtime,
    })
}
