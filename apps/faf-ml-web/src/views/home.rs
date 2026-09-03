use dioxus::prelude::*;

use crate::Route;

/// Landing page: what this platform is + quick links into the workflow.
#[component]
pub fn Home() -> Element {
    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans",
            div { class: "max-w-3xl mx-auto px-6 py-16",
                p { class: "text-sm tracking-[0.3em] text-amber-400 font-semibold mb-4",
                    "FAF MACHINE LEARNING"
                }
                h1 { class: "text-4xl font-bold text-white mb-4", "faf-ml data platform" }
                p { class: "text-lg text-neutral-300 mb-10",
                    "Phase 0 of the FAF unit-detection pipeline: collect screenshots, "
                    "review pre-generated bounding boxes, and freeze labeled data into "
                    "immutable dataset snapshots. Training and evaluation come later."
                }
                div { class: "grid grid-cols-1 sm:grid-cols-2 gap-4",
                    FeatureCard {
                        to: Route::Gallery {},
                        title: "Gallery",
                        desc: "Upload screenshots (PNG) and manage the raw image pool.",
                    }
                    FeatureCard {
                        to: Route::Datasets {},
                        title: "Datasets",
                        desc: "Snapshot labeled images into immutable, versioned datasets.",
                    }
                }
                div { class: "mt-10 rounded-lg border border-neutral-800 bg-neutral-900 p-5",
                    h2 { class: "text-base font-semibold text-white mb-2", "Workflow" }
                    ol { class: "list-decimal list-inside text-sm text-neutral-400 space-y-1",
                        li { "Upload screenshots in the Gallery (or import faf-datagen output via the API)." }
                        li { "Open an image, click a box to select it, fix its class, delete wrong boxes, save." }
                        li { "Create a dataset snapshot once a batch is reviewed — snapshots embed their labels and never change." }
                    }
                }
            }
        }
    }
}

/// One feature card linking to an app page.
#[component]
fn FeatureCard(to: Route, title: &'static str, desc: &'static str) -> Element {
    rsx! {
        Link {
            to,
            class: "block rounded-lg border border-neutral-800 bg-neutral-900 p-5 hover:border-blue-500 hover:bg-neutral-800/60 transition-colors",
            h3 { class: "text-lg font-semibold text-white mb-2", "{title}" }
            p { class: "text-sm text-neutral-400", "{desc}" }
        }
    }
}
