use crate::db::Store;
use crate::drift::check_drift;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Kinds of hook events that kimi-cli can emit.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum HookEventKind {
    PostToolUse,
    #[serde(other)]
    Other,
}

/// Tools whose input contains a file path we care about.
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
enum ToolName {
    #[default]
    Other,
    WriteFile,
    StrReplaceFile,
}

/// Input shape for file-writing tools (WriteFile, StrReplaceFile).
#[derive(Debug, Deserialize, Default)]
struct FileToolInput {
    #[serde(default)]
    path: String,
}

/// Kimi CLI PostToolUse event payload.
#[derive(Debug, Deserialize)]
struct KimiEvent {
    #[serde(rename = "hook_event_name")]
    kind: HookEventKind,
    #[serde(default)]
    #[allow(dead_code)]
    session_id: String,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    tool_name: ToolName,
    #[serde(default)]
    tool_input: FileToolInput,
    #[serde(default)]
    #[allow(dead_code)]
    tool_output: String,
    #[serde(default)]
    #[allow(dead_code)]
    tool_call_id: String,
}

/// Run docref as a kimi-cli PostToolUse hook.
///
/// Reads the JSON event from stdin, determines the modified source file,
/// and checks for documentation drift. Always exits 0 so the tool use is
/// never blocked; warnings are emitted to stderr.
pub fn run_kimi_hook(store: &Store, project_root: &Path) -> Result<()> {
    let mut stdin = String::new();
    io::stdin()
        .read_to_string(&mut stdin)
        .context("failed to read stdin")?;

    if stdin.trim().is_empty() {
        return Ok(());
    }

    let event: KimiEvent = serde_json::from_str(&stdin).context("invalid kimi event JSON")?;

    if !matches!(event.kind, HookEventKind::PostToolUse) {
        return Ok(());
    }

    let file_path = match event.tool_name {
        ToolName::WriteFile | ToolName::StrReplaceFile => {
            if event.tool_input.path.is_empty() {
                return Ok(());
            }
            PathBuf::from(event.tool_input.path)
        }
        ToolName::Other => return Ok(()),
    };

    // Resolve against cwd if relative.
    let abs_path = Path::new(&event.cwd).join(&file_path);
    let source_path = if abs_path.is_absolute() {
        match abs_path.strip_prefix(&event.cwd) {
            Ok(p) => p.to_path_buf(),
            Err(_) => abs_path,
        }
    } else {
        file_path
    };

    // Only check code files.
    if !is_code_file(&source_path) {
        return Ok(());
    }

    let summary =
        check_drift(store, project_root, &[source_path.clone()]).context("drift check failed")?;

    if summary.issues.is_empty() && summary.warnings.is_empty() {
        return Ok(());
    }

    eprintln!(
        "[docref] {} issue(s), {} warning(s) for {}",
        summary.issues.len(),
        summary.warnings.len(),
        source_path.display()
    );

    for issue in &summary.issues {
        let r = &issue.reference;
        if let Some(cur) = issue.current_line {
            eprintln!(
                "[docref] DRIFT: {}:{} ~line {} — {} now at ~line {} ({})",
                r.doc_path.display(),
                r.source_path.display(),
                r.doc_line,
                r.item_name,
                cur,
                issue.message
            );
        } else {
            eprintln!(
                "[docref] DRIFT: {}:{} ~line {} — {} ({})",
                r.doc_path.display(),
                r.source_path.display(),
                r.doc_line,
                r.item_name,
                issue.message
            );
        }
    }

    for warning in &summary.warnings {
        let r = &warning.reference;
        eprintln!(
            "[docref] WARN: {}:{} ~line {} — {} ({})",
            r.doc_path.display(),
            r.source_path.display(),
            r.doc_line,
            r.item_name,
            warning.message
        );
    }

    Ok(())
}

fn is_code_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("rs")
            | Some("py")
            | Some("js")
            | Some("ts")
            | Some("go")
            | Some("java")
            | Some("c")
            | Some("cpp")
            | Some("h")
            | Some("hpp")
    )
}
