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

The game reads its block definitions from the `blocks/` directory at
startup and won't run without it (see "Add a block" below) — `cargo run`
finds the one committed at the repo root automatically since Cargo runs
with the package root as the working directory; nothing extra to set up.

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

- **Survival** shows the hotbar plus three rows of personal storage
  (`inventory::Inventory`, `STORAGE_ROWS` x `STORAGE_ROW_WIDTH` slots —
  constants, deliberately easy to make configurable later). There's no
  block-pickup-on-break yet, so Survival currently has no way to fill either
  the hotbar or storage; that's a natural next step. Middle-click "pick
  block" is Creative-only for the same reason — Survival has no free items.
- **Creative** shows the same hotbar+storage view, or a scrollable grid of
  every registered block — a button above the grid switches between the two
  (`inventory::CreativeTab`). Clicking a block in the grid gives a full
  stack of it in the currently selected hotbar slot.
- **`Inventory` is one resource shared by both modes** — switching modes
  (including mid-session via `/mode`) never resets or duplicates it, so
  whatever's in storage/the hotbar stays exactly where you left it.
- Hovering any occupied slot (in either mode) shows the block's name in a
  cursor-following tooltip, Minecraft-style. Scrolling or pressing a number
  key to change the selected hotbar slot also briefly shows that block's
  name above the hotbar itself (`ui::hotbar_label`), fading out after
  `HOTBAR_LABEL_DURATION` seconds — no inventory screen needs to be open.

Opening the inventory frees the cursor and freezes player movement/hotbar
shortcuts/block interaction, the same way chat and the pause menu do — all
three overlays are mutually exclusive (only one can be open at a time) and
Escape closes whichever one is open before it would open the pause menu.

### Stacking

Every slot holds an `ItemStack { id, count }` (`blocks::ItemStack`), not just
a bare block id. `max_stack` (defaults to `DEFAULT_MAX_STACK` = 124, see "Add
a block" below) caps how many of a given block one slot holds. Placing a
block consumes one from the stack **in Survival**, clearing the slot once it
hits zero; Creative's stacks never deplete on placement — its "free items"
already come from being able to refill a slot from the block grid at will,
same asymmetry as middle-click pick-block. A slot's count shows as a small
badge in its bottom-right corner once it's above 1.

### Item models

How a block's icon is drawn is a separate concern from how it renders
in-world (which is always the real block mesh) — controlled by
`item_model` (`blocks::ItemModel`), shared terminology with any future
standalone item system:

- **`"default"`** — a baked isometric icon (top + two side faces, Minecraft-
  style), generated once at startup by `icons.rs` from the block's own top
  and side textures. Pure CPU pixel math (inverse-mapped so the three faces
  tile without gaps), no extra cameras or shaders — same "everything is
  generated at runtime" approach `atlas.rs` uses for the textures
  themselves. This is the default for every block.
- **`"face"`** — the flat single-face 2D icon every block used before
  `ItemModel` existed. Better for anything thin/flat (a future flower or
  sign, say) than a forced-3D icon would read.
- **`"custom"`** — points at `custom_item_model`, a path to an external
  model (required when this is chosen; the loader panics on a block file
  missing it). No model format/loader exists yet, so this renders as
  `"face"` for now — the path still round-trips through the registry so a
  model system can pick it up later without another schema change.

`ui::block_icon` is the one place every icon-drawing call site (the hotbar,
the inventory screen, Creative's block grid) goes through, so they all stay
consistent as `ItemModel` grows more variants.

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

**Known gap:** `self_update` only ever extracts and swaps the named binary
out of the downloaded archive — it never touches other files. That means an
in-place auto-update does **not** refresh an existing install's `blocks/`
folder, only the `.exe`. A release that changes block definitions needs a
fresh install (or manually replacing `blocks/`) until something closes that
gap.

## Releasing a new version

Bump `version` in `Cargo.toml` (and the matching `craftmjne` entry near the
top of `Cargo.lock`) and push to `main` — that's it. `.github/workflows/
auto-tag.yml` notices the version changed, creates and pushes the matching
`vX.Y.Z` tag, and dispatches `release.yml` against it, which builds
Windows/Linux/macOS binaries, packages the Windows installer, and publishes
everything to a GitHub Release. Installed copies of the game pick it up
automatically within one restart (see "Auto-updating" above).

If you ever need to cut a release by hand instead (e.g. `auto-tag.yml`
itself is broken), the manual path still works: `git tag vX.Y.Z && git push
origin vX.Y.Z`. Note that some environments — including Claude sessions
working in this repo — get an HTTP 403 pushing tags even though branch
pushes work fine; that's exactly the gap `auto-tag.yml` exists to close,
since the Actions bot's own `GITHUB_TOKEN` *can* push tags within this repo.

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
├── blocks.rs    # BlockRegistry: loads blocks/*.json -> compiled flat lookup Tables
├── atlas.rs     # Painters resource: 16x16 procedural tiles -> RGBA atlas
├── icons.rs     # bakes isometric ItemModel::Default inventory icons from the atlas
├── terrain.rs   # TerrainGenerator: heightmap, biomes, caves, ores, trees
├── mesher.rs    # culled + AO-baked chunk meshing (runs on task pool)
├── world.rs     # WorldPlugin: ChunkMap, streaming, gen/mesh tasks, edits, save/load
├── render.rs    # RenderSetupPlugin: ChunkMaterial, atlas + icon atlas images, fog
├── chunk.wgsl   # the chunk fragment shader (embedded asset)
├── player.rs    # PlayerPlugin: AABB physics, swimming, fly mode, pause/cursor, camera
├── interact.rs  # InteractPlugin: voxel DDA raycast, break/place/pick, hotbar
├── inventory.rs # InventoryPlugin: hotbar+storage (Survival) or block list (Creative), tooltips
├── chat.rs      # ChatPlugin: chat box UI + input, routes "/" lines to commands::execute
├── commands.rs  # chat command dispatcher (/mode ...) + the cheats-flag rule
├── ui.rs        # UiPlugin: crosshair, hotbar icons, hint, F3 debug panel, update banner
└── updater.rs   # UpdaterPlugin: background GitHub-release check + self-swap
blocks/
└── *.json                # one block definition per file - see "Add a block" below
installer/
└── craftmjne.nsi        # NSIS script -> CraftmjneSetup.exe (bundles blocks/)
.github/workflows/
├── auto-tag.yml          # Cargo.toml version bump on main -> tags + dispatches release.yml
└── release.yml           # tag push (or auto-tag.yml's dispatch) -> cross-platform build + release
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

## Fluid flow

Any block with `fluid: true` (water today) spreads and dries up
automatically — no fluid-specific code required. `world.rs` keeps a per-cell
`fluid_level` byte alongside the block grid:

- **`0` (source)** — permanent, full height, never recomputed. Ocean water
  generated at sea level and any fluid block a player places both start here.
- **`1..=flow_distance`** — a flowing cell that many blocks from its supply,
  capped by that fluid's `flow_distance`. Height steps down linearly from
  `flow_distance` levels, so a long `flow_distance` slopes gently and a short
  one drops off steeply — a single formula (`mesher::fluid_height`), no
  per-fluid tuning.
- **`FLUID_FALLING`** — a full-height column fed from directly above (a
  waterfall); renders like a source but dries up if its supply is cut.

**Infinite water**: a flowing/falling cell with 2 or more of its 4 side
neighbours already permanent sources of the same fluid is promoted to a
source itself — the classic "two water source blocks either side of a gap"
trick, so a bounded pool built from placed sources doesn't dry up like a
transient flow would.

A budgeted queue (`FluidQueue`/`recompute_cell`) reacts to `BlockSetEvent`
and relaxes affected cells a few hundred at a time per tick, so a large
spread is visibly gradual rather than resolving in one frame. The mesher
draws a partial "step" wall between same-fluid neighbours at different
levels instead of culling that face outright, so adjacent flow heights never
show a gap. See the "Known gaps" note in Roadmap ideas for what's simplified.

## Extending the framework

Write a Bevy plugin and add it in `main.rs`. Content registration happens in
`Plugin::build` (before startup); game logic is ordinary Bevy systems.

### Add a block

Drop a new `*.json` file in `blocks/` — no recompile needed. This is how all
16 built-in blocks are defined; `src/blocks.rs` loads every file in that
directory at startup (next to the exe once installed, the repo root for
`cargo run`/tests — see `find_blocks_dir`).

```json
{
  "id": "ruby_block",
  "name": "Ruby Block",
  "textures": { "all": "ruby" }
}
```

Only `id` (a no-spaces registry key — also what saves reference block edits
by) is required. Everything else defaults sanely:

| Field | Default | Notes |
|---|---|---|
| `name` | title-cased `id` | display name, shown in inventory tooltips |
| `transparent` | `"no"` | `"no"` \| `"partial"` \| `"full"` — see below |
| `fluid` | `false` | swimmable liquid with a lowered top surface (water) that spreads/dries via the fluid sim |
| `flow_distance` | `0` | how far (in blocks) this fluid spreads from a source before drying up; also controls the slope — long distances step down gently, short ones drop off steeply |
| `solid` | `true` (`false` if `fluid`) | collides with entities |
| `selectable` | `true` (`false` if `fluid`) | can be targeted by the crosshair |
| `replaceable` | `false` (`true` if `fluid`) | placing into this cell overwrites it |
| `breakable` | `true` | bedrock sets this `false` |
| `max_stack` | `124` (`DEFAULT_MAX_STACK`) | how many fit in one inventory/hotbar slot — see "Stacking" above |
| `item` | `true` | `false` means this block has no inventory item at all — left out of Creative's grid, can't be middle-click picked. `air` is the built-in example; use it for a block that's only ever meant to be obtained via a separate item later (a bucket-of-water instead of a raw water block, say) |
| `item_model` | `"default"` | `"default"` \| `"face"` \| `"custom"` — see "Item models" below |
| `custom_item_model` | *(none)* | path to an external model, required when `item_model` is `"custom"` |
| `textures` | tile named after `id` on every face | `{ "all": "..." }` or `{ "top": "...", "bottom": "...", "side": "..." }` |

`transparent`'s three options all still respect the block's own texture
alpha (a `partial` block with a fully-opaque texture just looks solid):

- `"no"` — fully opaque, occludes neighbours, no blending.
- `"partial"` — like glass/leaves today: doesn't occlude neighbours, and
  texture pixels below the alpha-cutoff threshold are punched out entirely
  ("see the back geometry of the block from the front").
- `"full"` — like water: the whole face renders at reduced opacity, so you
  only ever see what you're actually looking at (nothing punched out), but
  everything behind it shows through.

New texture painters (referenced by a block's `textures`) are still
registered from Rust — see `atlas::default_painters` for examples, or
register your own via `Painters` in a plugin's `build()`:

```rust
use bevy::prelude::*;
use craftmjne::atlas::Painters;

pub struct RubyPaintPlugin;

impl Plugin for RubyPaintPlugin {
    fn build(&self, app: &mut App) {
        app.world_mut().resource_mut::<Painters>().register("ruby", |tile, rng| {
            for y in 0..16 {
                for x in 0..16 {
                    let j = (rng() - 0.5) * 60.0;
                    tile.px(x, y, [200.0 + j, 30.0 + j / 3.0, 60.0 + j / 3.0]);
                }
            }
        });
    }
}
```

Blocks can also still be registered straight from Rust (useful for a mod
that wants to generate variants programmatically) via
`BlockRegistry::register(BlockDef { .. })` in a plugin's `build()` — same
struct, same fields, just constructed in code instead of parsed from JSON.

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
commands (`commands::execute`'s match is the extension point), an
achievements system that reads `WorldMeta::cheats` to exclude worlds that
have had commands used in them, and block-pickup-on-break (to actually let
Survival fill its inventory).

Known gaps in the fluid sim (`world.rs`'s `FluidQueue`/`recompute_cell`):
fluid levels aren't persisted across save/load (a reload snaps flowing water
back to whatever the ocean/edit log encodes — only sourced cells and
generated sea water survive as-is), two different fluids meeting has no
special interaction (a more "sourced" fluid just overwrites a less-sourced
one, since fluids default `replaceable: true`), and per-cell heights are flat
(a stepped wall renders between different-height neighbours rather than a
fully smooth, per-vertex-averaged Minecraft-style corner blend).

## License

MIT — all textures are generated at runtime; no third-party assets.
