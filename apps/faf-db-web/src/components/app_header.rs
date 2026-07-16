use dioxus::prelude::*;
use dioxus_router::Link;

use crate::route::Route;

#[component]
pub fn AppHeader(active: Route) -> Element {
    let home_active = active == Route::Home {};
    let simulate_active = active == Route::SimulateBuild {};
    let scheduler_active = active == Route::Scheduler {};

    rsx! {
        header {
            class: "flex items-center gap-4 px-4 py-3 border-b border-neutral-800 bg-neutral-900/50 shrink-0",
            h1 { class: "text-lg font-semibold text-white tracking-wide", "FAF Unit Database" }
            nav { class: "flex items-center gap-2",
                Link {
                    to: Route::Home {},
                    class: if home_active { "px-3 py-1.5 text-sm rounded bg-blue-700 text-white transition-colors" } else { "px-3 py-1.5 text-sm rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 transition-colors" },
                    "Home"
                }
                Link {
                    to: Route::SimulateBuild {},
                    class: if simulate_active { "px-3 py-1.5 text-sm rounded bg-blue-700 text-white transition-colors" } else { "px-3 py-1.5 text-sm rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 transition-colors" },
                    "Simulate Build"
                }
                Link {
                    to: Route::Scheduler {},
                    class: if scheduler_active { "px-3 py-1.5 text-sm rounded bg-blue-700 text-white transition-colors" } else { "px-3 py-1.5 text-sm rounded bg-neutral-800 text-neutral-300 hover:bg-neutral-700 transition-colors" },
                    "Scheduler"
                }
            }
        }
    }
}
