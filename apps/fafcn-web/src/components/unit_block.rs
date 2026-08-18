use dioxus::prelude::*;

use faf_dioxus_ui::CountSlider;

use crate::components::UnitSummary;
use crate::i18n::{self, Text};

/// A builder or target slot with portrait, name, count slider, and hint.
#[component]
pub fn UnitBlock(
    label: String,
    unit: Option<UnitSummary>,
    count: u32,
    hint: String,
    on_click: EventHandler<()>,
    on_count: EventHandler<u32>,
    #[props(default = false)] disabled: bool,
) -> Element {
    let button_class = if disabled {
        "w-12 h-12 p-1 rounded bg-black border border-neutral-700 flex items-center justify-center self-center cursor-not-allowed opacity-60"
    } else {
        "w-12 h-12 p-1 rounded bg-black border border-neutral-600 flex items-center justify-center transition-colors hover:border-neutral-400 self-center"
    };

    let portrait_src = unit.as_ref().map(|u| crate::net::portrait_url(&u.id));
    let t = i18n::use_t();
    let select_hint = t.t(Text::ClickToSelectUnit);

    rsx! {
        div { class: "flex flex-col gap-1.5 p-1.5 rounded bg-neutral-800/50 border border-neutral-700",
            span { class: "text-[10px] uppercase tracking-wide text-neutral-500", "{label}" }
            button {
                class: "{button_class}",
                disabled,
                onclick: move |_| {
                    if !disabled {
                        on_click.call(());
                    }
                },
                title: "{select_hint}",
                if let Some(ref u) = unit {
                    img {
                        src: portrait_src.clone().unwrap_or_default(),
                        alt: "{u.name}",
                        class: "w-full h-full object-contain",
                    }
                } else {
                    span { class: "text-neutral-500 text-2xl", "?" }
                }
            }
            div { class: "flex flex-col items-center text-center gap-1",
                span { class: "text-sm text-neutral-300 truncate w-full",
                    {unit.as_ref().map(|u| u.name.as_str()).unwrap_or("—")}
                }
                CountSlider {
                    value: count,
                    on_change: on_count,
                    disabled,
                }
                span { class: "text-[10px] text-neutral-500", "{hint}" }
            }
        }
    }
}
