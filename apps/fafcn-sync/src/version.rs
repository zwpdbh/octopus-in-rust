//! Detect the FAF patch version from the local gamedata files.
//!
//! FAF stores the game version in `lua/version.lua` inside the `lua.nx2`
//! archive (which is a plain ZIP): `local Version = "3837"`. When this can
//! be read, the uploader must not type a version by hand.

use std::{io::Read, path::Path};

/// Read the FAF patch version from `<dir>/lua.nx2`, if present and parseable.
pub fn detect_patch_version(dir: &Path) -> Option<String> {
    let archive = dir.join("lua.nx2");
    let file = std::fs::File::open(archive).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name("lua/version.lua").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    parse_version_lua(&text)
}

/// Extract `Version` from the contents of `lua/version.lua`.
fn parse_version_lua(text: &str) -> Option<String> {
    const MARKER: &str = "local Version = \"";
    let start = text.find(MARKER)? + MARKER.len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    let version = &rest[..end];
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(version.to_string())
}

/// The newest map generator version found below the FAForever root
/// (`map_generator/MapGenerator_*.jar`).
pub fn detect_generator_version(faf_root: &Path) -> Option<String> {
    let mut versions: Vec<String> = std::fs::read_dir(faf_root.join("map_generator"))
        .ok()?
        .filter_map(|item| {
            let name = item.ok()?.file_name().to_string_lossy().into_owned();
            fafcn_gamedata::map_generator_jar_version(&name)
        })
        .collect();
    versions.sort_by(|a, b| {
        fafcn_gamedata::compare_version_strings(b, a).unwrap_or(std::cmp::Ordering::Equal)
    });
    versions.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const VERSION_LUA: &str = r#"
local GameType = "FAF"
local Commit = "82a2612d083deafb6bec99fc85a3f1aa6cbcfedb"
local Version = "3837"
function GetVersion()
    return Version
end
"#;

    #[test]
    fn parses_version_lua() {
        assert_eq!(parse_version_lua(VERSION_LUA).as_deref(), Some("3837"));
        assert_eq!(parse_version_lua("local Version = \"\""), None);
        assert_eq!(parse_version_lua("local Version = \"abc\""), None);
        assert_eq!(parse_version_lua("no version here"), None);
    }

    #[test]
    fn detects_from_nx2_zip() {
        let dir = std::env::temp_dir().join(format!("fafcn-ver-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("lua.nx2");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer.start_file("lua/version.lua", options).unwrap();
            writer.write_all(VERSION_LUA.as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        assert_eq!(detect_patch_version(&dir).as_deref(), Some("3837"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_archive_yields_none() {
        assert_eq!(detect_patch_version(Path::new("/nonexistent")), None);
    }

    #[test]
    fn detects_generator_version() {
        let dir = std::env::temp_dir().join(format!("fafcn-gen-test-{}", std::process::id()));
        let gen = dir.join("map_generator");
        std::fs::create_dir_all(&gen).unwrap();
        std::fs::write(gen.join("MapGenerator_1.22.0.jar"), b"a").unwrap();
        std::fs::write(gen.join("MapGenerator_1.22.1.jar"), b"b").unwrap();
        std::fs::write(gen.join("MapGenerator_1.9.9.jar"), b"c").unwrap();
        assert_eq!(detect_generator_version(&dir).as_deref(), Some("1.22.1"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
