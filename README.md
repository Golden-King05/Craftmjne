# Craftmjne

A well-optimized 3D Minecraft-style voxel game, built in **Rust + Bevy** as a
**framework** you can expand. All block textures are procedurally generated
**16×16 pixel art** — the project ships zero image assets.

> This is the native rewrite of the original JavaScript/Electron prototype
> (still available in git history). Same architecture, now with real threads,
> no GC pauses, and Bevy's ECS as the extension model.

## Quick start

```bash
cargo run --release
# options:
cargo run --release -- --seed 42 --render-distance 10
```

Dev builds are configured for fast iteration (`opt-level = 1` for the game,
`opt-level = 3` for dependencies), so plain `cargo run` is playable too.

### Controls

| Input | Action |
|---|---|
| Click | capture mouse |
| Esc | release mouse |
| W A S D | move |
| Space | jump / swim up / fly up |
| Shift | fly down |
| Ctrl | sprint |
| F | toggle fly mode |
| Left / right click | break / place block |
| Middle click | pick targeted block |
| 1–9 / mouse wheel | hotbar selection |
| F3 | debug overlay |

## Performance design

- **Task-pool pipeline** — terrain generation *and* meshing run on Bevy's
  async compute pool (all cores), keeping the main schedule free for
  rendering. Chunk data moves by ownership — no copies, no locks.
- **Padded-shell meshing** — each mesh job receives the chunk plus a 1-block
  shell from its 8 neighbours, so face culling and ambient occlusion never do
  cross-chunk lookups and chunk borders are seamless.
- **Hidden-face culling** — only faces exposed to air/transparent blocks emit
  geometry; same-type transparent neighbours (water–water, glass–glass) are
  culled too.
- **Baked lighting + custom shader** — directional sky shading and per-vertex
  ambient occlusion are baked into vertex colors by the mesher. Chunks render
  with one tiny unlit WGSL fragment shader (`src/chunk.wgsl`) that also does
  alpha cutout (leaves/glass), water translucency, and distance fog — no
  lights, no normals, no shadow passes, two pipeline states total.
- **Flat typed tables** — `Vec<u16>` chunk storage (Y-major, so column ops are
  contiguous slice copies) and flat `Vec<bool>` block-property tables in every
  hot loop; the mesher's AO neighbourhood offsets are precomputed integers.
- **Streaming with budgets** — chunks generate/mesh sorted by distance with
  capped in-flight tasks; far meshes are dropped while block data (and your
  edits) are kept. Bevy frustum-culls per chunk automatically.
- **Fixed-timestep physics** — 120 Hz substeps, framerate-independent, with
  swept axis-separated AABB collision against the voxel grid.

## Architecture

Everything is a Bevy plugin; the binary just assembles them.

```
src/
├── main.rs      # binary: CLI args + plugin assembly
├── lib.rs       # library surface (used by tests and downstream crates)
├── config.rs    # chunk size, world height, atlas layout, WorldSettings
├── noise.rs     # seeded simplex noise, fBm, integer hashes
├── blocks.rs    # BlockRegistry: defs -> compiled flat lookup Tables
├── atlas.rs     # Painters resource: 16x16 procedural tiles -> RGBA atlas
├── terrain.rs   # TerrainGenerator: heightmap, biomes, caves, ores, trees
├── mesher.rs    # culled + AO-baked chunk meshing (runs on task pool)
├── world.rs     # WorldPlugin: ChunkMap, streaming, gen/mesh tasks, edits
├── render.rs    # RenderSetupPlugin: ChunkMaterial, atlas image, fog
├── chunk.wgsl   # the chunk fragment shader (embedded asset)
├── player.rs    # PlayerPlugin: AABB physics, swimming, fly mode, camera
├── interact.rs  # InteractPlugin: voxel DDA raycast, break/place/pick, hotbar
└── ui.rs        # UiPlugin: crosshair, hotbar icons, hint, F3 debug panel
```

Data flow for a chunk: `stream_chunks` → generation task → blocks arrive →
neighbours ready → padded copy → mesh task → `Mesh3d` entity spawned. Edits
bump a chunk version, mark it (and border neighbours) dirty, and the same
pipeline remeshes them; results from a stale version are detected and
re-queued automatically.

## Extending the framework

Write a Bevy plugin and add it in `main.rs`. Content registration happens in
`Plugin::build` (before startup); game logic is ordinary Bevy systems.

### Add a block with a custom 16×16 texture

```rust
use bevy::prelude::*;
use craftmjne::atlas::Painters;
use craftmjne::blocks::{BlockDef, BlockRegistry, FaceTextures};

pub struct RubyPlugin;

impl Plugin for RubyPlugin {
    fn build(&self, app: &mut App) {
        let world = app.world_mut();
        world.resource_mut::<Painters>().register("ruby", |tile, rng| {
            for y in 0..16 {
                for x in 0..16 {
                    let j = (rng() - 0.5) * 60.0;
                    tile.px(x, y, [200.0 + j, 30.0 + j / 3.0, 60.0 + j / 3.0]);
                }
            }
        });
        world.resource_mut::<BlockRegistry>().register(BlockDef {
            name: "ruby_block".into(),
            textures: FaceTextures::all("ruby"),
            ..BlockDef::default()
        });
    }
}
```

`BlockDef` flags (defaults shown): `solid: true`, `transparent: false`
(doesn't occlude neighbours — glass/leaves), `translucent: false` (water
pass), `selectable: true`, `replaceable: false`, `breakable: true`.

### React to game events

```rust
fn on_block_set(mut events: EventReader<craftmjne::world::BlockSetEvent>) {
    for e in events.read() {
        info!("block {} -> {} at {}", e.prev, e.id, e.pos);
    }
}
// also: craftmjne::world::ChunkMeshedEvent
```

### Touch the world from any system

```rust
fn my_system(mut map: ResMut<craftmjne::world::ChunkMap>) {
    map.set_block(IVec3::new(0, 40, 0), 10);
    let id = map.get_block(IVec3::new(8, 30, 8));
}
```

### Customize world generation

`src/terrain.rs` is a plain struct constructed in `world::compile_content`;
swap in your own generator there. Generation is deterministic per
`(seed, chunk)` with no cross-chunk dependencies so chunks can generate in any
order on any thread — keep that property (trees use a border margin for
exactly this reason).

## Tests

```bash
cargo test
```

Unit tests cover the noise, atlas, registry, terrain, mesher (face counts, AO,
water bucket), physics (falling, landing, wall collision) and raycasting.
`tests/headless.rs` boots the real ECS app **without a window or GPU** and
drives the full streaming pipeline: generation tasks → meshing tasks → chunk
entities, plus block edits, remeshing and cross-chunk dirty propagation.

## Roadmap ideas

Natural next steps the architecture is prepared for: greedy meshing as an
alternative mesher, block light propagation (extra vertex-color channel),
chunk persistence (serialize `ChunkMap` regions), entities/mobs as plugins,
day/night cycle (shader uniforms already in place), biome-driven generation
parameters, and audio (enable Bevy's `bevy_audio` feature).

## License

MIT — all textures are generated at runtime; no third-party assets.
