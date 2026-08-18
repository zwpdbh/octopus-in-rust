use dioxus::prelude::*;
use fafcn_gamedata::{Manifest, StatusResponse};
use gloo_net::http::Request;

use crate::i18n::{self, Text};

/// Localized channel display title.
fn channel_title(t: i18n::T, name: &str) -> &'static str {
    match name {
        fafcn_gamedata::CHANNEL_MAP_GENERATOR => t.t(Text::ChannelMapGenerator),
        fafcn_gamedata::CHANNEL_FAF_CLIENT => t.t(Text::FafClientTitle),
        _ => t.t(Text::ChannelGamedata),
    }
}

/// Gamedata mirror status and sync-client download page.
#[component]
pub fn Sync() -> Element {
    let t = i18n::use_t();
    let client_download_url =
        crate::net::api_url("/api/gamedata/client/fafcn-sync-x86_64-pc-windows-gnu.exe");
    let status = use_resource(move || async move {
        Request::get(&crate::net::api_url("/api/gamedata/status"))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json::<StatusResponse>()
            .await
            .map_err(|e| e.to_string())
    });
    // The faf-client manifest carries the installer file list for download links.
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

    let client_tag = match status.read().as_ref() {
        Some(Ok(resp)) => resp.client_tag.clone(),
        _ => None,
    };

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans",
            div { class: "max-w-2xl mx-auto px-6 py-8 flex flex-col gap-6",
                h1 { class: "text-2xl font-bold text-white", "{t.t(Text::SyncTitle)}" }
                p { class: "text-sm text-neutral-400", "{t.t(Text::SyncIntro)}" }

                // Mirror status card.
                div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-5",
                    h2 { class: "text-lg font-semibold text-white mb-3", "{t.t(Text::MirrorStatus)}" }
                    match status.read().as_ref() {
                        None => rsx! {
                            p { class: "text-neutral-400 text-sm", "{t.t(Text::LoadingStatus)}" }
                        },
                        Some(Err(err)) => rsx! {
                            p { class: "text-red-400 text-sm", "{t.t(Text::LoadStatusFailed)}{err}" }
                        },
                        Some(Ok(resp)) => {
                            let all_empty = resp.channels.iter().all(|c| c.manifest.is_none());
                            rsx! {
                                if all_empty {
                                    p { class: "text-amber-400 text-sm", "{t.t(Text::MirrorEmpty)}" }
                                }
                                for ch in &resp.channels {
                                    div { class: "mb-3 last:mb-0",
                                        h3 { class: "text-sm font-semibold text-blue-300 mb-2", {channel_title(t, &ch.name)} }
                                        match &ch.manifest {
                                            None => rsx! {
                                                p { class: "text-neutral-500 text-xs", "{t.t(Text::ChannelNotPublished)}" }
                                            },
                                            Some(m) => {
                                                let last_updated =
                                                    m.last_updated.format("%Y-%m-%d %H:%M UTC").to_string();
                                                let total_mb = format!("{:.1} MB", m.total_size as f64 / 1e6);
                                                rsx! {
                                                    dl { class: "grid grid-cols-2 gap-y-1.5 text-xs",
                                                        dt { class: "text-neutral-400", "{t.t(Text::PatchVersion)}" }
                                                        dd { class: "text-white font-mono", "{m.patch_version}" }
                                                        dt { class: "text-neutral-400", "{t.t(Text::LastUpdated)}" }
                                                        dd { class: "text-white font-mono", "{last_updated}" }
                                                        dt { class: "text-neutral-400", "{t.t(Text::UploadedBy)}" }
                                                        dd { class: "text-white", "{m.uploader}" }
                                                        dt { class: "text-neutral-400", "{t.t(Text::FileCount)}" }
                                                        dd { class: "text-white", "{m.file_count}" }
                                                        dt { class: "text-neutral-400", "{t.t(Text::TotalSize)}" }
                                                        dd { class: "text-white", "{total_mb}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Client download + usage.
                div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-5",
                    h2 { class: "text-lg font-semibold text-white mb-3", "{t.t(Text::SyncClient)}" }
                    a {
                        class: "inline-block px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 text-white text-sm transition-colors",
                        href: "{client_download_url}",
                        "{t.t(Text::DownloadClient)}"
                    }
                    p { class: "mt-2 text-xs text-neutral-500 font-mono",
                        "{t.t(Text::ClientVersion)}: "
                        {
                            match &client_tag {
                                Some(tag) => rsx! {
                                    span { class: "text-green-400", "{tag}" }
                                },
                                None => rsx! {
                                    span { "{t.t(Text::ClientVersionMissing)}" }
                                },
                            }
                        }
                    }
                    ol { class: "list-decimal list-inside mt-4 space-y-2 text-sm text-neutral-300",
                        li { "{t.t(Text::SyncStepDownload)}" }
                        li { "{t.t(Text::SyncStepFirstRun)}" }
                        li { "{t.t(Text::SyncStepResync)}" }
                        li { "{t.t(Text::SyncStepPlay)}" }
                    }
                    p { class: "mt-4 text-xs text-neutral-500", "{t.t(Text::SyncClientNote)}" }
                    p { class: "mt-2 text-xs text-neutral-500", "{t.t(Text::UploadHint)}" }
                }
            }
        }
    }
}
