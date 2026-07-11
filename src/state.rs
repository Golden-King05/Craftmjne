//! App-level state machine (main menu <-> worlds list <-> settings <-> mods
//! <-> in-game) and the resources carrying which world is currently loaded
//! and whether the in-game pause menu is up.

use bevy::prelude::*;

use crate::save::WorldMeta;

#[derive(States, Debug, Clone, Copy, Default, Eq, PartialEq, Hash)]
pub enum AppState {
    #[default]
    MainMenu,
    Worlds,
    Settings,
    Mods,
    InGame,
}

/// Which world is loaded into `InGame`. Set by the Worlds screen just before
/// requesting the transition; read by `world::enter_world` to pick the seed
/// and load that world's save data.
#[derive(Resource, Clone)]
pub struct ActiveWorld {
    pub slug: String,
    pub meta: WorldMeta,
}

/// Whether the pause menu is open. This is a resource rather than an
/// `AppState` variant on purpose: pausing must not despawn/reload the world
/// (unlike leaving `AppState::InGame` would), it just frees the cursor,
/// freezes player input, and shows an overlay on top of the still-rendering
/// game. Reset to closed on every `OnEnter(AppState::InGame)`.
#[derive(Resource, Default)]
pub struct PauseState {
    pub open: bool,
}
