use dioxus::prelude::*;

use crate::components::UnitBrowser;

#[component]
pub fn Home() -> Element {
    rsx! {
        div { class: "h-full bg-neutral-950 overflow-hidden",
            UnitBrowser {}
        }
    }
}
