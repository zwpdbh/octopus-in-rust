use dioxus::prelude::*;

use crate::i18n::{self, Text};

/// Landing page (placeholder for now).
#[component]
pub fn Home() -> Element {
    let t = i18n::use_t();
    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans",
            div { class: "max-w-2xl mx-auto px-6 py-16 flex flex-col gap-4",
                h1 { class: "text-3xl font-bold text-white", "{t.t(Text::HomeTitle)}" }
                p { class: "text-neutral-400", "{t.t(Text::HomeSubtitle)}" }
            }
        }
    }
}
