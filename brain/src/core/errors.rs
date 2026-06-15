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

impl BrainError {
    /// Classify a kosong provider error into a typed BrainError.
    pub fn from_kosong_error(err: kosong::ChatProviderError) -> Self {
        let msg = err.message;

        if msg.starts_with("API status error") {
            // Try to extract a status code from messages like:
            // "API status error 401: ..." or "API status error 429: ..."
            let rest = msg.strip_prefix("API status error ").unwrap_or("");
            if let Some((code_str, tail)) = rest.split_once(':') {
                if let Ok(status_code) = code_str.trim().parse::<u16>() {
                    return BrainError::ApiStatus {
                        status_code,
                        message: tail.trim().to_string(),
                    };
                }
            }
        }

        if msg.starts_with("API connection error") {
            return BrainError::ApiConnection(msg);
        }

        if msg.starts_with("API timeout error") {
            return BrainError::ApiTimeout(msg);
        }

        if msg == "API returned an empty response" {
            return BrainError::ApiEmptyResponse;
        }

        BrainError::Llm(msg)
    }

    /// Returns the HTTP status code if this is an API status error.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            BrainError::ApiStatus { status_code, .. } => Some(*status_code),
            _ => None,
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
