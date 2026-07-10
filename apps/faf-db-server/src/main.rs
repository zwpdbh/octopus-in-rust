use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use faf_units::{DataIndex, Unit};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;
use tracing::info;

#[derive(Clone)]
struct AppState {
    index: Arc<DataIndex>,
}

#[derive(Debug, Clone, Serialize)]
struct UnitSummary {
    id: String,
    display_name: String,
    faction: String,
    tech: String,
    category: String,
    strategic_icon_name: Option<String>,
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
        index: Arc::new(index),
    };

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/api/units", get(list_units))
        .route("/api/units/:id", get(get_unit))
        .route("/api/portraits/:id", get(get_portrait))
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
        r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <title>FAF Unit DB</title>
  </head>
  <body>
    <div id="main"></div>
    <script type="module">
      import init from "/assets/dioxus/faf_db_web.js";
      init();
    </script>
  </body>
</html>"#
            .to_string(),
    )
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
enum BrowserCategory {
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
    fn label(self) -> &'static str {
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

fn browser_category(unit: &Unit) -> BrowserCategory {
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

fn workspace_path(rel: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().parent().unwrap().join(rel)
}
