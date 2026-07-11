use std::collections::HashSet;

use dioxus::prelude::*;

#[component]
pub fn FilterBar(
    active_factions: Signal<HashSet<String>>,
    active_kinds: Signal<HashSet<String>>,
    active_techs: Signal<HashSet<String>>,
) -> Element {
    rsx! {
        div {
            class: "flex items-center gap-4",
            FilterGroup {
                items: vec!["uef", "cybran", "aeon", "seraphim"],
                active: active_factions,
                icon_dir: "embed_icons",
                extension: "svg",
            }
            FilterGroup {
                items: vec!["Base", "Land", "Air", "Naval"],
                active: active_kinds,
                icon_dir: "ui",
                extension: "png",
            }
            FilterGroup {
                items: vec!["T1", "T2", "T3", "EXP"],
                active: active_techs,
                icon_dir: "ui",
                extension: "png",
            }
        }
    }
}

#[component]
fn FilterGroup(
    items: Vec<&'static str>,
    active: Signal<HashSet<String>>,
    icon_dir: &'static str,
    extension: &'static str,
) -> Element {
    rsx! {
        div {
            class: "flex items-center gap-1",
            for item in items {
                FilterButton {
                    item,
                    active,
                    icon_dir,
                    extension,
                }
            }
        }
    }
}

#[component]
fn FilterButton(
    item: &'static str,
    active: Signal<HashSet<String>>,
    icon_dir: &'static str,
    extension: &'static str,
) -> Element {
    let is_active = active.read().contains(item);
    let title = item.to_string();
    let src = format!("/{}/{}.{}", icon_dir, item, extension);
    let active_class = if is_active {
        "opacity-100 bg-white/15 ring-1 ring-white/30"
    } else {
        "opacity-75 hover:opacity-100 bg-neutral-800/50 hover:bg-neutral-700/50"
    };
    let img_class = "w-full h-full object-contain";

    rsx! {
        button {
            class: "w-8 h-8 p-1 rounded cursor-pointer transition-all {active_class}",
            title: "{title}",
            onclick: move |_| {
                let mut set = active.write();
                if set.contains(item) {
                    set.remove(item);
                } else {
                    set.insert(item.to_string());
                }
            },
            img {
                src: "{src}",
                alt: "{title}",
                class: "{img_class}",
            }
        }
    }
}
