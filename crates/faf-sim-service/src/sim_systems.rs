use bevy_ecs::prelude::*;
use crossbeam_channel::{Receiver, Sender};
use faf_blueprints::ConstructionAction;
use faf_game_engine::*;
use faf_sim_protocol::SimEvent;
use uuid::Uuid;

/// Inbound channel for construction actions fed by `run_sim_thread`.
///
/// `run_sim_thread` pushes one action at a time and waits for a completion
/// report before pushing the next.  A Bevy system reads from this resource
/// and spawns the corresponding entities.
#[derive(Resource)]
pub struct ActionReceiver(pub Receiver<(Uuid, ConstructionAction)>);

/// Outbound channel used by Bevy systems to report simulation events back to
/// the service thread / external caller.
///
/// This is the Bevy-app → normal-application bridge.  Anything the service
/// or CLI needs to observe (eco snapshots, finished actions, etc.) is sent
/// through here as a `SimEvent`.
#[derive(Resource)]
pub struct EventSender(pub Sender<SimEvent>);

/// Internal channel used to signal that a construction task finished.
///
/// `run_sim_thread` reads from this channel so it can dispatch the next
/// queued action.  It is separate from `EventSender` because the external
/// `Receiver<SimEvent>` is held by the caller and cannot be cloned.
#[derive(Resource)]
pub struct FinishedSender(pub Sender<Uuid>);

/// Spawn builder and target entities for every construction action that has
/// been pushed into the simulation thread.
///
/// Because `run_sim_thread` sends actions one at a time, this system will
/// normally see at most one action per tick.
pub fn spawn_incoming_actions(mut commands: Commands, action_receiver: Res<ActionReceiver>) {
    while let Ok((task_id, action)) = action_receiver.0.try_recv() {
        spawn_action_entities(&mut commands, task_id, &action);
    }
}

fn spawn_action_entities(commands: &mut Commands, task_id: Uuid, action: &ConstructionAction) {
    for builder in action.builders() {
        spawn_builder(commands, task_id, builder);
    }

    spawn_target(commands, task_id, action.target());
}

fn spawn_builder(commands: &mut Commands, task_id: Uuid, builder: &faf_blueprints::UnitBlueprint) {
    commands.spawn((
        BuildPower(builder.unit_eco_effect().build_power),
        ConstructionBuilder { task: task_id },
    ));
}

fn spawn_target(commands: &mut Commands, task_id: Uuid, target: &faf_blueprints::UnitBlueprint) {
    commands.spawn((
        UnitCost(target.unit_cost()),
        ConstructionTarget::new(
            task_id,
            0.0,
            target.unit_eco_effect().clone(),
            target.tech_level(),
        ),
    ));
}

/// Forward internal `BuildingFinished` messages to both the external service
/// channel and the internal completion channel.
///
/// `BuildingFinished` is a Bevy `Message`, so multiple readers can observe the
/// same completion signal.  This system runs alongside the engine's own
/// `apply_finished_constructions` system.
pub fn report_finished_constructions(
    mut finished_reader: MessageReader<BuildingFinished>,
    event_sender: Res<EventSender>,
    finished_sender: Res<FinishedSender>,
) {
    for finished in finished_reader.read() {
        let _ = event_sender
            .0
            .send(SimEvent::ActionFinished(finished.task_id));
        let _ = finished_sender.0.send(finished.task_id);
    }
}

/// Emit the current economy snapshot every simulation tick.
///
/// This is what feeds the live chart and stats panel in the web frontend.
pub fn emit_eco_summary(player_eco: Res<PlayerEco>, event_sender: Res<EventSender>) {
    // best-effort reporting; the receiver may be gone during shutdown
    let _ = event_sender
        .0
        .send(SimEvent::EcoSummary(player_eco.0.clone()));
}
