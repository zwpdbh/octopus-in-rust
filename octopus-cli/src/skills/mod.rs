use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Discover skills from the given directories.
    ///
    /// Two layouts are supported:
    /// 1. Subdirectory form: `<dir>/<name>/SKILL.md`
    /// 2. Flat form: `<dir>/<name>.md` (but not `SKILL.md` itself)
    pub fn discover(&mut self, dirs: &[PathBuf]) {
        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            self._discover_dir(dir);
        }
    }

    fn _discover_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to read skills directory {}: {}", dir.display(), e);
                return;
            }
        };

        let mut discovered: HashMap<String, Skill> = HashMap::new();

        // Pass 1: subdirectory form (canonical)
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() {
                continue;
            }
            match Self::_parse_skill_file(&skill_md, &path) {
                Some(skill) => {
                    let key = normalize_skill_name(&skill.name);
                    discovered.insert(key, skill);
                }
                None => {
                    tracing::debug!("Skipping invalid skill at {}", skill_md.display());
                }
            }
        }

        // Pass 2: flat .md form, skipping names already claimed by subdir
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.to_lowercase().ends_with(".md") {
                continue;
            }
            if name.to_uppercase() == "SKILL.MD" {
                continue;
            }

            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s,
                None => continue,
            };
            let key = normalize_skill_name(stem);
            if discovered.contains_key(&key) {
                continue; // subdirectory form wins
            }

            match Self::_parse_skill_file(&path, dir) {
                Some(skill) => {
                    discovered.insert(key, skill);
                }
                None => {
                    tracing::debug!("Skipping invalid skill at {}", path.display());
                }
            }
        }

        self.skills.extend(discovered);
    }

    fn _parse_skill_file(skill_md: &Path, skill_dir: &Path) -> Option<Skill> {
        let content = std::fs::read_to_string(skill_md).ok()?;
        let frontmatter = parse_frontmatter(&content)?;
        let name = frontmatter.get("name")?.to_string();
        let description = frontmatter.get("description").cloned().unwrap_or_default();
        Some(Skill {
            name,
            description,
            path: skill_dir.to_path_buf(),
        })
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(&normalize_skill_name(name))
    }

    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }

    /// Format skills as a markdown list for system prompt injection.
    pub fn to_system_prompt(&self) -> String {
        if self.skills.is_empty() {
            return String::new();
        }
        let mut lines = vec!["## Available Skills".to_string()];
        for skill in self.skills.values() {
            lines.push(format!("- **{}**: {}", skill.name, skill.description));
        }
        lines.join("\n")
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse simple YAML-like frontmatter from markdown content.
/// Returns a map of key-value pairs.
///
/// Supports:
/// ```markdown
/// ---
/// name: My Skill
/// description: Does something useful
/// ---
///
/// # Content
/// ```
fn parse_frontmatter(content: &str) -> Option<HashMap<String, String>> {
    if !content.trim_start().starts_with("---") {
        return None;
    }
    let after_first = &content[content.find("---")? + 3..];
    let end_pos = after_first.find("---")?;
    let frontmatter = after_first[..end_pos].trim();

    let mut map = HashMap::new();
    for line in frontmatter.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_string();
            let value = value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string();
            map.insert(key, value);
        }
    }

    if map.is_empty() { None } else { Some(map) }
}

fn normalize_skill_name(name: &str) -> String {
    name.to_lowercase().replace(' ', "_")
}
