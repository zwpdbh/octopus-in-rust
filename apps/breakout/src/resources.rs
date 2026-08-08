use bevy::prelude::*;

// This resource tracks the game's score
#[derive(Resource, Deref, DerefMut)]
pub struct Score(pub usize);

#[derive(Resource, Deref)]
pub struct CollisionSound(pub Handle<AudioSource>);
