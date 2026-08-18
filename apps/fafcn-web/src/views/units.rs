use dioxus::prelude::*;

use crate::components::UnitBrowser;

/// Unit comparison page: the browsable unit database.
#[component]
pub fn Units() -> Element {
    rsx! {
        div { class: "h-full bg-neutral-950 overflow-hidden",
            UnitBrowser {}
        }
    }
}
