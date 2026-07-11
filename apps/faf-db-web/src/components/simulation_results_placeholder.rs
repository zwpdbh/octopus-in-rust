use dioxus::prelude::*;

#[component]
pub fn SimulationResultsPlaceholder(requested: bool) -> Element {
    rsx! {
        div { class: "flex-1 flex items-center justify-center",
            div { class: "text-center space-y-2",
                if requested {
                    p { class: "text-sm text-neutral-500", "will be implement" }
                } else {
                    p { class: "text-sm text-neutral-500", "Click Begin Simulation to run the simulation." }
                }
            }
        }
    }
}
