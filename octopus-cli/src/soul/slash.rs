use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::soul::KimiSoul;
use crate::wire::{StatusUpdate, TextPart};

pub type SoulSlashCmdFunc = Arc<
    dyn for<'a> Fn(&'a mut KimiSoul, &'a str) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>
        + Send
        + Sync,
>;

pub struct SlashCommand {
    pub name: String,
    pub func: SoulSlashCmdFunc,
    pub description: String,
    pub aliases: Vec<String>,
}

pub struct SlashCommandRegistry {
    commands: HashMap<String, Arc<SlashCommand>>,
}

impl SlashCommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    pub fn register(&mut self, command: SlashCommand) {
        let arc_cmd = Arc::new(command);
        self.commands.insert(arc_cmd.name.clone(), arc_cmd.clone());
        for alias in &arc_cmd.aliases {
            self.commands.insert(alias.clone(), arc_cmd.clone());
        }
    }

    pub fn get(&self, name: &str) -> Option<Arc<SlashCommand>> {
        self.commands.get(name).cloned()
    }

    pub fn list_commands(&self) -> Vec<Arc<SlashCommand>> {
        let mut seen = HashMap::new();
        for cmd in self.commands.values() {
            seen.entry(&cmd.name).or_insert_with(|| cmd.clone());
        }
        seen.into_values().collect()
    }
}

impl Default for SlashCommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn build_default_slash_commands() -> SlashCommandRegistry {
    let mut registry = SlashCommandRegistry::new();

    registry.register(SlashCommand {
        name: "clear".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                if let Err(e) = soul.context.clear().await {
                    crate::wire::wire_send(TextPart {
                        text: format!("Failed to clear context: {e}"),
                    });
                    return;
                }
                if let Some(prompt) = soul.agent.as_ref().map(|a| a.system_prompt.clone()) {
                    let _ = soul.context.write_system_prompt(&prompt).await;
                }
                crate::wire::wire_send(TextPart {
                    text: "The context has been cleared.".to_string(),
                });
                let snap = soul.status_snapshot();
                crate::wire::wire_send(StatusUpdate {
                    context_usage: Some(snap.context_usage),
                    context_tokens: Some(snap.context_tokens),
                    max_context_tokens: Some(snap.max_context_tokens),
                    ..Default::default()
                });
            })
        }),
        description: "Clear the context".to_string(),
        aliases: vec!["reset".to_string()],
    });

    registry.register(SlashCommand {
        name: "yolo".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                if soul.approval.yolo {
                    soul.approval.yolo = false;
                    if soul.approval.afk {
                        crate::wire::wire_send(TextPart {
                            text: "Yolo disabled, but afk is still on — tool calls remain auto-approved. Use /afk to turn off afk.".to_string(),
                        });
                    } else {
                        crate::wire::wire_send(TextPart {
                            text: "You only die once! Actions will require approval.".to_string(),
                        });
                    }
                } else {
                    soul.approval.yolo = true;
                    crate::wire::wire_send(TextPart {
                        text: "You only live once! All actions will be auto-approved.".to_string(),
                    });
                }
            })
        }),
        description: "Toggle YOLO mode (auto-approve all actions)".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "afk".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                if soul.approval.afk {
                    soul.approval.afk = false;
                    let msg = if soul.approval.yolo {
                        "afk mode disabled. You are back at the terminal. Yolo is still on."
                    } else {
                        "afk mode disabled. You are back at the terminal."
                    };
                    crate::wire::wire_send(TextPart { text: msg.to_string() });
                } else {
                    soul.approval.afk = true;
                    crate::wire::wire_send(TextPart {
                        text: "afk mode enabled. AskUserQuestion will be auto-dismissed and tool calls auto-approved.".to_string(),
                    });
                }
            })
        }),
        description: "Toggle afk mode (auto-dismiss AskUserQuestion, auto-approve tool calls)"
            .to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "plan".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, args: &str| {
            Box::pin(async move {
                let subcmd = args.trim().to_lowercase();
                match subcmd.as_str() {
                    "on" => {
                        if !soul.plan_mode {
                            soul.toggle_plan_mode();
                        }
                        crate::wire::wire_send(TextPart {
                            text: format!("Plan mode ON. Plan file: {:?}", soul.get_plan_file_path()),
                        });
                        crate::wire::wire_send(StatusUpdate {
                            plan_mode: Some(soul.plan_mode),
                            ..Default::default()
                        });
                    }
                    "off" => {
                        if soul.plan_mode {
                            soul.toggle_plan_mode();
                        }
                        crate::wire::wire_send(TextPart {
                            text: "Plan mode OFF. All tools are now available.".to_string(),
                        });
                        crate::wire::wire_send(StatusUpdate {
                            plan_mode: Some(soul.plan_mode),
                            ..Default::default()
                        });
                    }
                    "view" => {
                        let content = soul.read_current_plan().unwrap_or_default();
                        if content.is_empty() {
                            crate::wire::wire_send(TextPart {
                                text: "No plan file found for this session.".to_string(),
                            });
                        } else {
                            crate::wire::wire_send(TextPart { text: content });
                        }
                    }
                    "clear" => {
                        soul.clear_current_plan();
                        crate::wire::wire_send(TextPart {
                            text: "Plan cleared.".to_string(),
                        });
                    }
                    _ => {
                        let new_state = soul.toggle_plan_mode();
                        if new_state {
                            crate::wire::wire_send(TextPart {
                                text: format!(
                                    "Plan mode ON. Write your plan to: {:?}\nUse ExitPlanMode when done, or /plan off to exit manually.",
                                    soul.get_plan_file_path()
                                ),
                            });
                        } else {
                            crate::wire::wire_send(TextPart {
                                text: "Plan mode OFF. All tools are now available.".to_string(),
                            });
                        }
                        crate::wire::wire_send(StatusUpdate {
                            plan_mode: Some(soul.plan_mode),
                            ..Default::default()
                        });
                    }
                }
            })
        }),
        description: "Toggle plan mode. Usage: /plan [on|off|view|clear]".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "compact".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, args: &str| {
            Box::pin(async move {
                if soul.context.n_checkpoints() == 0 {
                    crate::wire::wire_send(TextPart {
                        text: "The context is empty.".to_string(),
                    });
                    return;
                }
                if let Err(e) = soul.compact_context(args.trim()).await {
                    crate::wire::wire_send(TextPart {
                        text: format!("Compaction failed: {e}"),
                    });
                    return;
                }
                crate::wire::wire_send(TextPart {
                    text: "The context has been compacted.".to_string(),
                });
                let snap = soul.status_snapshot();
                crate::wire::wire_send(StatusUpdate {
                    context_usage: Some(snap.context_usage),
                    context_tokens: Some(snap.context_tokens),
                    max_context_tokens: Some(snap.max_context_tokens),
                    ..Default::default()
                });
            })
        }),
        description: "Compact the context (optionally with a custom focus, e.g. /compact keep db discussions)".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "help".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                let mut lines = vec![
                    "Available slash commands:".to_string(),
                    String::new(),
                ];
                for cmd in soul.slash_registry.list_commands() {
                    lines.push(format!("  /{} - {}", cmd.name, cmd.description));
                    for alias in &cmd.aliases {
                        lines.push(format!("  /{} - alias for /{}", alias, cmd.name));
                    }
                }
                crate::wire::wire_send(TextPart {
                    text: lines.join("\n"),
                });
            })
        }),
        description: "Show help information".to_string(),
        aliases: vec!["h".to_string(), "?".to_string()],
    });

    registry.register(SlashCommand {
        name: "changelog".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Release notes are not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Show release notes".to_string(),
        aliases: vec!["release-notes".to_string()],
    });

    registry.register(SlashCommand {
        name: "debug".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                let snap = soul.status_snapshot();
                let lines = vec![
                    "Debug context:".to_string(),
                    String::new(),
                    format!("  Session ID:     {}", soul.session.id),
                    format!("  Model:          {}", soul.llm.as_ref().map(|l| l.model_name.clone()).unwrap_or_else(|| "none".to_string())),
                    format!("  Plan mode:      {}", soul.plan_mode),
                    format!("  YOLO:           {}", soul.approval.yolo),
                    format!("  AFK:            {}", soul.approval.afk),
                    format!("  Context tokens: {} / {} ({:.1}%)",
                        snap.context_tokens,
                        snap.max_context_tokens,
                        snap.context_usage * 100.0
                    ),
                    format!("  Checkpoints:    {}", soul.context.n_checkpoints()),
                ];
                crate::wire::wire_send(TextPart {
                    text: lines.join("\n"),
                });
            })
        }),
        description: "Debug the context".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "add-dir".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, args: &str| {
            Box::pin(async move {
                let path = args.trim();
                if path.is_empty() {
                    let dirs = &soul.config.workspace_dirs;
                    if dirs.is_empty() {
                        crate::wire::wire_send(TextPart {
                            text: "No additional directories in the workspace.\nUsage: /add-dir <path>".to_string(),
                        });
                    } else {
                        let mut lines = vec!["Added directories:".to_string()];
                        for d in dirs {
                            lines.push(format!("  - {}", d.display()));
                        }
                        crate::wire::wire_send(TextPart {
                            text: lines.join("\n"),
                        });
                    }
                    return;
                }
                let p = std::path::PathBuf::from(path);
                if !p.exists() {
                    crate::wire::wire_send(TextPart {
                        text: format!("Path does not exist: {}", path),
                    });
                    return;
                }
                if !p.is_dir() {
                    crate::wire::wire_send(TextPart {
                        text: format!("Path is not a directory: {}", path),
                    });
                    return;
                }
                let canonical = p.canonicalize().unwrap_or(p);
                if !soul.config.workspace_dirs.contains(&canonical) {
                    soul.config.workspace_dirs.push(canonical.clone());
                }
                crate::wire::wire_send(TextPart {
                    text: format!("Added directory to workspace: {}", canonical.display()),
                });
            })
        }),
        description: "Add a directory to the workspace. Usage: /add-dir <path>. Run without args to list added dirs".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "exit".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Use Ctrl-D or type 'exit' to quit.".to_string(),
                });
            })
        }),
        description: "Exit the CLI".to_string(),
        aliases: vec!["quit".to_string()],
    });

    registry.register(SlashCommand {
        name: "version".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: format!("kimi, version {}", crate::constant::get_version()),
                });
            })
        }),
        description: "Show version information".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "model".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                let model = soul.llm.as_ref().map(|l| l.model_name.clone()).unwrap_or_else(|| "no model".to_string());
                crate::wire::wire_send(TextPart {
                    text: format!("Current model: {}", model),
                });
            })
        }),
        description: "Show or switch the current model".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "feedback".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Spot a bug or have feedback? Visit https://github.com/MoonshotAI/kimi-cli/issues".to_string(),
                });
            })
        }),
        description: "Submit feedback to make Kimi Code CLI better".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "new".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                if soul.session.is_empty() {
                    let _ = soul.session.delete().await;
                }
                let work_dir = soul.session.work_dir.clone();
                match crate::session::Session::create(&work_dir, None).await {
                    Ok(new_session) => {
                        let id = new_session.id.clone();
                        crate::wire::wire_send(TextPart {
                            text: format!("New session created: {}. Restart octopus to switch to it.", id),
                        });
                    }
                    Err(e) => {
                        crate::wire::wire_send(TextPart {
                            text: format!("Failed to create new session: {}", e),
                        });
                    }
                }
            })
        }),
        description: "Start a new session".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "title".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, args: &str| {
            Box::pin(async move {
                let new_title = args.trim();
                if new_title.is_empty() {
                    crate::wire::wire_send(TextPart {
                        text: format!("Session title: {}", soul.session.title),
                    });
                    return;
                }
                let trimmed = new_title.chars().take(200).collect::<String>();
                let session_dir = soul.session.dir();
                let mut fresh = crate::session_state::load_session_state(&session_dir);
                fresh.custom_title = Some(trimmed.clone());
                fresh.title_generated = true;
                crate::session_state::save_session_state(&fresh, &session_dir).ok();
                soul.session.state.custom_title = Some(trimmed.clone());
                soul.session.state.title_generated = true;
                soul.session.title = trimmed.clone();
                crate::wire::wire_send(TextPart {
                    text: format!("Session title set to: {}", trimmed),
                });
            })
        }),
        description: "Set or show the session title".to_string(),
        aliases: vec!["rename".to_string()],
    });

    registry.register(SlashCommand {
        name: "sessions".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                let work_dir = soul.session.work_dir.clone();
                let sessions = crate::session::Session::list(&work_dir).await;
                if sessions.is_empty() {
                    crate::wire::wire_send(TextPart {
                        text: "No sessions found.".to_string(),
                    });
                    return;
                }
                let mut lines = vec!["Sessions:".to_string(), String::new()];
                for s in sessions {
                    let marker = if s.id == soul.session.id { " ← current" } else { "" };
                    lines.push(format!("  {} - {}{}", &s.id[..8.min(s.id.len())], s.title, marker));
                }
                crate::wire::wire_send(TextPart {
                    text: lines.join("\n"),
                });
            })
        }),
        description: "List sessions and resume optionally".to_string(),
        aliases: vec!["resume".to_string()],
    });

    registry.register(SlashCommand {
        name: "web".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Web UI is not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Open Kimi Code Web UI in browser".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "vis".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Visualizer is not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Open Kimi Agent Tracing Visualizer in browser".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "mcp".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                if let Some(ref mcp) = soul.status_snapshot().mcp_status {
                    let mut lines = vec!["MCP Servers:".to_string(), String::new()];
                    for server in &mcp.servers {
                        lines.push(format!("  {} - {} ({} tools)", server.name, server.status, server.tools.len()));
                    }
                    crate::wire::wire_send(TextPart {
                        text: lines.join("\n"),
                    });
                } else {
                    crate::wire::wire_send(TextPart {
                        text: "No MCP servers configured.".to_string(),
                    });
                }
            })
        }),
        description: "Show MCP servers and tools".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "hooks".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Hooks are not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "List configured hooks".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "undo".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Undo is not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Undo: fork the session at a previous turn and retry".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "fork".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Fork is not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Fork the current session (copy all history to a new session)".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "btw".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Side questions (/btw) are not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Ask a side question without interrupting the main conversation".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "editor".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Editor configuration is not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Set default external editor for Ctrl-O".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "task".to_string(),
        func: Arc::new(|_soul: &mut KimiSoul, _args: &str| {
            Box::pin(async move {
                crate::wire::wire_send(TextPart {
                    text: "Background task browser is not yet implemented in octopus-cli.".to_string(),
                });
            })
        }),
        description: "Browse and manage background tasks".to_string(),
        aliases: Vec::new(),
    });

    registry.register(SlashCommand {
        name: "theme".to_string(),
        func: Arc::new(|soul: &mut KimiSoul, args: &str| {
            Box::pin(async move {
                let arg = args.trim().to_lowercase();
                if arg.is_empty() {
                    crate::wire::wire_send(TextPart {
                        text: format!("Current theme: {}. Usage: /theme dark | /theme light", soul.config.theme),
                    });
                    return;
                }
                if arg != "dark" && arg != "light" {
                    crate::wire::wire_send(TextPart {
                        text: "Unknown theme. Use 'dark' or 'light'.".to_string(),
                    });
                    return;
                }
                soul.config.theme = arg.clone();
                crate::wire::wire_send(TextPart {
                    text: format!("Theme set to: {}. Restart to apply.", arg),
                });
            })
        }),
        description: "Switch terminal color theme (dark/light)".to_string(),
        aliases: Vec::new(),
    });

    registry
}

pub fn parse_slash_command_call(text: &str) -> Option<SlashCommandCall> {
    let text = text.trim();
    if !text.starts_with('/') {
        return None;
    }
    let rest = &text[1..];
    let mut parts = rest.splitn(2, ' ');
    let name = parts.next()?.trim();
    let args = parts.next().unwrap_or("").trim();
    if name.is_empty() {
        return None;
    }
    Some(SlashCommandCall {
        name: name.to_string(),
        args: args.to_string(),
    })
}

pub struct SlashCommandCall {
    pub name: String,
    pub args: String,
}
