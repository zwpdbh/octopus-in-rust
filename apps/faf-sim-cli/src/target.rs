//! Faction-scoped research targets for the FAF build-order CLI.
//!
//! Instead of a flat list of blueprint ids, the `simulate` and `train`
//! subcommands are organized as:
//!
//! ```text
//! faf-sim simulate -s mcts cybran monkeylord
//! faf-sim train -e 1000 cybran monkeylord
//! ```
//!
//! Each faction has its own unit enum so that clap can constrain the possible
//! values of `<UNIT>` to the units actually available for that faction.

use clap::ValueEnum;

use faf_sim::{Goal, TechLevel, Units};

/// One of the four playable factions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Faction {
    /// United Earth Federation.
    #[value(name = "uef", alias = "u")]
    Uef,
    /// Cybran Nation.
    #[value(name = "cybran", alias = "c")]
    Cybran,
    /// Aeon Illuminate.
    #[value(name = "aeon", alias = "a")]
    Aeon,
    /// Seraphim.
    #[value(name = "seraphim", alias = "s")]
    Seraphim,
}

impl Faction {
    /// English display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            Faction::Uef => "UEF",
            Faction::Cybran => "Cybran",
            Faction::Aeon => "Aeon",
            Faction::Seraphim => "Seraphim",
        }
    }
}

/// Cross-faction unit kind. Used internally after parsing a faction-scoped
/// unit enum.
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
    /// Technology tier of this target.
    pub fn tech_level(&self) -> TechLevel {
        match self {
            UnitKind::Nuke | UnitKind::Arty => TechLevel::T3,
            _ => TechLevel::T4,
        }
    }

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
}

/// UEF-specific research targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UefUnit {
    #[value(name = "fatboy", alias = "fb")]
    Fatboy,
    #[value(name = "mavor")]
    Mavor,
    #[value(name = "atlantis")]
    Atlantis,
    #[value(name = "novaxcenter", alias = "novax_center")]
    NovaxCenter,
    #[value(name = "nuke", alias = "nukelauncher")]
    Nuke,
    #[value(name = "arty", alias = "artillery")]
    Arty,
}

impl From<UefUnit> for UnitKind {
    fn from(u: UefUnit) -> Self {
        match u {
            UefUnit::Fatboy => UnitKind::Fatboy,
            UefUnit::Mavor => UnitKind::Mavor,
            UefUnit::Atlantis => UnitKind::Atlantis,
            UefUnit::NovaxCenter => UnitKind::NovaxCenter,
            UefUnit::Nuke => UnitKind::Nuke,
            UefUnit::Arty => UnitKind::Arty,
        }
    }
}

/// Cybran-specific research targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CybranUnit {
    #[value(name = "monkeylord", alias = "ml")]
    Monkeylord,
    #[value(name = "scathis", alias = "scathis_mobile")]
    Scathis,
    #[value(name = "megalith")]
    Megalith,
    #[value(name = "soulripper", alias = "soul_ripper")]
    SoulRipper,
    #[value(name = "nuke", alias = "nukelauncher")]
    Nuke,
    #[value(name = "arty", alias = "artillery")]
    Arty,
}

impl From<CybranUnit> for UnitKind {
    fn from(u: CybranUnit) -> Self {
        match u {
            CybranUnit::Monkeylord => UnitKind::Monkeylord,
            CybranUnit::Scathis => UnitKind::Scathis,
            CybranUnit::Megalith => UnitKind::Megalith,
            CybranUnit::SoulRipper => UnitKind::SoulRipper,
            CybranUnit::Nuke => UnitKind::Nuke,
            CybranUnit::Arty => UnitKind::Arty,
        }
    }
}

/// Aeon-specific research targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AeonUnit {
    #[value(name = "galacticcolossus", alias = "galactic_colossus", alias = "gc")]
    GalacticColossus,
    #[value(name = "salvation")]
    Salvation,
    #[value(name = "czar")]
    Czar,
    #[value(name = "tempest")]
    Tempest,
    #[value(name = "paragon")]
    Paragon,
    #[value(name = "nuke", alias = "nukelauncher")]
    Nuke,
    #[value(name = "arty", alias = "artillery")]
    Arty,
}

impl From<AeonUnit> for UnitKind {
    fn from(u: AeonUnit) -> Self {
        match u {
            AeonUnit::GalacticColossus => UnitKind::GalacticColossus,
            AeonUnit::Salvation => UnitKind::Salvation,
            AeonUnit::Czar => UnitKind::Czar,
            AeonUnit::Tempest => UnitKind::Tempest,
            AeonUnit::Paragon => UnitKind::Paragon,
            AeonUnit::Nuke => UnitKind::Nuke,
            AeonUnit::Arty => UnitKind::Arty,
        }
    }
}

/// Seraphim-specific research targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SeraphimUnit {
    #[value(name = "ythotha")]
    Ythotha,
    #[value(name = "yolonaoss", alias = "yolona_oss")]
    YolonaOss,
    #[value(name = "ahwassa")]
    Ahwassa,
    #[value(name = "nuke", alias = "nukelauncher")]
    Nuke,
    #[value(name = "arty", alias = "artillery")]
    Arty,
}

impl From<SeraphimUnit> for UnitKind {
    fn from(u: SeraphimUnit) -> Self {
        match u {
            SeraphimUnit::Ythotha => UnitKind::Ythotha,
            SeraphimUnit::YolonaOss => UnitKind::YolonaOss,
            SeraphimUnit::Ahwassa => UnitKind::Ahwassa,
            SeraphimUnit::Nuke => UnitKind::Nuke,
            SeraphimUnit::Arty => UnitKind::Arty,
        }
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

    /// Convert this CLI target to the optimizer's abstract `UnitKind`.
    ///
    /// All current CLI targets are faction-unique units, so this maps to the
    /// blueprint id wrapped in `UnitKind::Unique`.
    pub fn to_sim_unit_kind(&self) -> faf_sim::UnitKind {
        faf_sim::UnitKind::Unique(faf_sim::UnitId(self.blueprint_id().to_string()))
    }

    /// Convert this CLI target to the optimizer's abstract `Goal`.
    ///
    /// The goal captures the tech level and resource cost of the concrete unit
    /// while discarding faction-specific identity.
    pub fn to_goal(&self, units: &Units) -> Goal {
        let kind = self.to_sim_unit_kind();
        let def = units
            .def(&kind)
            .expect("target blueprint must exist in index");
        Goal {
            tech_level: self.unit.tech_level(),
            mass_cost: def.cost.mass,
            energy_cost: def.cost.energy,
            build_time: def.cost.build_time,
        }
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
        let mut out = String::from("Available targets by faction:\n\n");

        writeln!(
            out,
            "uef:    fatboy, mavor, atlantis, novaxcenter, nuke, arty"
        )
        .unwrap();
        writeln!(
            out,
            "cybran: monkeylord, scathis, megalith, soulripper, nuke, arty"
        )
        .unwrap();
        writeln!(
            out,
            "aeon:   galacticcolossus, salvation, czar, tempest, paragon, nuke, arty"
        )
        .unwrap();
        writeln!(out, "seraphim: ythotha, yolonaoss, ahwassa, nuke, arty").unwrap();

        out
    }
}
