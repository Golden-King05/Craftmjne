# Craftmjne — read this before doing anything else

This is a **Rust + Bevy native game** (see `Cargo.toml`, `src/*.rs`). It is a
complete, working, well-optimized voxel engine framework with procedurally
generated 16x16 textures, chunked async terrain generation/meshing, physics,
a main menu with per-user saves, a Windows installer, and a self-updater.
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

- `cargo run --release` to play; `cargo test` to run the test suite.
- `src/` is organized as Bevy plugins — one file per plugin/subsystem
  (`world.rs`, `player.rs`, `terrain.rs`, `mesher.rs`, `atlas.rs`, etc.).
  `main.rs` just assembles them.
- Building the Windows installer: see README's "Building the Windows
  installer yourself" section (`rustup target add x86_64-pc-windows-gnu`,
  cross-compile, then `makensis` against `installer/craftmjne.nsi`).
- Releases are cut by tagging `vX.Y.Z` (matching `Cargo.toml`'s `version`)
  and pushing the tag; `.github/workflows/release.yml` builds and publishes
  binaries + the installer automatically.

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
  edits.** `ChunkMap::set_block` fires a `BlockSetEvent` (which `record_edits`
  persists to the save file) — a per-tick simulation writing through it would
  bloat every save with thousands of transient cells. Simulated writes get
  their own setter (`ChunkMap::set_fluid_cell`) that updates the grid + marks
  chunks dirty for remeshing, same as `set_block`, but skips the event.
- **A block's per-cell dynamic state (beyond its id) lives in a second
  `Vec` parallel to `Chunk::blocks`**, not packed into the `BlockId` or a
  separate side-table keyed by position. `Chunk::fluid_level: Option<Vec<u8>>`
  mirrors `blocks` exactly (same index, same lifecycle — both `Some` the
  moment generation finishes, both copied together in `build_padded`). Reuse
  this shape for any future per-block runtime state (growth stage, charge
  level, etc.) rather than inventing a `HashMap<IVec3, T>` side-channel.
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
