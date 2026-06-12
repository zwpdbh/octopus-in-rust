use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A code block in a markdown document that lacks a source-location comment.
#[derive(Debug, Clone)]
pub struct UnmatchedBlock {
    pub doc_path: PathBuf,
    pub block_start_line: usize, // 1-based line of ```
    pub lang: String,
    pub lines: Vec<String>,
}

/// A proposed source-location comment for an unmatched block.
#[derive(Debug, Clone)]
pub struct Proposal {
    pub block: UnmatchedBlock,
    pub source_path: PathBuf,
    pub source_line: usize,
    pub item_name: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Exact,     // first line(s) matched exactly in one source file
    Signature, // parsed item name found in one source file
    Ambiguous, // multiple matches
    None,      // no match
}

/// Programming language supported by the migration heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Language {
    Rust,
    Python,
    Go,
    JavaScript,
    TypeScript,
    Java,
    C,
    Cpp,
}

impl Language {
    /// Parse a markdown fence language tag into a known language.
    fn from_str(s: &str) -> Option<Language> {
        match s {
            "rust" | "rs" => Some(Language::Rust),
            "python" | "py" => Some(Language::Python),
            "go" => Some(Language::Go),
            "javascript" | "js" => Some(Language::JavaScript),
            "typescript" | "ts" => Some(Language::TypeScript),
            "java" => Some(Language::Java),
            "c" => Some(Language::C),
            "cpp" | "c++" | "h" | "hpp" => Some(Language::Cpp),
            _ => None,
        }
    }

    /// Regexes that extract an item name from the first meaningful line of code.
    fn item_patterns(&self) -> &'static Vec<Regex> {
        match self {
            Language::Rust => rust_patterns(),
            Language::Python => python_patterns(),
            Language::Go => go_patterns(),
            Language::JavaScript | Language::TypeScript => js_patterns(),
            Language::Java => java_patterns(),
            Language::C | Language::Cpp => c_patterns(),
        }
    }

    /// File extensions that correspond to this language.
    fn file_extensions(&self) -> &'static [&'static str] {
        match self {
            Language::Rust => &["rs"],
            Language::Python => &["py"],
            Language::Go => &["go"],
            Language::JavaScript => &["js"],
            Language::TypeScript => &["ts", "tsx"],
            Language::Java => &["java"],
            Language::C => &["c", "h"],
            Language::Cpp => &["cpp", "hpp", "cc", "cxx"],
        }
    }
}

macro_rules! define_patterns {
    ($fn_name:ident, $($pat:expr),+ $(,)?) => {
        fn $fn_name() -> &'static Vec<Regex> {
            static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
            PATTERNS.get_or_init(|| vec![$(Regex::new($pat).unwrap()),+])
        }
    };
}

define_patterns!(
    rust_patterns,
    r"(?:pub\s+)?(?:async\s+)?fn\s+(\w+)",
    r"(?:pub\s+)?struct\s+(\w+)",
    r"(?:pub\s+)?enum\s+(\w+)",
    r"(?:pub\s+)?trait\s+(\w+)",
    r"impl\s+(?:<[^>]+>\s+)?(\w+)",
);

define_patterns!(python_patterns, r"def\s+(\w+)", r"class\s+(\w+)",);

define_patterns!(
    go_patterns,
    r"func\s+(?:\([^)]+\)\s+)?(\w+)",
    r"type\s+(\w+)",
);

define_patterns!(
    js_patterns,
    r"function\s+(\w+)",
    r"class\s+(\w+)",
    r"const\s+(\w+)",
);

define_patterns!(
    java_patterns,
    r"(?:public\s+|private\s+|protected\s+)?(?:static\s+)?(?:final\s+)?(?:<[^>]+>\s+)?\w+(?:<[^>]+>)?\s+(\w+)\s*\(",
    r"class\s+(\w+)",
);

define_patterns!(
    c_patterns,
    r"\w+(?:\s+\w+)*\s+(\w+)\s*\(",
    r"struct\s+(\w+)",
);

/// A marker found inside a code block that determines migration eligibility.
///
/// Source-location comments and demo markers are mutually exclusive in our
/// domain model: a block is either managed code (maps to a source file) or
/// a teaching example (invented for documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    /// Found a source-location comment at this line.
    SourceRef { at_line: usize },
    /// Found a demo / example marker at this line.
    Demo { at_line: usize },
}

/// Scan markdown documents and find code blocks without source-location comments.
pub fn find_unmatched_blocks(docs: &[PathBuf]) -> Result<Vec<UnmatchedBlock>> {
    let source_re = Regex::new(r"^\s*(?://|#|\*|--|<!--|;|;;|\(\*|%)\s+(\S+)\s+~line\s+(\d+)")
        .expect("invalid source ref regex");

    let demo_re = Regex::new(
        r"^\s*(?://|#|\*|--|<!--|;|;;|\(\*|%)\s+\(demo|example|pseudo-code|teaching\)\s*$",
    )
    .expect("invalid demo regex");

    let mut blocks = Vec::new();

    for doc in docs {
        let content =
            fs::read_to_string(doc).with_context(|| format!("failed to read {}", doc.display()))?;
        let lines: Vec<&str> = content.lines().collect();

        let mut inside_code_block = false;
        let mut current_lang = String::new();
        let mut current_lines: Vec<String> = Vec::new();
        let mut block_start = 0usize;
        let mut marker: Option<Marker> = None;

        for (idx, line) in lines.iter().enumerate() {
            let line_no = idx + 1;

            if line.trim_start().starts_with("```") {
                if inside_code_block {
                    // End of block
                    if !current_lang.is_empty() && !current_lines.is_empty() && marker.is_none() {
                        blocks.push(UnmatchedBlock {
                            doc_path: doc.clone(),
                            block_start_line: block_start,
                            lang: current_lang.clone(),
                            lines: current_lines.clone(),
                        });
                    }
                    inside_code_block = false;
                    current_lang.clear();
                    current_lines.clear();
                    marker = None;
                } else {
                    // Start of block
                    inside_code_block = true;
                    block_start = line_no;
                    current_lang = line
                        .trim_start()
                        .strip_prefix("```")
                        .unwrap_or("")
                        .trim()
                        .to_string();
                }
                continue;
            }

            if inside_code_block {
                current_lines.push(line.to_string());
                if source_re.is_match(line) {
                    marker = Some(Marker::SourceRef { at_line: line_no });
                }
                if demo_re.is_match(line) && marker.is_none() {
                    marker = Some(Marker::Demo { at_line: line_no });
                }
            }
        }
    }

    Ok(blocks)
}

/// Build an index of source file lines for fast lookup.
pub fn build_source_index(project_root: &Path) -> Result<HashMap<String, Vec<(PathBuf, usize)>>> {
    let mut index: HashMap<String, Vec<(PathBuf, usize)>> = HashMap::new();

    let code_extensions = [
        "rs", "py", "js", "ts", "go", "java", "c", "cpp", "h", "hpp", "rb", "sh", "pl", "lua",
        "hs", "erl", "ex", "exs", "ml", "clj", "cljs", "scala", "kt", "swift",
    ];

    for entry in walkdir::WalkDir::new(project_root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        // Skip common non-source directories
        let relative = path.strip_prefix(project_root).unwrap_or(path);
        let skip_dirs = ["target", "node_modules", ".git", "dist", "build", "vendor"];
        if relative
            .components()
            .any(|c| skip_dirs.iter().any(|s| c.as_os_str() == *s))
        {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !code_extensions.contains(&ext) {
            continue;
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue, // binary or unreadable
        };

        let rel_path = relative.to_path_buf();
        for (line_no, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.len() > 500 {
                continue;
            }
            let key = trimmed.to_string();
            index
                .entry(key)
                .or_default()
                .push((rel_path.clone(), line_no + 1));
        }
    }

    Ok(index)
}

/// Try to find a source location for each unmatched block.
pub fn propose_locations(
    blocks: &[UnmatchedBlock],
    index: &HashMap<String, Vec<(PathBuf, usize)>>,
) -> Vec<Proposal> {
    let mut proposals = Vec::new();

    for block in blocks {
        let proposal = match_single_block(block, index);
        proposals.push(proposal);
    }

    proposals
}

fn match_single_block(
    block: &UnmatchedBlock,
    index: &HashMap<String, Vec<(PathBuf, usize)>>,
) -> Proposal {
    // 1. Try exact match of first non-empty, non-comment line
    let meaningful: Vec<&str> = block
        .lines
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !looks_like_comment(s, &block.lang))
        .collect();

    if meaningful.is_empty() {
        return Proposal {
            block: block.clone(),
            source_path: PathBuf::new(),
            source_line: 0,
            item_name: String::new(),
            confidence: Confidence::None,
        };
    }

    // Try first line exact
    let first = meaningful[0];
    if let Some(matches) = index.get(first) {
        if matches.len() == 1 {
            let (path, line) = &matches[0];
            let item = guess_item_name(first, &block.lang);
            return Proposal {
                block: block.clone(),
                source_path: path.clone(),
                source_line: *line,
                item_name: item,
                confidence: Confidence::Exact,
            };
        }

        // Ambiguous: try first 3 lines combined
        let combined: String = meaningful
            .iter()
            .take(3)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        let combined_matches: Vec<_> = matches
            .iter()
            .filter(|(p, l)| {
                // Check if lines l, l+1, l+2 in source match meaningful[0..3]
                let Some(source_content) = fs::read_to_string(p).ok() else {
                    return false;
                };
                let source_lines: Vec<&str> = source_content.lines().collect();
                let start = l.saturating_sub(1);
                let end = (start + 3).min(source_lines.len());
                let slice = source_lines[start..end].join("\n");
                slice == combined
            })
            .collect();

        if combined_matches.len() == 1 {
            let (path, line) = combined_matches[0];
            let item = guess_item_name(first, &block.lang);
            return Proposal {
                block: block.clone(),
                source_path: path.clone(),
                source_line: *line,
                item_name: item,
                confidence: Confidence::Exact,
            };
        }
    }

    // 2. Try signature match (parse item name from first line)
    let item = guess_item_name(first, &block.lang);
    if !item.is_empty() {
        // Search index for lines that contain the item name as a word
        let candidates: Vec<_> = index
            .iter()
            .filter(|(k, _)| k.contains(&item))
            .flat_map(|(_, v)| v.iter())
            .filter(|(p, _)| language_matches_extension(&block.lang, p))
            .cloned()
            .collect();

        if candidates.len() == 1 {
            let (path, line) = &candidates[0];
            return Proposal {
                block: block.clone(),
                source_path: path.clone(),
                source_line: *line,
                item_name: item,
                confidence: Confidence::Signature,
            };
        }
    }

    Proposal {
        block: block.clone(),
        source_path: PathBuf::new(),
        source_line: 0,
        item_name: String::new(),
        confidence: Confidence::None,
    }
}

fn looks_like_comment(line: &str, _lang: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//")
        || trimmed.starts_with('#')
        || trimmed.starts_with("/*")
        || trimmed.starts_with("*")
        || trimmed.starts_with("<!--")
}

fn guess_item_name(first_line: &str, lang: &str) -> String {
    let trimmed = first_line.trim_start();

    let Some(language) = Language::from_str(lang) else {
        return String::new();
    };

    for re in language.item_patterns() {
        if let Some(m) = re.captures(trimmed).and_then(|caps| caps.get(1)) {
            return m.as_str().to_string();
        }
    }

    String::new()
}

fn language_matches_extension(lang: &str, path: &Path) -> bool {
    let Some(language) = Language::from_str(lang) else {
        return true;
    };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    language.file_extensions().contains(&ext)
}

/// Apply proposals to markdown documents.
pub fn apply_proposals(
    proposals: &[Proposal],
    dry_run: bool,
) -> Result<Vec<(PathBuf, usize, String)>> {
    // Group proposals by document
    let mut by_doc: HashMap<PathBuf, Vec<&Proposal>> = HashMap::new();
    for p in proposals
        .iter()
        .filter(|p| p.confidence == Confidence::Exact || p.confidence == Confidence::Signature)
    {
        by_doc.entry(p.block.doc_path.clone()).or_default().push(p);
    }

    let mut changes = Vec::new();

    for (doc_path, doc_proposals) in by_doc {
        let content = fs::read_to_string(&doc_path)?;
        let lines: Vec<&str> = content.lines().collect();
        let mut new_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();

        // Sort by line number descending so inserting doesn't shift later indices
        let mut sorted = doc_proposals.clone();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.block.block_start_line));

        for p in sorted {
            let insert_idx = p.block.block_start_line; // 0-based: this is the ``` line, insert after it
            if insert_idx >= new_lines.len() {
                continue;
            }

            let comment_prefix = match guess_comment_prefix(&p.block.lang) {
                Some(c) => c,
                None => continue,
            };

            let annotation = if p.block.lines.len() < 3 {
                " (abbreviated)"
            } else {
                ""
            };

            let source_line = if p.source_line > 0 {
                p.source_line.to_string()
            } else {
                "?".to_string()
            };
            let source_comment = format!(
                "{} {} ~line {} — {}{}",
                comment_prefix,
                p.source_path.display(),
                source_line,
                p.item_name,
                annotation
            );

            changes.push((doc_path.clone(), insert_idx + 1, source_comment.clone()));

            if !dry_run {
                new_lines.insert(insert_idx + 1, source_comment);
            }
        }

        if !dry_run {
            fs::write(&doc_path, new_lines.join("\n"))
                .with_context(|| format!("failed to write {}", doc_path.display()))?;
        }
    }

    Ok(changes)
}

fn guess_comment_prefix(lang: &str) -> Option<&'static str> {
    let language = Language::from_str(lang);
    match language {
        Some(
            Language::Rust
            | Language::C
            | Language::Cpp
            | Language::Go
            | Language::Java
            | Language::JavaScript
            | Language::TypeScript,
        ) => Some("//"),
        Some(Language::Python) => Some("#"),
        None => match lang {
            "swift" | "kotlin" | "kt" | "scala" => Some("//"),
            "ruby" | "rb" | "sh" | "bash" | "yaml" | "yml" | "perl" | "pl" | "r" | "makefile"
            | "dockerfile" => Some("#"),
            "sql" | "lua" | "haskell" | "hs" => Some("--"),
            "html" | "xml" | "svg" | "markdown" | "md" => Some("<!--"),
            "erlang" | "prolog" | "matlab" | "tex" | "latex" => Some("%"),
            "lisp" | "clojure" | "clj" | "cljs" | "ini" | "asm" => Some(";"),
            "scheme" | "elisp" | "emacs-lisp" => Some(";;"),
            "ocaml" | "ml" | "pascal" | "pas" => Some("(*"),
            _ => Some("//"),
        },
    }
}
