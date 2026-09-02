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
//! - `coop` — co-op mission support files (`bin/init_coop.lua`,
//!   `gamedata/lobby_coop.cop`, `gamedata/*_VO.nx2` voice-overs), synced into
//!   the FAForever ROOT (manifest paths carry their `bin/`/`gamedata/`
//!   prefix). Manually uploaded; the auto-updater does not mirror it (the
//!   official coop file list is not anonymously visible).
//! - `bin` — the FAF-patched game binary (`ForgedAlliance.exe`) that the
//!   official client otherwise downloads from FAF's content server on first
//!   launch. Pre-seeding it via the mirror lets new players skip that
//!   download. NOTE: FAF deliberately distributes this exe only through
//!   their official client to ownership-verified accounts; see
//!   `docs/fafcn/game-binary-channel.md` before changing this channel.

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

/// Channel id for co-op mission support files. Synced into the FAForever
/// ROOT (paths carry their own `bin/`/`gamedata/` prefix).
pub const CHANNEL_COOP: &str = "coop";

/// Channel id for the FAF-patched game binary (`ForgedAlliance.exe`).
/// Synced into the FAForever `bin/` folder.
pub const CHANNEL_BIN: &str = "bin";

/// All known channel ids (rejected at the API boundary otherwise).
pub const CHANNELS: &[&str] = &[
    CHANNEL_GAMEDATA,
    CHANNEL_MAP_GENERATOR,
    CHANNEL_FAF_CLIENT,
    CHANNEL_MAPS,
    CHANNEL_COOP,
    CHANNEL_BIN,
];

/// Channels the sync client syncs into the FAForever folder.
pub const SYNC_CHANNELS: &[&str] = &[
    CHANNEL_GAMEDATA,
    CHANNEL_MAP_GENERATOR,
    CHANNEL_COOP,
    CHANNEL_BIN,
];

/// The only gamedata files players actually need mirrored.
pub const GAMEDATA_SYNC_FILES: &[&str] = &["env.nx2", "units.nx2", "textures.nx2"];

/// The complete set of archive names the base `faf` featured mod deploys to
/// `gamedata/` (all ten are re-packed on every FAF deploy). The coop upload
/// whitelist uses this to EXCLUDE base-game archives: coop-specific archives
/// are whatever `.nx2` remains.
pub const FAF_STANDARD_NX2: &[&str] = &[
    "effects.nx2",
    "env.nx2",
    "etc.nx2",
    "loc.nx2",
    "lua.nx2",
    "meshes.nx2",
    "projectiles.nx2",
    "schook.nx2",
    "textures.nx2",
    "units.nx2",
];

/// Subfolder (below the FAForever root) each channel syncs into.
/// Mirror-only channels (faf-client) return `None`; coop syncs into the
/// root itself (its manifest paths carry `bin/`/`gamedata/` prefixes).
pub fn channel_subdir(channel: &str) -> Option<&'static str> {
    match channel {
        CHANNEL_GAMEDATA => Some("gamedata"),
        CHANNEL_MAP_GENERATOR => Some("map_generator"),
        CHANNEL_COOP => Some(""),
        CHANNEL_BIN => Some("bin"),
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
///
/// New-style names like `faf_windows-x64_2026_7_1.exe` glue the arch token
/// (`x64`) onto the version run, so a leading `32`/`64`/`86` part is dropped
/// when the run has more than three parts.
pub fn detect_version_from_filename(file_name: &str) -> Option<String> {
    let mut best: Option<String> = None;
    let mut current = String::new();
    let flush = |current: &mut String, best: &mut Option<String>| {
        let mut parts: Vec<&str> = current
            .split(['.', '_'])
            .filter(|p| !p.is_empty())
            .collect();
        if parts.len() > 3 && matches!(parts.first(), Some(&"32" | &"64" | &"86")) {
            parts.remove(0);
        }
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

/// Extract the mod version from a FAF `mod_info.lua` body: the first
/// `version = <digits>` line. Tolerates whitespace and Lua `--` comments.
/// Used for both the faf deploy branch (gamedata patch version) and the
/// fa-coop repo (coop mod version).
pub fn parse_mod_info_version(body: &str) -> Option<String> {
    for line in body.lines() {
        // Strip Lua line comments, then require `version = <digits>`.
        let line = line.split_once("--").map_or(line, |(code, _)| code).trim();
        let Some(rest) = line.strip_prefix("version") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    None
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
        assert_eq!(
            detect_version_from_filename("faf_windows-x64_2026_7_1.exe").as_deref(),
            Some("2026.7.1")
        );
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
    fn parse_mod_info_version_works() {
        assert_eq!(
            parse_mod_info_version("name = \"FAF\"\nversion = 3838\n"),
            Some("3838".to_string())
        );
        assert_eq!(
            parse_mod_info_version("  version   =   66  -- bumped\n"),
            Some("66".to_string())
        );
        assert_eq!(parse_mod_info_version("version=66"), Some("66".to_string()));
        assert_eq!(parse_mod_info_version(""), None);
        assert_eq!(parse_mod_info_version("no version here"), None);
        assert_eq!(parse_mod_info_version("version = \"66\""), None);
        assert_eq!(parse_mod_info_version("version_number = 66"), None);
        assert_eq!(parse_mod_info_version("-- version = 66"), None);
        assert_eq!(parse_mod_info_version("version = "), None);
    }

    #[test]
    fn coop_channel_registration() {
        assert!(CHANNELS.contains(&CHANNEL_COOP));
        assert!(SYNC_CHANNELS.contains(&CHANNEL_COOP));
        assert_eq!(channel_subdir(CHANNEL_COOP), Some(""));
    }

    #[test]
    fn bin_channel_registration() {
        assert!(CHANNELS.contains(&CHANNEL_BIN));
        assert!(SYNC_CHANNELS.contains(&CHANNEL_BIN));
        assert_eq!(channel_subdir(CHANNEL_BIN), Some("bin"));
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
