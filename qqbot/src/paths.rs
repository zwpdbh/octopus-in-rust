use std::path::{Path, PathBuf};

/// Return the project/workspace root directory.
///
/// When the binary lives under `target/{profile}/`, the root is two levels
/// above the executable's directory. Otherwise the executable's own directory
/// is used as a fallback (useful for installed binaries).
pub fn project_root() -> PathBuf {
    let exe = std::env::current_exe().ok();
    if let Some(exe) = exe {
        let dir = exe.parent().map(Path::to_path_buf);
        // If the binary is in target/{profile}/, go up to the workspace root.
        if let Some(ref d) = dir {
            let grandparent = d.parent().and_then(|p| p.parent()).map(Path::to_path_buf);
            if d.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "debug" || n == "release")
                .unwrap_or(false)
                && d.parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    == Some("target")
            {
                return grandparent.unwrap_or(d.clone());
            }
        }
        return dir.unwrap_or_else(|| PathBuf::from("."));
    }
    PathBuf::from(".")
}

/// Resolve a path relative to the project root if it is relative.
/// Absolute paths are returned unchanged. The result is normalized to remove
/// redundant `.` and `..` components.
pub fn resolve<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project_root().join(path)
    };
    normalize(&joined)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::Prefix(p) => out.push(p.as_os_str()),
            std::path::Component::RootDir => out.push("/"),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::Normal(c) => out.push(c),
        }
    }
    out
}
