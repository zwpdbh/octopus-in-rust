use dioxus::prelude::*;

use crate::Route;

#[component]
pub fn Navbar() -> Element {
    rsx! {
        div { class: "flex flex-col min-h-screen bg-neutral-950",
            nav { class: "flex items-center gap-4 px-4 py-3 bg-neutral-900 border-b border-neutral-800 shrink-0",
                Link {
                    class: "text-lg font-bold text-white hover:text-blue-400 transition-colors",
                    to: Route::Home {},
                    "faf-ml"
                }
                div { class: "flex-1" }
                NavLink { to: Route::Home {}, label: "Home" }
                NavLink { to: Route::Gallery {}, label: "Gallery" }
                NavLink { to: Route::Datasets {}, label: "Datasets" }
            }
            div { class: "flex-1 min-h-0 flex flex-col",
                Outlet::<Route> {}
            }
        }
    }
}

#[component]
fn NavLink(to: Route, label: &'static str) -> Element {
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
