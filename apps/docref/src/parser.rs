use crate::db::checksum;
use crate::types::SourceReference;
use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

/// Parse a markdown document and extract source-location references.
pub fn parse_document<P: AsRef<Path>>(doc_path: P) -> Result<Vec<SourceReference>> {
    let content = fs::read_to_string(&doc_path)
        .with_context(|| format!("failed to read {}", doc_path.as_ref().display()))?;

    let mut refs = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let source_re = Regex::new(
        // Matches common line-comment styles inside code blocks:
        //   //    — Rust, C, C++, Java, JS, TS, Go, Swift, Kotlin
        //   #     — Python, Ruby, Bash, Perl, YAML, R, Makefile
        //   *     — Javadoc/Doxygen mid-block
        //   --    — SQL, Lua, Haskell
        //   <!--  — HTML, XML
        //   ;     — Lisp, Clojure, Assembly, Ini
        //   ;;    — Scheme, Emacs Lisp
        //   (*    — OCaml, Pascal
        //   %     — Prolog, Erlang, LaTeX, Matlab
        r"^\s*(?://|#|\*|--|<!--|;|;;|\(\*|%)\s+(\S+)\s+~line\s+(\d+)(?:\s+—\s+(.+))?\s*$",
    )
    .expect("invalid source ref regex");

    let mut inside_code_block = false;

    for (idx, line) in lines.iter().enumerate() {
        // Track code blocks so we only look for source refs inside them.
        if line.trim_start().starts_with("```") {
            inside_code_block = !inside_code_block;
            continue;
        }

        if !inside_code_block {
            continue;
        }

        if let Some(caps) = source_re.captures(line) {
            let source_path_raw = caps.get(1).unwrap().as_str();
            let doc_line: usize = caps
                .get(2)
                .unwrap()
                .as_str()
                .parse()
                .context("invalid line number in source-location comment")?;
            let rest = caps.get(3).map(|m| m.as_str().trim());

            let (item_name, annotation) = parse_item_annotation(rest);

            // Collect the snippet body: everything after the source-location comment
            // until the next source-location comment or the end of the code block.
            let mut body_lines: Vec<String> = Vec::new();
            for j in (idx + 1)..lines.len() {
                if lines[j].trim_start().starts_with("```") {
                    break;
                }
                if source_re.is_match(lines[j]) {
                    break;
                }
                body_lines.push(lines[j].to_string());
            }
            // Trim trailing blank lines for stable checksums.
            while body_lines
                .last()
                .map(|s| s.trim().is_empty())
                .unwrap_or(false)
            {
                body_lines.pop();
            }
            let snippet_body = body_lines.join("\n");

            refs.push(SourceReference {
                doc_path: doc_path.as_ref().to_path_buf(),
                source_path: PathBuf::from(source_path_raw),
                doc_line,
                item_name,
                annotation,
                snippet_body: snippet_body.clone(),
                snippet_checksum: checksum(&snippet_body),
            });
        }
    }

    Ok(refs)
}

fn parse_item_annotation(rest: Option<&str>) -> (String, Option<String>) {
    let mut rest = rest.unwrap_or("").trim();
    if rest.is_empty() {
        return (String::new(), None);
    }

    // Strip trailing comment terminators that may appear on the same line
    // as the annotation: OCaml/Pascal *)  HTML/XML -->  C-style block */
    for suffix in ["*)", "-->", "*/"] {
        if rest.ends_with(suffix) {
            rest = rest[..rest.len() - suffix.len()].trim_end();
            break;
        }
    }

    // "ItemName (abbreviated)" -> item=ItemName, annotation=abbreviated
    // "ItemName (private associated fn)" -> item=ItemName, annotation=private associated fn
    if let Some(open) = rest.rfind("(") {
        if rest.ends_with(")") {
            let item = rest[..open].trim().to_string();
            let ann = rest[open + 1..rest.len() - 1].trim().to_string();
            return (item, Some(ann));
        }
    }

    (rest.to_string(), None)
}

/// Discover markdown documents under a root directory.
pub fn find_markdown_files<P: AsRef<Path>>(root: P) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            files.push(path.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_simple_reference() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"# Docs

```rust
// src/foo.rs ~line 42 — bar
pub fn bar() {{}}
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_path, PathBuf::from("src/foo.rs"));
        assert_eq!(refs[0].doc_line, 42);
        assert_eq!(refs[0].item_name, "bar");
        assert_eq!(refs[0].annotation, None);
    }

    #[test]
    fn test_parse_with_annotation() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"```rust
// src/foo.rs ~line 10 — Widget (abbreviated)
struct Widget;
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].item_name, "Widget");
        assert_eq!(refs[0].annotation.as_deref(), Some("abbreviated"));
    }

    #[test]
    fn test_parse_python_hash() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"```python
# src/app.py ~line 7 — main
def main():
    pass
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_path, PathBuf::from("src/app.py"));
        assert_eq!(refs[0].item_name, "main");
    }

    #[test]
    fn test_parse_sql_dash_dash() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"```sql
-- schema/tables.sql ~line 15 — users_table
CREATE TABLE users (id INT PRIMARY KEY);
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_path, PathBuf::from("schema/tables.sql"));
        assert_eq!(refs[0].item_name, "users_table");
    }

    #[test]
    fn test_parse_lisp_semicolon() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"```clojure
; src/core.clj ~line 3 — greet
(defn greet [name] (str "Hello " name))
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_path, PathBuf::from("src/core.clj"));
        assert_eq!(refs[0].item_name, "greet");
    }

    #[test]
    fn test_parse_ocaml_parens_star() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"```ocaml
(* src/lib.ml ~line 5 — factorial *)
let rec factorial n = if n <= 1 then 1 else n * factorial (n - 1)
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_path, PathBuf::from("src/lib.ml"));
        assert_eq!(refs[0].item_name, "factorial");
    }

    #[test]
    fn test_parse_erlang_percent() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            r#"```erlang
% src/app.erl ~line 20 — start
start() -> ok.
```
"#
        )
        .unwrap();

        let refs = parse_document(file.path()).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].source_path, PathBuf::from("src/app.erl"));
        assert_eq!(refs[0].item_name, "start");
    }
}
