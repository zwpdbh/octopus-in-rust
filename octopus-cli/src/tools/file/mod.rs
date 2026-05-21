use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::Tool;

const MAX_LINES: usize = 1000;
const MAX_LINE_LENGTH: usize = 2000;
const _MAX_BYTES: usize = 100 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadFileParams {
    pub path: String,
    #[serde(default = "default_line_offset")]
    pub line_offset: i32,
    #[serde(default = "default_n_lines")]
    pub n_lines: usize,
}

fn default_line_offset() -> i32 {
    1
}

fn default_n_lines() -> usize {
    MAX_LINES
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteFileParams {
    pub path: String,
    pub content: String,
    #[serde(default = "default_write_mode")]
    pub mode: String,
}

fn default_write_mode() -> String {
    "overwrite".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edit {
    pub old: String,
    pub new: String,
    #[serde(default)]
    pub replace_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrReplaceFileParams {
    pub path: String,
    pub edit: Edit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobParams {
    pub pattern: String,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default = "default_include_dirs")]
    pub include_dirs: bool,
}

fn default_include_dirs() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepParams {
    pub pattern: String,
    #[serde(default = "default_grep_path")]
    pub path: String,
    #[serde(default)]
    pub glob: Option<String>,
    #[serde(default = "default_output_mode")]
    pub output_mode: String,
    #[serde(default)]
    pub before_context: Option<usize>,
    #[serde(default)]
    pub after_context: Option<usize>,
    #[serde(default)]
    pub context: Option<usize>,
    #[serde(default = "default_line_number")]
    pub line_number: bool,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default = "default_head_limit")]
    pub head_limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub multiline: bool,
    #[serde(default)]
    pub include_ignored: bool,
}

fn default_grep_path() -> String {
    ".".to_string()
}

fn default_output_mode() -> String {
    "files_with_matches".to_string()
}

fn default_line_number() -> bool {
    true
}

fn default_head_limit() -> usize {
    250
}

pub struct ReadFileTool;
pub struct WriteFileTool;
pub struct StrReplaceFileTool;
pub struct GlobTool;
pub struct GrepTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl StrReplaceFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl GlobTool {
    pub fn new() -> Self {
        Self
    }
}

impl GrepTool {
    pub fn new() -> Self {
        Self
    }
}

fn _resolve_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

fn _read_file_lines(path: &Path, line_offset: i32, n_lines: usize) -> Result<String, String> {
    let content =
        std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    let start = if line_offset < 0 {
        let tail_count = line_offset.abs() as usize;
        total_lines.saturating_sub(tail_count)
    } else {
        (line_offset as usize).saturating_sub(1)
    };

    let end = (start + n_lines).min(total_lines).min(start + MAX_LINES);

    let mut result = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let line_num = start + i + 1;
        let truncated = if line.len() > MAX_LINE_LENGTH {
            &line[..MAX_LINE_LENGTH]
        } else {
            line
        };
        result.push_str(&format!("{:6}\t{}\n", line_num, truncated));
    }

    let mut msg = format!(
        "{} lines read from file starting from line {}.",
        end - start,
        start + 1
    );
    msg.push_str(&format!(" Total lines in file: {}.", total_lines));
    if end < total_lines && end - start >= MAX_LINES {
        msg.push_str(&format!(" Max {} lines reached.", MAX_LINES));
    } else if end < total_lines {
        msg.push_str(" End of file reached.");
    }

    Ok(result)
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "ReadFile"
    }

    fn description(&self) -> &str {
        "Read a file from the local filesystem."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "ReadFile",
            "description": "Read a file from the local filesystem. Can read partial content with line_offset and n_lines.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "line_offset": { "type": "integer", "default": 1, "description": "Line to start from (1-based, negative for tail)" },
                    "n_lines": { "type": "integer", "default": 1000, "description": "Max lines to read" }
                },
                "required": ["path"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: ReadFileParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        let path = _resolve_path(&params.path);

        if !path.exists() {
            return Err(format!("`{}` does not exist.", params.path));
        }
        if !path.is_file() {
            return Err(format!("`{}` is not a file.", params.path));
        }

        _read_file_lines(&path, params.line_offset, params.n_lines)
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "WriteFile"
    }

    fn description(&self) -> &str {
        "Write content to a file."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "WriteFile",
            "description": "Write content to a file. Supports overwrite and append modes.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "content": { "type": "string", "description": "Content to write" },
                    "mode": { "type": "string", "enum": ["overwrite", "append"], "default": "overwrite" }
                },
                "required": ["path", "content"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: WriteFileParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        let path = _resolve_path(&params.path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent directory: {}", e))?;
        }

        match params.mode.as_str() {
            "overwrite" => {
                std::fs::write(&path, &params.content)
                    .map_err(|e| format!("Failed to write file: {}", e))?;
            }
            "append" => {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .map_err(|e| format!("Failed to open file: {}", e))?;
                file.write_all(params.content.as_bytes())
                    .map_err(|e| format!("Failed to append to file: {}", e))?;
            }
            _ => return Err(format!("Invalid mode: {}", params.mode)),
        }

        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(format!(
            "File successfully written. Current size: {} bytes.",
            size
        ))
    }
}

#[async_trait]
impl Tool for StrReplaceFileTool {
    fn name(&self) -> &str {
        "StrReplaceFile"
    }

    fn description(&self) -> &str {
        "Replace text in a file."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "StrReplaceFile",
            "description": "Replace text in a file. Use exact string matching.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file" },
                    "edit": {
                        "type": "object",
                        "properties": {
                            "old": { "type": "string", "description": "Old string to replace" },
                            "new": { "type": "string", "description": "New string to replace with" },
                            "replace_all": { "type": "boolean", "default": false }
                        },
                        "required": ["old", "new"]
                    }
                },
                "required": ["path", "edit"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: StrReplaceFileParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        let path = _resolve_path(&params.path);

        if !path.exists() {
            return Err(format!("`{}` does not exist.", params.path));
        }

        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))?;

        let new_content = if params.edit.replace_all {
            content.replace(&params.edit.old, &params.edit.new)
        } else {
            content.replacen(&params.edit.old, &params.edit.new, 1)
        };

        if new_content == content {
            return Err(
                "No replacements were made. The old string was not found in the file.".to_string(),
            );
        }

        std::fs::write(&path, &new_content).map_err(|e| format!("Failed to write file: {}", e))?;

        Ok("File successfully edited.".to_string())
    }
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "Glob",
            "description": "Find files matching a glob pattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern (e.g. src/**/*.rs)" },
                    "directory": { "type": "string", "description": "Directory to search in" },
                    "include_dirs": { "type": "boolean", "default": true }
                },
                "required": ["pattern"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: GlobParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        if params.pattern.starts_with("**") {
            return Err(
                "Pattern starting with '**' is not allowed. Use a more specific pattern."
                    .to_string(),
            );
        }

        let dir = params
            .directory
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let pattern = dir.join(&params.pattern);
        let pattern_str = pattern.to_string_lossy();

        let entries =
            glob::glob(&pattern_str).map_err(|e| format!("Invalid glob pattern: {}", e))?;

        let mut matches = Vec::new();
        for entry in entries {
            if let Ok(path) = entry {
                if params.include_dirs || path.is_file() {
                    matches.push(path.to_string_lossy().to_string());
                }
            }
        }

        matches.sort();

        if matches.len() > 1000 {
            matches.truncate(1000);
        }

        Ok(matches.join("\n"))
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents using regex."
    }

    fn schema(&self) -> Value {
        serde_json::json!({
            "name": "Grep",
            "description": "Search file contents using regex.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern" },
                    "path": { "type": "string", "default": ".", "description": "File or directory to search" },
                    "glob": { "type": "string", "description": "Glob filter" },
                    "output_mode": { "type": "string", "enum": ["content", "files_with_matches", "count_matches"], "default": "files_with_matches" },
                    "before_context": { "type": "integer", "description": "Lines before match" },
                    "after_context": { "type": "integer", "description": "Lines after match" },
                    "context": { "type": "integer", "description": "Lines before and after" },
                    "line_number": { "type": "boolean", "default": true },
                    "ignore_case": { "type": "boolean", "default": false },
                    "type": { "type": "string", "description": "File type" },
                    "head_limit": { "type": "integer", "default": 250 },
                    "offset": { "type": "integer", "default": 0 },
                    "multiline": { "type": "boolean", "default": false },
                    "include_ignored": { "type": "boolean", "default": false }
                },
                "required": ["pattern"]
            }
        })
    }

    async fn call(&self, arguments: Value) -> Result<String, String> {
        let params: GrepParams =
            serde_json::from_value(arguments).map_err(|e| format!("Invalid parameters: {}", e))?;

        let path = _resolve_path(&params.path);

        let mut args = vec!["rg".to_string()];

        if params.ignore_case {
            args.push("--ignore-case".to_string());
        }
        if params.multiline {
            args.push("--multiline".to_string());
            args.push("--multiline-dotall".to_string());
        }

        match params.output_mode.as_str() {
            "content" => {
                if let Some(b) = params.before_context {
                    args.push("--before-context".to_string());
                    args.push(b.to_string());
                }
                if let Some(a) = params.after_context {
                    args.push("--after-context".to_string());
                    args.push(a.to_string());
                }
                if let Some(c) = params.context {
                    args.push("--context".to_string());
                    args.push(c.to_string());
                }
                if params.line_number {
                    args.push("--line-number".to_string());
                }
            }
            "files_with_matches" => {
                args.push("--files-with-matches".to_string());
            }
            "count_matches" => {
                args.push("--count-matches".to_string());
            }
            _ => {}
        }

        if let Some(glob) = params.glob {
            args.push("--glob".to_string());
            args.push(glob);
        }
        if let Some(ty) = params.r#type {
            args.push("--type".to_string());
            args.push(ty);
        }

        if params.include_ignored {
            args.push("--no-ignore".to_string());
        }
        args.push("--hidden".to_string());

        args.push("--".to_string());
        args.push(params.pattern);
        args.push(path.to_string_lossy().to_string());

        let output = tokio::process::Command::new("rg")
            .args(&args[1..])
            .output()
            .await
            .map_err(|e| format!("Failed to run grep: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines: Vec<&str> = stdout.lines().collect();

        if params.offset > 0 {
            lines = lines.into_iter().skip(params.offset).collect();
        }

        if params.head_limit > 0 && lines.len() > params.head_limit {
            lines.truncate(params.head_limit);
        }

        Ok(lines.join("\n"))
    }
}
