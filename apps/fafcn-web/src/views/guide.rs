use dioxus::prelude::*;
use fafcn_gamedata::Manifest;
use gloo_net::http::Request;

use crate::i18n::{self, Text};
use crate::Route;

/// GitHub releases page for the official FAF client.
const GITHUB_RELEASES: &str = "https://github.com/FAForever/downlords-faf-client/releases";

/// Onboarding guide: step-by-step instructions for new players.
#[component]
pub fn Guide() -> Element {
    let t = i18n::use_t();
    // The faf-client manifest decides step 1: mirror download vs GitHub link.
    let faf_client = use_resource(move || async move {
        let resp = Request::get(&crate::net::api_url(
            "/api/gamedata/channels/faf-client/manifest.json",
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;
        if resp.status() == 404 {
            return Ok(None);
        }
        resp.json::<Manifest>()
            .await
            .map(Some)
            .map_err(|e| e.to_string())
    });

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans",
            div { class: "max-w-2xl mx-auto px-6 py-8 flex flex-col gap-6",
                h1 { class: "text-2xl font-bold text-white", "{t.t(Text::GuideTitle)}" }
                p { class: "text-sm text-neutral-400", "{t.t(Text::GuideIntro)}" }

                // Step 1: download the FAF client.
                GuideStepCard {
                    number: "1",
                    title: t.t(Text::GuideStep1Title),
                    match faf_client.read().as_ref() {
                        Some(Ok(Some(manifest))) => rsx! {
                            p { class: "text-sm text-neutral-300", "{t.t(Text::GuideStep1Mirror)}" }
                            div { class: "flex flex-wrap gap-3 mt-2",
                                for file in &manifest.files {
                                    a {
                                        class: "inline-block px-4 py-2 rounded bg-emerald-700 hover:bg-emerald-600 text-white text-sm transition-colors",
                                        href: crate::net::api_url(&format!("/api/gamedata/channels/faf-client/files/{}", file.path)),
                                        "{t.t(Text::DownloadFafClient)} ({file.path})"
                                    }
                                }
                            }
                        },
                        _ => rsx! {
                            p { class: "text-sm text-neutral-300", "{t.t(Text::GuideStep1Github)}" }
                            a {
                                class: "inline-block mt-2 px-4 py-2 rounded bg-neutral-700 hover:bg-neutral-600 text-white text-sm transition-colors",
                                href: GITHUB_RELEASES,
                                target: "_blank",
                                "{t.t(Text::GuideStep1GithubBtn)}"
                            }
                        },
                    }
                    p { class: "text-xs text-neutral-500 mt-3", "{t.t(Text::GuideStep1Note)}" }
                }

                // Step 2: sync patches & map generator.
                GuideStepCard {
                    number: "2",
                    title: t.t(Text::GuideStep2Title),
                    p { class: "text-sm text-neutral-300", "{t.t(Text::GuideStep2Desc)}" }
                    Link {
                        class: "inline-block mt-2 px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 text-white text-sm transition-colors",
                        to: Route::Sync {},
                        "{t.t(Text::GuideStep2Btn)}"
                    }
                }

                // Step 3: accelerator first, then play.
                GuideStepCard {
                    number: "3",
                    title: t.t(Text::GuideStep3Title),
                    p { class: "text-sm text-neutral-300", "{t.t(Text::GuideStep3Desc)}" }
                }
            }
        }
    }
}

/// One numbered step card.
#[component]
fn GuideStepCard(number: String, title: String, children: Element) -> Element {
    rsx! {
        div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-5",
            div { class: "flex items-center gap-3 mb-3",
                span { class: "flex items-center justify-center w-8 h-8 rounded-full bg-blue-700 text-white font-bold text-sm shrink-0",
                    "{number}"
                }
                h2 { class: "text-lg font-semibold text-white", "{title}" }
            }
            {children}
        }
    }
}
