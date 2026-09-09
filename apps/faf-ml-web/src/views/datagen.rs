use std::time::Duration;

use dioxus::prelude::*;
use faf_ml_core::{DatagenConfig, DatagenJob, DatagenStatus};
use gloo_net::http::Request;

use crate::Route;

/// Fetch all datagen jobs (newest first).
async fn fetch_jobs() -> Result<Vec<DatagenJob>, String> {
    Request::get(&crate::net::api_url("/api/datagen/jobs"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<DatagenJob>>()
        .await
        .map_err(|e| e.to_string())
}

/// Start a datagen job; surfaces the server's error text (e.g. the 400
/// "mark backgrounds first" message) verbatim.
async fn start_job(config: &DatagenConfig) -> Result<(), String> {
    let resp = Request::post(&crate::net::api_url("/api/datagen"))
        .json(config)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.ok() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("HTTP {status}: {text}"))
    }
}

/// Parse one optional form field into a config slot (empty = keep default).
fn parse_field<T: std::str::FromStr>(field: &str, raw: &str, slot: &mut T) -> Result<(), String> {
    let raw = raw.trim();
    if !raw.is_empty() {
        *slot = raw
            .parse()
            .map_err(|_| format!("invalid {field}: {raw:?}"))?;
    }
    Ok(())
}

/// Parse the form fields into a config (empty field = server default).
fn parse_config(
    count: &str,
    size: &str,
    max_units: &str,
    scale_min: &str,
    scale_max: &str,
    seed: &str,
) -> Result<DatagenConfig, String> {
    let mut config = DatagenConfig::default();
    parse_field("count", count, &mut config.count)?;
    parse_field("size", size, &mut config.size)?;
    parse_field("max units", max_units, &mut config.max_units)?;
    parse_field("scale min", scale_min, &mut config.scale_min)?;
    parse_field("scale max", scale_max, &mut config.scale_max)?;
    parse_field("seed", seed, &mut config.seed)?;
    Ok(config)
}

/// Bump the refresh counter (free fn: signals are Copy, so this keeps the
/// effect/poll closures non-mutable).
fn bump_refresh(mut refresh: Signal<u32>) {
    *refresh.write() += 1;
}

/// Submit the form: start the job, then refresh the jobs table.
fn submit(
    count: Signal<String>,
    size: Signal<String>,
    max_units: Signal<String>,
    scale_min: Signal<String>,
    scale_max: Signal<String>,
    seed: Signal<String>,
    mut status: Signal<String>,
    refresh: Signal<u32>,
) {
    let config = match parse_config(
        &count.read(),
        &size.read(),
        &max_units.read(),
        &scale_min.read(),
        &scale_max.read(),
        &seed.read(),
    ) {
        Ok(config) => config,
        Err(e) => {
            status.set(e);
            return;
        }
    };
    spawn(async move {
        match start_job(&config).await {
            Ok(()) => status.set("job started — progress below".to_string()),
            Err(e) => status.set(e),
        }
        bump_refresh(refresh);
    });
}

/// Datagen: generation form + live job table (polls while jobs run).
#[component]
pub fn Datagen() -> Element {
    // Bump to force the jobs resource to re-run after a submit / poll tick.
    let refresh = use_signal(|| 0u32);
    let status = use_signal(String::new);
    let count = use_signal(String::new);
    let size = use_signal(String::new);
    let max_units = use_signal(String::new);
    let scale_min = use_signal(String::new);
    let scale_max = use_signal(String::new);
    let seed = use_signal(String::new);

    let jobs = use_resource(move || async move {
        refresh();
        fetch_jobs().await
    });

    // Poll every 2 s while any job is Running (no WebSocket until phase 2).
    use_effect(move || {
        let running = matches!(&*jobs.read(), Some(Ok(list)) if list
            .iter()
            .any(|j| matches!(j.status, DatagenStatus::Running { .. })));
        if running {
            spawn(async move {
                gloo_timers::future::sleep(Duration::from_secs(2)).await;
                bump_refresh(refresh);
            });
        }
    });

    let defaults = DatagenConfig::default();
    let count_placeholder = defaults.count.to_string();
    let size_placeholder = defaults.size.to_string();
    let max_units_placeholder = defaults.max_units.to_string();
    let scale_min_placeholder = defaults.scale_min.to_string();
    let scale_max_placeholder = defaults.scale_max.to_string();
    let seed_placeholder = defaults.seed.to_string();

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans p-6",
            div { class: "max-w-4xl mx-auto",
                h1 { class: "text-2xl font-bold text-white mb-6", "Synthetic data generation" }

                // Generation form (empty fields fall back to the defaults).
                div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-4 mb-6",
                    h2 { class: "text-sm font-semibold text-white mb-3", "Generate samples" }
                    p { class: "text-xs text-neutral-400 mb-3",
                        "Pastes strategic-icon sprites onto crops of "
                        b { "background" }
                        "-marked screenshots (triage them in the Gallery first). Samples stream "
                        "into the store as synthetic, auto-labeled screenshots."
                    }
                    div { class: "grid grid-cols-2 md:grid-cols-3 gap-2 mb-3",
                        Field { label: "count", placeholder: count_placeholder, value: count }
                        Field { label: "size (px)", placeholder: size_placeholder, value: size }
                        Field { label: "max units", placeholder: max_units_placeholder, value: max_units }
                        Field { label: "scale min", placeholder: scale_min_placeholder, value: scale_min }
                        Field { label: "scale max", placeholder: scale_max_placeholder, value: scale_max }
                        Field { label: "seed", placeholder: seed_placeholder, value: seed }
                    }
                    button {
                        class: "px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 text-white text-sm font-semibold transition-colors",
                        onclick: move |_| submit(count, size, max_units, scale_min, scale_max, seed, status, refresh),
                        "Generate"
                    }
                    if !status.read().is_empty() {
                        p { class: "text-xs text-amber-400 mt-2", "{status}" }
                    }
                }

                h2 { class: "text-sm font-semibold text-white mb-3", "Jobs" }
                match &*jobs.read() {
                    None => rsx! { p { class: "text-neutral-400", "Loading..." } },
                    Some(Err(e)) => rsx! { p { class: "text-red-400", "{e}" } },
                    Some(Ok(list)) if list.is_empty() => rsx! {
                        p { class: "text-neutral-400", "No jobs yet — jobs live in server memory and reset on restart." }
                    },
                    Some(Ok(list)) => rsx! {
                        div { class: "space-y-2",
                            for job in list.iter() {
                                JobRow { key: "{job.id}", job: job.clone() }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// One labeled form input bound to a string signal.
#[component]
fn Field(label: &'static str, placeholder: String, mut value: Signal<String>) -> Element {
    rsx! {
        label { class: "flex flex-col gap-1 text-xs text-neutral-400",
            "{label}"
            input {
                class: "px-3 py-2 rounded bg-neutral-800 border border-neutral-700 text-sm text-white",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |e| value.set(e.value()),
            }
        }
    }
}

/// One job row: config summary, progress, status; Done links to the
/// Gallery's synthetic filter.
#[component]
fn JobRow(job: DatagenJob) -> Element {
    let started = job.started_at.format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let summary = format!(
        "count {} · size {} · max-units {} · scale {:.2}–{:.2} · seed {}",
        job.config.count,
        job.config.size,
        job.config.max_units,
        job.config.scale_min,
        job.config.scale_max,
        job.config.seed,
    );
    let (status_text, status_class) = match &job.status {
        DatagenStatus::Running { done, total } => {
            (format!("running {done}/{total}"), "text-blue-300")
        }
        DatagenStatus::Done { generated } => {
            (format!("done — {generated} samples"), "text-green-300")
        }
        DatagenStatus::Failed { error } => (format!("failed: {error}"), "text-red-400"),
    };
    let is_done = matches!(job.status, DatagenStatus::Done { .. });

    rsx! {
        div { class: "rounded-lg border border-neutral-800 bg-neutral-900 px-4 py-3",
            div { class: "flex items-center gap-4",
                span { class: "font-mono text-xs text-neutral-500", "{started}" }
                span { class: "text-sm text-neutral-300", "{summary}" }
                div { class: "flex-1" }
                span { class: "text-sm {status_class}", "{status_text}" }
            }
            if is_done {
                p { class: "text-xs text-neutral-500 mt-1",
                    "Samples are in the store — open the "
                    Link { class: "text-blue-400 hover:underline", to: Route::Gallery {}, "Gallery" }
                    " and filter by \"synthetic\" to review them."
                }
            }
        }
    }
}
