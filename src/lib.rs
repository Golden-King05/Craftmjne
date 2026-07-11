//! Library surface so integration tests (and downstream game crates) can use
//! the engine modules directly. The binary in `main.rs` wires these into an
//! app; see README for the framework guide.

pub mod atlas;
pub mod blocks;
pub mod chat;
pub mod config;
pub mod interact;
pub mod menu;
pub mod mesher;
pub mod noise;
pub mod player;
pub mod render;
pub mod save;
pub mod state;
pub mod terrain;
pub mod ui;
pub mod updater;
pub mod world;
