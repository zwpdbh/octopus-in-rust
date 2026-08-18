//! Relative-path validation shared by server (upload paths) and client
//! (manifest paths before writing to disk).

use crate::error::{Error, Result};

/// Validate that `path` is a safe, forward-slash relative path:
///
/// - non-empty, no leading/trailing slash, no backslashes
/// - no absolute prefix (`/`, `C:\`, UNC)
/// - no `.` / `..` components
/// - no NUL or control characters
///
/// Both sides must run this before joining the path onto a base directory so
/// a malicious or buggy peer cannot escape the gamedata root.
pub fn validate_relative_path(path: &str) -> Result<()> {
    let reject = |reason: &str| Error::InvalidPath {
        path: path.to_string(),
        reason: reason.to_string(),
    };

    if path.is_empty() {
        return Err(reject("path is empty"));
    }
    if path.starts_with('/') || path.ends_with('/') {
        return Err(reject("leading or trailing slash"));
    }
    if path.contains('\\') {
        return Err(reject("backslash separators are not allowed"));
    }
    if path.chars().any(|c| c.is_control()) {
        return Err(reject("control characters are not allowed"));
    }
    for component in path.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            return Err(reject("empty, `.`, or `..` path component"));
        }
        if component.ends_with(':') {
            return Err(reject("drive-letter component"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_paths() {
        for p in ["faf.scd", "init/lua.nxt", "loc_cn.scd", "a/b/c.dat"] {
            assert!(validate_relative_path(p).is_ok(), "should accept {p}");
        }
    }

    #[test]
    fn rejects_unsafe_paths() {
        for p in [
            "",
            "/etc/passwd",
            "../evil",
            "a/../../b",
            "foo/",
            "foo\\bar",
            "C:/Windows",
            "a//b",
            "./x",
        ] {
            assert!(validate_relative_path(p).is_err(), "should reject {p:?}");
        }
    }
}
