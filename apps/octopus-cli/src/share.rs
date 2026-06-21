use std::path::PathBuf;

pub fn get_share_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("KIMI_SHARE_DIR") {
        let path = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&path);
        return path;
    }
    let path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kimi");
    let _ = std::fs::create_dir_all(&path);
    path
}

pub fn get_logs_dir() -> PathBuf {
    get_share_dir().join("logs")
}

pub fn get_history_dir() -> PathBuf {
    get_share_dir().join("user-history")
}

pub fn get_telemetry_dir() -> PathBuf {
    let path = get_share_dir().join("telemetry");
    let _ = std::fs::create_dir_all(&path);
    path
}
