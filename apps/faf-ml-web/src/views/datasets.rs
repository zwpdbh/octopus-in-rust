use dioxus::prelude::*;
use faf_ml_core::{DatasetManifest, ScreenshotMeta};
use gloo_net::http::Request;
use serde::Serialize;

/// Fetch all dataset snapshot manifests.
async fn fetch_datasets() -> Result<Vec<DatasetManifest>, String> {
    Request::get(&crate::net::api_url("/api/datasets"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<DatasetManifest>>()
        .await
        .map_err(|e| e.to_string())
}

/// Fetch the screenshot index (used to collect ids for a snapshot).
async fn fetch_screenshot_ids() -> Result<Vec<String>, String> {
    let metas = Request::get(&crate::net::api_url("/api/screenshots"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<ScreenshotMeta>>()
        .await
        .map_err(|e| e.to_string())?;
    Ok(metas.iter().map(|m| m.id.to_string()).collect())
}

/// Request body for `POST /api/datasets`.
#[derive(Serialize)]
struct CreateDatasetRequest {
    name: String,
    image_ids: Vec<String>,
}

/// Datasets: list immutable snapshots; create one from all current
/// screenshots (labels are embedded at snapshot time).
#[component]
pub fn Datasets() -> Element {
    let mut refresh = use_signal(|| 0u32);
    let mut name = use_signal(String::new);
    let mut status = use_signal(String::new);

    let datasets = use_resource(move || async move {
        refresh();
        fetch_datasets().await
    });

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans p-6",
            div { class: "max-w-4xl mx-auto",
                h1 { class: "text-2xl font-bold text-white mb-6", "Dataset snapshots" }

                // Create form: name + snapshot of all current screenshots.
                div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-4 mb-6",
                    h2 { class: "text-sm font-semibold text-white mb-3", "Create snapshot" }
                    div { class: "flex gap-2",
                        input {
                            class: "flex-1 px-3 py-2 rounded bg-neutral-800 border border-neutral-700 text-sm text-white",
                            placeholder: "dataset name (e.g. v1)",
                            value: "{name}",
                            oninput: move |e| name.set(e.value()),
                        }
                        button {
                            class: "px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 text-white text-sm font-semibold transition-colors",
                            onclick: move |_| {
                                let dataset_name = name.read().trim().to_string();
                                if dataset_name.is_empty() {
                                    status.set("name is required".to_string());
                                    return;
                                }
                                spawn(async move {
                                    let result = async {
                                        let image_ids = fetch_screenshot_ids().await?;
                                        Request::post(&crate::net::api_url("/api/datasets"))
                                            .json(&CreateDatasetRequest { name: dataset_name, image_ids })
                                            .map_err(|e| e.to_string())?
                                            .send()
                                            .await
                                            .map_err(|e| e.to_string())
                                    }
                                    .await;
                                    match result {
                                        Ok(resp) if resp.ok() => {
                                            status.set("snapshot created".to_string());
                                            name.set(String::new());
                                        }
                                        Ok(resp) => {
                                            status.set(format!("create failed: HTTP {}", resp.status()));
                                        }
                                        Err(e) => status.set(format!("create failed: {e}")),
                                    }
                                    *refresh.write() += 1;
                                });
                            },
                            "Snapshot all screenshots"
                        }
                    }
                    if !status.read().is_empty() {
                        p { class: "text-xs text-amber-400 mt-2", "{status}" }
                    }
                }

                match &*datasets.read() {
                    None => rsx! { p { class: "text-neutral-400", "Loading..." } },
                    Some(Err(e)) => rsx! { p { class: "text-red-400", "{e}" } },
                    Some(Ok(list)) if list.is_empty() => rsx! {
                        p { class: "text-neutral-400", "No datasets yet." }
                    },
                    Some(Ok(list)) => rsx! {
                        div { class: "space-y-2",
                            for ds in list.iter() {
                                DatasetRow { key: "{ds.name}", ds: ds.clone() }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// One dataset row: name, image/box counts, creation date.
#[component]
fn DatasetRow(ds: DatasetManifest) -> Element {
    let images = ds.entries.len();
    let boxes: usize = ds.entries.iter().map(|e| e.labels.len()).sum();
    let created = ds.created_at.format("%Y-%m-%d %H:%M UTC").to_string();
    rsx! {
        div { class: "flex items-center gap-4 rounded-lg border border-neutral-800 bg-neutral-900 px-4 py-3",
            span { class: "font-mono text-white", "{ds.name}" }
            div { class: "flex-1" }
            span { class: "text-sm text-neutral-400", "{images} images · {boxes} boxes" }
            span { class: "text-xs text-neutral-500", "{created}" }
        }
    }
}
