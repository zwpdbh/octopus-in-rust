mod blueprint_graph;
mod schedule_api;

use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use faf_blueprints::BlueprintLibrary;
use faf_build_scheduler::Scheduler;
use faf_units::{DataIndex, Unit};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tracing::info;

#[derive(Clone)]
struct AppState {
    index: Arc<DataIndex>,
    sim_service: Arc<faf_sim_service::SimulationService>,
    scheduler: Arc<Scheduler>,
    blueprint_graph: Arc<blueprint_graph::ConcreteGraphResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UnitSummary {
    id: String,
    display_name: String,
    faction: String,
    tech: String,
    category: String,
    strategic_icon_name: Option<String>,
    kind: String,
    build_rate: Option<f64>,
    build_cost_mass: Option<f64>,
    build_cost_energy: Option<f64>,
    build_time: Option<f64>,
    production_per_second_mass: Option<f64>,
    production_per_second_energy: Option<f64>,
    maintenance_consumption_per_second_energy: Option<f64>,
    mass_storage: Option<f64>,
    energy_storage: Option<f64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let data_path = workspace_path("plugins/faf-units/data/faf_units.json");
    let assets_path = workspace_path("assets");

    info!("Loading unit database from {}", data_path.display());
    let json = std::fs::read_to_string(&data_path)?;
    let index: DataIndex = serde_json::from_str(&json)?;
    info!("Loaded {} units", index.units.len());

    let state = AppState {
        blueprint_graph: Arc::new(blueprint_graph::concrete_graph_response(&index)),
        scheduler: Arc::new(Scheduler::new(BlueprintLibrary::new(index.clone()))),
        index: Arc::new(index),
        sim_service: Arc::new(faf_sim_service::SimulationService::new()),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/units", get(list_units))
        .route("/api/units/:id", get(get_unit))
        .route("/api/blueprint-graph", get(blueprint_graph))
        .route("/api/schedule", post(schedule_api::schedule))
        .route("/api/portraits/:id", get(get_portrait))
        .route("/ws/simulate", get(simulate_ws_handler))
        .nest_service("/assets", ServeDir::new(assets_path))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await?;
    info!("Server listening on http://localhost:8081");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index_handler() -> impl IntoResponse {
    // Production fallback shell. In development `dx serve` provides its own index.html.
    axum::response::Html(
        r##"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>FAF Unit DB</title>
  </head>
  <body>
    <div id="main"></div>
    <script src="/assets/g6.min.js"></script>
    <script>
(function () {
  "use strict";
  function toG6Data(input) {
    const nodes = input.nodes.map(function (n) {
      return { id: n.id, label: n.label, color: n.color || "#f8f9fa", layer: n.layer, icon: n.icon, highlight: !!n.highlight, data: n.data };
    });
    const edges = input.edges.map(function (e, i) {
      return { id: "e" + i, source: e.source, target: e.target, color: e.color || "#9ca3af", dashed: !!e.dashed };
    });
    return { nodes, edges };
  }
  window.fafG6 = {
    graph: null,
    init: async function (containerId, jsonString) {
      this.destroy();
      const container = document.getElementById(containerId);
      if (!container) { console.error("[fafG6] container not found:", containerId); return; }
      await new Promise(requestAnimationFrame);
      console.log("[fafG6] init in container", containerId, "size", container.clientWidth, container.clientHeight);
      let input;
      try { input = JSON.parse(jsonString); }
      catch (err) { console.error("[fafG6] failed to parse JSON:", err); return; }
      const data = toG6Data(input);
      console.log("[fafG6] data nodes:", data.nodes.length, "edges:", data.edges.length);
      try {
        const graph = new G6.Graph({
          container: container,
          autoFit: "view",
          autoResize: true,
          data: data,
          layout: { type: "antv-dagre", rankdir: "LR", ranksep: 120, nodesep: 50, edgesep: 20, align: "UL" },
          node: {
            type: "image",
            style: function (d) {
              return {
                src: d.icon, size: [56, 56], cursor: "pointer",
                labelText: d.label, labelPlacement: "bottom", labelFill: "#e5e7eb", labelFontSize: 12, labelMaxWidth: 110,
                halo: true, haloStroke: d.highlight ? "#ffffff" : d.color, haloLineWidth: d.highlight ? 6 : 4, haloOpacity: d.highlight ? 0.9 : 0.4
              };
            }
          },
          edge: {
            type: "cubic-horizontal",
            style: function (d) {
              return { stroke: d.color, lineWidth: 1.5, lineDash: d.dashed ? [4, 4] : [], endArrow: true, endArrowFill: d.color, endArrowSize: 10 };
            }
          },
          behaviors: ["drag-canvas", "zoom-canvas"]
        });
        graph.on("node:click", function (e) {
          const id = e.item && e.item.id;
          if (id) { document.dispatchEvent(new CustomEvent("faf:g6-node-click", { detail: id })); }
        });
        await graph.render();
        console.log("[fafG6] render complete");
        this.graph = graph;
      } catch (err) {
        console.error("[fafG6] failed to create/render graph:", err);
      }
    },
    destroy: function () {
      if (this.graph) { try { this.graph.destroy(); } catch (err) { console.error("[fafG6] destroy failed:", err); } this.graph = null; }
    }
  };
})();
    </script>
    <script type="module">
      import init from "/assets/dioxus/faf_db_web.js";
      init();
    </script>
  </body>
</html>"##
            .to_string(),
    )
}

async fn blueprint_graph(
    State(state): State<AppState>,
) -> Json<blueprint_graph::ConcreteGraphResponse> {
    Json(state.blueprint_graph.as_ref().clone())
}

async fn list_units(State(state): State<AppState>) -> Json<Vec<UnitSummary>> {
    let summaries: Vec<_> = state
        .index
        .units
        .iter()
        .map(|unit| UnitSummary {
            id: unit.id.clone(),
            display_name: unit.display_name(),
            faction: unit.faction().unwrap_or("Unknown").to_string(),
            tech: unit.tech_level().unwrap_or("TECH1").to_string(),
            category: browser_category(unit).label().to_string(),
            strategic_icon_name: unit.strategic_icon_name.clone(),
            kind: unit_kind(unit).to_string(),
            build_rate: unit.economy.as_ref().and_then(|e| e.build_rate),
            build_cost_mass: unit.economy.as_ref().and_then(|e| e.build_cost_mass),
            build_cost_energy: unit.economy.as_ref().and_then(|e| e.build_cost_energy),
            build_time: unit.economy.as_ref().and_then(|e| e.build_time),
            production_per_second_mass: unit
                .economy
                .as_ref()
                .and_then(|e| e.production_per_second_mass),
            production_per_second_energy: unit
                .economy
                .as_ref()
                .and_then(|e| e.production_per_second_energy),
            maintenance_consumption_per_second_energy: unit
                .economy
                .as_ref()
                .and_then(|e| e.maintenance_consumption_per_second_energy),
            mass_storage: unit.economy.as_ref().and_then(|e| e.storage_mass),
            energy_storage: unit.economy.as_ref().and_then(|e| e.storage_energy),
        })
        .collect();
    Json(summaries)
}

async fn get_unit(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Unit>, StatusCode> {
    state
        .index
        .find_unit(&id)
        .map(|unit| Json(unit.clone()))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn get_portrait(Path(id): Path<String>) -> Result<impl IntoResponse, StatusCode> {
    let id = id.strip_suffix(".png").map(|s| s.to_string()).unwrap_or(id);
    let path = workspace_path("assets")
        .join("icons")
        .join("units")
        .join(format!("{id}.png"));
    tracing::info!("Serving portrait: {}", path.display());
    match std::fs::read(&path) {
        Ok(bytes) => Ok(([(axum::http::header::CONTENT_TYPE, "image/png")], bytes)),
        Err(e) => {
            tracing::warn!("Portrait not found: {} ({e})", path.display());
            Err(StatusCode::NOT_FOUND)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BrowserCategory {
    Land,
    Air,
    Naval,
    StructuresFactories,
    StructuresEconomy,
    StructuresWeapons,
    StructuresSupport,
    StructuresIntelligence,
    ConstructionBuildpower,
    Experimental,
}

impl BrowserCategory {
    pub(crate) fn label(self) -> &'static str {
        match self {
            BrowserCategory::Land => "Land",
            BrowserCategory::Air => "Air",
            BrowserCategory::Naval => "Naval",
            BrowserCategory::StructuresFactories => "Structures - Factories",
            BrowserCategory::StructuresEconomy => "Structures - Economy",
            BrowserCategory::StructuresWeapons => "Structures - Weapons",
            BrowserCategory::StructuresSupport => "Structures - Support",
            BrowserCategory::StructuresIntelligence => "Structures - Intelligence",
            BrowserCategory::ConstructionBuildpower => "Construction - Buildpower",
            BrowserCategory::Experimental => "Experimental",
        }
    }
}

pub(crate) fn unit_kind(unit: &Unit) -> &'static str {
    if unit.has_category("MOBILE") {
        if unit.has_category("AIR") {
            return "Air";
        }
        if unit.has_category("NAVAL") {
            return "Naval";
        }
        return "Land";
    }
    if unit.has_category("STRUCTURE") {
        return "Base";
    }
    "Unknown"
}

pub(crate) fn browser_category(unit: &Unit) -> BrowserCategory {
    if unit.has_category("ENGINEER") {
        return BrowserCategory::ConstructionBuildpower;
    }
    if unit.has_category("EXPERIMENTAL") || unit.has_category("TECH4") {
        return BrowserCategory::Experimental;
    }
    if unit.has_category("FACTORY") && !unit.has_category("GATE") {
        return BrowserCategory::StructuresFactories;
    }
    if unit.has_category("STRUCTURE") {
        if unit.has_category("INTELLIGENCE")
            || unit.has_category("OMNI")
            || unit.has_category("RADAR")
            || unit.has_category("SONAR")
        {
            return BrowserCategory::StructuresIntelligence;
        }
        if unit.has_category("ECONOMIC")
            || unit.has_category("MASSEXTRACTION")
            || unit.has_category("ENERGYPRODUCTION")
            || unit.has_category("ENERGYSTORAGE")
            || unit.has_category("MASSSTORAGE")
        {
            return BrowserCategory::StructuresEconomy;
        }
        if unit.has_category("WEAPON")
            || unit.has_category("ARTILLERY")
            || unit.has_category("NUKE")
            || unit.has_category("ANTIMISSILE")
        {
            return BrowserCategory::StructuresWeapons;
        }
        return BrowserCategory::StructuresSupport;
    }
    if unit.has_category("AIR") {
        return BrowserCategory::Air;
    }
    if unit.has_category("NAVAL") {
        return BrowserCategory::Naval;
    }
    BrowserCategory::Land
}

async fn simulate_ws_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_simulation_socket(state.sim_service.clone(), socket))
}

async fn handle_simulation_socket(
    service: Arc<faf_sim_service::SimulationService>,
    mut socket: axum::extract::ws::WebSocket,
) {
    use axum::extract::ws::Message;
    use faf_quantities::{StepTime, Time};
    use faf_sim::protocol::{SimClientMessage, SimServerMessage};
    use faf_sim_service::SimServiceEvent;

    // Wait for the client to start or subscribe to a simulation.
    let (sim_id, rx) = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<SimClientMessage>(&text) {
                    Ok(SimClientMessage::Start {
                        queue,
                        dt_seconds,
                        max_time_seconds,
                        mode,
                    }) => {
                        let Some(dt) = StepTime::from_seconds(dt_seconds) else {
                            let error = SimServerMessage::Error {
                                message: "dt_seconds must be >= 1".to_string(),
                            };
                            if send_server_message(&mut socket, error).await.is_err() {
                                return;
                            }
                            continue;
                        };
                        let max_time = max_time_seconds.map(|s| Time::from_raw(s as f64));
                        let id = match mode {
                            faf_sim::protocol::SimulationMode::Active => {
                                service.start_active_sim(queue, dt, max_time)
                            }
                            faf_sim::protocol::SimulationMode::Passive { tick_interval_ms } => {
                                service.start_passive_sim(queue, dt, max_time, tick_interval_ms)
                            }
                        };
                        let started = SimServerMessage::Started { simulation_id: id };
                        if send_server_message(&mut socket, started).await.is_err() {
                            return;
                        }
                        match service.subscribe(id) {
                            Ok(rx) => break (id, rx),
                            Err(e) => {
                                if send_server_message(
                                    &mut socket,
                                    SimServerMessage::Error {
                                        message: e.to_string(),
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    Ok(SimClientMessage::Subscribe { simulation_id }) => {
                        match service.subscribe(simulation_id) {
                            Ok(rx) => break (simulation_id, rx),
                            Err(e) => {
                                if send_server_message(
                                    &mut socket,
                                    SimServerMessage::Error {
                                        message: e.to_string(),
                                    },
                                )
                                .await
                                .is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                    Ok(_) => {
                        let error = SimServerMessage::Error {
                            message: "expected Start or Subscribe first".to_string(),
                        };
                        if send_server_message(&mut socket, error).await.is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        let error = SimServerMessage::Error {
                            message: format!("invalid message: {e}"),
                        };
                        if send_server_message(&mut socket, error).await.is_err() {
                            return;
                        }
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return,
            Some(Ok(_)) => continue,
            Some(Err(e)) => {
                tracing::warn!("WebSocket receive error: {e}");
                return;
            }
        }
    };

    // Bridge the synchronous simulation receiver into the async WebSocket task.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<SimServiceEvent>();
    tokio::task::spawn_blocking(move || {
        while let Ok(event) = rx.recv() {
            let is_finished = matches!(
                event,
                SimServiceEvent::Simulation(faf_sim::sim::SimulationEvent::Finished)
            );
            if event_tx.send(event).is_err() || is_finished {
                break;
            }
        }
    });

    // Stream events to the client while listening for control messages.
    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                let is_finished = matches!(
                    event,
                    SimServiceEvent::Simulation(faf_sim::sim::SimulationEvent::Finished)
                );
                let msg = match event {
                    SimServiceEvent::Simulation(e) => SimServerMessage::Event(e),
                    SimServiceEvent::Control(e) => SimServerMessage::ControlEvent(e),
                };
                if send_server_message(&mut socket, msg).await.is_err() {
                    return;
                }
                if is_finished {
                    break;
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<SimClientMessage>(&text) {
                            Ok(SimClientMessage::Pause { .. }) => {
                                let _ = service.pause(sim_id);
                            }
                            Ok(SimClientMessage::Resume { .. }) => {
                                let _ = service.resume(sim_id);
                            }
                            Ok(SimClientMessage::Stop { .. }) => {
                                let _ = service.stop(sim_id);
                                break;
                            }
                            Ok(SimClientMessage::Advance { dt_seconds, .. }) => {
                                if let Some(dt) = StepTime::from_seconds(dt_seconds) {
                                    let _ = service.advance(sim_id, dt);
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    // Close the WebSocket gracefully.
    let _ = socket.close().await;
}

async fn send_server_message(
    socket: &mut axum::extract::ws::WebSocket,
    msg: faf_sim::protocol::SimServerMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(&msg).unwrap_or_else(|_| "{}".to_string());
    socket.send(axum::extract::ws::Message::Text(text)).await
}

fn workspace_path(rel: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().parent().unwrap().join(rel)
}
