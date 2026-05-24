pub mod agent;
pub mod approval;
pub mod compaction;
pub mod context;
pub mod dynamic_injection;
pub mod dynamic_injections;
pub mod message;
pub mod slash;
pub mod toolset;

pub use approval::{Approval, ApprovalResult, ApprovalState};

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
    pub agent: Option<crate::soul::agent::Agent>,
    pub slash_registry: crate::soul::slash::SlashCommandRegistry,
    pub root_wire_hub: Option<RootWireHub>,
    pub hook_engine: HookEngine,
    pub notification_manager: NotificationManager,
    pub oauth: OAuthManager,
    pub denwa_renji: std::sync::Arc<std::sync::Mutex<crate::soul::agent::DenwaRenji>>,
    steer_queue: Vec<String>,
    compaction: SimpleCompaction,
    _current_step_no: usize,
    _current_turn_id: Option<String>,
    _last_tool_calls: Vec<(String, String)>,
    _injection_providers: Vec<Box<dyn DynamicInjectionProvider>>,
    _pending_plan_activation_injection: bool,
    _plan_session_id: Option<String>,
    _checkpoint_with_user_message: bool,
}

impl KimiSoul {
    pub fn new(
        config: Config,
        session: Session,
        llm: Option<LLM>,
        approval: ApprovalState,
    ) -> Self {
        let mut approval_wrapper = Approval::with_state(approval);
        let approval_runtime = ApprovalRuntime::new();
        let root_wire_hub = RootWireHub::new();
        approval_runtime.bind_root_wire_hub(&root_wire_hub);
        approval_wrapper.set_runtime(approval_runtime);
        let context_file = session.context_file.clone();
        let mut context = Context::new(context_file);
        let _ = context.restore_sync();

        let denwa_renji =
            std::sync::Arc::new(std::sync::Mutex::new(crate::soul::agent::DenwaRenji::new()));

        let mut toolset = KimiToolset::new();
        toolset.register(Box::new(crate::tools::shell::ShellTool::new()));
        toolset.register(Box::new(crate::tools::file::ReadFileTool::new()));
        toolset.register(Box::new(crate::tools::file::WriteFileTool::new()));
        toolset.register(Box::new(crate::tools::file::StrReplaceFileTool::new()));
        toolset.register(Box::new(crate::tools::file::GlobTool::new()));
        toolset.register(Box::new(crate::tools::file::GrepTool::new()));
        toolset.register(Box::new(crate::tools::web::SearchWebTool::new()));
        toolset.register(Box::new(crate::tools::web::FetchURLTool::new()));
        toolset.register(Box::new(crate::tools::ask_user::AskUserTool::new()));
        toolset.register(Box::new(crate::tools::todo::SetTodoListTool::new()));
        toolset.register(Box::new(crate::tools::think::ThinkTool::new()));
        toolset.register(Box::new(crate::tools::plan::EnterPlanModeTool::new()));
        toolset.register(Box::new(crate::tools::plan::ExitPlanModeTool::new()));
        toolset.register(Box::new(crate::tools::agent::AgentTool::new()));
        toolset.register(Box::new(crate::tools::background::TaskOutputTool::new()));
        toolset.register(Box::new(crate::tools::background::TaskStopTool::new()));
        toolset.register(Box::new(crate::tools::dmail::SendDMailTool::new(
            denwa_renji.clone(),
        )));
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
        toolset.set_approval(Some(approval_wrapper.share()));
        let toolset = std::sync::Arc::new(toolset);

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
            approval: approval_wrapper,
            plan_mode,
            max_steps_per_turn: max_steps,
            max_retries_per_step: max_retries,
            agent: None,
            slash_registry: build_default_slash_commands(),
            root_wire_hub: Some(root_wire_hub),
            hook_engine,
            notification_manager,
            oauth: OAuthManager::new(),
            denwa_renji,
            steer_queue: Vec::new(),
            compaction: SimpleCompaction::default(),
            _current_step_no: 0,
            _current_turn_id: None,
            _last_tool_calls: Vec::new(),
            _injection_providers: vec![
                Box::new(
                    crate::soul::dynamic_injections::plan_mode::PlanModeInjectionProvider::new(),
                ),
                Box::new(
                    crate::soul::dynamic_injections::afk_mode::AfkModeInjectionProvider::new(),
                ),
            ],
            _pending_plan_activation_injection: false,
            _plan_session_id: plan_session_id,
            _checkpoint_with_user_message: checkpoint_with_user_message,
        }
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
        crate::wire::set_current_wire_soul_side(Some(soul_side.clone()));

        // Start notification pump — delivers pending notifications to wire.
        let pump_handle = self._start_notification_pump(soul_side.clone());

        let result = self._run_turn(text).await;

        // Cleanup
        if let Some(handle) = pump_handle {
            handle.abort();
        }
        crate::wire::set_current_wire_soul_side(None);
        // Dropping `wire` drops the broadcast senders, which causes the
        // recorder task to exit cleanly after flushing.
        drop(wire);

        result
    }

    async fn _run_turn(&mut self, text: &str) -> Result<String> {
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

        // Track turn lifecycle for interruption telemetry and approval cleanup.
        let created_approval_source = Some(ApprovalSource {
            kind: "foreground_turn".to_string(),
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: None,
        });

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
                    wire_send(crate::wire::TextPart {
                        text: format!("UserPromptSubmit hook blocked: {}", reason),
                    });
                    return Ok(String::new());
                }
            }
        }

        wire_send(TurnBegin {
            user_input: Some(text.to_string()),
        });

        let turn_result = self._turn(user_message).await;

        // TurnEnd is sent regardless of success or failure.
        wire_send(TurnEnd {});
        if turn_result.is_err() {
            tracing::warn!("Turn interrupted at step {}", self._current_step_no);
            crate::track!(
                "turn_interrupted",
                step_no = self._current_step_no,
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

        // Cancel any pending approval requests tied to this turn's approval source.
        if let Some(ref source) = created_approval_source {
            self.approval
                .runtime()
                .cancel_by_source(&source.kind, &source.id);
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

    fn _start_notification_pump(
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
                        soul_side.send(serde_json::to_value(&notification).unwrap_or_default());
                        std::future::ready(())
                    })
                    .await;
            }
        }))
    }

    async fn _turn(&mut self, user_message: Message) -> Result<TurnOutcome> {
        if self.llm.is_none() {
            return Err(LLMNotSet::NotSet.into());
        }

        if let Some(ref llm) = self.llm {
            let missing = check_message(&user_message, &llm.capabilities);
            if !missing.is_empty() {
                return Err(LLMNotSupported::NotSupported(format!("{:?}", missing)).into());
            }
        }

        self._current_turn_id = Some(uuid::Uuid::new_v4().to_string());
        self._last_tool_calls = Vec::new();

        self.context
            .checkpoint(false)
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .append_message(user_message)
            .await
            .map_err(|e| OctopusError::Io(e))?;

        self._agent_loop().await
    }

    async fn _agent_loop(&mut self) -> Result<TurnOutcome> {
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
                    wire_send(crate::wire::StatusUpdate {
                        mcp_status: Some(snapshot),
                        ..Default::default()
                    });
                    wire_send(crate::wire::MCPLoadingBegin {});
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
                wire_send(crate::wire::StatusUpdate {
                    mcp_status: Some(mcp_snap),
                    ..Default::default()
                });
                wire_send(crate::wire::MCPLoadingEnd {});
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
                .checkpoint(self._checkpoint_with_user_message)
                .await
                .map_err(|e| OctopusError::Io(e))?;
            self.denwa_renji
                .lock()
                .unwrap()
                .set_n_checkpoints(self.context.n_checkpoints());

            self._current_step_no = step_no;
            wire_send(StepBegin { n: step_no });

            let step_result = match self._step().await {
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
                    self._last_tool_calls = Vec::new();
                    self.context
                        .checkpoint(self._checkpoint_with_user_message)
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
                    wire_send(StepInterrupted {});
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
            let has_steers = self._consume_pending_steers().await;
            if has_steers {
                continue;
            }

            if let Some(outcome) = step_result {
                let stop_reason = outcome.stop_reason.clone();
                return Ok(TurnOutcome {
                    stop_reason: outcome.stop_reason,
                    final_message: if stop_reason == "no_tool_calls" {
                        Some(outcome.assistant_message)
                    } else {
                        None
                    },
                    step_count: step_no,
                });
            }
        }
    }

    async fn _step(&mut self) -> Result<Option<StepOutcome>> {
        let max_attempts = self.max_retries_per_step;
        let mut last_error: Option<OctopusError> = None;

        for attempt in 1..=max_attempts {
            match self._run_step_once().await {
                Ok(outcome) => return Ok(outcome),
                Err(e) if Self::_is_retryable_error(&e) && attempt < max_attempts => {
                    let wait_s = Self::_retry_wait_secs(attempt);
                    self._emit_step_retry(attempt, max_attempts, wait_s, &e);
                    tracing::warn!(
                        "Retrying step {} for the {} time (last error: {:?}). Waiting {:.1}s.",
                        self._current_step_no,
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
    async fn _run_step_once(&mut self) -> Result<Option<StepOutcome>> {
        let mut auth_retried = false;
        let mut connection_retried = false;

        loop {
            let llm = self
                .llm
                .clone()
                .ok_or_else(|| OctopusError::Other("LLM not set".to_string()))?;
            let result = self._run_step_once_inner(&llm).await;
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
                        self._current_step_no
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
                        self._current_step_no,
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
                        self._current_step_no,
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
    async fn _run_step_once_inner(&mut self, llm: &LLM) -> Result<Option<StepOutcome>> {
        // ── Dynamic Injection ───────────────────────────────────────────────
        // Mirrors Python's `_collect_injections()` in `kimisoul.py`.
        let injections = self._collect_injections().await;
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
        // Mirrors Python's notification delivery in `_step()`.
        // Deliver pending notifications to LLM context before the LLM call.
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
        // Mirrors Python's `normalize_history()` in `dynamic_injection.py`.
        let history = normalize_history(self.context.history());

        // Call LLM with streaming + early tool dispatch
        let tools_slice: Vec<&dyn crate::tools::Tool> = self.toolset.tools();
        let system_prompt = self.agent.as_ref().map(|a| a.system_prompt.as_str());

        let t0 = std::time::Instant::now();

        let mut on_message_part = |part: kosong::StreamedMessagePart| {
            use kosong::chat_provider::Part;
            match part {
                Part::Content(cp) => {
                    let wire_cp = crate::llm::kosong_to_wire_content_part(cp);
                    wire_send(wire_cp);
                }
                Part::ToolCall(_) | Part::ToolCallPart(_) => {}
            }
        };

        // Spawn tool execution tasks as soon as each tool call is assembled
        // from the stream, so tools start running before the full message
        // has been received.
        let toolset = self.toolset.clone();
        let tool_handles: std::sync::Arc<
            std::sync::Mutex<Vec<tokio::task::JoinHandle<crate::wire::ToolResult>>>,
        > = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let handles = tool_handles.clone();

        let mut on_tool_call = move |tc: kosong::ToolCall| {
            let toolset = toolset.clone();
            let wire_tc = crate::llm::kosong_to_wire_tool_call(tc);
            let handle = tokio::spawn(async move { toolset.handle(&wire_tc).await });
            handles.lock().unwrap().push(handle);
        };

        let completion_result = llm
            .generate_streaming(
                system_prompt,
                &history,
                Some(&tools_slice),
                &mut on_message_part,
                &mut on_tool_call,
            )
            .await;
        let elapsed = t0.elapsed();

        let completion = match completion_result {
            Ok(c) => c,
            Err(e) => {
                let (error_type, status_code) = Self::_classify_api_error(&e);
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
                return Err(e);
            }
        };

        let assistant_message = completion.message;
        let usage = completion.usage;

        // Update token count
        if let Some(ref u) = usage {
            self.context
                .update_token_count(u.input)
                .await
                .map_err(|e| OctopusError::Io(e))?;
        }

        let status_update = StatusUpdate {
            token_usage: usage.clone(),
            message_id: completion.id,
            plan_mode: Some(self.plan_mode),
            context_usage: Some(self._context_usage()),
            context_tokens: Some(self.context.token_count()),
            max_context_tokens: self.llm.as_ref().map(|l| l.max_context_size),
            ..Default::default()
        };
        wire_send(status_update);

        // Execute tool calls
        if !completion.tool_calls.is_empty() {
            let turn_id = self._current_turn_id.clone().unwrap_or_default();
            self.toolset.begin_step(
                self._last_tool_calls.clone(),
                self._current_step_no,
                turn_id,
            );

            // Set up streaming callback so each tool result is sent to wire
            // as soon as it completes, rather than waiting for all tools.
            self.toolset.set_on_tool_result(Some(Box::new(|result| {
                crate::wire::wire_send(result.clone());
            })));

            // Await tool handles that were spawned during streaming.
            let handles = std::sync::Arc::try_unwrap(tool_handles)
                .unwrap()
                .into_inner()
                .unwrap();
            let tool_results: Vec<crate::wire::ToolResult> = futures::future::join_all(handles)
                .await
                .into_iter()
                .map(|r| match r {
                    Ok(result) => result,
                    Err(e) => crate::wire::ToolResult {
                        tool_call_id: String::new(),
                        return_value: crate::wire::ToolReturnValue::error(
                            format!("Tool task panicked: {e}"),
                            "Tool panic".to_string(),
                            None,
                        ),
                    },
                })
                .collect();

            self.toolset.set_on_tool_result(None);
            self._last_tool_calls = self.toolset.end_step();

            // Grow context
            self._grow_context(&assistant_message, &tool_results)
                .await?;

            // Check for rejections
            let rejected: Vec<_> = tool_results
                .iter()
                .filter(|r| r.return_value.is_error())
                .collect();

            if !rejected.is_empty() {
                // Clear any pending D-Mail on rejection.
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

            // Continue loop
            Ok(None)
        } else {
            // No tool calls - stop
            Ok(Some(StepOutcome {
                stop_reason: "no_tool_calls".to_string(),
                assistant_message,
            }))
        }
    }

    fn _is_retryable_error(error: &OctopusError) -> bool {
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

    fn _retry_wait_secs(attempt: usize) -> f64 {
        // Exponential backoff with jitter: initial=0.3, max=5, jitter=0.5
        let base = 0.3_f64 * 2_f64.powi((attempt - 1) as i32);
        let capped = base.min(5.0);
        let jitter = rand::random::<f64>() * 0.5;
        capped + jitter
    }

    fn _emit_step_retry(
        &self,
        attempt: usize,
        max_attempts: usize,
        wait_s: f64,
        error: &OctopusError,
    ) {
        let (error_type, status_code) = Self::_classify_api_error(error);
        wire_send(StepRetry {
            n: self._current_step_no,
            next_attempt: attempt + 1,
            max_attempts,
            wait_s,
            error_type,
            status_code,
        });
    }

    fn _classify_api_error(error: &OctopusError) -> (String, Option<u16>) {
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

    async fn _grow_context(
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

    async fn _consume_pending_steers(&mut self) -> bool {
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
            wire_send(SteerInput {
                user_input: content,
            });
            consumed = true;
        }
        consumed
    }

    pub fn steer(&mut self, content: &str) {
        self.steer_queue.push(content.to_string());
    }

    /// Sync the in-memory approval state to the session and persist it.
    fn _sync_approval_state(&mut self) {
        self.session.state.approval.yolo = self.approval.yolo();
        self.session.state.approval.afk = self.approval.afk();
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

        wire_send(CompactionBegin {});

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
        if let Some(ref agent) = self.agent {
            self.context
                .write_system_prompt(&agent.system_prompt)
                .await
                .map_err(|e| OctopusError::Io(e))?;
        }
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

        wire_send(CompactionEnd {});

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
        self._notify_injection_providers_compacted().await;

        Ok(())
    }

    // ========================================================================
    // Dynamic Injection
    // ========================================================================

    async fn _collect_injections(
        &mut self,
    ) -> Vec<crate::soul::dynamic_injection::DynamicInjection> {
        let plan_file_path = self.get_plan_file_path();
        let ctx = InjectionContext {
            plan_mode: self.plan_mode,
            is_afk: self.approval.is_afk(),
            is_afk_flag: self.approval.is_afk_flag(),
            plan_file_path: plan_file_path.as_deref(),
            pending_plan_activation: self.consume_pending_plan_activation_injection(),
        };
        let mut injections = Vec::new();
        for provider in &mut self._injection_providers {
            match provider.get_injections(self.context.history(), &ctx).await {
                result => injections.extend(result),
            }
        }
        injections
    }

    async fn _notify_injection_providers_compacted(&mut self) {
        for provider in &mut self._injection_providers {
            provider.on_context_compacted().await;
        }
    }

    pub async fn notify_afk_changed(&mut self, enabled: bool) {
        for provider in &mut self._injection_providers {
            provider.on_afk_changed(enabled).await;
        }
    }

    // ========================================================================
    // Plan Mode
    // ========================================================================

    pub fn get_plan_file_path(&self) -> Option<PathBuf> {
        self._plan_session_id.as_ref().map(|id| {
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
        if !self.plan_mode || !self._pending_plan_activation_injection {
            return false;
        }
        self._pending_plan_activation_injection = false;
        true
    }

    pub fn set_plan_mode(&mut self, enabled: bool) {
        self.plan_mode = enabled;
        self.session.state.plan_mode = enabled;
        if enabled {
            if self._plan_session_id.is_none() {
                self._plan_session_id = Some(uuid::Uuid::new_v4().to_string());
                self.session.state.plan_session_id = self._plan_session_id.clone();
            }
            self._pending_plan_activation_injection = true;
        } else {
            self._pending_plan_activation_injection = false;
            self._plan_session_id = None;
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
            yolo_enabled: self.approval.yolo(),
            afk_enabled: self.approval.afk(),
            plan_mode: self.plan_mode,
            context_tokens: token_count,
            max_context_tokens: max_size,
            mcp_status: None,
        }
    }

    fn _context_usage(&self) -> f64 {
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
    pub stop_reason: String,
    pub final_message: Option<Message>,
    pub step_count: usize,
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
