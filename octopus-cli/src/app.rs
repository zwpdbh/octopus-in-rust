use std::collections::HashMap;
use std::path::PathBuf;

use tracing;

use crate::cli::UiMode;
use crate::config::{Config, LLMModel, LLMProvider, load_config};
use crate::exception::Result;
use crate::llm::{LLM, augment_provider_with_env_vars, create_llm};
use crate::session::Session;

use crate::soul::{ApprovalState, KimiSoul};

pub fn enable_logging(debug: bool, redirect_stderr: bool) {
    let filter = if debug {
        tracing::level_filters::LevelFilter::DEBUG
    } else {
        tracing::level_filters::LevelFilter::INFO
    };

    let _ = tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_target(true)
        .init();

    let _ = redirect_stderr;
}

pub struct OctopusCLI {
    pub soul: Option<KimiSoul>,
    pub runtime: AppRuntime,
    pub env_overrides: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AppRuntime {
    pub config: Config,
    pub session: Session,
    pub llm: Option<LLM>,
    pub approval: ApprovalRuntime,
    pub ui_mode: UiMode,
    pub resumed: bool,
}

#[derive(Debug, Clone)]
pub struct ApprovalRuntime {
    yolo: bool,
    afk: bool,
}

impl ApprovalRuntime {
    pub fn is_yolo(&self) -> bool {
        self.yolo
    }

    pub fn is_afk(&self) -> bool {
        self.afk
    }
}

impl OctopusCLI {
    pub async fn create(
        session: Session,
        config: Option<Config>,
        config_path: Option<PathBuf>,
        model_name: Option<String>,
        thinking: Option<bool>,
        yolo: bool,
        afk: bool,
        plan_mode: bool,
        resumed: bool,
        ui_mode: UiMode,
        max_steps_per_turn: Option<usize>,
        max_retries_per_step: Option<usize>,
        max_ralph_iterations: Option<i32>,
        agent_file: Option<PathBuf>,
        mcp_configs: Vec<crate::mcp::McpConfig>,
    ) -> Result<Self> {
        let mut config = match config {
            Some(c) => c,
            None => load_config(config_path.as_deref())?,
        };

        if let Some(max_steps) = max_steps_per_turn {
            config.loop_control.max_steps_per_turn = max_steps;
        }
        if let Some(max_retries) = max_retries_per_step {
            config.loop_control.max_retries_per_step = max_retries;
        }
        if let Some(max_ralph) = max_ralph_iterations {
            config.loop_control.max_ralph_iterations = max_ralph;
        }

        let mut model: Option<LLMModel> = None;
        let mut provider: Option<LLMProvider> = None;

        if model_name.is_none() && !config.default_model.is_empty() {
            if let Some(m) = config.models.get(&config.default_model) {
                model = Some(m.clone());
                provider = config.providers.get(&m.provider).cloned();
            }
        }
        if let Some(ref name) = model_name {
            if let Some(m) = config.models.get(name) {
                model = Some(m.clone());
                provider = config.providers.get(&m.provider).cloned();
            }
        }

        let mut model = model.unwrap_or_else(|| LLMModel {
            provider: String::new(),
            model: String::new(),
            max_context_size: 100_000,
            capabilities: None,
            display_name: None,
        });
        let mut provider = provider.unwrap_or_else(|| LLMProvider {
            provider_type: crate::config::ProviderType::Kimi,
            base_url: String::new(),
            api_key: None,
            env: None,
            custom_headers: None,
            reasoning_key: None,
            oauth: None,
        });

        let env_overrides = augment_provider_with_env_vars(&mut provider, &mut model);

        let _thinking = thinking.unwrap_or(config.default_thinking);
        let yolo = yolo || config.default_yolo;
        let _plan_mode = if resumed {
            false
        } else {
            plan_mode || config.default_plan_mode
        };

        let llm = if !provider.base_url.is_empty() && !model.model.is_empty() {
            create_llm(&provider, &model, thinking, Some(&session.id))
        } else {
            None
        };

        let approval = ApprovalState {
            yolo: session.state.approval.yolo || yolo,
            afk: session.state.approval.afk || afk,
            auto_approve_actions: session.state.approval.auto_approve_actions.clone(),
        };

        // Build the heavyweight runtime for agent loading.
        let approval_wrapper = crate::soul::approval::Approval::with_state(approval.clone());
        let builtin_args = crate::soul::agent::BuiltinSystemPromptArgs {
            kimi_now: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            kimi_work_dir: session.work_dir.clone(),
            kimi_work_dir_ls: String::new(),
            kimi_agents_md: String::new(),
            kimi_skills: String::new(),
            kimi_additional_dirs_info: String::new(),
            kimi_os: std::env::consts::OS.to_string(),
            kimi_shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        };
        let app_runtime = crate::soul::agent::AppRuntime::new(
            config.clone(),
            session.clone(),
            llm.clone(),
            approval_wrapper,
            builtin_args,
        );

        // Load agent from spec, or fall back to a default agent.
        let agent = if let Some(ref path) = agent_file {
            match crate::soul::agent::load_agent(path, app_runtime, mcp_configs.clone()).await {
                Ok(agent) => agent,
                Err(e) => {
                    tracing::warn!(
                        "Failed to load agent from {}: {}, using default",
                        path.display(),
                        e
                    );
                    crate::soul::agent::Agent::new_basic(
                        "default".to_string(),
                        "You are a helpful assistant.".to_string(),
                        config.clone(),
                        session.clone(),
                        llm.clone(),
                        approval.clone(),
                        mcp_configs.clone(),
                    )
                }
            }
        } else {
            crate::soul::agent::Agent::new_basic(
                "default".to_string(),
                "You are a helpful assistant.".to_string(),
                config.clone(),
                session.clone(),
                llm.clone(),
                approval.clone(),
                mcp_configs.clone(),
            )
        };

        let soul = KimiSoul::new(
            config.clone(),
            session.clone(),
            llm.clone(),
            approval.clone(),
            agent,
            None,
        );

        let approval_runtime = ApprovalRuntime { yolo, afk };

        let runtime = AppRuntime {
            config: config.clone(),
            session: session.clone(),
            llm: llm.clone(),
            approval: approval_runtime,
            ui_mode,
            resumed,
        };

        // Initialize telemetry
        let telemetry_disabled =
            !config.telemetry || std::env::var("KIMI_DISABLE_TELEMETRY").is_ok();
        if telemetry_disabled {
            crate::telemetry::disable();
        } else {
            let device_id = crate::telemetry::get_or_create_device_id();
            crate::telemetry::set_context(device_id.clone(), session.id.clone());
            let transport = crate::telemetry::transport::AsyncTransport::new(
                device_id,
                std::sync::Arc::new(|| None),
                crate::share::get_telemetry_dir(),
            );
            let ui_mode_str = format!("{:?}", ui_mode).to_lowercase();
            let sink = crate::telemetry::sink::EventSink::new(
                transport,
                String::new(),
                model.model.clone(),
                ui_mode_str.clone(),
            );
            sink.start_periodic_flush();
            crate::telemetry::attach_sink(sink);
            crate::telemetry::track_session_started_once(&ui_mode_str, resumed);
        }

        Ok(OctopusCLI {
            soul: Some(soul),
            runtime,
            env_overrides: env_overrides.into_iter().collect(),
        })
    }

    pub async fn run_shell(
        &mut self,
        command: Option<String>,
        prefill_text: Option<String>,
    ) -> Result<bool> {
        tracing::info!("Running shell UI");

        let soul = self.soul.take().ok_or_else(|| {
            crate::exception::OctopusError::Other("Soul already consumed".to_string())
        })?;

        let mut shell = crate::ui::shell::ShellUI::new(soul);
        let result = shell
            .run(command.or(prefill_text))
            .await
            .map_err(|e| crate::exception::OctopusError::Other(e.to_string()))?;

        Ok(result)
    }

    pub async fn run_print(
        &mut self,
        input_format: crate::cli::InputFormat,
        output_format: crate::cli::OutputFormat,
        command: Option<String>,
        final_only: bool,
    ) -> Result<i32> {
        tracing::info!("Running print UI");

        let soul = self.soul.take().ok_or_else(|| {
            crate::exception::OctopusError::Other("Soul already consumed".to_string())
        })?;

        let mut print =
            crate::ui::print::PrintUI::new(soul, input_format, output_format, final_only);
        let result = print
            .run(command)
            .await
            .map_err(|e| crate::exception::OctopusError::Other(e.to_string()))?;

        Ok(result)
    }

    pub async fn run_acp(&mut self) -> Result<()> {
        tracing::info!("Running ACP server");
        println!("ACP server not yet implemented");
        Ok(())
    }

    pub async fn run_wire_stdio(&mut self) -> Result<()> {
        tracing::info!("Running Wire server over stdio");
        println!("Wire server not yet implemented");
        Ok(())
    }

    pub async fn shutdown_background_tasks(&mut self) {
        if let Some(ref mut soul) = self.soul {
            soul.shutdown().await;
        }
    }

    pub async fn await_bg_tasks_shutdown(&mut self, _timeout: f64) {
        // Background task cleanup is handled synchronously by KimiSoul::shutdown()
    }
}
