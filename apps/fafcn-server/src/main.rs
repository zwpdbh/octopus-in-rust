//! Backend server for the fafcn-web construction simulator.
//!
//! Serves the Dioxus web build as static files and exposes:
//!
//! - `GET /api/units` — list all unit summaries.
//! - `GET /api/units/:id` — single unit blueprint.
//! - `GET /api/portraits/:id.png` — unit portrait image.
//! - `GET /ws/simulate` — WebSocket to run a simulation and stream events.

use axum::{
    extract::{Path, State, WebSocketUpgrade},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use faf_blueprints::{FafBlueprints, TechLevel, UnitBlueprint};
use faf_sim_protocol::{SimClientMessage, SimEvent, SimServerMessage};
use faf_sim_service::SimulationService;
use serde::Serialize;
use std::{path::PathBuf, sync::Arc};
use tower_http::cors::{Any, CorsLayer};
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

/// Shared application state.
#[derive(Clone)]
struct AppState {
    blueprints: Arc<FafBlueprints>,
    portraits_dir: Arc<PathBuf>,
    assets_dir: Arc<PathBuf>,
}

/// Summary sent to the frontend for unit selection.
#[derive(Serialize)]
struct UnitSummary {
    id: String,
    name: String,
    description: String,
    faction: String,
    tech_level: TechLevel,
    cost: faf_blueprints::UnitCostMetrics,
    eco_effect: faf_blueprints::UnitEffectEcoMetrics,
    category: Option<String>,
    kind: Option<String>,
    strategic_icon_name: Option<String>,
}

impl From<UnitBlueprint> for UnitSummary {
    fn from(bp: UnitBlueprint) -> Self {
        Self {
            id: bp.unit_id().to_string(),
            name: bp.unit_description().to_string(),
            description: bp.unit_description().to_string(),
            faction: infer_faction(bp.unit_id()).to_string(),
            tech_level: bp.tech_level(),
            cost: bp.unit_cost(),
            eco_effect: bp.unit_eco_effect().clone(),
            category: bp.category().map(|c| c.label().to_string()),
            kind: bp.kind().map(|k| k.label().to_string()),
            strategic_icon_name: bp.strategic_icon_name().map(|s| s.to_string()),
        }
    }
}

/// Best-effort faction inference from the second letter of a blueprint id.
///
/// FAF blueprint ids use the pattern `<category><faction>...`, where the
/// faction letter is `E` (UEF), `A` (Aeon), `R` (Cybran), or `S` (Seraphim).
fn infer_faction(id: &str) -> &str {
    match id.chars().nth(1) {
        Some('E') => "uef",
        Some('A') => "aeon",
        Some('R') => "cybran",
        Some('S') => "seraphim",
        _ => "unknown",
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let blueprints = Arc::new(FafBlueprints::new()?);

    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let portraits_dir = Arc::new(workspace_root.join("assets/icons/units"));
    let assets_dir = Arc::new(
        std::env::var("FAFCN_WEB_DIST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| workspace_root.join("target/dx/fafcn-web/release/web/public")),
    );

    tracing::info!("loaded {} units", blueprints.all_units().len());
    tracing::info!("serving static assets from {}", assets_dir.display());

    let state = AppState {
        blueprints,
        portraits_dir,
        assets_dir,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE]);

    let app = Router::new()
        .route("/api/units", get(list_units))
        .route("/api/units/:id", get(get_unit))
        .route("/api/portraits/:id", get(get_portrait))
        .route("/ws/simulate", get(simulate_ws_handler))
        .fallback_service(
            ServeDir::new(state.assets_dir.as_ref())
                .fallback(ServeFile::new(state.assets_dir.join("index.html"))),
        )
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}

async fn list_units(State(state): State<AppState>) -> impl IntoResponse {
    let units: Vec<UnitSummary> = state
        .blueprints
        .all_units()
        .into_iter()
        .filter(|bp| {
            let faction = infer_faction(bp.unit_id());
            matches!(faction, "uef" | "aeon" | "cybran" | "seraphim")
        })
        .map(UnitSummary::from)
        .collect();
    axum::Json(units)
}

async fn get_unit(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let blueprint = state.blueprints.get_one_unit_from_search(&id)?;
    Ok(axum::Json(blueprint))
}

async fn get_portrait(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let path = state
        .portraits_dir
        .join(format!("{}.png", id.to_ascii_uppercase()));
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    Ok(([(axum::http::header::CONTENT_TYPE, "image/png")], bytes))
}

async fn simulate_ws_handler(
    ws: WebSocketUpgrade,
    State(_state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: axum::extract::ws::WebSocket) {
    use axum::extract::ws::Message;

    // Wait for the client to send the plan to simulate.
    let start_msg = loop {
        match socket.recv().await {
            Some(Ok(Message::Text(text))) => {
                match serde_json::from_str::<SimClientMessage>(&text) {
                    Ok(SimClientMessage::StartPlan { plan, speed }) => {
                        break (plan, speed);
                    }
                    Ok(SimClientMessage::Command(_)) => {
                        let _ = send_json(
                            &mut socket,
                            &SimServerMessage::Error(
                                "expected StartPlan before commands".to_string(),
                            ),
                        )
                        .await;
                    }
                    Err(e) => {
                        let _ = send_json(
                            &mut socket,
                            &SimServerMessage::Error(format!("invalid message: {e}")),
                        )
                        .await;
                    }
                }
            }
            Some(Ok(Message::Close(_))) | None => return,
            _ => continue,
        }
    };

    let (plan, speed) = start_msg;

    // Start the simulation.  It runs in a background thread and communicates
    // through the controller's channels.
    let service = SimulationService::new();
    let controller = service.run(plan, speed);
    let cmd_tx = controller.cmd_tx;
    let event_rx = controller.event_rx;

    // Bridge the synchronous simulation event channel into the async task.
    let (event_tx, mut event_rx_async) = tokio::sync::mpsc::unbounded_channel::<SimEvent>();
    tokio::task::spawn_blocking(move || {
        while let Ok(event) = event_rx.recv() {
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    // Stream events to the client and read runtime commands in the same task.
    let mut finished = false;
    loop {
        tokio::select! {
            event = event_rx_async.recv() => {
                match event {
                    Some(event) => {
                        if send_json(&mut socket, &SimServerMessage::Event(event))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => {
                        // Simulation thread dropped the sender; queue is done.
                        finished = true;
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<SimClientMessage>(&text) {
                            Ok(SimClientMessage::Command(cmd)) => {
                                let _ = cmd_tx.send(cmd);
                            }
                            Ok(SimClientMessage::StartPlan { .. }) => {
                                let _ = send_json(
                                    &mut socket,
                                    &SimServerMessage::Error(
                                        "already started".to_string(),
                                    ),
                                )
                                .await;
                            }
                            Err(e) => {
                                let _ = send_json(
                                    &mut socket,
                                    &SimServerMessage::Error(format!("invalid message: {e}")),
                                )
                                .await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    if finished {
        let _ = send_json(&mut socket, &SimServerMessage::Finished).await;
    }
    let _ = socket.close().await;
}

async fn send_json(
    socket: &mut axum::extract::ws::WebSocket,
    message: &SimServerMessage,
) -> Result<(), axum::Error> {
    let text = serde_json::to_string(message).unwrap_or_default();
    socket.send(axum::extract::ws::Message::Text(text)).await
}

#[derive(Debug)]
enum AppError {
    NotFound,
    Blueprint(faf_blueprints::Error),
}

impl From<faf_blueprints::Error> for AppError {
    fn from(err: faf_blueprints::Error) -> Self {
        AppError::Blueprint(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "not found").into_response(),
            AppError::Blueprint(err) => {
                (StatusCode::INTERNAL_SERVER_ERROR, format!("{err}")).into_response()
            }
        }
    }
}
