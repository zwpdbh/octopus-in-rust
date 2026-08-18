//! Error type for the fafcn-gamedata crate.

/// Result alias for fafcn-gamedata operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by manifest and path helpers.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A relative path failed validation (absolute, parent traversal, etc.).
    #[error("invalid relative path {path:?}: {reason}")]
    InvalidPath {
        /// The offending path.
        path: String,
        /// Why it was rejected.
        reason: String,
    },

    /// I/O failure while hashing files.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
