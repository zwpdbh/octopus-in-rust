use dioxus::prelude::*;
use faf_sim::GameEcoMetrics;

use crate::components::EcoSnapshotView;
use crate::types::{
    Action, CandidateReasoning, CandidateScoreBreakdown, DirectionScores, PriorityTable,
    ScoreCategory, StepReasoning, StepResult, UnitKind,
};
use crate::utils::kind_label;

/// Ordered list of scheduled steps rendered as a todo list. Each row shows the
/// start/end time on the left and a concise instruction like
/// "4 Eng T1 build Mex T2" on the right. Clicking a row toggles an inline
/// details section that shows the economy state before the decision and the
/// top candidate actions considered.
#[component]
pub fn StepTimeline(
    steps: Vec<StepResult>,
    reasoning: Vec<StepReasoning>,
    initial_eco: GameEcoMetrics,
    initial_inventory: Vec<UnitKind>,
    mut selected_step: Signal<Option<usize>>,
) -> Element {
    rsx! {
        div { class: "flex-1 min-h-0 overflow-auto pr-1",
            if steps.is_empty() {
                div { class: "text-neutral-500 text-sm text-center py-8", "No steps in the schedule." }
            }
            div { class: "flex flex-col gap-1",
                for (idx , step) in steps.iter().enumerate() {
                    {
                        let is_selected = *selected_step.read() == Some(idx);
                        let (accent_border, accent_text) = step_accent(&step.action);
                        let (tag_label, tag_class) = step_tag(&step.action);
                        let description = describe_step(&step.action);
                        let row_class = if is_selected {
                            format!(
                                "flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent_border} bg-neutral-800 transition-colors cursor-pointer",
                            )
                        } else {
                            format!(
                                "flex items-center gap-3 w-full text-left px-3 py-2 rounded border {accent_border} bg-neutral-900/60 hover:bg-neutral-800/80 transition-colors cursor-pointer",
                            )
                        };
                        let start_seconds = if idx == 0 {
                            0.0
                        } else {
                            steps[idx - 1].finish_time_seconds
                        };
                        let end_seconds = step.finish_time_seconds;
                        let time_label = format_duration_range(start_seconds, end_seconds);
                        let pre_eco = pre_step_eco(&steps, &initial_eco, idx);
                        let mass_net = pre_eco.production_per_second_mass.value();
                        let mass_net_class = "text-emerald-300";
                        let mass_net_sign = if mass_net >= 0.0 { "+" } else { "" };
                        let step_reasoning = reasoning.get(idx).cloned();
                        let current_units = inventory_after_step(&initial_inventory, &steps, idx);
                        rsx! {
                            div { class: "flex flex-col",
                                div {
                                    class: "{row_class}",
                                    role: "button",
                                    tabindex: "0",
                                    onclick: move |_| {
                                        if is_selected {
                                            selected_step.set(None);
                                        } else {
                                            selected_step.set(Some(idx));
                                        }
                                    },
                                    span { class: "text-xs font-mono text-neutral-500 w-8 shrink-0 text-right", "#{idx + 1}" }
                                    span { class: "text-xs font-mono {mass_net_class} w-16 shrink-0 text-right whitespace-nowrap",
                                        "{mass_net_sign}{mass_net:.1}/s"
                                    }
                                    span { class: "w-14 shrink-0 inline-flex items-center justify-center px-1 py-0.5 rounded text-[10px] font-semibold uppercase tracking-wide {tag_class}",
                                        "{tag_label}"
                                    }
                                    span { class: "text-xs font-mono text-sky-300 shrink-0 w-48 whitespace-nowrap text-right",
                                        "{time_label}"
                                    }
                                    span { class: "flex-1 min-w-0 text-sm {accent_text} truncate", "{description}" }
                                    CopyStepButton {
                                        idx,
                                        start_seconds,
                                        end_seconds,
                                        step: step.clone(),
                                        reasoning: step_reasoning.clone(),
                                        pre_eco: pre_eco.clone(),
                                    }
                                }
                                if is_selected {
                                    StepDetails {
                                        step: step.clone(),
                                        reasoning: step_reasoning,
                                        pre_eco,
                                        current_units,
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

#[component]
fn CopyStepButton(
    idx: usize,
    start_seconds: f64,
    end_seconds: f64,
    step: StepResult,
    reasoning: Option<StepReasoning>,
    pre_eco: GameEcoMetrics,
) -> Element {
    rsx! {
        button {
            class: "ml-2 px-1.5 py-0.5 text-xs rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-400 hover:text-white border border-neutral-700 transition-colors shrink-0",
            title: "Copy step details for debug",
            onclick: move |e: MouseEvent| {
                e.stop_propagation();
                let payload = serde_json::json!(
                    { "step_number" : idx + 1, "start_seconds" : start_seconds, "end_seconds" :
                    end_seconds, "action" : step.action, "economy_before_decision" : pre_eco,
                    "reasoning" : reasoning, }
                );
                let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
                if let Some(window) = web_sys::window() {
                    let _ = window.navigator().clipboard().write_text(&text);
                }
            },
            "📋"
        }
    }
}

#[component]
fn StepDetails(
    step: StepResult,
    reasoning: Option<StepReasoning>,
    pre_eco: GameEcoMetrics,
    current_units: Vec<UnitKind>,
) -> Element {
    let (scores, priorities) = reasoning
        .as_ref()
        .map(|r| (r.direction_scores, r.priority_table))
        .unwrap_or_default();

    rsx! {
        div { class: "mt-2 rounded border border-neutral-700 bg-neutral-950/60 p-3 flex flex-col gap-4",
            // Top row: economy snapshot and current units next to decision scores.
            div { class: "flex flex-col lg:flex-row gap-4",
                EconomyBeforeDecision { pre_eco }
                CurrentUnits { units: current_units }
                DecisionScores { scores, priorities }
            }

            // Bottom row: candidate reasoning with relative score bars.
            TopCandidates { step, reasoning }
        }
    }
}

/// Economy state just before the scheduler committed this step.
#[component]
fn EconomyBeforeDecision(pre_eco: GameEcoMetrics) -> Element {
    rsx! {
        div { class: "flex flex-col gap-1 lg:w-[420px] shrink-0",
            h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide",
                "Economy before decision"
            }
            EcoSnapshotView { snapshot: pre_eco }
        }
    }
}

/// Units that exist after this step has been committed, grouped by kind.
#[component]
pub fn CurrentUnits(
    units: Vec<UnitKind>,
    #[props(default = "Units after step")] title: &'static str,
    #[props(default = "flex flex-col gap-1 lg:w-[260px] shrink-0")] class: &'static str,
) -> Element {
    let grouped = group_unit_counts(&units);
    rsx! {
        div { class: "{class}",
            h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide",
                "{title}"
            }
            div { class: "flex-1 rounded border border-neutral-800 bg-neutral-900/50 p-2 overflow-auto",
                if grouped.is_empty() {
                    p { class: "text-xs text-neutral-500", "No units." }
                } else {
                    div { class: "flex flex-col gap-1",
                        for (kind , count) in grouped {
                            div { class: "flex items-center justify-between gap-2",
                                span { class: "text-xs text-neutral-300 truncate", "{kind_label(&kind)}" }
                                span { class: "text-xs font-mono text-neutral-200 shrink-0",
                                    "× {count}"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Compute the inventory after committing step `idx`.
fn inventory_after_step(
    initial_inventory: &[UnitKind],
    steps: &[StepResult],
    idx: usize,
) -> Vec<UnitKind> {
    let mut inventory = initial_inventory.to_vec();
    for step in steps.iter().take(idx + 1) {
        apply_step_to_inventory(&mut inventory, step);
    }
    inventory
}

/// Compute the inventory after all steps have been committed.
pub fn inventory_after_steps(
    initial_inventory: &[UnitKind],
    steps: &[StepResult],
) -> Vec<UnitKind> {
    let mut inventory = initial_inventory.to_vec();
    for step in steps {
        apply_step_to_inventory(&mut inventory, step);
    }
    inventory
}

fn apply_step_to_inventory(inventory: &mut Vec<UnitKind>, step: &StepResult) {
    match &step.action {
        Action::Build { target, .. } => {
            inventory.push(target.clone());
        }
        Action::Upgrade { from, to, .. } => {
            if let Some(pos) = inventory.iter().position(|u| u == from) {
                inventory.remove(pos);
            }
            inventory.push(to.clone());
        }
    }
}

/// Group a unit list into (kind, count) pairs sorted by display label.
fn group_unit_counts(units: &[UnitKind]) -> Vec<(UnitKind, usize)> {
    let mut counts: std::collections::HashMap<UnitKind, usize> = std::collections::HashMap::new();
    for unit in units {
        *counts.entry(unit.clone()).or_insert(0) += 1;
    }
    let mut grouped: Vec<_> = counts.into_iter().collect();
    grouped.sort_by(|a, b| kind_label(&a.0).cmp(&kind_label(&b.0)));
    grouped
}

/// Direction-confidence and priority-weight tables used for this decision.
#[component]
fn DecisionScores(scores: DirectionScores, priorities: PriorityTable) -> Element {
    rsx! {
        div { class: "flex flex-col gap-2 flex-1 min-w-0",
            h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide",
                "Decision scores"
            }
            div { class: "flex flex-col sm:flex-row gap-2",
                ScoreBlock { label: "Direction confidence", scores }
                ScoreBlock { label: "Priority weights", scores: priorities }
            }
        }
    }
}

/// Highest-scoring candidate actions considered for this step, with a per-candidate
/// score breakdown.
#[component]
fn TopCandidates(step: StepResult, reasoning: Option<StepReasoning>) -> Element {
    let max_score = reasoning
        .as_ref()
        .and_then(|r| {
            r.top_candidates
                .iter()
                .map(|c| c.score)
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap_or(1.0)
        .max(0.001);

    rsx! {
        div { class: "flex flex-col gap-1",
            h5 { class: "text-[10px] font-semibold text-neutral-400 uppercase tracking-wide",
                "Top candidates"
            }
            if let Some(reasoning) = reasoning {
                if reasoning.top_candidates.is_empty() {
                    p { class: "text-xs text-neutral-500", "No candidate data recorded." }
                } else {
                    div { class: "flex flex-col gap-1",
                        for candidate in reasoning.top_candidates.iter() {
                            CandidateRow {
                                candidate: candidate.clone(),
                                is_chosen: candidate.action == step.action,
                                max_score,
                            }
                        }
                    }
                }
            } else {
                p { class: "text-xs text-neutral-500", "No reasoning available for this step." }
            }
        }
    }
}

/// Single candidate row showing its absolute score, action description, and a
/// compact score-computation breakdown. The chosen candidate is highlighted by
/// a category-coloured background that matches the step timeline.
#[component]
fn CandidateRow(candidate: CandidateReasoning, is_chosen: bool, max_score: f64) -> Element {
    let ratio = (candidate.score / max_score).clamp(0.0, 1.0);
    let pct = ratio * 100.0;
    let row_class = candidate_row_class(&candidate.action, is_chosen);
    let score_color = if is_chosen {
        candidate_score_color(&candidate.action)
    } else {
        "text-neutral-300"
    };
    rsx! {
        div { class: "{row_class}",
            // Absolute score and relative percentage.
            div { class: "flex flex-col items-end w-16 shrink-0 pt-0.5",
                span { class: "text-xs font-mono {score_color}", "{candidate.score:.1}" }
                span { class: "text-[10px] text-neutral-500", "{pct:.0}%" }
            }

            // Action description and computation breakdown.
            div { class: "flex-1 min-w-0 flex flex-col gap-1",
                span { class: "text-xs text-neutral-200 truncate", "{describe_step(&candidate.action)}" }
                if let Some(ref breakdown) = candidate.breakdown {
                    CandidateBreakdown { breakdown: breakdown.clone(), score: candidate.score }
                }
            }
        }
    }
}

/// Dispatcher that renders the appropriate score-computation breakdown for a
/// candidate.
#[component]
fn CandidateBreakdown(breakdown: CandidateScoreBreakdown, score: f64) -> Element {
    rsx! {
        div { class: "text-xs font-mono text-neutral-500",
            match breakdown {
                CandidateScoreBreakdown::Eco {
                    category,
                    confidence,
                    efficiency,
                    time_penalty,
                    priority,
                    priority_multiplier,
                    base,
                    ..
                } => rsx! {
                    EcoBreakdown {
                        category,
                        confidence,
                        efficiency,
                        time_penalty,
                        priority,
                        priority_multiplier,
                        base,
                        score,
                    }
                },
                CandidateScoreBreakdown::Unit { time_seconds, distance_to_target, .. } => {
                    rsx! {
                        UnitBreakdown { time_seconds, distance_to_target }
                    }
                }
            }
        }
    }
}

/// Eco-score formula terms: direction confidence, efficiency, time penalty,
/// priority multiplier, base score, and final score.
#[component]
fn EcoBreakdown(
    category: ScoreCategory,
    confidence: u8,
    efficiency: f64,
    time_penalty: f64,
    priority: u8,
    priority_multiplier: f64,
    base: f64,
    score: f64,
) -> Element {
    rsx! {
        div { class: "flex flex-col gap-0.5",
            div { class: "flex flex-wrap gap-x-2",
                ScoreCategoryLabel { category }
                span { "conf={confidence}" }
                span { "eff={efficiency:.4}" }
                span { "time_penalty={time_penalty:.6}" }
            }
            div { class: "flex flex-wrap gap-x-2",
                span { "base = {base:.4}" }
                span { "priority = {priority} (x{priority_multiplier:.2})" }
                span { class: "text-neutral-300", "score = {score:.4}" }
            }
        }
    }
}

/// Unit-score terms: graph distance to target and simulated completion time.
#[component]
fn UnitBreakdown(time_seconds: f64, distance_to_target: Option<u32>) -> Element {
    rsx! {
        div { class: "flex flex-wrap gap-x-2",
            if let Some(distance) = distance_to_target {
                span { "distance to target = {distance}," }
            } else {
                span { "direct build," }
            }
            span { "time = {time_seconds:.1}s" }
        }
    }
}

/// Human-readable label for a score category.
#[component]
fn ScoreCategoryLabel(category: ScoreCategory) -> Element {
    let label = match category {
        ScoreCategory::MassIncome => "mass income",
        ScoreCategory::Energy => "energy",
        ScoreCategory::BuildPower => "build power",
        ScoreCategory::TechT2 => "tech T2",
        ScoreCategory::TechT3 => "tech T3",
        ScoreCategory::Other => "other",
    };
    rsx! {
        span { class: "text-neutral-400", "{label}" }
    }
}

#[component]
fn ScoreBlock(label: &'static str, #[props(into)] scores: ScoreValues) -> Element {
    rsx! {
        div { class: "flex-1 rounded border border-neutral-800 bg-neutral-900/50 p-2",
            span { class: "text-[10px] text-neutral-500 block mb-1", "{label}" }
            div { class: "flex flex-col gap-1",
                for (name , value , max) in scores.rows() {
                    div {
                        class: "grid items-center gap-2",
                        style: "grid-template-columns: 1fr auto",
                        span { class: "text-xs text-neutral-400", "{name}" }
                        div { class: "flex items-center gap-2",
                            div { class: "w-16 h-1.5 rounded bg-neutral-800 overflow-hidden",
                                div {
                                    class: "h-full bg-blue-500",
                                    style: "width: {(value as f64 / max as f64 * 100.0):.1}%",
                                }
                            }
                            span { class: "font-mono text-neutral-200 text-xs w-5 text-right",
                                "{value}"
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum ScoreValues {
    Directions(DirectionScores),
    Priorities(PriorityTable),
}

impl Default for ScoreValues {
    fn default() -> Self {
        ScoreValues::Directions(DirectionScores::default())
    }
}

impl ScoreValues {
    fn rows(&self) -> Vec<(&'static str, u8, u8)> {
        match self {
            ScoreValues::Directions(s) => vec![
                ("Energy", s.energy, 100),
                ("Mass income", s.mass_income, 100),
                ("Build power", s.build_power, 100),
                ("Tech T2", s.tech_t2, 100),
                ("Tech T3", s.tech_t3, 100),
            ],
            ScoreValues::Priorities(p) => vec![
                ("Mass", p.mass, 10),
                ("Energy", p.energy, 10),
                ("Build power", p.build_power, 10),
            ],
        }
    }
}

impl From<DirectionScores> for ScoreValues {
    fn from(value: DirectionScores) -> Self {
        ScoreValues::Directions(value)
    }
}

impl From<PriorityTable> for ScoreValues {
    fn from(value: PriorityTable) -> Self {
        ScoreValues::Priorities(value)
    }
}

fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u32;
    let mins = total / 60;
    let secs = total % 60;
    format!("{:02}:{:02}", mins, secs)
}

fn format_duration_range(start_seconds: f64, end_seconds: f64) -> String {
    let duration = (end_seconds - start_seconds).max(0.0) as u32;
    let duration_text = if duration < 60 {
        format!("{}s", duration)
    } else {
        let mins = duration / 60;
        let secs = duration % 60;
        if secs == 0 {
            format!("{} {}", mins, if mins == 1 { "min" } else { "mins" })
        } else {
            format!(
                "{} {} {}s",
                mins,
                if mins == 1 { "min" } else { "mins" },
                secs
            )
        }
    };
    format!(
        "{} -> {} ({})",
        format_duration(start_seconds),
        format_duration(end_seconds),
        duration_text
    )
}

fn pre_step_eco(
    steps: &[StepResult],
    initial_eco: &GameEcoMetrics,
    idx: usize,
) -> GameEcoMetrics {
    if idx == 0 {
        initial_eco.clone()
    } else {
        steps
            .get(idx - 1)
            .map(|s| s.economy.clone())
            .unwrap_or_else(|| initial_eco.clone())
    }
}

fn describe_step(action: &Action) -> String {
    match action {
        Action::Build { target, builder } => {
            let builder_text = describe_builders(builder);
            format!("{} build {}", builder_text, kind_label(target))
        }
        Action::Upgrade {
            from,
            to,
            assisted_by,
        } => {
            let assist_text = if assisted_by.is_empty() {
                String::new()
            } else {
                format!(" (assisted by {})", describe_builders(assisted_by))
            };
            format!(
                "{} upgrade to {}{}",
                kind_label(from),
                kind_label(to),
                assist_text
            )
        }
    }
}

/// Short label and badge classes for a step row based on what the step builds
/// or upgrades.
fn step_tag(action: &Action) -> (&'static str, &'static str) {
    if is_mex_related(action) {
        return (
            "Mass",
            "bg-emerald-900/50 text-emerald-300 border border-emerald-700/50",
        );
    }
    if is_upgrade(action) {
        let label = upgrade_label(action);
        return (
            label,
            "bg-purple-900/50 text-purple-300 border border-purple-700/50",
        );
    }
    match action {
        Action::Build { target, .. } => match target {
            UnitKind::Pgen(_) => (
                "Power",
                "bg-yellow-900/30 text-yellow-300 border border-yellow-600/40",
            ),
            _ => (
                "Build",
                "bg-sky-900/30 text-sky-300 border border-sky-700/40",
            ),
        },
        Action::Upgrade { .. } => unreachable!(),
    }
}

fn upgrade_label(action: &Action) -> &'static str {
    match action {
        Action::Upgrade { to: target, .. } => match target {
            UnitKind::Pgen(_) => "Power",
            UnitKind::Factory(_) => "Tech",
            _ => "Upgrade",
        },
        _ => "Upgrade",
    }
}

/// Border and text colour classes for a step row based on what the step builds
/// or upgrades.
fn step_accent(action: &Action) -> (&'static str, &'static str) {
    if is_mex_related(action) {
        return (
            "border-emerald-700 hover:border-emerald-600",
            "text-emerald-300",
        );
    }
    if is_upgrade(action) {
        return (
            "border-purple-700 hover:border-purple-600",
            "text-purple-300",
        );
    }
    match action {
        Action::Build { target, .. } => match target {
            UnitKind::Pgen(_) => (
                "border-yellow-600 hover:border-yellow-500",
                "text-yellow-300",
            ),
            _ => ("border-sky-700 hover:border-sky-600", "text-sky-300"),
        },
        Action::Upgrade { .. } => unreachable!(),
    }
}

/// Background and border classes for a candidate row. The chosen candidate uses
/// a category colour matching the step timeline; non-chosen rows stay neutral.
fn candidate_row_class(action: &Action, is_chosen: bool) -> &'static str {
    if !is_chosen {
        return "flex items-start gap-3 px-2 py-1.5 rounded bg-neutral-900/40 border border-neutral-800";
    }
    if is_mex_related(action) {
        return "flex items-start gap-3 px-2 py-1.5 rounded bg-emerald-900/20 border border-emerald-700/50";
    }
    if is_upgrade(action) {
        return "flex items-start gap-3 px-2 py-1.5 rounded bg-purple-900/20 border border-purple-700/50";
    }
    match action {
        Action::Build { target, .. } => match target {
            UnitKind::Pgen(_) => {
                "flex items-start gap-3 px-2 py-1.5 rounded bg-yellow-900/20 border border-yellow-700/50"
            }
            _ => "flex items-start gap-3 px-2 py-1.5 rounded bg-sky-900/20 border border-sky-700/50",
        },
        Action::Upgrade { .. } => unreachable!(),
    }
}

/// Score text colour for a chosen candidate row, matching its category.
fn candidate_score_color(action: &Action) -> &'static str {
    if is_mex_related(action) {
        return "text-emerald-300";
    }
    if is_upgrade(action) {
        return "text-purple-300";
    }
    match action {
        Action::Build { target, .. } => match target {
            UnitKind::Pgen(_) => "text-yellow-300",
            _ => "text-sky-300",
        },
        Action::Upgrade { .. } => unreachable!(),
    }
}

/// True when the action involves mass extractors (Mex or CapMex) in either the
/// source or target unit.
fn is_mex_related(action: &Action) -> bool {
    match action {
        Action::Build { target, .. } => is_mex_kind(target),
        Action::Upgrade { from, to, .. } => is_mex_kind(from) || is_mex_kind(to),
    }
}

fn is_mex_kind(kind: &UnitKind) -> bool {
    matches!(kind, UnitKind::Mex(_) | UnitKind::CapMex(_))
}

/// True when the action is any upgrade (e.g. mex cap, factory tech, etc.).
fn is_upgrade(action: &Action) -> bool {
    matches!(action, Action::Upgrade { .. })
}

/// Summarise a list of builders as a count + kind string. Identical consecutive
/// kinds are collapsed; mixed kinds are listed with counts.
fn describe_builders(builders: &[UnitKind]) -> String {
    if builders.is_empty() {
        return "0 builders".to_string();
    }
    let mut groups: Vec<(UnitKind, usize)> = Vec::new();
    for b in builders {
        if let Some((last_kind, count)) = groups.last_mut() {
            if last_kind == b {
                *count += 1;
                continue;
            }
        }
        groups.push((b.clone(), 1));
    }
    groups
        .iter()
        .map(|(kind, count)| format!("{} {}", count, kind_label(kind)))
        .collect::<Vec<_>>()
        .join(" + ")
}
