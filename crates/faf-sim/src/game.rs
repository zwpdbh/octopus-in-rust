//! Minimal Bevy-powered eco/build simulator.
//!
//! The user selects a unit, opens its build palette, chooses a target, and
//! places it on the board. The selected builder constructs the target while
//! the shared economy drains mass and energy. Completed units are rendered
//! with FAF strategic icons and grouped by category in the UI.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass, EguiTextureHandle, EguiUserTextures};
use faf_units::DataIndex;

use crate::economy::{apply_tick_graph, compute_drain, EconomyState, RequestedBuildPower};
use crate::units::{category_of, UnitCategory, Units};
use crate::units::UnitKind;
use crate::BuildDrain;

const BOARD_SIZE: i32 = 16;
const TILE_SIZE: f32 = 32.0;

/// Category used to group units in the browser panel, mirroring etfreeman-db sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
enum BrowserCategory {
    #[default]
    Commander,
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
            BrowserCategory::Commander => "Commander",
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

/// Derive a browser category for a unit kind using the raw index for unique units.
fn browser_category_for_kind(kind: &UnitKind, units: &Units, index: &DataIndex) -> BrowserCategory {
    match kind {
        UnitKind::Unique(id) => index
            .find_unit(&id.0)
            .map(browser_category)
            .unwrap_or(BrowserCategory::Land),
        _ => {
            let id = units.blueprint_id(kind).unwrap_or_default();
            index
                .find_unit(&id)
                .map(browser_category)
                .unwrap_or(BrowserCategory::Land)
        }
    }
}

/// Derive a browser category from a raw unit.
fn browser_category(unit: &faf_units::Unit) -> BrowserCategory {
    if unit.has_category("COMMAND") {
        return BrowserCategory::Commander;
    }
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
        if unit.has_category("INTELLIGENCE") || unit.has_category("OMNI") || unit.has_category("RADAR") || unit.has_category("SONAR") {
            return BrowserCategory::StructuresIntelligence;
        }
        if unit.has_category("ECONOMIC") || unit.has_category("MASSEXTRACTION") || unit.has_category("ENERGYPRODUCTION") || unit.has_category("ENERGYSTORAGE") || unit.has_category("MASSSTORAGE") {
            return BrowserCategory::StructuresEconomy;
        }
        if unit.has_category("WEAPON") || unit.has_category("ARTILLERY") || unit.has_category("NUKE") || unit.has_category("ANTIMISSILE") {
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

/// Wrapper so the unit database can live as a Bevy resource.
#[derive(Resource, Debug, Clone)]
struct UnitLibrary {
    units: Units,
    index: DataIndex,
}

/// Wrapper so the economy state can live as a Bevy resource.
#[derive(Resource, Debug, Clone, Copy)]
struct EcoState(EconomyState);

/// Board geometry configuration.
#[derive(Resource, Debug, Clone, Copy)]
struct BoardConfig {
    tile_size: f32,
}

impl BoardConfig {
    fn world_pos(&self, x: i32, y: i32) -> Vec3 {
        let offset = (BOARD_SIZE as f32) / 2.0 - 0.5;
        Vec3::new(
            (x as f32 - offset) * self.tile_size,
            (y as f32 - offset) * self.tile_size,
            0.0,
        )
    }

    fn grid_pos(&self, world: Vec2) -> Option<(i32, i32)> {
        let offset = (BOARD_SIZE as f32) / 2.0 - 0.5;
        let x = (world.x / self.tile_size + offset).floor() as i32;
        let y = (world.y / self.tile_size + offset).floor() as i32;
        if x >= 0 && x < BOARD_SIZE && y >= 0 && y < BOARD_SIZE {
            Some((x, y))
        } else {
            None
        }
    }
}

/// Current simulation wall-clock time in seconds.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct SimTime(f64);

/// Global stall factor computed each tick.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct StallFactor(f64);

/// The unit currently selected by the player.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct SelectedUnit(Option<Entity>);

/// The unit currently under the mouse cursor.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct HoveredUnit(Option<Entity>);

/// The build target currently chosen from a builder's palette.
#[derive(Resource, Debug, Clone, Default)]
struct SelectedBuildTarget(Option<UnitKind>);

/// Unit kind selected in the browser panel for detail viewing.
#[derive(Resource, Debug, Clone, Default)]
struct BrowserSelection(Option<UnitKind>);

/// Currently active category tab in the unit browser.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct BrowserCategoryFilter(BrowserCategory);

/// Completed unit kinds, used to check build prerequisites.
#[derive(Resource, Debug, Clone, Default)]
struct CompletedUnitKinds(HashSet<UnitKind>);

/// Count of completed units grouped by category.
#[derive(Resource, Debug, Clone, Default)]
struct UnitCounts(HashMap<UnitCategory, usize>);

/// Loaded strategic icon textures keyed by unit kind.
#[derive(Resource, Debug, Clone, Default)]
struct IconAtlas(HashMap<UnitKind, Handle<Image>>);

/// A plain white 1x1 texture used for colored sprites that have no portrait.
#[derive(Resource, Debug, Clone)]
struct FallbackImage(Handle<Image>);

/// Grid coordinate of a board entity.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GridPos {
    x: i32,
    y: i32,
}

/// The kind of unit represented by an entity.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
struct UnitKindComp(UnitKind);

/// A unit that can build other units.
#[derive(Component, Debug, Clone, Copy)]
struct Builder {
    power: f64,
}

/// A unit that contributes to economy income.
#[derive(Component, Debug, Clone, Copy)]
#[allow(dead_code)]
struct Producer {
    mass_income: f64,
    energy_income: f64,
}

/// A construction site currently being built.
#[derive(Component, Debug, Clone)]
struct ConstructionSite {
    target: UnitKind,
    remaining_work: f64,
    total_work: f64,
    power: f64,
}

/// The builder assigned to a construction site.
#[derive(Component, Debug, Clone, Copy)]
struct AssignedBuilder(Entity);

/// Visual marker for the currently selected unit.
#[derive(Component, Debug, Clone, Copy)]
struct SelectionOutline;

/// Queue of construction sites that finished this frame.
#[derive(Resource, Debug, Clone, Default)]
struct PendingCompletions(Vec<ConstructionCompleted>);

/// Event fired when a construction site finishes.
#[derive(Debug, Clone)]
struct ConstructionCompleted {
    entity: Entity,
    grid_pos: GridPos,
    target: UnitKind,
}

/// Marker for board tile entities so they are not treated as units.
#[derive(Component, Debug, Clone, Copy)]
struct BoardTile;

/// Run the interactive simulator.
pub fn run() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FAF Eco Sim".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(UnitLibrary::load())
        .insert_resource(EcoState(initial_economy()))
        .insert_resource(BoardConfig {
            tile_size: TILE_SIZE,
        })
        .insert_resource(SimTime(0.0))
        .insert_resource(StallFactor(1.0))
        .insert_resource(SelectedUnit::default())
        .insert_resource(HoveredUnit::default())
        .insert_resource(SelectedBuildTarget::default())
        .insert_resource(BrowserSelection::default())
        .insert_resource(BrowserCategoryFilter::default())
        .insert_resource(CompletedUnitKinds::default())
        .insert_resource(UnitCounts::default())
        .insert_resource(IconAtlas::default())
        .insert_resource(PendingCompletions::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                input_system,
                hover_system,
                eco_system,
                progress_system,
                completion_system,
                render_system,
                selection_visual_system,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, ui_system)
        .run();
}

impl UnitLibrary {
    fn load() -> Self {
        let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
        let index: DataIndex =
            serde_json::from_str(json).expect("embedded FAF unit index should parse");
        Self {
            units: Units::new(index.clone()),
            index,
        }
    }
}

/// Create a 1x1 white RGBA image for use as a colored sprite base.
fn create_white_image() -> Image {
    let mut image = Image::default();
    image.resize(bevy::render::render_resource::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    });
    image.data = Some(vec![255, 255, 255, 255]);
    image
}

fn initial_economy() -> EconomyState {
    use crate::quantities::{Energy, EnergyRate, Mass, MassRate};
    EconomyState {
        net_mass_income: MassRate::from_raw(1.0),
        net_energy_income: EnergyRate::from_raw(20.0),
        mass_storage: Mass::from_raw(650.0),
        energy_storage: Energy::from_raw(3900.0),
        mass_storage_cap: Mass::from_raw(650.0),
        energy_storage_cap: Energy::from_raw(3900.0),
    }
}

fn setup(
    mut commands: Commands,
    board: Res<BoardConfig>,
    library: Res<UnitLibrary>,
    mut eco: ResMut<EcoState>,
    asset_server: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut atlas: ResMut<IconAtlas>,
    mut egui_textures: ResMut<EguiUserTextures>,
    mut completed: ResMut<CompletedUnitKinds>,
    mut counts: ResMut<UnitCounts>,
) {
    commands.spawn(Camera2d);

    // Create a fallback white texture so colored sprites always render, even before portraits load.
    let fallback = FallbackImage(images.add(create_white_image()));
    let fallback_handle = fallback.0.clone();
    commands.insert_resource(fallback.clone());

    // Spawn the empty board tiles.
    for x in 0..BOARD_SIZE {
        for y in 0..BOARD_SIZE {
            commands.spawn((
                Sprite {
                    image: fallback_handle.clone(),
                    color: Color::srgb(0.30, 0.33, 0.30),
                    custom_size: Some(Vec2::new(board.tile_size - 2.0, board.tile_size - 2.0)),
                    ..default()
                },
                Transform::from_translation(board.world_pos(x, y)),
                GridPos { x, y },
                BoardTile,
            ));
        }
    }

    // Pre-load portrait icons for all known unit kinds so every spawned unit can use them.
    preload_portraits(&library.units, &asset_server, &mut atlas);

    // Register the loaded portraits as egui user textures so the UI can display them.
    for handle in atlas.0.values() {
        egui_textures.add_image(EguiTextureHandle::Strong(handle.clone()));
    }

    // Spawn the ACU in the center as the starting builder + producer.
    let acu_pos = GridPos {
        x: BOARD_SIZE / 2,
        y: BOARD_SIZE / 2,
    };
    let acu_def = library
        .units
        .def(&UnitKind::Commander)
        .expect("ACU defined");
    spawn_unit(
        &mut commands,
        &board,
        &library.units,
        &mut atlas,
        &fallback,
        acu_pos,
        UnitKind::Commander,
        true,
    );

    // Apply the ACU's production to the starting economy.
    eco.0.net_mass_income =
        eco.0.net_mass_income + crate::quantities::MassRate::from_raw(acu_def.mass_income());
    eco.0.net_energy_income = eco.0.net_energy_income
        + crate::quantities::EnergyRate::from_raw(
            acu_def.energy_income() - acu_def.maintenance_energy(),
        );

    // The ACU is given at game start and counts as completed.
    completed.0.insert(UnitKind::Commander);
    *counts
        .0
        .entry(category_of(&UnitKind::Commander))
        .or_insert(0) += 1;
}

/// Spawn a completed unit on the board.
fn spawn_unit(
    commands: &mut Commands,
    board: &BoardConfig,
    units: &Units,
    atlas: &mut IconAtlas,
    fallback: &FallbackImage,
    pos: GridPos,
    kind: UnitKind,
    is_acu: bool,
) -> Entity {
    let def = units.def(&kind).expect("unit definition should exist");
    let size = TILE_SIZE * 0.75;
    let icon = atlas.0.get(&kind).cloned();

    let mut entity = commands.spawn_empty();
    entity.insert((
        Transform::from_translation(board.world_pos(pos.x, pos.y)),
        pos,
        UnitKindComp(kind.clone()),
        Producer {
            mass_income: def.mass_income(),
            energy_income: def.energy_income() - def.maintenance_energy(),
        },
    ));

    if let Some(handle) = icon {
        entity.insert(Sprite {
            image: handle,
            custom_size: Some(Vec2::splat(size)),
            color: Color::srgb(1.0, 1.0, 1.0),
            ..default()
        });
    } else {
        entity.insert(Sprite {
            image: fallback.0.clone(),
            color: unit_color(&kind),
            custom_size: Some(Vec2::splat(size)),
            ..default()
        });
    }

    if def.build_rate() > 0.0 {
        entity.insert(Builder {
            power: def.build_rate(),
        });
    }

    // ACU is not built by a player order, so do not add it to completion tracking here.
    let _ = is_acu;

    entity.id()
}

/// Load portrait icons for all known unit kinds.
fn preload_portraits(units: &Units, asset_server: &AssetServer, atlas: &mut IconAtlas) {
    // Diagnostic: verify the asset root and a sample file.
    let cwd = std::env::current_dir().unwrap_or_default();
    let sample_path = format!("assets/icons/units/{}.png", "UEL0001");
    eprintln!(
        "[faf-sim] preload_portraits cwd={:?}, sample_exists={}",
        cwd,
        std::path::Path::new(&sample_path).exists()
    );

    for kind in units.defs().keys() {
        if let Some(path) = portrait_path(units, kind) {
            let handle: Handle<Image> = asset_server.load(path);
            atlas.0.insert(kind.clone(), handle);
        }
    }
}

/// Portrait icon path for a unit kind.
fn portrait_path(units: &Units, kind: &UnitKind) -> Option<String> {
    units
        .blueprint_id(kind)
        .map(|id| format!("icons/units/{}.png", id))
}

fn input_system(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform)>,
    board: Res<BoardConfig>,
    library: Res<UnitLibrary>,
    mut selected: ResMut<SelectedUnit>,
    mut build_target: ResMut<SelectedBuildTarget>,
    builders: Query<(Entity, &Builder, &GridPos, &UnitKindComp), With<UnitKindComp>>,
    units_on_tile: Query<
        (Entity, &GridPos, &UnitKindComp),
        (
            With<UnitKindComp>,
            Without<BoardTile>,
            Without<ConstructionSite>,
        ),
    >,
    occupied: Query<&GridPos, Or<(With<UnitKindComp>, With<ConstructionSite>)>>,
    mut contexts: EguiContexts,
) {
    // Cancel build target on right click or Escape.
    if mouse.just_pressed(MouseButton::Right) || keyboard.just_pressed(KeyCode::Escape) {
        build_target.0 = None;
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    // Do not steal clicks that egui wants.
    if contexts
        .ctx_mut()
        .map_or(false, |ctx| ctx.egui_wants_pointer_input())
    {
        return;
    }

    let Ok(window) = windows.single() else { return };
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let Ok((camera, camera_transform)) = camera.single() else {
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };

    let Some((x, y)) = board.grid_pos(world) else {
        return;
    };
    let pos = GridPos { x, y };

    // If we have an active build target, try to place it.
    if let Some(target) = build_target.0.clone() {
        if let Some(builder_entity) = selected.0 {
            if place_build_order(
                &mut commands,
                &board,
                &library.units,
                builder_entity,
                pos,
                target,
                &builders,
                &occupied,
            ) {
                build_target.0 = None;
            }
        }
        return;
    }

    // Otherwise, select the unit under the cursor (if any).
    selected.0 = units_on_tile
        .iter()
        .find(|(_, p, _)| **p == pos)
        .map(|(e, _, _)| e);
}

/// Try to place a construction site for `target` using `builder_entity`.
/// Returns true if the order was accepted.
fn place_build_order(
    commands: &mut Commands,
    board: &BoardConfig,
    units: &Units,
    builder_entity: Entity,
    pos: GridPos,
    target: UnitKind,
    builders: &Query<(Entity, &Builder, &GridPos, &UnitKindComp), With<UnitKindComp>>,
    occupied: &Query<&GridPos, Or<(With<UnitKindComp>, With<ConstructionSite>)>>,
) -> bool {
    let Ok((_, builder, builder_pos, builder_kind)) = builders.get(builder_entity) else {
        return false;
    };

    if !units.can_build(&builder_kind.0, &target) {
        return false;
    }

    let Some(cost) = units.build_cost(&target) else {
        return false;
    };
    let stats = cost.to_target_stats();

    // Factories build mobile units on their own tile; other builders place structures on empty tiles.
    let is_factory = matches!(builder_kind.0, UnitKind::Factory(_));
    let target_is_mobile = is_mobile(&target);

    let (spawn_pos, attach_to_factory) = if is_factory && target_is_mobile {
        (*builder_pos, true)
    } else {
        if occupied.iter().any(|p| *p == pos) {
            return false;
        }
        (pos, false)
    };

    let transform = Transform::from_translation(board.world_pos(spawn_pos.x, spawn_pos.y));

    commands.spawn((
        Sprite {
            color: Color::srgb(0.9, 0.7, 0.2),
            custom_size: Some(Vec2::splat(board.tile_size * 0.7)),
            ..default()
        },
        transform,
        spawn_pos,
        UnitKindComp(target.clone()),
        ConstructionSite {
            target: target.clone(),
            remaining_work: stats.build_time,
            total_work: stats.build_time,
            power: builder.power,
        },
        AssignedBuilder(builder_entity),
    ));

    // Mark the builder as busy by removing its Builder component.
    commands.entity(builder_entity).remove::<Builder>();

    // If the order is attached to a factory, visually keep the factory selected so the player
    // can queue more units without re-selecting.
    let _ = attach_to_factory;

    true
}

/// True if the unit kind represents a mobile unit rather than a building.
fn is_mobile(kind: &UnitKind) -> bool {
    matches!(
        kind,
        UnitKind::Commander | UnitKind::Engineer(_) | UnitKind::Unique(_)
    )
}

fn hover_system(
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform)>,
    board: Res<BoardConfig>,
    mut hovered: ResMut<HoveredUnit>,
    units_on_tile: Query<
        (Entity, &GridPos),
        (
            With<UnitKindComp>,
            Without<BoardTile>,
            Without<ConstructionSite>,
        ),
    >,
    mut contexts: EguiContexts,
) {
    if contexts
        .ctx_mut()
        .map_or(false, |ctx| ctx.egui_wants_pointer_input())
    {
        hovered.0 = None;
        return;
    }

    let Ok(window) = windows.single() else {
        hovered.0 = None;
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        hovered.0 = None;
        return;
    };
    let Ok((camera, camera_transform)) = camera.single() else {
        hovered.0 = None;
        return;
    };
    let Ok(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        hovered.0 = None;
        return;
    };

    hovered.0 = board.grid_pos(world).and_then(|(x, y)| {
        let pos = GridPos { x, y };
        units_on_tile
            .iter()
            .find(|(_, p)| **p == pos)
            .map(|(e, _)| e)
    });
}

fn eco_system(
    sites: Query<&ConstructionSite>,
    mut eco: ResMut<EcoState>,
    mut stall: ResMut<StallFactor>,
    time: Res<Time>,
    mut sim_time: ResMut<SimTime>,
    library: Res<UnitLibrary>,
) {
    let dt = time.delta_secs().min(0.1) as f64;
    sim_time.0 += dt;

    let mut total_mass_drain = 0.0;
    let mut total_energy_drain = 0.0;

    for site in sites.iter() {
        if site.power <= 0.0 {
            continue;
        }
        let Some(BuildDrain {
            mass_per_second,
            energy_per_second,
            ..
        }) = compute_drain_for(site.target.clone(), site.power, &library.units)
        else {
            continue;
        };
        total_mass_drain += mass_per_second;
        total_energy_drain += energy_per_second;
    }

    let result = apply_tick_graph(total_mass_drain, total_energy_drain, &eco.0, dt);
    eco.0.mass_storage = result.new_mass_storage;
    eco.0.energy_storage = result.new_energy_storage;
    eco.0.net_mass_income = result.scaled_net_mass_income;
    stall.0 = result.effective_factor;
}

fn compute_drain_for(
    target: UnitKind,
    power: f64,
    units: &Units,
) -> Option<crate::economy::BuildDrain> {
    let cost = units.build_cost(&target)?.to_target_stats();
    compute_drain(&cost, RequestedBuildPower(power))
}

fn progress_system(
    mut sites: Query<(Entity, &mut ConstructionSite)>,
    stall: Res<StallFactor>,
    time: Res<Time>,
    mut pending: ResMut<PendingCompletions>,
    occupied: Query<&GridPos, With<ConstructionSite>>,
) {
    let dt = time.delta_secs().min(0.1) as f64;

    for (entity, mut site) in sites.iter_mut() {
        if site.power <= 0.0 {
            continue;
        }
        let progress = stall.0 * site.power * dt;
        if progress > 0.0 && site.remaining_work <= progress {
            let grid_pos = occupied
                .get(entity)
                .copied()
                .unwrap_or(GridPos { x: 0, y: 0 });
            pending.0.push(ConstructionCompleted {
                entity,
                grid_pos,
                target: site.target.clone(),
            });
        } else {
            site.remaining_work -= progress;
        }
    }
}

fn completion_system(
    mut commands: Commands,
    mut pending: ResMut<PendingCompletions>,
    sites: Query<(&ConstructionSite, &AssignedBuilder, &Transform)>,
    mut eco: ResMut<EcoState>,
    library: Res<UnitLibrary>,
    atlas: Res<IconAtlas>,
    fallback: Res<FallbackImage>,
    mut completed: ResMut<CompletedUnitKinds>,
    mut counts: ResMut<UnitCounts>,
) {
    for event in pending.0.drain(..) {
        let Ok((site, &AssignedBuilder(builder_entity), transform)) = sites.get(event.entity)
        else {
            continue;
        };

        let Some(def) = library.units.def(&event.target) else {
            continue;
        };

        commands.entity(event.entity).despawn();

        let size = TILE_SIZE * 0.75;
        let icon = atlas.0.get(&event.target).cloned();
        let mut entity = commands.spawn_empty();
        entity.insert((
            *transform,
            event.grid_pos,
            UnitKindComp(event.target.clone()),
            Producer {
                mass_income: def.mass_income(),
                energy_income: def.energy_income() - def.maintenance_energy(),
            },
        ));

        if let Some(handle) = icon {
            entity.insert(Sprite {
                image: handle,
                custom_size: Some(Vec2::splat(size)),
                color: Color::srgb(1.0, 1.0, 1.0),
                ..default()
            });
        } else {
            entity.insert(Sprite {
                image: fallback.0.clone(),
                color: unit_color(&event.target),
                custom_size: Some(Vec2::splat(size)),
                ..default()
            });
        }

        if def.build_rate() > 0.0 {
            entity.insert(Builder {
                power: def.build_rate(),
            });
        }

        // Add its economy contribution.
        eco.0.net_mass_income =
            eco.0.net_mass_income + crate::quantities::MassRate::from_raw(def.mass_income());
        eco.0.net_energy_income = eco.0.net_energy_income
            + crate::quantities::EnergyRate::from_raw(
                def.energy_income() - def.maintenance_energy(),
            );
        eco.0.mass_storage_cap =
            eco.0.mass_storage_cap + crate::quantities::Mass::from_raw(def.mass_storage());
        eco.0.energy_storage_cap =
            eco.0.energy_storage_cap + crate::quantities::Energy::from_raw(def.energy_storage());

        // Free the builder.
        commands
            .entity(builder_entity)
            .insert(Builder { power: site.power });

        // Track completion.
        completed.0.insert(event.target.clone());
        *counts.0.entry(category_of(&event.target)).or_insert(0) += 1;
    }
}

fn render_system(mut sites: Query<(&ConstructionSite, &mut Sprite)>) {
    for (site, mut sprite) in sites.iter_mut() {
        let progress = 1.0 - (site.remaining_work / site.total_work) as f32;
        let value = 0.9 - 0.4 * progress;
        sprite.color = Color::srgb(value, value * 0.7, 0.2);
    }
}

fn selection_visual_system(
    mut commands: Commands,
    selected: Res<SelectedUnit>,
    hovered: Res<HoveredUnit>,
    board: Res<BoardConfig>,
    units: Query<&GridPos, With<UnitKindComp>>,
    outlines: Query<Entity, With<SelectionOutline>>,
) {
    if !selected.is_changed() && !hovered.is_changed() {
        return;
    }

    // Remove existing outlines.
    for entity in outlines.iter() {
        commands.entity(entity).despawn();
    }

    // Spawn a highlight for the hovered unit.
    if let Some(entity) = hovered.0 {
        if let Ok(pos) = units.get(entity) {
            commands.spawn((
                Sprite {
                    color: Color::srgba(1.0, 1.0, 1.0, 0.25),
                    custom_size: Some(Vec2::splat(board.tile_size)),
                    ..default()
                },
                Transform::from_translation(board.world_pos(pos.x, pos.y)),
                SelectionOutline,
            ));
        }
    }

    // Spawn a selection ring for the selected unit.
    if let Some(entity) = selected.0 {
        if let Ok(pos) = units.get(entity) {
            commands.spawn((
                Sprite {
                    color: Color::srgba(0.2, 0.8, 1.0, 0.6),
                    custom_size: Some(Vec2::splat(board.tile_size + 2.0)),
                    ..default()
                },
                Transform::from_translation(board.world_pos(pos.x, pos.y)),
                SelectionOutline,
            ));
        }
    }
}

fn ui_system(
    mut contexts: EguiContexts,
    library: Res<UnitLibrary>,
    eco: Res<EcoState>,
    sim_time: Res<SimTime>,
    stall: Res<StallFactor>,
    selected: Res<SelectedUnit>,
    mut browser: ResMut<BrowserSelection>,
    mut category_filter: ResMut<BrowserCategoryFilter>,
    _counts: Res<UnitCounts>,
    atlas: Res<IconAtlas>,
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    unit_query: Query<(&UnitKindComp, Option<&Builder>, Option<&Producer>)>,
) {
    // Pre-compute selected unit info and portrait texture id before borrowing the egui context.
    let selected_info = selected.0.and_then(|entity| {
        unit_query
            .get(entity)
            .ok()
            .map(|(kind, builder, producer)| {
                let portrait_id = atlas.0.get(&kind.0).and_then(|h| contexts.image_id(h));
                (kind.0.clone(), builder, producer, portrait_id)
            })
    });

    // The browser panel selects a unit kind directly.
    let browser_kind = browser.0.clone();

    // Group all known units by browser category for the selection panel.
    let mut browser_groups: HashMap<BrowserCategory, Vec<UnitKind>> = HashMap::new();
    for kind in library.units.defs().keys() {
        let category = browser_category_for_kind(kind, &library.units, &library.index);
        browser_groups.entry(category).or_default().push(kind.clone());
    }
    for group in browser_groups.values_mut() {
        group.sort_by(|a, b| library.units.display_name(a).cmp(&library.units.display_name(b)));
    }

    // Pre-compute egui texture ids for all browser portraits.
    let portrait_ids: HashMap<UnitKind, egui::TextureId> = library
        .units
        .defs()
        .keys()
        .filter_map(|kind| {
            atlas
                .0
                .get(kind)
                .and_then(|h| contexts.image_id(h))
                .map(|id| (kind.clone(), id))
        })
        .collect();
    let browser_portrait_id = browser_kind.as_ref().and_then(|k| portrait_ids.get(k).copied());

    // Diagnostic: log once how many portraits are registered in egui.
    static mut LOGGED_ONCE: bool = false;
    unsafe {
        if !LOGGED_ONCE {
            LOGGED_ONCE = true;
            eprintln!(
                "[faf-sim] ui_system: atlas={}, portrait_ids={}, loaded={}",
                atlas.0.len(),
                portrait_ids.len(),
                atlas.0.values().filter(|h| asset_server.is_loaded(*h)).count()
            );
        }
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let screen = ctx.viewport_rect();

    let total_portraits = atlas.0.len();
    let loaded_portraits = atlas
        .0
        .values()
        .filter(|h| asset_server.is_loaded(*h))
        .count();

    // Top section: full-width unit browser with category tabs and a portrait grid.
    egui::Window::new("Unit Browser")
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .fixed_pos([0.0, 0.0])
        .default_size([screen.width(), 260.0])
        .min_size([screen.width(), 120.0])
        .max_size([screen.width(), screen.height() * 0.7])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Unit Browser");
                ui.label("Click a unit to view its eco details.");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!(
                        "Portraits: {} registered, {} loaded, {} in atlas",
                        portrait_ids.len(),
                        loaded_portraits,
                        total_portraits
                    ));
                });
            });
            ui.separator();

            // Category tabs.
            ui.horizontal_wrapped(|ui| {
                let mut sorted_categories: Vec<_> = browser_groups.keys().copied().collect();
                sorted_categories.sort();
                for category in sorted_categories {
                    let is_selected = category_filter.0 == category;
                    let text = egui::RichText::new(category.label())
                        .color(if is_selected {
                            ui.visuals().selection.bg_fill
                        } else {
                            ui.visuals().text_color()
                        });
                    if ui.selectable_label(is_selected, text).clicked() {
                        category_filter.0 = category;
                    }
                }
            });
            ui.separator();

            // Portrait grid for the selected category.
            let selected_count = browser_groups
                .get(&category_filter.0)
                .map(|v| v.len())
                .unwrap_or(0);
            ui.label(format!(
                "{}: {} units",
                category_filter.0.label(),
                selected_count
            ));

            egui::Frame::new()
                .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(kinds) = browser_groups.get(&category_filter.0) {
                            ui.horizontal_wrapped(|ui| {
                                for kind in kinds {
                                    let is_selected = browser.0.as_ref() == Some(kind);
                                    if browser_portrait_button(
                                        ui,
                                        &library.units,
                                        &portrait_ids,
                                        kind,
                                        is_selected,
                                    ) {
                                        browser.0 = Some(kind.clone());
                                    }
                                }
                            });
                        } else {
                            ui.label("No units in this category.");
                        }
                    });
                });
        });

    // Bottom section: eco summary | placeholder | selected unit detail.
    egui::Window::new("Bottom Bar")
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::LEFT_BOTTOM, [0.0, 0.0])
        .default_size([screen.width(), 220.0])
        .min_size([screen.width(), 120.0])
        .max_size([screen.width(), screen.height() * 0.7])
        .show(ctx, |ui| {
            let section_width = ui.available_width() / 3.0;
            let section_height = ui.available_height();

            ui.horizontal(|ui| {
                // 1. Eco summary.
                ui.allocate_ui_with_layout(
                    egui::vec2(section_width, section_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.heading("Eco Summary");
                        ui.separator();
                        ui.label(format!("Mass: {:.1}", eco.0.mass_storage.value()));
                        ui.label(format!("Energy: {:.1}", eco.0.energy_storage.value()));
                        ui.label(format!(
                            "Mass income: +{:.1}",
                            eco.0.net_mass_income.value()
                        ));
                        ui.label(format!(
                            "Energy income: +{:.1}",
                            eco.0.net_energy_income.value()
                        ));
                        ui.label(format!("Time: {:.1}s", sim_time.0));
                        ui.label(format!("Stall: {:.2}", stall.0));
                    },
                );

                ui.separator();

                // 2. Placeholder (left empty).
                ui.allocate_ui_with_layout(
                    egui::vec2(section_width, section_height),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.heading("Placeholder");
                        ui.separator();
                    },
                );

                ui.separator();

                // 3. Selected unit eco detail.
                ui.allocate_ui_with_layout(
                    egui::vec2(section_width, section_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.heading("Selected Unit");
                        ui.separator();

                        let detail_kind = browser_kind
                            .as_ref()
                            .or(selected_info.as_ref().map(|(k, _, _, _)| k));
                        let detail_portrait_id = browser_portrait_id
                            .or_else(|| selected_info.as_ref().and_then(|(_, _, _, id)| *id));

                        if let Some(kind) = detail_kind {
                            let def = library.units.def(kind);
                            let cost = library.units.build_cost(kind);

                            ui.horizontal(|ui| {
                                if let Some(texture_id) = detail_portrait_id {
                                    ui.image(egui::load::SizedTexture::new(
                                        texture_id,
                                        egui::vec2(64.0, 64.0),
                                    ));
                                }
                                ui.vertical(|ui| {
                                    ui.heading(library.units.display_name(kind));
                                    if let Some(cost) = cost {
                                        ui.label(format!(
                                            "Mass: {:.0}   Energy: {:.0}   Time: {:.0}",
                                            cost.mass, cost.energy, cost.build_time
                                        ));
                                    }
                                });
                            });
                            ui.separator();
                            if let Some(def) = def {
                                if let Some((_, Some(builder), _, _)) = selected_info
                                    .as_ref()
                                    .filter(|(k, _, _, _)| k == kind)
                                {
                                    ui.label(format!("Build power: {:.1}", builder.power));
                                }
                                ui.label(format!("Mass income: +{:.1}", def.mass_income()));
                                ui.label(format!(
                                    "Energy income: +{:.1} / maintenance -{:.1}",
                                    def.energy_income(),
                                    def.maintenance_energy()
                                ));
                            }
                        } else {
                            ui.label("Select a unit from the browser to see its details.");
                        }
                    },
                );
            });
        });
}

/// Render a single portrait button in the unit browser.
fn browser_portrait_button(
    ui: &mut egui::Ui,
    units: &Units,
    portrait_ids: &HashMap<UnitKind, egui::TextureId>,
    kind: &UnitKind,
    is_selected: bool,
) -> bool {
    let portrait_id = portrait_ids.get(kind).copied();
    let size = egui::vec2(40.0, 40.0);

    let response = if let Some(texture_id) = portrait_id {
        let tint = if is_selected {
            egui::Color32::WHITE
        } else {
            egui::Color32::LIGHT_GRAY
        };
        let image = egui::Image::new(egui::load::SizedTexture::new(texture_id, size))
            .tint(tint)
            .sense(egui::Sense::click());
        ui.add_sized(size, image)
    } else {
        ui.add_sized(size, egui::Button::new("?"))
    };

    response.on_hover_text(units.display_name(kind)).clicked()
}

fn unit_color(kind: &UnitKind) -> Color {
    match kind {
        UnitKind::Commander => Color::srgb(0.2, 0.6, 1.0),
        UnitKind::Mex(_) => Color::srgb(0.2, 0.8, 0.3),
        UnitKind::Pgen(_) => Color::srgb(0.9, 0.8, 0.2),
        UnitKind::Engineer(_) => Color::srgb(0.7, 0.7, 0.7),
        UnitKind::Factory(_) => Color::srgb(0.5, 0.5, 0.5),
        UnitKind::EnergyStorage => Color::srgb(0.2, 0.7, 0.9),
        UnitKind::CapT2Mex | UnitKind::CapT3Mex => Color::srgb(0.1, 0.6, 0.2),
        UnitKind::Unique(_) => Color::srgb(0.8, 0.2, 0.8),
    }
}
