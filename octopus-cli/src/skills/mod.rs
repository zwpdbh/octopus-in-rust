use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub path: PathBuf,
}

pub struct SkillRegistry {
    _skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            _skills: HashMap::new(),
        }
    }

    pub fn discover(&mut self, _dirs: &[PathBuf]) {
        // TODO: implement skill discovery
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}
