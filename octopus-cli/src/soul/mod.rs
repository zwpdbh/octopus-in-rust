pub mod agent;
pub mod approval;
pub mod compaction;
pub mod context;
pub mod message;
pub mod slash;
pub mod toolset;

pub use approval::{Approval, ApprovalResult, ApprovalState};

use std::path::PathBuf;

use crate::config::Config;
use crate::exception::{LLMNotSet, LLMNotSupported, MaxStepsReached, OctopusError, Result};
use crate::llm::LLM;
use crate::session::Session;
use crate::soul::compaction::{SimpleCompaction, should_auto_compact};
use crate::soul::context::Context;
use crate::soul::message::{check_message, tool_result_to_message};
use crate::soul::slash::{build_default_slash_commands, parse_slash_command_call};
use crate::soul::toolset::KimiToolset;
use crate::wire::{
    CompactionBegin, CompactionEnd, ContentPart, Message, StatusUpdate, SteerInput, StepBegin,
    StepInterrupted, StepRetry, TurnBegin, TurnEnd, wire_send,
};

pub struct KimiSoul {
    pub config: Config,
    pub session: Session,
    pub llm: Option<LLM>,
    pub context: Context,
    pub toolset: KimiToolset,
    pub approval: ApprovalState,
    pub plan_mode: bool,
    pub max_steps_per_turn: usize,
    pub max_retries_per_step: usize,
    pub agent: Option<crate::soul::agent::Agent>,
    pub slash_registry: crate::soul::slash::SlashCommandRegistry,
    steer_queue: Vec<String>,
    compaction: SimpleCompaction,
    _current_step_no: usize,
}

impl KimiSoul {
    pub fn new(
        config: Config,
        session: Session,
        llm: Option<LLM>,
        approval: ApprovalState,
    ) -> Self {
        let context_file = session.context_file.clone();
        let mut context = Context::new(context_file);
        let _ = context.restore_sync();

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

        let max_steps = config.loop_control.max_steps_per_turn;
        let max_retries = config.loop_control.max_retries_per_step;

        Self {
            config,
            session,
            llm,
            context,
            toolset,
            approval,
            plan_mode: false,
            max_steps_per_turn: max_steps,
            max_retries_per_step: max_retries,
            agent: None,
            slash_registry: build_default_slash_commands(),
            steer_queue: Vec::new(),
            compaction: SimpleCompaction::default(),
            _current_step_no: 0,
        }
    }

    pub async fn run(&mut self, user_input: &str) -> Result<String> {
        let text = user_input.trim();
        if text.is_empty() {
            return Ok(String::new());
        }

        let user_message = Message {
            role: "user".to_string(),
            content: vec![ContentPart::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            tool_calls: None,
        };

        if let Some(ref llm) = self.llm {
            let missing = check_message(&user_message, &llm.capabilities);
            if !missing.is_empty() {
                return Err(LLMNotSupported::NotSupported(format!("{:?}", missing)).into());
            }
        }

        // Slash command handling
        // Mirrors Python's `soul.run()` slash-command branch in `kimi_cli/soul/__init__.py`.
        // Python checks `parse_slash_command_call(user_input)` and then calls the
        // registered function with `(soul, args)`. Rust does the same via the registry.
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

        wire_send(TurnBegin {
            user_input: Some(text.to_string()),
        });

        let turn_result = self._turn(user_message).await;

        wire_send(TurnEnd {});

        match turn_result {
            Ok(outcome) => {
                if let Some(msg) = outcome.final_message {
                    Ok(msg.extract_text(" "))
                } else {
                    Ok(String::new())
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn _turn(&mut self, user_message: Message) -> Result<TurnOutcome> {
        if self.llm.is_none() {
            return Err(LLMNotSet::NotSet.into());
        }

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
                .checkpoint(false)
                .await
                .map_err(|e| OctopusError::Io(e))?;

            self._current_step_no = step_no;
            wire_send(StepBegin { n: step_no });

            let step_result = match self._step().await {
                Ok(result) => result,
                Err(e) => {
                    wire_send(StepInterrupted {});
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
        let llm = self.llm.clone().unwrap();
        let max_attempts = self.max_retries_per_step;
        let mut last_error: Option<OctopusError> = None;

        for attempt in 1..=max_attempts {
            match self._run_step_once(&llm).await {
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

    /// Execute a single LLM step without retry logic.
    async fn _run_step_once(&mut self, llm: &LLM) -> Result<Option<StepOutcome>> {
        // Build effective history
        let history = self.context.history().to_vec();

        // Call LLM
        let tools_slice: Vec<&dyn crate::tools::Tool> = self.toolset.tools();
        let system_prompt = self.agent.as_ref().map(|a| a.system_prompt.as_str());

        let t0 = std::time::Instant::now();
        let completion = llm
            .complete(system_prompt, &history, Some(&tools_slice))
            .await?;
        let _elapsed = t0.elapsed();

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
            let mut tool_results = Vec::new();
            for call in &completion.tool_calls {
                let result = self.toolset.handle(call).await;
                tool_results.push(result);
            }

            // Grow context
            self._grow_context(&assistant_message, &tool_results)
                .await?;

            // Check for rejections
            let rejected: Vec<_> = tool_results
                .iter()
                .filter(|r| r.return_value.is_error())
                .collect();

            if !rejected.is_empty() {
                return Ok(Some(StepOutcome {
                    stop_reason: "tool_rejected".to_string(),
                    assistant_message,
                }));
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
        use crate::exception::{APIConnectionError, APIEmptyResponseError, APIStatusError, APITimeoutError};
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

    fn _emit_step_retry(&self, attempt: usize, max_attempts: usize, wait_s: f64, error: &OctopusError) {
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
        use crate::exception::{APIConnectionError, APIEmptyResponseError, APIStatusError, APITimeoutError};
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

    pub fn toggle_plan_mode(&mut self) -> bool {
        self.plan_mode = !self.plan_mode;
        self.session.state.plan_mode = self.plan_mode;
        let _ = crate::session_state::save_session_state(&self.session.state, &self.session.dir());
        self.plan_mode
    }

    pub fn get_plan_file_path(&self) -> Option<PathBuf> {
        if self.plan_mode {
            Some(self.session.dir().join("plan.md"))
        } else {
            None
        }
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

    pub async fn compact_context(&mut self, custom_instruction: &str) -> Result<()> {
        let llm = self.llm.as_ref().ok_or(LLMNotSet::NotSet)?;
        let messages = self.context.history().to_vec();

        wire_send(CompactionBegin {});

        let result = self
            .compaction
            .compact(&messages, llm, custom_instruction)
            .await;

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
        let estimated = result.estimated_token_count();
        self.context
            .append_message(result.messages)
            .await
            .map_err(|e| OctopusError::Io(e))?;
        self.context
            .update_token_count(estimated)
            .await
            .map_err(|e| OctopusError::Io(e))?;

        wire_send(CompactionEnd {});

        Ok(())
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
            yolo_enabled: self.approval.yolo,
            afk_enabled: self.approval.afk,
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
