//! Faction-scoped research targets for the FAF build-order CLI.
//!
//! Instead of a flat list of blueprint ids, the CLI is organized as:
//!
//! ```text
//! faf-sim deps -u fatboy      # UEF Fatboy
//! faf-sim deps -c monkeylord  # Cybran Monkeylord
//! faf-sim deps -a nuke        # Aeon T3 nuke launcher
//! ```
//!
//! Each target name is interpreted within the chosen faction, so `nuke` and
//! `arty` can refer to different blueprint ids depending on the flag.

use std::str::FromStr;

/// One of the four playable factions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Faction {
    Uef,
    Cybran,
    Aeon,
    Seraphim,
}

impl Faction {
    pub fn display_name(&self) -> &'static str {
        match self {
            Faction::Uef => "UEF",
            Faction::Cybran => "Cybran",
            Faction::Aeon => "Aeon",
            Faction::Seraphim => "Seraphim",
        }
    }
}

impl FromStr for Faction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "uef" | "u" => Ok(Faction::Uef),
            "cybran" | "c" => Ok(Faction::Cybran),
            "aeon" | "a" => Ok(Faction::Aeon),
            "seraphim" | "s" => Ok(Faction::Seraphim),
            _ => Err(format!("unknown faction '{}'", s)),
        }
    }
}

/// A faction-scoped target unit. The same `UnitKind` can map to different
/// blueprint ids depending on the faction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    // T4 land experimentals (faction-specific)
    Monkeylord,
    GalacticColossus,
    Ythotha,
    Fatboy,
    Scathis,
    Megalith,

    // T4 artillery / game-enders (faction-specific)
    Mavor,
    Salvation,
    YolonaOss,

    // T4 air/naval experimentals (faction-specific)
    Czar,
    SoulRipper,
    Ahwassa,
    Tempest,
    Atlantis,

    // T4 economic / special (faction-specific)
    Paragon,
    NovaxCenter,

    // T3 strategic weapons (one per faction)
    Nuke,
    Arty,
}

impl UnitKind {
    /// English display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            UnitKind::Monkeylord => "Monkeylord",
            UnitKind::GalacticColossus => "Galactic Colossus",
            UnitKind::Ythotha => "Ythotha",
            UnitKind::Fatboy => "Fatboy",
            UnitKind::Scathis => "Scathis",
            UnitKind::Megalith => "Megalith",
            UnitKind::Mavor => "Mavor",
            UnitKind::Salvation => "Salvation",
            UnitKind::YolonaOss => "Yolona Oss",
            UnitKind::Czar => "Czar",
            UnitKind::SoulRipper => "Soul Ripper",
            UnitKind::Ahwassa => "Ahwassa",
            UnitKind::Tempest => "Tempest",
            UnitKind::Atlantis => "Atlantis",
            UnitKind::Paragon => "Paragon",
            UnitKind::NovaxCenter => "Novax Center",
            UnitKind::Nuke => "T3 Nuke Launcher",
            UnitKind::Arty => "T3 Heavy Artillery",
        }
    }

    /// Accepted aliases, lowercase.
    pub fn aliases(&self) -> Vec<&'static str> {
        match self {
            UnitKind::Monkeylord => vec!["monkeylord", "ml", "猴王"],
            UnitKind::GalacticColossus => {
                vec!["galacticcolossus", "galactic_colossus", "gc", "银河巨像"]
            }
            UnitKind::Ythotha => vec!["ythotha", "伊若塔"],
            UnitKind::Fatboy => vec!["fatboy", "fb", "胖小子"],
            UnitKind::Scathis => vec!["scathis", "scathis_mobile", "冷酷"],
            UnitKind::Megalith => vec!["megalith", "巨石"],
            UnitKind::Mavor => vec!["mavor", "马维"],
            UnitKind::Salvation => vec!["salvation", "救赎"],
            UnitKind::YolonaOss => vec!["yolonaoss", "yolona_oss", "攸罗纳欧斯"],
            UnitKind::Czar => vec!["czar", "萨尔"],
            UnitKind::SoulRipper => vec!["soulripper", "soul_ripper", "死神"],
            UnitKind::Ahwassa => vec!["ahwassa", "阿瓦萨"],
            UnitKind::Tempest => vec!["tempest", "暴风雪"],
            UnitKind::Atlantis => vec!["atlantis", "亚特兰蒂斯"],
            UnitKind::Paragon => vec!["paragon", "钻石"],
            UnitKind::NovaxCenter => vec!["novaxcenter", "novax_center", "诺瓦司中心"],
            UnitKind::Nuke => vec!["nuke", "nukelauncher", "核弹"],
            UnitKind::Arty => vec!["arty", "artillery", "火炮"],
        }
    }

    /// All unit kinds.
    pub fn all() -> &'static [UnitKind] {
        &[
            UnitKind::Monkeylord,
            UnitKind::GalacticColossus,
            UnitKind::Ythotha,
            UnitKind::Fatboy,
            UnitKind::Scathis,
            UnitKind::Megalith,
            UnitKind::Mavor,
            UnitKind::Salvation,
            UnitKind::YolonaOss,
            UnitKind::Czar,
            UnitKind::SoulRipper,
            UnitKind::Ahwassa,
            UnitKind::Tempest,
            UnitKind::Atlantis,
            UnitKind::Paragon,
            UnitKind::NovaxCenter,
            UnitKind::Nuke,
            UnitKind::Arty,
        ]
    }
}

impl FromStr for UnitKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase();
        for kind in UnitKind::all() {
            if kind
                .aliases()
                .iter()
                .any(|a| a.to_lowercase() == normalized)
            {
                return Ok(*kind);
            }
        }
        Err(format!("unknown unit '{}'", s))
    }
}

/// A fully resolved research target: faction + unit kind → blueprint id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchTarget {
    pub faction: Faction,
    pub unit: UnitKind,
}

impl ResearchTarget {
    /// Resolve to a blueprint id.
    pub fn blueprint_id(&self) -> &'static str {
        match (self.faction, self.unit) {
            // UEF
            (Faction::Uef, UnitKind::Fatboy) => "UEL0401",
            (Faction::Uef, UnitKind::Mavor) => "UEB2401",
            (Faction::Uef, UnitKind::Atlantis) => "UES0401",
            (Faction::Uef, UnitKind::NovaxCenter) => "XEB2402",
            (Faction::Uef, UnitKind::Nuke) => "UEB2305",
            (Faction::Uef, UnitKind::Arty) => "UEB2302",

            // Cybran
            (Faction::Cybran, UnitKind::Monkeylord) => "URL0402",
            (Faction::Cybran, UnitKind::Scathis) => "URL0401",
            (Faction::Cybran, UnitKind::Megalith) => "XRL0403",
            (Faction::Cybran, UnitKind::SoulRipper) => "URA0401",
            (Faction::Cybran, UnitKind::Nuke) => "URB2305",
            (Faction::Cybran, UnitKind::Arty) => "URB2302",

            // Aeon
            (Faction::Aeon, UnitKind::GalacticColossus) => "UAL0401",
            (Faction::Aeon, UnitKind::Czar) => "UAA0310",
            (Faction::Aeon, UnitKind::Tempest) => "UAS0401",
            (Faction::Aeon, UnitKind::Salvation) => "XAB2307",
            (Faction::Aeon, UnitKind::Paragon) => "XAB1401",
            (Faction::Aeon, UnitKind::Nuke) => "UAB2305",
            (Faction::Aeon, UnitKind::Arty) => "UAB2302",

            // Seraphim
            (Faction::Seraphim, UnitKind::Ythotha) => "XSL0401",
            (Faction::Seraphim, UnitKind::Ahwassa) => "XSA0402",
            (Faction::Seraphim, UnitKind::YolonaOss) => "XSB2401",
            (Faction::Seraphim, UnitKind::Nuke) => "XSB2305",
            (Faction::Seraphim, UnitKind::Arty) => "XSB2302",

            // Invalid faction/unit combinations.
            _ => "",
        }
    }

    pub fn display_name(&self) -> String {
        format!(
            "{} {} ({})",
            self.faction.display_name(),
            self.unit.display_name(),
            self.blueprint_id()
        )
    }

    /// Validate that the chosen unit exists for the chosen faction.
    pub fn validate(&self) -> Result<(), String> {
        if self.blueprint_id().is_empty() {
            return Err(format!(
                "{} does not have a {}",
                self.faction.display_name(),
                self.unit.display_name()
            ));
        }
        Ok(())
    }

    /// Generate a help string listing targets grouped by faction.
    pub fn help_text() -> String {
        use std::fmt::Write;
        let mut out = String::from("Available targets (use one faction flag + unit name):\n\n");

        let factions = [
            (Faction::Uef, "-u, --uef"),
            (Faction::Cybran, "-c, --cybran"),
            (Faction::Aeon, "-a, --aeon"),
            (Faction::Seraphim, "-s, --seraphim"),
        ];

        for (faction, flag) in factions {
            writeln!(out, "{} {}:", flag, faction.display_name()).unwrap();
            for unit in UnitKind::all() {
                let target = ResearchTarget {
                    faction,
                    unit: *unit,
                };
                if target.blueprint_id().is_empty() {
                    continue;
                }
                let aliases = unit.aliases().join(", ");
                writeln!(
                    out,
                    "  {} — {} ({})",
                    unit.display_name(),
                    target.blueprint_id(),
                    aliases
                )
                .unwrap();
            }
            out.push('\n');
        }
        out
    }
}
