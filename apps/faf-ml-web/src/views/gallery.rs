use dioxus::prelude::*;
use faf_ml_core::ScreenshotMeta;
use gloo_net::http::Request;

use crate::Route;

/// Fetch the screenshot index from the server.
async fn fetch_screenshots() -> Result<Vec<ScreenshotMeta>, String> {
    Request::get(&crate::net::api_url("/api/screenshots"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<ScreenshotMeta>>()
        .await
        .map_err(|e| e.to_string())
}

/// Gallery: thumbnail grid + multi-file PNG upload + delete.
#[component]
pub fn Gallery() -> Element {
    // Bump to force the list resource to re-run after an upload/delete.
    let mut refresh = use_signal(|| 0u32);
    let mut status = use_signal(String::new);

    let shots = use_resource(move || async move {
        refresh();
        fetch_screenshots().await
    });

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans p-6",
            div { class: "max-w-6xl mx-auto",
                div { class: "flex items-center gap-4 mb-6",
                    h1 { class: "text-2xl font-bold text-white", "Screenshot gallery" }
                    div { class: "flex-1" }
                    // Multi-file upload: every selected PNG is POSTed in one
                    // multipart request.
                    label { class: "px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 text-white text-sm font-semibold cursor-pointer transition-colors",
                        "Upload PNGs"
                        input {
                            class: "hidden",
                            r#type: "file",
                            accept: "image/png",
                            multiple: true,
                            onchange: move |e| {
                                let files: Vec<web_sys::File> = e
                                    .files()
                                    .iter()
                                    .filter_map(|f| {
                                        f.inner().downcast_ref::<web_sys::File>().cloned()
                                    })
                                    .collect();
                                if files.is_empty() {
                                    return;
                                }
                                spawn(async move {
                                    let form = web_sys::FormData::new().unwrap();
                                    for file in &files {
                                        let _ = form.append_with_blob_and_filename(
                                            "files",
                                            file,
                                            &file.name(),
                                        );
                                    }
                                    let request = Request::post(
                                        &crate::net::api_url("/api/screenshots"),
                                    )
                                    .body(form);
                                    let response = match request {
                                        Ok(req) => req.send().await.map_err(|e| e.to_string()),
                                        Err(e) => Err(e.to_string()),
                                    };
                                    match response {
                                        Ok(resp) if resp.ok() => {
                                            status.set(format!("uploaded {} file(s)", files.len()));
                                        }
                                        Ok(resp) => {
                                            status
                                                .set(format!("upload failed: HTTP {}", resp.status()));
                                        }
                                        Err(e) => status.set(format!("upload failed: {e}")),
                                    }
                                    *refresh.write() += 1;
                                });
                            },
                        }
                    }
                }
                if !status.read().is_empty() {
                    p { class: "text-sm text-amber-400 mb-4", "{status}" }
                }
                match &*shots.read() {
                    None => rsx! { p { class: "text-neutral-400", "Loading..." } },
                    Some(Err(e)) => rsx! { p { class: "text-red-400", "{e}" } },
                    Some(Ok(list)) if list.is_empty() => rsx! {
                        p { class: "text-neutral-400", "No screenshots yet — upload some PNGs." }
                    },
                    Some(Ok(list)) => rsx! {
                        div { class: "grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-4",
                            for shot in list.iter() {
                                ShotCard { key: "{shot.id}", shot: shot.clone(), refresh }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// One thumbnail card linking to the label view, with a delete button.
#[component]
fn ShotCard(shot: ScreenshotMeta, refresh: Signal<u32>) -> Element {
    let id = shot.id.to_string();
    let delete_id = id.clone();
    let dimensions = format!("{}×{}", shot.width, shot.height);
    let uploaded = shot.uploaded_at.format("%Y-%m-%d %H:%M").to_string();
    rsx! {
        div { class: "rounded-lg border border-neutral-800 bg-neutral-900 overflow-hidden",
            Link {
                to: Route::Label { id },
                img {
                    class: "w-full aspect-video object-cover",
                    src: crate::net::image_url(&shot.id.to_string()),
                    alt: "{shot.filename}",
                }
            }
            div { class: "flex items-center gap-2 px-3 py-2",
                div { class: "min-w-0",
                    p { class: "text-xs text-neutral-300 truncate", "{shot.filename}" }
                    p { class: "text-xs text-neutral-500",
                        "{dimensions} · {uploaded}"
                    }
                }
                div { class: "flex-1" }
                button {
                    class: "px-2 py-1 rounded text-xs text-red-400 hover:bg-neutral-800 transition-colors",
                    title: "Delete screenshot and its labels",
                    onclick: move |_| {
                        let id = delete_id.clone();
                        spawn(async move {
                            let _ = Request::delete(&crate::net::api_url(&format!(
                                    "/api/screenshots/{id}"
                                )))
                                .send()
                                .await;
                            *refresh.write() += 1;
                        });
                    },
                    "Delete"
                }
            }
        }
    }
}
