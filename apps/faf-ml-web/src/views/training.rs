use dioxus::prelude::*;
use faf_dioxus_ui::{ChartMetric, ChartSeries, ChartTab, RGBColor, UplotChart};
use faf_ml_core::{
    TrainingCommand, TrainingConfig, TrainingMetricsPoint, TrainingServerMessage, TrainingStatus,
};

use crate::components::TrainingConnection;

/// Page lifecycle; drives which controls are enabled (mirrors fafcn's
/// `SimulationStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageStatus {
    Idle,
    Running,
    Paused,
    Finished,
}

// ── chart extractors (fn pointers for `ChartMetric`) ────────────────────────

fn x_seq(p: &TrainingMetricsPoint) -> f64 {
    p.seq as f64
}
fn y_train(p: &TrainingMetricsPoint) -> f64 {
    p.train_loss
}
fn y_cls(p: &TrainingMetricsPoint) -> f64 {
    p.cls_loss
}
fn y_bbox(p: &TrainingMetricsPoint) -> f64 {
    p.bbox_loss
}
/// `None` → NaN: uPlot leaves a gap, so epoch-eval series appear as points.
fn y_valid(p: &TrainingMetricsPoint) -> f64 {
    p.valid_loss.unwrap_or(f64::NAN)
}
fn y_map(p: &TrainingMetricsPoint) -> f64 {
    p.map.unwrap_or(f64::NAN)
}

/// Parse the three config fields (empty = keep default).
fn parse_config(epochs: &str, batch_size: &str, lr: &str) -> Result<TrainingConfig, String> {
    let mut config = TrainingConfig::default();
    let raw = epochs.trim();
    if !raw.is_empty() {
        config.epochs = raw
            .parse()
            .map_err(|_| format!("invalid epochs: {raw:?}"))?;
    }
    let raw = batch_size.trim();
    if !raw.is_empty() {
        config.batch_size = raw
            .parse()
            .map_err(|_| format!("invalid batch size: {raw:?}"))?;
    }
    let raw = lr.trim();
    if !raw.is_empty() {
        config.lr = raw.parse().map_err(|_| format!("invalid lr: {raw:?}"))?;
    }
    Ok(config)
}

/// Training monitor: start/pause/resume/reset a (currently dummy) training
/// run and watch its metrics live. Mirrors fafcn's simulate page: a
/// WebSocket streams events into a signal, uPlot renders the buffer.
#[component]
pub fn Training() -> Element {
    let mut status = use_signal(|| PageStatus::Idle);
    let mut detail = use_signal(String::new);
    let mut connection = use_signal(|| None::<TrainingConnection>);
    let mut data: Signal<Vec<TrainingMetricsPoint>> = use_signal(Vec::new);
    let mut latest: Signal<Option<TrainingMetricsPoint>> = use_signal(|| None);

    let defaults = TrainingConfig::default();
    let epochs = use_signal(|| defaults.epochs.to_string());
    let batch_size = use_signal(|| defaults.batch_size.to_string());
    let mut lr = use_signal(|| defaults.lr.to_string());
    let speed = use_signal(|| "10".to_string());
    let speed_value = move || speed.read().parse::<f64>().unwrap_or(10.0);

    // Forward speed changes to a running/paused job (mirrors the sim page).
    use_effect(move || {
        let s = speed_value();
        if matches!(*status.read(), PageStatus::Running | PageStatus::Paused) {
            if let Some(conn) = connection.read().as_ref() {
                conn.send_command(TrainingCommand::SetSpeed { batches_per_sec: s });
            }
        }
    });

    let start = move |_| {
        let config = match parse_config(&epochs.read(), &batch_size.read(), &lr.read()) {
            Ok(config) => config,
            Err(e) => {
                detail.set(e);
                return;
            }
        };
        data.write().clear();
        latest.set(None);
        detail.set(String::new());

        let on_message = move |msg: TrainingServerMessage| match msg {
            TrainingServerMessage::Metrics(point) => {
                latest.set(Some(point.clone()));
                data.write().push(point);
            }
            TrainingServerMessage::Status(TrainingStatus::Running) => {
                status.set(PageStatus::Running);
            }
            TrainingServerMessage::Status(TrainingStatus::Paused) => {
                status.set(PageStatus::Paused);
            }
            TrainingServerMessage::Status(TrainingStatus::Done { duration_secs }) => {
                status.set(PageStatus::Finished);
                detail.set(format!("done in {duration_secs}s"));
            }
            TrainingServerMessage::Status(TrainingStatus::Failed { error }) => {
                status.set(PageStatus::Finished);
                detail.set(format!("failed: {error}"));
            }
            TrainingServerMessage::Finished | TrainingServerMessage::Error(_) => {}
        };
        let on_status = move |text: String| {
            if text == "finished" {
                status.set(PageStatus::Finished);
            } else if text != "connected" {
                detail.set(text);
            }
        };
        match TrainingConnection::open(config, speed_value(), on_message, on_status) {
            Ok(conn) => connection.set(Some(conn)),
            Err(e) => detail.set(format!("failed to connect: {e:?}")),
        }
    };

    let send = move |cmd: TrainingCommand| {
        if let Some(conn) = connection.read().as_ref() {
            conn.send_command(cmd);
        }
    };

    let reset = move |_| {
        if let Some(conn) = connection.read().as_ref() {
            conn.close();
        }
        connection.set(None);
        data.write().clear();
        latest.set(None);
        status.set(PageStatus::Idle);
        detail.set(String::new());
    };

    let can_start = matches!(*status.read(), PageStatus::Idle | PageStatus::Finished);
    let can_pause = matches!(*status.read(), PageStatus::Running);
    let can_resume = matches!(*status.read(), PageStatus::Paused);
    let can_reset = !matches!(*status.read(), PageStatus::Idle);
    let (badge_text, badge_class) = match *status.read() {
        PageStatus::Idle => ("idle", "bg-neutral-800 text-neutral-400"),
        PageStatus::Running => ("running", "bg-blue-900 text-blue-200"),
        PageStatus::Paused => ("paused", "bg-amber-900 text-amber-200"),
        PageStatus::Finished => ("finished", "bg-green-900 text-green-200"),
    };

    rsx! {
        div { class: "flex-1 overflow-y-auto bg-neutral-950 text-gray-200 font-sans p-6",
            div { class: "max-w-6xl mx-auto",
                div { class: "flex items-center gap-3 mb-4",
                    h1 { class: "text-2xl font-bold text-white", "Training monitor" }
                    span { class: "px-2 py-0.5 rounded text-xs font-semibold {badge_class}", "{badge_text}" }
                }

                // Controls.
                div { class: "rounded-lg border border-neutral-800 bg-neutral-900 p-4 mb-4",
                    p { class: "text-xs text-neutral-400 mb-3",
                        "Runs a training job on the server (currently a "
                        b { "dummy pipeline" }
                        " generating realistic curves) and streams metrics over a WebSocket — the same path the real burn training will use."
                    }
                    div { class: "grid grid-cols-2 md:grid-cols-4 gap-x-6 gap-y-3 mb-4",
                        SliderField { label: "epochs", min: "1", max: "100", step: "1", value: epochs }
                        SliderField { label: "batch size", min: "1", max: "8", step: "1", value: batch_size }
                        SliderField { label: "speed (batches/s)", min: "1", max: "50", step: "1", value: speed }
                        label { class: "flex flex-col gap-1 text-xs text-neutral-400",
                            div { class: "flex items-center justify-between",
                                span { "learning rate" }
                                span { class: "font-mono text-sm text-neutral-100", "{lr}" }
                            }
                            input {
                                class: "px-3 py-1.5 rounded bg-neutral-800 border border-neutral-700 text-sm text-white",
                                value: "{lr}",
                                oninput: move |e| lr.set(e.value()),
                            }
                        }
                    }
                    div { class: "flex items-center gap-2",
                        button {
                            class: "px-4 py-2 rounded bg-blue-700 hover:bg-blue-600 disabled:opacity-40 disabled:hover:bg-blue-700 text-white text-sm font-semibold transition-colors",
                            disabled: !can_start,
                            onclick: start,
                            "Start"
                        }
                        button {
                            class: "px-3 py-2 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 text-neutral-200 text-sm transition-colors",
                            disabled: !can_pause,
                            onclick: move |_| send(TrainingCommand::Pause),
                            "Pause"
                        }
                        button {
                            class: "px-3 py-2 rounded bg-neutral-800 hover:bg-neutral-700 disabled:opacity-40 text-neutral-200 text-sm transition-colors",
                            disabled: !can_resume,
                            onclick: move |_| send(TrainingCommand::Resume),
                            "Resume"
                        }
                        button {
                            class: "px-3 py-2 rounded bg-red-900/60 hover:bg-red-800 disabled:opacity-40 text-red-200 text-sm transition-colors",
                            disabled: !can_reset,
                            onclick: reset,
                            "Reset"
                        }
                        if let Some(point) = latest.read().as_ref() {
                            span { class: "ml-2 text-xs text-neutral-400 tabular-nums",
                                "epoch {point.epoch} · batch {point.batch} · train {point.train_loss:.4} · cls {point.cls_loss:.4} · bbox {point.bbox_loss:.4}"
                            }
                        }
                        if !detail.read().is_empty() {
                            span { class: "ml-2 text-xs text-amber-400", "{detail}" }
                        }
                    }
                }

                // Charts (stacked so each gets the full page width).
                div { class: "flex flex-col gap-4",
                    div {
                        h2 { class: "text-sm font-semibold text-white mb-2", "Loss" }
                        // Sized flex parent: UplotChart's root is `flex-1`, which
                        // only gets height inside a flex container (its own
                        // border/background is built in).
                        div { class: "h-96 flex flex-col",
                            UplotChart {
                                data,
                                x_extractor: ChartMetric::new(x_seq),
                                tabs: vec![ChartTab {
                                    label: "loss".to_string(),
                                    series: vec![
                                        ChartSeries::new("train", RGBColor(240, 240, 240), ChartMetric::new(y_train)),
                                        ChartSeries::new("cls", RGBColor(96, 165, 250), ChartMetric::new(y_cls)),
                                        ChartSeries::new("bbox", RGBColor(251, 146, 60), ChartMetric::new(y_bbox)),
                                        ChartSeries {
                                            label: "valid".to_string(),
                                            color: RGBColor(74, 222, 128),
                                            y_extractor: ChartMetric::new(y_valid),
                                            dash: Some(vec![4.0, 4.0]),
                                            span_gaps: true,
                                        },
                                    ],
                                }],
                            }
                        }
                    }
                    div {
                        h2 { class: "text-sm font-semibold text-white mb-2", "mAP (dummy)" }
                        div { class: "h-96 flex flex-col",
                            UplotChart {
                                data,
                                x_extractor: ChartMetric::new(x_seq),
                                tabs: vec![ChartTab {
                                    label: "mAP".to_string(),
                                    series: vec![ChartSeries::new(
                                        "mAP",
                                        RGBColor(192, 132, 252),
                                        ChartMetric::new(y_map),
                                    )
                                    .with_span_gaps()],
                                }],
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One labeled slider (string signal; mirrors the datagen form).
#[component]
fn SliderField(
    label: &'static str,
    min: &'static str,
    max: &'static str,
    step: &'static str,
    mut value: Signal<String>,
) -> Element {
    rsx! {
        label { class: "flex flex-col gap-1 text-xs text-neutral-400",
            div { class: "flex items-center justify-between",
                span { "{label}" }
                span { class: "font-mono text-sm text-neutral-100", "{value}" }
            }
            input {
                r#type: "range",
                class: "w-full accent-blue-500",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |e| value.set(e.value()),
            }
        }
    }
}
