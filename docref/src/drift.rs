use crate::db::Store;
use crate::resolver::find_item_line;
use crate::types::{CheckSummary, DriftIssue, DriftKind, SourceReference};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Default number of lines of tolerance before we consider a reference drifted.
const LINE_DRIFT_THRESHOLD: usize = 5;

/// Check drift for source files that were modified.
///
/// If `sources` is empty, checks all stored snippets.
pub fn check_drift(
    store: &Store,
    project_root: &Path,
    sources: &[PathBuf],
) -> Result<CheckSummary> {
    let snippets = if sources.is_empty() {
        store.get_all_snippets()?
    } else {
        store.get_snippets_for_sources(sources)?
    };

    let mut summary = CheckSummary {
        scanned_refs: snippets.len(),
        ..Default::default()
    };

    for snippet in snippets {
        let source_abs = project_root.join(&snippet.source_path);
        let issue = check_single(&snippet.to_source_reference(), &source_abs)?;

        store.update_verification(snippet.id, issue.current_line, None, issue.has_drift())?;

        if issue.is_warning() {
            summary.warnings.push(issue);
        } else if issue.has_drift() {
            summary.issues.push(issue);
        }
    }

    Ok(summary)
}

fn check_single(reference: &SourceReference, source_abs: &Path) -> Result<DriftIssue> {
    let source_exists = source_abs.exists();

    let (current_line, current_item_found) = if source_exists {
        match find_item_line(source_abs, &reference.item_name)? {
            Some(line) => (Some(line), true),
            None => {
                // Item name is not resolvable (may be a descriptive label).
                // Fall back to checking whether the recorded line is still valid.
                let line_count = count_lines(source_abs)?;
                if reference.doc_line > 0 && reference.doc_line <= line_count {
                    (Some(reference.doc_line), false)
                } else {
                    (None, false)
                }
            }
        }
    } else {
        (None, false)
    };

    let kind = if !source_exists {
        DriftKind::Missing
    } else if current_item_found {
        let drifted = current_line
            .map(|l| l.abs_diff(reference.doc_line) > LINE_DRIFT_THRESHOLD)
            .unwrap_or(true);
        if drifted {
            DriftKind::LineDrift {
                from: reference.doc_line,
                to: current_line.unwrap_or(reference.doc_line),
            }
        } else {
            DriftKind::Exact
        }
    } else if current_line.is_some() {
        // Descriptive label — line is in bounds but item not found.
        DriftKind::Warning("descriptive label".to_string())
    } else {
        DriftKind::Missing
    };

    let message = match &kind {
        DriftKind::Missing if !source_exists => {
            format!("source file does not exist: {}", source_abs.display())
        }
        DriftKind::Missing => {
            format!(
                "item '{}' not found in {} and recorded line {} is out of bounds",
                reference.item_name,
                reference.source_path.display(),
                reference.doc_line
            )
        }
        DriftKind::LineDrift { from, to } => {
            format!(
                "{} moved from ~line {} to ~line {} (drift > {} lines)",
                reference.item_name, from, to, LINE_DRIFT_THRESHOLD
            )
        }
        DriftKind::Warning(_) => {
            format!(
                "item '{}' not found in {} (descriptive label? line {} is within file bounds)",
                reference.item_name,
                reference.source_path.display(),
                reference.doc_line
            )
        }
        DriftKind::Exact => String::new(),
        DriftKind::BodyDrift => "snippet body has changed".to_string(),
    };

    Ok(DriftIssue {
        reference: reference.clone(),
        kind,
        current_line,
        message,
    })
}

fn count_lines(path: &Path) -> Result<usize> {
    let content = std::fs::read_to_string(path)?;
    Ok(content.lines().count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_check_single_finds_item() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "\n\npub fn rebuild_index() {{}}\n").unwrap();

        let reference = SourceReference {
            doc_path: PathBuf::from("doc.md"),
            source_path: PathBuf::from(tmp.path()),
            doc_line: 1,
            item_name: "rebuild_index".to_string(),
            annotation: None,
            snippet_body: String::new(),
            snippet_checksum: String::new(),
        };

        let issue = check_single(&reference, tmp.path()).unwrap();
        assert!(matches!(issue.kind, DriftKind::Exact));
        assert_eq!(issue.current_line, Some(3));
    }

    #[test]
    fn test_check_single_line_drift() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "\n\npub fn rebuild_index() {{}}\n").unwrap();

        let reference = SourceReference {
            doc_path: PathBuf::from("doc.md"),
            source_path: PathBuf::from(tmp.path()),
            doc_line: 100,
            item_name: "rebuild_index".to_string(),
            annotation: None,
            snippet_body: String::new(),
            snippet_checksum: String::new(),
        };

        let issue = check_single(&reference, tmp.path()).unwrap();
        assert!(matches!(
            issue.kind,
            DriftKind::LineDrift { from: 100, to: 3 }
        ));
    }

    #[test]
    fn test_check_single_descriptive_label() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        writeln!(tmp, "\n\npub fn rebuild_index() {{}}\n").unwrap();

        let reference = SourceReference {
            doc_path: PathBuf::from("doc.md"),
            source_path: PathBuf::from(tmp.path()),
            doc_line: 3,
            item_name: "Some descriptive label".to_string(),
            annotation: None,
            snippet_body: String::new(),
            snippet_checksum: String::new(),
        };

        let issue = check_single(&reference, tmp.path()).unwrap();
        assert!(matches!(issue.kind, DriftKind::Warning(_)));
        assert!(!issue.has_drift());
    }
}
