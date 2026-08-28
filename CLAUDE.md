# Craftmjne — read this before doing anything else

This is a **Rust + Bevy native game** (see `Cargo.toml`, `src/*.rs`). It is a
complete, working, well-optimized voxel engine framework with procedurally
generated 16x16 textures, chunked async terrain generation/meshing, physics,
colored voxel lighting, a day/night cycle, a main menu with per-user saves,
a Windows installer, and a separate launcher that manages game versions.
Full details: `README.md`.

## Do not rewrite this project

A past session mistakenly assumed the repo was empty (it was working from a
stale local checkout that predated this project) and started building a
parallel implementation in JavaScript/Three.js from scratch. **Do not repeat
that mistake.** Specifically:

- **Never start a rewrite in another language or framework** (JS, Electron,
  Unity, Godot, etc.) unless the user explicitly asks for a full rewrite and
  confirms they understand the existing Rust/Bevy game will be replaced.
- **Never assume the repo is empty or minimal** based on `ls` or an old local
  clone. Before concluding there's little/nothing to build on, run
  `git fetch origin main && git log origin/main` and compare against your
  local `HEAD` — local checkouts in this environment can be stale relative
  to GitHub.
- If asked to "build a Minecraft clone", "make it a framework", "optimize
  it", "add 16x16 textures", etc. — that almost certainly means **extend
  this existing Rust/Bevy project**, not start over. Read `README.md`'s
  "Extending the framework" section and add a Bevy plugin.
- If you genuinely believe a rewrite is warranted (e.g. the user wants a
  browser-playable version alongside the native one), say so explicitly and
  get clear confirmation before writing any code — this is a decision only
  the user should make, not something to infer from an ambiguous request.

## Push to `claude/minecraft-clone-framework-2vjlng`, not `main`

As of 2026-07-12, the user asked that all work go to the
`claude/minecraft-clone-framework-2vjlng` branch instead of `main` — they'll
merge it over themselves when ready. This reverses earlier guidance in this
file/session history to work directly on `main`; that branch had been
sitting stale since the project's first two PRs while `main` moved on for
many sessions, and it's now been fast-forwarded to match `main`'s tip
(`fcc9677`) as of the switch. **Commit and push new work to
`claude/minecraft-clone-framework-2vjlng`** (`git push origin
claude/minecraft-clone-framework-2vjlng`) unless the user says otherwise —
don't default back to `main`. Before starting work each session, `git fetch
origin claude/minecraft-clone-framework-2vjlng && git checkout -B
claude/minecraft-clone-framework-2vjlng origin/claude/minecraft-clone-framework-2vjlng`
to make sure local state matches the real remote branch (see "local disk can
silently reset" below — the same staleness risk applies here).

## Quick orientation

**This is a two-crate Cargo workspace.** The game is the root package; the
launcher is `launcher/`. `cargo test` at the root still runs only the game
(that's Cargo's behavior for a workspace with a root package) - use `cargo
test --workspace` to include the launcher's tests, and remember to, because
it's easy to "run the tests" and never touch the launcher at all.

- `cargo run --release` to play; `cargo test --workspace` for everything.
- `src/` is organized as Bevy plugins — one file per plugin/subsystem
  (`world.rs`, `player.rs`, `terrain.rs`, `mesher.rs`, `atlas.rs`, etc.).
  `main.rs` just assembles them.
- `launcher/` is deliberately Bevy-free (egui/eframe): it downloads game
  versions into `versions/<version>/`, manages instances, and starts them.
  Nothing in it may depend on the `craftmjne` crate - it has to build and
  start fast, and pulling in the engine would defeat both.
- **The game no longer updates itself.** `src/updater.rs` is gone; don't
  reintroduce anything that rewrites the running executable. Version
  management belongs to the launcher.
- Building the Windows installer: see README's "Building the Windows
  installer yourself" section. Note it now packages the *launcher*, not the
  game (`-p craftmjne-launcher`).
- Game releases are cut by tagging `vX.Y.Z` (matching `Cargo.toml`'s
  `version`) and pushing the tag; `.github/workflows/release.yml` publishes
  the archives + installer. **Launcher releases are separate**: bump
  `version` in `launcher/Cargo.toml` and push to the `launcher` branch,
  which fires `.github/workflows/launcher-release.yml`.

## Bevy 0.16 API notes (verified by actually compiling, not guessed)

Confirmed working against this exact dependency set (`bevy = "0.16"`,
see `Cargo.toml`) — re-verify with `cargo check` if bumping the version.

- `Query::single()` / `single_mut()` return `Result`, they do **not** panic.
  Standard idiom throughout this codebase:
  `let Ok(x) = q.single_mut() else { return };`
- `Res<T>`/`ResMut<T>` deref-coerce to `&T`/`&mut T` at call sites, so you
  can pass `&mut some_res_mut` straight into a plain helper `fn(x: &mut T)`
  without manual derefs. Used everywhere to share logic between a system and
  its match arms (e.g. `chat.rs`'s `restore_grab`, `inventory.rs`'s `close`).
- The `Button` component **requires** `Interaction` (Bevy's required-
  components relationship) — spawn `(Button, ...)` and `Interaction`
  tracking (hover/press) comes for free, no need to insert `Interaction::None`
  yourself.
- Two `Query` params in the *same* system that both want `&mut` on the same
  component type must have provably disjoint filters or Bevy panics at
  startup with a conflict error. Fix: add `With<A>, Without<B>` /
  `With<B>, Without<A>` to each (see `chat.rs`'s `sync_chat_ui`).
- `KeyboardInput` event fields: `.key_code: KeyCode`, `.state: ButtonState`
  (compare to `ButtonState::Pressed`), `.text: Option<SmolStr>` — `.text` is
  the actual typed character(s), separate from `key_code`; read it for text
  input, not `ButtonInput<KeyCode>`.
- `EventReader<T>::clear()` drains pending events without processing them —
  used to swallow the same keypress that toggled a mode open so it doesn't
  also get typed as a character (see `chat.rs`'s `just_opened` handling).
- Scrollable UI: put `overflow: Overflow::scroll_y()` on the `Node` *and* a
  `ScrollPosition` component (plain `offset_x: f32, offset_y: f32`, directly
  mutable) on the same entity. There is no built-in mouse-wheel-to-scroll —
  you write a system that reads `MouseWheel` events and adjusts
  `ScrollPosition.offset_y` yourself (see `inventory.rs`'s
  `scroll_creative_list`).
- `AlignContent::FlexStart` is the correct variant name (not `Start`) for
  aligning wrapped flex content.
- `Window::cursor_position() -> Option<Vec2>` gives window-space pixel
  coordinates (origin top-left) — use it to position cursor-following UI
  like tooltips via `Node.left`/`.top` in `Val::Px` (see `inventory.rs`'s
  `sync_tooltip_ui`).
- `CursorGrabMode::Locked` is the FPS-style mouse-capture state;
  `CursorGrabMode::None` releases it.

## This project's established UI/input patterns

Follow these when adding another modal overlay (a new screen, HUD panel,
etc.) instead of inventing a new approach:

- **Toggleable overlay resource**: `#[derive(Resource, Default)] struct
  XState { open: bool, was_grabbed: bool, ... }`. On open: record whether
  the cursor was grabbed (`was_grabbed = grab_mode != None`), then free it
  (`grab_mode = None, visible = true`). On close: restore it only if
  `was_grabbed`. Copy this from `chat.rs` (`ChatState`) or `inventory.rs`
  (`InventoryState`) rather than re-deriving it.
- **Mutual exclusion between overlays is manual and easy to miss.** Every
  overlay's open-toggle system must check every *other* overlay's `open`
  flag before firing, and `player.rs`'s `cursor_grab` (Escape → pause) and
  `player_update` (movement freeze) must check all of them too. When adding
  overlay N+1: `grep -rn "chat.open\|paused.open\|inventory.open"` across
  `player.rs`, `interact.rs`, and every other overlay's toggle system, and
  add the new flag everywhere an existing one appears.
- **"Blocks input" and "freezes the world" are different things — don't
  conflate them.** `player_update` used to skip `player.step()` entirely
  whenever chat *or* the inventory *or* the pause menu was open, which
  looked like the whole game pausing just from opening your inventory
  (gravity stopped, you'd hang frozen mid-fall). Only the real pause menu
  (`paused.open`) should stop simulation; overlays like chat/inventory that
  merely want the cursor and WASD should instead call `player.step()` every
  tick as normal but pass it an empty `ButtonInput::<KeyCode>::default()`
  in place of the real one, so gravity/buoyancy/momentum keep integrating
  underneath them (matches vanilla Minecraft: E doesn't stop you falling).
- **Spawn-on-change UI rebuild**: a marker `Component` for the root entity,
  and a system that despawns-and-respawns the whole subtree whenever the
  backing resource(s) `.is_changed()` — don't hand-patch individual nodes.
  Pattern used by `ui::rebuild_hotbar`, `menu::rebuild_worlds_content`,
  `menu::sync_pause_screen`, `inventory::sync_inventory_screen`.
- **Block icon rendering**: always go through `ui::block_icon(id, &registry,
  &tables, &atlas, &icon_atlas) -> ImageNode` rather than constructing an
  `ImageNode` by hand - it's the one place that honors `ItemModel` (baked
  isometric icon for `Default`, flat single-face crop for `Face`/`Custom`)
  so every call site (hotbar, inventory screen, Creative's grid) stays
  consistent as that enum grows more variants. Always special-case `id ==
  blocks::AIR` and skip drawing an icon entirely before calling it - the
  tiles table has no meaningful entry for air (defaults to 0, i.e. garbage/
  first-tile), it is not "no texture" by convention.
- **Baking a derived image from the procedural atlas at startup**:
  `icons.rs`'s isometric icon baker is the template for "generate a second
  texture from the first one, once, at startup" - build it as pure CPU
  pixel math operating on `AtlasData`'s raw buffer (no GPU/shader
  involvement), store the non-render data as one `Resource` (`world::
  IconAtlas`, built in `world::compile_content` right after the main
  atlas), then upload it to the GPU as a second `Image` in `render::
  setup_render` (mirrors exactly how the main atlas itself is uploaded) and
  expose it as its own `Resource` (`render::IconAtlasImage`). For any
  "map every destination pixel back to a source pixel" transform
  (shearing, projecting, tiling), inverse-map from the destination side -
  iterating destination pixels and solving for the source coordinate is
  gap-free by construction, where forward-mapping source pixels onto a
  larger/differently-shaped destination is not.
- **Block registry**: `Res<BlockRegistry>.def(id) -> &BlockDef`,
  `.id(name)` (panics if unknown, fine for hardcoded names), `.by_name(name)
  -> Result<..., UnknownBlock>` (non-panicking, use when loading untrusted
  save data), `.defs: Vec<BlockDef>` (iterate `.enumerate().skip(1)` to
  skip `AIR = 0`). Block content itself is data, not code — one JSON file
  per block in `blocks/`, loaded by `BlockRegistry::with_defaults` at
  startup (`blocks.rs`'s module docs have the full schema). Programmatic
  `.register(BlockDef {..})` from a plugin still works too, for content
  that's easier to generate than to hand-write as JSON.
- **`texture_scheme` derives per-face texture *names* from a block's `id`,
  so most blocks never need to spell out a `textures` field by hand.**
  `TextureScheme` (`blocks.rs`) is a fixed enum of naming conventions
  (`default`/`log`/`organic`/`interface`/`advanced_interface`, plus
  `custom` reserved for a future fully-independent per-face mapping) - each
  variant just maps a face index to a `{id}_{suffix}` name (or plain `id`
  for an un-suffixed face) via `TextureScheme::suffix`, consumed by
  `BlockDef::texture_name(face)`. An explicit `textures.top`/`bottom`/
  `side`/`all` value, if a block's file sets one, always overrides whatever
  the scheme derived for that specific face - `blocks/grass.json` uses
  `organic` (which alone would look for `grass_bottom`) but overrides just
  `bottom` to reuse `dirt`, demonstrating the two compose per-face rather
  than being mutually exclusive. This only solves *naming* - every name a
  block ends up needing (derived or explicit) still has to resolve to an
  actual tile, which is a second, previously-separate problem: every
  `atlas.rs` painter is manually registered by name, so a scheme deriving a
  name nobody wrote a painter for used to panic at `compile()`. Fixed by
  `BlockRegistry::texture_names()` (every name every block's six faces
  resolve to) walked once at startup (`world::compile_content`, before
  `build_atlas`) to call `Painters::ensure_registered` for any name not
  already known — which registers a checkerboard "missing texture"
  placeholder painter (visually obvious, never silently reuses another
  block's art) rather than leaving the name unresolvable. A later
  `textures/blocks/<name>.png` overrides the placeholder exactly like any
  other tile, so the intended workflow is: write the JSON with a scheme,
  get an obviously-placeholder-textured block that runs fine, then drop in
  real art whenever it's ready - never a hard blocker either way.
- **Finding a shipped data directory at runtime** (`blocks.rs`'s
  `find_blocks_dir`): try `std::env::current_exe()`'s parent dir first (how
  an installed/distributed build finds files shipped next to it), fall back
  to a plain relative path (how `cargo run`/`cargo test` find one at the
  repo root — Cargo runs both with the package root as cwd). Never resolve
  via `CARGO_MANIFEST_DIR`/other compile-time env vars for this — that path
  only exists on the machine that *built* the binary, not the end user's.
  Reuse this pattern for any future shipped-data-folder feature.
- **Separate "how it renders" from "what it does."** When generalizing a
  special-cased block (water) into a data-driven flag, don't let a single
  boolean/field control two unrelated things just because the one existing
  example (water) happens to want both. `mesher.rs`'s fluid-surface-height
  cap is driven by `tables.fluid[id]`, independent of `tables.translucent
  [id]` (which drives solid-vs-blend bucket routing) — a hypothetical
  non-fluid translucent block, or a future non-translucent fluid, both stay
  representable. If you catch yourself reusing one flag to gate two
  behaviors "because that's what the current content needs," that's the
  moment to split it, before more content ossifies the coupling.
- **`ChildSpawnerCommands`** is the parameter type for small reusable
  `fn spawn_thing(parent: &mut ChildSpawnerCommands, ...)` helpers called
  from inside `.with_children(|parent| ...)` closures (see `menu::
  spawn_button`, `inventory::spawn_slot_row`).
- **Test pure logic, not system wiring.** There are no tests for the Bevy
  systems in `chat.rs`/`menu.rs`/`ui.rs`/`inventory.rs` themselves (would
  need a full headless app harness for little payoff); do unit-test the
  pure helper functions inside them (parsers, name formatting, round-trips)
  the way `commands.rs` and `inventory.rs::display_name` do.
- **Turning a compile-time constant into a runtime-determined value ripples
  further than it looks - find every consumer before writing code.**
  `TILE_SIZE`/`ATLAS_PX` went from a `pub const` in `config.rs` to a value
  only known once the atlas is actually built (`atlas::AtlasData::
  tile_size`, auto-detected from whatever custom textures exist in
  `textures/blocks/`), so the game could render at 32x32/64x64 when real
  art is supplied instead of being stuck at the base procedural 16x16
  forever. The grep that mattered before writing a single line: `grep -rn
  "TILE_SIZE\|ATLAS_PX\|ATLAS_TILES"` across the whole `src/` tree - it
  touched `atlas.rs` (obviously), but also `mesher.rs`'s UV padding math,
  `icons.rs`'s entire isometric-projection geometry *and* its own derived
  `ICON_SIZE`/`ICON_ATLAS_PX`, `render.rs`'s GPU image dimensions, and
  `ui.rs`'s pixel-space icon-cropping rects - five files, none of them
  obviously "about textures" from their names alone. Two things made this
  tractable instead of a sprawling mess:
  - **Not everything that referenced the old constant actually needed the
    new runtime value.** `mesher.rs`'s `FLUID_SURFACE` (water sits one
    sixteenth of a block below the true top) and the falling-water taper's
    sliver height are gameplay-geometry constants that happened to reuse
    `TILE_SIZE` for convenience, not because a higher-resolution atlas
    should make water dip by a smaller fraction - these correctly stayed
    pinned to the base resolution (`atlas::BASE_TILE_SIZE`, a real
    always-16 constant, kept separate from the atlas's *actual* resolution
    on purpose). Don't reflexively thread the new value everywhere the old
    constant appeared; ask what each usage was actually *for* first.
  - **Give tests a way to inject the controlled input a real startup path
    resolves automatically.** `atlas::build_atlas()` (the real entry point,
    used everywhere, resolving `textures/blocks/` itself) stayed untouched
    so none of the other ~10 call sites needed editing; a second function,
    `build_atlas_from_dir(painters, dir)`, took the actual resolution-
    picking logic and an explicit directory parameter, letting a test drop
    a real 32x32 PNG in a scratch dir and assert the *whole atlas* (not
    just that one tile) ended up at 32x32 with correctly upscaled
    procedural neighbours - mirrors `blocks.rs`'s `with_defaults()` /
    `load_from_dir()` split for exactly the same reason.

## This project's simulation patterns

- **Generic per-cell simulations use a budgeted queue + a single pure
  "recompute this cell" function, never a full-grid scan.** The fluid sim
  (`world.rs`'s `FluidQueue`/`recompute_cell`, driven by `blocks.rs`'s
  `FLUID_SOURCE`/`FLUID_FALLING` + `Tables::fluid`/`flow_distance`/
  `replaceable`) is the template: `BlockSetEvent` seeds the queue with the
  changed cell + its 6 neighbours, a `Local<f32>` accumulator ticks a fixed
  number of times per frame, and each tick pops a bounded budget and calls
  the pure recompute fn, which itself re-enqueues neighbours only when it
  actually changed something. This makes spread visibly gradual instead of
  resolving in one frame, and keeps the algorithm keyed only on `Tables`
  data (never a hardcoded block id) so it needs zero changes for a second
  fluid. Reuse this shape for any future propagating simulation (light,
  fire spread, etc.) instead of writing a fresh scan-the-world system.
- **Simulated state changes must not go through the same path as player
  edits, even when that state IS persisted.** `ChunkMap::set_block` fires a
  `BlockSetEvent` (which `record_edits` accumulates into `EditLog`) — a
  per-tick simulation writing through it would insert into that map
  thousands of times a second during a big spread, for no reason (only the
  *final* state matters). Simulated writes get their own setter
  (`ChunkMap::set_fluid_cell`), same grid update + dirty-marking as
  `set_block`, but skipping the event entirely.
  **This doesn't mean fluid state goes unsaved** (an earlier version of
  this file said so — that was the wrong call: it left an in-progress
  design where a placed water source came back on reload but everything it
  had spread into didn't, since that spread was never captured any other
  way, and re-deriving it via `FluidQueue` on load risked the exact
  live-vs-reloaded convergence mismatch a save is supposed to prevent).
  Fluid state genuinely is fully saved now — every cell, not just sources —
  via a *different* mechanism than edits: `write_save` scans every
  currently-loaded chunk fresh on every save (`save::FluidCell`s: id +
  level, not a diff against terrain) and falls back to the previous save's
  data (`world::OriginalFluids`) only for chunks the player didn't revisit
  this session. Reapplying on load (`collect_gen_tasks`) is a straight
  `set_block` + `set_fluid_level_raw` per saved cell - zero `FluidQueue`
  involvement, so a reload never has to (and structurally *can't*) converge
  to something different than what was actually there. The general
  takeaway: "must not share the player-edit event path" and "must not be
  persisted" are two separate decisions - a continuously-changing
  simulation still needs a snapshot-style persistence strategy of its own
  if the alternative (re-deriving on load) can't be guaranteed to reproduce
  the exact same result, it just can't be the same *incremental,
  event-driven* mechanism blocks use.
- **`EditLog` must be seeded from the save file at load time, not started
  empty.** It used to reset to `EditLog::default()` on every `enter_world`
  and only grow from this session's own `BlockSetEvent`s; `write_save`
  serializes *only* `EditLog`. Combine those two facts and any edit whose
  chunk the player didn't happen to revisit this session - its data lived
  only in the separate, chunk-generation-triggered `PendingEdits`, which
  nothing ever serializes - would silently vanish from the save the moment
  something else triggered the *next* autosave or exit. One session
  wouldn't show the bug (the edit's still correctly visible in memory,
  reapplied via `PendingEdits` same as ever); it takes a *second* reload to
  notice the edit never made it back, by which point there's no trail
  connecting the loss to its cause. Fixed by building `EditLog` from
  `data.edits` up front (same loop that builds `PendingEdits`), so it always
  holds the complete old+new picture regardless of what got visited. Same
  root-cause shape as the fluid point above - anything that's supposed to
  be "the complete authoritative record for saving" has to actually start
  complete, not empty-plus-hope-everything-gets-revisited. Test this kind
  of bug with a *second* reload cycle, not just one - `tests/headless.rs`'s
  `edits_in_unvisited_chunks_survive_a_second_reload` is the pattern: edit
  something, leave, reload-without-revisiting-it, leave again (the save
  that silently drops it), reload once more and check it's still there.
- **A block's per-cell dynamic state (beyond its id) lives in a second
  `Vec` parallel to `Chunk::blocks`**, not packed into the `BlockId` or a
  separate side-table keyed by position. `Chunk::fluid_level: Option<Vec<u8>>`
  mirrors `blocks` exactly (same index, same lifecycle — both `Some` the
  moment generation finishes, both copied together in `build_padded`). Reuse
  this shape for any future per-block runtime state (growth stage, charge
  level, etc.) rather than inventing a `HashMap<IVec3, T>` side-channel.
  `Chunk::axis` (block rotation) is the second example of this shape, and
  it's persisted through a *different* mechanism than `fluid_level` even
  though both are now saved: `axis` only ever changes on a discrete player
  action (placing a rotating block), so it fits the same incremental,
  `BlockSetEvent`-driven path as ordinary block edits (`EditLog`, now keyed
  to `(BlockId, u8)` instead of bare `BlockId`; `save::BlockEdit` gained a
  `#[serde(default)] axis: u8` field - the `default` matters, so old saves
  without the field still load instead of failing to parse). `fluid_level`
  changes continuously (every simulation tick, not a discrete action), so it
  needs the scan-based snapshot approach described above instead. Before
  adding a new per-cell `Vec`, figure out which shape its updates have -
  sparse and event-driven (reuse the edit-log path) or dense and continuous
  (reuse the fluid scan-and-snapshot path) - since copying the wrong sibling
  silently drops or bloats data.
- **When a per-instance variation only kicks in for a handful of block ids,
  give `Tables` a `Vec<bool>` gate (`rotates`, mirroring `fluid`/
  `replaceable`) and make the general-case formula reduce to a no-op when
  the gate is false**, rather than branching between two separate code
  paths. `mesher.rs`'s `rotated_tile` (remaps a face index through a stored
  rotation axis to pick the right atlas tile) is written so that `axis ==
  AXIS_Y` (the default every non-rotating block implicitly has) produces
  exactly the original unrotated `tiles[id*6+f]` lookup - so the mesher can
  call it unconditionally for every block, and the *only* thing gating
  behavior is whether `padded_axis` is even consulted (`tables.rotates[id]
  ? padded_axis[cell] : AXIS_Y`). This sidesteps a whole class of staleness
  bug for free: a cell's leftover `axis` value from a rotating block that
  was later broken and replaced with a non-rotating one is simply never
  read, so there's no need to reset it on every `set_block` "just in case."
- **A pull-based relaxation ("what's the best value my neighbours currently
  offer me") must never let an already-filled cell adopt a *worse* value
  than it already has — only improve, or reset to empty.** `world.rs`'s
  `recompute_cell` first allowed a flowing fluid cell to fall back to a
  worse-but-still-wet level when its real supply was cut, reasoning
  "closest neighbour's level + 1" fresh each time. That's fine for filling
  empty cells, but for an *already-fluid* cell it lets a removed source's
  former network "downgrade through itself" indefinitely — cell A relaxes
  to a worse level derived from B, which enqueues B, which relaxes to a
  worse level derived from A's new value, forever (this is the classic
  "Dijkstra doesn't handle edge/source removal" problem: relaxation only
  has a termination proof when values monotonically improve). Fix: compare
  the candidate against the cell's current value via a rank function
  (`fluid_rank`, source/falling both rank `0`, best); accept only if it's
  a genuine improvement, otherwise drop straight to empty instead of the
  worse value. Emptying is monotonic (a cell only empties once) and a
  neighbour with a real remaining path simply re-fills it on a later pass.
  Apply this to *any* future pull-based propagating sim, not just fluids.
- **An "am I near the edge" collision probe must scan a range, not check one
  exact cell — a moving body will drift past a single-cell window before
  anything reacts to it.** `player.rs`'s swim-to-shore climb assist
  (`Player::assist_climb_out`) first checked only `feet+1`/`feet+2` for a
  clear opening; that's only true in the single block nearest the surface,
  so a player still a block or two deep (the common case — nothing pins you
  to the top of a pool) sinks past that window before horizontal contact
  with the wall ever triggers the check, and the assist never fires. Fixed
  by scanning upward from the current feet cell (bounded by
  `MAX_CLIMB_HEIGHT`) for the first opening with headroom, so it keeps
  re-checking and pulling you up every tick from wherever you actually are,
  not just the one instant you'd need to already be at the top. Also don't
  key an assist like this off a *different* system's existing "am I
  submerged" sample if that sample uses a different reference point (here,
  `step()`'s chest-height `in_water` flips false the instant your chest
  clears the surface, well before your feet reach ledge height — reusing it
  cut the climb short right at the finish line). Give the assist its own
  probe at the reference point it actually cares about (feet, here).
- **When a "does this converge" test times out, don't assume it's a true
  infinite loop before measuring.** The fix above was first diagnosed as a
  hang from a 10k-iteration guard tripping; instrumenting the loop (a
  `guard % N == 0` print) showed it was actually converging correctly at
  ~15-20k iterations in under the same test run — the *test's* synthetic
  chunk had no floor, so an unrelated waterfall fell through open space and
  flooded a much bigger volume than the scenario needed. The real fix was
  giving the test a floor (`fill_floor` in `world.rs`'s test module) so it
  only exercises what it's actually testing, not raising the guard blindly.
- **Rendering variable per-block height needs the "step wall," not just a
  lower cap.** Culling a face just because the neighbour is the same block
  id (`mesher.rs`'s original `nid == id` skip) is only correct when every
  instance of that id renders at the same height. Once instances can differ
  (flowing water at different levels), same-id neighbours need a corner-level
  check: fully cull only if the neighbour's rendered top is >= this cell's,
  otherwise emit a partial quad from the neighbour's height up to this
  cell's. See `mesh_chunk`'s `is_side`/`bottom` handling — the same pattern
  generalizes to any future variable-height content (snow layers, etc.).
- **When asked to keep an old visual/behavior around "in case we want it
  later" instead of deleting it on a replace, wire it behind a real
  compile-time (or runtime) switch, don't just leave the removed code
  commented out or only in git history.** `mesher.rs`'s `FallingWaterStyle`
  (`Blocky` vs `Sloped`) is the pattern: an enum + a single const the whole
  behavior is gated on, so flipping it is a one-line, actually-compiled,
  actually-tested change rather than an archaeology exercise through commits.
  A variant that's only reachable by editing the const needs `#[allow
  (dead_code)]` on it specifically (with a comment saying why) or it warns.

## Environment gotchas (this remote session, not Bevy)

- **Local disk can silently reset between conversation turns** — the git
  working tree and `~/.cargo` cache have both reverted to an earlier
  snapshot mid-session more than once. Always `git status --short && git
  log --oneline -3` before trusting local state; resync with `git fetch
  origin <branch> && git checkout -B <branch> origin/<branch>` if stale.
  Don't assume a clean `cargo check` means the tree is what you last left it.
- **`git push` works for branches but 403s on tags** (both creating and
  deleting) with the credentials available in this environment. That means
  Claude sessions in this repo **cannot cut releases themselves** — releases
  are manual and belong to the user. As of 2026-07-12 the user explicitly
  chose manual releases over the old auto-tag-on-Cargo.toml-bump workflow
  (which has been removed); they tag/release by hand from a normal checkout
  or the GitHub UI (Releases → Draft a new release → type the new tag),
  which fires `release.yml`'s `on: push: tags: ["v*"]` trigger directly. If
  the user reports "I tagged/released and nothing happened," the far more
  likely explanation is they checked within a minute or two of tagging — a
  full build across all three platforms takes ~10-15 minutes, and Windows in
  particular has consistently been the slowest leg. Check
  `mcp__github__actions_list` (`list_workflow_runs` for `release.yml`) and
  `list_workflow_jobs` for the run before assuming anything is broken.
- **A GitHub Actions matrix job can get zero hosted-runner capacity and sit
  "queued" forever** (`runner_id: 0`, never assigned) — this happened to
  `macos-13` for the `v1.1.1` release, which sat stuck for hours and
  produced a GitHub Release with **no assets at all**, breaking the in-game
  auto-updater for everyone until it was diagnosed (`macos-13` was removed
  from `release.yml`'s matrix as a result — Apple Silicon (`macos-14`)
  covers current Macs; re-add an Intel leg if GitHub ships a working runner
  image for it again). This is a *different* failure mode than a leg merely
  failing or getting cancelled: `release.yml`'s `release` job uses `if: ${{
  !cancelled() }}` so a matrix leg that fails/cancels doesn't block
  publishing the platforms that did succeed, but that guard only helps once
  every leg reaches *some* terminal state — a job that never gets scheduled
  at all keeps `needs: build` unsatisfied indefinitely, and GitHub only
  force-cancels a queued-forever job after 24h. If a release is suspiciously
  slow or an in-game update check keeps finding nothing new, check
  `actions_list`/`list_workflow_jobs` for the latest release run for a leg
  stuck at `status: queued` with no `runner_id` — don't assume the workflow
  is just being slow.
- **NSIS (`makensis`) resolves relative `File` paths against the `.nsi`
  script's own directory**, not the invoking working directory. `SRC_EXE`
  in `installer/craftmjne.nsi` must be absolute or the build silently
  resolves it wrong and fails with "no files found" (this bit `v1.1.0`'s
  release before `SRC_EXE` was made absolute in CI).
- `.claude/hooks/session-start.sh` (registered in `.claude/settings.json`)
  pre-warms the Cargo cache in the background on remote session start —
  print `{"async": true, "asyncTimeout": ...}` as the *first* line of stdout
  to run it non-blocking.
- **Two different, non-opaque blocks sharing a face plane z-fight** (e.g.
  water next to glass): `mesher.rs`'s culling only skips a face when the
  neighbour is opaque, or the *same* fluid at the *same* height — a
  different non-opaque neighbour (glass, or a different fluid) correctly
  keeps both faces (you're meant to see through one to the other), but that
  means both faces sit at the exact same world-space plane, which is a
  textbook z-fight (flickering/tearing, reported as "water on glass is
  clipping"). Fixed with a tiny inward nudge (`COINCIDENT_FACE_BIAS =
  1/512`, well under a texel) along each face's own outward normal
  (`Face::dir`), applied only when `nid != 0 && nid != id` — i.e. only the
  genuinely-coincident case, not the normal opaque-culled or
  same-fluid-step-wall cases. When writing the test for this
  (`glass_next_to_water_does_not_z_fight`), the first version filtered
  `mesh.water.positions` for any vertex near `x=5` and took a min/max — that
  incorrectly swept in the water block's *other* faces (top/bottom/±Z),
  which also touch `x=5` as part of their own footprint but were never
  supposed to be biased. A temporary `eprintln!` in the hot loop confirmed
  the production bias math was right all along; the fix was tightening the
  test to assert the *exact* expected biased coordinate
  (`5.0 - COINCIDENT_FACE_BIAS`) via `.any()` with a `1e-6` epsilon instead
  of a loose filter+fold. Lesson: when a new test fails, don't assume the
  production code is wrong — a quick throwaway probe (temporary test or
  eprintln, removed once the question is answered) is faster than guessing
  which side of the assertion is broken.
- **The auto-updater used to download-and-swap the `.exe` silently mid-session,
  the instant a background thread noticed a new release** — reported as "no
  update window on close, and the exe never actually updates." Diagnosed by
  reading the actual vendored `self_update`/`self-replace` crate source
  (`~/.cargo/registry/src/.../self_update-*`, `self-replace-*`) rather than
  guessing from memory, since the failure mode ("banner said it worked, but
  relaunching still runs the old build") pointed at something subtler than
  our own wrapper code. The mechanism itself checked out as correct
  (same-directory rename dance, no cross-volume issues, no admin rights
  needed); the far more likely culprit is a real, unfixable-from-our-side
  one: **a process silently rewriting its own on-disk binary is textbook
  behavior antivirus/EDR real-time protection is built to catch and
  revert** - and it can do so *after* our code already reported success,
  since Windows Defender scans asynchronously on file events. Given that,
  the fix was architectural: split the updater into "check + download +
  stage" (still eagerly in the background, `updater::check_and_stage`,
  using `self_update`'s lower-level `ReleaseList`/`Download`/`Extract`
  building blocks instead of its monolithic `Update::update()`, since that
  method inseparably bundles the download with the swap) and "swap"
  (deferred to the moment the game is actually closing, via
  `updater::gate_quit`/`apply_update_then_exit`). Both the in-game Quit
  button and the OS window's close button now route through a
  `QuitRequested` event instead of writing `AppExit` directly, so a staged
  update gets exactly one chance to apply - with a real, visible
  "Updating..."/failure banner held up for a minimum dwell
  (`MIN_APPLY_VISIBLE`/`_FAILED`) - before the process actually exits. This
  needed `WindowPlugin { close_when_requested: false, exit_condition:
  ExitCondition::DontExit, .. }` in `main.rs` to fully take over window-close
  handling; Bevy's default behavior despawns the window (and thus loses the
  ability to render an overlay into it) before ever getting a chance to
  intercept the close request. **Test hazard worth remembering**: never let
  a test reach the code path that actually calls
  `self_update::self_replace::self_replace(..)` - it operates on
  `env::current_exe()`, so calling it from `cargo test` would rewrite the
  *test binary's own executable* on disk. `updater.rs`'s tests only exercise
  `gate_quit`'s branching (does a staged update defer the exit or not) and
  deliberately never add `apply_update_then_exit` to the test schedule.
- **A fully unlit renderer (see `render.rs`'s module docs - no lights, no
  normals, lighting pre-baked into vertex colors by the mesher) has no real
  light source to dim for a day/night cycle.** `sky.rs`'s fix: one global
  `f32` uniform (`ChunkMaterialParams::sky_light`), written into *both*
  chunk materials once a frame from `update_sky`, multiplied straight into
  `chunk.wgsl`'s final lit color alongside the existing baked-AO vertex
  color. Cheap (two tiny uniform writes, no remeshing) despite touching the
  whole visible world's apparent brightness at once - the general pattern
  for "this renderer has no per-fragment lighting pass to hook a new light
  into" is a single small uniform broadcast to the shared materials, not a
  new render pass.
- **A billboard that must face the camera at every possible angle,
  including straight overhead, breaks `Transform::looking_at` at the
  overhead instant.** `sky.rs`'s sun/moon orbit passes exactly through the
  zenith once a cycle (straight up from the camera) - at that instant the
  look direction is exactly parallel to `Vec3::Y`, `looking_at`'s only
  degenerate case (forward and up vectors can't both define "which way is
  right"). Fixed by using `Quat::from_rotation_arc(Vec3::NEG_Z, dir)`
  instead, which needs no separate up vector at all and so has no pole to
  break at - safe here specifically because both textures are radially
  symmetric discs, so the uncontrolled roll `from_rotation_arc` leaves free
  is never visible. Reach for `looking_at` only when roll actually matters
  (and the look direction is guaranteed never parallel to `up`); reach for
  `from_rotation_arc` for anything rotationally symmetric that must face a
  point from every angle.
- **Compass directions weren't invented for `sky.rs` - they already existed.**
  `blocks.rs`'s face-order doc comment (`0:+x east, 1:-x west, 2:+y top,
  3:-y bottom, 4:+z south, 5:-z north`, driving `TextureScheme::Interface`'s
  north-face naming) is this engine's one and only definition of which
  world axis is which cardinal direction. The sun/moon's "rise due east,
  set due west" reuses it verbatim rather than picking a fresh mapping -
  worth grepping for before inventing compass semantics anywhere new.
- **Continuous per-world state that isn't a fluid still needs the same
  save discipline fluids established.** `sky::DayNightClock` persists via
  `save::WorldData::time_of_day`/`::day_count` (`#[serde(default)]` so a
  pre-cycle save just resumes at dawn/a new moon), read in
  `world::enter_world` and written by both `autosave` and `exit_world`
  through the same `write_save` every other per-world resource already
  flows through - not a new persistence mechanism, just two more fields
  riding the existing one. Tested with a plain single-reload round-trip
  (`tests/headless.rs`'s `time_of_day_and_moon_phase_persist_across_a_reload`)
  rather than the fluid-specific two-reload-cycle pattern, since the clock
  has no "unvisited chunk" concept to lose data through - it's two scalars,
  not a per-cell scan.
- **A masked-image "phase" system (moon phases, or anything similar - a
  card face, a damage-state overlay) is simpler as one base texture plus a
  per-pixel visibility test than as N independent images.** `sky.rs`'s 8
  moon phases are all generated from the *same* base moon texture
  (procedural or a custom `moon.png`) by `mask_moon_phase`, which forces
  everything `moon_lit` calls dark to fully transparent - so a custom texture
  override automatically gets correct phase shapes with zero extra files,
  and adding a 9th "phase" concept later would need one new mask function,
  not 9 new art assets. Don't reach for N separate override slots when "one
  base image + a pure per-pixel classifier" covers the same ground with far
  less to keep in sync.
- **A real elliptical terminator (lit/dark boundary on a sphere-viewed-as-
  disc) is barely more code than a flat vertical chord, and looks
  meaningfully more authentic - work out the closed form before settling
  for the cheap version.** The disc's own edge at height `ny` sits at
  `sqrt(1-ny²)`; the terminator at that same height is just that edge
  scaled by `cos(theta)` (`theta` = phase angle, `0`=new, `PI`=full) -
  `moon_lit`'s entire boundary test is `nx >= cos(theta) * sqrt(1-ny²)`
  (mirrored for the waning half). This collapses to an exact vertical line
  at the quarter phases (`cos(PI/2)==0`, astronomically correct - the
  terminator really is a straight diameter exactly at quarter moon) and
  bows into a proper tapering lens shape everywhere else, for one extra
  multiply over a flat-chord version. Verify trig-heavy pure functions like
  this with a real calculator/script before trusting hand-derived
  intuition about the shape - an earlier test draft asserted a fixed screen
  position went dark moving away from the equator, reasoning "the crescent
  tapers to a point so it must narrow inward"; running the actual formula
  through Python showed the opposite (the lit *fraction* of each row is
  constant across latitude, so a fixed x can cross from dark to lit further
  from the equator, not the reverse) - the geometry was right the whole
  time, the intuition-first test was wrong, caught by computing rather than
  asserting first.
- **A cosmetic-only calendar/season system is still a real feature worth
  building deliberately, not a rushed stub - but it also doesn't need to
  reach further than what it's actually gating.** Asked to add red/blue/
  green full moons where blue must never fall in winter and green must
  never fall in spring, the honest blocker was that no season concept
  existed yet - rather than silently picking arbitrary months and hoping
  they'd stay non-conflicting forever, or silently building a full
  temperature/biome-affecting season system nobody asked for, the right
  move was asking whether to build a minimal one now (see `AskUserQuestion`
  - this is exactly the kind of scope-defining call that's the user's to
  make, not an assumption to bake in either direction). `sky::Season` is
  the result: a pure calendar enum derived from `DayNightClock::day_count`
  (`DAYS_PER_MONTH=8` intentionally matches the moon's own phase cycle, so
  a full moon always lands mid-month; `MONTHS_PER_YEAR=12` in the real
  spring/summer/autumn/winter order), consulted by exactly one thing
  (`moon_event`) and nothing else - no terrain/temperature/gameplay hook,
  since none of that was asked for. (A follow-up request replaced the
  original fixed-month constants with a random-per-year schedule - see the
  next entry - but `Season` itself and its single consumer are unchanged.)
- **A recolor-only game event ("this full moon is special") is a tint
  multiply on the existing material, never a second texture or asset.**
  `sky::moon_event_tint` returns a plain `LinearRgba`; `update_sky` folds
  it straight into the same `CelestialParams.tint` uniform already driving
  the horizon fade (`tint.rgb` = the event color, `tint.a` = the existing
  fade), applied only to whichever phase material is currently shown - so
  red/blue/green moons cost nothing beyond three constants and one extra
  multiply already sitting in the per-frame update, no new draw call, no
  new mesh, no new image upload. Reach for a tint uniform before a new
  sprite/material any time the "special" version is still fundamentally
  the same shape as the normal one.
- **When a user explicitly asks to avoid rewriting the same rule N times,
  that's a request for one declarative table + one generic algorithm, not
  N parallel `if`/`match` arms that happen to look similar.** A follow-up
  to the red/blue/green moon feature above asked for: only ever on a full
  moon (a reusable yes/no, defaulting no); season exclusion expressed as a
  plain list ("spring, summer, ..." with commas for more than one) instead
  of a hand-picked month + a bespoke test proving it doesn't conflict; red
  bumped from "every month" to twice a year; and blue/green (now red too)
  landing on a *different, randomly chosen* month each year instead of a
  fixed one. `sky::MoonEventDef` is the single declarative shape all three
  events share (`per_year`, `excluded_seasons: &[Season]`,
  `requires_full_moon: bool`) collected into one `MOON_EVENTS` table;
  `year_schedule` is the one generic algorithm that reads it - looping the
  table, for each entry filtering `0..MONTHS_PER_YEAR` down to months both
  unclaimed *this year* and not in an excluded season, then drawing
  `per_year` of them via a seeded RNG and marking them claimed before the
  next entry runs. Adding a fourth event, or changing red from 2/year to
  3, is a one-line table edit - no new branch anywhere. **The "random"
  still has to be the engine's usual seeded-not-true-random**, so a reload
  shows the same year's schedule and different worlds/years genuinely
  differ: seeded from `hash_str("moon-events-{world_seed}-{year}")`, with
  each table entry's own RNG stream derived by offsetting that seed by a
  large prime times its index (`i * 104_729`) - cheap, no stored/cached
  schedule needed anywhere, since `year_schedule` is a pure function of
  `(world_seed, year)` and can just be recomputed on demand (currently:
  every frame in `update_sky`, negligible cost for 12 months x 3 events).
  **Claim-as-you-go is what makes collisions structurally impossible**
  without any cross-event coordination: since each entry's candidate pool
  excludes every month already claimed by an earlier entry in the same
  `year_schedule` call, two events can never end up double-booking a
  month regardless of how their excluded seasons happen to overlap (blue's
  non-winter pool and green's non-spring pool actually *do* overlap in
  summer/autumn once expressed this generically - unlike the old fixed-
  month version where that never came up) - test this invariant directly
  (`year_schedule_always_places_the_right_counts_with_no_overlap_or_excluded_season`
  loops several seeds/years and asserts zero double-booked months), don't
  just trust the algorithm's shape to guarantee it.
- **"No two X back to back" is a constraint on the *shared claimed set*,
  not a per-event rule - implement it once, in the one place that already
  tracks what's claimed.** A follow-up asked that no two special months
  ever land adjacent (any event next to any other, not just the same kind
  twice), as a reusable opt-in flag like `requires_full_moon`. The natural
  place to enforce it is inside `year_schedule`'s own claiming loop
  (`requires_gap_month` on `MoonEventDef`, checked by `touches_a_claimed_
  month` against the in-progress `schedule` array) rather than as a
  separate post-hoc validation pass - the function already recomputes each
  occurrence's eligible pool fresh against the current claims (needed
  anyway so an event drawing 2+ occurrences, like red, can't land its own
  two next to each other either), so the gap check is just one more
  predicate in that same filter. **Before trusting a greedy sequential
  picker to always satisfy a new constraint, measure the failure rate
  rather than assume it from the algorithm's shape** - added a throwaway
  test that swept 15,000 (seed, year) pairs counting how often the
  eligible pool ran dry before an event's full quota was drawn, saw zero
  shortfalls, and only then kept the existing tests' exact-count
  assertions rather than loosening them defensively; deleted the sweep
  once it had answered the question (see `no_two_special_months_are_ever_
  adjacent` for the permanent, narrower regression test that stayed).
  Originally did *not* wrap year-end into the next year's month `0`
  (each year was scheduled independently, so Dec of year N next to Jan of
  year N+1 could still slip through) - documented as a known
  simplification rather than silently ignored, and fixed in a follow-up
  once the user confirmed they actually wanted it closed rather than left
  as a documented gap (see the next entry).
- **A "documented simplification" is still worth asking about before
  assuming it's acceptable - the user may have meant "fix it," not
  "acknowledge it."** The December/January boundary gap above was
  deliberately left open with a comment explaining why; the very next ask
  was "make sure checks across boundaries... I don't want that problem."
  Closing it needed a real (if small) design decision: December's and
  January's placements are mutually exclusive but neither is inherently
  "first," so the resolution is to always let the chronologically earlier
  year win - process years in increasing order and thread forward a single
  rolling fact (`previous_december_claimed: bool`) from `year_schedule_one_
  year`'s December outcome into the next year's own scheduling call.
  Renamed the old single-year function to `year_schedule_one_year` and
  gave it that new parameter; the public `year_schedule(seed, year)` now
  walks forward from year `0`, discarding every prior year's full
  schedule and keeping only that one boolean - `O(year)` tiny 12-month
  passes per call (recomputed fresh every frame in `update_sky`, like
  everything else here), not `O(year)` of retained state. This stays
  negligible for a *very* long-lived save (one in-game year is 48 real
  hours at 30 minutes per in-game day, so `year` climbing into the
  thousands would take a real user years of continuous play) - re-ran the
  same "sweep many (seed, year) pairs, count shortfalls" throwaway-test
  technique from the entry above (this time also varying `year` up to 20,
  since a stricter cross-year constraint could plausibly starve the pool
  differently than the within-year-only version did) before trusting the
  stricter constraint still hits its exact occurrence counts, saw zero
  shortfalls across 6,000 pairs, then deleted the sweep and kept a small
  permanent regression test
  (`no_special_lands_in_january_right_after_a_claimed_december`) instead.
- **An `O(year)` cost the user explicitly said would "never realistically
  be reached" was still worth fixing once asked "just for theoretical" -
  but the fix should exploit the actual access pattern, not add a generic
  cache.** `year_schedule` walking forward from year `0` every call is
  fine for a one-off, but `update_sky` calls it every frame; the naive fix
  (memoize by `(seed, year)` key, LRU-evict, etc.) would be real new
  complexity for a problem with a much simpler shape once you notice how
  the caller actually uses it: `DayNightClock::year()` only ever advances
  by exactly one at a time during normal play (one in-game year = 48 real
  hours at this cycle's pacing), so the "cache" only ever needs to
  remember *one* thing - the immediately preceding year's December outcome
  - to turn the common case into an `O(1)` incremental step via the same
  `year_schedule_one_year(seed, year, previous_december_claimed)` building
  block `year_schedule` itself already uses internally. `MoonScheduleCache`
  is a single `Option<(seed, year, schedule, december_claimed)>`, not a
  map: an exact match returns the cached schedule directly, `year + 1`
  triggers the one-step incremental path, and *anything* else (a fresh
  world, a different seed, a save loaded straight into an arbitrary year)
  falls back to the plain `year_schedule` - exactly as expensive as
  before this cache existed, but now only paid once for that one jump
  instead of every frame after it. **Splitting "which schedule applies"
  from "what does this month's schedule mean" made the caching layer
  purely additive** - `DayNightClock::moon_event_in(&self, schedule)` is
  the same lookup `moon_event` always did, just taking the schedule as a
  parameter instead of computing it inline, so every existing test
  (written against the old `moon_event(seed)`, kept as a thin wrapper
  around `moon_event_in`) needed zero changes; only `update_sky` itself
  had to switch call sites, to `cached_year_schedule(&mut cache, seed,
  clock.year())` feeding `moon_event_in`. Verify a caching layer against
  its own uncached reference implementation directly, not just "the game
  still looks right" - `cached_year_schedule_always_agrees_with_the_
  uncached_reference` walks a fresh cache year-by-year (including
  re-querying already-cached years) and asserts every single step matches
  `year_schedule` called fresh, since a caching bug that silently returns
  a *plausible-looking but wrong* schedule is exactly the kind of thing
  that wouldn't fail loudly.
- **When asked to fix "the texture crash" after the moon-events breakdown
  flagged sun/moon specifically, the fix covered `atlas.rs`'s block/UI
  tiles too, not just `sky.rs`.** Both had the *exact same* shape of bug
  (`load_custom_tile`/`load_custom_sky_texture` panicking on a malformed
  custom PNG - disallowed size, unreadable/corrupt file), and leaving one
  fixed while the other still crashed would have directly contradicted the
  user's stated goal in the very message that triggered this ("I don't
  want anything to be able to break"). Fixing only the literally-named
  half of a symmetric problem because that's the half that was asked about
  is a trap worth watching for - when two subsystems share a bug's shape,
  fix both and say so, rather than waiting to be asked twice.
- **A malformed custom texture now degrades to a placeholder instead of
  panicking, via a genuine tri-state return type, not `Option` plus a
  swallowed error.** `atlas::CustomTile`/`sky::CustomSkyTexture` are
  `Absent` (no file - use the procedural default, unchanged), `Malformed`
  (file exists but failed to decode/validate - use the placeholder, *not*
  this name's real painter, so a broken custom file stays visibly wrong
  instead of quietly rendering normal-looking art), and `Loaded(pixels,
  size)`. Collapsing `Malformed` into `Absent` (both "just use `None`")
  would have silently hidden a real user mistake behind normal-looking
  output - the whole point of a *distinct* placeholder color is that it
  has to stay distinguishable from "intentionally not customized."
  `atlas.rs` reuses one `painted_tile(paint_fn, name, tile_size)` helper
  for both a name's real painter (the `Absent` case) and the placeholder
  painter (the `Malformed` case) rather than duplicating the
  scratch-buffer-then-upscale dance twice.
- **Reporting "how many textures are broken" needs a way to tell "no
  custom art, using the block's own intended procedural painter" (fine)
  apart from "nobody ever registered a painter for this name, `ensure_
  registered` silently filled the gap with the placeholder" (not fine) -
  even though `build_atlas_from_dir` calls the exact same `paint` closure
  either way by the time it runs.** By the time the atlas is built, a
  name that got `ensure_registered`'s auto-placeholder and a name with a
  genuinely hand-written painter are indistinguishable from the *call
  site* - both are just "call this closure." The fix was tracking the
  distinction at the moment it's still knowable: `Painters` gained a
  `placeholders: HashSet<String>` field, populated only inside
  `ensure_registered` itself, checked when `build_atlas_from_dir` decides
  each `Absent`-case tile's status. General lesson: when two code paths
  converge to the same shape before the point where you need to
  distinguish them, the distinguishing fact has to be captured *at the
  point they diverge*, not reconstructed later from data that no longer
  carries it.
- **`/texture-report`'s red ("completely broken") tier is a real computed
  invariant check, not a hardcoded zero used to look reassuring.** Once
  every malformed-file crash path is fixed, there's genuinely no reachable
  "nothing renders at all" state left - so red *should* always read `0`.
  Rather than special-casing that as a constant (which would silently lie
  if some future change broke the guarantee), `world::compile_content`
  diffs `BlockRegistry::texture_names()` against the atlas's own built
  `indices` after the fact and reports any name that didn't make it in -
  currently always empty because `ensure_registered` runs for every
  required name first, but this recomputes that fact fresh every startup
  instead of assuming it holds forever. This is the same "give the user
  a redundancy check they can actually trust" instinct as `MoonScheduleCache`
  falling back to the plain computation on any mismatch, or the
  `cached_year_schedule_always_agrees_with_the_uncached_reference` test -
  don't just assert an invariant once in a comment, give the running game
  a cheap way to keep proving it.
- **One shared inline-color-marker mechanism (`text_color.rs`) serves both
  a system-generated report and free-form player chat, because `ChatLog::
  push` is the single place any text - typed or generated - enters the
  scrollback.** `/texture-report` builds its counts with `text_color::
  colorize(text, hex)` (wraps text in `~(#hex)~...~(#hex)~`); a player can
  type the exact same syntax by hand. Neither has a separate rendering
  path - `ChatLog::push` parses every message once via `parse_colored_
  segments`, and `chat::sync_chat_ui` renders whatever segments came out.
  **The marker is a toggle, not a matched pair**: the first `~(#hex)~`
  starts a colored run using that hex, and the next one ends it
  *regardless of what hex digits it contains* - deliberately, so a player
  mistyping one digit in the closing marker still closes the span instead
  of coloring the rest of their message by accident. Bevy's rich-text API
  for mixing colors within one text block is `TextSpan` child entities
  under a `Text` root (see `TextSpan`'s own doc example: "children must be
  `TextSpan`, not `Text`") - `sync_chat_ui` puts segment 0 directly on the
  existing `ChatLogText` root entity and respawns the rest as `TextSpan`
  children each frame (`despawn_related::<Children>()` then
  `with_children`, the same rebuild-the-subtree pattern as `ui::
  rebuild_hotbar`), which is simple enough at `VISIBLE_MESSAGES`-line
  scale that diffing instead of respawning isn't worth the complexity.
- **`OnEnter(AppState::InGame)` cursor-grab systems need an `OnExit` mirror,
  or leaving the state leaves the cursor exactly as the last frame in that
  state left it.** `player::enter_game_grab` locks and hides the cursor the
  moment a world loads; nothing undid that when leaving back to the main
  menu (`MenuButton::BackToMainMenu`/the pause screen's quit path both just
  `next_state.set(AppState::MainMenu)`), so the menu - plain point-and-click
  UI - was left with an invisible, locked cursor and looked unresponsive to
  the mouse. Fixed with `player::exit_game_release_cursor` on
  `OnExit(AppState::InGame)`, the direct mirror of `enter_game_grab`. General
  lesson: an `OnEnter` system that mutates shared, persistent state (window
  settings, not a per-world resource that naturally resets) needs a
  same-plugin `OnExit` counterpart considered explicitly, not assumed to be
  someone else's problem just because the mutation happened in a different
  plugin than the state transition trigger.
- **A "select all" in a single-line input with no real cursor/selection
  range doesn't need one built to make Ctrl+A/C/V work - a whole-input-or-
  nothing boolean is enough.** `chat::ChatState::selected` (set by Ctrl+A)
  is consulted by exactly three places: Ctrl+C only copies when it's set
  (matches how a real text field does nothing on Ctrl+C with no selection,
  rather than always copying and giving Ctrl+A no real effect), Backspace
  clears the whole input instead of popping one character while it's set,
  and typing (or Ctrl+V) replaces the whole input instead of appending.
  Every one of those also clears the flag afterward, so a stale "everything
  selected" from an earlier Ctrl+A never lingers into an unrelated later
  edit. This is deliberately *not* a real selection range (no anchor/cursor
  positions, no shift+arrow, no click-drag) - the existing input widget has
  no cursor position concept at all (append-only, Backspace always pops
  from the end), and building one just for Ctrl+A support would be a much
  bigger feature than what was actually asked for.
- **OS clipboard access needs a real crate (`arboard`) - Bevy/winit doesn't
  expose one.** Added with `default-features = false` (skips arboard's
  optional `image-data` feature, which pulls in `image`'s own clipboard-
  image support that nothing here needs - text-only get/set is always
  available regardless of that feature). `Clipboard::new()` can fail (no
  display/clipboard server - e.g. a headless CI run), so `chat::
  clipboard_copy`/`clipboard_paste` both treat that as a silent no-op via
  `.ok()`/`if let Ok(..)` rather than unwrapping, matching this project's
  established "never crash on an external environment failure" stance
  (same instinct as the updater's self-replace guard and the texture-
  loading placeholder fallback).
- **A command-open hotkey (`/`) is a second *trigger* for the same open
  path, not a second input box.** `chat::toggle_chat` already checked one
  key (`T`); adding `/` was extending the same trigger check
  (`keys.just_pressed(KeyCode::Slash)` alongside the existing `KeyT` check)
  and, only when it was the slash that opened it, pre-filling `chat.input`
  with `"/"` before the box appears - not a parallel code path. The
  existing "swallow this frame's pending KeyboardInput events so the key
  that opened the box doesn't also get typed" logic (`chat.just_opened`)
  needed no changes at all, since it already clears *all* pending events
  for that frame regardless of which key triggered the open.
- **A relaxation that reports "changed" when its write silently didn't land
  is an unbounded work generator, and it looks exactly like a convergence
  bug.** `light.rs`'s propagation seeds cells one block *outside* a chunk
  (the far side of each seam), and those can belong to a chunk that isn't
  loaded yet. `ChunkMap::set_light` originally returned `()` and no-opped
  for a missing chunk, so `recompute_light_cell` computed a new value, saw
  `next != current`, and enqueued all 6 neighbours - every single visit,
  forever, spreading outward through terrain that doesn't exist. The queue
  grew by a steady ~24k cells/frame and never drained. Fix: `set_light`
  returns `bool` like `set_fluid_cell` already did, and the "it changed"
  path is gated on the write actually landing. **The general rule: any
  setter a propagating simulation drives has to report whether it wrote,
  because "did this change?" is what decides whether to schedule more
  work** - a silent no-op turns that question into a permanent yes.
- **"Every settled cell enqueues neighbours it could improve" (a push rule)
  does not terminate, even though each individual value is correct.**
  Written as the mirror of the pull rule it looks obviously right, and the
  *values* it produces are right - but it fires on every visit of every lit
  cell, and since a cell gets visited once per neighbour that changed, the
  queue regenerates work faster than the budget drains it. Measured: 20M+
  pops on a 2197-cell room that should settle in a few thousand, with every
  sampled pop reporting no change. What light removal actually needs is far
  narrower: when `relax` empties a channel rather than downgrading it, that
  cell re-queues *itself* (one entry, only on an actual drop, and each drop
  lands it on a strictly lower value so there can only be `MAX_LIGHT` of
  them). Same effect as the two-pass "clear the region then refill from
  whatever still has a real supply" BFS voxel engines hand-write, without a
  second queue. The narrow-vs-general lesson generalizes: in a budgeted
  queue, a rule that can fire on *unchanged* cells is a red flag, because
  the queue's only termination argument is "work is proportional to
  changes."
- **A percentage-shaped knob needs a storage scale fine enough to hold it -
  otherwise the schema silently becomes a switch.** Adding per-block light
  `transmission` ("glass passes 98%, water 65% of red") looked like it only
  needed a new field, but the light scale was `0..=16`, so 2% of a full
  level rounded to zero: a hundred panes of glass would have blocked
  *nothing*, and anything that did round up to 1 was indistinguishable from
  dense leaves. Two scales now exist deliberately - `MAX_LEVEL` (16) is what
  `blocks/*.json` authors emission in and still equals a distance in blocks,
  `MAX_LIGHT` (255) is what's stored, at 1/16th-of-a-level resolution, with
  `LEVEL_STEP` (16) the per-block falloff so the *range* is unchanged.
  Keeping authoring in levels while storing in finer units meant making
  light more precise invalidated zero existing block files. The general
  shape: when a new field is a fraction/percentage, check what it multiplies
  against before designing the field, because the quantisation of the
  *target* decides whether the field can express anything at all.
- **Transmission is multiplicative, not a subtracted number of levels,
  because that's what makes stacking compose.** Light crossing glass and
  then water is scaled by each in turn, so the pair genuinely differs from
  either alone (the case that motivated the feature). It's per-channel
  `[r,g,b]` so a medium can absorb colors *unequally* - water eats red and
  keeps blue, so a pool floor reads blue-green at noon rather than merely
  darker. `attenuate` returns its input untouched for a transmission of 255,
  so a clear medium is an exact no-op and every pre-existing lighting
  expectation is unchanged - same "make the general formula reduce to the
  old behavior" shape as `mesher.rs`'s `rotated_tile`. Opaque blocks resolve
  to `[0;3]` in `Tables::transmission` at compile time, so nothing downstream
  has to consult two fields that could disagree.
- **Sky light needed three channels to be *tintable*, and the third and
  fourth floats came from an unused attribute rather than a new one.** A
  single stored sky value can only ever be scaled, i.e. made darker - it
  cannot be made bluer, which is exactly what light through water has to be.
  `LightCell::sky` is `[u8;3]` now (cell storage 4 -> 6 bytes). The vertex
  side needed three interpolated floats where there was one: red stays in the
  vertex color's alpha, green and blue ride in `Mesh::ATTRIBUTE_UV_1`
  (`MeshBucket::sky_gb`), which chunks don't otherwise use since they sample
  a single atlas. Inserting that attribute is *also* what makes Bevy define
  `VERTEX_UVS_B` and pass `uv_b` through to the fragment stage, so this
  needed no custom vertex shader and no `specialize` change - worth
  preferring precisely because a rendering mistake can't be seen from a
  remote session with no display. **Do not be tempted to bit-pack three
  channels into the one spare float**: vertex attributes are interpolated
  across each triangle, and interpolating packed integers produces garbage
  between vertices. Each channel needs its own float.
- **When a lighting change breaks a test, check whether the test's *scene*
  is what changed meaning.** Two headless tests started failing with values
  like `[166, 219, 242]` - which is exactly water's transmission applied to
  full sky. `ChunkMap::surface_y` returns the topmost **solid** block and
  water isn't solid, so `surface_y + 1` is a *water* cell on any lake
  column; the tests had always been probing water and it simply hadn't
  mattered until media started attenuating light. The fix was a
  `open_air_above_ground` helper that scans for a genuinely dry column
  rather than trusting a hardcoded one, not touching the production code.
- **CLAUDE.md's own "measure before assuming an infinite loop" rule paid off
  twice here, in opposite directions.** The first light failure looked like
  slowness (raise the guard?) and was a true non-termination; the fix for
  it looked like a correctness bug and was also non-termination, from a
  completely different cause. Both were found the same cheap way - print
  the pop count, then dump which positions repeat and what they transition
  to - and neither would have been found by reading the code, because in
  both cases every individual value being computed was correct. When a
  propagating sim misbehaves, instrument *what repeats*, not *what's
  wrong*.
- **A "does light stop at a wall" test needs a corridor, not a room.** The
  first version put a single stone block next to a torch in an open carved
  box and asserted the far side was dark; it was lit, because light
  correctly travels *around* one block. The production code was right and
  the test was wrong - the same intuition-first trap as the moon-terminator
  test. Bore a one-cell-wide tunnel so a single block genuinely seals it.
- **Separating "how much sky reaches this cell" from "what the sky
  currently looks like" is what makes a day/night cycle free.** The stored
  sky-light level describes the world's *shape* and only changes when
  blocks do; `render::ChunkMaterialParams::sky_light` (now an RGB value,
  not the old brightness scalar) describes the sky *right now* and is
  rewritten every frame by `sky::update_sky`. Dawn, dusk, and a red/blue/
  green moon tinting every outdoor surface therefore cost one uniform
  write - no relighting, no remeshing. If they'd been baked together into
  one "final brightness" per cell, every sunrise would have had to relight
  and remesh the visible world. Worth applying to any future
  per-cell-value-times-global-state pair (weather, seasons affecting
  ground color, etc.): store the part that changes with the world, uniform
  the part that changes with time.
- **The mesher's vertex alpha channel was free real estate, and that's a
  fact worth checking before adding a vertex attribute.** Chunk fragments
  get their alpha from the texture and `base_alpha`, so `in.color.a` was
  being written (`1.0`) and never read. Sky light now rides there while
  block light rides in RGB, which is what lets the shader combine them per
  frame - no new attribute, no bigger vertex buffer, no mesh format change.
  Grep for what actually *reads* an existing channel before growing the
  vertex layout.
- **Four parallel arrays that must always be indexed together are a struct,
  not four positional parameters.** `mesh_chunk(padded, padded_fluid,
  padded_axis, tables)` was already at the edge; adding light would have
  made it five positional slices with nothing stopping a caller swapping
  two of the same type. `mesher::PaddedChunk` (blocks/fluid/axis/light, plus
  an `empty()` constructor the tests build on) made `build_padded`'s return
  type and the mesher's signature both simpler than before the feature.
- **The fix for "our updates keep breaking" was to stop having the running
  program update itself, not to make the self-replace more careful.** Two
  previous rounds went into hardening the in-game updater - deferring the
  swap to quit time, adding a visible dwell, reading the vendored
  `self_replace` source to confirm the rename dance was correct - and it
  was all correct, and it still didn't work reliably, because a process
  silently rewriting its own binary is something the OS and its security
  software get a vote on. The launcher wins by removing the premise: the
  process doing the downloading (the launcher) is never the process being
  replaced (a game build under `versions/`), so a version install is just
  writing a new folder. When a mechanism keeps failing after the third
  careful fix, check whether the mechanism itself is the thing to delete.
- **Deciding a launcher's instances share one saves folder is a decision
  about *save compatibility*, not about folders.** Isolated per-instance
  saves would have needed no format work at all; sharing them makes both
  directions of version skew reachable in ordinary use, which is what
  forced `save::SAVE_FORMAT`, `SaveCompatibility`, `migrate` and
  `SaveStore::open_world`. The dangerous direction is the *backwards* one -
  an older build opening a newer world would load it (every field is
  `#[serde(default)]`, so it deserializes fine) and then write it back out
  minus everything the newer build added. That's silent data loss that
  looks like a successful load, so it's refused rather than warned about,
  and the worlds list greys the world out instead of hiding it (a hidden
  world reads as "my save is gone").
- **Build the migration mechanism at the moment the *first* migration
  becomes possible, not when it becomes necessary.** There are currently
  zero data-shape migrations - format 0 -> 1 rewrites nothing, because
  serde defaults already covered every field added so far. Writing the
  table-plus-loop anyway costs a few lines now; improvising it later, at
  the point where a real format change has already shipped and someone's
  world is the test case, costs a lot more. Same instinct as
  `MoonEventDef`'s declarative table: the generic algorithm goes in while
  it's cheap and obvious.
- **`cargo test` in a workspace with a root package does NOT test the
  workspace.** Converting the repo to a workspace silently changed what
  "run the tests" means - the launcher's 40 tests don't run under a bare
  `cargo test`. Nothing warns about this. Use `--workspace` (and check that
  CI/docs say so) whenever adding a crate.
- **A launcher's "is it installed?" check should read the filesystem, not
  an index file.** `Library::is_installed` is just "does
  `versions/<version>/craftmjne[.exe]` exist" - so deleting a folder by
  hand really does uninstall that version, and there's no catalogue that
  can disagree with reality. The matching hazard is a *partial* extraction
  leaving the executable in place: that would read as installed and then
  crash on the missing `blocks/`. Fixed by extracting to a scratch
  directory and renaming into place only on success, so the check can
  never observe a half-written version (`remote::install`, tested by
  pointing an install at an unreachable URL and asserting no directory is
  left behind).
- **Test temp directories must be unique per call, not per fixture.** Two
  launcher tests both built `craftmjne-launch-test-<pid>-1.0.0`, ran in
  parallel, and one's `TempRoot` drop deleted the other's fixture mid-test
  - which surfaced as an unrelated-looking "isn't downloaded yet" failure
  that only appeared sometimes. Every temp-dir helper in this repo uses an
  `AtomicU64` counter for exactly this reason; copy that, don't key the
  name on the fixture's contents.
- **"The mechanism is correct and tested" doesn't mean "there's nothing to
  fix" - a screenshot can reveal the mechanism was answering the wrong
  question.** The day/night investigation above concluded the code already
  did what was asked (dawn = `elapsed 0.0`, correctly persisted) and very
  nearly stopped there. What actually landed once the user sent a real
  screenshot: a brand-new world's near-black first frame is a real UX
  problem even though the *data* is correct, and it's a genuinely different
  complaint than "the sun's position is wrong." `sky::NEW_WORLD_START_TIME`
  (`DAY_SECONDS * 0.2` - sun 36° up, `daylight() ≈ 0.59`) is what a
  brand-new world starts at now, instead of the literal dawn instant.
  Getting only a *new* world this treatment (not an existing save that
  merely predates the `time_of_day` field) needed a real distinction
  `save::SaveStore::world_data_exists` didn't have a reason to draw before -
  both cases used to reach the same `WorldData::default()` fallback by
  coincidence, so "does `data.json` exist at all" had to become an explicit
  check in `world::enter_world`, not just a different default value. Two
  tests guard the split: one confirms the new starting value, a second
  (`an_existing_save_still_resumes_at_literal_dawn_not_a_bright_morning`)
  manufactures an already-saved world and confirms it does *not* get the
  new-world treatment - verified both actually fail without the
  `world_data_exists` guard before trusting them, same discipline as every
  other regression test in this file.
- **"Enable dev builds" needed a rolling GitHub Release, not a new
  distribution mechanism, and the game version machinery didn't need to
  know dev builds exist at all.** Asked for a launcher toggle that installs
  whatever's on `main` without cutting a real `vX.Y.Z` release each time,
  the shape that reuses the most existing, working code: `dev-build.yml`
  builds on every push to `main` and replaces one fixed `dev`-tagged
  GitHub Release (`gh release delete dev --yes --cleanup-tag || true` then
  `gh release create dev ... --target "$GITHUB_SHA"` - delete-then-recreate
  is what actually moves the tag, not an in-place update), reusing the
  exact staging/packaging steps `release.yml` already has proven working.
  `remote::fetch_dev_build` finds that one release by its known tag,
  `remote::install` (already generic over "a URL and a destination
  directory") installs it into `versions/dev/` exactly like a tagged
  version installs into `versions/1.3.0/` - `instances.rs`/`launch.rs`
  needed zero changes, since an `Instance.version` was already just a
  directory-name string with no assumption it came from a real release.
  The one genuinely new piece: `self_update::Release` carries no per-build
  identity for something that isn't semver-tagged, so "is the installed
  dev build stale" needed a real answer - `dev-manifest.json` (`{"commit":
  "<sha>"}`), published as an extra release asset alongside the platform
  archives, is what the commit travels in; `Library::dev_commit`/
  `record_dev_commit` are the launcher's own bookkeeping for what's
  actually on disk (a small sidecar file in `versions/dev/`, the one
  deliberate exception to this module's "no index file, the directory
  *is* the record" rule - the commit isn't something a directory listing
  can ever answer on its own).
- **A GitHub Actions release job needs to filter by asset name, not assume
  "this repo's releases are all mine to list."** Reported as "the launcher
  shows its own version in the game's version list": `RemoteVersion`'s
  fetch matched any release with an asset containing the target-triple
  substring, and a launcher release's asset name (`craftmjne-launcher-
  x86_64-...`) contains that substring too, same as a game release's does -
  `self_update`'s own `version` field (tag with a leading `v` stripped)
  doesn't filter this out either, since `"launcher-v1.0.2"` doesn't start
  with `v`. `dev` (this session's own new tag) would have hit the exact
  same bug. Fixed with `looks_like_a_game_version` (starts with a digit) -
  deliberately a property of what a real version string looks like, not a
  list of the other tag prefixes that happen to exist today, so a future
  tag this repo publishes for some other reason doesn't need this function
  edited to stay excluded.
- **A plain (unquoted) YAML `run:` scalar containing `": "` anywhere breaks
  the parser - `run: |` (a block scalar) is the safe default for any shell
  command with punctuation, not just multi-line ones.** `dev-build.yml`'s
  manifest-writing step was originally `run: echo "{\"commit\": \"${{
  github.sha }}\"}" > dist/dev-manifest.json` on one line - valid-looking
  bash, but YAML parses an unquoted scalar's `: ` as a mapping key
  separator wherever it appears, not just at the start, so this failed to
  parse at all rather than running wrong. Caught by actually validating the
  YAML (`python3 -c "import yaml; yaml.safe_load(...)"`) before trusting
  it, the same "verify by actually running it" instinct as everywhere else
  in this file - a workflow file with a syntax error doesn't even queue,
  so this would otherwise have surfaced as "nothing happened" on the very
  first real push, with no clue why from the change itself.
- **egui/eframe does not call `update` on a schedule of its own** - only in
  response to input, or an explicit `Context::request_repaint_after`. A
  periodic "check every N seconds while this is open" feature (the dev
  build panel re-checking for a newer commit once a minute) needs that
  call issued every frame it's still relevant, re-arming the next wake-up
  each time, the same way the existing download-progress spinner already
  had to (`if downloads.is_busy() { ctx.request_repaint_after(200ms) }`) -
  otherwise the timer field being past due is true in memory but nothing
  ever calls the code that checks it while the window sits idle with no
  input. **Order matters when the same tick both fires the action and
  re-arms the next wait**: computing `request_repaint_after`'s duration
  from `last_check.elapsed()` *before* an overdue check's own refresh
  resets that field schedules the next repaint using the stale
  (already-elapsed, i.e. ~zero) duration, which reschedules almost
  immediately instead of a minute out. Correct order is refresh-then-
  reschedule (`if due { refresh() }` - which resets the timestamp -
  `then` compute the repaint delay from the now-current field), not the
  other way around.
- **A running process spawning a hidden supervisor of itself to reappear
  later is exactly the shape antivirus heuristics are built to catch -
  don't reach for it even when the goal (auto-reopen after a child
  process exits) sounds like it needs one.** Asked to make the launcher
  close when a game instance starts and reopen when it closes, the two
  live options were: (a) exit the launcher and have some second process
  re-launch it once the game ends, or (b) keep the one launcher process
  alive, hide its window, and show it again. (a) needs a hand-off - the
  exiting process re-invoking itself with an internal flag, or a detached
  helper - which is the "process launches a hidden copy of itself to
  supervise, then reappears" pattern, structurally identical to what got
  flagged as suspicious in the old in-game updater's self-replace saga.
  (b) is `launcher/src/app.rs`'s `play`: `egui::ViewportCommand::
  Visible(false)`, a background thread `Child::wait()`s on the spawned
  game (blocking is fine off the UI thread), then `Context::
  request_repaint()` - documented as safe and expected to call from
  another thread specifically for this "wake a UI with no live input"
  case - delivers `JobDone::GameExited` through the same `jobs.rs`
  channel every other background op already uses, and `poll_jobs` sends
  `Visible(true)`. One process the whole time, no re-exec, no hand-off
  to get wrong, and specifically avoids adding a second thing that looks
  like malware to the exact AV heuristics this project is already fighting.
- **When something is flagged as suspicious by name (a "sketchy" console
  window, an AV signature warning) but the underlying mechanism turns out
  to be correct, check whether the input the mechanism is *reacting to* is
  actually current before assuming there's a bug to fix.** Asked to fix
  "the game starting at 1pm instead of dawn," `sky::DayNightClock`'s
  `elapsed: 0.0` is genuinely dawn (verified via its own doc comment, the
  `phase_angle`/`daylight` formulas, and a passing integration test
  literally named `time_of_day_and_moon_phase_persist_across_a_reload`
  asserting `elapsed == 0.0` for a fresh world) - and checking out the
  actual tagged `v1.2.4` release confirmed the identical logic was already
  there, not something only fixed on an unreleased branch. Don't "fix" a
  mechanism that's already correct and tested just because a symptom was
  reported against it; ask what's actually being observed instead, since a
  changed based on a guess at this point risks masking whatever the real
  cause is.
- **A version-metadata/code-signing change for a platform you can't compile
  for is exactly where "verify by actually compiling" (this file's own
  standing rule) has no cheap way to be followed - that's a reason to hold
  off, not a reason to skip verification and ship a guess.** Embedding
  Windows executable version info (`ProductName`/`FileDescription`/etc. via
  a `build.rs` and `winresource`) was a real, safe-in-principle improvement
  for the antivirus-flagging complaint, but this environment can only
  compile for Linux - a wrong method name or field type wouldn't surface
  until the next real Windows CI run, on a release the user is about to
  cut. Fetched the crate's docs first rather than trusting memory, and the
  summary still left the exact `.set()` vs `.set_version_info()` split
  ambiguous enough not to trust blind. Left it undone and said why, rather
  than landing a plausible-looking build.rs neither of us could check.
- **A staging step that cherry-picks individual files instead of copying a
  directory silently rots the moment new content is added to that
  directory.** `release.yml`'s "Stage game files" step copied only
  `textures/blocks/README.md` and `textures/sky/README.md` into the release
  archive - not the actual `.png` files. That was invisible for a long time
  because `textures/blocks/` held only a README when the step was written;
  once `dirt.png`/`stone.png` were added to the repo, every released build
  silently kept shipping the procedural placeholder art for every block,
  with the real textures sitting right there in the source tree the whole
  time and zero error or warning anywhere - `atlas.rs`'s own fallback logic
  (missing custom texture -> use the procedural painter) is *designed* to
  degrade silently, which is exactly right for a genuinely absent file and
  exactly wrong for a packaging bug hiding a file that does exist. Fixed by
  copying the whole `textures` directory (`cp -r textures "$stage/textures"`)
  instead of naming individual files, so anything dropped into
  `textures/blocks/` or `textures/sky/` in the future ships automatically -
  verified by running the exact staging commands locally and checking the
  resulting file list, not just reading the YAML and assuming it was right.
- **A "query everything registered, including future mods" feature needs a
  registry to query, not a second hardcoded list next to the first one.**
  Command autocomplete could have been built as its own name list inside
  `chat.rs`, kept in sync with `commands.rs`'s dispatch `match` by hand - the
  two would drift the first time either changed without the other.
  `commands::CommandRegistry` (mirroring `BlockRegistry`'s already-
  established shape: `with_defaults()` seeds the built-ins, `.register()` is
  the same call a mod's `build()` would make) is the single source both
  `execute` and `chat.rs`'s dropdown read from - a command is discoverable
  in the dropdown *because* it's real, invocable data, not a separately
  maintained fact about it. The cheats flag moved from `execute`'s call site
  into `CommandRegistry::execute` itself for the same reason: a mod's
  command needs to trip it too, and putting the check in every handler would
  be exactly the kind of fact a mod author could forget to include.
- **A test whose input coincidentally satisfies the guard some *other* way
  isn't testing the guard.** The chat autocomplete dropdown stops offering
  suggestions once a space appears (composing an argument, not still typing
  the command name) - `command_suggestions`' `rest.contains(char::
  is_whitespace)` check. The first test written for this used `"/mode c"`/
  `"/mode "` as inputs and passed - but deleting the guard entirely left it
  passing too, because no built-in command name contains a space, so
  `starts_with`'s own length check already rejects any prefix longer than a
  real name regardless of the guard. The guard only has an observable effect
  when some registered name's own text would otherwise still prefix-match
  past the space - reproduced with a test command literally named `"big
  heal"`, where `"big "` genuinely is a valid prefix of it and only the
  explicit whitespace check stops it being suggested. Same lesson as
  CLAUDE.md's other regression-test entries, generalized: before trusting a
  new test, break the thing it claims to guard and confirm the test actually
  turns red - a test that stays green either way isn't a regression test,
  it's a coincidence with good intentions.
- **The launcher exposed a real packaging bug that had been latent for
  months: the Windows release zip contained only `craftmjne.exe`.** That was
  fine while the only consumer was the old in-game updater, which extracted
  just the named binary out of it - but the archive is now what a fresh
  install *is*, and the game refuses to start without `blocks/` beside its
  executable. `release.yml` now stages executable + `blocks/` + `textures/`
  into one folder and archives that folder's *contents* (note the `\*` in
  the `Compress-Archive` path - archiving the folder itself would nest
  everything one level down and break it just as thoroughly). General
  lesson: when something starts consuming an artifact differently than the
  thing it was built for, re-check what's actually in the artifact.
- **Biome grass tint reuses the exact "repurpose a free vertex attribute"
  trick sky light already established, one level further.** The vertex
  color/UV1 budget was already fully spent (RGB=block light, A+UV1.xy=sky
  light's three channels), so a third per-face signal - biome color, so
  `grass_top.png`/`grass_side.png` can be plain grayscale masks instead of
  flat-green art - went into `Mesh::ATTRIBUTE_NORMAL` (`mesher::MeshBucket::
  tint`, `chunk.wgsl` multiplies `in.world_normal` straight into the lit
  color). Safe for the same reason `ATTRIBUTE_UV_1` was safe: chunks are
  unlit and spawned with a pure-translation `Transform`, so nothing reads
  `world_normal` for real lighting and the identity normal-matrix leaves
  the smuggled color untouched. `[1.0, 1.0, 1.0]` (a multiply no-op) is
  baked for every untinted face, so this needed zero changes to any
  existing block or test. **Which faces get tinted is a property of the
  (block, face) slot, not the texture name** - `FaceTint`/`Tables::tinted`
  mirror `FaceTextures`/`Tables::tiles`'s exact `[id*6+face]` shape, because
  grass's bottom face reuses `dirt.png` (the very tile plain `dirt` blocks
  render with) and must stay untinted - a per-block flag would have
  incorrectly tinted every dirt block too. Tint color itself
  (`biome::grass_tint`) is a pure function of `(world_seed, x, z)` via a
  dedicated `SimplexNoise` stream (`biome::noise_for_seed`, `seed ^
  SEED_OFFSET` like every other decorrelated stream), computed fresh every
  mesh - deliberately *not* stored per-chunk or persisted, same "nothing to
  invalidate, a reload recomputes the same answer" reasoning as `light.rs`.
  `mesh_chunk` gained an explicit `chunk_origin: (i32, i32)` parameter
  (world-space column of the chunk's local `(0,0)`) so adjacent chunks
  sample the same continuous noise field instead of each restarting at
  local `(0,0)`, which would show as a tint seam at every chunk boundary.
  The actual `textures/blocks/grass_top.png`/`grass_side.png` files are now
  the grayscale masks (swapped in via `git mv`, since `texture_scheme:
  organic` always looks for those exact names) - the flat-color versions an
  earlier session wired in first got moved to `grass_top_color.png`/
  `grass_side_color.png`, unread by anything but kept for a future block
  that wants plain grass-style art without going through tinting.
