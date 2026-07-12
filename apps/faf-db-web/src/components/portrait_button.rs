use dioxus::prelude::*;

use crate::types::UnitSummary;
use crate::utils::faction_glow_class;

#[component]
pub fn PortraitButton(
    unit: UnitSummary,
    faction: &'static str,
    selected: Signal<Option<UnitSummary>>,
    on_select: EventHandler<UnitSummary>,
) -> Element {
    let id = unit.id.clone();
    let name = unit.display_name.clone();
    let glow = faction_glow_class(faction);
    let is_selected = selected
        .read()
        .as_ref()
        .map(|s| s.id == unit.id)
        .unwrap_or(false);
    let strategic_src = unit
        .strategic_icon_name
        .as_deref()
        .map(|icon_name| format!("/strategic/{}_{}.png", faction, icon_name));

    rsx! {
        button {
            class: "relative w-12 h-12 p-[3px] rounded-[5px] bg-black border cursor-pointer transition-transform hover:scale-105 active:scale-[0.99] active:translate-y-px {glow}",
            class: if is_selected { "ring-2 ring-white" },
            title: "{name}",
            onclick: move |_| {
                on_select.call(unit.clone());
            },
            img {
                src: "/api/portraits/{id}.png",
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
