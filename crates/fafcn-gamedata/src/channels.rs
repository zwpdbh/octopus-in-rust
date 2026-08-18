//! Sync channel definitions shared by server, client, and web.
//!
//! Players only struggle to download specific things, so the mirror is
//! organized into two well-known channels below the `FAForever` folder:
//!
//! - `gamedata` — only the big patch archives (`env.nx2`, `units.nx2`,
//!   `textures.nx2`), versioned by the FAF patch version from `lua.nx2`.
//! - `map-generator` — the newest few `MapGenerator_*.jar` files, versioned
//!   by the newest jar's version.

/// Channel id for the gamedata patch archives.
pub const CHANNEL_GAMEDATA: &str = "gamedata";

/// Channel id for the map generator jars.
pub const CHANNEL_MAP_GENERATOR: &str = "map-generator";

/// All known channel ids (rejected at the API boundary otherwise).
pub const CHANNELS: &[&str] = &[CHANNEL_GAMEDATA, CHANNEL_MAP_GENERATOR];

/// The only gamedata files players actually need mirrored.
pub const GAMEDATA_SYNC_FILES: &[&str] = &["env.nx2", "units.nx2", "textures.nx2"];

/// Subfolder (below the FAForever root) each channel syncs into.
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

/// Extract the version string from a `MapGenerator_<version>.jar` file name.
pub fn map_generator_jar_version(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix(MAP_GENERATOR_JAR_PREFIX)?;
    let version = rest.strip_suffix(".jar")?;
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    Some(version.to_string())
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
    fn version_comparison() {
        assert_eq!(compare_version_strings("3837", "3825"), Some(Greater));
        assert_eq!(compare_version_strings("1.22.1", "1.22.0"), Some(Greater));
        assert_eq!(compare_version_strings("1.22.10", "1.22.1"), Some(Greater));
        assert_eq!(compare_version_strings("1.22", "1.22.0"), Some(Equal));
        assert_eq!(compare_version_strings("1.22.0", "1.22.1"), Some(Less));
        assert_eq!(compare_version_strings("abc", "1.0"), None);
    }
}
