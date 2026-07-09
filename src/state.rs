//! App-level state machine (main menu <-> worlds list <-> settings <-> mods
//! <-> in-game) and the resource carrying which world is currently loaded.

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
