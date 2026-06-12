use serde::Serialize;
use std::str::FromStr;

/// A built-in tool shipped with octopus-cli.
///
/// These are the tools whose implementations live in `octopus-cli/src/tools/`
/// and are known at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum BuiltinTool {
    Shell,
    ReadFile,
    WriteFile,
    StrReplaceFile,
    Glob,
    Grep,
    FetchURL,
    SearchWeb,
    AskUserQuestion,
    SetTodoList,
    Think,
    EnterPlanMode,
    ExitPlanMode,
    Agent,
    SendDMail,
    TaskList,
    TaskOutput,
    TaskStop,
    ReadMediaFile,
}

impl BuiltinTool {
    /// The canonical tool name exposed to the LLM.
    pub const fn as_str(&self) -> &'static str {
        match self {
            BuiltinTool::Shell => "Shell",
            BuiltinTool::ReadFile => "ReadFile",
            BuiltinTool::WriteFile => "WriteFile",
            BuiltinTool::StrReplaceFile => "StrReplaceFile",
            BuiltinTool::Glob => "Glob",
            BuiltinTool::Grep => "Grep",
            BuiltinTool::FetchURL => "FetchURL",
            BuiltinTool::SearchWeb => "SearchWeb",
            BuiltinTool::AskUserQuestion => "AskUserQuestion",
            BuiltinTool::SetTodoList => "SetTodoList",
            BuiltinTool::Think => "Think",
            BuiltinTool::EnterPlanMode => "EnterPlanMode",
            BuiltinTool::ExitPlanMode => "ExitPlanMode",
            BuiltinTool::Agent => "Agent",
            BuiltinTool::SendDMail => "SendDMail",
            BuiltinTool::TaskList => "TaskList",
            BuiltinTool::TaskOutput => "TaskOutput",
            BuiltinTool::TaskStop => "TaskStop",
            BuiltinTool::ReadMediaFile => "ReadMediaFile",
        }
    }
}

impl std::fmt::Display for BuiltinTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for BuiltinTool {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            // Short names
            "Shell" => Ok(BuiltinTool::Shell),
            "ReadFile" => Ok(BuiltinTool::ReadFile),
            "WriteFile" => Ok(BuiltinTool::WriteFile),
            "StrReplaceFile" => Ok(BuiltinTool::StrReplaceFile),
            "Glob" => Ok(BuiltinTool::Glob),
            "Grep" => Ok(BuiltinTool::Grep),
            "FetchURL" => Ok(BuiltinTool::FetchURL),
            "SearchWeb" => Ok(BuiltinTool::SearchWeb),
            "AskUser" | "AskUserQuestion" => Ok(BuiltinTool::AskUserQuestion),
            "SetTodoList" => Ok(BuiltinTool::SetTodoList),
            "Think" => Ok(BuiltinTool::Think),
            "EnterPlanMode" => Ok(BuiltinTool::EnterPlanMode),
            "ExitPlanMode" => Ok(BuiltinTool::ExitPlanMode),
            "Agent" => Ok(BuiltinTool::Agent),
            "SendDMail" => Ok(BuiltinTool::SendDMail),
            "TaskList" => Ok(BuiltinTool::TaskList),
            "TaskOutput" => Ok(BuiltinTool::TaskOutput),
            "TaskStop" => Ok(BuiltinTool::TaskStop),
            "ReadMediaFile" => Ok(BuiltinTool::ReadMediaFile),
            // Python-style fully qualified names (backward compat)
            "kimi_cli.tools.shell:Shell" => Ok(BuiltinTool::Shell),
            "kimi_cli.tools.file:ReadFile" => Ok(BuiltinTool::ReadFile),
            "kimi_cli.tools.file:WriteFile" => Ok(BuiltinTool::WriteFile),
            "kimi_cli.tools.file:StrReplaceFile" => Ok(BuiltinTool::StrReplaceFile),
            "kimi_cli.tools.file:Glob" => Ok(BuiltinTool::Glob),
            "kimi_cli.tools.file:Grep" => Ok(BuiltinTool::Grep),
            "kimi_cli.tools.web:FetchURL" => Ok(BuiltinTool::FetchURL),
            "kimi_cli.tools.web:SearchWeb" => Ok(BuiltinTool::SearchWeb),
            "kimi_cli.tools.ask_user:AskUserQuestion" => Ok(BuiltinTool::AskUserQuestion),
            "kimi_cli.tools.todo:SetTodoList" => Ok(BuiltinTool::SetTodoList),
            "kimi_cli.tools.think:Think" => Ok(BuiltinTool::Think),
            "kimi_cli.tools.plan.enter:EnterPlanMode" => Ok(BuiltinTool::EnterPlanMode),
            "kimi_cli.tools.plan:ExitPlanMode" => Ok(BuiltinTool::ExitPlanMode),
            "kimi_cli.tools.agent:Agent" => Ok(BuiltinTool::Agent),
            "kimi_cli.tools.dmail:SendDMail" => Ok(BuiltinTool::SendDMail),
            "kimi_cli.tools.background:TaskOutput" => Ok(BuiltinTool::TaskOutput),
            "kimi_cli.tools.background:TaskStop" => Ok(BuiltinTool::TaskStop),
            "kimi_cli.tools.background:TaskList" => Ok(BuiltinTool::TaskList),
            "kimi_cli.tools.file:ReadMediaFile" => Ok(BuiltinTool::ReadMediaFile),
            _ => Err(format!("Unknown builtin tool name: {}", s)),
        }
    }
}

impl<'de> serde::Deserialize<'de> for BuiltinTool {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<BuiltinTool>().map_err(serde::de::Error::custom)
    }
}

/// Where a tool originates from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolSource {
    Builtin,
    Mcp { server: String },
    Plugin { plugin: String },
}

/// A tool name, categorized by its source.
///
/// This type replaces bare `String` tool names anywhere the codebase
/// needs to distinguish built-in tools from dynamically loaded ones.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "source", content = "name")]
pub enum ToolName {
    /// Built-in tool shipped with octopus-cli.
    Builtin(BuiltinTool),
    /// Tool from an MCP server.
    Mcp { server: String, name: String },
    /// Tool from a WASM plugin.
    Plugin { plugin: String, name: String },
}

impl ToolName {
    /// The bare tool name as exposed to the LLM.
    pub fn name(&self) -> &str {
        match self {
            ToolName::Builtin(b) => b.as_str(),
            ToolName::Mcp { name, .. } => name,
            ToolName::Plugin { name, .. } => name,
        }
    }

    /// The source category of this tool.
    pub fn source(&self) -> ToolSource {
        match self {
            ToolName::Builtin(_) => ToolSource::Builtin,
            ToolName::Mcp { server, .. } => ToolSource::Mcp {
                server: server.clone(),
            },
            ToolName::Plugin { plugin, .. } => ToolSource::Plugin {
                plugin: plugin.clone(),
            },
        }
    }

    /// Returns `true` if this is a built-in tool.
    pub fn is_builtin(&self) -> bool {
        matches!(self, ToolName::Builtin(_))
    }
}

impl std::fmt::Display for ToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolName::Builtin(b) => write!(f, "{}", b),
            ToolName::Mcp { server, name } => write!(f, "mcp:{}/{}", server, name),
            ToolName::Plugin { plugin, name } => write!(f, "plugin:{}/{}", plugin, name),
        }
    }
}

impl FromStr for ToolName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Try builtin first
        if let Ok(builtin) = s.parse::<BuiltinTool>() {
            return Ok(ToolName::Builtin(builtin));
        }

        // MCP: mcp:server/name
        if let Some(rest) = s.strip_prefix("mcp:") {
            let (server, name) = rest
                .split_once('/')
                .ok_or_else(|| format!("Invalid MCP tool name format: {}", s))?;
            return Ok(ToolName::Mcp {
                server: server.to_string(),
                name: name.to_string(),
            });
        }

        // Plugin: plugin:plugin_name/name
        if let Some(rest) = s.strip_prefix("plugin:") {
            let (plugin, name) = rest
                .split_once('/')
                .ok_or_else(|| format!("Invalid plugin tool name format: {}", s))?;
            return Ok(ToolName::Plugin {
                plugin: plugin.to_string(),
                name: name.to_string(),
            });
        }

        Err(format!(
            "'{}' is not a known builtin tool and does not use mcp: or plugin: prefix",
            s
        ))
    }
}

impl<'de> serde::Deserialize<'de> for ToolName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<ToolName>().map_err(serde::de::Error::custom)
    }
}

impl From<BuiltinTool> for ToolName {
    fn from(t: BuiltinTool) -> Self {
        ToolName::Builtin(t)
    }
}
