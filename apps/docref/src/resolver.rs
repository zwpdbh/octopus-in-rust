use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::Path;

/// Find the current line of an item in a source file.
///
/// Returns `Ok(Some(line))` if found, `Ok(None)` if not found.
/// Line numbers are 1-based to match human conventions.
pub fn find_item_line<P: AsRef<Path>>(source_path: P, item_name: &str) -> Result<Option<usize>> {
    let content = fs::read_to_string(&source_path)
        .with_context(|| format!("failed to read {}", source_path.as_ref().display()))?;

    if item_name.is_empty() {
        return Ok(None);
    }

    let ext = source_path
        .as_ref()
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match ext {
        "rs" => find_rust_item_line(&content, item_name),
        "py" => find_python_item_line(&content, item_name),
        "js" | "ts" => find_js_item_line(&content, item_name),
        "go" => find_go_item_line(&content, item_name),
        _ => find_generic_item_line(&content, item_name),
    }
}

fn find_rust_item_line(content: &str, item_name: &str) -> Result<Option<usize>> {
    // Handle qualified names like "HookEngine::trigger" or "KimiToolset::requires_approval".
    // For MVP we search for the final identifier and assume the doc author got the context right.
    let simple_name = item_name.split("::").last().unwrap_or(item_name);

    // Patterns:
    //   pub fn item_name
    //   fn item_name
    //   pub struct item_name
    //   struct item_name
    //   pub enum item_name
    //   enum item_name
    //   pub trait item_name
    //   trait item_name
    //   impl ... for item_name
    //   impl item_name
    //   pub type item_name
    //   type item_name
    //   pub const item_name
    //   const item_name
    //   pub static item_name
    //   static item_name
    //   mod item_name
    //   pub mod item_name
    let patterns = [
        // fn / async fn / pub fn / pub async fn / pub(crate) async fn
        format!(
            r"(?m)^[^\S\r\n]*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+{0}\b",
            regex::escape(simple_name)
        ),
        format!(
            r"(?m)^[^\S\r\n]*(?:pub(?:\([^)]*\))?\s+)?(?:struct|enum|trait|type|const|static|mod)\s+{0}\b",
            regex::escape(simple_name)
        ),
        format!(
            r"(?m)^[^\S\r\n]*impl\s+(?:<[^>]+>\s+)?{0}\b",
            regex::escape(simple_name)
        ),
        format!(
            r"(?m)^[^\S\r\n]*impl\s+.*\s+for\s+{0}\b",
            regex::escape(simple_name)
        ),
    ];

    for pat in &patterns {
        let re = Regex::new(pat)?;
        if let Some(m) = re.find(content) {
            return Ok(Some(line_number(content, m.start())));
        }
    }

    Ok(None)
}

fn find_python_item_line(content: &str, item_name: &str) -> Result<Option<usize>> {
    let pat = format!(
        r"(?m)^[^\S\r\n]*(?:async\s+)?def\s+{0}\b|^[^\S\r\n]*class\s+{0}\b",
        regex::escape(item_name)
    );
    let re = Regex::new(&pat)?;
    Ok(re.find(content).map(|m| line_number(content, m.start())))
}

fn find_js_item_line(content: &str, item_name: &str) -> Result<Option<usize>> {
    let pat = format!(
        r"(?m)^[^\S\r\n]*(?:export\s+)?(?:async\s+)?function\s+{0}\b|^[^\S\r\n]*(?:export\s+)?(?:class|const|let|var)\s+{0}\b|^[^\S\r\n]*{0}\s*[:=]\s*(?:async\s*)?\(",
        regex::escape(item_name)
    );
    let re = Regex::new(&pat)?;
    Ok(re.find(content).map(|m| line_number(content, m.start())))
}

fn find_go_item_line(content: &str, item_name: &str) -> Result<Option<usize>> {
    let pat = format!(
        r"(?m)^[^\S\r\n]*func\s+(?:\([^)]+\)\s+)?{0}\b|^[^\S\r\n]*type\s+{0}\b|^[^\S\r\n]*var\s+{0}\b|^[^\S\r\n]*const\s+{0}\b",
        regex::escape(item_name)
    );
    let re = Regex::new(&pat)?;
    Ok(re.find(content).map(|m| line_number(content, m.start())))
}

fn find_generic_item_line(content: &str, item_name: &str) -> Result<Option<usize>> {
    // Very permissive fallback.
    let pat = format!(
        r"(?m)^[^\S\r\n]*(?:pub\s+)?(?:function|func|def|fn|class|struct|interface|trait|enum|type|const|let|var)\s+{0}\b",
        regex::escape(item_name)
    );
    let re = Regex::new(&pat)?;
    Ok(re.find(content).map(|m| line_number(content, m.start())))
}

fn line_number(content: &str, byte_offset: usize) -> usize {
    content[..byte_offset]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_rust_fn() {
        let src = "\n\npub fn rebuild_index() {}\nfn other() {}\n";
        assert_eq!(find_rust_item_line(src, "rebuild_index").unwrap(), Some(3));
        assert_eq!(find_rust_item_line(src, "other").unwrap(), Some(4));
    }

    #[test]
    fn test_find_rust_struct() {
        let src = "pub struct HookEngine;\npub fn foo() {}\n";
        assert_eq!(find_rust_item_line(src, "HookEngine").unwrap(), Some(1));
    }

    #[test]
    fn test_find_rust_impl() {
        let src = "impl Clone for HookEngine {\n";
        assert_eq!(find_rust_item_line(src, "HookEngine").unwrap(), Some(1));
    }
}
