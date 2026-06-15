/// Errors that can occur while running the Brain.
#[derive(Debug, thiserror::Error)]
pub enum BrainError {
    #[error("No LLM provider configured")]
    NoProvider,

    #[error("LLM error: {0}")]
    Llm(String),

    #[error("{0}")]
    Other(String),

    #[error("API status error {status_code}: {message}")]
    ApiStatus { status_code: u16, message: String },

    #[error("API connection error: {0}")]
    ApiConnection(String),

    #[error("API timeout error: {0}")]
    ApiTimeout(String),

    #[error("API returned an empty response")]
    ApiEmptyResponse,

    #[error("Tool error: {0}")]
    Tool(String),

    #[error("Turn exceeded maximum steps ({0})")]
    MaxSteps(usize),

    #[error("Recovery failed: {0}")]
    Recovery(String),
}

/// High-level category used for retry/recovery telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrainErrorCategory {
    RateLimit,
    Auth,
    Server5xx,
    Client4xx,
    Api,
    Network,
    Timeout,
    EmptyResponse,
    Other,
}

impl BrainError {
    /// Classify a kosong provider error into a typed BrainError.
    pub fn from_kosong_error(err: kosong::ChatProviderError) -> Self {
        match err.kind {
            kosong::ChatProviderErrorKind::Status {
                status_code,
                message,
                ..
            } => BrainError::ApiStatus {
                status_code,
                message,
            },
            kosong::ChatProviderErrorKind::Connection(message) => {
                BrainError::ApiConnection(message)
            }
            kosong::ChatProviderErrorKind::Timeout(message) => BrainError::ApiTimeout(message),
            kosong::ChatProviderErrorKind::EmptyResponse => BrainError::ApiEmptyResponse,
            kosong::ChatProviderErrorKind::Other(message) => BrainError::Llm(message),
        }
    }

    /// Returns the HTTP status code if this is an API status error.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            BrainError::ApiStatus { status_code, .. } => Some(*status_code),
            _ => None,
        }
    }

    /// Classify this error for retry/recovery telemetry.
    pub fn category(&self) -> BrainErrorCategory {
        match self {
            BrainError::ApiStatus { status_code, .. } => match *status_code {
                429 => BrainErrorCategory::RateLimit,
                401 | 403 => BrainErrorCategory::Auth,
                s if s >= 500 => BrainErrorCategory::Server5xx,
                s if (400..500).contains(&s) => BrainErrorCategory::Client4xx,
                _ => BrainErrorCategory::Api,
            },
            BrainError::ApiConnection(_) => BrainErrorCategory::Network,
            BrainError::ApiTimeout(_) => BrainErrorCategory::Timeout,
            BrainError::ApiEmptyResponse => BrainErrorCategory::EmptyResponse,
            _ => BrainErrorCategory::Other,
        }
    }

    /// Whether this error represents a transient network failure.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            BrainError::ApiConnection(_) | BrainError::ApiTimeout(_) | BrainError::ApiEmptyResponse
        ) || matches!(self, BrainError::ApiStatus { status_code, .. } if matches!(status_code, 429 | 500 | 502 | 503 | 504))
    }

    /// Whether this error is an authentication failure that may be recoverable
    /// by refreshing the provider.
    pub fn is_auth_failure(&self) -> bool {
        matches!(
            self,
            BrainError::ApiStatus {
                status_code: 401,
                ..
            }
        )
    }
}
