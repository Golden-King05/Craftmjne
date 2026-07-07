//! Global engine constants and world settings.

use bevy::prelude::*;

pub const CHUNK_SIZE: i32 = 16; // chunk footprint in blocks (X and Z)
pub const WORLD_HEIGHT: i32 = 64; // world height in blocks (one chunk = full column)
pub const SEA_LEVEL: i32 = 26;

pub const TILE_SIZE: usize = 16; // pixels per texture tile (16x16 textures)
pub const ATLAS_TILES: usize = 16; // atlas is ATLAS_TILES x ATLAS_TILES tiles

pub const CS: usize = CHUNK_SIZE as usize;
pub const H: usize = WORLD_HEIGHT as usize;

/// Block storage layout: `y + WORLD_HEIGHT * (x + CHUNK_SIZE * z)`.
/// Y is the fastest axis so vertical column operations are contiguous.
#[inline]
pub fn block_index(x: usize, y: usize, z: usize) -> usize {
    y + H * (x + CS * z)
}

pub const SKY_COLOR: Color = Color::srgb(0.53, 0.73, 0.90);

#[derive(Resource, Clone)]
pub struct WorldSettings {
    pub seed: u32,
    pub render_distance: i32, // in chunks
}

impl Default for WorldSettings {
    fn default() -> Self {
        Self { seed: 1337, render_distance: 8 }
    }
}
