# Craftmjne

A well-optimized 3D Minecraft-style voxel game, built in **Rust + Bevy** as a
**framework** you can expand. All block textures are procedurally generated
**16×16 pixel art** — the project ships zero image assets.

> This is the native rewrite of the original JavaScript/Electron prototype
> (still available in git history). Same architecture, now with real threads,
> no GC pauses, and Bevy's ECS as the extension model.

## Install (Windows)

Download `CraftmjneSetup.exe` from the [latest release](../../releases/latest)
and run it — no admin rights needed. It installs to
`%LOCALAPPDATA%\Craftmjne`, adds Start Menu and Desktop shortcuts, and
registers a normal uninstaller in "Add or remove programs".

From then on the game **updates itself**: every time it starts it checks
GitHub Releases in the background and, if a newer version exists, downloads
it and swaps the installed `.exe` in place (see "Auto-update" below) — you
never need to re-run the installer. New builds are published automatically
by CI whenever a `v*` tag is pushed (`.github/workflows/release.yml`), for
Windows, Linux, and macOS (Intel + Apple Silicon).

## Build from source

```bash
cargo run --release
# options:
cargo run --release -- --seed 42 --render-distance 10 --no-update-check
```

Dev builds are configured for fast iteration (`opt-level = 1` for the game,
`opt-level = 3` for dependencies), so plain `cargo run` is playable too.

### Building the Windows installer yourself

Cross-compiles fine from Linux/macOS with `mingw-w64` + NSIS installed
(`apt install mingw-w64 nsis` / `brew install mingw-w64 makensis`), or
natively on Windows with the MSVC target — CI uses the latter.

```bash
rustup target add x86_64-pc-windows-gnu   # once
cargo build --release --target x86_64-pc-windows-gnu
makensis -DAPP_VERSION=0.2.0 \
         -DSRC_EXE="$(pwd)/target/x86_64-pc-windows-gnu/release/craftmjne.exe" \
         installer/craftmjne.nsi
# -> CraftmjneSetup.exe in the repo root
```

### Controls

The mouse locks automatically the moment you enter a world — no click needed.

| Input | Action |
|---|---|
| W A S D | move |
| Space | jump / swim up / fly up (Creative only) |
| Shift | fly down (Creative only) |
| Ctrl | sprint |
| F | toggle fly mode (Creative only) |
| Left / right click | break / place block |
| Middle click | pick targeted block (Creative only) |
| 1–9 / mouse wheel | hotbar selection |
| E | open/close inventory |
| T | open chat |
| F3 | debug overlay |
| Esc | pause (Resume / Quit to Menu / Quit Game); Esc again resumes; also closes the inventory/chat if one is open |

## Menus and saved worlds

The game boots into a main menu (**Worlds**, **Settings**, **Mods**, **Quit
Game**) rather than straight into a world. **Worlds** lists your saves and has
a **Create World** button (name, optional seed, and a **Survival / Creative**
game mode picker) at the bottom, Minecraft-style; clicking a saved world loads
it. **Settings** currently exposes render distance (persisted, applied on next
launch); **Mods** is a placeholder for future mod support.

Worlds save to the same per-user app-data directory the installer and
auto-updater use (`%LOCALAPPDATA%\Craftmjne\saves\<name>` on Windows,
`~/.local/share/craftmjne/saves/<name>` on Linux, `~/Library/Application
Support/Craftmjne/saves/<name>` on macOS) — naturally scoped to the OS user
account. Each world is a small `meta.json` (name, seed, game mode, timestamps)
plus a `data.json` recording the player's position and every block edit (by
block *name*, not numeric id, so saves survive block-registry changes).
Terrain itself is never saved — it's deterministic from the seed, so only the
diff from procedural generation needs to persist. Autosaves every 30s and on
returning to the menu.

### Game modes

Chosen once, at world creation (`save::GameMode`, in `meta.json`). For now the
only behavioral difference is flying: Creative can toggle it with `F`;
Survival can't, and any stale `fly: true` from an older save is cleared on
load. More differences (mining speed, hunger, PvP damage, etc.) are natural
follow-ups — see `player::player_update` for where mode is checked.

The mode can also be changed mid-game with `/mode <survival|creative>` (see
"Chat and commands" below).

## Inventory

Press `E` to open/close it (`src/inventory.rs`). The hotbar and the
inventory screen both start **completely empty** in every mode — nothing is
pre-filled — matching Minecraft's own behavior.

- **Survival** shows the hotbar plus a second row of personal storage
  (`inventory::Inventory`, `INVENTORY_SIZE` slots — a constant, deliberately
  easy to make configurable later). There's no block-pickup-on-break yet, so
  Survival currently has no way to fill either row; that's a natural next
  step. Middle-click "pick block" is Creative-only for the same reason —
  Survival has no free items.
- **Creative** shows one scrollable grid of every registered block instead
  of personal storage. Clicking a block puts it in the currently selected
  hotbar slot.
- Hovering any occupied slot (in either mode) shows the block's name in a
  cursor-following tooltip, Minecraft-style.

Opening the inventory frees the cursor and freezes player movement/hotbar
shortcuts/block interaction, the same way chat and the pause menu do — all
three overlays are mutually exclusive (only one can be open at a time) and
Escape closes whichever one is open before it would open the pause menu.

## Chat and commands

Press `T` to open a one-line chat box; `Enter` sends, `Escape` cancels. Sent
lines land in a local scrollback that fades out a few seconds after the box
closes — there's no multiplayer, so this exists mainly as a place to type
`/`-prefixed commands.

`src/commands.rs` is the dispatcher. The first (and so far only) command is:

- `/mode <survival|creative>` — also accepts `s`/`c` or `1`/`2`. Changes the
  running world's game mode immediately and persists it to `meta.json`.

Running *any* recognized command — even one that fails with a usage error —
permanently sets a `cheats: true` flag on the world's `meta.json`
(`save::WorldMeta::cheats`). This mirrors Minecraft's own "cheats" world flag:
it's never shown in the UI and never cleared, and exists so a future
achievements system can check it and skip a world that's had commands used in
it. An unrecognized command name (a typo, not a real command) does not set it.

Add a command by extending the match in `commands::execute`.

## Auto-update

`src/updater.rs` is the whole mechanism: on startup, a background thread asks
GitHub Releases for the latest tag and, if it's newer than the running
build, downloads the matching platform archive and rewrites the on-disk
`.exe` — using [`self_update`](https://docs.rs/self_update)'s rename-based
replace, which works even while that same binary is currently running. The
game doesn't relaunch itself mid-session (a HUD banner just tells you a new
version is ready); the swap takes effect next launch, the same model Steam
and VS Code use. Failures (offline, rate-limited, no releases yet) are
logged and silently ignored — an update check never blocks or interrupts play.

This is why the installer targets `%LOCALAPPDATA%` instead of
`Program Files`: an unprivileged process can overwrite its own exe there,
so updates need no UAC prompt and no separate updater service.

Turn it off with `--no-update-check` or `CRAFTMJNE_NO_UPDATE_CHECK=1` (it's
also auto-disabled under `CRAFT_SMOKE`, so CI screenshots never depend on
network access).

## Releasing a new version

1. Bump `version` in `Cargo.toml`.
2. `git tag v0.3.0 && git push origin v0.3.0`.
3. CI builds Windows/Linux/macOS binaries, packages the Windows installer,
   and publishes everything to a GitHub Release — installed copies of the
   game will pick it up automatically within one restart.

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
├── state.rs     # AppState (menu <-> in-game), ActiveWorld and PauseState resources
├── save.rs      # SaveStore: per-user world/settings persistence (serde)
├── menu.rs      # MenuPlugin: main menu, worlds list + create form, settings, mods
├── noise.rs     # seeded simplex noise, fBm, integer hashes
├── blocks.rs    # BlockRegistry: defs -> compiled flat lookup Tables
├── atlas.rs     # Painters resource: 16x16 procedural tiles -> RGBA atlas
├── terrain.rs   # TerrainGenerator: heightmap, biomes, caves, ores, trees
├── mesher.rs    # culled + AO-baked chunk meshing (runs on task pool)
├── world.rs     # WorldPlugin: ChunkMap, streaming, gen/mesh tasks, edits, save/load
├── render.rs    # RenderSetupPlugin: ChunkMaterial, atlas image, fog
├── chunk.wgsl   # the chunk fragment shader (embedded asset)
├── player.rs    # PlayerPlugin: AABB physics, swimming, fly mode, pause/cursor, camera
├── interact.rs  # InteractPlugin: voxel DDA raycast, break/place/pick, hotbar
├── inventory.rs # InventoryPlugin: hotbar+storage (Survival) or block list (Creative), tooltips
├── chat.rs      # ChatPlugin: chat box UI + input, routes "/" lines to commands::execute
├── commands.rs  # chat command dispatcher (/mode ...) + the cheats-flag rule
├── ui.rs        # UiPlugin: crosshair, hotbar icons, hint, F3 debug panel, update banner
└── updater.rs   # UpdaterPlugin: background GitHub-release check + self-swap
installer/
└── craftmjne.nsi        # NSIS script -> CraftmjneSetup.exe
.github/workflows/
└── release.yml           # tag push -> cross-platform build + GitHub Release
```

Data flow for a chunk: `stream_chunks` → generation task → blocks arrive →
pending saved edits applied → neighbours ready → padded copy → mesh task →
`Mesh3d` entity spawned. Edits bump a chunk version, mark it (and border
neighbours) dirty, and the same pipeline remeshes them; results from a stale
version are detected and re-queued automatically.

Entering a world (`world::enter_world`, on `OnEnter(AppState::InGame)`)
builds a fresh `TerrainGenerator` for that world's seed, resets `ChunkMap`
(despawning any previous world's chunk entities), loads its save data, and
restores the player's saved position — or leaves them unspawned so the usual
`try_spawn` logic places them, for a brand new world. Leaving
(`world::exit_world`, on `OnExit`) and the periodic autosave both write the
session's accumulated block edits and current player pose back out.

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
water bucket), physics (falling, landing, wall collision), raycasting,
save/load (slugging, sanitization, round-tripping worlds and settings against
a throwaway temp directory — never a real user profile), and the command
dispatcher (`/mode`'s alias forms, persistence, and the cheats-flag rule).
`tests/headless.rs` boots the real ECS app **without a window or GPU** and
drives the full streaming pipeline through the actual `AppState` machine:
menu → `InGame` → generation tasks → meshing tasks → chunk entities, plus
block edits, remeshing, cross-chunk dirty propagation, and a full
leave-and-relaunch cycle that confirms edits and player position survive a
save/load round trip.

## Roadmap ideas

Natural next steps the architecture is prepared for: greedy meshing as an
alternative mesher, block light propagation (extra vertex-color channel),
multiple save slots per world / world deletion and rename in the Worlds
screen, real mod loading (the Mods screen is a placeholder), entities/mobs as
plugins, day/night cycle (shader uniforms already in place), biome-driven
generation parameters, audio (enable Bevy's `bevy_audio` feature), more chat
commands (`commands::execute`'s match is the extension point), and an
achievements system that reads `WorldMeta::cheats` to exclude worlds that
have had commands used in them.

## License

MIT — all textures are generated at runtime; no third-party assets.
