use dioxus::prelude::*;

use crate::components::UnitSummary;
use crate::i18n::{self, Text};
use crate::utils::faction_color;

/// Ratio of `other` against `base` for one stat (clamped to avoid div-by-zero).
fn ratio(base: f64, other: f64) -> f64 {
    ((other.max(1.0) / base.max(1.0)) * 100.0).round() / 100.0
}

/// Text color for a ratio badge (mirrors the faf_cn eco-guides thresholds).
fn ratio_class(ratio: f64) -> &'static str {
    if ratio < 0.8 {
        "text-green-400"
    } else if ratio > 5.0 {
        "text-red-400"
    } else if ratio > 1.5 {
        "text-orange-400"
    } else {
        "text-yellow-300"
    }
}

/// Right-side comparison panel for the Units page: shows the selected units
/// sorted by mass (descending) and tiered cross-comparisons — each unit
/// against every cheaper one ("1 A ≈ x B").
#[component]
pub fn ComparisonPanel(selected: Signal<Vec<UnitSummary>>) -> Element {
    let t = i18n::use_t();
    let units = selected.read().clone();

    if units.is_empty() {
        return rsx! {
            div { class: "h-full flex items-center justify-center text-neutral-500 text-sm px-4 text-center",
                "{t.t(Text::CompareEmpty)}"
            }
        };
    }

    // Sort by mass descending so ratios read "1 big ≈ N small".
    let mut sorted = units;
    sorted.sort_by(|a, b| {
        b.cost
            .mass
            .partial_cmp(&a.cost.mass)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_mass: f64 = sorted.iter().map(|u| u.cost.mass).sum();
    let total_energy: f64 = sorted.iter().map(|u| u.cost.energy).sum();
    let count = sorted.len();

    rsx! {
        div { class: "space-y-4",
            // Header: selection count + clear button.
            div { class: "flex items-center justify-between",
                h2 { class: "text-base font-semibold text-white",
                    "{t.t(Text::CompareTitle)} ({count})"
                }
                button {
                    class: "px-2 py-0.5 rounded bg-red-900/40 hover:bg-red-900/60 text-red-300 text-xs transition-colors",
                    onclick: move |_| selected.write().clear(),
                    "{t.t(Text::CompareClear)}"
                }
            }

            // Selected units with absolute stats (removable).
            div { class: "space-y-1.5",
                for (idx , unit) in sorted.iter().enumerate() {
                    {
                        let color = faction_color(&unit.faction);
                        rsx! {
                            div { class: "flex items-center gap-3 p-2 rounded bg-neutral-800/60 border border-neutral-800",
                                img {
                                    src: crate::net::portrait_url(&unit.id),
                                    alt: "{unit.name}",
                                    class: "w-12 h-12 object-contain rounded shrink-0",
                                }
                                div { class: "flex-1 min-w-0",
                                    div { class: "text-sm font-medium truncate", style: "color: {color};",
                                        "{unit.name}"
                                        span { class: "ml-1.5 text-[10px] uppercase tracking-wide text-neutral-500",
                                            "{unit.faction}"
                                        }
                                    }
                                    div { class: "text-xs text-neutral-400 tabular-nums mt-0.5",
                                        "{t.t(Text::MassCost)} {unit.cost.mass:.0} · {t.t(Text::EnergyCost)} {unit.cost.energy:.0} · {t.t(Text::BuildTime)} {unit.cost.build_time:.0}"
                                    }
                                }
                                button {
                                    class: "px-1.5 text-neutral-500 hover:text-red-300 text-base transition-colors shrink-0",
                                    title: "{t.t(Text::CompareRemove)}",
                                    onclick: move |_| {
                                        selected.write().remove(idx);
                                    },
                                    "x"
                                }
                            }
                        }
                    }
                }
            }

            // Tiered cross-comparison: each unit vs every cheaper unit.
            if count >= 2 {
                div { class: "space-y-2",
                    for i in 0..count - 1 {
                        {
                            let base = &sorted[i];
                            rsx! {
                                div { class: "rounded border border-neutral-800 bg-neutral-900/60 p-2.5",
                                    div { class: "flex items-center gap-2.5 mb-2",
                                        img {
                                            src: crate::net::portrait_url(&base.id),
                                            alt: "{base.name}",
                                            class: "w-10 h-10 object-contain rounded shrink-0",
                                        }
                                        div { class: "text-sm font-semibold text-white",
                                            "1 {base.name}"
                                            span { class: "ml-1.5 text-[10px] uppercase tracking-wide text-neutral-500",
                                                "{base.faction}"
                                            }
                                        }
                                    }
                                    for target in sorted.iter().skip(i + 1) {
                                        {
                                            // Sorted by mass descending, so base is the
                                            // bigger unit: "1 base ≈ (base/target)x target".
                                            let mass_r = ratio(target.cost.mass, base.cost.mass);
                                            let energy_r = ratio(target.cost.energy, base.cost.energy);
                                            let time_r = ratio(target.cost.build_time, base.cost.build_time);
                                            rsx! {
                                                div { class: "flex items-center justify-between gap-2 py-1.5 border-t border-neutral-800/60",
                                                    span { class: "flex items-center gap-2 min-w-0",
                                                        img {
                                                            src: crate::net::portrait_url(&target.id),
                                                            alt: "{target.name}",
                                                            class: "w-7 h-7 object-contain rounded shrink-0",
                                                        }
                                                        span { class: "text-sm text-neutral-300 truncate",
                                                            "≈ {target.name}"
                                                            span { class: "ml-1 text-[10px] uppercase tracking-wide text-neutral-500",
                                                                "{target.faction}"
                                                            }
                                                        }
                                                    }
                                                    span { class: "flex gap-2.5 text-sm tabular-nums shrink-0",
                                                        span { class: "{ratio_class(mass_r)}", title: "{t.t(Text::MassCost)}",
                                                            "{mass_r}x {t.t(Text::MassShort)}"
                                                        }
                                                        span { class: "{ratio_class(energy_r)}", title: "{t.t(Text::EnergyCost)}",
                                                            "{energy_r}x {t.t(Text::EnergyShort)}"
                                                        }
                                                        span { class: "{ratio_class(time_r)}", title: "{t.t(Text::BuildTime)}",
                                                            "{time_r}x {t.t(Text::BuildTimeShort)}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Quick stats.
            div { class: "rounded border border-neutral-800 bg-neutral-900/60 p-2.5 text-sm text-neutral-300",
                div { class: "font-semibold text-white mb-1", "{t.t(Text::CompareQuickStats)}" }
                div { class: "tabular-nums", "{t.t(Text::CompareTotalMass)}: {total_mass:.0}" }
                div { class: "tabular-nums", "{t.t(Text::CompareTotalEnergy)}: {total_energy:.0}" }
            }
        }
    }
}
