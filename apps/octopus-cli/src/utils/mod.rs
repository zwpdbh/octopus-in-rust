pub mod clipboard;

pub mod path {
    use std::path::{Path, PathBuf};

    pub fn get_share_dir() -> PathBuf {
        crate::share::get_share_dir()
    }

    pub fn is_within_workspace(path: &Path, work_dir: &Path, additional_dirs: &[PathBuf]) -> bool {
        if path.starts_with(work_dir) {
            return true;
        }
        for dir in additional_dirs {
            if path.starts_with(dir) {
                return true;
            }
        }
        false
    }
}

pub mod datetime {
    pub fn now() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }
}

pub mod term {
    pub fn ensure_tty_sane() {
        // TODO: restore terminal state
    }
}

pub mod logging {
    pub fn redirect_stderr_to_logger() {
        // TODO: implement stderr redirection
    }

    pub fn restore_stderr() {
        // TODO: implement stderr restoration
    }

    pub fn open_original_stderr() -> Option<std::fs::File> {
        None
    }
}
