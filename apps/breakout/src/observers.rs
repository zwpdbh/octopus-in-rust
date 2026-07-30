use bevy::prelude::*;

use crate::{CollisionSound, components::BallCollided};

pub fn play_collision_sound(
    _collided: On<BallCollided>,
    mut commands: Commands,
    sound: Res<CollisionSound>,
) {
    commands.spawn((AudioPlayer(sound.clone()), PlaybackSettings::DESPAWN));
}
