use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Invalid configuration file {path}: {source}")]
    InvalidFile {
        path: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Invalid configuration text: {0}")]
    InvalidText(String),

    #[error("Configuration text cannot be empty")]
    EmptyConfig,
}

#[derive(Error, Debug)]
pub enum AgentSpecError {
    #[error("Invalid agent specification: {0}")]
    InvalidSpec(String),
}

#[derive(Error, Debug)]
pub enum SystemPromptTemplateError {
    #[error("Invalid system prompt template: {0}")]
    InvalidTemplate(String),
}

#[derive(Error, Debug)]
pub enum InvalidToolError {
    #[error("Invalid tool: {0}")]
    InvalidTool(String),
}

#[derive(Error, Debug)]
pub enum MCPConfigError {
    #[error("Invalid MCP configuration: {0}")]
    InvalidConfig(String),
}

#[derive(Error, Debug)]
pub enum MCPRuntimeError {
    #[error("MCP runtime error: {0}")]
    RuntimeError(String),
}

#[derive(Error, Debug)]
pub enum LLMNotSet {
    #[error("LLM is not set")]
    NotSet,
}

#[derive(Error, Debug)]
pub enum LLMNotSupported {
    #[error("LLM does not support required capability: {0}")]
    NotSupported(String),
}

#[derive(Error, Debug)]
pub enum ChatProviderError {
    #[error("Chat provider error: {0}")]
    ProviderError(String),
}

#[derive(Error, Debug, Clone)]
#[error("API connection error: {0}")]
pub struct APIConnectionError(pub String);

#[derive(Error, Debug, Clone)]
#[error("API timeout error: {0}")]
pub struct APITimeoutError(pub String);

#[derive(Error, Debug, Clone)]
#[error("API status error {status_code}: {message}")]
pub struct APIStatusError {
    pub status_code: u16,
    pub message: String,
}

#[derive(Error, Debug, Clone)]
#[error("API returned an empty response")]
pub struct APIEmptyResponseError;

#[derive(Error, Debug)]
pub enum MaxStepsReached {
    #[error("Maximum number of steps reached")]
    Reached,
}

#[derive(Error, Debug)]
pub enum RunCancelled {
    #[error("Run cancelled")]
    Cancelled,
}

#[derive(Error, Debug)]
pub enum ToolRejectedError {
    #[error("Tool call rejected by user")]
    Rejected {
        message: String,
        brief: String,
        has_feedback: bool,
    },
}

impl ToolRejectedError {
    pub fn new(message: impl Into<String>) -> Self {
        let msg = message.into();
        Self::Rejected {
            brief: msg.clone(),
            message: msg,
            has_feedback: false,
        }
    }

    pub fn with_feedback(message: impl Into<String>, brief: impl Into<String>) -> Self {
        Self::Rejected {
            message: message.into(),
            brief: brief.into(),
            has_feedback: true,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Rejected { message, .. } => message,
        }
    }

    pub fn brief(&self) -> &str {
        match self {
            Self::Rejected { brief, .. } => brief,
        }
    }

    pub fn has_feedback(&self) -> bool {
        match self {
            Self::Rejected { has_feedback, .. } => *has_feedback,
        }
    }
}

#[derive(Error, Debug)]
pub enum OctopusError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    AgentSpec(#[from] AgentSpecError),

    #[error(transparent)]
    SystemPromptTemplate(#[from] SystemPromptTemplateError),

    #[error(transparent)]
    InvalidTool(#[from] InvalidToolError),

    #[error(transparent)]
    MCPConfig(#[from] MCPConfigError),

    #[error(transparent)]
    MCPRuntime(#[from] MCPRuntimeError),

    #[error(transparent)]
    LLMNotSet(#[from] LLMNotSet),

    #[error(transparent)]
    LLMNotSupported(#[from] LLMNotSupported),

    #[error(transparent)]
    ChatProvider(#[from] ChatProviderError),

    #[error(transparent)]
    APIConnection(#[from] APIConnectionError),

    #[error(transparent)]
    APITimeout(#[from] APITimeoutError),

    #[error(transparent)]
    APIStatus(#[from] APIStatusError),

    #[error(transparent)]
    APIEmptyResponse(#[from] APIEmptyResponseError),

    #[error(transparent)]
    MaxStepsReached(#[from] MaxStepsReached),

    #[error(transparent)]
    RunCancelled(#[from] RunCancelled),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("TOML error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, OctopusError>;
