use faf_sim::units::UnitKind;

/// Human-readable label for an abstract unit kind (matches the graph nodes).
pub fn kind_label(kind: &UnitKind) -> String {
    match kind {
        UnitKind::Commander => "ACU".to_string(),
        UnitKind::Engineer(t) => format!("Eng {t:?}"),
        UnitKind::Factory(t) => format!("Factory {t:?}"),
        UnitKind::Mex(t) => format!("Mex {t:?}"),
        UnitKind::Pgen(t) => format!("Pgen {t:?}"),
        UnitKind::CapMex(t) => format!("Cap {t:?} Mex"),
        UnitKind::EnergyStorage => "Energy Storage".to_string(),
        UnitKind::Experimental => "Experimental".to_string(),
        UnitKind::Unique(id) => id.0.clone(),
    }
}

pub const CATEGORY_ORDER: &[&str] = &[
    "Land",
    "Air",
    "Naval",
    "Structures - Factories",
    "Structures - Economy",
    "Structures - Weapons",
    "Structures - Support",
    "Structures - Intelligence",
    "Construction - Buildpower",
    "Experimental",
];

pub const FACTION_ORDER: &[&str] = &["UEF", "Cybran", "Aeon", "Seraphim"];

pub fn tech_short(tech: &str) -> String {
    match tech {
        "TECH1" => "T1",
        "TECH2" => "T2",
        "TECH3" => "T3",
        "TECH4" | "EXPERIMENTAL" => "EXP",
        _ => tech,
    }
    .to_string()
}

pub fn faction_color(faction: &str) -> &'static str {
    match faction.to_lowercase().as_str() {
        "uef" => "#2d78b2",
        "cybran" => "#df2d0e",
        "aeon" => "#19b340",
        "seraphim" => "#fcb419",
        _ => "#888",
    }
}

/// Tailwind-aware portrait glow class. The returned literals are scanned by
/// Tailwind so the arbitrary values end up in the generated CSS.
pub fn faction_glow_class(faction: &str) -> &'static str {
    match faction.to_lowercase().as_str() {
        "uef" => "border-[rgba(148,193,227,0.33)] shadow-[inset_0_0_4px_rgba(70,174,255,0.43)] bg-[rgba(45,120,178,0.13)]",
        "cybran" => "border-[rgba(247,157,142,0.3)] shadow-[inset_0_0_4px_rgba(255,109,84,0.4)] bg-[rgba(223,45,14,0.1)]",
        "aeon" => "border-[rgba(120,236,150,0.33)] shadow-[inset_0_0_4px_rgba(51,255,103,0.43)] bg-[rgba(25,179,64,0.13)]",
        "seraphim" => "border-[rgba(253,229,176,0.3)] shadow-[inset_0_0_4px_rgba(255,213,124,0.4)] bg-[rgba(252,180,25,0.1)]",
        _ => "border-neutral-600",
    }
}
