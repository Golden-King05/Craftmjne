// Global engine constants. Chunk layout constants are compile-time (shared
// between the main thread and workers); gameplay values live in Engine config.

export const CHUNK_SIZE = 16;    // chunk footprint in blocks (X and Z)
export const WORLD_HEIGHT = 64;  // world height in blocks (one chunk = full column)
export const SEA_LEVEL = 26;

export const TILE_SIZE = 16;     // pixels per texture tile (16x16 textures)
export const ATLAS_TILES = 16;   // atlas is ATLAS_TILES x ATLAS_TILES tiles

// Block storage layout: index = y + WORLD_HEIGHT * (x + CHUNK_SIZE * z).
// Y is the fastest axis so vertical column operations are contiguous.
export function blockIndex(x, y, z) {
  return y + WORLD_HEIGHT * (x + CHUNK_SIZE * z);
}

export const DEFAULT_CONFIG = {
  seed: 1337,
  renderDistance: 8, // in chunks
};
