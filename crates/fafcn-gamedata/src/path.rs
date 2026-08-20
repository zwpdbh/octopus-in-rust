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

/// Percent-encode a manifest relative path for transport (URL paths and the
/// `x-gamedata-path` upload header), preserving `/` separators. Everything
/// outside `[A-Za-z0-9.\-_]` is `%XX`-encoded per UTF-8 byte, so non-ASCII
/// names (e.g. Cyrillic map files) survive ASCII-only transports.
pub fn encode_relative_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_segment(segment: &str) -> String {
    let mut out = String::new();
    for b in segment.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
            out.push(*b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode a path produced by [`encode_relative_path`], then validate the
/// decoded result with [`validate_relative_path`].
pub fn decode_relative_path(encoded: &str) -> Result<String> {
    let reject = |reason: &str| Error::InvalidPath {
        path: encoded.to_string(),
        reason: reason.to_string(),
    };
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = encoded
                .get(i + 1..i + 3)
                .ok_or_else(|| reject("truncated percent-encoding"))?;
            let byte =
                u8::from_str_radix(hex, 16).map_err(|_| reject("invalid percent-encoding"))?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let decoded =
        String::from_utf8(out).map_err(|_| reject("percent-decoded path is not UTF-8"))?;
    validate_relative_path(&decoded)?;
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_preserves_simple_paths() {
        assert_eq!(encode_relative_path("faf.scd"), "faf.scd");
        assert_eq!(encode_relative_path("init/lua.nxt"), "init/lua.nxt");
    }

    #[test]
    fn encode_escapes_special_chars() {
        assert_eq!(encode_relative_path("my mod.scd"), "my%20mod.scd");
        assert!(encode_relative_path("地图/faf.scd").contains('/'));
        assert!(!encode_relative_path("地图.scd").contains('图'));
    }

    #[test]
    fn decode_roundtrip() {
        for p in [
            "faf.scd",
            "init/lua.nxt",
            "my mod.v0001/1 — копия — копия.dds",
            "地图/faf.scd",
        ] {
            let decoded = decode_relative_path(&encode_relative_path(p)).unwrap();
            assert_eq!(decoded, p);
        }
    }

    #[test]
    fn decode_rejects_malformed() {
        assert!(decode_relative_path("a%2").is_err());
        assert!(decode_relative_path("a%zz").is_err());
        // `%2E%2E` decodes to `..` — rejected by relative-path validation.
        assert!(decode_relative_path("%2E%2E/evil").is_err());
        // Raw `..` sneaking past encoding is still rejected.
        assert!(decode_relative_path("../evil").is_err());
    }

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
