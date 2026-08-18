use std::collections::HashSet;

use dioxus::prelude::*;
use gloo_net::http::Request;

use crate::components::{ComparisonPanel, UnitSelector, UnitSummary};
use crate::i18n::{self, Text};

/// Unit comparison page body: multi-select unit grid + comparison panel.
///
/// Note: this component is exclusive to the Units page — the shared
/// `UnitSelector` stays single-select for other pages (Simulate modal);
/// only the selection *handling* here is multi-select (click toggles).
#[component]
pub fn UnitBrowser() -> Element {
    let units = use_resource(move || async move {
        Request::get(&crate::net::api_url("/api/units"))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json::<Vec<UnitSummary>>()
            .await
            .map_err(|e| e.to_string())
    });
    let mut selected = use_signal(Vec::<UnitSummary>::new);
    let t = i18n::use_t();

    let unit_list = match units.read().as_ref() {
        Some(Ok(list)) => list.clone(),
        Some(Err(err)) => {
            return rsx! {
                div { class: "flex items-center justify-center h-full text-red-400",
                    "{t.t(Text::LoadUnitsFailed)}{err}"
                }
            };
        }
        None => {
            return rsx! {
                div { class: "flex items-center justify-center h-full text-neutral-400",
                    "{t.t(Text::Loading)}"
                }
            };
        }
    };

    let selected_ids: HashSet<String> = selected.read().iter().map(|u| u.id.clone()).collect();

    rsx! {
        div { class: "flex flex-col h-full bg-neutral-950 text-gray-200 overflow-hidden font-sans select-none",
            div { class: "flex flex-1 min-h-0 overflow-hidden",
                div { class: "flex-1 overflow-hidden",
                    UnitSelector {
                        units: unit_list,
                        selected: selected_ids,
                        on_select: move |unit: UnitSummary| {
                            selected.with_mut(|list| {
                                if let Some(pos) = list.iter().position(|u| u.id == unit.id) {
                                    list.remove(pos);
                                } else {
                                    list.push(unit);
                                }
                            });
                        },
                    }
                }
                div { class: "w-96 xl:w-[30rem] 2xl:w-[36rem] shrink-0 border-l border-neutral-800 bg-neutral-900/50 overflow-y-auto p-4",
                    ComparisonPanel { selected }
                }
            }
        }
    }
}
