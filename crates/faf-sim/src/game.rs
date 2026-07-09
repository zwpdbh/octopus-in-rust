//! Minimal Bevy-powered eco/build simulator.
//!
//! The user clicks an empty board tile to queue a T1 mex. The ACU builds it,
//! draining mass/energy from a shared economy pool. The simulator runs both
//! natively and on the web via WASM.

use bevy::prelude::*;

use crate::economy::{apply_tick_graph, compute_drain, EconomyState, RequestedBuildPower};
use crate::units::{TechLevel, UnitKind, Units};
use crate::BuildDrain;

const BOARD_SIZE: i32 = 16;
const TILE_SIZE: f32 = 32.0;

/// Wrapper so the unit database can live as a Bevy resource.
#[derive(Resource, Debug, Clone)]
struct UnitLibrary(Units);

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

/// Which unit the player is currently placing.
#[derive(Resource, Debug, Clone)]
struct SelectedTool(UnitKind);

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

/// Event fired when a construction site finishes.
#[derive(Event, Debug, Clone)]
struct ConstructionCompleted {
    entity: Entity,
    grid_pos: GridPos,
    target: UnitKind,
}

/// UI label so the text-update system knows which line to rewrite.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum UiLabel {
    Mass,
    Energy,
    MassIncome,
    EnergyIncome,
    Time,
    Stall,
}

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
        .insert_resource(UnitLibrary(load_units()))
        .insert_resource(EcoState(initial_economy()))
        .insert_resource(BoardConfig {
            tile_size: TILE_SIZE,
        })
        .insert_resource(SimTime(0.0))
        .insert_resource(StallFactor(1.0))
        .insert_resource(SelectedTool(UnitKind::Mex(TechLevel::T1)))
        .add_event::<ConstructionCompleted>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                input_system,
                eco_system.before(progress_system),
                progress_system.before(completion_system),
                completion_system,
                render_system.after(progress_system),
                ui_system.after(eco_system),
            ),
        )
        .run();
}

fn load_units() -> Units {
    let json = include_str!("../../../plugins/faf-units/data/faf_units.json");
    Units::new(serde_json::from_str(json).expect("embedded FAF unit index should parse"))
}

fn initial_economy() -> EconomyState {
    // Start with a small ACU-like economy.
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
    units: Res<UnitLibrary>,
    mut eco: ResMut<EcoState>,
) {
    // Camera centered on the board.
    commands.spawn(Camera2dBundle::default());

    // Spawn the empty board tiles.
    for x in 0..BOARD_SIZE {
        for y in 0..BOARD_SIZE {
            commands.spawn((
                SpriteBundle {
                    sprite: Sprite {
                        color: Color::srgb(0.15, 0.18, 0.15),
                        custom_size: Some(Vec2::new(board.tile_size - 1.0, board.tile_size - 1.0)),
                        ..default()
                    },
                    transform: Transform::from_translation(board.world_pos(x, y)),
                    ..default()
                },
                GridPos { x, y },
            ));
        }
    }

    // Spawn the ACU in the center as the starting builder + producer.
    let acu_pos = GridPos {
        x: BOARD_SIZE / 2,
        y: BOARD_SIZE / 2,
    };
    let acu_def = units.0.def(&UnitKind::Commander).expect("ACU defined");
    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.2, 0.6, 1.0),
                custom_size: Some(Vec2::new(board.tile_size * 0.8, board.tile_size * 0.8)),
                ..default()
            },
            transform: Transform::from_translation(board.world_pos(acu_pos.x, acu_pos.y))
                .with_scale(Vec3::new(1.0, 1.0, 1.0)),
            ..default()
        },
        acu_pos,
        UnitKindComp(UnitKind::Commander),
        Builder {
            power: acu_def.build_rate(),
        },
        Producer {
            mass_income: acu_def.mass_income(),
            energy_income: acu_def.energy_income() - acu_def.maintenance_energy(),
        },
    ));

    // Apply the ACU's production to the starting economy.
    eco.0.net_mass_income = eco.0.net_mass_income + crate::quantities::MassRate::from_raw(acu_def.mass_income());
    eco.0.net_energy_income = eco.0.net_energy_income
        + crate::quantities::EnergyRate::from_raw(acu_def.energy_income() - acu_def.maintenance_energy());

    // Spawn UI text.
    let text_style = TextStyle {
        font_size: 18.0,
        color: Color::srgb(1.0, 1.0, 1.0),
        ..default()
    };

    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("Mass: ", text_style.clone()),
            TextSection::new("0", text_style.clone()),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            left: Val::Px(10.0),
            ..default()
        }),
        UiLabel::Mass,
    ));

    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("Energy: ", text_style.clone()),
            TextSection::new("0", text_style.clone()),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(30.0),
            left: Val::Px(10.0),
            ..default()
        }),
        UiLabel::Energy,
    ));

    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("Mass income: ", text_style.clone()),
            TextSection::new("0", text_style.clone()),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(50.0),
            left: Val::Px(10.0),
            ..default()
        }),
        UiLabel::MassIncome,
    ));

    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("Energy income: ", text_style.clone()),
            TextSection::new("0", text_style.clone()),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(70.0),
            left: Val::Px(10.0),
            ..default()
        }),
        UiLabel::EnergyIncome,
    ));

    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("Time: ", text_style.clone()),
            TextSection::new("0", text_style.clone()),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(90.0),
            left: Val::Px(10.0),
            ..default()
        }),
        UiLabel::Time,
    ));

    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("Stall: ", text_style.clone()),
            TextSection::new("1.00", text_style.clone()),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            top: Val::Px(110.0),
            left: Val::Px(10.0),
            ..default()
        }),
        UiLabel::Stall,
    ));
}

fn input_system(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform)>,
    board: Res<BoardConfig>,
    units: Res<UnitLibrary>,
    tool: Res<SelectedTool>,
    occupied: Query<&GridPos, Or<(With<UnitKindComp>, With<ConstructionSite>)>>,
    builders: Query<(Entity, &Builder, &GridPos), With<UnitKindComp>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let window = windows.single();
    let Some(cursor) = window.cursor_position() else {
        return;
    };

    let (camera, camera_transform) = camera.single();
    let Some(world) = camera.viewport_to_world_2d(camera_transform, cursor) else {
        return;
    };

    let Some((x, y)) = board.grid_pos(world) else {
        return;
    };

    let pos = GridPos { x, y };
    if occupied.iter().any(|p| *p == pos) {
        return;
    }

    // For now, assign the closest idle builder (the ACU at game start).
    let Some((builder_entity, builder, _)) = builders.iter().next() else {
        return;
    };

    let Some(cost) = units.0.build_cost(&tool.0) else {
        return;
    };
    let stats = cost.to_target_stats();

    commands.spawn((
        SpriteBundle {
            sprite: Sprite {
                color: Color::srgb(0.9, 0.7, 0.2),
                custom_size: Some(Vec2::new(board.tile_size * 0.7, board.tile_size * 0.7)),
                ..default()
            },
            transform: Transform::from_translation(board.world_pos(pos.x, pos.y)),
            ..default()
        },
        pos,
        UnitKindComp(tool.0.clone()),
        ConstructionSite {
            target: tool.0.clone(),
            remaining_work: stats.build_time,
            total_work: stats.build_time,
            power: builder.power,
        },
        AssignedBuilder(builder_entity),
    ));

    // Mark the builder as busy by removing its Builder component.
    commands.entity(builder_entity).remove::<Builder>();
}

fn eco_system(
    sites: Query<&ConstructionSite>,
    mut eco: ResMut<EcoState>,
    mut stall: ResMut<StallFactor>,
    time: Res<Time>,
    mut sim_time: ResMut<SimTime>,
    units: Res<UnitLibrary>,
) {
    let dt = time.delta_seconds().min(0.1) as f64;
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
        }) = compute_drain_for(site.target.clone(), site.power, &units.0)
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

fn compute_drain_for(target: UnitKind, power: f64, units: &Units) -> Option<crate::economy::BuildDrain> {
    let cost = units.build_cost(&target)?.to_target_stats();
    compute_drain(&cost, RequestedBuildPower(power))
}

fn progress_system(
    mut sites: Query<(Entity, &mut ConstructionSite)>,
    stall: Res<StallFactor>,
    time: Res<Time>,
    mut events: EventWriter<ConstructionCompleted>,
    occupied: Query<&GridPos, With<ConstructionSite>>,
) {
    let dt = time.delta_seconds().min(0.1) as f64;

    for (entity, mut site) in sites.iter_mut() {
        if site.power <= 0.0 {
            continue;
        }
        let progress = stall.0 * site.power * dt;
        if progress > 0.0 && site.remaining_work <= progress {
            let _fraction = site.remaining_work / progress;
            // We don't have per-entity sim time here; completion_system will
            // stamp the current global time minus the remaining fraction.
            let grid_pos = occupied
                .get(entity)
                .copied()
                .unwrap_or(GridPos { x: 0, y: 0 });
            events.send(ConstructionCompleted {
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
    mut events: EventReader<ConstructionCompleted>,
    sites: Query<(&ConstructionSite, &AssignedBuilder, &Transform)>,
    mut eco: ResMut<EcoState>,
    units: Res<UnitLibrary>,
) {
    for event in events.read() {
        let Ok((site, &AssignedBuilder(builder_entity), transform)) = sites.get(event.entity) else {
            continue;
        };

        // Spawn the completed unit.
        let Some(def) = units.0.def(&event.target.clone()) else {
            continue;
        };

        commands.entity(event.entity).despawn();
        commands.spawn((
            SpriteBundle {
                sprite: Sprite {
                    color: unit_color(&event.target.clone()),
                    custom_size: Some(Vec2::new(TILE_SIZE * 0.75, TILE_SIZE * 0.75)),
                    ..default()
                },
                transform: *transform,
                ..default()
            },
            event.grid_pos,
            UnitKindComp(event.target.clone()),
            Producer {
                mass_income: def.mass_income(),
                energy_income: def.energy_income() - def.maintenance_energy(),
            },
        ));

        // Add its economy contribution.
        eco.0.net_mass_income = eco.0.net_mass_income + crate::quantities::MassRate::from_raw(def.mass_income());
        eco.0.net_energy_income = eco.0.net_energy_income
            + crate::quantities::EnergyRate::from_raw(def.energy_income() - def.maintenance_energy());
        eco.0.mass_storage_cap = eco.0.mass_storage_cap + crate::quantities::Mass::from_raw(def.mass_storage());
        eco.0.energy_storage_cap = eco.0.energy_storage_cap + crate::quantities::Energy::from_raw(def.energy_storage());

        // Free the builder.
        commands
            .entity(builder_entity)
            .insert(Builder { power: site.power });
    }
}

fn render_system(mut sites: Query<(&ConstructionSite, &mut Sprite)>) {
    for (site, mut sprite) in sites.iter_mut() {
        let progress = 1.0 - (site.remaining_work / site.total_work) as f32;
        // Darken as it gets closer to completion.
        let value = 0.9 - 0.4 * progress;
        sprite.color = Color::srgb(value, value * 0.7, 0.2);
    }
}

fn ui_system(
    eco: Res<EcoState>,
    sim_time: Res<SimTime>,
    stall: Res<StallFactor>,
    mut texts: Query<(&mut Text, &UiLabel)>,
) {
    for (mut text, label) in texts.iter_mut() {
        let section = &mut text.sections[1].value;
        match label {
            UiLabel::Mass => *section = format!("{:.1}", eco.0.mass_storage.value()),
            UiLabel::Energy => *section = format!("{:.1}", eco.0.energy_storage.value()),
            UiLabel::MassIncome => *section = format!("{:.1}", eco.0.net_mass_income.value()),
            UiLabel::EnergyIncome => *section = format!("{:.1}", eco.0.net_energy_income.value()),
            UiLabel::Time => *section = format!("{:.1}s", sim_time.0),
            UiLabel::Stall => *section = format!("{:.2}", stall.0),
        }
    }
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
