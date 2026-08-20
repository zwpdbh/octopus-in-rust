//! Sync channel definitions shared by server, client, and web.
//!
//! Players only struggle to download specific things, so the mirror is
//! organized into well-known channels:
//!
//! - `gamedata` — only the big patch archives (`env.nx2`, `units.nx2`,
//!   `textures.nx2`), versioned by the FAF patch version from `lua.nx2`.
//! - `map-generator` — the newest few `MapGenerator_*.jar` files, versioned
//!   by the newest jar's version.
//! - `faf-client` — the client installer (mirror-only).
//! - `maps` — FAF maps (folders like `name.v0001`), synced into the FAF
//!   Client's `maps_and_mods/maps` folder instead of the FAForever folder.

/// Channel id for the gamedata patch archives.
pub const CHANNEL_GAMEDATA: &str = "gamedata";

/// Channel id for the map generator jars.
pub const CHANNEL_MAP_GENERATOR: &str = "map-generator";

/// Channel id for the FAF client installer (mirror-only: players download it
/// from the web page; it is NOT synced into the FAForever folder).
pub const CHANNEL_FAF_CLIENT: &str = "faf-client";

/// Channel id for FAF maps. Synced into the FAF Client's
/// `maps_and_mods/maps` folder, NOT the FAForever folder (hence absent from
/// [`SYNC_CHANNELS`]); uploads MERGE into the existing manifest.
pub const CHANNEL_MAPS: &str = "maps";

/// All known channel ids (rejected at the API boundary otherwise).
pub const CHANNELS: &[&str] = &[
    CHANNEL_GAMEDATA,
    CHANNEL_MAP_GENERATOR,
    CHANNEL_FAF_CLIENT,
    CHANNEL_MAPS,
];

/// Channels the sync client syncs into the FAForever folder.
pub const SYNC_CHANNELS: &[&str] = &[CHANNEL_GAMEDATA, CHANNEL_MAP_GENERATOR];

/// The only gamedata files players actually need mirrored.
pub const GAMEDATA_SYNC_FILES: &[&str] = &["env.nx2", "units.nx2", "textures.nx2"];

/// Subfolder (below the FAForever root) each channel syncs into.
/// Mirror-only channels (faf-client) return `None`.
pub fn channel_subdir(channel: &str) -> Option<&'static str> {
    match channel {
        CHANNEL_GAMEDATA => Some("gamedata"),
        CHANNEL_MAP_GENERATOR => Some("map_generator"),
        _ => None,
    }
}

/// Filename pattern of map generator jars, e.g. `MapGenerator_1.22.1.jar`.
pub const MAP_GENERATOR_JAR_PREFIX: &str = "MapGenerator_";

/// How many recent map generator versions to keep (server and client).
pub const MAP_GENERATOR_KEEP: usize = 3;

/// Extract a dotted version from a file name, e.g. `dfc_windows_1_6_3.exe`
/// or `downlords-faf-client-1.6.3.exe` → `1.6.3`. Returns the first run of
/// digits separated by `.`/`_` containing at least two numeric parts.
pub fn detect_version_from_filename(file_name: &str) -> Option<String> {
    let mut best: Option<String> = None;
    let mut current = String::new();
    let flush = |current: &mut String, best: &mut Option<String>| {
        let parts: Vec<&str> = current
            .split(['.', '_'])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() >= 2 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
            let candidate = parts.join(".");
            let better = match (
                &best,
                compare_version_strings(&candidate, best.as_deref().unwrap_or("0")),
            ) {
                (None, _) => true,
                (Some(_), Some(std::cmp::Ordering::Greater)) => true,
                _ => false,
            };
            if better {
                *best = Some(candidate);
            }
        }
        current.clear();
    };
    for c in file_name.chars() {
        if c.is_ascii_digit() || c == '.' || c == '_' {
            current.push(c);
        } else {
            flush(&mut current, &mut best);
        }
    }
    flush(&mut current, &mut best);
    best
}

/// Extract the version string from a `MapGenerator_<version>.jar` file name.
pub fn map_generator_jar_version(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix(MAP_GENERATOR_JAR_PREFIX)?;
    let version = rest.strip_suffix(".jar")?;
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    Some(version.to_string())
}

/// Parse a FAF map folder name of the form `base.vNNNN` (e.g.
/// `my_map.v0001`) into `(base, version)`. Returns `None` for names that
/// don't follow the convention.
pub fn map_folder_version(folder_name: &str) -> Option<(&str, u32)> {
    let (base, version) = folder_name.rsplit_once(".v")?;
    if base.is_empty() || version.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((base, version.parse().ok()?))
}

/// Today's date as `YYYY-MM-DD` — the display version for maps commits
/// (the maps channel merges uploads, so there is no single patch version).
pub fn today_stamp() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Compare two version strings as dotted numeric tuples (`3837` > `3825`,
/// `1.22.10` > `1.22.1`). Returns `None` when either side is not numeric.
pub fn compare_version_strings(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parse = |s: &str| -> Option<Vec<u64>> {
        s.trim()
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect()
    };
    let mut av = parse(a)?.into_iter();
    let mut bv = parse(b)?.into_iter();
    Some(loop {
        match (av.next(), bv.next()) {
            (None, None) => break std::cmp::Ordering::Equal,
            (None, Some(x)) if x == 0 => continue,
            (None, Some(_)) => break std::cmp::Ordering::Less,
            (Some(_), None) => break std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => match x.cmp(&y) {
                std::cmp::Ordering::Equal => continue,
                other => break other,
            },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn version_from_filename() {
        assert_eq!(
            detect_version_from_filename("dfc_windows_1_6_3.exe").as_deref(),
            Some("1.6.3")
        );
        assert_eq!(
            detect_version_from_filename("downlords-faf-client-1.6.10.exe").as_deref(),
            Some("1.6.10")
        );
        assert_eq!(
            detect_version_from_filename("MapGenerator_1.22.1.jar").as_deref(),
            Some("1.22.1")
        );
        assert_eq!(detect_version_from_filename("faf-client.exe"), None);
    }

    #[test]
    fn jar_version_parsing() {
        assert_eq!(
            map_generator_jar_version("MapGenerator_1.22.1.jar").as_deref(),
            Some("1.22.1")
        );
        assert_eq!(
            map_generator_jar_version("MapGenerator_2.0.jar").as_deref(),
            Some("2.0")
        );
        assert_eq!(map_generator_jar_version("other.jar"), None);
        assert_eq!(map_generator_jar_version("MapGenerator_beta.jar"), None);
    }

    #[test]
    fn map_folder_version_parsing() {
        assert_eq!(map_folder_version("my_map.v0001"), Some(("my_map", 1)));
        assert_eq!(map_folder_version("astro.v0012"), Some(("astro", 12)));
        assert_eq!(map_folder_version("a.b.v0002"), Some(("a.b", 2)));
        assert_eq!(map_folder_version("noversion"), None);
        assert_eq!(map_folder_version("map.v"), None);
        assert_eq!(map_folder_version("map.vxyz"), None);
        assert_eq!(map_folder_version(".v0001"), None);
    }

    #[test]
    fn version_comparison() {
        assert_eq!(compare_version_strings("3837", "3825"), Some(Greater));
        assert_eq!(compare_version_strings("1.22.1", "1.22.0"), Some(Greater));
        assert_eq!(compare_version_strings("1.22.10", "1.22.1"), Some(Greater));
        assert_eq!(compare_version_strings("1.22", "1.22.0"), Some(Equal));
        assert_eq!(compare_version_strings("1.22.0", "1.22.1"), Some(Less));
        assert_eq!(compare_version_strings("abc", "1.0"), None);
    }
}
