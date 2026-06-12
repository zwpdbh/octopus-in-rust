use crate::db::Store;
use crate::drift::check_drift;
use crate::hook::run_kimi_hook;
use crate::init::{apply_tool, detect_tools, parse_tool, print_detected};
use crate::migrate::{
    Confidence, apply_proposals, build_source_index, find_unmatched_blocks, propose_locations,
};
use crate::parser::{find_markdown_files, parse_document};
use crate::types::CheckSummary;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "docref")]
#[command(about = "Keep markdown documentation in sync with code")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Path to the project root (default: current directory).
    #[arg(long, global = true, value_name = "PATH")]
    pub root: Option<PathBuf>,

    /// Path to the SQLite database.
    #[arg(long, global = true, default_value = ".docref.db", value_name = "PATH")]
    pub db: PathBuf,

    /// Output format.
    #[arg(long, global = true, value_enum, default_value = "text")]
    pub format: OutputFormat,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scan markdown documents and index source references.
    Scan {
        /// Specific markdown file or directory to scan.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Check documentation for drift against current source files.
    Check {
        /// Only check references to this source file.
        #[arg(long, value_name = "PATH")]
        source: Option<PathBuf>,

        /// Check all references, not just recently changed ones.
        #[arg(long)]
        all: bool,
    },

    /// Show current index status.
    Status,

    /// Run as an LLM agent hook.
    Hook {
        /// Which agent's event format to consume.
        #[arg(value_name = "AGENT")]
        agent: HookAgent,
    },

    /// Detect installed LLM CLI tools and configure docref hooks.
    Init {
        /// Apply the hook configuration automatically instead of just printing.
        #[arg(long)]
        apply: bool,

        /// Only configure a specific tool (kimi, claude, codex, cursor).
        #[arg(long, value_name = "TOOL")]
        tool: Option<String>,
    },

    /// Migrate existing markdown docs by adding source-location comments.
    Migrate {
        /// Only print what would change; do not modify files.
        #[arg(long)]
        dry_run: bool,

        /// Specific markdown file or directory to migrate.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HookAgent {
    /// Kimi CLI PostToolUse event format (JSON on stdin).
    Kimi,
}

#[derive(Serialize)]
struct ScanOutput {
    scanned_docs: usize,
    indexed_refs: usize,
}

#[derive(Serialize)]
struct StatusOutput {
    indexed_docs: usize,
    indexed_snippets: usize,
    drift_count: usize,
}

pub fn run(cli: Cli) -> Result<()> {
    let root = cli
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current dir"));
    let db_path = if cli.db.is_absolute() {
        cli.db.clone()
    } else {
        root.join(&cli.db)
    };

    let store = Store::open(&db_path)
        .with_context(|| format!("failed to open docref store at {}", db_path.display()))?;

    match cli.command {
        Commands::Scan { path } => run_scan(&store, &root, path.as_deref(), cli.format),
        Commands::Check { source, all } => {
            run_check(&store, &root, source.as_deref(), all, cli.format)
        }
        Commands::Status => run_status(&store, cli.format),
        Commands::Hook { agent } => run_hook(&store, &root, agent),
        Commands::Init { apply, tool } => run_init(apply, tool.as_deref()),
        Commands::Migrate { dry_run, path } => run_migrate(&root, dry_run, path.as_deref()),
    }
}

fn run_hook(store: &Store, root: &Path, agent: HookAgent) -> Result<()> {
    match agent {
        HookAgent::Kimi => run_kimi_hook(store, root),
    }
}

fn run_init(apply: bool, tool_name: Option<&str>) -> Result<()> {
    if apply {
        let tool = match tool_name {
            Some(name) => parse_tool(name).context("unknown tool name"),
            None => {
                let detected = detect_tools();
                let configurable: Vec<_> = detected
                    .into_iter()
                    .filter(|(t, s)| t.supports_hooks() && !s.hook_already_configured)
                    .map(|(t, _)| t)
                    .collect();
                if configurable.is_empty() {
                    bail!("no supported tools found that need configuration");
                }
                if configurable.len() > 1 {
                    bail!(
                        "multiple tools detected; use --tool <name> to choose one of: {}",
                        configurable
                            .iter()
                            .map(|t| t.command())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                Ok(configurable[0])
            }
        }?;
        apply_tool(tool)
    } else {
        let detected = detect_tools();
        print_detected(&detected);
        Ok(())
    }
}

fn default_doc_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut docs = Vec::new();
    if root.join("AGENTS.md").exists() {
        docs.push(root.join("AGENTS.md"));
    }
    if root.join("README.md").exists() {
        docs.push(root.join("README.md"));
    }
    if root.join("docs").is_dir() {
        docs.extend(find_markdown_files(root.join("docs"))?);
    }
    Ok(docs)
}

fn resolve_doc_path(root: &Path, path: Option<&Path>) -> Result<Vec<PathBuf>> {
    match path {
        Some(p) => {
            let abs = root.join(p);
            if abs.is_dir() {
                Ok(find_markdown_files(&abs)?)
            } else {
                Ok(vec![abs])
            }
        }
        None => default_doc_paths(root),
    }
}

fn run_scan(store: &Store, root: &Path, path: Option<&Path>, format: OutputFormat) -> Result<()> {
    let docs = resolve_doc_path(root, path)?;

    let mut total_refs = 0usize;
    for doc in &docs {
        let refs = parse_document(doc)?;
        total_refs += refs.len();
        store.record_scan(doc, &refs)?;
    }

    match format {
        OutputFormat::Text => {
            println!(
                "Scanned {} document(s), indexed {} source reference(s).",
                docs.len(),
                total_refs
            );
        }
        OutputFormat::Json => {
            let out = ScanOutput {
                scanned_docs: docs.len(),
                indexed_refs: total_refs,
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
    }

    Ok(())
}

fn run_check(
    store: &Store,
    root: &Path,
    source: Option<&Path>,
    all: bool,
    format: OutputFormat,
) -> Result<()> {
    let sources: Vec<PathBuf> = match (source, all) {
        (Some(s), _) => vec![s.to_path_buf()],
        (None, true) => Vec::new(),
        (None, false) => {
            anyhow::bail!("either --source <path> or --all is required for check")
        }
    };

    let summary = check_drift(store, root, &sources)?;

    match format {
        OutputFormat::Text => print_text_summary(&summary),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&summary)?),
    }

    if !summary.issues.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn run_status(store: &Store, format: OutputFormat) -> Result<()> {
    let docs = store.get_doc_paths()?;
    let snippets = store.get_all_snippets()?;
    let drift_count = snippets
        .iter()
        .filter(|s| s.drift_detected_at.is_some())
        .count();

    match format {
        OutputFormat::Text => {
            println!("Indexed documents: {}", docs.len());
            println!("Indexed snippets:  {}", snippets.len());
            println!("Snippets with drift: {}", drift_count);
        }
        OutputFormat::Json => {
            let out = StatusOutput {
                indexed_docs: docs.len(),
                indexed_snippets: snippets.len(),
                drift_count,
            };
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
    }
    Ok(())
}

fn print_text_summary(summary: &CheckSummary) {
    println!("Checked {} reference(s).", summary.scanned_refs);

    if !summary.issues.is_empty() {
        println!("Found {} issue(s):", summary.issues.len());
        for issue in &summary.issues {
            print_issue(issue);
        }
    }

    if !summary.warnings.is_empty() {
        println!("Found {} warning(s):", summary.warnings.len());
        for warning in &summary.warnings {
            print_issue(warning);
        }
    }

    if summary.issues.is_empty() && summary.warnings.is_empty() {
        println!("No drift detected.");
    }
}

fn print_issue(issue: &crate::types::DriftIssue) {
    let r = &issue.reference;
    println!();
    println!("  Doc:  {}", r.doc_path.display());
    println!(
        "  Ref:  {} ~line {} — {}",
        r.source_path.display(),
        r.doc_line,
        r.item_name
    );
    if let Some(cur) = issue.current_line {
        println!("  Now:  ~line {}", cur);
    }
    println!("  Msg:  {}", issue.message);
}

fn run_migrate(root: &Path, dry_run: bool, path: Option<&Path>) -> Result<()> {
    let docs = resolve_doc_path(root, path)?;

    println!(
        "Scanning {} document(s) for unannotated code blocks...",
        docs.len()
    );
    let blocks = find_unmatched_blocks(&docs)?;
    println!(
        "Found {} code block(s) without source-location comments.",
        blocks.len()
    );

    if blocks.is_empty() {
        println!("Nothing to migrate.");
        return Ok(());
    }

    println!("Building source index...");
    let index = build_source_index(root)?;
    println!("Indexed {} unique line(s) from source files.", index.len());

    let proposals = propose_locations(&blocks, &index);

    let exact: Vec<_> = proposals
        .iter()
        .filter(|p| p.confidence == Confidence::Exact)
        .collect();
    let sig: Vec<_> = proposals
        .iter()
        .filter(|p| p.confidence == Confidence::Signature)
        .collect();
    let none: Vec<_> = proposals
        .iter()
        .filter(|p| p.confidence == Confidence::None)
        .collect();

    println!();
    println!("Migration summary:");
    println!("  Exact matches:      {}", exact.len());
    println!("  Signature matches:  {}", sig.len());
    println!("  Needs manual work:  {}", none.len());
    println!();

    if !exact.is_empty() {
        println!("Exact matches (will be auto-annotated):");
        for p in &exact {
            println!(
                "  {}:{} → {} ~line {} — {}",
                p.block.doc_path.display(),
                p.block.block_start_line,
                p.source_path.display(),
                p.source_line,
                p.item_name
            );
        }
        println!();
    }

    if !sig.is_empty() {
        println!("Signature matches (will be auto-annotated):");
        for p in &sig {
            println!(
                "  {}:{} → {} ~line {} — {}",
                p.block.doc_path.display(),
                p.block.block_start_line,
                p.source_path.display(),
                p.source_line,
                p.item_name
            );
        }
        println!();
    }

    if !none.is_empty() {
        println!("Blocks that need manual annotation (or mark as demo):");
        for p in &none {
            let first = p
                .block
                .lines
                .iter()
                .find(|l| !l.trim().is_empty())
                .map(|s| s.trim())
                .unwrap_or("(empty)");
            println!(
                "  {}:{}  first line: {}",
                p.block.doc_path.display(),
                p.block.block_start_line,
                truncate(first, 60)
            );
        }
        println!();
        println!("For teaching examples / pseudo-code that have no source counterpart,");
        println!("add a demo marker as the first line of the code block:");
        println!("  // (demo)        — Rust, C, JS, Go...");
        println!("  # (example)      — Python, Bash, Ruby...");
        println!("  -- (teaching)    — SQL, Haskell...");
        println!();
    }

    let all_auto: Vec<_> = exact
        .iter()
        .chain(sig.iter())
        .map(|p| (*p).clone())
        .collect();
    if all_auto.is_empty() {
        println!("No auto-annotatable blocks found.");
        return Ok(());
    }

    let changes = apply_proposals(&all_auto, dry_run)?;

    if dry_run {
        println!(
            "Dry run complete. {} change(s) would be made.",
            changes.len()
        );
        println!("Run without --dry-run to apply.");
    } else {
        println!("Applied {} source-location comment(s).", changes.len());
        println!("Run 'docref scan' to index the updated docs.");
    }

    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
