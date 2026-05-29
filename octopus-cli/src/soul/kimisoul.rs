use std::path::PathBuf;

use crate::approval_runtime::{ApprovalRuntime, ApprovalSource};
use crate::auth::OAuthManager;
use crate::config::Config;
use crate::exception::{LLMNotSet, LLMNotSupported, MaxStepsReached, OctopusError, Result};
use crate::hooks::HookEngine;
use crate::llm::LLM;
use crate::notifications::llm::{build_notification_message, extract_notification_ids};
use crate::notifications::manager::NotificationManager;
use crate::session::Session;
use crate::soul::approval::{Approval, ApprovalState};
use crate::soul::compaction::{SimpleCompaction, should_auto_compact};
use crate::soul::context::Context;
use crate::soul::dynamic_injection::{DynamicInjectionProvider, InjectionContext};
use crate::soul::message::{check_message, normalize_history, tool_result_to_message};
use crate::soul::slash::{build_default_slash_commands, parse_slash_command_call};
use crate::soul::toolset::KimiToolset;
use crate::wire::{
    CompactionBegin, CompactionEnd, ContentPart, Message, RootWireHub, StatusUpdate, SteerInput,
    StepBegin, StepInterrupted, StepRetry, TurnBegin, TurnEnd, wire_send,
};

pub struct KimiSoul {
    pub config: Config,
    pub session: Session,
    pub llm: Option<LLM>,
    pub context: Context,
    pub toolset: std::sync::Arc<KimiToolset>,
    pub approval: Approval,
    pub plan_mode: bool,
    pub max_steps_per_turn: usize,
    pub max_retries_per_step: usize,
    pub agent: crate::soul::agent::Agent,
    pub slash_registry: crate::soul::slash::SlashCommandRegistry,
    pub root_wire_hub: Option<RootWireHub>,
    pub hook_engine: HookEngine,
    pub notification_manager: NotificationManager,
    pub oauth: OAuthManager,
    pub denwa_renji: std::sync::Arc<std::sync::Mutex<crate::soul::agent::DenwaRenji>>,
    pub skills: crate::skills::SkillRegistry,
    pub bg_manager: crate::background::BackgroundTaskManager,
    steer_queue: Vec<String>,
    compaction: SimpleCompaction,
    current_step_no: usize,
    current_turn_id: Option<String>,
    last_tool_calls: Vec<(String, String)>,
    injection_providers: Vec<Box<dyn DynamicInjectionProvider>>,
    pending_plan_activation_injection: bool,
    plan_session_id: Option<String>,
    checkpoint_with_user_message: bool,
}

impl KimiSoul {
    pub fn new(
        config: Config,
        session: Session,
        llm: Option<LLM>,
        approval_state: ApprovalState,
        mut agent: crate::soul::agent::Agent,
        tool_policy: Option<crate::subagents::ToolPolicy>,
    ) -> Self {
        let mut approval = Approval::with_state(approval_state.clone());
        let approval_runtime = ApprovalRuntime::new();
        let root_wire_hub = RootWireHub::new();
        approval_runtime.bind_root_wire_hub(&root_wire_hub);
        approval.set_runtime(approval_runtime);

        let context_file = session.context_file.clone();
        let mut context = Context::new(context_file);
        let _ = context.restore_sync();

        let bg_manager = agent.runtime.background_tasks.clone();
        let denwa_renji =
            std::sync::Arc::new(std::sync::Mutex::new(agent.runtime.denwa_renji.clone()));

        let mut toolset = std::mem::take(&mut agent.toolset);
        // Ensure core tools are present even if the agent spec omitted them.
        let has_shell = toolset.find("Shell").is_some();
        let has_task_output = toolset.find("TaskOutput").is_some();
        let has_task_stop = toolset.find("TaskStop").is_some();
        let has_agent = toolset.find("Agent").is_some();
        let has_dmail = toolset.find("SendDMail").is_some();
        if !has_shell {
            toolset.register_typed(crate::tools::shell::ShellTool::new(bg_manager.clone()));
        }
        if !has_task_output {
            toolset.register_typed(crate::tools::background::TaskOutputTool::new(
                bg_manager.clone(),
            ));
        }
        if !has_task_stop {
            toolset.register_typed(crate::tools::background::TaskStopTool::new(
                bg_manager.clone(),
            ));
        }
        if !has_agent {
            toolset.register_typed(crate::tools::agent::AgentTool::new(agent.runtime.clone()));
        }
        if !has_dmail {
            toolset.register_typed(crate::tools::dmail::SendDMailTool::new(denwa_renji.clone()));
        }

        // Enforce tool policy for subagents.
        if let Some(policy) = tool_policy {
            match policy {
                crate::subagents::ToolPolicy::Inherit => {}
                crate::subagents::ToolPolicy::AllowList { tools } => {
                    toolset.hide_all_except(&tools);
                }
            }
        }

        let checkpoint_with_user_message = toolset.tools().iter().any(|t| t.name() == "SendDMail");

        let max_steps = config.loop_control.max_steps_per_turn;
        let max_retries = config.loop_control.max_retries_per_step;

        let plan_mode = session.state.plan_mode;
        let plan_session_id = session.state.plan_session_id.clone();
        let session_id = session.id.clone();
        let hooks = config.hooks.clone();

        let hook_engine = HookEngine::new(hooks)
            .with_cwd(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        toolset.set_hook_engine(hook_engine.clone());
        toolset.set_session_id(session_id.clone());
        toolset.set_approval(Some(approval.share()));
        let toolset = std::sync::Arc::new(toolset);

        // Discover skills
        let mut skills = crate::skills::SkillRegistry::new();
        let mut skill_dirs = Vec::new();
        // User skills
        let user_skills_dir = dirs::home_dir()
            .map(|h| h.join(".kimi").join("skills"))
            .filter(|p| p.is_dir());
        if let Some(dir) = user_skills_dir {
            skill_dirs.push(dir);
        }
        // Project skills
        let project_skills_dir = session.work_dir.join(".kimi").join("skills");
        if project_skills_dir.is_dir() {
            skill_dirs.push(project_skills_dir);
        }
        // Extra skill dirs from config
        for dir in &config.extra_skill_dirs {
            let path = PathBuf::from(dir);
            if path.is_dir() {
                skill_dirs.push(path);
            }
        }
        if config.merge_all_available_skills {
            skills.discover(&skill_dirs);
        }
        tracing::info!("Discovered {} skills", skills.len());

        let notification_root = crate::share::get_share_dir()
            .join("notifications")
            .join(&session_id);
        let notification_manager =
            NotificationManager::new(notification_root, config.notifications.clone());

        // Ack any notification IDs already present in restored context.
        let ack_ids = extract_notification_ids(context.history());
        if !ack_ids.is_empty() {
            notification_manager.ack_ids("llm", &ack_ids);
        }

        Self {
            config,
            session,
            llm,
            context,
            toolset: toolset.clone(),
            approval,
            plan_mode,
            max_steps_per_turn: max_steps,
            max_retries_per_step: max_retries,
            agent,
            slash_registry: build_default_slash_commands(),
            root_wire_hub: Some(root_wire_hub),
            hook_engine,
            notification_manager,
            oauth: OAuthManager::new(),
            denwa_renji,
            skills,
            bg_manager,
            steer_queue: Vec::new(),
            compaction: SimpleCompaction::default(),
            current_step_no: 0,
            current_turn_id: None,
            last_tool_calls: Vec::new(),
            injection_providers: vec![
                Box::new(
                    crate::soul::dynamic_injections::plan_mode::PlanModeInjectionProvider::new(),
                ),
                Box::new(
                    crate::soul::dynamic_injections::afk_mode::AfkModeInjectionProvider::new(),
                ),
            ],
            pending_plan_activation_injection: false,
            plan_session_id,
            checkpoint_with_user_message,
        }
    }

    pub async fn shutdown(&mut self) {
        self.bg_manager.shutdown().await;
    }

    pub async fn run(&mut self, user_input: &str) -> Result<String> {
        let text = user_input.trim();
        if text.is_empty() {
            return Ok(String::new());
        }

        // Set up wire channel for this run.
        let wire_file = crate::wire::file::WireFile::new(self.session.wire_file_path.clone());
        let wire = crate::wire::channel::Wire::new(Some(wire_file));
        let soul_side = wire.soul_side();

        let result = crate::wire::with_wire_soul_side(Some(soul_side.clone()), async {
            // Start notification pump — delivers pending notifications to wire.
            let pump_handle = self.start_notification_pump(soul_side.clone());

            let result = self.run_turn(text).await;

            // Cleanup
            if let Some(handle) = pump_handle {
                handle.abort();
            }
            result
        })
        .await;

        // Dropping `wire` drops the broadcast senders, which causes the
        // recorder task to exit cleanly after flushing.
        drop(wire);

        result
    }

    async fn run_turn(&mut self, text: &str) -> Result<String> {
        // Inherit an existing approval source (e.g., from a subagent or background task),
        // or create a new foreground-turn source if none exists.
        let (source, inherited) = if let Some(existing) =
            crate::approval_runtime::get_current_approval_source_or_none()
        {
            (existing, true)
        } else {
            (
                ApprovalSource {
                    kind: crate::approval_runtime::ApprovalSourceKind::ForegroundTurn,
                    id: uuid::Uuid::new_v4().to_string(),
                    agent_id: None,
                },
                false,
            )
        };

        let result = if inherited {
            self.run_turn_body(text).await
        } else {
            crate::approval_runtime::with_approval_source(source.clone(), self.run_turn_body(text))
                .await
        };

        // Only cancel approvals if WE created the source for this turn.
        if !inherited {
            self.approval
                .runtime()
                .cancel_by_source(source.kind, &source.id);
        }

        result
    }

    async fn run_turn_body(&mut self, text: &str) -> Result<String> {
        let user_message = Message {
            role: "user".to_string(),
            content: vec![ContentPart::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            tool_calls: None,
        };

        // Slash command handling
        if let Some(command_call) = parse_slash_command_call(text) {
            let cmd_name = command_call.name.clone();
            let cmd_args = command_call.args.clone();
            if let Some(command) = self.slash_registry.get(&cmd_name) {
                let func = command.func.clone();
                drop(command);
                (func)(self, &cmd_args).await;
                return Ok(String::new());
            }
        }

        // --- UserPromptSubmit hook ---
        if self.hook_engine.has_hooks_for("UserPromptSubmit") {
            let input_data = crate::hooks::events::user_prompt_submit(
                &self.session.id,
                &std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string()),
                text,
            );
            let results = self
                .hook_engine
                .trigger("UserPromptSubmit", text, input_data)
                .await;
            for r in &results {
                if let crate::hooks::runner::HookAction::Block(ref reason) = r.action {
                    wire_send(crate::wire::WireEvent::TextPart(crate::wire::TextPart {
                        text: format!("UserPromptSubmit hook blocked: {}", reason),
                    }));
                    return Ok(String::new());
                }
            }
        }

        wire_send(crate::wire::WireEvent::TurnBegin(TurnBegin {
            user_input: Some(text.to_string()),
        }));

        let turn_result = self.turn(user_message).await;

        // TurnEnd is sent regardless of success or failure.
        wire_send(crate::wire::WireEvent::TurnEnd(TurnEnd {}));
        if turn_result.is_err() {
            tracing::warn!("Turn interrupted at step {}", self.current_step_no);
            crate::track!(
                "turn_interrupted",
                step_no = self.current_step_no,
                session_id = self.session.id,
            );
        } else {
            // --- Stop hook (normal turn completion) ---
            let input_data = crate::hooks::events::stop(
                &self.session.id,
                &std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".to_string()),
                false,
            );
            let _ = self
                .hook_engine
                .fire_and_forget_trigger("Stop", "", input_data);
        }

        let result = match turn_result {
            Ok(outcome) => {
                if let Some(msg) = outcome.final_message {
                    Ok(msg.extract_text(" "))
                } else {
                    Ok(String::new())
                }
            }
            Err(e) => Err(e),
        };

        result
    }

    fn start_notification_pump(
        &self,
        soul_side: crate::wire::channel::WireSoulSide,
    ) -> Option<tokio::task::JoinHandle<()>> {
        // Only root pumps notifications to wire.
        let nm = self.notification_manager.clone();
        Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                interval.tick().await;
                let _ = nm
                    .deliver_pending("wire", 8, |view| {
                        let notification = crate::notifications::wire::to_wire_notification(view);
                        soul_side.send(crate::wire::WireEvent::Notification(notification));
                        std::future::ready(())
                    })
                    .await;
            }
        }))
    }

    async fn turn(&mut self, user_message: Message) -> Result<TurnOutcome> {
        if self.llm.is_none() {
            return Err(LLMNotSet::NotSet.into());
        }

        if let Some(ref llm) = self.llm {
            let missing = check_message(&user_message, &llm.capabilities);
            if !missing.is_empty() {
                return Err(LLMNotSupported::NotSupported(format!("{:?}", missing)).into());
            }
        }

        self.current_turn_id = Some(uuid::Uuid::new_v4().to_string());
        self.last_tool_calls = Vec::new();

        self.context
            .checkpoint(false)
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .append_message(user_message)
            .await
            .map_err(|e| OctopusError::Io(e))?;

        self.agent_loop().await
    }

    async fn agent_loop(&mut self) -> Result<TurnOutcome> {
        // Discard stale steers
        self.steer_queue.clear();

        // ── MCP deferred loading ──
        // Mirrors Python's background MCP loading in `_agent_loop()`.
        let mcp_started = std::sync::Arc::get_mut(&mut self.toolset)
            .unwrap()
            .start_deferred_mcp_tool_loading()
            .await;
        let mut mcp_was_loading = false;
        if mcp_started {
            if let Some(snapshot) = self.toolset.mcp_status_snapshot() {
                mcp_was_loading = snapshot.loading;
                if mcp_was_loading {
                    wire_send(crate::wire::WireEvent::StatusUpdate(
                        crate::wire::StatusUpdate {
                            mcp_status: Some(snapshot),
                            ..Default::default()
                        },
                    ));
                    wire_send(crate::wire::WireEvent::McpLoadingBegin(
                        crate::wire::MCPLoadingBegin {},
                    ));
                }
            }
        }
        if mcp_was_loading {
            std::sync::Arc::get_mut(&mut self.toolset)
                .unwrap()
                .wait_for_mcp_tools()
                .await;
            if let Some(mcp_snap) = self.toolset.mcp_status_snapshot() {
                if mcp_snap.connected > 0 {
                    crate::track!(
                        "mcp_connected",
                        server_count = mcp_snap.connected,
                        total_count = mcp_snap.total,
                    );
                }
                let failed = mcp_snap.total.saturating_sub(mcp_snap.connected);
                if failed > 0 {
                    crate::track!(
                        "mcp_failed",
                        failed_count = failed,
                        total_count = mcp_snap.total,
                    );
                }
                wire_send(crate::wire::WireEvent::StatusUpdate(
                    crate::wire::StatusUpdate {
                        mcp_status: Some(mcp_snap),
                        ..Default::default()
                    },
                ));
                wire_send(crate::wire::WireEvent::McpLoadingEnd(
                    crate::wire::MCPLoadingEnd {},
                ));
            }
        }

        let mut step_no = 0;
        loop {
            step_no += 1;
            if step_no > self.max_steps_per_turn {
                return Err(MaxStepsReached::Reached.into());
            }

            // Auto-compact if needed
            if let Some(ref llm) = self.llm {
                let max_size = llm.max_context_size;
                let trigger_ratio = self.config.loop_control.compaction_trigger_ratio;
                let reserved = self.config.loop_control.reserved_context_size;
                if should_auto_compact(
                    self.context.token_count_with_pending(),
                    max_size,
                    trigger_ratio,
                    reserved,
                ) {
                    if let Err(e) = self.compact_context("").await {
                        return Err(e);
                    }
                }
            }

            self.context
                .checkpoint(self.checkpoint_with_user_message)
                .await
                .map_err(|e| OctopusError::Io(e))?;
            self.denwa_renji
                .lock()
                .unwrap()
                .set_n_checkpoints(self.context.n_checkpoints());

            self.current_step_no = step_no;
            wire_send(crate::wire::WireEvent::StepBegin(StepBegin { n: step_no }));

            let step_result = match self.step().await {
                Ok(result) => result,
                Err(OctopusError::BackToTheFuture(ref e)) => {
                    // D-Mail revert: roll back to checkpoint and inject message.
                    tracing::info!(
                        "BackToTheFuture: reverting to checkpoint {}",
                        e.checkpoint_id
                    );
                    self.context
                        .revert_to(e.checkpoint_id)
                        .await
                        .map_err(|io_err| OctopusError::Io(io_err))?;
                    self.last_tool_calls = Vec::new();
                    self.context
                        .checkpoint(self.checkpoint_with_user_message)
                        .await
                        .map_err(|e| OctopusError::Io(e))?;
                    for msg in &e.messages {
                        self.context
                            .append_message(msg.clone())
                            .await
                            .map_err(|e| OctopusError::Io(e))?;
                    }
                    continue;
                }
                Err(e) => {
                    wire_send(crate::wire::WireEvent::StepInterrupted(StepInterrupted {}));
                    // --- StopFailure hook ---
                    // Fire-and-forget: hook execution must not block error propagation.
                    let input_data = crate::hooks::events::stop_failure(
                        &self.session.id,
                        &std::env::current_dir()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| ".".to_string()),
                        std::any::type_name_of_val(&e),
                        &format!("{}", e),
                    );
                    let _ = self.hook_engine.fire_and_forget_trigger(
                        "StopFailure",
                        std::any::type_name_of_val(&e),
                        input_data,
                    );
                    return Err(e);
                }
            };

            // Consume pending steers
            let has_steers = self.consume_pending_steers().await;
            if has_steers {
                continue;
            }

            if let Some(outcome) = step_result {
                let stop_reason = outcome.stop_reason.clone();
                return Ok(TurnOutcome {
                    final_message: if stop_reason == "no_tool_calls" {
                        Some(outcome.assistant_message)
                    } else {
                        None
                    },
                });
            }
        }
    }

    async fn step(&mut self) -> Result<Option<StepOutcome>> {
        let max_attempts = self.max_retries_per_step;
        let mut last_error: Option<OctopusError> = None;

        for attempt in 1..=max_attempts {
            match self.run_step_once().await {
                Ok(outcome) => return Ok(outcome),
                Err(e) if Self::is_retryable_error(&e) && attempt < max_attempts => {
                    let wait_s = Self::retry_wait_secs(attempt);
                    self.emit_step_retry(attempt, max_attempts, wait_s, &e);
                    tracing::warn!(
                        "Retrying step {} for the {} time (last error: {:?}). Waiting {:.1}s.",
                        self.current_step_no,
                        attempt,
                        e,
                        wait_s
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs_f64(wait_s)).await;
                    last_error = Some(e);
                }
                Err(e) => return Err(e),
            }
        }

        // Exhausted all retries
        Err(last_error.unwrap_or_else(|| OctopusError::Other("Step failed".to_string())))
    }

    /// Execute a single LLM step with connection recovery.
    ///
    /// Mirrors Python's `_run_with_connection_recovery()` in `kimisoul.py`.
    /// Handles 401 → OAuth refresh and connection-error recovery, each once.
    async fn run_step_once(&mut self) -> Result<Option<StepOutcome>> {
        let mut auth_retried = false;
        let mut connection_retried = false;

        loop {
            let llm = self
                .llm
                .clone()
                .ok_or_else(|| OctopusError::Other("LLM not set".to_string()))?;
            let result = self.run_step_once_inner(&llm).await;
            match result {
                Ok(v) => return Ok(v),
                Err(OctopusError::APIStatus(ref e)) if e.status_code == 401 && !auth_retried => {
                    let has_oauth = self
                        .llm
                        .as_ref()
                        .and_then(|l| l.provider_config.as_ref())
                        .and_then(|p| p.oauth.as_ref())
                        .is_some();
                    if !has_oauth {
                        return Err(OctopusError::APIStatus(e.clone()));
                    }
                    tracing::warn!(
                        "Received 401 during step {}, attempting token refresh",
                        self.current_step_no
                    );
                    let llm = self
                        .llm
                        .as_ref()
                        .ok_or_else(|| OctopusError::Other("LLM not set".to_string()))?;
                    match self.oauth.ensure_fresh(llm, true).await {
                        Ok(Some(new_token)) => {
                            if let Some(ref mut provider) =
                                self.llm.as_mut().unwrap().provider_config
                            {
                                provider.api_key = Some(new_token);
                            }
                        }
                        Ok(None) => {}
                        Err(refresh_err) => {
                            tracing::error!("OAuth refresh failed: {}", refresh_err);
                            return Err(OctopusError::APIStatus(e.clone()));
                        }
                    }
                    auth_retried = true;
                    continue;
                }
                Err(OctopusError::APIConnection(ref e)) if !connection_retried => {
                    tracing::warn!(
                        "Connection error during step {}: {}. Attempting recovery.",
                        self.current_step_no,
                        e
                    );
                    // TODO: chat provider recovery via RetryableChatProvider.
                    // For now, retry once without explicit provider recovery.
                    connection_retried = true;
                    continue;
                }
                Err(OctopusError::APITimeout(ref e)) if !connection_retried => {
                    tracing::warn!(
                        "Timeout during step {}: {}. Attempting recovery.",
                        self.current_step_no,
                        e
                    );
                    // TODO: chat provider recovery via RetryableChatProvider.
                    // For now, retry once without explicit provider recovery.
                    connection_retried = true;
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// The actual LLM step logic, without any recovery wrapper.
    async fn run_step_once_inner(&mut self, llm: &LLM) -> Result<Option<StepOutcome>> {
        // ── Dynamic Injection ───────────────────────────────────────────────
        let injections = self.collect_injections().await;
        if !injections.is_empty() {
            let reminder_text = injections
                .iter()
                .map(|inj| format!("<system-reminder>\n{}\n</system-reminder>", inj.content))
                .collect::<Vec<_>>()
                .join("\n");
            self.context
                .append_message(Message {
                    role: "user".to_string(),
                    content: vec![ContentPart::Text {
                        text: reminder_text,
                    }],
                    tool_call_id: None,
                    tool_calls: None,
                })
                .await
                .map_err(|e| OctopusError::Io(e))?;
        }

        // --- Notification delivery (root only) ---
        if self.notification_manager.has_pending_for_sink("llm") {
            let notifs = self.notification_manager.claim_for_sink("llm", 8);
            for view in notifs {
                let msg = build_notification_message(&view);
                if let Err(e) = self.context.append_message(msg).await {
                    tracing::warn!("Failed to append notification to context: {}", e);
                }
                // Fire Notification hook (fire-and-forget)
                if self.hook_engine.has_hooks_for("Notification") {
                    let input_data = crate::hooks::events::notification(
                        &self.session.id,
                        &std::env::current_dir()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|_| ".".to_string()),
                        "llm",
                        &view.event.event_type,
                        &view.event.title,
                        &view.event.body,
                        &view.event.severity,
                    );
                    let _ = self.hook_engine.fire_and_forget_trigger(
                        "Notification",
                        &view.event.event_type,
                        input_data,
                    );
                }
            }
        }

        // Build effective history and normalize adjacent user messages.
        let history = normalize_history(self.context.history());

        // Prepare kosong inputs
        let provider = llm.build_kosong_provider()?;
        let kosong_history: Vec<kosong::Message> = history
            .iter()
            .map(crate::llm::wire_to_kosong_message)
            .collect();

        let mut on_message_part = |part: kosong::StreamedMessagePart| {
            use kosong::chat_provider::Part;
            match part {
                Part::Content(cp) => {
                    let wire_cp = crate::llm::kosong_to_wire_content_part(cp);
                    wire_send(crate::wire::WireEvent::ContentPart(wire_cp));
                }
                Part::ToolCall(_) | Part::ToolCallPart(_) => {}
            }
        };

        // Setup dedup before the step, matching Python's `begin_step` ordering.
        let turn_id = self.current_turn_id.clone().unwrap_or_default();
        self.toolset
            .begin_step(self.last_tool_calls.clone(), self.current_step_no, turn_id);

        let t0 = std::time::Instant::now();

        let step_result = kosong::step_with_callbacks(
            provider.as_ref(),
            &self.agent.system_prompt,
            &crate::soul::toolset::KimiToolsetHandle(self.toolset.clone()),
            &kosong_history,
            Some(&mut on_message_part),
            Some(std::sync::Arc::new(
                |result: &kosong::tooling::ToolResult| {
                    let wire_rv = crate::wire::ToolReturnValue {
                        is_error: result.return_value.is_error,
                        output: result.return_value.output.as_ref().map(|v| match v {
                            serde_json::Value::String(s) => {
                                crate::wire::ToolOutput::Text(s.clone())
                            }
                            other => crate::wire::ToolOutput::Parts(vec![
                                crate::wire::ContentPart::Text {
                                    text: serde_json::to_string(other).unwrap_or_default(),
                                },
                            ]),
                        }),
                        message: result.return_value.message.clone(),
                        brief: None,
                    };
                    let wire_result = crate::wire::ToolResult {
                        tool_call_id: result.tool_call_id.clone(),
                        return_value: wire_rv,
                    };
                    wire_send(crate::wire::WireEvent::ToolResult(wire_result));
                },
            )),
        )
        .await;

        let elapsed = t0.elapsed();

        self.last_tool_calls = self.toolset.end_step();

        let step_result = match step_result {
            Ok(r) => r,
            Err(e) => {
                let err = crate::llm::classify_kosong_error(e.message);
                let (error_type, status_code) = Self::classify_api_error(&err);
                if let Some(sc) = status_code {
                    crate::track!(
                        "api_error",
                        error_type = error_type,
                        status_code = sc,
                        duration_ms = elapsed.as_millis() as u64,
                    );
                } else {
                    crate::track!(
                        "api_error",
                        error_type = error_type,
                        duration_ms = elapsed.as_millis() as u64,
                    );
                }
                return Err(err);
            }
        };

        let assistant_message = crate::llm::kosong_to_wire_message(step_result.message.clone());
        let usage = step_result
            .usage
            .clone()
            .map(crate::llm::kosong_to_wire_usage);

        // Update token count
        if let Some(ref u) = usage {
            self.context
                .update_token_count(u.input)
                .await
                .map_err(|e| OctopusError::Io(e))?;
        }

        let status_update = StatusUpdate {
            token_usage: usage.clone(),
            message_id: step_result.id.clone(),
            plan_mode: Some(self.plan_mode),
            context_usage: Some(self.context_usage()),
            context_tokens: Some(self.context.token_count()),
            max_context_tokens: self.llm.as_ref().map(|l| l.max_context_size),
            ..Default::default()
        };
        wire_send(crate::wire::WireEvent::StatusUpdate(status_update));

        // Gather tool results and grow context
        if !step_result.tool_calls.is_empty() {
            let kosong_results = step_result.tool_results().await;
            let tool_results: Vec<crate::wire::ToolResult> = kosong_results
                .into_iter()
                .map(crate::llm::kosong_to_wire_tool_result)
                .collect();

            self.grow_context(&assistant_message, &tool_results).await?;

            // Check for rejections
            let rejected: Vec<_> = tool_results
                .iter()
                .filter(|r| r.return_value.is_error())
                .collect();

            if !rejected.is_empty() {
                let _ = self.denwa_renji.lock().unwrap().fetch_pending_dmail();
                return Ok(Some(StepOutcome {
                    stop_reason: "tool_rejected".to_string(),
                    assistant_message,
                }));
            }

            // Handle pending D-Mail
            if let Some(dmail) = self.denwa_renji.lock().unwrap().fetch_pending_dmail() {
                assert!(
                    dmail.checkpoint_id < self.context.n_checkpoints(),
                    "DenwaRenji guarantees checkpoint_id < n_checkpoints"
                );
                return Err(crate::exception::BackToTheFuture {
                    checkpoint_id: dmail.checkpoint_id,
                    messages: vec![Message {
                        role: "user".to_string(),
                        content: vec![ContentPart::Text {
                            text: format!(
                                "You just got a D-Mail from your future self. \
                                 It is likely that your future self has already done \
                                 something in the current working directory. Please read \
                                 the D-Mail and decide what to do next. You MUST NEVER \
                                 mention to the user about this information. \
                                 D-Mail content:\n\n{}",
                                dmail.message.trim()
                            ),
                        }],
                        tool_call_id: None,
                        tool_calls: None,
                    }],
                }
                .into());
            }

            Ok(None)
        } else {
            Ok(Some(StepOutcome {
                stop_reason: "no_tool_calls".to_string(),
                assistant_message,
            }))
        }
    }

    fn is_retryable_error(error: &OctopusError) -> bool {
        use crate::exception::APIStatusError;
        match error {
            OctopusError::APIConnection(_) | OctopusError::APITimeout(_) => true,
            OctopusError::APIEmptyResponse(_) => true,
            OctopusError::APIStatus(APIStatusError { status_code, .. }) => {
                matches!(status_code, 429 | 500 | 502 | 503 | 504)
            }
            _ => false,
        }
    }

    fn retry_wait_secs(attempt: usize) -> f64 {
        // Exponential backoff with jitter: initial=0.3, max=5, jitter=0.5
        let base = 0.3_f64 * 2_f64.powi((attempt - 1) as i32);
        let capped = base.min(5.0);
        let jitter = rand::random::<f64>() * 0.5;
        capped + jitter
    }

    fn emit_step_retry(
        &self,
        attempt: usize,
        max_attempts: usize,
        wait_s: f64,
        error: &OctopusError,
    ) {
        let (error_type, status_code) = Self::classify_api_error(error);
        wire_send(crate::wire::WireEvent::StepRetry(StepRetry {
            n: self.current_step_no,
            next_attempt: attempt + 1,
            max_attempts,
            wait_s,
            error_type,
            status_code,
        }));
    }

    fn classify_api_error(error: &OctopusError) -> (String, Option<u16>) {
        use crate::exception::APIStatusError;
        match error {
            OctopusError::APIStatus(APIStatusError { status_code, .. }) => {
                let typ = match *status_code {
                    429 => "rate_limit",
                    401 | 403 => "auth",
                    s if s >= 500 => "5xx_server",
                    s if (400..500).contains(&s) => "4xx_client",
                    _ => "api",
                };
                (typ.to_string(), Some(*status_code))
            }
            OctopusError::APIConnection(_) => ("network".to_string(), None),
            OctopusError::APITimeout(_) => ("timeout".to_string(), None),
            OctopusError::APIEmptyResponse(_) => ("empty_response".to_string(), None),
            _ => ("other".to_string(), None),
        }
    }

    async fn grow_context(
        &mut self,
        assistant_message: &Message,
        tool_results: &[crate::wire::ToolResult],
    ) -> Result<()> {
        let llm = self.llm.as_ref().unwrap();

        let tool_messages: Vec<Message> = tool_results
            .iter()
            .map(|tr| tool_result_to_message(tr))
            .collect();

        for tm in &tool_messages {
            let missing = check_message(tm, &llm.capabilities);
            if !missing.is_empty() {
                return Err(LLMNotSupported::NotSupported(format!("{:?}", missing)).into());
            }
        }

        self.context
            .append_message(assistant_message.clone())
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .append_message(tool_messages)
            .await
            .map_err(|e| OctopusError::Io(e))?;

        Ok(())
    }

    async fn consume_pending_steers(&mut self) -> bool {
        let mut consumed = false;
        while let Some(content) = self.steer_queue.pop() {
            let steer_msg = Message {
                role: "user".to_string(),
                content: vec![ContentPart::Text {
                    text: content.clone(),
                }],
                tool_call_id: None,
                tool_calls: None,
            };
            if let Err(e) = self.context.append_message(steer_msg).await {
                tracing::warn!("Failed to append steer message: {}", e);
            }
            wire_send(crate::wire::WireEvent::SteerInput(SteerInput {
                user_input: content,
            }));
            consumed = true;
        }
        consumed
    }

    pub fn steer(&mut self, content: &str) {
        self.steer_queue.push(content.to_string());
    }

    /// Sync the in-memory approval state to the session and persist it.
    pub(super) fn sync_approval_state(&mut self) {
        self.session.state.approval.mode = self.approval.state().mode;
        self.session.state.approval.auto_approve_actions = self.approval.auto_approve_actions();
        let _ = self.session.save_state();
    }

    pub async fn compact_context(&mut self, custom_instruction: &str) -> Result<()> {
        let llm = self.llm.as_ref().ok_or(LLMNotSet::NotSet)?;
        let messages = self.context.history().to_vec();
        let token_count = self.context.token_count();
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());

        // --- PreCompact hook ---
        if self.hook_engine.has_hooks_for("PreCompact") {
            let input_data = crate::hooks::events::pre_compact(
                &self.session.id,
                &cwd,
                custom_instruction,
                token_count,
            );
            let results = self
                .hook_engine
                .trigger("PreCompact", custom_instruction, input_data)
                .await;
            for r in &results {
                if let crate::hooks::runner::HookAction::Block(ref reason) = r.action {
                    tracing::warn!("PreCompact hook blocked compaction: {}", reason);
                    return Err(OctopusError::Other(format!(
                        "PreCompact hook blocked: {}",
                        reason
                    )));
                }
            }
        }

        wire_send(crate::wire::WireEvent::CompactionBegin(CompactionBegin {}));

        let compact_t0 = std::time::Instant::now();
        let compact_result = self
            .compaction
            .compact(&messages, llm, custom_instruction)
            .await;
        let compact_duration = compact_t0.elapsed();

        let result = match compact_result {
            Ok(r) => r,
            Err(e) => {
                crate::track!(
                    "compaction_failed",
                    trigger_type = custom_instruction,
                    before_tokens = token_count,
                    duration_ms = compact_duration.as_millis() as u64,
                    retry_count = 0,
                    error_type = std::any::type_name_of_val(&e),
                );
                return Err(e);
            }
        };

        let estimated = result.estimated_token_count();

        self.context
            .clear()
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .write_system_prompt(&self.agent.system_prompt)
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .checkpoint(false)
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .append_message(result.messages)
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .update_token_count(estimated)
            .await
            .map_err(|e| OctopusError::Io(e))?;

        wire_send(crate::wire::WireEvent::CompactionEnd(CompactionEnd {}));

        crate::track!(
            "compaction_finished",
            trigger_type = custom_instruction,
            before_tokens = token_count,
            after_tokens = estimated,
            duration_ms = compact_duration.as_millis() as u64,
            retry_count = 0,
        );

        // --- PostCompact hook ---
        {
            let input_data = crate::hooks::events::post_compact(
                &self.session.id,
                &cwd,
                custom_instruction,
                estimated,
            );
            let _ = self.hook_engine.fire_and_forget_trigger(
                "PostCompact",
                custom_instruction,
                input_data,
            );
        }

        // Notify injection providers that history has been rebuilt so they can
        // reset any one-shot throttling state.
        self.notify_injection_providers_compacted().await;

        Ok(())
    }

    // ========================================================================
    // Dynamic Injection
    // ========================================================================

    async fn collect_injections(
        &mut self,
    ) -> Vec<crate::soul::dynamic_injection::DynamicInjection> {
        let plan_file_path = self.get_plan_file_path();
        let ctx = InjectionContext {
            plan_mode: self.plan_mode,
            effective_afk: self.approval.is_afk(),
            persisted_afk: self.approval.state().mode.is_afk(),
            plan_file_path: plan_file_path.as_deref(),
            pending_plan_activation: self.consume_pending_plan_activation_injection(),
        };
        let mut injections = Vec::new();
        for provider in &mut self.injection_providers {
            match provider.get_injections(self.context.history(), &ctx).await {
                result => injections.extend(result),
            }
        }
        injections
    }

    async fn notify_injection_providers_compacted(&mut self) {
        for provider in &mut self.injection_providers {
            provider.on_context_compacted().await;
        }
    }

    pub async fn notify_afk_changed(&mut self, enabled: bool) {
        for provider in &mut self.injection_providers {
            provider.on_afk_changed(enabled).await;
        }
    }

    // ========================================================================
    // Plan Mode
    // ========================================================================

    pub fn get_plan_file_path(&self) -> Option<PathBuf> {
        self.plan_session_id.as_ref().map(|id| {
            // TODO: use hero-name slug system like Python (heroes.py).
            // For now, use a deterministic path based on session id.
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".kimi")
                .join("plans")
                .join(format!("{id}.md"))
        })
    }

    pub fn consume_pending_plan_activation_injection(&mut self) -> bool {
        if !self.plan_mode || !self.pending_plan_activation_injection {
            return false;
        }
        self.pending_plan_activation_injection = false;
        true
    }

    pub fn set_plan_mode(&mut self, enabled: bool) {
        self.plan_mode = enabled;
        self.session.state.plan_mode = enabled;
        if enabled {
            if self.plan_session_id.is_none() {
                self.plan_session_id = Some(uuid::Uuid::new_v4().to_string());
                self.session.state.plan_session_id = self.plan_session_id.clone();
            }
            self.pending_plan_activation_injection = true;
        } else {
            self.pending_plan_activation_injection = false;
            self.plan_session_id = None;
            self.session.state.plan_session_id = None;
            self.session.state.plan_slug = None;
        }
        let _ = self.session.save_state();
    }

    pub fn toggle_plan_mode(&mut self) -> bool {
        let new_state = !self.plan_mode;
        self.set_plan_mode(new_state);
        new_state
    }

    pub fn read_current_plan(&self) -> Option<String> {
        let path = self.get_plan_file_path()?;
        std::fs::read_to_string(&path).ok()
    }

    pub fn clear_current_plan(&self) {
        if let Some(path) = self.get_plan_file_path() {
            let _ = std::fs::remove_file(&path);
        }
    }

    pub fn status_snapshot(&self) -> StatusSnapshot {
        let token_count = self.context.token_count();
        let max_size = self.llm.as_ref().map(|l| l.max_context_size).unwrap_or(0);
        StatusSnapshot {
            context_usage: if max_size > 0 {
                token_count as f64 / max_size as f64
            } else {
                0.0
            },
            yolo_enabled: self.approval.is_yolo(),
            afk_enabled: self.approval.state().mode.is_afk(),
            plan_mode: self.plan_mode,
            context_tokens: token_count,
            max_context_tokens: max_size,
            mcp_status: None,
        }
    }

    fn context_usage(&self) -> f64 {
        let max_size = self.llm.as_ref().map(|l| l.max_context_size).unwrap_or(0);
        if max_size > 0 {
            self.context.token_count() as f64 / max_size as f64
        } else {
            0.0
        }
    }

    pub fn list_slash_commands(&self) -> Vec<(String, String, Vec<String>)> {
        self.slash_registry
            .list_commands()
            .into_iter()
            .map(|cmd| {
                (
                    cmd.name.clone(),
                    cmd.description.clone(),
                    cmd.aliases.clone(),
                )
            })
            .collect()
    }
}

pub struct StepOutcome {
    pub stop_reason: String,
    pub assistant_message: Message,
}

pub struct TurnOutcome {
    pub final_message: Option<Message>,
}

pub struct StatusSnapshot {
    pub context_usage: f64,
    pub yolo_enabled: bool,
    pub afk_enabled: bool,
    pub plan_mode: bool,
    pub context_tokens: usize,
    pub max_context_tokens: usize,
    pub mcp_status: Option<crate::wire::MCPStatusSnapshot>,
}
