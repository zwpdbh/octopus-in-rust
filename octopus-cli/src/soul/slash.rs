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
