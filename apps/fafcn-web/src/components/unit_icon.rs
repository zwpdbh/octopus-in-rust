use dioxus::prelude::*;

use crate::components::UnitSummary;
use crate::utils::faction_glow_class;

/// Compact square portrait with faction glow and optional strategic icon overlay.
#[component]
pub fn UnitIcon(
    unit: UnitSummary,
    faction: String,
    selected: bool,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let id = unit.id.clone();
    let name = unit.name.clone();
    let glow = faction_glow_class(&faction);
    let portrait_src = crate::net::portrait_url(&id);
    let strategic_src = unit
        .strategic_icon_name
        .as_deref()
        .map(|icon| format!("/strategic/{}_{}.png", faction, icon));

    rsx! {
        button {
            class: "relative w-12 h-12 p-[3px] rounded-[5px] bg-black border cursor-pointer transition-transform hover:scale-105 active:scale-[0.99] active:translate-y-px {glow}",
            class: if selected { "ring-2 ring-white" },
            title: "{name}",
            onclick: move |_| on_select.call(unit.clone()),
            img {
                src: "{portrait_src}",
                alt: "{name}",
                class: "w-full h-full object-contain block",
            }
            if let Some(src) = strategic_src {
                img {
                    src: "{src}",
                    alt: "",
                    class: "absolute top-0.5 left-0.5 w-3.5 h-3.5 object-contain pointer-events-none",
                }
            }
        }
    }
}
