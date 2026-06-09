use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// An LLM CLI tool that docref knows how to integrate with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTool {
    Kimi,
    Claude,
    Codex,
    Cursor,
}

impl AgentTool {
    pub fn name(&self) -> &'static str {
        match self {
            AgentTool::Kimi => "Kimi CLI",
            AgentTool::Claude => "Claude Code",
            AgentTool::Codex => "OpenAI Codex CLI",
            AgentTool::Cursor => "Cursor",
        }
    }

    pub fn command(&self) -> &'static str {
        match self {
            AgentTool::Kimi => "kimi",
            AgentTool::Claude => "claude",
            AgentTool::Codex => "codex",
            AgentTool::Cursor => "cursor",
        }
    }

    pub fn config_path(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        Some(match self {
            AgentTool::Kimi => home.join(".kimi").join("config.toml"),
            AgentTool::Claude => home.join(".claude").join("settings.json"),
            AgentTool::Codex => home.join(".codex").join("config.toml"),
            AgentTool::Cursor => home.join(".cursor").join("settings.json"),
        })
    }

    pub fn supports_hooks(&self) -> bool {
        matches!(self, AgentTool::Kimi)
    }

    pub fn hook_snippet(&self) -> Option<String> {
        match self {
            AgentTool::Kimi => Some(
                r#"
[[hooks]]
event = "PostToolUse"
matcher = "WriteFile|StrReplaceFile"
command = "cd {cwd} && docref scan --format json >/dev/null 2>&1 && docref hook kimi"
timeout = 30
"#
                .trim_start()
                .to_string(),
            ),
            _ => None,
        }
    }
}

/// Detect which agent tools are installed and have known config files.
pub fn detect_tools() -> Vec<(AgentTool, DetectedState)> {
    let candidates = [
        AgentTool::Kimi,
        AgentTool::Claude,
        AgentTool::Codex,
        AgentTool::Cursor,
    ];

    candidates
        .iter()
        .filter_map(|tool| {
            let in_path = command_exists(tool.command());
            let config_exists = tool.config_path().map(|p| p.exists()).unwrap_or(false);

            if in_path || config_exists {
                let state = DetectedState {
                    in_path,
                    config_exists,
                    hook_already_configured: hook_already_configured(*tool),
                };
                Some((*tool, state))
            } else {
                None
            }
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct DetectedState {
    pub in_path: bool,
    pub config_exists: bool,
    pub hook_already_configured: bool,
}

fn command_exists(cmd: &str) -> bool {
    which::which(cmd).is_ok()
}

fn hook_already_configured(tool: AgentTool) -> bool {
    let Some(config_path) = tool.config_path() else {
        return false;
    };
    if !config_path.exists() {
        return false;
    }
    let Ok(content) = fs::read_to_string(&config_path) else {
        return false;
    };
    match tool {
        AgentTool::Kimi => content.contains("docref hook kimi"),
        _ => false,
    }
}

/// Print a friendly report of what was detected and what to do next.
pub fn print_detected(tools: &[(AgentTool, DetectedState)]) {
    if tools.is_empty() {
        println!("No supported LLM CLI tools were detected.");
        println!();
        println!("docref currently supports:");
        println!("  - Kimi CLI     (https://www.kimi.com)");
        println!("  - Claude Code  (https://claude.ai/code)");
        println!("  - OpenAI Codex CLI");
        println!("  - Cursor");
        println!();
        println!("If you have one installed in a non-standard location, you can still");
        println!("configure it manually. See the README for examples.");
        return;
    }

    println!("Detected LLM CLI tools:");
    for (tool, state) in tools {
        let status = if state.hook_already_configured {
            "✓ hook already configured"
        } else if tool.supports_hooks() {
            "◯ hook available"
        } else {
            "- hooks not yet supported by docref"
        };
        println!("  {} ({})  — {}", tool.name(), tool.command(), status);
    }
    println!();

    let configurable: Vec<_> = tools
        .iter()
        .filter(|(t, s)| t.supports_hooks() && !s.hook_already_configured)
        .collect();

    if !configurable.is_empty() {
        if configurable.len() == 1 {
            println!(
                "Run 'docref init --apply' to add the PostToolUse hook for {}.",
                configurable[0].0.name()
            );
        } else {
            println!("To automatically add the PostToolUse hook, run:");
            for (tool, _) in &configurable {
                println!("  docref init --tool {} --apply", tool.command());
            }
        }
        println!();
        println!("Or add the following to your config files manually:");
        for (tool, _) in &configurable {
            if let Some(path) = tool.config_path() {
                println!();
                println!("# {}", path.display());
                if let Some(snippet) = tool.hook_snippet() {
                    println!("{}", snippet);
                }
            }
        }
    } else if tools.iter().any(|(_, s)| s.hook_already_configured) {
        println!("All detected tools already have a docref hook configured.");
    }
}

/// Apply the hook configuration for a specific tool.
pub fn apply_tool(tool: AgentTool) -> Result<()> {
    if !tool.supports_hooks() {
        bail!("{} does not yet support hooks in docref", tool.name());
    }

    let config_path = tool
        .config_path()
        .context("could not determine config path")?;

    let snippet = tool
        .hook_snippet()
        .context("no hook snippet available for this tool")?;

    ensure_config_dir(&config_path)?;

    if hook_already_configured(tool) {
        println!("Hook already configured in {}.", config_path.display());
        return Ok(());
    }

    // Append the snippet to the config file.
    let mut content = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&snippet);
    content.push('\n');

    fs::write(&config_path, content)
        .with_context(|| format!("failed to write {}", config_path.display()))?;

    println!("Added docref hook to {}.", config_path.display());
    Ok(())
}

fn ensure_config_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

pub fn parse_tool(name: &str) -> Option<AgentTool> {
    match name.to_lowercase().as_str() {
        "kimi" => Some(AgentTool::Kimi),
        "claude" => Some(AgentTool::Claude),
        "codex" => Some(AgentTool::Codex),
        "cursor" => Some(AgentTool::Cursor),
        _ => None,
    }
}
