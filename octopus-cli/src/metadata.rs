use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::share::get_share_dir;

fn get_metadata_file() -> PathBuf {
    get_share_dir().join("kimi.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkDirMeta {
    pub path: String,
    pub kaos: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_session_id: Option<String>,
}

impl WorkDirMeta {
    pub fn sessions_dir(&self) -> PathBuf {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.path.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());

        let dir_basename = if self.kaos == "local" {
            hash
        } else {
            format!("{}_{}", self.kaos, hash)
        };

        let dir = get_share_dir().join("sessions").join(dir_basename);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub work_dirs: Vec<WorkDirMeta>,
}

impl Metadata {
    pub fn get_work_dir_meta<P: AsRef<Path>>(&self, work_dir: P) -> Option<&WorkDirMeta> {
        let path = work_dir
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| work_dir.as_ref().to_path_buf());
        let path_str = path.to_string_lossy().to_string();
        self.work_dirs
            .iter()
            .find(|wd| wd.path == path_str && wd.kaos == "local")
    }

    pub fn get_work_dir_meta_mut<P: AsRef<Path>>(
        &mut self,
        work_dir: P,
    ) -> Option<&mut WorkDirMeta> {
        let path = work_dir
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| work_dir.as_ref().to_path_buf());
        let path_str = path.to_string_lossy().to_string();
        self.work_dirs
            .iter_mut()
            .find(|wd| wd.path == path_str && wd.kaos == "local")
    }

    pub fn new_work_dir_meta<P: AsRef<Path>>(&mut self, work_dir: P) -> &WorkDirMeta {
        let path = work_dir
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|_| work_dir.as_ref().to_path_buf());
        let meta = WorkDirMeta {
            path: path.to_string_lossy().to_string(),
            kaos: "local".to_string(),
            last_session_id: None,
        };
        self.work_dirs.push(meta);
        self.work_dirs.last().unwrap()
    }
}

pub fn load_metadata() -> Metadata {
    let path = get_metadata_file();
    if path.exists() {
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Metadata::default(),
        }
    } else {
        Metadata::default()
    }
}

pub fn save_metadata(metadata: &Metadata) {
    let path = get_metadata_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(content) = serde_json::to_string_pretty(metadata) {
        let _ = std::fs::write(path, content);
    }
}
