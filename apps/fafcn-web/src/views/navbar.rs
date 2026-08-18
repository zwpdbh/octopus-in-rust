use crate::i18n::{self, Text};
use crate::Route;
use dioxus::prelude::*;

#[component]
pub fn Navbar() -> Element {
    let t = i18n::use_t();
    rsx! {
        div { class: "flex flex-col min-h-screen bg-neutral-950",
            nav { class: "flex items-center gap-4 px-4 py-3 bg-neutral-900 border-b border-neutral-800 shrink-0",
                Link {
                    class: "text-lg font-bold text-white hover:text-blue-400 transition-colors",
                    to: Route::Home {},
                    "fafcn"
                }
                div { class: "flex-1" }
                NavLink { to: Route::Home {}, label: t.t(Text::NavHome) }
                NavLink { to: Route::Units {}, label: t.t(Text::NavUnits) }
                NavLink { to: Route::Simulate {}, label: t.t(Text::NavSimulate) }
                NavLink { to: Route::Qa {}, label: t.t(Text::NavQa) }
                NavLink { to: Route::Sync {}, label: t.t(Text::NavSync) }
                LangToggle {}
            }
            div { class: "flex-1 min-h-0 flex flex-col",
                Outlet::<Route> {}
            }
        }
    }
}

#[component]
fn NavLink(to: Route, label: String) -> Element {
    let current = use_route::<Route>();
    let active = current == to;
    rsx! {
        Link {
            class: if active {
                "px-3 py-1.5 rounded bg-blue-700 text-white text-sm"
            } else {
                "px-3 py-1.5 rounded text-neutral-300 hover:bg-neutral-800 text-sm transition-colors"
            },
            to,
            "{label}"
        }
    }
}

/// Button switching the UI between English and Chinese.
#[component]
fn LangToggle() -> Element {
    let t = i18n::use_t();
    let mut lang = i18n::use_lang_signal();
    let label = match t.0 {
        i18n::Lang::En => "中文",
        i18n::Lang::Zh => "EN",
    };
    rsx! {
        button {
            class: "ml-2 px-2.5 py-1.5 rounded border border-neutral-700 text-neutral-300 hover:bg-neutral-800 text-xs transition-colors",
            title: "Switch language / 切换语言",
            onclick: move |_| lang.set(t.0.toggled()),
            "{label}"
        }
    }
}
