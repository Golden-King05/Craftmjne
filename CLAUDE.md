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

## Two working branches: minor updates and major updates

As of 2.0.0 the user wants two Claude branches so a minor release and the
next major one can be developed at the same time (they merge each to `main`
and tag by hand when ready):

- **`claude/minor-updates`** - the sub-releases of the current major version
  (2.0.x: texturing, then modeling/sticks/pebbles). Small, shippable changes.
- **`claude/major-updates`** - the update *after* those sub-releases. Nothing
  here ships until the minor line is out.

**Ask which branch a task belongs to if it isn't obvious**, and push only to
that one. `claude/minecraft-clone-framework-2vjlng` is the old shared branch
(2.0.0 lives there); don't push new work to it unless told. When a minor
release lands on `main`, merge `main` into `claude/major-updates` so the
major line stays current. Each session start: `git fetch origin <branch> &&
git checkout -B <branch> origin/<branch>` for the branch being used.

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
  overlay N+1: `grep -rn "chat.open\|paused.open\|inventory.open\|death.open"` across
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
  the smuggled color's *hue* untouched. **Its length is not untouched**
  (found later, while drawing dropped items): Bevy's vertex shader
  normalizes `world_normal` (`mesh_normal_local_to_world`, checked in the
  vendored `bevy_pbr-0.16.1` source). So the `[1.0, 1.0, 1.0]` baked for
  untinted faces arrives as `[0.577; 3]` - every untinted surface has
  always rendered at 58% of the intended brightness - and a grass tint
  arrives at unit length, so its own brightness is thrown away (the
  colors tuned by eye were tuned against that). **Fixed at the user's
  request:** `chunk.wgsl` multiplies the normalized tint by `sqrt(3)`
  (so `[1, 1, 1]` is white again), and the mesher folds a tint's own
  brightness into the vertex light (`mesher::tint_brightness`), which the
  shader multiplies in the same product. Everything untinted got 1.73x
  brighter, i.e. textures now show their true colors at noon; `EXPOSURE`
  in `chunk.wgsl` is the one knob if that reads too bright. The grass
  constants (`biome::DRY`/`LUSH`/`COOL`) were rewritten to the normalized
  values that had actually been on screen, so the approved grass color
  didn't move. **Which faces get tinted is a property of the
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
- **A Windows-only feature that can't compile here doesn't have to stay
  unverified - installing a cross-compile target plus a real linker plus
  Wine turned "reviewed by eye" into "actually ran."** The launcher's
  "Add Desktop/Start Menu shortcut" buttons (`launcher/src/shortcut.rs`)
  needed `windows-sys`'s `SHGetKnownFolderPath`/`CoTaskMemFree` (resolving
  the real Shell folders rather than guessing `%USERPROFILE%\Desktop`,
  which breaks under OneDrive Known Folder Move - the same reason NSIS's
  own `$DESKTOP`/`$SMPROGRAMS` don't hardcode a path either) and a crate
  (`mslnk`) to write the actual `.lnk` binary format. This machine can only
  natively run `x86_64-unknown-linux-gnu`, but `rustup target add
  x86_64-pc-windows-gnu` plus `apt-get install mingw-w64` got a real `cargo
  build --target x86_64-pc-windows-gnu --tests` producing a genuine Windows
  PE test binary, and `apt-get install wine64` then actually *ran* it -
  `SHGetKnownFolderPath` resolving both known folders, `mslnk` writing real
  `.lnk` files, and (checked once via a temporary test, then deleted before
  committing) the real files landing at
  `~/.wine/.../Desktop/*.lnk` and `.../Start Menu/Programs/Craftmjne/*.lnk`,
  matching `installer/craftmjne.nsi`'s own two shortcut locations exactly.
  Only bother with this when a target is realistically installable
  (`rustup target list` showed multiple `windows` triples available) -
  this project's earlier "embed exe version info" idea was correctly left
  undone specifically because no such verification path existed for it.
- **`mslnk` looked like a portable, cross-platform crate from its docs.rs
  page (dependencies are just bitflags/byteorder/log, and the visible
  source excerpt showed no `#[cfg(windows)]` gates) - it isn't.** It calls
  `std::os::windows::ffi::OsStrExt` directly, which doesn't exist outside
  `cfg(windows)` in `std` itself, so it fails to even compile on Linux.
  Caught immediately by actually trying `cargo check` rather than trusting
  the fetched summary, which is exactly the kind of gap a WebFetch summary
  of a docs page can hide (it only shows what happened to be excerpted).
  Fixed by moving the dependency into `[target.'cfg(windows)'.dependencies]`
  alongside `windows-sys`, and gating the whole `shortcut` module on
  `#[cfg(windows)]` at its `mod` declaration in `main.rs` rather than
  giving it an internal non-Windows stub - once `app.rs`'s own "Add
  shortcut" buttons are `#[cfg(windows)]`-gated too (there's no Desktop/
  Start Menu concept to offer elsewhere), nothing on another platform ever
  calls into the module, so a stub inside it would be permanently dead
  code rather than a real fallback path.
- **A "grayscale tint mask" bug report turned out to be two different, real
  mismatches between what the art files actually contained and what the
  rendering code assumed - found by decoding the actual PNG pixel bytes,
  not by guessing from the visual symptom.** Reported as grass "not using
  textures and tinting whole block": `grass_side.png` turned out to be
  ~60% fully transparent (alpha 0) except a thin grass-colored fringe at
  the top - an *overlay* asset (matching real Minecraft's own
  `grass_block_side` + `..._overlay` split), not a full-face mask - so
  treating it as the whole face's texture meant most of it got discarded
  by `chunk.wgsl`'s alpha cutout with nothing opaque behind it.
  `grass_top.png` (a real full-face mask, no transparency problem) had a
  separate issue: its grayscale values only spanned 53-79 out of 255, not
  the bright near-white range a "multiply by tint" scheme assumes, so it
  read muddy and dark once tinted. Two independent fixes, chosen to adapt
  to whatever art gets dropped in rather than demand artists re-export
  differently:
  - **Overlay compositing**: `BlockDef::overlay` (a `FaceTextures`,
    mirroring `textures`'s own per-face shape) names an *extra*, always-
    tinted decal texture layered on top of a face's own plain (untinted)
    texture, instead of tinting that face's texture directly. `Tables::
    has_overlay`/`overlay_tile` mirror `tinted`/`tiles`'s exact `[id*6+
    face]` indexing. `mesher.rs` renders it as a genuinely separate second
    quad - not a shader-side two-texture blend - because there's no spare
    vertex attribute left for a second UV (the budget was already fully
    spent: RGB=block light, A+UV1=sky light, NORMAL=biome tint), and
    because a decal quad reuses 100% of the existing single-UV pipeline.
    The overlay quad is nudged outward along the face normal
    (`OVERLAY_DECAL_BIAS`, the mirror image of the existing
    `COINCIDENT_FACE_BIAS` z-fight fix - same problem, opposite direction:
    coincident quads still need separating even when one is *meant* to sit
    in front of the other) so its transparent majority shows the untinted
    base through it instead of a hole. `blocks/grass.json`'s side faces
    now set `"textures": {"side": "dirt"}` (the base) plus `"overlay":
    {"side": "grass_side"}` (the decal); its top face has no transparency
    problem to solve, so it keeps the simpler direct `"tinted": {"top":
    true}` from the previous session. The per-face AO/light/UV-padding
    math is identical either way, so the mesher's per-face corner loop was
    restructured once into a `push_quad(tile, tint, bias)` closure over
    precomputed per-corner data, called once for the base and, when
    `has_overlay` is set, again for the decal - so the two quads can never
    disagree about AO, lighting, or triangle winding, only about which
    tile/tint/plane they use.
  - **Brightness normalization**: `atlas::normalize_tint_mask_tile`
    contrast-stretches an already-baked atlas tile's RGB channels (min/max
    of its *opaque* pixels only, so an overlay's transparent majority
    doesn't dilute the measurement) up toward a bright target range,
    applied as a follow-up pass in `world::compile_content` over every
    name `BlockRegistry::tint_mask_names` reports (both directly-`tinted`
    faces and `overlay` faces) - deliberately *not* threaded through
    `build_atlas` itself as a new parameter, which would have rippled
    through every test that constructs an atlas for no benefit to any of
    them; this only ever needs to run for the handful of tiles an actual
    tinted block uses, so a small targeted post-process pass over the
    already-built pixel buffer was the smaller, more surgical change.
- **Once the overlay/brightness fix above actually shipped, the follow-up
  report ("works! but colorings off, its too light needs to look like a
  health green") was pure art direction, not a mechanism bug - and worth
  telling apart from one.** The mask this multiplies against is itself
  bright (stretched toward white by `normalize_tint_mask_tile`), so a
  correspondingly pale/desaturated tint color reads as washed-out pastel
  once combined with it - `biome.rs`'s `DRY`/`LUSH`/`COOL` moved to
  noticeably darker, more saturated values (`LUSH` in particular: `[0.42,
  0.70, 0.32]` -> `[0.20, 0.62, 0.18]`, a real "health bar green" rather
  than a mid-tone one). This is a color you can't get right from a remote
  session with no display - said so plainly and treated the change as a
  best-effort first pass to react to, not a final answer. **Changing the
  darker constants moved `the_gradient_is_continuous_at_its_midpoint`'s
  `assert_eq!` from "happened to round exactly" to "off by one f32 ULP"**
  (`lerp3(DRY, LUSH, 1.0)` came out `[0.20000002, 0.62, 0.18]` against a
  `LUSH` of `[0.2, 0.62, 0.18]`) - `a + (b - a) * 1.0` was never guaranteed
  bit-identical to `b` in floating point, the old constants just happened
  to round away the error and the new ones don't. Fixed by switching to an
  epsilon comparison (`1e-6`, matching this file's other float-tolerance
  tests) rather than picking constants to dodge the rounding - a test
  asserting a *design invariant* (the two branches must agree at the
  boundary) shouldn't constrain which numbers are allowed to be chosen for
  unrelated (art-direction) reasons. Verified the epsilon is still tight
  enough to catch a real discontinuity, not just loosened until anything
  passes, the same way every other regression test in this file is
  checked: broke `lerp3` (`+ 0.1` on its output) and confirmed the test
  still goes red before restoring it.
- **The first real biome (`Biome::Snow`) is a trait-method enum, not a
  hardcoded name check anywhere it matters - the user's own framing for
  the request ("save this water freezing feature as a biome trait so
  whenever we institute another cold feature, we can have it do this as
  well").** `Biome::freezes_water`/`Biome::drier` are the two behaviors
  `Snow` needs today; a future cold biome (tundra, glacier, ...) gets both
  just by returning `true` from the same two methods, with zero changes to
  either `terrain.rs`'s generator or `world.rs`'s runtime freeze rule -
  neither ever checks `== Biome::Snow` directly. `biome.rs`'s existing
  grass-tint noise and the new biome-*region* noise are deliberately
  separate streams (`REGION_SEED_OFFSET`, a much larger `REGION_SCALE`) -
  a snow-biome column has no grass to tint at all, and conflating the two
  would mean a change tuned for one cosmetic gradient silently reshaping
  the other's regions.
- **A biome-driven height change needs to blend on the same continuous
  noise value a hard classification thresholds, not step at the
  classification's own edge.** `Biome::drier` pushes terrain up (so fewer
  columns dip below `SEA_LEVEL` and form a lake) - if that boost snapped
  on/off exactly where `region_biome_at` flips from `Plains` to `Snow`, the biome
  edge would read as a real elevation cliff, which is far more visually
  jarring than the existing hard edge in which *texture* a column's
  surface uses (already accepted at `SNOW_LINE`). `biome::drier_strength`
  ramps `0.0..=1.0` across a blend window centered on the same
  `SNOW_THRESHOLD` `region_biome_at` itself uses (not a second, potentially-
  disagreeing threshold), so terrain is already rising as a column
  approaches Snow biome and the elevation change is smooth even though the
  surface-block change right at the edge still isn't.
- **A trait property tested by sampling a wide grid, not by trusting the
  formula - same discipline the grass-tint noise already established.**
  `SNOW_THRESHOLD` was picked by writing `region_noise_area_fractions_
  land_in_a_reasonable_range` first (sample a grid across several seeds,
  assert the measured Snow fraction lands in a sane 5-35% range) and only
  then choosing a threshold value that passed it, rather than guessing a
  number against `fbm2`'s own "roughly -1..=1" bound and hoping the area
  it carves out is reasonable.
- **Freezing an exposed lake surface only ever needs to look one layer
  deep, which is what makes it safe to check directly off `BlockSetEvent`
  instead of a ticked, budgeted queue like `FluidQueue`.** Converting the
  top water source to ice means whatever's beneath it no longer has air
  contact at all - there's structurally nothing left to cascade into, so
  one check per relevant event (the changed position itself, and the cell
  below it) is the whole rule, unlike fluid spread's genuine multi-step
  relaxation. It's also why this is written to go through the same
  `set_block` + a written `BlockSetEvent` a player action would use,
  *not* `set_fluid_cell`'s player-edit-bypassing sibling: freezing is a
  one-shot, irreversible transition (water never thaws back in this
  feature), not a continuously re-derived value the way fluid spread is,
  so it doesn't share fluid's reason to dodge `EditLog` - `record_edits`
  picks the written event up for free, which is what makes a frozen cell
  survive a reload with zero extra persistence code, the same way a
  player breaking a block does. See `world.rs`'s other simulation-pattern
  notes for the general version of this "does this write need to dodge
  the player-edit path" question - the answer here is no, for a different
  reason than fluid's own "no" would have been.
- **A system can't take both `EventReader<T>` and `EventWriter<T>` for the
  *same* event type as separate parameters - Bevy panics at schedule
  build time (`B0002`, `ResMut<Events<T>>` conflicting with a previous
  `Res<Events<T>>` access) - and only the real headless integration tests
  (`tests/headless.rs`, which build an actual `App`/schedule) caught it;
  every isolated unit test calling the pure `try_freeze_cell` function
  directly passed the whole time, since they never go through Bevy's
  schedule at all.** `freeze_exposed_water` needs to both read
  `BlockSetEvent` (what changed) and write one (the resulting ice
  placement) in the same system, so it takes `ParamSet<(EventReader
  <BlockSetEvent>, EventWriter<BlockSetEvent>)>` instead - Bevy's
  mutually-exclusive-access wrapper built for exactly this "same
  resource, two conflicting kinds of access, never needed at the same
  instant" case. General lesson: a pure-function unit test proves the
  *logic* is right; it says nothing about whether the system wrapping
  that logic can even be scheduled, which is exactly the gap `tests/
  headless.rs` exists to cover for this whole codebase.
- **A test that searches for a column matching a condition has to use the
  same effective value the code under test actually acts on, or it can
  pass for the wrong reason.** The first version of `snow_biome_columns_
  generate_snow_at_low_altitude_not_grass` searched for a column below
  `SNOW_LINE` using `TerrainGenerator::surface_height` directly - but
  `generate()` applies `Biome::drier`'s height boost *after* that call, so
  the column the search found could easily have been boosted back above
  `SNOW_LINE` by the time terrain actually placed a block there, making
  the test pass on the pre-existing altitude-cap path even with the new
  biome-driven override deliberately disabled (caught by breaking the
  override and finding the test stayed green - the same "does a new test
  actually turn red" discipline as everywhere else in this file). Fixed by
  extracting the real formula into `TerrainGenerator::effective_height`
  (used by both `generate()` and the test) instead of letting the test
  re-derive its own copy that could quietly drift from the real one.
- **"So land doesn't just become one giant undivided landmass" plus "rivers
  that run high down to sea level, real flow simulation" is two genuinely
  different features wearing one request - oceans need large-scale
  *connectivity*, rivers need genuine per-tile *flow accumulation*, and
  they don't share a mechanism.** Oceans turned out to need no architecture
  change at all: `terrain.rs`'s `continent`/`CONTINENT_SCALE` is just one
  more very-low-frequency noise stream blended into height (mirrors the
  existing `mountain` mask's own low-freq-decides-the-shape pattern),
  blended toward a real `DEEP_OCEAN_FLOOR` via `surface_height`'s
  `ocean_t` - still a pure per-column function, zero cross-chunk
  dependency. Rivers are the opposite: a river's shape is fundamentally
  non-local (it has to know what's uphill of it, arbitrarily far away), so
  before writing any code the real fork was surfaced to the user via
  `AskUserQuestion` - a cheap noise-band approximation (zero architecture
  change) vs. genuine flow accumulation (needs a precomputed/cached
  heightmap, a real change to "chunks generate independently"). The user
  picked real simulation, so `terrain.rs`'s own "no cross-chunk
  dependencies" invariant (this file's very first line about the module)
  got its first deliberate, documented exception.
- **"A global flow simulation can't work in an unbounded world" turned out
  to be wrong - it just has to be *lazy and memoized* instead of computed
  up front.** Rivers were first built over bounded 512-block regions
  (`RegionHydrology`, now deleted), each fading out before its region's
  edge, on the reasoning that a catchment could extend arbitrarily far
  upstream. The user then asked for rivers "not bound by that box."
  `drainage.rs`'s `Network` covers the whole world. It's a 16-block cell
  grid where each cell's downstream neighbour is a *local* fact (steepest
  descent over its 8 neighbours). Accumulation and water level are walked
  upstream on demand, memoized per node in `OnceLock`s inside lazily
  created 32x32-cell tiles. Three things make it terminate and stay
  deterministic:
  - **Routing over a surface with no pits.** Steepest descent on raw noise
    strands water in local dips, so rivers came out short and many ended in
    inland lakes. The cure was routing over a separate `routing_height`:
    ground smoothed over a 40-block ring, plus a signed
    `SEAWARD_TILT * (continent - OCEAN_THRESHOLD)` that makes the whole
    continent lean toward its ocean. Rivers only end at real ocean
    (`Landscape::is_sea`); a below-sea-level inland basin is something a
    river flows *through*, not into.
  - **Measuring flow at the unjittered cell center.** Node positions are
    jittered for drawing, but sampling the routing height at the jittered
    point manufactured fake pits between cells.
  - **A hard catchment cap.** An unbounded upstream walk OOM-killed the
    test process (SIGKILL) on an infinite synthetic slope.
    `MAX_ACCUMULATION` aborts any walk that has explored more than that
    many cells and memoizes the cap. It stays order-independent because
    whether a walk hits the cap depends only on the true catchment size,
    never on which chunk asked first.
  River *length* was limited by geography rather than by the algorithm
  until `CONTINENT_SCALE` was cut 3x. Before that, land sat a median 160
  blocks from the coast, so no river could ever be long. Measured after
  the change: median ~300 blocks, p90 ~700, longest ~1100.
- **A shared memo behind a `Mutex` is still the right concurrency shape for
  chunk generation.** `Network::tiles` (`Mutex<HashMap<Cell, Arc<Tile>>>`)
  is touched from every async gen task, but the lock is held only to find
  or create a tile. Each node's values live in `OnceLock`s, so two tasks
  racing on the same node at worst both compute the same deterministic
  answer. Measured at ~2ms per chunk cold in release, no different from
  before.
- **Give the flow algorithm a synthetic world it can be tested against.**
  `drainage::Landscape` is a trait (routing height, ground, sea, incision,
  base level), and `TerrainGenerator` is just one implementation. The
  drainage tests use a hand-built `Valley` closed by ridges, so it has a
  finite catchment and doesn't trip the cap. With that valley they can
  assert water reaches the sea along the valley floor, accumulation grows
  downstream, results don't depend on query order, and a surface never
  rises downstream. None of that can be hand-predicted on noise terrain.
  Break-tested: removing the downstream cap on water level turns the
  "never rises" test red.
- **When splicing a big file with a script, anchor on something unique.** A
  Python `rfind("#[cfg(test)]")` meant to find the test module instead
  matched a later `#[cfg(test)]` *attribute* and silently deleted the
  whole `mod tests` in `terrain.rs`. Caught only because the test count
  dropped. Restored it with `git show HEAD:src/terrain.rs`. Anchor on
  `"#[cfg(test)]\nmod tests"` instead, and check the test count after any
  scripted edit.
- **Hand-picked constants for a brand new noise-driven system are exactly
  the case this file's "measure, don't assume" rule exists for, and a
  first guess was wrong in a way that was only visible by measuring, not
  by reasoning about the formula.** (Describes the old per-region version;
  the lesson stands.) `MAX_ACCUM_FOR_FULL_CARVE` was first
  guessed at `700` (near `FLOW_GRID`'s full `64*64` cell count, reasoning
  "a river could in principle drain the whole region") - real fbm terrain
  never gets close: a throwaway test dumping the actual accumulated-flow
  grid for several seeds showed real maximums landing around `25-70`, not
  in the hundreds, because `mountain`/`terrain`'s own local relief (not
  just the broad regional slope) constantly redirects steepest-descent
  paths before they can all converge into one channel. Left at `700`, the
  real rivers this produced were carving only 1-3 blocks deep everywhere -
  technically "a river" by the `> 0` test that first caught it, but nowhere
  near strong enough to read as a real feature. Recalibrated to `55`
  (near the measured ceiling) so a genuinely well-fed real channel reaches
  full depth. Similarly, `OCEAN_THRESHOLD` was first set assuming "land
  should stay the clear majority, like before oceans existed" - but a
  real connectivity test (flood-filling sampled land/ocean grids) showed
  that with land as the strong majority, land *itself* becomes the one
  giant connected mass (with the ocean fragmented into many small inland
  seas) - the exact mirror image of the original complaint, just with the
  labels swapped. Moving the threshold toward the point where land and
  ocean are close to evenly split is what actually let *both* sides
  percolate into multiple large, separate connected regions - a basic
  percolation-theory fact (a minority phase on a random field fragments;
  only a share close to 50/50 lets either phase form multiple large
  components) that wasn't obvious from the formula alone and only showed
  up by measuring real connected-component sizes.
- **The "is this really solved" test has to match the user's literal
  complaint, and the naive version of that test can be topologically
  impossible to satisfy.** A first attempt at the ocean test asserted a
  *single* connected ocean covering most of total ocean area, mirroring
  how `region_noise_area_fractions_land_in_a_reasonable_range` measures a
  single fraction - but real coastlines legitimately produce several
  separate seas (Pacific, Atlantic, Indian...), so demanding one dominant
  basin is stricter than reality itself. `oceans_split_land_into_multiple_
  masses_separated_by_real_seas` instead asserts what the user actually
  asked for: at least two *real* (>=1% of sampled area, so a handful of
  noise-driven single-cell ponds in the long tail don't count) separate
  landmasses, and at least one ocean basin large enough to read as a real
  sea - satisfiable by both "two continents, one ocean" and "three
  continents, several seas," because the user's complaint was about
  landmass separation existing at all, not about ocean topology
  specifically.
- **"The autocomplete dropdown stops at the first space" was a real,
  deliberate design decision (its own CLAUDE.md entry above) - extending it
  to complete a command's *arguments* too (`/locate`) had to generalize
  that decision, not just bolt a special case onto it.** The dropdown's
  core data stayed `commands::CommandSuggestion { text, usage, description
  }`, unchanged in shape - the only shift was what `text` means: always
  "whatever replaces everything after the leading `/`," which used to be
  only ever a bare command name and is now sometimes a whole `name arg1
  arg2` line. Because `chat.rs`'s Tab-fill and click-fill handlers
  (`chat_text_input`'s `KeyCode::Tab` arm, `click_suggestion`) already just
  did `chat.input = format!("/{} ", suggestion.text)` with no assumption
  baked in about *how many words* `text` holds, **neither needed a single
  line changed** - the exact "give the general formula a no-op for the old
  case" shape this file keeps finding elsewhere (`mesher.rs`'s
  `rotated_tile`, `light.rs`'s `attenuate`). Only `command_suggestions`
  itself (decide *what* to suggest for the current input) needed new logic:
  no space yet -> still the old command-name path (`registry.suggestions`,
  completely untouched); a space -> split into the already-finished
  command name plus argument tokens (`split_whitespace`, with a trailing-
  space check tracked separately so "nothing typed yet for the next token"
  and "still mid-word" stay distinguishable - `split_whitespace` alone
  would silently swallow a trailing space and conflate the two), look that
  exact name up (reusing the existing private `CommandSpec::matches`, not a
  new prefix search), and call its new optional `arg_candidates` hook if it
  has one.
- **An argument-completion hook only ever needs the tokens *before* the one
  being typed, never the partial text of that token itself - prefix
  filtering is a generic, one-time concern, not something every command's
  hook should re-implement.** `CommandSpec::arg_candidates: Option<Box<dyn
  Fn(&[&str]) -> Vec<ArgCandidate>>>` takes only `prior` (the fully-typed
  tokens so far); `CommandRegistry::arg_suggestions` is the one place that
  filters the returned candidates by whatever prefix is currently typed
  (case-insensitive `starts_with`, mirroring `suggestions`'s own top-level
  filtering) and sorts them. `/locate`'s own hook
  (`locate_arg_candidates`) is consequently a two-armed `match` on `prior`'s
  *length* (`[] =>` the qualifier position, `[qualifier] =>` that
  qualifier's own names, reading `Biome::ALL`/`Feature::ALL` directly so a
  third biome or feature needs zero changes here) with no filtering logic
  of its own at all - exactly the same "declarative data, one generic
  algorithm" shape as `sky::MoonEventDef`'s table.
- **An argument completion must echo back whatever command name the player
  actually typed - an alias or the primary name - never silently normalize
  it, because normalizing would make Tab/click fill in text different from
  what's already on screen.** `CommandRegistry::arg_suggestions` builds
  each result's `text` by joining the literal `command_name` parameter
  (exactly as typed) with `prior` and the new candidate, not
  `spec.name` (the canonical name) - caught by writing `an_argument_
  completion_echoes_back_whatever_command_name_was_actually_typed` as a
  real regression test (a throwaway command registered under one name
  with an alias, completed by its alias) rather than trusting the
  "obviously correct" choice between the two fields without checking.
- **`Biome`/`Feature` both grew an `ALL`/`name`/`parse` triplet for the same
  reason, in two different modules, and that's a feature of the pattern,
  not duplication worth merging.** `/locate biome <name>` needs to parse
  and enumerate biomes; `/locate feature <name>` needs the same for
  terrain features - but biomes and features are unrelated concepts
  (`biome.rs` vs. a new enum local to `terrain.rs`) with no shared base
  worth generalizing into. Each gets its own tiny, self-contained table
  (`const ALL: [Self; N]`, `fn name`, `fn parse` derived from the other
  two) exactly where its variants already live, so a third biome or a
  fourth feature is a one-line edit to `ALL`/`name` in the one file that
  already defines it - not a shared registry `/locate` would otherwise
  need to reach into two unrelated modules to maintain.
- **A "find the nearest X" search needs an explicit, generous-but-finite
  search bound, or it silently promises to find something that might not
  exist within any reachable distance.** `terrain::locate_nearest`'s
  expanding-ring search is capped at `LOCATE_MAX_RADIUS` (6000 blocks -
  generous relative to every noise scale a predicate could be built on:
  `biome::REGION_SCALE` 640, the drainage network's river catchments, continent noise's
  far larger wavelength), returning `None` rather than looping forever if
  nothing within that bound ever matches. It deliberately is *not* a
  perfect global nearest-neighbor search - it stops at the first ring with
  any match and picks that ring's own closest-by-real-distance hit, which
  can in principle miss a slightly closer match just inside the next
  ring's near edge. Documented as an accepted approximation rather than
  fixed, for the same reason the bounded-region river search itself is
  accepted as-is: a `/locate` command promises "get me close to a real
  one," not "the provably single closest coordinate in the universe."
- **Reusing the generator's own definitions for `/locate feature` is what
  keeps "what counts as a river/mountain" from ever disagreeing with what
  the generator actually placed.** `Feature::River` is `column_profile(..)
  .in_river` - the exact wet channel `generate` floods - and
  `Feature::Mountain` is `mountainness >= MOUNTAIN_RANGE_THRESHOLD`, the same
  mask the range was raised by and the `Mountain` biome is gated on. (Both
  were first written as altitude/carve-depth approximations; the river/
  mountain rework below replaced them with these rather than keeping two
  definitions of each concept.)
- **"Can the game already do X?" gets answered by measuring, not by reading
  the code that's supposed to do it.** Asked to confirm rivers could sit
  flush or dug-in and mountains could be their own thing, a throwaway probe
  over three seeds found ~96% of river columns at or below sea level with
  banks *under* the water on average (rivers only formed in lowlands that
  were already flooded; upland reaches were dry trenches), and columns
  inside the mountain mask mostly 26-31 tall, almost never reaching the
  snow line. Neither was visible from the code - the carve formula and the
  mountain mask both looked reasonable in isolation. Reporting those
  numbers was the honest answer to "confirm," and they set the targets the
  rework was then tuned against.
- **Water that can stand above sea level isn't held in by anything except
  the generator's own care, so containment has to be an explicit rule with
  its own invariant test.** Sea water is safe by construction - only
  columns whose ground is *below* `SEA_LEVEL` fill. A river's surface sits
  wherever its valley is, so a dry column beside the channel can easily be
  lower than the water. `TerrainGenerator::column_profile`'s **levee rule**
  raises any dry near-river column to at least every 4-neighbour's water
  surface, computed from those neighbours' *raw* (pre-levee) profiles so
  it's still a pure per-column function with no cascade.
  `river_water_never_stands_beside_open_air_on_dry_land` checks every wet
  column's neighbours across four real river windows, and turning the levee
  rule off makes it fail immediately. The one deliberate exception is water
  meeting water at different heights *within* a river - a 1-block rapid
  where the surface steps down. Voxel water can't slope, so a river that
  descends has to step somewhere; the step is static until something
  nearby triggers the fluid sim.
- **A river's surface is a pull-style value propagated downstream, and it
  needs the same "only ever improve" cap as `recompute_cell`'s fluid
  levels.** Each river cell's candidate surface is `bank height - incision`.
  Incision varies along a river (`river_incision`), so a deeply incised
  stretch followed by a flush one would have its surface jump *up* to the
  flush stretch's banks - water climbing uphill. `drainage::Network::
  water_level` caps each cell at the lowest surface of any river cell
  draining into it (memoized, so walking upstream happens once per cell),
  so a river's surface can only stay level or step down. Tested against the
  synthetic `Valley` landscape in `drainage.rs` - break-tested by removing
  the cap.
- **Rivers became segments with real width instead of a blurred grid, and
  that's what made "flush vs dug in" expressible at all.** The old carve
  field bilinearly interpolated an 8-block grid, so a river's cross-section
  was whatever interpolation happened to produce - one fixed soft V. Each
  river cell is now a segment to the cell it drains into, carrying
  water level, half-width, depth and incision at both ends.
  `TerrainGenerator::river_sample` finds the nearest segment and interpolates
  along it, so a column knows its distance from the centerline and can be
  shaped deliberately: a rounded wet channel, then banks at `water +
  incision` (a vertical wall `incision` blocks tall at the water's edge, or
  none when flush), then terrain easing back to natural over
  `VALLEY_WIDTH`. Narrower for incised rivers, the start of a canyon.
- **A bug can hide behind an unrelated feature, and only shows when that
  feature is switched off.** While break-testing the cliff code (setting
  coast steepness to zero), the *mountain* tests went red: only 59 of 4,493
  range columns reached the snow line. The soft ceiling (`soft_ceiling`,
  which eases tall terrain toward the build limit instead of clamping it
  flat) was being applied twice, once in `land_relief` and again in
  `base_height`, compressing every peak below the snow line. The steep-
  coast uplift happened to stack enough extra height on ~30% of land to
  hide it. The break test was aimed at cliffs and caught a mountain bug -
  worth remembering that "switch one feature off" also checks which
  *other* tests secretly depended on it.
- **A test can pass with its feature removed because a different feature
  produces a lookalike - exclude the confounder rather than loosen the
  test.** `coasts_come_both_as_cliffs_and_as_beaches` stayed green with
  coast steepness forced to zero, because a mountain range running into
  the sea is also a tall shore. It now skips columns with any
  `mountainness`, after which disabling cliffs fails it (0 of 247 shoreline
  columns). Same lesson as the `big heal` chat test: break the thing a test
  claims to guard and confirm it goes red.
- **Altitude-layered biomes need the column's height, so the "real" biome
  moved onto the generator, and the region-only classifier was renamed so
  it can't be mistaken for it.** A mountain's base keeps its region biome,
  and `ALTITUDE_ZONES` (a declarative table, lowest band first - adding
  spruce on the lower slopes is one entry) takes over above each band's
  `min_height`. That makes a column's biome depend on its generated height,
  which only `TerrainGenerator` knows. `biome::biome_at` became
  `biome::region_biome_at` (it still can't return `Mountain`, which is
  tested). `TerrainGenerator::biome_at` is the full answer, and every
  consumer that cared about a column's actual biome switched to it:
  `generate`, `world.rs`'s freeze rule (`try_freeze_cell` now takes a
  `biome_at` closure, so its tests can still feed region noise), and
  `/locate biome`. The `world::BiomeMap` resource, which existed only to
  hand region noise to the freeze rule and `/locate`, was deleted rather
  than kept around as a second, incomplete way to ask "what biome is this?"
- **"Mountain generation separate, but in line with the mountain biome"
  means one mask drives both.** `mountainness` is the range mask:
  `land_relief` lifts terrain by it, `zoned_biome` only applies altitude
  zones where it's at least `MOUNTAIN_RANGE_THRESHOLD`, and `/locate feature
  mountain` searches for it. A tall column that *isn't* on a range (a
  steep coast's raised headland) therefore keeps its region biome, and the
  `Mountain` biome can never appear somewhere the terrain wasn't actually
  built as a mountain.
- **"Generator changes never alter explored land" is a snapshot of each
  chunk's *generated* terrain, not a save of every block.** `snapshot.rs`
  writes a chunk once, the first time it generates
  (`saves/<world>/chunks/c.<cx>.<cz>.bin`, ~2.5 KB deflated: a block-*name*
  palette plus `u16` indices). Every later visit loads it instead of
  generating. Player edits and fluid still ride the existing
  `EditLog`/fluid-save paths on top, unchanged. The write is temp file +
  rename, so a neighbour reading the file concurrently sees it whole or not
  at all. An unreadable file is generated again and replaced, never a crash.
- **Only chunks from a *different* generator count as old, or the blend
  creeps outward forever.** A new chunk blends into old neighbours
  (`TerrainGenerator::generate_beside`): height is offset by the nearest
  old column's difference from the current generator, fading over
  `BLEND_DISTANCE` (16). A river fades to dry ground within
  `RIVER_FADE_DISTANCE` (8). If *every* saved neighbour were old, chunk B
  would blend into chunk A, which already blended into O, and so on
  without end. Results would also depend on load order. Each snapshot
  stores a fingerprint of the generator's actual *output* (FNV over a few
  sample chunks, plus a manual `GENERATOR_REVISION`), and only neighbours
  with a different fingerprint are blended against. Old chunks are never
  written again, so the set a new chunk blends against is fixed, and two
  new chunks agree on every column between them in either order.
  `new_chunks_come_out_the_same_whichever_generates_first` goes red the
  moment same-fingerprint neighbours are treated as old. Hashing output
  rather than a version number has a useful side effect: a spurious
  mismatch (a refactor, a toolchain change) is harmless, because an old
  column that matches what the generator makes now contributes an offset
  of zero (`an_old_chunk_this_generator_would_have_made_anyway_changes_
  nothing`).
- **Water in or near old chunks needs the levee rule's guarantee across
  the seam too.** Three rules, each with a test that goes red without it:
  - A new river near a *differing* old column is filled to its own water
    level and eased to natural ground. The ordinary levee rule then holds
    back the water still upstream.
  - A blended channel whose bed ends up at or above its water counts as
    dry.
  - A new column beside old water standing above sea level is raised to
    hold it in.
  The leak test checks old land both higher *and* lower than the new
  terrain. Only the lower case can actually leak, and the first draft
  tested only the higher one.
- **`write_save` was saving every fluid cell in every loaded chunk,
  including every generated ocean cell, and that was hiding a migration
  bug.** Harmless while terrain was deterministic from the seed. Once a
  world's terrain can be generated anew (a pre-snapshot world opened on a
  changed generator), those saved cells would put the old seas and rivers
  back, floating over the new land. Two fixes:
  - `Chunk::base_fluid` (one bit per cell, set where the chunk was
    generated with fluid) lets `write_save` skip sources the snapshot
    already restores.
  - `collect_gen_tasks` only reapplies saved fluid onto a chunk restored
    from its snapshot (`GeneratedChunk::restored`).
  The bit has to be cleared whenever a cell's block changes (`set_block`,
  and `set_fluid_cell` when the id changes). Otherwise generated water
  that was edited to air and then refilled by the sim as a source looks
  "generated", gets skipped, and the reload shows the edit's air instead.
  The existing `water_restores_exactly_after_leaving_and_reentering_a_world`
  caught exactly that, because its pocket happened to sit in a lake.
- **To test "the generator changed" without changing the generator, use a
  different seed.** It makes entirely different terrain through the same
  code, which is all a snapshot or blend test needs. `snapshot.rs`'s tests
  and `tests/headless.rs`'s `a_reload_shows_saved_chunk_snapshots_not_
  freshly_generated_terrain` (which swaps spawn's snapshot for a seed-99
  one) both do this.
- **Waterfalls are a step in the river's surface plus a curtain over the
  face, and both had to follow rules that already existed.**
  - *The step:* in a segment whose water drops by at least
    `WATERFALL_DROP` (3), `river_sample` keeps the upper level for every
    column whose `natural` ground can hold it, then switches to the lower
    one. Without that per-column check, the lip would sit wherever the
    segment's midpoint fell. A lip out past a cliff edge would leave a
    wall of river standing over the sea.
  - *The curtain:* any wet column beside river water at least
    `WATERFALL_CURTAIN` (2) higher is filled up to it with a flowing
    level-1 top cell over `FLUID_FALLING`. That's exactly the state
    `recompute_cell` settles a ledge into, so the simulation leaves it
    alone. Generated fluid is no longer all sources, so snapshots store
    every non-source level (sparse index + level after the block indices),
    and `Chunk::base_fluid`'s bit now means "this cell's fluid is exactly
    as generated". It clears on any change to the block *or* the level.
  - Break-testing showed the curtain fixes a gap that had been there all
    along. With stepping disabled, the search still found 5+ block drops at
    river mouths, so exposed walls of standing river water already existed
    there (the profile-level leak test allows water beside lower water).
  - Measured over 6000x6000 blocks on three seeds: roughly as many falls
    into the sea as inland, up to 28 blocks tall. Generation stays at
    ~2-3ms per chunk.
- **Two lighting bugs that looked unrelated had one shared cost: a FIFO
  light queue flooded by work that shouldn't have existed.**
  - *Dark lines along every chunk border under the sea.* The generator's
    straight-down sky fill passed full sunlight through water, because
    water isn't opaque. Propagation dims it. `seed_new_chunk` skips
    interior full-sky cells but always seeds border cells, so the borders
    settled dim and the interiors stayed wrong until a slow cascade crept
    in from the edges. Fixed by sharing one rule: `light::sky_falling_into`
    is both `offered`'s downward case and what `TerrainGenerator::
    sky_columns` steps with. **A "settles eventually" test can't catch
    this.** The first version of `sunlight_under_the_sea_has_no_seams_at_
    chunk_borders` checked for a fixed point and passed with the bug
    reintroduced, because the cascade does get there (9 s instead of
    0.25 s). It now asserts the generator's fill already equals the
    settled light in open sea.
  - *A mined hole stays dark for a long time.* The light values were
    right (`digging_a_shaft_lets_sunlight_down_it` passed at once). The
    edit's updates were waiting at the back of a queue full of streaming
    work, much of it that cascade. `LightQueue` now has an `urgent` queue
    for `BlockSetEvent` work and everything it re-enqueues, drained first,
    with a remesh flush the moment it empties.
    `a_mined_hole_lights_up_even_while_chunks_are_still_loading` piles on
    background work before digging and goes red if edits share the normal
    queue.
- **A river's surface eases down a block with the fluid sim's own flowing
  levels, derived locally from the fraction the generator already had.** A
  river's water level is a float interpolated along each segment, and the
  surface used to be its floor: a whole-block step every few dozen blocks.
  `RiverSample::slope` (drop per *grid step*, i.e. per Manhattan block,
  because that's how the sim measures distance) turns the leftover
  fraction into "how many steps downstream of the last drop is this
  column". Within water's `flow_distance` the column gets one more water
  block on top at that flowing level (`ColumnProfile::top_level`), which is
  what the sim settles to downstream of a higher source. It's part of
  `water_top`, so the levee rule raises the banks around it for free.
  Measured against the sim's exact rule (a level is one more than the best
  same-height neighbour), 83% of cells match. Measuring per straight-line
  block instead of per grid step got 80% under a looser check, because a
  diagonal river covers fewer grid steps than the sim counts. The rest are
  diagonal stretches: static until something nearby triggers the sim,
  which then settles them itself. An exact version would need a BFS over a
  padded region per chunk *and* a second definition of a column's profile
  beside `column_profile`, which is the drift this file keeps warning
  about. `river_surfaces_ease_down_in_flowing_levels_between_steps` keeps
  the 75% floor and checks the generated blocks carry the level.
- **Cliffs read as "a terrain glitch" when they were a steep *ramp* with
  grass on every step. Modelling them on real sea-cliff geomorphology
  fixed that, as the user suggested.** The old steep coast just narrowed
  the land-to-sea-floor blend (`CLIFF_BLEND`), which still left a ramp a
  few blocks wide. Every column in it got the normal grass + 3 dirt +
  stone, so the face was a staircase of grassy ledges. `coast_shape` now
  converts the continent field into *blocks inland of the edge* (value
  past the threshold divided by its local gradient) and builds the
  features coastal geomorphology describes:
  - a vertical edge, pushed in and out by a "rock hardness" noise into
    headlands and bays (`CLIFF_EDGE_WOBBLE`)
  - a wave-cut platform at `SEA_LEVEL - 1` with boulders thickest under
    the face
  - sea stacks offshore
  In `generate`:
  - Any column with a 4+ block drop *and* a 3+ block rise beside it is
    bare rock.
  - Any column with a 4+ drop keeps only one block of soil.
  - Sea-facing faces get a wave-cut notch above the waterline.
  These are generic slope rules, so steep mountain ground benefits too.
  `generate` now builds one profile grid with a one-column ring
  (`at(x, z)`) and reads neighbours from it. That serves both the slope
  rules and the waterfall curtains without asking for any profile twice.
  Mapping the result (an ASCII height map of a real stretch of coast,
  printed from a throwaway test) caught a leftover: partly steep coasts
  mixed the cliff and beach profiles into a thin low ridge just past the
  platform. Cliff shape now applies fully from half steepness up
  (`smoothstep(steep * 2)`).
  `cliffed_coasts_have_rock_faces_platforms_and_sea_stacks` goes red for
  deep soil on faces or sand on platforms.
- **"Rivers flow to ponds and stop" was two different dead ends, found by
  classifying every river mouth rather than guessing from the report.** A
  throwaway survey of all mouths over 300x300 cells (seed 7) found:
  - 76 real ocean mouths.
  - 15 "sea" mouths with no deep water anywhere near: coastal hollows
    where the continent field was a hair under the ocean threshold.
  - 46 that weren't water at all: *pits*, cells with nothing lower around
    them. Almost all were less than a block deep, on flat plains far
    inland where the seaward tilt is too weak.
  Fixes:
  - **`Landscape::is_sea` means open ocean.** `OPEN_SEA_MARGIN` puts it a
    quarter of the way into a beach's blend. A river crossing shallower
    water on its way out is invisible, since its surface can't drop below
    sea level.
  - **Pits spill** (`drainage::Network::spill`). Flood outward from the pit
    lowest-rim-first (a min-heap on the highest point water must rise
    over) until reaching a cell whose *steepest-descent* chain ends at the
    sea or at a pit strictly lower than this one. The cells on that path
    flow along it. A cell can sit on several pits' paths along its chain
    (its own terminal, the pit that one spills into, and so on); it
    follows the lowest one's.
  - **Why it can't loop:** each accepted outlet leads to strictly lower
    pits. The natural `down` / `terminal` stay separate from the effective
    `flow`, so a spill search never needs another pit's spill, and there's
    no recursion that could deadlock a `OnceLock`.
  - **`terminal` is capped at `MAX_CHAIN`** and returns `DRAINS_AWAY`. The
    test `Valley` descends forever outside its ridges, and the uncapped walk
    OOM-killed the test process, the same way the uncapped accumulation
    walk once did.
  `every_river_flows_on_until_it_reaches_the_open_sea` follows every river
  cell over 200x200 cells on two seeds. `a_river_spills_out_of_a_hollow_
  and_carries_on_to_the_sea` covers a synthetic hollow. Both go red with
  spill paths ignored. Re-surveyed: 0 pit mouths on both seeds. Cold
  generation on river-heavy seeds went from ~3 to ~5.5 ms per chunk (the
  first spill searches; memoized after).
  Ponds were deliberately left as standalone depressions rather than wired
  to streams: the drainage network doesn't target them, and a river may
  pass through one.
  River size is also capped by *length* (`Network::length`, the longest
  upstream path, computed in the same walk as accumulation): `size =
  min(by_flow, by_length)`, so a short river stays a stream however much
  land drains into it.
- **Marshes and salt seas reuse the spill search's own byproduct: a
  depression's footprint.** The user's framing was "have a solution for
  when this happens instead of mangling the terrain." `drainage::Basin`
  (what `spill` became) keeps every cell its flood covered before reaching
  the outlet: exactly the lake the depression would hold filled to its
  rim. `TerrainGenerator::wetland` turns a *river-fed* basin into:
  - a **marsh** if it overflows. Ground at or below a low level (40th
    percentile of the footprint's own ground) becomes flat mud, with
    pools where a noise field says so. The river keeps its channel
    through it.
  - a **salt sea** if it keeps its water. A lake to just under the rim's
    lowest point, on a salt bed, with salt flats on the shore, where
    rivers end.
  Things to keep in mind:
  - **Whether a basin keeps its water must not depend on how much flows
    into it** (`Landscape::keeps_its_water` sees only the pit's position
    and the footprint's size). Spilling changes which cells flow where, so
    keying it on accumulation would make the decision depend on itself.
    "Is a river feeding it" is checked afterwards, only to decide whether
    to *render* anything.
  - **A footprint is whole 16-block cells,** and the first salt sea came
    out with a ruler-straight shore along the cell grid (caught by an
    ASCII map). `wetland_near` interpolates membership between the four
    surrounding cell centres and wobbles the 0.5 threshold with noise.
  - **Containment reuses the levee rule** through a new `near_wetland`
    flag, deliberately separate from `near_river` (same two jobs, levee
    and no caves, but a different reason).
    `wetland_water_never_stands_beside_open_air` goes red without it.
  - **A rarity test has to fail when the rarity knob is broken, not just
    pass when it's set.** The first version asserted salt seas were at
    most 20% of all wetlands and stayed green with `SALT_SEA_CHANCE = 1.0`,
    because most basins are too small to qualify anyway, so size alone
    kept it rare. It now compares among basins big enough to be one.
  - **Memoize what every column asks.** Each column checks the basins of
    its 9 nearest cells, and `is_sea` (continent noise + natural height)
    was being re-evaluated on each. `Node::sea` brought generation back
    from 6-9 ms to 3-6 ms per chunk.
  Measured over 6x6 km on three seeds: 53-91 river-fed wetlands, 3-7 of
  them salt seas (each a few thousand blocks²); marsh is ~0.5% of the
  area.
- **"The game got jittery" was measured, not guessed: a throwaway headless
  benchmark (the real `WorldPlugin` + `LightPlugin` at render distance 8,
  a player flying ~18 blocks/s, `app.update()` timed per frame) plus
  temporary per-system timers (a drop-guard `Span` writing into a static
  map, removed afterwards).** Before: p90 5.2 ms, p99 8.5 ms of main-thread
  time, worst frames 20-30 ms. After: p90 2.4 ms, p99 5.3 ms, nothing over
  16 ms. What it found, in order of size:
  - **The light queue was growing to ~870,000 cells while moving and never
    caught up.** `seed_new_chunk` queued every chunk-border cell, and,
    since the undersea-seam fix made water cells no longer "full sky",
    every underwater cell too. Each time the `VecDeque` outgrew its buffer
    it was copied in one frame, which was the spike that ignored the time
    budget. Lighting also lagged far behind the terrain. Now a cell is
    queued only if a sideways neighbour is brighter. The fill already
    gives exactly what arrives from above, and below can only be dimmer.
    Neighbours across a seam are read from the loaded world, so a border
    cell next to an already-lit chunk still gets queued. The backlog now
    peaks under 10k. `streaming_while_moving_keeps_the_light_backlog_small`
    goes red with the skip removed.
  - **Count budgets aren't time budgets.** `process_light_updates` processed
    up to 8,192 cells per frame whatever that cost (up to ~8 ms).
    Background work now also stops at `LIGHT_TIME_BUDGET` (2.5 ms). Edits
    (`urgent`) aren't held to it. `collect_gen_tasks` likewise stops taking
    in finished chunks after `COLLECT_TIME_BUDGET`.
  - **The seam pass did two `HashMap` lookups per cell, about 4,000 cells
    per chunk landing.** The four neighbour chunks are now fetched once
    and indexed directly.
  - **Autosave scanned every chunk ever loaded** (24 ms with 1,100 loaded,
    growing all session) and wrote JSON on the main thread. Now:
    - `Chunk::touched` lets it skip chunks nothing has changed, whose only
      fluid is what their snapshot restores.
    - `FluidSaveCache` reuses a touched chunk's list until its `version`
      changes.
    - `persist` writes on the `IoTaskPool` for autosaves, synchronously on
      exit. Writes are sequence-numbered and serialized behind one lock,
      so an older in-flight autosave can never land after, and overwrite,
      a newer save.
    - `SaveStore::save_data` now writes a temp file and renames it into
      place, so a cut-off background write can't truncate the save.
  The benchmark can't see GPU mesh uploads, since it has no renderer. It
  measured ~1.7 meshes per chunk entering view, so re-meshing isn't
  running away, but upload cost on the user's machine is unmeasured from
  here.
- **Biomes come from climate maps through declarative tables, not from
  one-off noise thresholds.** `biome::ClimateMaps` holds temperature and
  humidity, both low-frequency, so similar climates clump.
  `LAND_BIOMES`/`SEA_BIOMES` are rows of `ClimateBand` (temperature and
  humidity ranges), and `classify` takes the first row that matches. A new
  biome is a new row, and nothing else needs to change. Humidity drives
  only grass tint so far; it's there for the next biome that needs it. The
  old `BiomeNoise` resource became `world::WorldClimate`, and `mesh_chunk`
  takes `&ClimateMaps`, so tint and biome read the same maps.
  - **"Open sea" is its own check, not "water at sea level".**
    `TerrainGenerator::is_open_sea` also requires no river, a `Normal`
    surface, and the continent field to be (just about) ocean. Without
    those, inland ponds, marsh pools and salt seas at sea level would have
    been labelled `warm_sea` and the like. Sea biomes take over from the
    region biome there, which is what ended the snow biome's ice sheets
    running out over the ocean: the runtime freeze rule asks `biome_at`,
    and sea biomes don't freeze water.
  - **Shore ice and icebergs are both generation-time, keyed on the column's
    biome.** Shore ice is a top-block swap where a cold sea is shallow (with
    noise so the edge is patchy). Icebergs are an *overlay* sea biome:
    `Icebergs` replaces `ColdSea` where a regional noise plus a coast bonus
    clears a threshold, so they're commonest near land. Measured on seed 7:
    ~50% of cold sea within 150 blocks of land is iceberg water, against
    ~19% further out.
  - **A clump test on simplex noise needs a ratio, not a tight absolute
    bound.** Simplex has a built-in gradient even over short distances.
    `climates_clump_together_over_hundreds_of_blocks` first required nearby
    samples to differ by almost nothing and failed (0.113). It now asserts
    near < 0.15 *and* far is at least 2.5x near, which is what "clumps"
    actually means.
  - **Waterfall curtains made an old leak test fail for the right reason.**
    `a_river_running_into_an_old_chunk_ends_before_it_without_leaking`
    flagged falling water beside air, which a curtain is supposed to have.
    The test now checks only standing (source) water. Re-break-tested
    after the change: disabling the river's dry fade still turns it red.
    When a test that guards one rule starts failing because of a *newer*
    rule, narrow it to what it was guarding, then prove it still catches
    the original bug.
- **A periodic check against GitHub's REST API eats the unauthenticated
  rate limit (60 requests/hour per IP), and a 403 from it looks like an
  outage.** The launcher re-checked the dev build every 60 s by listing all
  releases through `api.github.com`, exactly the hourly allowance on its
  own, so the Versions list and dev check both failed with a raw
  `NetworkError: ... status: 403` (reported as a screenshot). The dev
  release's tag and file names are fixed by `dev-build.yml`, so
  `remote::fetch_dev_build` now reads `dev-manifest.json` and installs the
  archive from `github.com/<owner>/<repo>/releases/download/dev/<file>`,
  plain downloads that don't count against the API limit. A 404 there means
  "no dev build yet". `selfupdate.rs` already did the same via
  `raw.githubusercontent.com`. Only the Versions list still uses the API,
  once per launch or Refresh, and `remote::explain_github_error` turns a
  403/429 into "GitHub is limiting requests, wait then Refresh". Verified
  with a throwaway live test that ran the real fetch + install against the
  published dev release. Anything new that polls GitHub should use a
  download URL like this, not the API.
- **"No mountains" was the world height, not the mountain code.** The world
  was 64 tall with the sea at 26, so land could rise only 38 blocks.
  `WORLD_HEIGHT` is now 128, with the sea left at 26 on purpose: moving the
  sea would have left every explored chunk's coast and rivers at the wrong
  level. Things that went with it:
  - **Snapshots record their height and load shorter ones.** `parse_header`
    used to reject any height but the current one, which would have
    silently regenerated every explored chunk (breaking "explored land never
    changes"). `decode` now remaps a shorter chunk into the taller layout
    with air above. `a_snapshot_saved_when_the_world_was_shorter_still_
    loads_as_it_was` writes a real 64-tall file via `encode_at`, and goes
    red with the remap removed.
  - **128 is also a format limit.** Snapshot fluid levels are stored as a
    `u16` cell index; 16x16x128 = 32,768 fits, 256 would not. Memory scales
    too (~10 bytes a cell, so ~330 KB a chunk, and visited chunks are never
    freed). Taller than 128 needs both looked at first.
  - Land got lowlands-to-uplands (`LOWLAND`/`UPLAND_RISE`, by distance
    inland from the continent field), a hill-country mask (`hilliness`), and
    ranges about three times taller. Snow line, the mountain altitude zone
    and the soft ceiling moved up to match. The range mask's ramp was
    widened (0.2 -> 0.3) after a probe found a range front rising 47 blocks
    in 20: taller mountains need wider foothills.
- **Rivers "look like scars" was measured as trench walls, and fixed with
  valleys sized to the cut.** A quarter of river channel columns sat more
  than 8 blocks below the surrounding land (some 27, and up to ~50 once the
  land got taller), but the valley eased back to the land over only 5-14
  blocks. A river's valley now has a floodplain, then sides at
  `VALLEY_SLOPE` (steeper with `mountainness`), steepening only when the
  side couldn't otherwise climb out within `MAX_VALLEY_REACH` (a gorge).
  - **Take the lowest valley of every nearby segment, not the nearest
    segment's.** Wider valleys overlap, and switching to whichever segment
    is nearest makes the floor jump along a seam. The minimum of continuous
    surfaces is continuous. `RiverSample::valley` carries it.
  - **A wider search needed a per-chunk segment list.** The reach went from
    ~30 to ~63 blocks. `river_segments` gathers every segment that could
    reach an area once, `Blend::for_chunk` caches it for a whole chunk, and
    a lone `column_profile` call builds its own. Generation stayed at
    ~3 ms a chunk.
  - **The first metric didn't measure the complaint.** "3+ block steps near
    rivers" over the whole valley reach barely separated old from new (and
    once favoured the old code), because most counted columns were beyond
    where the old valley reached at all. Restricting it to columns beside
    rivers cut 6+ blocks below the land gave 1.7% vs 0.6%, now
    `rivers_below_the_land_run_in_valleys_not_trenches` (red with the old
    valley). An ASCII height map of a real deep crossing confirmed the
    shape before trusting the number.
- **The "weird spires" were a sign error.** Sea-stack height was
  `land - 2 - (strength - threshold) * 30`, so a stack was tallest at its
  edge and sank in the middle: thin rings of pillars with low slabs inside
  (the floating squares in the screenshot). Stacks now rise toward their
  middle, taper over `STACK_TAPER`, stand below the clifftop and under
  `STACK_MAX_HEIGHT`. `sea_stacks_are_stocky_blocks_of_rock_not_spires`
  measures every stack's footprint and height (height <= 6 x sqrt(footprint))
  and goes red with the old formula ("56 blocks tall on 10 columns").
- **A terrain change moves every test that searched for "the first X".**
  Four tests broke for reasons that weren't bugs: the first waterfall found
  was now in a snow biome (frozen solid by design), chunk (0,0) now sits on
  a slope so its seam gap is the land's own step, spawn sits on a hillside
  so a torch's probe cell was inside the hill, and the wetland-rarity test
  only counted the few wetlands its searches passed. Each was checked by a
  probe before touching the test, and each test was narrowed or made to
  measure against the land itself (`own_gap`, `open_air_run`, a fixed
  survey area) rather than loosened.
- **"Can we go higher than 128?" needed three things besides the
  constant, and each was found before it bit.** (`WORLD_HEIGHT` 256, sea
  still 26.)
  - **The snapshot format had two one-byte/two-byte assumptions.** The
    header stored the chunk height in a `u8` (256 doesn't fit) and fluid
    cells by `u16` index. Format 2 widens both to `u16`/`u32`.
    `parse_header` reads either (the header's length depends on its
    format), and `encode_as(.., height, format)` lets a test write a genuine
    format-1, 64-tall file. The per-column ground/water bytes still fit,
    since both are clamped to 255.
  - **Memory had to become bounded.** Every visited chunk stayed loaded,
    ~10 bytes a cell, ~640 KB a chunk at 256 tall. `unload_far_chunks`
    drops chunks `UNLOAD_MARGIN` beyond the render distance by putting
    them back in the state of a chunk not visited this session: edits
    stay in `EditLog` and go back into `PendingEdits`; fluid is scanned
    (`chunk_fluid_cells`, shared with `write_save`) into `OriginalFluids`
    and `PendingFluids`; terrain is the snapshot on disk. So it adds no new
    persistence path, and only unloads a chunk that has a snapshot (fluid
    is only reapplied onto restored chunks). Measured at render distance 8,
    flying: chunks capped at ~330 / ~210 MB.
    `chunks_unloaded_far_away_come_back_exactly_as_they_were` compares the
    whole chunk (blocks and fluid levels) after a walk back *and* after a
    save made while unloaded, and goes red with any of the three hand-backs
    removed.
  - **An "only when the player changes chunk" trigger stranded chunks.**
    If a far chunk was mid-mesh at that one moment it was skipped until the
    player next crossed a chunk border. Found because the break-tests
    failed with "never unloaded" instead of the expected message. It now
    retries every frame while any far chunk was only temporarily busy.
    When a break-test fails for the wrong reason, the test was passing by
    luck.
  - **Light seeding was queueing work that could never land.** The seam
    pass offered light to the neighbour chunk across each seam, and an
    unloaded neighbour reads as dark air, so every open cell up the whole
    border looked improvable: ~6,500 cells a chunk at 256 tall, and a
    341k backlog while flying (it had been 4x smaller and under the test's
    limit at 64). Counters on each seeding path found it. It now skips
    unloaded neighbours; the neighbour's own seeding pulls the light in
    when it arrives. `a_mined_hole_lights_up_even_while_chunks_are_still_
    loading` had been using that waste as its "lots of background work",
    so it now queues a real backlog itself (re-break-tested).
  - **Steep mountains came from the range mask, not the peaks.** Splitting
    the slope into its parts showed the mask's gradient times the range's
    full height dominated (p90 2.4 blocks/block vs 1.0 from the ridges),
    because the mask's higher octaves wiggle its edge. Bigger ranges
    (`MOUNTAIN_SCALE` 0.0017), two octaves, a wider ramp, and a base lift
    that rises with `range` (peaks with `range²`) brought p90 to ~1.8 and
    p99 to ~3.6. Lowering the ridge frequency alone had barely moved it.
- **New biomes reshape terrain, so a biome's roughness is a continuous
  field, not a lookup on the classified biome.** Plains (flat), forest
  (rolling, humid) and alpha plains (the old rough grassland) differ in
  relief. `biome::relief_scale` ramps between them across each climate
  boundary, from the same thresholds `classify` uses (the `drier_strength`
  pattern), so terrain never steps at a biome edge.
  - **Alpha plains are an overlay, not a climate row.** Rare patches come
    from `ClimateMaps::alpha_patch`, applied inside `region_biome_at`.
    Basins come from `WetlandKind::AlphaPlains`, chosen by a hash of the
    pit, never by flow, for the same reason salt seas aren't.
    `ColumnProfile::alpha_plains` carries it to `biome_of`. Basin
    roughness is added after drainage (like marsh flattening), so it can't
    feed back into where the water goes.
  - **Streams are a lower drainage threshold, and that forced one water-
    level threshold everywhere.** `Network::water_level` memoizes per node
    regardless of the threshold it was called with, so mixed thresholds
    would make a node's level depend on which caller came first.
    `WATER_LEVEL_THRESHOLD` is the stream threshold, used by every caller,
    which also means a river is capped by the streams feeding it.
    `waterway` decides what's drawn: rivers anywhere, streams off mountain
    ranges. Drawing streams only in grassland and forest left ~5% of
    streams sinking into the ground where they flowed into snow
    (`streams_run_on_into_rivers_or_the_sea` goes red with that rule).
  - **Ponds are a widening of the river around a node, and a column inside
    one has to take the pond's sample.** Choosing the nearest centerline
    let a stream flowing in give its edge of the pond the stream's higher
    water. `river_sample`'s `in_pond` wins over `best`. The first version
    of the pond test was itself wrong: it truncated negative coordinates
    toward zero and measured from block corners, so it "found" dry ground
    that was really outside the pond. A probe of the exact column showed
    it, before anything in the generator was changed.
  - **Trees are a pure per-column rule (`tree_at`) so canopies cross chunk
    borders.** Each chunk evaluates columns `TREE_CANOPY_RADIUS` past its
    edge and draws the leaves that reach in. Trunks are drawn before any
    leaves, and leaves only into air, so the result doesn't depend on
    order. `Blend::for_chunk` covers 4 columns out for those lookups. The
    border test counts a neighbouring trunk as "carried": 5 of 70
    "misses" turned out to be another tree's trunk in the way, which was
    confirmed before tightening the test to require all of them.
  - **Two old tests broke for reasons that were the test's.** The trench
    test now skips waterfall segments (`RiverSample::falls`): 106 of 190
    "walls" were waterfall lips, which streams made far more common. The
    old-chunk river test had built its "different" old land from
    `natural_height`, which a new valley could lower by exactly the
    offset, making the old column identical and rightly unblended. It now
    offsets `effective_height`. Both were re-break-tested.
- **Mining, drops, health and death: where each rule lives.**
  - `hardness`/`drops`/`breakable` are block-file data (`blocks.rs`).
    `BlockRegistry::drop_of` resolves a drop. `compile` refuses an unknown
    drop name, the same way it refuses a missing texture.
  - `interact::Mining::advance` is the pure mining rule (time, restart on
    a new target or a changed block, a short cooldown after each break).
    `interact` only feeds it input and spawns the drop.
  - `items::step_item` (physics) and `items::give` (hotbar first, then
    storage; top up before empty slots) are pure and unit-tested.
  - `health::Vitals::tick` covers breath, drowning and regen.
    `health::fall_damage` turns a fall into damage. The fall itself is
    measured in `Player::step` (`track_fall`), because only the physics
    knows when you landed.
  - The death screen is a fourth mutually exclusive overlay
    (`state::DeathState`). Every toggle that checked `chat.open`/
    `inventory.open`/`paused.open` now checks it too.
- **Rotating a mesh drawn through the chunk material breaks the tint
  trick, so free-standing cubes send a zero "normal".** The tint rides in
  `ATTRIBUTE_NORMAL` (see the biome-tint entry), and a rotated entity's
  normal matrix rotates it: a spinning item's `[1, 1, 1]` would turn into a
  shifting colour. `mesher::cube_mesh` (dropped items, the crack overlay)
  sends `[0, 0, 0]`, which every transform leaves alone and which Bevy
  skips normalizing. `chunk.wgsl` reads exact zero as "untinted" and
  substitutes white, so items match the world's brightness. Interpolating three zero vertices
  gives exactly zero, so the comparison is safe. The shader was checked by
  parsing and validating it with the vendored `naga` crate in a scratch
  project with Bevy's imports stubbed out. A deliberately broken copy was
  rejected, so the check does catch real errors. That's the cheapest way to
  check a WGSL edit from a session with no GPU.
- **Hotbar and inventory are resources that outlive a world, so loading
  has to reset them.** Before inventories were saved, nothing reset them,
  and a second world in the same session started with the first world's
  items. `world::enter_world` now calls `inventory::restore_inventory`,
  which empties both and then applies the save (if any).
  `a_new_world_does_not_inherit_the_last_worlds_inventory` covers it.
  `Hotbar`/`Inventory`/`LoadedItems` are `init_resource`d by
  `WorldPlugin` as well as their own plugins, so the headless app (world
  and light only) can still save and load them.
- **A save field the world module doesn't own goes through a
  `SystemParam`.** `world::CarriedState` bundles the hotbar, inventory and
  dropped-item query. `autosave`/`exit_world` were already at 13-14
  parameters, and Bevy's limit is 16. Item entities are despawned on leaving
  a world by `items::despawn_items`, ordered
  `.after(world::WorldSaveSet)` so the save still sees them.
- **`SAVE_FORMAT` went 1 -> 2 with an empty migration step.** Every new
  field is a serde default, so old worlds load fine. The bump is for the
  *other* direction: a format-1 build would open a new world and save it
  back without the inventory.
- **Headless tests have no player physics plugin, so a test that needs the
  player on the ground places them** (`stand_on_ground` uses
  `open_air_run`). The first version waited for the player to land, and
  timed out, because nothing ever stepped the player.
- **Items are their own id space, separate from blocks.** `ItemStack.id`
  is an `item_registry::ItemId`. Every block with `item: true` gets an item
  with the same id string, and plain items (twig, stick) come from
  `items/*.json`. Because both are `u16`, the compiler did *not* catch places
  still treating an item id as a block id after the switch. They were found
  by grepping `registry.def(`/`tiles[`/`max_stack` across every file that
  touches `ItemStack`; the dropped-item mesher and the merge/pickup stack
  sizes were the misses. Item ids only line up with block ids by accident
  (every block before them has an item), so tests must look up items via
  `ItemRegistry`, never `BlockRegistry`. Saves store id strings, so nothing
  had to be migrated.
  - Placing goes `item -> ItemRegistry::block_of`; a plain item places
    nothing. This also fixed an old bug where right-clicking with an *empty*
    hand next to water "placed air" over it.
  - Icons go through `ui::ItemIcons` (a `SystemParam`), which falls back to
    `block_icon` for block items and a flat atlas crop for plain ones.
- **Crafting is data plus pure functions.** Recipes are `recipes/*.json`
  (`crafting::RecipeBook`). `times_craftable`/`craft` (storage first, then
  hotbar; overflow returned for dropping) and `RecipeBook::visible`
  (station in reach + an ingredient known) are what the tests exercise. The
  UI just calls them. A block is a station because some recipe names it,
  not because of a flag on the block. `KnownItems` is learned by
  `crafting::learn_items` (run from `ItemsPlugin`, so headless tests get
  it) and saved as `WorldData::known_items`.
- **Inventory moves are `click_slot`/`quick_move` on plain slices.** The
  stack on the cursor is `inventory::HeldStack`. It's saved
  (`InventorySave::held`), returned to the inventory when the screen
  closes (`return_held_stack`), and dropped on death. Any new way of
  closing the screen gets that for free, since it's keyed on `!inv.open`
  rather than on a particular close action.
- **Startup content has to exist before a world loads.** `ItemRegistry`
  and `RecipeBook` are built in `world::compile_content` (`Startup`). The
  headless harness used to switch to `InGame` before the first frame,
  which runs `OnEnter` *before* `Startup`, and `enter_world` failed on the
  missing resource. The real game always passes through the menu first.
  The harness now runs one menu frame first too, so the tests match.
- **A break-test helper that restores a file with `cp file.bak; ...; mv
  file.bak file` leaves cargo running the stale broken build.** The
  restored file's mtime is the time of the `cp`, older than the build of the
  broken version, so cargo sees nothing to rebuild. The next test run
  "fails" against the correct code. Found when a correct test kept failing
  after restore. `touch` the restored file, or rebuild, before trusting a
  post-restore result.
- **Discovery re-learns from the inventory, so a test of its persistence
  has to use an item that isn't held any more.** The first reload test
  passed with `known_items` not saved at all, because the reloaded hotbar
  taught the same items back. It now adds a twig to `KnownItems` without
  holding one.
- **Disk fills up in this environment:** `target/` reached 28 GB (debug +
  release) and the linker died with `signal 7 [Bus error]`, not an
  out-of-space message. `rm -rf target/release target/debug/incremental`
  freed enough. Check `df -h .` first when the linker crashes.
- **Pebbles and twigs are `Shape::Floor` blocks, not decorations or
  items.** A floor block is a real block in the grid, so the snapshot, edit
  log, mining, drops and lighting all work with no special cases. Its
  behaviour is spread across a handful of places, each keyed on
  `Tables::floor`/`BlockDef::shape`, never on a block id:
  - `mesher::push_floor` draws one quad at `FLOOR_HEIGHT` with the cell's
    own light.
  - `items.rs` drops it as a sprite.
  - `interact` gives it a flat outline, refuses to place it without solid
    ground, and breaks it off when its support goes (`pop_unsupported`,
    with the pure check `unsupported_floor`).
  - The file defaults (no collision, partial transparency, replaceable, a
    `face` icon) live in `BlockFile::into_def`, so `transparent` and
    `item_model` became `Option`s there.
  Twig used to be a plain item. It became a block, and its item is now the
  block's own item under the same id string, so recipes, saves and leaf
  drops didn't change.
- **Twigs scatter from the `trees` list, so they cross chunk borders.**
  `TWIG_RADIUS` must stay at most `TREE_CANOPY_RADIUS`, which is how far
  past its border a chunk looks for trees. Measured over 256 chunks (seed
  7): pebbles on ~1% of dry columns, twigs on ~6% of forest columns.
  `pebbles_lie_about_and_twigs_lie_under_trees` goes red if twigs ignore
  the trees.
- **A block that has to share a cell with water is two tags, not a special
  case in every system that touches water.** Pebbles on river/lake/sea beds
  first lived in a water cell *instead of* the water, and three systems
  (fluid sim, mesher culling, swimming) each needed a patch to pretend the
  cell was still wet - and the bed-pebble rule had to stay at two blocks deep
  so one in the top layer wouldn't leave a hole in the surface. The fix was a
  real second tag. `"waterloggable": true` makes the registry derive
  `block@layer` variants (`pebble@water`, `pebble@ice`) whose
  `BlockDef::layered` is `{block, layer}`, so the cell is one block id with
  two tags and every per-id mechanism (saves by name, snapshots, edit log,
  light `transmission`, floor-ness, drops via `of_block[variant] =
  of_block[base]`) works untouched. What did have to change, each guarded by a
  test that goes red without it:
  - **The sim compares the fluid a cell holds, not its id.**
    `Tables::fluid_kind` (water for both `water` and `pebble@water`),
    `fluid_host(id, kind)` (what a cell becomes when the fluid fills it:
    the variant, an overwrite, or nothing) and `Tables::dried` (what's left
    when it dries: the bare pebble, not air). Anything that wrote
    `id == cid` or `AIR` for "no fluid" was wrong for a two-tag cell.
  - **Freezing is a table, not a pair of ids.** `try_freeze_cell` reads
    `Tables::frozen[id]` (`water -> ice`, `pebble@water -> pebble@ice`, built
    from `freezes_to`), so a new fluid or waterloggable block needs no code.
  - **The mesher draws each half as its own part, against neighbours viewed
    as that half.** The first attempt resolved a layered neighbour by the
    *current cell's* side, so a plain water cell saw `pebble@water` as a
    pebble and walled it in - the part view has to prefer whichever half is
    the same block as the part being drawn (`part_at`), and only fall back to
    the part's own side for unrelated neighbours.
  - **Break leaves a fluid layer but not a solid one**
    (`BlockRegistry::left_behind`): mining a waterlogged pebble leaves
    water, while a pebble frozen in ice drops the pebble and breaks the ice
    too (ice itself drops nothing). It applies
    on all three paths that remove a block (Creative break, Survival mine,
    unsupported pop). Placing it into water picks the variant instead of
    displacing the fluid.
  - Variants share the main block's atlas slots by *copying* them at
    `compile` time - their derived texture name (`pebble@ice`) has no painter.
  - `SAVE_FORMAT` 3 -> 4: an older build would read the variant names as
    missing blocks and drop the pebbles on save.
- **Frozen water keeps the water's geometry by keeping its *level*, not by
  becoming a fluid.** A river's shape lives in the per-cell `fluid_level`
  array, and `set_block` leaves that array alone, so freezing a cell already
  preserved the slope - nothing drew it, and nothing saved it. Three pieces:
  `Tables::level_shaped` (ice, and variants holding it; its `flow_distance`
  is the water's, so heights match) tells the mesher to size a frozen cell
  by its level (`top_of`, shared by the cell and its step-wall neighbour);
  `Tables::holds_level(id, level)` is the one "does this cell carry fluid
  state" test, used by both the save scan and the generated-terrain
  `base_fluid` bit (a frozen cell at a still/falling level has nothing to
  save, so plain ice stays a plain cube and costs nothing); and placing a
  non-fluid block resets the stored level, since a stale flowing level left
  by earlier water would otherwise make a freshly placed ice block slope.
  Generated frozen rivers needed no generator change - they already wrote
  the flowing level under the ice. Runtime freezing is still only checked
  at `BlockSetEvent`s (the simulation's own writes emit none), so a river
  spreading into a cold biome freezes when something nearby changes.
- **Sloped fluid surfaces are a per-corner rule, not a per-cell height plus
  a wall.** The old mesher gave a fluid cell one top height and drew a
  partial step wall wherever a neighbour differed. The shape the user drew
  (each cell running from its own height down to the next cell's) is the
  same as "a corner is as low as the lowest same-fluid cell touching it",
  and that has to be a function of *which cells meet at the corner*, never
  of which one asks (`corner_top`, with `neighbour_corner` mirroring the
  coordinate across a face) - that's what makes two neighbours agree on
  their shared edge, so the surface is continuous and the wall disappears
  by itself instead of needing a second rule. Covered and unshaped cells
  (a plain ice cube, a deeper water cell under more water) stay flat at the
  top of their cell and don't lower anyone, so the wall survives exactly
  where it should: against a covered neighbour. The flat look is kept as
  `SurfaceStyle::Flat` behind `mesh_chunk_styled`, the `FallingWaterStyle`
  pattern, so the old step-wall tests still run against it.
- **Per-tool quality is per-*stack* data, which means every place that
  rebuilds a stack from its parts is now a place that can silently drop
  it.** `ItemStack` gained `quality: u8` (a whole percent, 0 for anything
  that isn't a tool), and `ItemStack { id, count }` literals were everywhere
  (50 of them). The mechanical part was a regex to `ItemStack::new`; the part
  that mattered was reading every site that *derives* a stack from another
  one - a partial move (`quick_move`'s leftovers, the cursor's remainder, a
  thrown drop, a stack landing in an empty slot) has to be `ItemStack {
  count, ..stack }`, not `new(stack.id, count)`, or the tool quietly comes
  back as quality 0. The regex had silently done exactly that to five sites,
  found only by grepping the rewritten calls for `stack.id`. Merging is the
  other half: `ItemStack::same_item` (id *and* quality) replaced `.id ==` in
  `click_slot` and `give`. Tools' `max_stack` is forced to 1 anyway, so
  nothing merges in practice today; the check is what keeps that true if a
  tool ever stacks. Each of the three (merge on click, merge on shift-click,
  save/reload of slots, and of a dropped item) has a test that goes red when
  it's reverted.
- **Tiers are independent of each other on purpose, so there is no
  cross-tier invariant to test - only a within-tier one.** An early draft
  of the design required each tier's worst tool to beat the previous tier's
  best, with the band ratios checked at load. The user wanted the opposite:
  a perfect iron sword that still beats a poor steel one is the interesting
  decision. `Tier::multiplier` is a straight line from the band's low to its
  high value, the loader only insists on `0 < low <= high` (quality never
  makes a tool worse), and a test pins the overlap case so nobody "fixes"
  it later. The tooltip shows the quality *name* (colour-coded) and never
  the number; the tier bands decide how much a name is worth.
- **`SAVE_FORMAT` 4 -> 5 for a field that's a plain serde default.** Same
  reason as every previous bump: an older build would read a saved tool at
  quality 0 and write that back over the real value. `quality` is skipped
  when zero, so saves of ordinary stacks look exactly as before.
- **A timing-sensitive headless test can fail under parallel load without
  being broken.** `streaming_while_moving_keeps_the_light_backlog_small`
  needs >150 chunks to stream in during a fixed number of frames; with 23
  headless apps competing for CPU it sometimes saw 99. It passes alone and
  with `--test-threads=1`, and it failed once with the new test skipped, so
  it isn't the new code. Not fixed here (it would mean reworking the test's
  frame budget); worth remembering before chasing a "regression" that is
  just a loaded machine.
- **Mining is "speed of the right tool" plus "does it drop", decided by the
  block's data, and its progress has to be a fraction.** `tools::
  mining_seconds(hardness, block_tool, requires_tool, held)` is one pure
  rule: `hardness * SECONDS_PER_HARDNESS (5s) / speed`, where `speed` is the
  held tool's (tier speed x quality multiplier) only if its kind matches the
  block's `tool`, and 1 for a hand or the wrong kind, times
  `NO_TOOL_PENALTY` (8) if the block `requires_tool` and this isn't it.
  `harvests` is the matching "does it drop" rule. (A first version removed
  the 8x penalty when you had the right tool, so a stone axe took a log from
  45s to ~2s - the user pointed out that leaves no headroom for iron, steel
  and the tiers after them. The penalty now only defines the *hand* time and
  a tool divides that whole time by its speed, so `mining_speed` is "times
  faster than a hand" and stone is 3.0; `stone_tools_are_slow_enough_to_
  leave_room_for_better_tiers` pins that.) The user's bare-hand times
  (dirt 2.5s, gravel 5s, stone 60s, wood 45s) are what pinned the constants:
  5s per hardness and an 8x penalty put stone (1.5) at exactly 60s, and a log
  has hardness 1.125 so it lands on 45s with the same constants - the
  numbers on the blocks, not a per-block "hand time" field, so the rule has
  one shape. Tools of the wrong kind, swords and hoes all fall out as speed 1
  for free. `Mining::progress` became a fraction instead of seconds: with
  seconds, swapping from your hand to a fast tool halfway through a 60-second
  block finished it instantly, because the stored 30 seconds beat the new
  3-second requirement. The crack overlay then no longer needs the block's
  time, only the fraction.
- **A flaky test that keeps failing under load is worth fixing once it's
  been blamed on load twice.** `streaming_while_moving_keeps_the_light_
  backlog_small` fixed its frame count, so how many chunks had landed by the
  end depended on CPU contention; it now keeps stepping (player stationary)
  until the setup threshold is met before judging, with the backlog still
  tracked throughout. Three full parallel runs passed after the change.
- **Soft blocks get a response curve, not a hard floor.** With tool speed a
  plain multiple of hand time, a stone shovel took dirt to 0.9s - under the
  second a cubic metre of earth should plausibly take, and a faster tier
  would only make it worse. The options were a per-block minimum time (but a
  floor makes tool *quality* irrelevant whenever it binds, which it did for
  every stone shovel) or letting a block take only part of the speedup.
  `BlockDef::tool_response` (`0..=1`, default 1) raises the tool's speed to
  that power, so dirt and gravel at `0.5` use its square root: a stone shovel
  is ~1.5s on dirt and ~3s on gravel, quality still moves it (1.7s down
  to 1.3s), iron ~1s, and even a speed-40 shovel is ~0.4s rather than
  instant. Logs and stone stay at 1 and take the full multiple. The rule
  reads the whole block now (`mining_seconds(&BlockDef, held)`, `harvests(&
  BlockDef, held)`) because passing hardness, tool, requires_tool and
  response separately had reached four positional arguments.

- **Knapping: a circle of force can't cut a sharp corner, so "exact match is
  100%" needed a second, gentler tool - and the measurements said so before
  any tuning did.** Pure logic is `knapping.rs` (`Session`: strike, tap,
  score, quality); the screen is `knap_ui.rs`; recipes carry a `knap`
  entry (`crafting::KnapSpec`) and `craft_clicks` opens the screen instead
  of crafting. Steps that mattered:
  - **Measure with bots, not by feel.** A greedy bot (best strike judged on
    force alone, as a player can) on the five shipped outlines never
    exceeded ~0.85-0.98 raw score with strikes alone, so Legendary would
    have been impossible. The right-click **tap** (breaks one cell, dents
    four, can't crack) is *my own addition*, not asked for; the user should
    be told and may veto it. With unlimited taps the bot made Legendary 41%
    of the time; hidden per-cell toughness (`INTEGRITY_SPREAD`) and strike
    force variance (`FORCE_VARIANCE`) cut that, and a per-stone budget
    (`MAX_TAPS` = 4) landed it at ~4% (careful bot: ~61% Rare, ~28% Epic).
    Those numbers are asserted in
    `a_careful_player_usually_makes_rare_tools_...`, and
    `heavy_strikes_alone_cannot_make_a_perfect_tool` /
    `striking_at_random_wrecks_the_stone` pin the other ends.
  - **The bot must depend on the rule the test guards.** The first
    two-phase bot looped `while session.taps_left() > 0`, so removing the
    budget from `Session::tap` left every test green. It now keeps tapping
    until the session refuses, and the budget break-test goes red.
  - **Don't draw hidden state.** The screen shades a cell by band
    (solid / weakened / flawed), never by raw integrity, which would leak the
    hidden toughness and make the stone readable.
  - **Take the stone on the first strike, not on opening.** Then "closing
    before touching it refunds" is true by construction (nothing was
    taken), with no refund path to keep in sync. The haft/binding are
    taken at Done (`finish_knapping`, which changes nothing if they're
    gone), and the tool is given with `ItemStack { quality, ..output }`.
  - **Escape/E ordering.** The knap systems sit at the end of
    `InventoryPlugin`'s chain; `toggle_inventory`/`inventory_escape` check
    `KnapState::is_open()` first (state at the start of the frame), so
    Escape closes the knap screen, not the inventory under it. The screen
    root has `FocusPolicy::Block` and a `GlobalZIndex` so the inventory's
    rebuilds (it respawns when the hotbar changes) can't draw over it or
    take clicks.
- **Swords were just data, because the tool kinds already existed.** The
  stone longsword and greatsword are an item file and a knapping recipe each
  (plus the dagger from before) - no code. The only real constraint is that an
  outline must fit inside *every* possible stone: `Stone::blob`'s radius is
  ~7.1-7.7, so a corner cell like (13,12) is outside some seeds' stone. The
  first greatsword guard was too wide and
  `every_shipped_outline_fits_inside_every_stone_and_is_one_piece` caught it
  over 500 seeds; keep guards within ~5 cells of centre. The aggregate
  difficulty test covers all shipped outlines together, so it needs no edit
  when shapes are added. Swords have no combat effect yet (damage is on the
  tier, unused until a combat step).
- **The training dummy needed an entity framework, and collision into the
  player was the part with a trap.** `entities.rs` is data (`entities/*.json`,
  `EntityRegistry`), a `WorldEntity` component, `SpawnEntity`/`ClearEntities`
  events, and `EntityColliders` (every box this frame). `player_model.rs` is
  pure geometry (six boxes, the 64x64 skin layout, mesh data), tested for
  the things that are easy to get silently wrong: no two boxes claim the same
  skin pixel, every face winds outward, the head's front is -Z.
  - **`collisionbox_binding`**: `true` = the entity's own `collision_size`
    box; `false` = that box rounded out to whole block cells ("like a
    block"). The user's wording was ambiguous about which value meant which;
    I read it so the dummy (bound to its model, 1x1x2) is `true`. Say so if it
    should be the other way round.
  - **Collision runs through `Player::solids`**, a `Vec<Aabb>` copied from
    `EntityColliders` each frame in `player_update`, rather than a new
    parameter on `Player::step` (nine existing tests call it, and the same
    field lets `interact.rs` refuse block placement inside an entity without
    another system param). `move_axis` merges entity bounds into the same
    "tightest bound" as the voxels.
  - **A box the player was already inside on that axis must be ignored.**
    Only a box they were clear of *before* the move can stop them
    (`was.max <= solid.min` going up the axis, and the mirror). Without it,
    summoning a dummy on someone ejects them or traps them. The break-test
    that removes that check turns `a_player_already_inside_a_box_is_not_thrown_
    out_of_it` red, and ignoring `solids` entirely turns the other two red.
  - **Lighting** can't use the chunk material (it only samples the block
    atlas), so entities use an unlit `StandardMaterial` whose `base_color` is
    `max(block, sky * current_sky_color)` at the entity's cell, the same
    formula as `chunk.wgsl`, recomputed per frame.
  - **Saved with the world** (`WorldData::entities`, by definition id so
    reordering the list can't scramble a save; `SAVE_FORMAT` 5 -> 6, defaults
    only). `enter_world` fires `SpawnEntity` events rather than spawning
    itself, and `CarriedState` reads `Option<Res<EntityRegistry>>` because
    the headless harness's apps may not have the plugin. The headless
    reload test goes red with the load loop disabled.
  - **A bare `App` can run the plugin's systems** (`MinimalPlugins`,
    `AssetPlugin`, `StatesPlugin`, `init_asset` for Mesh/Image/
    StandardMaterial/ChunkMaterial), which is what proves the schedule
    builds - a pure unit test of `collision_box` says nothing about that.
  - **Not done yet, on purpose:** gravity/physics for entities (a dummy
    summoned in mid-air floats), animation, a skin PNG override, slim arms,
    hit reactions. `/summon` places the dummy at the player's feet level.
- **The dummy became a block pair, and the entity framework was kept as its
  renderer.** The user wanted it "a block" you crouch-click to mine, taking
  time and dropping an item; of the two readings offered (stay an entity but
  mine like a block, or a real block) they chose the real block. So:
  `training_dummy` + `training_dummy_top` are ordinary blocks (`Shape::Model`,
  `upper`, `requires_crouch`), and `block_models.rs` stands the matching
  `EntityDef` (same id) in the lower cell as a `WorldEntity` + `BlockModel`.
  - **Two cells tall without special-casing the world:** `BlockDef::upper`
    names the block above. Placing checks `interact::upper_fits` and writes
    both cells; breaking either goes through `break_partner`, which removes
    the other (checking it still holds the right block, `partner_of`) and
    writes no drop. The upper half has `item: false` and drops the lower's
    item, so mining either gives one dummy.
  - **`requires_crouch` gates `interact::mineable`** (Shift, in Creative too),
    not the block id.
  - **Model entities are derived state.** `BlockModel` ones are excluded from
    `EntityColliders`, from the save (`Without<BlockModel>`) and from
    `/clearentities`; `sync_block_models` creates them from `BlockSetEvent`s and
    from every `ChunkMeshedEvent` (which is how a chunk loading with a dummy in
    it gets its model), and a half-second check drops any whose block is gone
    or chunk unloaded. They can't be wrong for long and nothing needs saving.
  - **A placeholder in `BlockModels` while the spawn event is in flight**
    keeps a second look in the same frame from spawning a duplicate;
    `attach_spawned` swaps in the real entity. The ordering is explicit
    (`EntitySet`: sync before, attach after) because commands from
    `spawn_entities` must be applied before `attach_spawned` can see them.
  - **Not done:** the model always faces +Z (blocks have no yaw), so placing
    it can't make it face you; `/summon` still makes the free entity version;
    the upper half still shows the whole 2-cell outline when targeted.
  - Tests that go red when the thing they guard is removed: the crouch gate,
    the mesher skipping model blocks, and the reload (chunk-meshed) path.
  - `Plugins` tuples top out at 15 members, so `main.rs` registers the
    entity plugins in a second `add_plugins` call.
- **The first particle effect, the damage number, is an entity with a
  hand-built text mesh - and its arc is solved, not tuned.** `particles.rs`:
  a `DamageNumberEvent { origin, amount }` spawns a mesh (one quad per
  character, from a procedurally generated 3x5 pixel font image) with its own
  unlit alpha-blended `StandardMaterial`, rotated to the player's camera
  rotation every frame (copying the camera's rotation needs no up vector, so
  it has no overhead pole to break, like the sun/moon billboards).
  - **Why a text mesh and not `Text2d`:** `Text2d` is drawn by the 2D render
    phase, which a 3D camera doesn't run; a font-atlas quad strip goes through
    the same 3D pipeline as everything else, and the font is generated, so
    there's no asset to ship or fail to load.
  - **`Flight::solve(start, rise, end, life)`** derives the launch speed and
    gravity that make the arc start at the head (1.7), peak 0.6 above that,
    and be at 0.375 (halfway down the 0.75-block legs) when it has fully
    faded, 1.2 s later. The user described the shape in words ("shoot up, come
    down a little, fully faded about halfway down the legs"), so those three
    numbers are the constants; with `v = sqrt(2 g rise)` the end height is a
    quadratic in `sqrt(g)`, solved in closed form. The test asserts all
    three properties (start, peak, end) and that it never goes lower.
  - **Fading is the material's alpha**, written each frame; each number has
    its own material, so they fade independently. Removing either the motion
    or the alpha update turns `a_hit_spawns_a_number...` red (it uses
    `TimeUpdateStrategy::ManualDuration`, so each update is exactly 0.1 s and
    the arc is checked frame by frame).
  - **Hitting is a plain left click on a `hittable` block** (a block flag, like
    `requires_crouch`, so the dummy's crouch-to-mine and click-to-hit never
    overlap), with a 0.45 s cooldown that, unlike the mining timers, isn't
    reset when the button is released, so clicking faster doesn't hit faster.
    Damage is `ItemRegistry::attack_damage` (tier damage at quality; hand/non-
    tool `HAND_DAMAGE` = 1). The dummy has no health: this step is only the
    number. `interact` is now at Bevy's 16-parameter system limit.
  - **Not done:** hit knockback or flash, a health bar, damage from anything
    but the held item, different attack speeds per sword kind (the dagger is
    meant to be fast and the greatsword slow; they all share the cooldown).
- **Throwing reuses the dropped-item entity as the projectile.** A thrown
  pebble is a `DroppedItem` with `thrown_mass: Some(m)`: `simulate_items` flies
  it with `step_projectile` (gravity + drag) instead of the normal item
  physics, and on hitting a solid cell it writes a `ProjectileHit`, drops the
  flag and becomes an ordinary item (lighting, rendering, pickup, merging and
  saving all came free; merging and pickup skip it while it flies).
  - **Mass is one rule, not per-item code.** Every throw has the same energy
    (`THROW_ENERGY`, 8 J), so `throw_speed = sqrt(2E/m)` and drag is
    `AIR_RESISTANCE / m * v^2`; `impact_damage = DAMAGE_PER_JOULE * 0.5 m v^2`.
    The user's constraint was ~1.5 damage for a pebble at close range, so the
    constants were chosen by simulating throws (a throwaway script mirroring
    the Rust step, then a test: `a_pebble_thrown_at_something_a_few_blocks_
    away_does_about_one_and_a_half`), not guessed. Drag was raised from
    0.0005 to 0.002 so a pebble visibly loses speed over a few blocks.
  - **A long lob hits harder, and that's physics, not a bug.** The first test
    asserted damage falls with distance and failed: a level throw over 24
    blocks falls ~20 blocks and arrives faster. Damage is the energy at
    impact, so it does; the test now documents that
    (`a_long_lob_that_falls_a_long_way_hits_harder_not_softer`) and a separate
    one checks the drag effect over the first few blocks, before gravity
    dominates. Compare like with like.
  - **Left click throws only for `throwable` items**, in `throw_on_click`
    (`interact.rs`), with `interact` skipping mine/hit while one is held.
    Throwing was not given its own `SpawnDrop` field because that struct is
    built in a dozen places; a separate `SpawnProjectile` event avoids
    touching them. `projectile_impacts` turns a hit on a `hittable` block into
    a `DamageNumberEvent`.
  - **Not done:** hit marks/bounces, throwing heavier things with a charge-up,
    damage to anything but the dummy, an in-flight spin, `mass` on tools, and
    the thrown stack doesn't carry the thrower's own movement.
- **"Accept all forms of damage" meant one event, not a list of cases.**
  Melee and thrown pebbles used to each write a `DamageNumberEvent`
  themselves, which would have meant every future source (a falling block,
  an explosion) re-implementing "is this the dummy, where are its feet". Now
  every source writes a `damage::DamageEvent { cell, amount, source }` and
  `damage_blocks` is the single receiver: a `hittable` block takes it and shows
  the number (`hit_origin` moved there). `DamageSource` has `Melee`,
  `Projectile` and `Falling`; `Falling` has no producer yet, but is in
  `DamageSource::ALL` so the test that loops every source (both halves of the
  dummy) fails the moment a new source is added and not handled. Break-tested
  by making the receiver ignore `Falling`.
  - **Pebble-only throwing is data, and tested:** only `pebble.json` has
    `"throwable": true`; `the_pebble_is_the_only_throwable_thing` fails if
    anything else gains it. (The `pebble@water`/`pebble@ice` variants copy
    the flag; they have no item, so the test excludes `layered` blocks.)
    `Q` still drops any item - throwing-as-a-weapon is only the click.
  - **Test hazard:** reading `Events::iter_current_update_events()` after an
    `app.update()` returned each event twice (two buffers), so the damage tests
    collect through a system into a resource instead.
- **Glass shattering is two numbers on the block, and the speed budget is the
  whole design.** `BlockDef::shatters_at` (min impact speed) and
  `shatter_keep` (fraction of speed kept) compile into `Tables::shatter_speed`
  (`INFINITY` = never) / `shatter_keep`; `items::shatter_outcome` is the pure
  rule. `simulate_items` now takes `ResMut<ChunkMap>` and a `BlockSetEvent`
  writer: a projectile hitting a shatterable block fast enough sets it to
  air (through the *player-edit* path, so it saves like mining), writes no
  `ProjectileHit` (no damage) and keeps flying at `speed * keep`; otherwise
  it lands as before. Pebble ~17.9 m/s, glass 5 / 0.5 gives 17.9 -> 8.9 ->
  4.5, so exactly two panes and a stall on the third; a second pebble
  finishes it. Break-tested both ways (keep = 1.0 lets one pebble through
  all three; threshold 500 breaks nothing). Because the borrow of the map
  for `is_solid` has to end before `set_block`, the step calls pass a
  closure over `&*map` scoped to each call, not one closure held across the
  loop. The headless test waits with `run_until` for the pebble to come to
  rest: a fixed number of `update`s wasn't enough (frame time is tiny).
- **Modder-friendly means a tag, not a constant: every knob a throwable
  needs is now per-item data.** `blocks::ThrowSpec` (`energy`, `drag`,
  `cooldown`, `damage_scale`, `ThrowSpec::DEFAULT` = the old globals) is parsed
  from `throw_energy`/`throw_drag`/`throw_cooldown`/`throw_damage` by one
  shared `#[serde(flatten)] ThrowFile`, used by *both* `BlockFile` and
  `ItemFile`, so block items and plain items accept the same tags and a
  mod author learns them once. Non-positive or non-finite values fall back to
  the default rather than being trusted. The spec rides on `SpawnProjectile`
  into `DroppedItem::thrown_spec`, and `ProjectileHit` carries `damage_scale`;
  `step_projectile_with`, `throw_speed_with` and `impact_damage_scaled` take
  the tag values, with the old two/one-argument functions kept as default
  wrappers so existing tests didn't move. Ice shatters with the same 5 / 0.5
  as glass (`glass_and_ice_both_shatter_and_stone_does_not`). When adding a
  gameplay rule from now on, put the number on the block/item file first;
  a `const` is only right for something no content should ever vary.
  Still global and *not* tags: `DAMAGE_PER_JOULE`, `ATTACK_COOLDOWN`,
  `HAND_DAMAGE`, gravity - say so if those should become tags too.
- **Two kinds of tags, and one table for each.** *Definition tags* are the
  fixed facts about a block/item type (the JSON keys: `hardness`, `shape`,
  `light`, ...); *state tags* are values on one placed cell that change at
  runtime (`axis`, `waterlogged`, `level`). Don't mix them: a thing that is a
  fact of the type goes in the JSON, a thing a player/command/simulation can
  change per cell is a state tag. `tags::DEFINITION_TAGS` lists every
  definition tag (name, scope Block/Item/Both, group, description);
  `warn_unknown_tags` (called from both loaders) warns on any other key with
  a nearest-match suggestion - a warning, not a panic, because an unknown key
  is also what an older build sees in newer content. `states::STATE_TAGS` is
  the matching list of state tags, each a row of three fn pointers (`values`
  for a block - empty means "doesn't apply", `get`, `set`), with
  `read_states`/`set_state` as the single generic algorithm and
  `StateEditRequest` + `apply_state_requests` as the in-game entry point
  (`/blockstate`, which queues a request through `WorldRequests::state_edits`
  and gets feedback via the chat log). A `set` returns the `BlockSetEvent`s to
  announce, because that is what remeshes, saves (`EditLog` keeps `(id, axis)`)
  and wakes the fluid sim - a state change that skipped the event would look
  right and then silently not survive a reload (the headless test reloads, and
  goes red with the event removed).
  **Storage is unified, with one stated exception.** `Chunk::state` is one
  `u16` per cell (the two bytes `fluid_level: Vec<u8>` + `axis: Vec<u8>` took,
  so no memory change); each tag's row has `storage: Storage::Cell(Field)`
  (`LEVEL` = bits 0-7, `AXIS` = bits 8-9, on the blocks that enable them; see the per-block layout entry below) or `Storage::BlockId`
  (`waterlogged`: the value is which block id the cell holds, because light,
  collision and the mesher are all per-id tables). **A new state tag goes in a
  spare field of the word - never a new per-cell `Vec`** - and the accessors
  (`get_fluid_level`, `get_axis`, `set_*_raw`) are the only code that touches
  the bits. `DEFAULT_STATE` is what a fresh cell holds; `pack_state(level, axis)`
  builds one from the generator's separate arrays (`GeneratedChunk` still
  produces `fluid`/`axis` vectors; they're packed on install, and the mesher's
  `PaddedChunk` still wants plain arrays, so `build_padded` unpacks per cell).
  On-disk formats are deliberately unchanged (no `SAVE_FORMAT` bump): axis via
  the edit log, level via the fluid scan. Moving *persistence* onto one path is
  separate and would need that bump and a migration. Not measured: the
  per-cell unpack in `build_padded` replaced two slice `copy_from_slice`s; the
  headless streaming tests still pass but I did not time mesh preparation.
  Break-tested: moving `AXIS` onto `LEVEL`'s bits turns four storage tests red.
  **Testing a table against the code it describes:** can't enumerate a serde
  struct's fields, so `every_listed_block_tag_is_a_field_the_block_loader_reads`
  probes each listed name with mistyped values (an unread key is silently
  ignored; a real field rejects at least one), and the shipped-files test covers
  the other direction (a field a shipped file uses but the table lacks). A
  field no shipped file uses and the table omits slips through; add the row when
  adding the field. Also now a tag: `attack_cooldown` (was the global
  `ATTACK_COOLDOWN`), per item/block, default 0.45; no existing item sets it
  yet (daggers fast / greatswords slow is the obvious use).
- **Every state tag is enabled per block through one definition tag,
  `states`.** `BlockDef::states` is the list; `STATE_TAGS` rows gained
  `enabled_by` (the behaviour tag that also implies it: `rotation`,
  `waterloggable`, `fluid`), `implied(def, is_freeze_target)` and
  `compatible(def, is_freeze_target) -> Result<(), why>`. `BlockRegistry::
  finalize_states` (runs at the end of `add_layered_variants`, so at load and
  compile) adds implied states, panics on an incompatible or unknown one with
  the block's id, and skips derived `block@layer` variants (their `states` is
  set when they're built: the base's plus `waterlogged` and `level`). The
  `states.rs` `values` fns now gate on `BlockDef::states`, no longer on
  `Tables::rotates/fluid/level_shaped`, so the list is the single source for
  "can this cell be edited". Declaring `axis`/`waterlogged` in a file turns on
  `rotation: log`/`waterloggable` in `into_def`, since the state is useless
  without the behaviour. Trap hit while writing it: variants are built
  *before* `finalize_states` runs, so a variant copying `base.states` got an
  empty list (the base's `waterlogged` was still only implied) - the variant
  adds `waterlogged`/`level` itself. Break-tested: with the full-cube rule
  removed the "can't waterlog a cube" tests go red.
- **The state word's layout is per block, not global, and every reader/writer
  of it is a "janitor" tracked in `docs/STATE_ACCESS.md`.** Plain-terms version
  of why: a block that doesn't use `level`/`axis` can put its own states in
  those bits, but then any code that writes the level bits for a cell it hasn't
  checked (the old `set_fluid_level_raw(place_pos, SOURCE)` on *every*
  placement) wipes the other block's data. So: (1) `DEFAULT_STATE` is `0` -
  level 0 is source, the axis is *stored* with `AXIS_Y` as 0 (`encode_axis`),
  and a declared state's first value is its default - which makes "clear the
  word" the one reset rule; `ChunkMap::set_block` does it, and
  `set_block_keep_level` keeps only the fluid's level for the cases where the
  fluid carries on (freezing, mining/placing a waterlogged block).
  (2) `BlockDef::state_defs`/`state_fields`: `states::allocate_fields` lays a
  block's own states into the bits its enabled built-in tags leave free
  (`level`/`waterlogged` reserve the level bits because the variants hold a
  level; `axis` reserves its two). `finalize_states` runs it, and copies a
  derived `block@layer` variant's layout from its block *after* the bases are
  done (the variant is built before the base's fields exist - same ordering
  trap as the earlier `states` list). (3) Persistence moved to the whole word:
  `BlockSetEvent` lost its `axis` field; `record_edits` reads the word from the
  map (so no sender can forget part of it), `EditLog`/`PendingEdits` hold
  `(BlockId, u16)`, `save::BlockEdit` gained `state: Option<u16>` (`axis` kept
  only to read old saves), and `SAVE_FORMAT` is 7. (4) A new file that touches
  the word must be added to `docs/STATE_ACCESS.md`'s `janitor-files` line, or
  `the_janitor_list_covers_every_file_that_touches_the_word` fails - the test
  scans code lines only, so a comment mentioning a pattern doesn't count.
  Break-tested: recording only the axis turns the whole-word reload test red,
  dropping the clear in `write_block` turns two layout tests red, and removing
  a file from the janitor line turns the janitor test red. **Still to do**:
  extended storage (a sparse per-chunk table for blocks needing > 16 bits; it
  would be a new janitor), and a mod loader that registers blocks before
  `finalize_states`.
- **A headless test that deletes a world's folder right after leaving it races
  the background snapshot writes.** `fluid_saved_over_since_regenerated_terrain_
  is_not_put_back` failed ~1 run in 8 with `remove_dir_all` -> "Directory not
  empty": leaving a world queues chunk-file writes on the IoTaskPool, and they
  were still creating files as the test deleted the folder. Not a state-word
  bug (diagnosed from the error text and by measuring: 2 of 15 runs failed
  before, 0 of 20 after). The test now retries the delete. I did not run the
  old commit to prove the race pre-dated the state-word change; the cause is
  in the test's own cleanup either way.
- **Extended state storage is a sparse per-chunk table, and the default is
  "no entry".** `BlockDef::state_storage` (`compact` default | `extended`)
  picks the tier for a block's `state_defs`. Extended: `states::allocate_extended`
  lays the states out one after another from bit 0 of the cell's byte string
  (`ExtField` { bit offset, bits }, `ext_get`/`ext_set` handle byte-straddling
  fields), capped at `MAX_EXTENDED_BITS` (256) so one block can't make a cell
  cost kilobytes; each state is also capped at 65,536 values. `Chunk::extended`
  is `HashMap<u32, Box<[u8]>>`; `set_extended_raw` stores *nothing* for all-zero
  bytes (first value = default = zero), which is what keeps memory proportional
  to non-default cells. `write_block` removes the entry on any block change
  (guarded by `!is_empty()` so the common case costs no hash). Persistence rides
  the edit: `EditLog`/`PendingEdits` hold `(BlockId, u16, Ext)`, `BlockEdit.ext`,
  `SAVE_FORMAT` 8, reapplied after the word on load. A derived `block@layer`
  variant copies its block's extended layout in `finalize_states` (same ordering
  rule as the compact fields). Break-tested: dropping the clear in `write_block`
  turns the replace test and the headless reload test red; recording no bytes in
  `record_edits` turns the reload test red. Janitor additions are in
  `docs/STATE_ACCESS.md`. Nothing *reads* a state yet; a texture or light per
  state value is the obvious next consumer. Not measured: the table's real memory
  or lookup cost at scale (the claim is structural: no entry, no cost).
- **Mods are an overlay, and "the core files are never written" is a tested
  invariant, not a hope.** `mods.rs` (format, discovery, `ModStorage`,
  `resolve_world_mods`, `create_world_with_mods`) + `content.rs`
  (`ContentPaths`, `build_content`/`try_build_content`) +
  `world::apply_world_mods`. Design decisions worth keeping:
  - **Overlay by merge, not replacement.** Blocks/items load through
    `blocks::overlay_by_id`: files sharing an `id` are combined by `merge_json`
    (objects merge recursively, `null` deletes the key so serde's default
    applies), later directories patching earlier ones. That is what lets a mod be
    `{"id": "dirt", "hardness": 20}` and nothing else. Recipes have no id, so a
    same-named file replaces (a new name adds). Textures search mod dirs first
    (`ContentPaths::textures` is highest-priority-first, the reverse of the
    block/item/recipe dirs), so a mod can re-skin. `building_with_mods_never_
    changes_the_core_files` byte-compares every core file before and after.
  - **`catch_unwind` is the validation for mod content.** `try_build_content`
    turns a loader panic (which names the file) into an `Err`; the loaders keep
    panicking for the core game's own files, where a mistake should fail loudly.
  - **A claim I got wrong and the test caught:** "core block ids never move". The
    derived `block@layer` variants are registered after every file's block, so a
    mod block shifts them. Harmless (nothing stores a numeric id - saves,
    snapshots and edits all use names) but the test and docs now say only that
    the blocks *core files define* keep their ids.
  - **Content is per world, rebuilt on entry.** `compile_content` (Startup) builds
    the core game; `apply_world_mods` (`OnEnter(InGame)`, `WorldContentSet`,
    before `EnterWorldSet`) resolves the world's mod folders and, unless both
    they and the loaded set are empty, rebuilds and `install_content`s (registry,
    items, recipes, atlas, icons, tables) and bumps `LoadedContent::generation`.
    `render::setup_render` runs again (`content_replaced`) to re-upload the atlas
    and materials. A modded world is rebuilt on *every* entry so editing a mod and
    re-entering shows the change. Unverified from here: the GPU re-upload and the
    hotbar refresh (`hotbar.set_changed()`), which need a display.
  - **Refusing a world must not let anything save it.** Setting
    `NextState(MainMenu)` from `OnEnter(InGame)` still runs one `Update` frame in
    `InGame` plus `OnExit`. First attempt skipped `enter_world` on refusal, which
    made `stream_chunks`/`freeze_exposed_water` panic on a missing `WorldGen`
    (Bevy 0.16 panics on a failed `Res` validation); chasing those one by one was
    the wrong fix. Instead `enter_world` always runs, and the *writers* -
    `exit_world`, `autosave` - are gated on `ContentOk` (`world_content_ok`).
    Break-tested: ungating `exit_world` overwrites the refused world's save
    (`a_world_with_a_missing_mod_goes_back_to_the_menu_with_a_message` goes red).
    The menu also checks first (`resolve_world_mods` before `NextState`), so this
    is the safety net, and the worlds list greys an unopenable world with the reason.
  - **Storage modes are data on `WorldMeta`** (`mod_storage`, `mods`, `branch_of`;
    all serde defaults, so old worlds are instance worlds with no mods - no
    `SAVE_FORMAT` bump). Instance: `meta.mods` ids from the game folder. World:
    *every* mod folder in `saves/<slug>/mods/` is used (so a hand-dropped folder
    works), and `meta.mods` is what was copied at creation (a missing one refuses).
    Branch: the host's folder, no copy; host must be World-storage and not a branch
    (`create_world_with_mods` checks before creating anything).
  - **Known gaps, on purpose:** no way to change an existing world's mods (turning
    one off would orphan its blocks/items in the save - needs the unknown-block
    data preserved rather than dropped, which `enter_world` currently skips);
    no entities/tiers/worldgen in mods; the texture report is replaced per world
    (`TextureReport::remove`), not merged.
- **Namespaces are a rewrite of references at load time, not a new kind of id.**
  `overlay.rs` owns it. A mod's `mod.json` must have an `id` (`mods::read_mod`
  refuses without one; `scan_all` returns the broken ones with a reason for the
  Mods screen); everything it adds is `ns:name`, and the core game's names stay
  *bare* (`dirt`; `craftmjne:dirt` is just another spelling, `overlay::normalize`,
  also applied in `BlockRegistry::by_name`/`ItemRegistry::by_name`) so no saved
  name or hardcoded `registry.id("water")` changed. Each directory is a `KindDir
  { ns, dir, locals }`; `locals` (`Locals::scan`) is the set of names that mod
  defines, which is what turns a *plain reference* into `ns:ref` (the mod's own) or
  leaves it for the core. `overlay_by_id` qualifies a file's references per kind
  (`qualify_block_refs`: drops, freezes_to, upper, textures, overlay;
  `qualify_item_refs`: texture, tool.tier; recipes inline in `crafting.rs`) *before*
  merging, because each file's plain names mean the mod it came from. A file whose
  id carries another namespace is a patch and must find its target (a mod can
  patch one that loads earlier; the reverse is a clear error). The same id twice in
  one folder is still a mistake (`seen`), distinct from the same id in two sources.
  Textures: `atlas::load_custom_tile_in` - `ns:granite` is `<mod>/textures/blocks/
  granite.png`; re-skinning someone else's name is `<mod>/textures/blocks/<their
  ns>/<name>.png`; a plain `dirt.png` in a mod is the mod's *own* dirt, never a
  re-skin (tested).
  **The conflict manager** (`mods::assign_namespaces`) is a pure function and its
  output is stored on `WorldMeta::mod_ids` (folder -> namespace): clashing newcomers
  become `id_1`, `id_2` in folder order, an existing assignment is *never* changed
  (a block saved as `x_1:thing` must stay that after the other `x` mod is removed),
  and `apply_world_mods` saves new assignments (a branch uses its host's). Worlds
  remember mods by *folder* (`meta.mods`), not declared id, since ids can clash.
  **Rarities are data now** (`rarities/*.json`, `tools::RarityBook`, kept on
  `ItemRegistry` beside the tiers so `tip_for`/`knap_ui` needed no new parameter);
  the `Quality` enum is gone. Tiers and rarities go through the same overlay, so a mod
  can add or patch either. **A packaging bug this turned up:** the release and dev
  workflows never copied `tiers/` or `entities/` (nothing in CI builds a release from a
  test), so a released build would have panicked at startup reading
  `tiers/`. They now copy `tiers`, `rarities` and `entities`, and
  `every_data_folder_the_game_reads_is_packaged_by_the_release_workflows` scans the
  source for every `find_data_dir("x")` and fails if either workflow doesn't `cp -r x`
  (break-tested by deleting the `tiers` line). Add the folder to the workflows whenever
  you add a new `find_data_dir`.
- **Turning a mod off must not destroy what it made: the shelf.** The old
  `enter_world` skipped any block/item name that didn't resolve, so the *next save*
  no longer contained it - toggling a mod would have silently deleted everything
  made with it. `shelf.rs`: `split_unknown(&mut WorldData, ...)` removes unresolvable
  entries (edits, fluid cells, dropped items, known items, inventory/cursor stacks)
  from the loaded data *before anything reads it* and returns them as a `Shelf`
  resource; `merge_into` writes them back into every save (called in `write_save`;
  the shelf rides in `CarriedState` because `autosave`/`exit_world` are at the
  16-parameter limit). Pure functions, so the rules are unit-tested without an app.
  Rules: live wins (a shelved edit/fluid at a position the player has since built
  on is dropped), and inventory stacks can't keep their slot (it may be occupied
  now) so they go in `WorldData::shelved_items` and, once their item resolves again,
  `enter_world` hands them over via `give` with the overflow lying by the player.
  `SAVE_FORMAT` 9 (an older build would write a shelved save back without the
  shelf). **Namespaces are kept for mods that are off** (`assign_namespaces` no
  longer forgets an absent folder) or a re-enabled `x_2` mod could come back as
  `x_1` and its shelved blocks would point at the wrong mod.
  `mods::set_world_mods` is the change operation: instance worlds just record the
  list; in-world worlds *move* a turned-off mod to `mods-disabled/` (a hand-dropped
  copy may be the only one, so nothing is deleted) and bring it back from there or
  copy it from the game's folder; everything is validated before anything is moved;
  a branch refuses. The menu's per-world **Mods** button uses `edit_candidates`
  (installed + the world's own + a ticked "(missing)" entry for a mod that's gone, which
  is how a world that won't open is fixed). Break-tested end to end:
  `turning_a_mod_off_and_on_loses_nothing_it_made` goes red with `merge_into` removed.
  Not covered: entities (mods can't add any) and chunk snapshots (they only ever hold
  generated terrain, which never contains a mod's blocks).
- **`target_ver` is a warning, not a gate, and it's checked against `mods::GAME_VERSION`
  (`env!("CARGO_PKG_VERSION")`).** `mod.json` takes a string or a list (an
  untagged serde enum). `version_matches` treats an entry as dotted numbers: a
  shorter entry covers every longer version with that prefix, `x`/`*` match any
  component. `ModInfo::warning(game)` is the one place the wording lives (no
  `target_ver`, or none matching); `ResolvedMods::warnings` carries them, `apply_world_mods`
  pushes them to `ChatLog` (as an `Option<ResMut<..>>`, since headless apps may lack the
  chat plugin) and the menu shows them via the same method. Don't make it refuse: an
  author may simply not have bumped the line yet. Tests that go red without it:
  the "any one of several will do" and world-warning tests (break-tested by forcing the
  match). A test mod without `target_ver` now shows `(!)` in `edit_candidates`.
- **Controls are `Action`s bound to `Chord`s, and the game asks `Actions`, never `KeyCode`.**
  `keybinds.rs` is pure logic (all tested without an `App`): `Chord` = optional
  Ctrl/Alt/Shift + one key *or* mouse button, parsed from and shown as `Ctrl+Alt+A`;
  `Bindings` = `Action -> Option<Chord>`; `search` (plain words, `key:`, `is:`);
  `Capture` = the "press the new combo" state machine. `Actions` is a `SystemParam`
  (keys + optional mouse + optional `Bindings`) that systems take *instead of*
  `Res<ButtonInput<..>>`: it falls back to the default bindings when those
  resources are missing, which is what lets `ItemsPlugin` run in the headless app
  unchanged. **Adding a control is one row in the `actions!` table plus asking
  `Actions` where it is used**; the screen, search, conflicts and saving follow.
  Things worth knowing:
  - **`Player::step` still reads fixed keys.** Rather than thread bindings through
    its nine tests, `Bindings::movement_input` turns the movement actions into a
    `ButtonInput<KeyCode>` of the default keys (W/A/S/D/Space/Shift/Ctrl) each
    frame - the same trick the overlays already use to hand it an empty input.
  - **Modifiers are "required", not "exact"**, so W works while Ctrl is held. The
    only tie-break is `shadowed`: a held chord with *strictly more* modifiers on the
    same input suppresses the plainer one (`Q` vs `Ctrl+Q`). Identical chords are a
    *conflict* (red in the UI), both fire. Don't "fix" that by silently picking one.
  - **Right-hand modifiers are folded into left-hand ones** (`canon_key`), and a chord
    whose input is a modifier drops that modifier from its flags, so `Shift` is never
    `Shift+Shift`.
  - **Capturing needs a warm-up frame.** The click on "Rebind" is itself a mouse press
    that frame; without `Capture::warmup` it binds Left Click. A lone modifier binds
    on *release* (so `Ctrl` can still become `Ctrl+A`). The settings systems run
    search -> button handler -> capture, in that order, so the click that ends a
    capture isn't also obeyed as a button, and the key being bound isn't typed into
    the search box (all three are covered by break-tested app-level tests).
  - **`key:a` means the A key exactly**, not "starts with a" (that would pull in Alt);
    only a token that isn't a whole key name falls back to a prefix (`spa` -> Space).
  - Saved as *differences from the defaults* (`keybinds.json`, `Bindings::overrides`),
    not a full dump, so a default changed in a later version still reaches players
    who never touched it. Unknown ids / unreadable chords are skipped, never an error.
  - **Not done:** `Action` is a closed enum, so a mod can't add a control yet (it would
    become an id registry like `CommandRegistry`); the Settings screen is only reachable
    from the main menu, not the pause menu; Player and Sound sections are empty shells.
    UI layout is unverified from a session with no display - the app-level tests prove
    the tree builds, clicks, rebinds, searches and saves, not how it looks.
  - `menu.rs` lost its Settings code to `settings.rs` (its helpers went `pub(crate)`).
    The Rebinding screen rebuilds wholesale on change (spawn-on-change), carrying the
    list's scroll offset across in `SettingsView::scroll`.
- **Controls became string-id'd so mods and plugins can add them, and "which controls exist"
  is data on `Bindings`, not the `Action` enum.** `Action` is still the typed handle for the
  game's *own* controls (`act.pressed(Action::Jump)`), but `Bindings` now holds
  `ControlDef { id, label, category, default, command, source }` for everything and every
  method takes `impl Control` (an `Action` or any id string). Mods add controls from
  `keybinds/*.json` (`mod_controls.rs`, read from the **installed** mods folder on startup, when
  Settings opens and when a world loads); plugins call `Bindings::register`. Things to keep:
  - **Ids are `modid:name`, so a mod can't take a built-in id** (no colon in the built-ins),
    and `register` refuses a taken id, first claim wins. Mod controls start **unbound** unless
    the file gives a `default` - never grab a player's key unasked.
  - **A key set for a control that isn't registered right now is parked, not dropped**
    (`Bindings::pending`, included in `overrides()`, re-applied on `register`). Without it,
    uninstalling a mod once and saving anything would silently delete the player's key for it.
    `reset_all` is the one thing that clears the parked ones. Same lesson as the world shelf.
  - **A mod control's `command` runs through the same `CommandEnv` as typed chat commands**
    (a `SystemParam` extracted from `chat_text_input`), so the cheats flag, `/locate` and every
    other command behave identically; `run_bound_commands` only fires when no overlay is open and
    the cursor is grabbed, and only for a mod that the *loaded world* uses (`LoadedContent.mods`,
    matched by folder path). Controls from a world-local mod copy aren't listed - only installed
    mods are scanned; say so if that becomes a problem.
  - **`CommandEnv` extraction means `ChatPlugin` now needs a `Bindings` resource** (it
    `init_resource`s one; `KeybindPlugin` replaces it with the saved one).
- **Controls are saved per launcher instance, keyed by a permanent id - not the name or version,
  both of which change.** The launcher gives every `Instance` an `id` (`new_instance_id`: clock +
  counter, filesystem-safe), starts the game with `--instance <id>`, and the game keeps
  `instances/<id>/keybinds.json` (`SaveStore::keybinds_path`; a game started alone uses plain
  `keybinds.json`). The launcher's instance *name is editable* in its UI - that's why the id exists;
  keying on the name would have lost the controls on the first rename. Details that matter:
  - **An id that is only in memory is invented afresh next launch**, orphaning whatever the game
    saved under it, so `Instances::ensure_ids` (old files have none; duplicate ids from a hand-copied
    entry are split) is followed by an immediate `save` in `LauncherApp::new`.
  - **The id reaches the file system, from an editable file and from the command line**, so both
    sides sanitize it to `[A-Za-z0-9_-]` (`instance_folder_name`, duplicated in `paths.rs` and
    `save.rs` like `app_data_dir` - change both) and the launcher's delete-instance cleanup can't be
    pointed outside `instances/` with `..`. Both have tests.
  - Deleting an instance in the launcher deletes its folder. Unverified: the launcher's egui UI.
- **"Controls" is the settings section; "Keybinds" and "Options" are its two halves, and flight is the first option.**
  `ControlOptions { flight_mode }` (a `Resource`, saved as `controls.json` beside the keybinds so it is per
  launcher instance too; missing/unreadable/unrecognised reads as the default) is *how controls behave*,
  where `Bindings` is *which keys*. `FlightMode::Toggle` flies on the `ToggleFly` control; `DoubleTapJump`
  flies on a double-tap of `Jump` (0.3 s, `DOUBLE_TAP_WINDOW`) and ignores the toggle key - the two modes never
  both act. The decision is one pure function (`ControlOptions::wants_fly_toggle` + `DoubleTap`) called from
  `player_update`, which already knew whether the player is frozen/Creative, so taps typed into chat or made in
  Survival aren't counted. **A double-tap uses both presses up** (`DoubleTap::tap` clears itself), otherwise three
  quick taps would toggle twice. Tests go through the real `player_update` system with a manual clock
  (`flight_app`), not just the pure function - the wiring is the thing that could be wrong. Next options that
  belong here rather than in Keybinds: sprint/sneak hold-vs-toggle, mouse sensitivity.


- **Menus share one theme (`ui_theme.rs`); a screen picks a `ButtonStyle` and never paints
  its own colours.** `ThemedButton { style, selected, accent }` is repainted by one system
  from the pure `button_colors`, so a screen shows a tab/toggle as chosen by flipping
  `selected` (not by recolouring), and `accent` (conflict red, listening yellow) replaces
  the border. Things learned from actually looking at screenshots:
  - **UI alpha blends in linear space**, so a faint white-alpha card reads much lighter than
    the sRGB number suggests (the first cards came out grey). Panels, cards and borders are
    opaque colours; only the full-screen dim (`OVERLAY`) uses alpha, and it was checked to
    darken the world by comparing a `game` shot against a `pause` shot.
  - **A bundle can't hold the same component twice** (Bevy panics): to tint
    `overlay_root()` differently (the death screen) `.insert(BackgroundColor(..))` after
    spawning instead of adding one to the tuple.
  - **`Added<T>` as well as `Changed`**: a text field spawned already focused/filled showed
    blank until typed in, because the sync system only ran on resource change. It now also
    runs for `Added<TextInputDisplay>`.
  - **Menus are inspectable from a session with no display**: `CRAFT_SHOT=<screen>` +
    `CRAFT_SHOT_OUT` (see `main.rs`'s `ui_shot`), run under `xvfb-run` with lavapipe
    (`apt-get install mesa-vulkan-drivers libxkbcommon-x11-0`), then read the PNG. Do this
    after any UI change; layout bugs (tabs wider than their card) only showed up this way.
  - The backdrop (`spawn_backdrop(root, seed)`) is banded sky + stars + three parallax-less
    hill layers from seeded noise, quantised to 8px so it stays blocky; each screen passes
    its own seed so they differ.

- **Menus are textured, not coloured: `gui_skin.rs` owns the images, `ui_theme.rs` picks them.**
  The earlier flat web-style look was replaced because the user wanted rustic wood, stone and
  leather - and wanted texture packs and mods to be able to retexture menus, crafting and
  inventory. So there is no colour to override, only named [`Element`]s (panel, card, button
  and its states, field, slot, plank, background). Facts worth keeping:
  - **Procedural by default, file-overridable by name.** Each element is painted at a
    *logical* size (e.g. button 16x12 "skin pixels") and upscaled 3x (4x for the tiled
    background); `textures/gui/<name>.png` replaces it if it is a whole multiple of that size
    with the same factor on both axes. The slice border is `border * scale`, derived from the
    PNG's own size, so a pack never declares a border. A wrong-shaped or undecodable file is
    skipped (logged), never trusted - same stance as the block-texture `Malformed` handling.
  - **Why paint big instead of letting Bevy scale:** Bevy's `TextureSlicer` draws a corner at
    1 image pixel = 1 screen pixel, so a 4-pixel pixel-art corner would be unreadably tiny.
    Sides and centre use `SliceScaleMode::Tile`, not stretch (stretching a 14px centre across
    a 600px panel is a smear of giant pixels). The sampler is nearest.
  - **The look is resolved at draw time by name** (`Skinned { element, hover, tint }` +
    `apply_skin`, `ThemedButton` + `themed_button_visuals`), not stored in spawn code, so
    spawn functions never need the `GuiSkin` resource (no signature ripple through every
    screen) and replacing the resource restyles everything. Both systems run every frame and
    write only when the image or tint differs. `ThemedButton.accent` is now a *tint* (the
    texture is multiplied by it): a conflicting key is red stone, an unaffordable recipe is
    dim stone.
  - **Mod skins apply game-wide, not per world.** The skin is built once in `PreStartup` from
    the game's `textures/gui/` plus every *installed* mod's, because the menus exist before
    any world (and its mod list) is chosen. So a mod that ships `textures/gui/` retextures
    the menus whenever it is installed, needing a restart. Known simplification.
  - **A bundle can't hold a component twice.** The death screen tints `overlay_root()` by
    `.insert`ing a second `BackgroundColor` after spawn; putting two in one tuple panics.
  - **`textures/gui/README.md` is tested against `Element::ALL`**, so adding an element
    without documenting it fails a test; add the row when adding the variant.
  - World Options is a third `WorldsScreenMode` (`Options`) reached from the create form; the
    mod/branch/storage controls moved there unchanged, and `options_summary` shows the
    choice back on the create form. Game rules get their own section there when they exist.

- **Keyboard navigation is a highlight plus a one-frame fake click, so no screen had to change.**
  `menu_nav.rs`: `MenuFocus` (a resource, not a component on the button) names the highlighted
  `ThemedButton`; `themed_button_visuals` shows it with the hover texture; Enter sets that
  button's `Interaction` to `Pressed` for exactly one frame (released at the start of the next),
  which is what every screen's existing click handler already reacts to. `navigate` is a pure
  function over button centres/sizes (arrows pick the nearest button in that direction, with a
  half-button-height tolerance for "same row"; Tab is reading order; everything wraps), tested
  without an app. Gotchas: the system runs in `PreUpdate` after `UiSystem::Focus` so handlers in
  `Update` see the press; a system can't hold `&mut Interaction` and `Changed<Interaction>` as two
  queries (B0001), so fresh mouse hover is detected with `Mut::is_changed` on the one query; it is
  gated by `nav_active` (main menu, mods, worlds list/options, pause) because the create form and
  Settings type into text boxes, where Space/arrows/Enter mean something else - extend there only
  with that in mind. Keys are fixed, not `Action`s: menus must work before the controls screen
  is ever opened. Not done: scrolling a long list to keep the highlighted button in view.

- **Controllers are a second binding per control, not a third kind of key.** `Bindings` keeps
  `pads: id -> Option<PadButton>` beside `chords`, `ControlDef::default_pad` beside `default`, and
  every keyboard method has a `pad_*` twin (`pad_get/set/reset/is_default/conflicts_with`).
  The Keybinds screen's `Device` switch (`SettingsView::device`) picks which set it shows and edits;
  `conflicting_on(device)` etc. are the device-parametrised forms, and the Controls tab/section label
  counts clashes on *either* device. Things worth keeping:
  - **Saved in the same file under an `@pad:<id>` key.** `@` can't be in a control id (ids are letters,
    digits, `_`, `-` and one `:`), so it can't collide with a keyboard entry or a mod whose id happens
    to be `pad`. An older build treats those entries as unknown ids and *parks* them (that's the
    existing rule), so a downgrade-and-back loses nothing. `@pad:` entries for controls that aren't
    registered are parked and re-applied on `register`, and `set_mod_controls` parks a leaving mod's
    button - the same shelf discipline as keys. `reset_all_keyboard`/`reset_all_pads` are the
    per-device resets the screen uses; `reset_all` does both.
  - **`gamepad.rs` owns `PadButton` (the game's own list, with stable ids and friendly labels) and
    `PadState`.** `read_pads` (PreUpdate) merges every `Gamepad` entity into one resource, so two pads act
    as one and tests just fill the resource. A test that goes through `read_pads` has to *spawn a
    `Gamepad`* (`digital_mut().press(..)`): inserting a `PadState` by hand is overwritten by the
    system the next PreUpdate.
  - **`Actions` asks both devices** (`pressed`/`just_pressed` = key chord OR pad button), and
    `movement_input` adds the pad buttons plus the left stick held past `MOVE_THRESHOLD` as W/A/S/D, so
    `Player::step` still reads only fixed keys. Stick movement is therefore digital (full speed) -
    analog walking speed would need a change inside `Player::step`; deliberately not done.
  - **The right stick turns the camera in `mouse_look`**, gated on the cursor being captured exactly
    like the mouse (so it can't spin the view behind the pause menu), scaled by `delta_secs`, with
    `apply_deadzone` rescaling so look speed ramps from nothing.
  - **Menu navigation reuses `menu_nav`**: the D-pad is a fresh-press direction, the left stick a
    *flick* (`MenuFocus::stick` remembers last frame's direction so holding doesn't repeat), A presses.
    Pause is just `Action::Pause`'s default button (Start).
  - **Two controls were added because the wheel isn't a chord:** `hotbar_prev`/`hotbar_next` (no
    keyboard default, bumpers on a pad). Four existing search tests counted "everything bound" and had
    to learn that two controls are intentionally key-unbound.
  - **`bevy_gilrs` is now on, which needs `libudev-dev` to build on Linux**; `release.yml` and
    `dev-build.yml` install it (the launcher doesn't use Bevy, so its workflow doesn't).
  - **Not done:** the inventory/crafting/knapping screens are mouse-only (needs a virtual cursor or
    slot-grid focus), analog stick speed, adjustable sensitivity/dead zone, a "which controller" choice,
    and showing button glyphs instead of names. The layout is verified from a screenshot; actual
    hardware input is not (no controller here) - the logic is tested through `PadState`/`Gamepad`
    entities, not through `bevy_gilrs`.

- **Stick tuning applies the dead zone once, where the sticks are read, not where they are used.**
  `read_pads` removes the player's dead zone (`ControlOptions::deadzone`, `apply_deadzone_with`) and
  rescales the rest to 0..1 before writing `PadState::left/right`, so movement, menu flicks and
  look all see the same cleaned value and none re-applies a threshold of its own. Look speed and
  invert are applied in `mouse_look` (`look_delta(right, dt, speed, invert)`). The settings are
  integer percentages (`look_speed_pct`, `deadzone_pct`) because `ControlOptions` is `Eq`/`Copy`
  and a float would break that; accessors clamp, so a hand-edited file can't set a zero dead zone
  or a 9999% look speed, and `Default` is written by hand because the derived one would give 0%
  (an old `controls.json` without the fields deserialises through per-field serde defaults).
- **Glyphs are a second small skin, not part of `Element`.** `pad_glyphs.rs` paints 16 pictures on
  a 12x12 grid (x2), keyed by `PadButton`, overridable from `textures/gui/glyphs/<id>.png` (square,
  whole multiple of 12; bad files ignored) through the same installed-mods directory list as the
  GUI skin. A node shows one with `PadGlyph(button)` + `apply_glyphs`; `ButtonOpts::glyph` puts one
  before a button's label. Kept out of `Element` so its README table test and logical-size/slice
  machinery didn't have to learn about 16 non-sliced images.
- **Inventory by controller is the menu highlight generalised, not a second input path.**
  `menu_nav` now highlights any `Button` (inventory slots, recipes, knap cells, Done/Cancel), is active
  while the inventory, knapping or death screen is open, and presses by setting `Interaction::Pressed`
  for a frame - which the existing click handlers already react to. Four additions made that work:
  - **Secondary press.** X sets `MenuFocus::secondary` for the frame; `click_slots`/`craft_clicks`
    treat it as Shift (quick move / craft all). A flag on the resource instead of faking a held Shift
    key, which would stay held if anything went wrong.
  - **Restoring the highlight after a rebuild.** The inventory despawns and respawns its whole tree
    after every click, which used to drop the highlight each time. `menu_nav` remembers the highlight's
    centre (updated at the *end* of the frame so a move followed by a rebuild remembers the new spot -
    the first version stored the previous frame's and the test caught it) and, `RESTORE_FRAMES` later
    (new nodes have no layout yet), re-highlights the nearest button.
  - **Scroll into view.** `scroll_focus_into_view` walks up from the highlighted node to the first
    `ScrollPosition` ancestor and scrolls by the exact overshoot (layout is one frame behind, so it
    never double-applies). Also fixes long lists on the main menus.
  - **No pointer, so things that hang off the pointer hang off the highlight**: the held stack and
    tooltip position (`sync_cursor_ui`), the slot hover texture (`apply_skin`), the tooltip text
    (`track_hovered_item`), and the knap cell under the highlight (`knap_input`, ringed by
    `knap_focus_ring`). A strikes and X taps a knap cell, as well as the bound triggers. B closes the
    inventory / abandons knapping (`inventory_escape`/`knap_input` check `PadButton::East`).
  `menu_nav_for_tests` exists because the real system is gated on app state; the inventory test drives
  it directly alongside the real `click_slots` (break-tested: removing the secondary flag turns it red).

- **Mods and controller bindings: adding was already one process; running was not.** A mod control's
  `default_pad` (in `keybinds/*.json`, parsed by `PadButton::parse`, a bad name is a note and the
  control still loads unbound on a controller) rides on `ControlDef::default_pad`, and the player's
  change is the same `@pad:<id>` override as for the built-ins. The gap that needed fixing: a
  control's `command` ran from `run_bound_commands`, which called `Bindings::just_pressed` directly
  (keys/mouse only) instead of going through `Actions`, so a mod control could be *bound* to a button
  and never fire. It is now `chat::fired_commands`, a pure function over keys, mouse and `PadState`
  (break-tested: dropping the `pad_just_pressed` half turns the test red). General rule: anything that
  asks "was this control pressed" must use `Actions` or ask both halves of `Bindings`; a direct
  `bindings.just_pressed` is a keyboard-only bug waiting for a controller user.
  **Not supported, deliberately:** a mod *changing the default* of an existing control (the game's or
  another mod's) - for the keyboard either; mods only add controls. It would need a priority rule
  between mods and a decision about players who never customised that control (they'd silently get the
  new key), so it was left as a question rather than guessed at.

- **Resource packs are a stack of layers, and "which pack decides" is a different question for looks than for
  physics.** `packs.rs`: game (bottom) < each mod's `resourcepack/` (load order) < the player's enabled packs
  (`resourcepacks/`, top of the Settings > Packs list wins). A mod's art lives *only* in its `resourcepack/`
  (old top-level `textures/ models/ sounds/` aren't read; `legacy_layout_note` warns). Name rule per layer
  (`KindDir::extends`/`file_in`): game and player packs - plain = core, `ns/name` = a mod's; a mod's pack - plain =
  its own, `craftmjne/name` = vanilla. Models (`models.rs`) are boxes in sixteenths with auto-UVs pinned to block
  space (`face_uv`; the first version had the bottom face mirrored and only the planned agreement test with the
  mesher's corner table caught it). **A model is a picture**: `Tables::collision_boxes` is filled only from a
  game/mod pack model with `"collision": "model"` (`may_change_collision`), never from a player pack, even when
  the player's pack draws the block. Culling uses a separate `Tables::occludes` (= opaque, but a model must fill
  its cell to count) so a half-block model doesn't punch a hole in the wall beside it. Collision consumers: the
  player (`ChunkMap::collider_at` -> `Collider::{None,Full,Boxes}`, boxes join the entity AABBs in `move_axis`)
  and targeting (`interact::raycast_voxel_with` + `ray_boxes`; a ray passes a slab's empty half and carries on).
  Items, projectiles, `surface_y`, climb assist still use cell-level `is_solid` - known gap. Break-tested: ignoring
  `Boxes` in `move_axis` turns `a_model_collision_box_replaces_the_blocks_full_cell` red ("stood at 0.001").
  Pack/mod selection is part of what a world's content depends on (`LoadedContent.packs`), so changing it rebuilds
  on the next world entry; GUI/glyph skins are built once at startup, so those need a restart.
- **Sound is data plus synthesis; the engine ships no audio files.** `sound.rs`: events are names
  (`block.<group>.break|place|step`, `item.pickup`, ...); `Block.sound` (the `sound` tag) is the group;
  `synthesize` makes seeded, deterministic defaults (3 variants) encoded to in-memory WAV, `build_sound_set`
  replaces an event's clips with the highest pack layer's files (`path.ext`, `path1..9.ext`, .ogg/.wav, magic-byte
  checked so a mislabelled file is skipped with a note, not handed to the audio thread). `Content.sounds` ->
  `SoundSet` resource -> `SoundBank` (handles, rebuilt when the set changes). Sound paths are *not* namespaced.
  Senders write `SoundEvent`; `interact` (break/place, via the `Content` SystemParam - it was at the 16-param
  limit), `pick_up_items`, and `sound.rs`'s own footsteps/damage/hurt/ui-click systems. Volumes are percent ints
  (`SoundOptions`, saved `sound.json` per instance) so the struct stays `Eq`. Building on Linux needs
  `libasound2-dev` (both workflows install it). Not verified from here: actual audible output (no sound device
  in this environment) - tests cover synthesis, WAV validity, pack lookup/priority and volume maths.
- **Pack scopes: one "what is in force" resource that every consumer watches, not each consumer
  reading the selection itself.** Global packs (`PackSelection`, `resourcepacks.json` per instance) and a
  world's own (`WorldMeta::world_packs`, `SAVE_FORMAT` 10) combine in `packs::effective_pack_paths` (global
  below, world above, a pack on both counted once at the world's position) into `packs::EffectivePacks`,
  refreshed in `PreUpdate` *and* `OnEnter(InGame)` after `WorldContentSet`. The second one matters: without
  it the first frame in a world sees the menu-time list, differs from what `apply_world_mods` just built
  with, and `world::refresh_content_live` would rebuild without the world's packs. Consumers: `gui_skin::
  rebuild_skin`, `pad_glyphs::rebuild`, `sky::refresh_sky` (each remembers the last list in a `Local`, skips the
  first run because its startup system already built from the same list, and swaps assets in place), and
  `refresh_content_live` (rebuild with `loaded.mods` + new packs, `install_content`, mark every chunk
  `dirty`, bump `generation`). `render::setup_render` now runs in `Update` too and, if `ChunkMaterials`
  exists, swaps the atlas *inside* the existing materials instead of making new ones - chunk entities
  hold the old handles. `apply_skin`/`apply_glyphs` already re-resolved images every frame, so the menus
  needed nothing but the rebuild. The panel is `pack_panel.rs` (shared by Settings > Packs and a pause-menu
  overlay, sub-mode `menu::PausePacksOpen`); its pure helpers (`toggle`, `shift`, `list_for`) are tested.
  Break-tested: removing `rebuild_skin` fails `choosing_a_pack_retextures_the_menus_without_a_restart`;
  disabling `refresh_content_live` fails `world_packs_apply_only_in_their_world_and_changes_apply_live_in_game`.
  Not verified from here: how the in-game remesh burst feels with many chunks loaded, and the layout of
  the pause-menu panel (no display).
- **Mods and resource packs are one stack, and "which layer" is separate from "which data order".**
  `load_order.rs` (pure, tested): `Entry::{Mod, Pack, Global}` keyed `mod:`/`pack:`/`global:<folder>`;
  `auto_order` = mods topologically sorted (`mod.json` `load_after` ids + `patches`, found by reading a mod's
  block/item files for `other:thing` ids; folder order breaks ties; a cycle or unknown id can't hang or drop a
  mod), then the world's packs, then global packs on top; `resolve` fits a saved manual order
  (`WorldMeta::load_order`, `Option`, `SAVE_FORMAT` 11) to what exists (new mod just under the globals, new
  global on top, vanished entries dropped); `None` = Auto, which is how "global packs back on top" works. The
  game is the bottom `PackStack::core()` layer and is not an `Entry`, so nothing can move below it by
  construction. `ContentPaths::with_layers(&[LayerSource])` restacks the *art* layers in that order (a mod's
  layer is its `resourcepack/`); `with_mods` still fixes the *data* order. **They deliberately differ in
  `refresh_content_live`**: data keeps `loaded.mods`' order (block ids must not move under loaded chunks)
  while art/models/sounds take the new order at once; `apply_world_mods` applies both on entry.
  `LoadedContent.packs` became `layers`. `EffectivePacks` (pack paths, for gui/glyph/sky) and
  `EffectiveLayers` (full stack, for content) are recomputed in `refresh_effective` only when an input
  `is_changed` (it reads each mod's `mod.json` via `read_mod`, so not every frame).
  **Reload key (Ctrl+R, `Action::ReloadPacks`)**: `PackReload(u64)` counter; `reload_on_key` rescans
  `PackFolder` (a resource, so tests can point it at a temp dir - the first test failed because the rescan
  read the real folder and emptied the injected list) and bumps it. Every consumer remembers the last
  counter beside the last list (`Local<Option<(Vec<PathBuf>, u64)>>`; `LoadedContent::reload` for content), so
  a reload rebuilds even when the list is unchanged, and `apply_world_mods` also respects it so edits made
  in a menu show on the next world entry. Break-tested both: `Some(saved) => return auto` fails
  `mods_and_packs_share_a_load_order_that_decides_who_wins`; `forced = false` fails
  `the_reload_key_rereads_pack_files_from_disk_in_game`. A global pack now ranks *above* a world pack
  by default (previously the reverse) because the player asked for global packs on top; the Load order
  tab lets them swap. UI: the pause menu's pack panel has a third `PackScope::Order`
  tab (`pack_panel::spawn_order`); settings' Packs stays global-only (no world there).
  Disk reminder: this session hit "No space left" three times; `rm -rf target/debug/incremental` plus
  deleting stale `craftmjne-<hash>`/`headless-<hash>` test binaries and old `libcraftmjne-*.rlib` freed ~10 GB.

- **The typeface is a real TTF built in memory, and the fonts plugin only decides which handle each text uses.**
  `font_gen::build_ttf` writes glyf/loca/cmap/COLR/CPAL tables from square pixel contours (em = 12 rows,
  `PIXEL`=100 units, baseline row 10), checked by parsing the output with `ttf-parser` (a dev-dependency) and by
  a real screenshot (`CRAFT_SHOT=fonts`). `pixel_font.rs` draws text, `emoji.rs` + `emoji/` draw ~340 emoji as
  palette-letter 12x12 grids, composed from heads plus eye/mouth overlays. Facts that bit:
  - **Bevy caches a font by handle id**, so a changed font needs a *new* handle; the default is replaced in place
    with `Assets<Font>::insert(&Handle::default(), ..)` and `"default_font"` is off in `Cargo.toml`. A font nothing
    references is not in the cosmic-text database, so the emoji font is registered directly
    (`load_font_source`) as the fallback.
  - **Sizes snap to a multiple of the font's unit** (`snap_size`, 12 for the built-in) with `FontSmoothing::None`
    and a fixed 1.25 line height, in `apply_fonts` (PostUpdate, before `UiSystem::Prepare`). Vector pack fonts
    are not snapped. A `FontTag(String)` (role or name; unknown -> default) is read there; spawn sites only add the
    tag, never a font handle. Roles map through `fonts/fonts.json`; block/item files take a `font` definition tag
    used by the tooltip (`Tip::font`).
  - **(Superseded below: the font now has GSUB ligatures, so flags, skin tones, keycaps and ZWJ sequences work.)** Colour emoji take a text colour tint only as far as the renderer
    does it (not verified beyond the screenshot).
  - **Chat** expands `:name:` through `emoji::expand_shortcodes` only for plain messages, not `/commands`.
  - A glyph defined as base + accent (`COMPOSED`) can't have a descender mark; `ç`/`Ç` are drawn by hand in `BASE`.
  - A screenshot of the preview text is the only way to see a glyph problem from here: `{}` first read as `()` until
    looked at at size 24.

- **More scripts went into the one built-in font: tables for the simple ones, a stroke rasteriser for Hangul and
  Devanagari, and a real `GSUB` for Arabic.** `pixel_scripts.rs` (Cyrillic, Hebrew; Latin lookalikes reuse the Latin
  drawing and accented ones are letter + mark through `pixel_font::compose`), `pixel_hangul.rs` (11,172 syllables
  built from jamo strokes in 11x12 boxes chosen by vowel shape and final consonant), `pixel_arabic.rs`,
  `pixel_devanagari.rs`. The default font is ~3.5 MB and builds in ~65 ms in a debug build. Things that mattered:
  - **A box too small makes distinct letters identical.** At 11 rows, ㄹ and ㅌ in a 3-row box came out pixel-for-pixel
    the same; a probe test counting duplicate syllables named the pair. The cell is the full 12-row em
    (`H = 12`), so the top part of a syllable with a final gets 4+ rows. `syllables_are_almost_all_visibly_
    different` keeps the duplicate count under 2%; same idea for `consonants_are_distinct_shapes`.
  - **Arabic needs `GSUB`, and the way to name a form without a code point is a private-use one.** `font_gen::
    Substitution { script, feature, from, to }` is resolved through the cmap, so callers never see glyph ids;
    `gsub_table` writes one type-1 lookup per (script, feature). Verified with `rustybuzz` (a dev-dependency,
    already in the tree through bevy_text): shaping "بيت" must give initial/medial/final glyphs, "باب" must end the
    connection after a right-joining alef. **Break-tested**: with no substitutions both joining tests go red.
  - **Don't trust zero-advance marks to be positioned by the font - test what the shaper does.** The Devanagari
    marks (ु े ं ् ...) are zero-advance glyphs drawn at negative x. `a_mark_takes_no_room_and_stays_where_it_was_
    drawn` shapes क+mark with rustybuzz and asserts advance 0 and x_offset 0, i.e. the shaper's fallback
    positioning does not also move them. `the_i_sign_is_put_before_its_consonant` checks the shaper's own reordering.
    The existing "glyphs stay in the cell" test had to learn that Devanagari marks reach left of their origin.
  - **Joining strokes live on one row (9) and run to the glyph's edge on every joining side**, so neighbouring forms
    meet; `joining_letters_meet_in_a_continuous_line` checks every letter's initial/medial/final. Forms that don't
    join on a side get a blank column there.
  - **Honest limits, written down in README**: no lam-alef ligature, no tashkeel, no Persian/Urdu, no Devanagari
    conjuncts (they need a GSUB ligature table), Devanagari digits are Latin-shaped, and the Hebrew, Arabic and
    Devanagari art is a best-effort first pass - looked at only in a software-rendered screenshot, not by someone
    who reads those scripts. `CRAFT_SHOT=scripts` draws a sample of every script.

- **GSUB ligatures were the one enabler behind flags, skin tones, keycaps, lam-alef and Devanagari conjuncts.**
  `font_gen::Ligature { script, feature, components, to }` (alongside `Substitution`) writes type-4 lookups,
  longest sequence first per starting glyph; components and `to` are characters, resolved through the cmap, so
  callers never see glyph ids. `emoji::Emoji::sequences` is the list of sequences that produce a glyph; the glyph's
  own `chars` is a private-use code point (flags `0xF0000+`, tones/keycaps `0xF1000+`). Things that bit:
  - **Joiners need a glyph each, not one shared glyph.** `ZERO_WIDTH` used to be one glyph for ZWJ, VS16 and
    U+20E3; ligature components are glyph ids, so a keycap pattern `[digit, 20E3]` would also have matched
    `digit + VS16`. `emoji::joiners()` now yields one zero-advance glyph per character (plus the tag letters
    `U+E0061..=E007A`, `E007F` that subdivision flags are spelled with). `keycaps_and_skin_tones_are_ligatures`
    asserts a stray VS16 is *not* a keycap.
  - **A single-character "sequence" is not a ligature** (needs 2+ components): `flags::flag` maps those characters
    straight to the glyph. Found because Texas shaped as `[notdef, ...]`: U+1F3F4 itself had no glyph.
  - **A glyph's character list must not include a joiner.** The first skull drawing listed `U+2620 U+FE0F` and
    silently took U+FE0F from the joiner glyph (first drawing of a character wins).
  - **Flags are a drawing DSL, not 258 grids.** `flags.rs`: `F::h("rwb")`, `.d(..)` disc, `.cross(..)`, `.tri(..)`,
    `diag(..)`, `quad(..)`, `uj(w,h)` Union Jack; `design(code)` is one match arm per code. A test requires every one
    of the 258 codes to have a design and that no two *different* countries look identical (a short allow-list of
    flags that really are identical, e.g. the French territories, Norway/Svalbard, Indonesia/Monaco). That test caught
    nine British territories sharing one ensign. Emblems are marks, not coats of arms.
  - **The Texas flag has no emoji.** It uses the tag sequence its subdivision code would have (U+1F3F4, tags `ustx`,
    cancel tag) and the shortcode `:flag_texas:`. It is drawn as the Lone Star flag (blue bar with a white star, white
    over red), which was also the Republic's national flag; say so if the Burnet or another historical design is meant.
  - **Shaper behaviour worth testing instead of assuming (rustybuzz):** Arabic and Hebrew marks (ccc != 0) are
    centred over their base by *fallback positioning*, so their art doesn't need to be placed; Devanagari marks with
    ccc 0 are *not* moved (x_offset 0). `rlig` lam-alef matches the *joined form* glyphs (`[lam_initial, alef_final]`),
    not the base letters. With `dev2`/`deva` in the GSUB, `half` ligatures on `[consonant, virama]` produce half forms
    and `rphf`/`blwf` give reph and the below-base ra, without any other lookup. All of this is in tests that shape
    real text.
  - **The gun is a water pistol** (`water_pistol`, U+1F52B), following the current standard; there is no `gun` emoji.
  - Not done: family/couple/profession ZWJ sequences, most objects and people emoji, Devanagari digits, the special
    क्ष/ज्ञ ligature shapes, stackable conjuncts for stemless consonants, Arabic `calt` variants.


- **"Every single emoji covered" is a different promise from "every emoji drawn", and the
  honest version is measured.** `src/emoji/data/standard.txt` (the fully-qualified rows of the
  official `emoji-test.txt`, `group|code points|name`) is the checklist; the test
  `every_standard_emoji_shows_as_one_real_glyph` shapes each row with rustybuzz and demands one
  visible glyph and no `.notdef`. Measured before this work: 760 of 3,963 passed. Now all do,
  but 2,171 are *labelled tiles* (`Emoji::placeholder`), not art - say so, don't call them
  drawn. `standard::all` decides per entry, in this order: recolour an existing picture (skin
  tone), draw a person scene by rule, put a badge on a drawn emoji, else a labelled tile.
  Facts that mattered:
  - **Processing order is part of the algorithm.** Untoned singles, untoned sequences, toned
    singles, toned sequences - a toned variant recolours the untoned art, a profession needs its
    prop's art, so those must already be in `known`. Glyph code points (`BASE + index`) are
    assigned in *list* order afterwards so they stay stable.
  - **The fully-qualified list omits the component emoji** (hair colours U+1F9B0-3), so the
    `man: red hair` rows silently had no ligature (a missing component glyph makes `font_gen`
    skip the ligature). The test caught it (72 failures); the four components are generated
    explicitly. Check "component" rows whenever a sequence fails to shape.
  - **A single code point never becomes a ligature** (needs 2+ components): a one-char row is a
    plain `cmap` entry, and `U+FE0F` after it is the existing zero-advance joiner glyph.
  - **Thousands of ligatures overflow a GSUB subtable.** Offsets are 16-bit, so
    `font_gen::gsub_table` now splits a ligature lookup into subtables of ~20 KB, keeping each
    starting glyph's whole set (longest first) in one subtable.
  - **The labels use a local 3x5 capital font**, because the pixel font's letters are 6 wide and two
    do not fit in a 12-pixel tile.
  - **Placeholders are listed, not hidden**: `/emoji-report` (also `/emojireport`) prints the counts
    and the first 30 names, in the same colours as `/texture-report`.
  - Not verified: the people sprites were only looked at in a software-rendered contact sheet
    (`CRAFT_EMOJI_SHEET=<png> CRAFT_EMOJI_CROP=<0..1> cargo test --lib emoji::tests::sheet`).
    They are small and plain; treat them as a first pass.

- **Chinese and Japanese came from a full bitmap font, because nothing hand-drawn reaches 21,000
  ideographs - and the data source decided the design.** GNU Unifont (SIL OFL 1.1) is complete for the
  BMP and ships three glyph sets (default/Simplified, `_t` Traditional, `_jp` Japanese; 9,665 shared
  ideographs differ between default and Japanese, 26,407 between default and Traditional). `tools/
  build_cjk_data.py` keeps only the CJK-related ranges as one base blob plus two *overlays that hold
  only the glyphs that differ* (1.6 MB total, `include_bytes!`, so no packaging step to forget -
  the notice in `third_party/unifont/` ships via the workflows' `cp -r third_party`). Facts that
  mattered:
  - **16 pixels does not fit a 12-row em, so the glyphs hang above it by design.** `cjk.rs` places
    row 0 at `BASELINE_ROW - 14` and uses the same `PIXEL` unit, so at Latin size 12k the CJK is 16k
    screen pixels: integer scaling for both, 4/3 the Latin height. Scaling it into 12 rows would have
    blurred 1-pixel strokes away. The cost is real: lines overlap, so `fonts::line_height_for` gives
    text containing CJK a 1.5 line height (1.25 otherwise).
  - **Unifont cannot make "all of Han" complete.** BMP is complete (tested against the Unicode ranges
    per style), Extensions B-I are ~8,000 of ~70,000. Say so; don't claim full Han coverage.
  - **The CJK font must not define anything the emoji font or the joiners own.** `glyph_specs` skips
    every emoji character and `emoji::joiners()` - the symbol blocks (arrows, dingbats) overlap emoji,
    and a ZWJ or ☀ resolving to the wrong face would split an emoji sequence across two fonts.
    `the_cjk_font_never_takes_over_an_emoji_or_a_joiner` guards it. Format characters (U+2000-200F,
    2028-202F, 205F-206F) are not in the ranges for the same reason.
  - **Build ~40,000 glyphs off the main thread.** `fonts::update_cjk` spawns the build on
    `AsyncComputeTaskPool`, polls with `block_on(poll_once)`, registers the face as a fallback beside
    the emoji, and re-registers (removing the old face) when `CjkStyle` changes. The app-level test
    waits for the task and checks the swap (and the old face's removal; break-tested by keeping it).
  - **Pinyin and romaji marks were a Latin gap, not a CJK one**: macron/caron accents and the four
    stacked ü tones were composed in `pixel_font.rs`. At this size caron and breve are the same
    three pixels. Capital Ǖ Ǘ Ǚ Ǜ are not made (no room above a capital for two marks).
  - Not done: a language setting (translations will own it), vertical writing, input method, and
    judging legibility of the smallest ideographs at 1x - checked only in software-rendered screenshots
    (`CRAFT_SHOT=cjk`).

- **Cherokee, Cree, Inuktitut, Icelandic, Yup'ik and Ndyuka: sort the request by what each script
  actually needs before choosing a method.** Measured first (CLDR exemplar letters, Wikipedia's
  orthography tables, a probe test listing which letters the Latin font lacked), and it split three ways:
  - **Syllabaries with hundreds of signs (Cherokee, Canadian syllabics)** go through the same Unifont
    pipeline as CJK: `tools/build_cjk_data.py` writes `cjk/syllabics.bin` (896 glyphs, 8 KB deflated),
    `syllabics.rs` builds the font, `fonts::load_syllabics` registers it in the background (not
    style-dependent, unlike CJK). `cjk::{decode, specs_from, font_from}` were made `pub(crate)` so both
    modules share one decoder/font builder. Unifont turned out to be *complete* for both blocks (96+80
    Cherokee, 640+80 UCAS), checked by a test against the assigned ranges. Note CLDR's Cherokee main set is
    the *lowercase* AB70.. letters, easy to miss if you only test U+13A0-13FF.
  - **Latin languages** need almost nothing: Ndyuka's Latin alphabet is plain Latin (+ acute), so the real
    gaps were Icelandic ð þ Ð Þ, Yup'ik U+02BC and the combining tie U+0361, and Inuit ł ŋ. Those are drawn
    in the 12-row Latin style, not taken from Unifont (a 16-pixel ð between 12-pixel letters would look
    wrong). The tie is a zero-advance mark reaching both sides of its origin, so
    `pixel_scripts::point_glyph` (which assumed marks only reach left) was generalised to min..max x, and the
    glyph-cell test allows a zero-width mark to reach 4 pixels right.
  - **Afaka and Yugtun are not in Unicode** (Afaka's U+16C80-16CCF is only tentative), and Wikimedia
    rate-limited me when I went for the chart. Drawing 56 signs from memory would be invented, so they are
    reported as not done rather than approximated. If they are wanted, the decision is whether to use the
    tentative code points or private-use ones, and who supplies reference shapes.
  - **A screenshot window is 1280x720**: a sample screen with too many 24-pixel lines silently cuts the
    last ones off (the Yup'ik line was missing from the first shot). Give a script group its own
    `CRAFT_SHOT` screen (`natives`) instead of appending lines.
  - Break-tested: the app-level fallback test needs the plugin's `load_syllabics` to run; the CJK swap test
    still guards the shared registration path.

- **Ten more scripts: the question was which ones belong in the game's own style and which need Unifont,
  and "joining" had to be answered by the shaper, not by guessing.**
  - **Greek is the one that earns hand-drawing** (`pixel_greek.rs`): it is running text next to Latin and
    Cyrillic, most capitals are Latin shapes (`LOOKALIKES`), and it needs only ~10 new capitals and ~25 new
    lowercase. Everything else (Armenian, Georgian, Syriac, Mandaic, Coptic, Ethiopic, Tifinagh, N'Ko, Vai,
    polytonic Greek) comes from Unifont: measured first, it covers every assigned code point of all of them
    (`every_assigned_character_of_every_promised_script_has_a_glyph`). One blob, `cjk/scripts.bin` (2,037
    glyphs, 20 KB), one fallback font (`scripts.rs`). `fonts::load_bitmap_fonts` replaced `load_syllabics`
    and builds both bitmap fonts in the background; line height keys off `scripts::is_bitmap_script`.
  - **The blob format grew two flags per glyph** (`width` byte: bit 7 combining, bits 4-5 joining), computed
    in `tools/build_cjk_data.py` from `unicodedata` and Unicode's `ArabicShaping.txt`. All blobs were
    rebuilt. Combining marks (Unifont draws them in the cell *right of* the letter they belong over) get
    advance 0 and are shifted left by their own width (`cjk::spec_of`); the shaper's fallback positioning
    then centres marks with a combining class over the base, as it did for Hebrew and Arabic.
  - **Syriac, Mandaic and N'Ko join, and Unifont has only isolated shapes**, so `scripts::forms` derives
    them: the letter plus a stroke on its lowest ink row (capped at the baseline) from its outermost ink to
    the cell edge, on the joining side(s). `fina` = stroke on the right, `init` = left, `medi` = both (right
    to left); right-joining letters (Alaph, Dalath, Waw ...) get only `fina`. They are `GSUB` single
    substitutions under `syrc`/`mand`/`nko ` through the existing `font_gen::Substitution`. It is a
    heuristic, called one in the docs.
  - **rustybuzz returns right-to-left text in visual order.** The first cursive test compared glyphs in the
    wrong order, "failed", and looked like a shaping bug; printing the glyph ids for a few strings showed the
    shaper was right and the test was backwards. `cursive_letters_take_the_form_...` now reverses to typing
    order and checks the exact expected form ids; break-tested by emptying `substitutions()`.
  - **The CJK app test's registration path is shared**: `the_bitmap_script_fonts_are_registered_...` waits for
    both faces and checks glyphs from each script.
  - Not done / said plainly: "Val" in the request is Vai; Syriac's variant finals and ligatures; Greek
    capital tonos to the left; the joined forms and every Unifont script judged by eye only in a
    software-rendered screenshot. Afaka and Yugtun are still unencoded (see the earlier entry).

- **Thirty-odd more scripts went into the same blob, and the lesson was what Unifont's "complete" does and
  does not buy.** `scripts.bin` is now 7,050 glyphs (78 KB deflated): measured first (every assigned
  code point of every block Unifont-covered, `PROMISED` in `scripts.rs` was generated from `unicodedata`),
  then the new ranges were added to `tools/build_cjk_data.py` and the blobs rebuilt. Things that mattered:
  - **"Every character has a glyph" is not "the script works".** The screenshot showed it plainly: Thai and
    Lao shape into real clusters (marks are Mn, zero-advance, drawn where the shaper puts them), but the
    Indic scripts, Khmer, Burmese and Tibetan show conjuncts as separate cells because Unifont's OTF gets
    those from its own GSUB/GPOS, which this pipeline does not carry. The README says so per group; the
    fix would be per-script conjunct drawing (what Devanagari got), not more data.
  - **Mongolian joins in principle, but its glyphs are upright vertical-script letters**, so deriving a
    baseline stroke (as for Syriac) would have been nonsense. `forms()` was restricted to Syriac/Mandaic/N'Ko
    through `is_cursive_script` instead of "anything with a joining type"; before that restriction the blob's
    joining flags (from `ArabicShaping.txt`) would have given Mongolian forms it cannot use.
  - **A distinctness test over a generated family caught a real bug.** `no_two_vietnamese_letters_look_alike_
    and_none_looks_like_its_base` failed for ỵ: the dot-below beside a descender landed on a pixel the "y"
    tail already inks, so ỵ was identical to y. Fixed by picking the nearest free pixel on the other side.
    For any family built by composition (accents, marks), test every member against every other and each
    against its base.
  - **Vietnamese capitals have two rows above them, and a shape mark plus a tone mark need four**, so
    capitals set them side by side (`pair_above`) while lowercase stacks (`compose` twice, like ǖ). The
    dotless i is the base whenever a mark goes above an i.
  - **Latin Extended was deliberately not added as a Unifont fallback** (it would give Polish/Czech/Turkish
    letters at the cost of 16-pixel letters among 12-pixel ones, and nobody asked); it is one range list
    away in `build_cjk_data.py` if wanted.
  - **The preview window is 1280x720**: asia1/asia2 are two screens for that reason (see the `natives`
    entry); wrapped lines are normal.

- **"Make Mongolian look right" needed a renderer, not a font, and the data dictated the algorithm.** The
  game's `Text` only flows horizontally, and Unifont draws each Mongolian letter once, upright, with a
  vertical spine (usually column 3) - so no GSUB trick could join them. `mongolian.rs` is pure bitmap work
  (`render(text, max_height) -> Bitmap`) behind a `VerticalText` component whose system
  (`build_vertical_text`, registered from `FontPlugin`) turns the bitmap into an `Image` + `ImageNode`
  sized with `Node`. Decisions that mattered:
  - **Forms by deleting, not drawing.** From the isolated glyph: find the spine (most-inked column, its
    longest vertical run); the head is the ink above the run and the hook left of its top two rows, the
    tail is everything below it. Initial = no tail, medial = neither, final = no head, isolated = whole;
    joined ends get a one-pixel spine stub so neighbours abut. Crop each piece to its rows, align spines,
    stack. A real Mongolian font draws every positional form of every letter; this is an approximation and
    is documented as one.
  - **Test the seams, not the whole spine.** The first tests asserted "spine inked on every row" and
    failed twice for the right reasons: the head sits *above* the spine, and Unifont's NA has a detached
    tick *below* it. The seam between letters is what joining means, so `a_word_is_one_unbroken_spine_...`
    now checks the spine is inked on both sides of each seam, using the same `piece()` heights.
  - **Columns left to right, one spine line per column** (every word in a column is aligned to the widest
    word's spine), wrapped at a pixel height; `scale` is an integer so pixels stay square. The system takes
    `Option<ResMut<Assets<Image>>>` so `FontPlugin` still works in the headless font tests.
  - **Joining flags had to be ignored for the wrong cases**: Mongolian is flagged dual-joining in the blob,
    but only `mongolian.rs` may use that - `scripts::forms()` stays restricted to Syriac/Mandaic/N'Ko
    (`is_cursive_script`), or it would derive horizontal baseline strokes for a vertical script.
  - Not done: contextual letter rules, FVS alternate glyphs, rotated digits, Phags-pa (vertical but
    non-joining - the same column layout would serve it), and any use in the game's own UI.

- **"Do the joined scripts actually flow together in game" was two questions: does the real text engine pick
  the joined forms, and does every script that should join have forms at all.** A test that shapes with
  `rustybuzz` alone proves the font's GSUB; it does not prove the engine Bevy actually uses (cosmic-text)
  chooses fonts and runs the same way. So `fonts.rs` now has engine-level tests: a `FontSystem` holding only
  the game's own fonts (no system fonts, so the result is deterministic), a `Buffer` set with the default
  family and `Shaping::Advanced`, and the glyph chosen for each character read from `layout_runs`. The
  assertion is the one that matters: *inside the word* each letter's glyph differs from the glyph the same
  letter gets alone, and the three positions give three different shapes. It covers Arabic, Persian,
  Syriac, N'Ko, Mandaic and Mongolian, plus lam-alef (one glyph), the Devanagari ksha conjunct (two glyphs
  for three characters), a right-joining alef not joining forward, and a joined word keeping its forms when
  Latin and other scripts surround it. Facts worth keeping:
  - **The answer was "no" for Mongolian in ordinary text.** It joined only in the `VerticalText` widget; a chat
    line was still separate upright letters. Fixed by also giving the font *rotated* Mongolian forms
    (`scripts::mongolian`): `mongolian::piece` forms turned a quarter turn counter-clockwise (the top becomes
    the left), the spine rows aligned (`MONGOLIAN_SPINE_ROW`), joined forms with no advance gap so their
    strokes meet, registered under script tag `mong` as `init`/`medi`/`fina` GSUB substitutions. Upright
    stays the widget's job. `Piece` and `piece()` became `pub(crate)` for it.
  - **System fonts are a variable this does not control.** Bevy's `CosmicFontSystem` also loads the machine's
    fonts, so on a player's machine a script the game's fonts lack may be drawn by a system font (which
    may join correctly in its own style). The tests deliberately use a database with only the game's fonts.
  - **Scripts that do not join in-game, said plainly**: Tamil and Sinhala (visible pulli / al-lakuna is
    their normal writing), Sundanese, Batak, Buginese and the Philippine scripts. The rest were done next (see below).

- **"Make the scripts that should fit together fit together": Unifont had nothing to extract, so the forms are
  derived, and a screenshot corrected the first derivation.** The question was whether Unifont's OTF carried the
  conjunct rules; dumping it with fontTools showed `GSUB` is *empty* (only `thai` in GPOS), so it could not be
  translated into `font_gen::Ligature`s. `conjuncts.rs` is a declarative table (`SCRIPTS`: script tags, feature(s),
  virama, consonant ranges, `Kind::{Half, Below}`, optional `only` list and reph ra) plus one generic `derived()`.
  - **Half** = consonant + virama -> the ink box squeezed to half width; **Below** = virama + consonant -> a 2x2-merged
    quarter-size letter, zero advance, centred under the previous 16-wide letter at em row 10; **reph** = ra +
    virama (`rphf`) -> quarter ra over the line. Registered under both the new and old script tags (`bng2`/`beng`)
    and, for Khmer, `pref`+`blwf`+`pstf`. Derived glyphs are plane-15 private use from `0xF4000`.
  - **Squeeze the ink, not the cell.** The first version squeezed the whole 16-pixel cell; Gujarati/Odia/Malayalam
    letters sit narrow inside it, so the half forms came out two pixels wide. Cropping to the ink box first
    (`trim_columns`) fixed that. Only a screenshot showed it (and showed that plain Gujarati is just as small in
    Unifont, i.e. not a regression).
  - **Tibetan needed no ligatures.** Its subjoined letters are code points; Unifont draws them in the *same cell*,
    overlapping the letter, so they are only lowered (`TIBETAN_DROP`).
  - **Tested through cosmic-text**: three characters must shape to two glyphs for every script, the virama's own
    glyph must be gone, a word-final virama stays visible, Sinhala/Tamil stay three glyphs. Break-tested by
    emptying `ligatures()`. HarfBuzz's Indic/Khmer/Myanmar/USE shapers all accepted plain type-4 `[C, virama]` /
    `[virama, C]` ligatures under `half`/`blwf`; no base-finding data was needed.
  - Not verified: how the stacks look to a reader of each script (software-rendered screenshot only), and line
    spacing under tall stacks (subscripts reach ~6 rows below the baseline, into the 1.5 line height's slack).

- **Autosave is a second file, not a second save of the same one.** The old autosave rewrote `data.json`
  every 30 s, so a crash mid-session left whatever the last write was and a clean close and a crash were
  indistinguishable on disk. Now `SaveKind::Real` (`data.json`: leaving the world, and `emergency_save` on
  `AppExit`, a `Last` system gated on being in-game with content ok) and `SaveKind::Autosave`
  (`autosave.json`, background, every `AutosaveOptions::interval_secs()`, default 5 minutes, 1-60,
  `saving.json` per instance). Recovery is `SaveStore::recover_autosave`: a snapshot whose mtime is
  *strictly newer* than `data.json`'s (or with no `data.json`) is promoted to the real save and deleted;
  older or unparseable is deleted. It runs in `open_world` (before migration reads the save) and again in
  `enter_world`, because the headless tests and any other entry skip the menu.
  - **The race that matters is a stale background write landing after a real save.** It would be newer than
    `data.json` and a later "recovery" would silently roll the world back. `persist` numbers every write;
    only real saves advance `LAST_REAL`, a snapshot older than the last real save is dropped, and a real
    save deletes the snapshot while it holds the same lock. (A real save can't be reordered by a snapshot:
    snapshots don't bump the counter.) Not tested at the race level - timing-dependent; the headless test
    only checks the end state after a real save with a write possibly in flight.
  - **`OnExit(InGame)` doesn't run on app exit** - that is why the window close needed its own system.
    `AppExit` is written in `Update` (window-close -> `exit_on_all_closed`) and read in `Last` the same frame.
  - **Bevy clamps a frame's delta to 0.25 s**, so a test can't "wait five minutes" by setting a big
    `ManualDuration`: `jump_clock` also raises `Time<Virtual>`'s `max_delta`, runs one update and restores
    `TimeUpdateStrategy::Automatic`.
  - Break-tested: removing the `emergency_save` registration, the real save's `delete_autosave`, or the
    recovery call in `enter_world` each turn their own test red. Settings tabs were narrowed (90/170 px)
    so seven fit the panel; checked in a screenshot.

- **Second conjunct round: the odd cases were each a different mechanism, and a probe-first pass found which.**
  Probing the real engine before writing anything (a throwaway test printing glyph ids per cluster) showed:
  - **A joiner needs a glyph in the font that holds the rest of the sequence.** Sinhala (`virama ZWJ C`) and
    Malayalam chillus (`C virama ZWJ`) left the joiner as glyph 1 in another font, splitting the run. `derived()`
    adds an empty zero-advance ZWJ glyph; `scripts::glyph_specs` otherwise skips joiners on purpose (the emoji font
    owns them) and ligature components that have no glyph are silently dropped by `font_gen`.
  - **Shapers order the vowel sign and the subjoined letter differently** (Khmer/Tibetan: letter then sign; Kannada
    put the sign first), so `lowered_vowels` registers `[sub, mark]` and `[mark, sub]` under `blws`/`psts`.
    The below-signs are found from the bitmaps (combining and ink starting in the lower rows), not listed.
    Kannada's signs turned out to be spacing marks in Unifont (not combining), so nothing overprints there and
    they get no ligature. Tibetan's default shaper has no `blws`, so it uses `ccmp`.
  - **Scripts with nothing to join were checked, not decorated.** Buginese reorders its e sign by itself (USE),
    Philippine kudlits are zero-width marks; no conjuncts were invented for Batak/Buginese/Philippine, and the
    test says what was verified.
  - **Chillu = a ligature whose output already has a code point**: `Ligature.to` is resolved through the cmap, so
    it points straight at U+0D7A..0D7F with no new glyph; the test asserts the glyph id equals the atomic letter's.
  - Break-tested: removing the ZWJ glyph, the lowered-vowel pass, or ya-phala each turns its tests red. A
    pre-existing test that used Tamil ksha as its "untouched" example had to change, since ksha is now a ligature.
  - **Found, not fixed (unrelated):** in the engine-level test setup an emoji ZWJ sequence that follows other
    text on the same line (`"a 👨‍👩"`) comes out as separate glyphs (5, not 3), with or without the scripts
    font's new ZWJ; the same sequence on its own is one glyph. It looks like cosmic-text's per-word fallback
    handing the joiner and the emoji to different faces. Worth its own look before relying on emoji ZWJ sequences
    inside chat lines.
