use dioxus::prelude::*;
use faf_blueprints::{ConstructionPlan, PlayerEcoMetrics};
use faf_sim_protocol::{SimCmd, SimEvent};

use crate::components::{
    to_sim_speed, use_sim_connection, EcoChart, EcoPoint, EcoStats, PlanEditor, QueueBuilder,
    SimulationControls, SimulationStatus,
};

const DEFAULT_PLAN: &str = r#"{
  "player_eco": {
    "mass_generate_rate": 2.0,
    "mass_drain": 0.0,
    "energy_generate_rate": 20.0,
    "energy_drain": 0.0,
    "mass_in_storage": 650.0,
    "max_capacity_in_mass_storage": 650.0,
    "energy_in_storage": 4000.0,
    "max_capacity_in_energy_storage": 4000.0
  },
  "building_queue": []
}"#;

const SAMPLE_PLAN: &str = r#"{
  "player_eco": {
    "mass_generate_rate": 2.0,
    "mass_drain": 0.0,
    "energy_generate_rate": 20.0,
    "energy_drain": 0.0,
    "mass_in_storage": 650.0,
    "max_capacity_in_mass_storage": 650.0,
    "energy_in_storage": 4000.0,
    "max_capacity_in_energy_storage": 4000.0
  },
  "building_queue": [
    {
      "builders": [
        {
          "unit_id": "UEL0309",
          "unit_description": "Engineer",
          "unit_cost": {"mass": 312.0, "energy": 1560.0, "build_time": 1560.0},
          "unit_eco_effect": {"generate_mass_rate": 0.0, "generate_energy_rate": 0.0, "maintainance_energy_drain": 0.0, "increase_mass_storage_capacity": 0.0, "increase_energy_storage_capacity": 0.0, "build_power": 32.5},
          "tech_level": "T3"
        }
      ],
      "target": {
        "unit_id": "UEB0101",
        "unit_description": "Land Factory",
        "unit_cost": {"mass": 240.0, "energy": 2100.0, "build_time": 300.0},
        "unit_eco_effect": {"generate_mass_rate": 0.0, "generate_energy_rate": 0.0, "maintainance_energy_drain": 0.0, "increase_mass_storage_capacity": 0.0, "increase_energy_storage_capacity": 0.0, "build_power": 20.0},
        "tech_level": "T1"
      }
    }
  ]
}"#;

#[component]
pub fn Simulate() -> Element {
    let mut plan_json = use_signal(|| DEFAULT_PLAN.to_string());
    let mut plan_error = use_signal(|| None::<String>);
    let mut status = use_signal(|| SimulationStatus::Idle);
    let speed = use_signal(|| 0.0_f64);
    let mut latest_eco = use_signal(|| None::<PlayerEcoMetrics>);
    let mut chart_data = use_signal(Vec::<EcoPoint>::new);
    let mut status_msg = use_signal(|| String::new());
    let mut sim_time = use_signal(|| 0.0_f64);

    let mut connection = use_sim_connection();

    let on_start = move |_| match serde_json::from_str::<ConstructionPlan>(&plan_json.read()) {
        Ok(plan) => {
            plan_error.set(None);
            chart_data.write().clear();
            status.set(SimulationStatus::Running);
            status_msg.set("connecting...".to_string());

            let mut status_writer = status;
            let mut eco_writer = latest_eco;
            let mut data_writer = chart_data;
            let mut msg_writer = status_msg;
            let mut conn_writer = connection;

            match crate::components::SimConnection::open(
                plan,
                to_sim_speed(*speed.read()),
                move |event| match event {
                    SimEvent::EcoSummary(eco) => {
                        sim_time.with_mut(|t| {
                            *t += 1.0;
                            data_writer.write().push(EcoPoint::new(*t, &eco));
                        });
                        eco_writer.set(Some(eco));
                    }
                    SimEvent::ActionFinished(_) => {
                        msg_writer.set("action finished".to_string());
                    }
                },
                move |msg| {
                    if msg == "finished" {
                        status_writer.set(SimulationStatus::Finished);
                    }
                    msg_writer.set(msg);
                },
            ) {
                Ok(conn) => conn_writer.set(Some(conn)),
                Err(err) => {
                    plan_error.set(Some(format!("failed to connect: {err:?}")));
                    status_writer.set(SimulationStatus::Idle);
                }
            }
        }
        Err(err) => {
            plan_error.set(Some(format!("invalid plan: {err}")));
        }
    };

    let on_pause = move |_| {
        if let Some(conn) = connection.read().as_ref() {
            conn.send_command(SimCmd::Pause);
            status.set(SimulationStatus::Paused);
        }
    };

    let on_resume = move |_| {
        if let Some(conn) = connection.read().as_ref() {
            conn.send_command(SimCmd::Resume);
            status.set(SimulationStatus::Running);
        }
    };

    let on_reset = move |_| {
        if let Some(conn) = connection.read().as_ref() {
            conn.close();
        }
        connection.set(None);
        chart_data.write().clear();
        latest_eco.set(None);
        sim_time.set(0.0);
        status.set(SimulationStatus::Idle);
        status_msg.set(String::new());
    };

    rsx! {
        div { class: "flex flex-col h-full bg-neutral-950 text-gray-200 overflow-hidden",
            div { class: "flex-1 flex min-h-0 p-4 gap-4",
                // Left sidebar: plan editor and queue builder.
                div { class: "w-96 flex flex-col gap-4 shrink-0 overflow-y-auto",
                    div { class: "border border-neutral-700 rounded-lg bg-neutral-900/80 p-4",
                        PlanEditor { plan_json, error: plan_error }
                        div { class: "mt-3 flex gap-2",
                            button {
                                class: "px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs transition-colors",
                                onclick: move |_| plan_json.set(SAMPLE_PLAN.to_string()),
                                "Load Sample"
                            }
                            button {
                                class: "px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 text-neutral-300 text-xs transition-colors",
                                onclick: move |_| plan_json.set(DEFAULT_PLAN.to_string()),
                                "Reset"
                            }
                        }
                    }
                    div { class: "border border-neutral-700 rounded-lg bg-neutral-900/80 p-4",
                        h3 { class: "text-sm font-semibold text-neutral-300 mb-2", "Queue Builder" }
                        QueueBuilder { plan_json }
                    }
                }

                // Right area: controls, stats, chart.
                div { class: "flex-1 flex flex-col gap-4 min-h-0",
                    div { class: "flex items-center justify-between border border-neutral-700 rounded-lg bg-neutral-900/80 p-3",
                        SimulationControls {
                            status,
                            speed,
                            on_start,
                            on_pause,
                            on_resume,
                            on_reset,
                        }
                        div { class: "text-xs text-neutral-400", "{status_msg}" }
                    }

                    div { class: "border border-neutral-700 rounded-lg bg-neutral-900/80 p-3",
                        EcoStats { eco: latest_eco }
                    }

                    div { class: "flex-1 border border-neutral-700 rounded-lg bg-neutral-900/80 p-3 min-h-0 flex flex-col",
                        EcoChart { data: chart_data }
                    }
                }
            }
        }
    }
}
