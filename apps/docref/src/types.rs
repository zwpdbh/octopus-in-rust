use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A source-location reference found inside a markdown code block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReference {
    /// Path to the markdown document containing the reference.
    pub doc_path: PathBuf,
    /// Path to the source file, relative to the project root.
    pub source_path: PathBuf,
    /// Line number recorded in the documentation at the time of the last scan/fix.
    pub doc_line: usize,
    /// Name of the item (function, struct, etc.) being referenced.
    pub item_name: String,
    /// Optional annotation like "(abbreviated)" or "(private associated fn)".
    pub annotation: Option<String>,
    /// Body of the snippet that follows the source-location comment.
    pub snippet_body: String,
    /// Checksum of the snippet body at the time of the last scan/fix.
    pub snippet_checksum: String,
}

/// Classification of the drift check result for a single reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriftKind {
    /// The item was found at roughly the expected line and the body matches.
    Exact,
    /// The item was found but has moved beyond the drift threshold.
    LineDrift { from: usize, to: usize },
    /// The snippet body no longer matches the stored checksum.
    BodyDrift,
    /// The source file or the item within it could not be found.
    Missing,
    /// Soft warning — e.g. a descriptive label that cannot be resolved.
    Warning(String),
}

impl DriftKind {
    pub fn has_drift(&self) -> bool {
        matches!(
            self,
            DriftKind::LineDrift { .. } | DriftKind::BodyDrift | DriftKind::Missing
        )
    }

    pub fn is_warning(&self) -> bool {
        matches!(self, DriftKind::Warning(_))
    }
}

/// Result of comparing a stored reference against the current source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriftIssue {
    pub reference: SourceReference,
    pub kind: DriftKind,
    /// The current line number of the item (if found), or the last-known valid line.
    pub current_line: Option<usize>,
    pub message: String,
}

impl DriftIssue {
    pub fn has_drift(&self) -> bool {
        self.kind.has_drift()
    }

    pub fn is_warning(&self) -> bool {
        self.kind.is_warning()
    }
}

/// Summary returned by a check run.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckSummary {
    pub scanned_docs: usize,
    pub scanned_refs: usize,
    pub issues: Vec<DriftIssue>,
    pub warnings: Vec<DriftIssue>,
}
