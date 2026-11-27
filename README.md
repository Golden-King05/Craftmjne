# Craftmjne

A well-optimized 3D Minecraft-style voxel game, built in **Rust + Bevy** as a
**framework** you can expand. Every block texture is procedurally generated
by default — the project ships zero image assets — but any tile can be
swapped for real art (16×16 up to 64×64) by dropping a PNG in
`textures/blocks/`, no code changes needed (see "Use real texture files"
below).

> This is the native rewrite of the original JavaScript/Electron prototype
> (still available in git history). Same architecture, now with real threads,
> no GC pauses, and Bevy's ECS as the extension model.

## Install (Windows)

Download `CraftmjneSetup.exe` from the [latest release](../../releases/latest)
and run it — no admin rights needed. It installs to
`%LOCALAPPDATA%\Craftmjne`, adds Start Menu and Desktop shortcuts, and
registers a normal uninstaller in "Add or remove programs".

That installs the **launcher**, which is what you'll open from then on. It
lists every published version, downloads the ones you want (once — they're
kept), lets you keep several as named instances, and updates itself. You
never need to re-run the installer. See "The launcher" below. New game
builds are published by CI whenever a `v*` tag is pushed
(`.github/workflows/release.yml`), for Windows, Linux, and macOS.

## Build from source

```bash
cargo run --release
# options:
cargo run --release -- --seed 42 --render-distance 10
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

The installer ships the launcher, so that's what gets built here.

```bash
rustup target add x86_64-pc-windows-gnu   # once
cargo build --release --target x86_64-pc-windows-gnu -p craftmjne-launcher
makensis -DAPP_VERSION=1.0.0 \
         -DSRC_EXE="$(pwd)/target/x86_64-pc-windows-gnu/release/craftmjne-launcher.exe" \
         installer/craftmjne.nsi
# -> CraftmjneSetup.exe in the repo root
```

### Controls

The mouse locks automatically the moment you enter a world — no click needed.

These are the **defaults**; every one except the mouse wheel and a few
screen conventions can be changed in **Settings → Controls → Keybinds** (see
[Rebinding controls](#rebinding-controls)).

| Input | Action |
|---|---|
| W A S D | move |
| Space | jump / swim up / fly up (Creative only) |
| Shift | fly down (Creative only) |
| Ctrl | sprint |
| F | toggle fly mode (Creative only; see *Flight mode* below - or double-tap Space instead) |
| Left / right click | mine (hold, in Survival) / place block |
| Middle click | pick targeted block (Creative only) |
| 1–9 / mouse wheel | hotbar selection |
| Q / Ctrl+Q | drop one of the selected item / the whole stack |
| E | open/close inventory |
| T | open chat |
| / | open chat with `/` already typed (command shortcut) |
| Ctrl+A / Ctrl+C / Ctrl+V | select all / copy / paste, while chat is open |
| F3 | debug overlay |
| Esc | pause (Resume / Quit to Menu / Quit Game); Esc again resumes; also closes the inventory/chat if one is open |

Swimming toward a shoreline climbs you out automatically — no need to hold
Space and wait for buoyancy to slowly float you up over the edge. Moving
into a wall while submerged (`player::Player::assist_climb_out`) scans up to
`MAX_CLIMB_HEIGHT` blocks above your feet for an opening with headroom to
stand in; if one exists within reach, `vel.y` gets a steady upward floor
until you actually climb out onto it. Bounded by `MAX_CLIMB_HEIGHT` so it
stays "climb out of a pool," not a general climb-any-cliff exploit.

## Menus and saved worlds

The game boots into a main menu (**Worlds**, **Settings**, **Mods**, **Quit
Game**) rather than straight into a world. **Worlds** lists your saves and has
a **Create World** button (name, optional seed, and a **Survival / Creative**
game mode picker) at the bottom, Minecraft-style; clicking a saved world loads
it. **Settings** has sections - Player, Rendering, Sound, Keybinds and Mods -
of which Rendering (render distance, applied on next launch), Keybinds and
Mods (where new worlds keep their mods) do something today and Player and
Sound are placeholders. **Mods** lists the installed mods.

### Keyboard navigation

Menus work without a mouse: **arrow keys** move the highlight (Up/Down between rows,
Left/Right along a row), **Tab / Shift+Tab** step through in reading order, **Enter**
presses the highlighted button. It applies on the main menu, worlds list, world options,
mods and the pause menu. Moving the mouse onto a button hands the highlight back to the
mouse. (Typing screens - the create-world form and Settings - are left alone for now.)

### How the menus look, and retexturing them

Every screen (main menu, worlds, create, mods, settings, pause, death, and the
inventory, crafting and knapping screens) is drawn from textures: dark oak boards,
stone-slab buttons, leather cards, dirt behind it all. `src/gui_skin.rs` paints them
procedurally at startup, so the game needs no art files, and `src/ui_theme.rs` decides
which texture each part of a screen uses.

**Retexturing.** Put `textures/gui/<name>.png` next to the game, or in `textures/gui/`
inside an installed mod, and it replaces that element (later wins: the game, then mods
in folder-name order). PNGs are a whole multiple of the element's size, so a texture pack
can be hi-res; a bad file is ignored. `textures/gui/README.md` lists every element, its
size and border. (Texture-pack folders will use the same layout.)

**World Options.** The create-world screen has a **World Options** button: mods (what
used to sit on the create form) live there now, and game rules will join them. An
existing world's **Options** button opens the same place.

To see a screen without playing, run with `CRAFT_SHOT=<screen>` and
`CRAFT_SHOT_OUT=<file.png>`: it opens that screen, saves a screenshot and exits.
Screens: `main`, `worlds`, `create`, `world-options`, `mods`, `settings-<section>`,
`pause`, `inventory`, `game`. On a machine with no GPU/display use `xvfb-run` with Mesa's
software Vulkan (`mesa-vulkan-drivers`, plus `libxkbcommon-x11-0`).

### Flight mode

**Settings → Controls → Options** chooses how flying starts and stops in Creative:

- **Toggle key** (the default): press the *Toggle flying* key (`F`) to start and again to stop.
- **Double-tap jump**: double-tap Jump (within 0.3 s) to start flying, and double-tap it
  again to stop. A single tap is still just a jump. The *Toggle flying* key does nothing
  in this mode, and the two never both act, so you can't switch flight on and off in one
  press by accident.

Survival never flies in either mode, and taps typed into chat or an open inventory
don't count. Like your keybinds, the choice is saved per launcher instance.

### Controller support

Plug in a controller (Xbox, PlayStation and most others) and it works in game and in the
menus. **Each control has two bindings, one for the keyboard/mouse and one for a
controller**, and the Keybinds screen has a **Keyboard & Mouse / Controller** switch to
edit either. Both work at the same time, so you can switch between them mid-game.

- **Left stick** moves, **right stick** looks (fixed; the buttons below are rebindable).
- Defaults: **A** jump, **B** sneak, **L-stick click** sprint, **Right Trigger** attack /
  break, **Left Trigger** place / use, **Y** inventory, **X** pick block, **D-pad Down**
  drop, **Bumpers** previous / next hotbar slot, **Back** toggle flying, **Start** pause.
- Menus: **D-pad** or a flick of the **left stick** moves the highlight, **A** presses.
  (Keyboard: arrows/Tab/Enter, as above.)
- Rebinding works like the keyboard: click **Rebind**, then press the button. Conflicts
  show in red (per device, and the Controls tab is red if either has one). Search takes
  `btn:a`, `btn:dpad`, `is:conflict`, `is:unbound`, `is:changed`.
- Mods can give a control a controller default with `"default_pad": "south"` in their
  `keybinds/*.json` (button names as shown on the screen, any case: `south`, `a`,
  `right bumper`, `dpad up`, `none`).
- **Inventory, crafting, knapping and the death screen work on a controller.** Open the
  inventory with **Y**; the **D-pad** or left stick moves a highlight over the slots and
  recipes (the list scrolls to follow it). **A** picks a stack up / puts it down / crafts
  once, **X** is a shift-click (quick-move a stack, craft as many as possible), **B** (or
  **Y**) closes it. While knapping, **A** strikes the highlighted cell, **X** taps, and
  **B** abandons the stone. The held stack and the item tooltip follow the highlight, and
  moving the mouse hands control back to it.
- **Stick tuning** (Settings > Controls > Options): look speed (25%-300%), dead zone
  (5%-50%, raise it if the view drifts) and invert look. Saved per launcher instance.
- **Button pictures:** the Controller tab shows each button as a small picture (a green A,
  a red B, bumpers, a D-pad with the pressed arm lit) next to its name. Texture packs and
  mods can replace them with `textures/gui/glyphs/<button>.png`; see
  `textures/gui/README.md`.
- Not done yet: sticks move at full speed (no analog walking), and there's no choice of
  which controller when several are plugged in (they act as one).

Building on Linux needs `libudev-dev` (the controller backend) and `libasound2-dev` (sound);
the release and dev-build workflows install them.

### Rebinding controls

**Settings → Controls → Keybinds** lists every control by category with the key it uses.
Click a control's button, then press the new input:

- a key (`J`), a mouse button (`Middle Click`), or a **combo** - hold modifiers
  and press a key or click: `Ctrl+Alt+A`, `Ctrl+Left Click`.
- a modifier on its own (`Shift`, `Ctrl`) binds when you release it without
  pressing anything else.
- `Esc` cancels. **Reset** puts one control back, **Unbind** clears it, and
  **Reset all** restores everything.

**Search** the list by typing: words match the control's name or category
(`hotbar`), `key:` finds what uses a key (`key:a` for A, `key:ctrl+left` for a
combo, `key:spa` finds Space), and `is:conflict`, `is:unbound` and
`is:changed` filter by state.

**Conflicts are shown in red** - the control, its key, and a line naming the
other control - and the Controls tab says how many are in conflict. Two controls
on exactly the same chord both fire; the game doesn't pick one for you. The one
exception is deliberate: a chord with no modifiers still works while Ctrl/Alt/
Shift are held (walking while sprinting), but gives way to a *more specific*
chord on the same key, so `Q` (drop one) and `Ctrl+Q` (drop the stack) don't both
fire.

**Controls belong to an instance.** Started from the launcher, your controls (the keys,
and the flight mode, in `keybinds.json` and `controls.json`) are saved with *that
instance* (`instances/<id>/` in the app-data folder),
so updating an instance to a newer game version, or renaming it, keeps them, and
two instances can have different controls. Each instance has a permanent id the
launcher gives it (the game is started with `--instance <id>`); the name and the
version can change freely without losing anything. Deleting an instance in the
launcher deletes its controls. A game started without the launcher keeps them in
plain `keybinds.json`.

Only your *changes* are saved, so a default we change later still reaches you
unless you'd customised that control. A bad or unknown entry is ignored rather
than stopping the game - and a key you set for a **mod's** control is kept even
while that mod is removed, and comes back with it.

**Mods add controls too.** They show up in the same list, under the mod's own
heading, with the same search, conflict highlighting, reset and unbind (see
[Mods](#mods) for the file format). A control that runs a command shows what it
runs (`runs /time set noon`).

Not rebindable on purpose: Esc closing an open screen, Enter/Tab/Backspace in text
boxes, the mouse wheel for the hotbar, and Shift-click in the inventory.

Worlds save to the per-user app-data directory
(`%LOCALAPPDATA%\Craftmjne\saves\<name>` on Windows,
`~/.local/share/craftmjne/saves/<name>` on Linux, `~/Library/Application
Support/Craftmjne/saves/<name>` on macOS) — naturally scoped to the OS user
account. Each world is a small `meta.json` (name, seed, game mode, timestamps)
plus a `data.json` recording the player's position and every block edit (by
block *name*, not numeric id, so saves survive block-registry changes).
`meta.json` also records the save format the world was written with, which
is what lets every installed version safely share one saves folder — see
"The launcher" below.
Each chunk's generated terrain is also saved, once, the first time it
generates (`chunks/`, about 2.5 KB a chunk; see `snapshot.rs`). Revisiting a
chunk loads it from there instead of generating it again, so an update that
changes world generation never changes land you've already explored. Only
unexplored chunks use the new generator, and where they meet old ones they
blend in over about a chunk's width. Height eases from the old ground to the
new terrain, and a river heading into old land runs dry a few blocks short
of it. A world from before snapshots existed regenerates once with the
current generator, then is protected the same way. A world is saved on returning
to the menu and when the game is closed with it open; see **Autosave and crash
recovery** below.

### Game modes

Chosen once, at world creation (`save::GameMode`, in `meta.json`).

- **Survival**: mining takes time and drops items, you have health and
  breath, and you can die (see "Survival" below). No flying, and any stale
  `fly: true` from an older save is cleared on load.
- **Creative**: blocks break instantly and drop nothing, the block grid in
  the inventory gives you anything, you can fly (`F`), and you take no
  damage.

The mode can also be changed mid-game with `/mode <survival|creative>` (see
"Chat and commands" below).

## Survival

### Mining and drops

Hold the left button on a block to mine it (`interact.rs`). How long it
takes is the block's `hardness` times `SECONDS_PER_HARDNESS` (5s), divided
by the speed of the tool in your hand **if it's the kind of tool that block
wants** (its `tool`: pickaxe, axe or shovel). A bare hand, or the wrong tool,
is speed 1. By hand that's dirt and grass 2.5s, gravel 5s, a torch
instantly - and stone **60s** and wood (logs, planks) **45s**.

Stone, cobblestone, bricks and ores want a pickaxe, and logs, planks and the
crafting bench an axe, and those are `requires_tool` blocks: their hand time
is eight times their hardness's, and without the right tool they still break
but **drop nothing**, so
nobody punches down a tree - you make a stone axe first. Dirt, grass, sand,
mud, gravel, snow and salt just prefer a shovel; they drop either way, and
they only take part of a tool's speedup (`tool_response` 0.5: the square
root of it), so even a very good shovel still needs real time to move a
block of loose material. A
tool's speed is its tier's `mining_speed` scaled by the tool's quality (see
[Tools, tiers and quality](#tools-tiers-and-quality)): a stone axe takes a
log from 45s to about 16s, a stone pickaxe takes stone from 60s to about
21s, a stone shovel takes dirt to about 1.5s and gravel to about 3s. A tier's `mining_speed` is
just "how many times faster than a hand": stone is 3, which leaves room for
iron, steel and the tiers beyond them to climb well above it. Swords and daggers, hoes (nothing
to till yet) and the wrong tool all mine at hand speed. Changing tool partway
through a block carries the work done over. Cracks spread across the
block as you go (ten `destroy_stage_N` tiles; drop a
`textures/blocks/destroy_stage_N.png` in to replace them). Looking at
another block or letting go starts over. Blocks with `"breakable": false`
(bedrock) can't be mined in either mode.

A mined block drops what its `drops` says: itself by default, something
else (grass drops dirt, stone drops cobblestone), sometimes something
(leaves drop a twig one time in five), or nothing (ice, glass). Drops are
small spinning cubes, or flat spinning pictures for items that aren't
blocks, (`items.rs`) that fall, settle, float
up in water, merge with nearby stacks of the same block, and are picked up
when you walk into them: topping up matching stacks first, then empty
hotbar slots, then storage. They vanish after five minutes, and are saved
with the world. `Q` throws one of the selected item, `Ctrl+Q` the stack.

### Pebbles and twigs

The first things to pick up. **Pebbles** lie scattered on dry, open ground
(about one column in a hundred) and on river, lake and sea beds (about one
in fifty columns of still water, at any depth), and **twigs** lie under trees, within two
blocks of a trunk (about two per tree), on top of the ones leaves drop.
Both are blocks lying flat on the ground (`"shape": "floor"`): no collision,
instant to pick up, and placeable again, but only on top of something solid.
Mine the block under one and it drops as an item. Both are
[waterloggable](#waterlogging): water that reaches one fills the rest of its
cell, and you swim past it as if it were water. They're the first step
toward the first tools.

### Grass plants

Four plants grow on grass, in patches (some bare, some thick; about a third of
the grass columns in all): **short grass** (half a block tall), **plain grass**
(one), **medium grass** (one and a half) and **large grass** (two), commonest
first. They are `"shape": "plant"` blocks: two copies of one texture crossed in
an X inside the cell, tinted by the biome like the grass block's top, with no
collision (you walk through them), instant to pick up, and placeable again
on solid ground. Medium and large grass are two blocks, `grass_medium` +
`grass_medium_top` (the top half is half a block) and `grass_large` +
`grass_large_top`, placed and broken together like the training dummy; mining
either half gives one item. If the ground under one goes, it breaks off.
Water that reaches one replaces it. Textures: `textures/blocks/grass_short.png`,
`grass_plain.png`, `grass_medium.png`, `grass_medium_top.png`, `grass_large.png`,
`grass_large_top.png` (grey masks: the biome colours them); a shorter-than-a-block
plant draws the *bottom* part of its tile.

### Throwing

**Left click with a pebble in hand throws it** (it stacks like any block item).
It leaves your eye along your look direction every 0.35 seconds while held,
and Survival uses one up. Anything with `"throwable": true` in its block or
item file works the same way; so far that's only the pebble, and holding one
means left click doesn't mine or hit (right click still places it).

Everything about throwing is a tag in the block or item JSON, so a modder can
make a new throwable (or retune the pebble) without touching code. All are
optional; leave one out for the default:

| tag | default | meaning |
|---|---|---|
| `throwable` | false | left click throws it |
| `mass` | 1.0 | kilograms |
| `throw_energy` | 8 | joules behind a throw (launch speed `sqrt(2E/m)`) |
| `throw_drag` | 0.002 | air resistance, kg/m |
| `throw_cooldown` | 0.35 | seconds between throws while held |
| `throw_damage` | 1.0 | multiplier on impact damage |

Values that aren't positive numbers are ignored. For example a heavy,
hard-hitting rock is `{"id":"boulder","throwable":true,"mass":4,
"throw_energy":30,"throw_cooldown":1.5,"throw_damage":2}`. The shatter tags
below are likewise per block.

With the default 8 joules of effort, **mass decides
everything else** (`"mass"` in kilograms; a pebble is `0.05`, anything unsaid
is `1.0`): a lighter thing leaves your hand faster (`v = sqrt(2E/m)`, so a
pebble goes about 18 m/s and a 1 kg thing 4 m/s) and the air slows it more
(`deceleration = 0.002/m * v^2`), while a heavy one hangs in the air longer
and has dropped further by the time it arrives. In flight it's an ordinary
dropped item with gravity and drag (`items::step_projectile`), and when it
hits something it lands where it hit and can be picked up again.

**Damage is the kinetic energy at impact** (`0.25 * 0.5 * m * v^2`), so speed
counts for more than mass: a pebble thrown at something a few blocks away does
about 1.5, deliberately little (ranged weapons come later). Hit the training
dummy and the damage number pops up as usual. A long lob that falls a long
way hits a bit harder, since it's gained speed.

**Glass and ice shatter.** A block with `"shatters_at"` (minimum impact speed in m/s)
breaks when a thrown thing at least that fast hits it, and `"shatter_keep"`
(default 1.0) is the fraction of its speed the projectile keeps afterwards.
Glass and ice are both `5.0` and `0.5` (a pebble frozen into ice isn't
shatterable, since that cell is the pebble block): a pebble (~18 m/s) goes through one pane at about 9
and a second at about 4.5, then is too slow for a third, so a line of three
panes takes two pebbles. A shatter drops nothing, makes no damage number, and
is an ordinary block edit, so it's saved like mining it.

### Tools, tiers and quality

Tools are items with a `"tool"` entry, and they come in **tiers** (`tiers/*.json`,
one per material):

```json
{ "id": "stone", "name": "Stone", "mining_speed": 3.0, "damage": 3.0, "quality_band": [0.75, 1.15] }
```

```json
{ "id": "stone_pickaxe", "name": "Stone Pickaxe", "tool": { "kind": "pickaxe", "tier": "stone" } }
```

Kinds are `axe`, `pickaxe`, `shovel`, `hoe`, `dagger`, `longsword` and
`greatsword`. Stone has all seven (the dagger is the light, quick sword; the longsword the
middle one; the greatsword the slow, heavy one), each made by
[knapping](#knapping) (placeholder art). There's no combat yet, so swords only
carry their tier's damage and quality for now.

**Quality** is a whole percent, 0 to 100, stored on the tool's stack (so
tools never stack; their `max_stack` is forced to 1). It says how good *this*
tool is *for its tier*: a tool's stats are the tier's base stats times a
multiplier that runs from the band's low value at 0% to its high value at
100%. **Tiers are independent**: nothing requires a tier's worst tool to beat
the previous tier's best, so a perfect iron sword can still out-damage a poor
steel one. The overlap is a matter of the bases and bands you pick.

The tooltip never shows the number. A tool shows a quality name in that
quality's colour: Trash (0-19, grey), Common (20-59, white), Rare (60-84,
blue), Epic (85-99, purple), and Legendary only at exactly 100 (gold). Tools
from Creative start at Common (quality 50). Quality saves with the tool, in
slots and as a dropped item; [knapping](#knapping) is what sets it for stone
tools (forging will for metal).

### Health

`health.rs`. Ten hearts (20 points) and ten bubbles of breath above the
hotbar; the bubbles only show while you're short of breath.

- Falling more than 3 blocks costs a point per extra block. Landing in
  water or flying cancels a fall.
- Breath lasts 15 seconds with your head underwater, and refills quickly
  in air. With none left you lose 2 points a second.
- Health regenerates a point every 4 seconds, starting 5 seconds after you
  were last hurt. (There's no hunger yet.)
- At zero you die: everything you carried is dropped where you fell, and
  **Respawn** puts you back at the world's spawn with full health.

Creative takes no damage. Health, breath, the hotbar and storage are saved
with the world.

## Inventory

Press `E` to open/close it (`src/inventory.rs`). The hotbar and the
inventory screen both start **completely empty** in a new world, in every
mode — nothing is pre-filled — matching Minecraft's own behavior. Both are
saved with the world.

- **Survival** shows the hotbar plus three rows of personal storage
  (`inventory::Inventory`, `STORAGE_ROWS` x `STORAGE_ROW_WIDTH` slots —
  constants, deliberately easy to make configurable later), filled by
  picking up what you mine. Middle-click "pick block" is Creative-only —
  Survival has no free items.
- **Creative** shows the same hotbar+storage view, or a scrollable grid of
  every registered block — a button above the grid switches between the two
  (`inventory::CreativeTab`). Clicking a block in the grid gives a full
  stack of it in the currently selected hotbar slot.
- **`Inventory` is one resource shared by both modes** — switching modes
  (including mid-session via `/mode`) never resets or duplicates it, so
  whatever's in storage/the hotbar stays exactly where you left it.
- **Moving items:** click a slot to pick its stack up onto the cursor, and
  click another to put it down: into an empty slot, onto the same item (as
  much as fits), or swapping with a different one. **Shift-click** sends a
  stack straight across, from the hotbar into storage or from storage into
  the hotbar. Close the screen while holding something and it goes back
  into your inventory (anything that doesn't fit is dropped).
- Hovering any occupied slot (in either mode) shows the block's name in a
  cursor-following tooltip, Minecraft-style. Scrolling or pressing a number
  key to change the selected hotbar slot also briefly shows that block's
  name above the hotbar itself (`ui::hotbar_label`), fading out after
  `HOTBAR_LABEL_DURATION` seconds — no inventory screen needs to be open.

Opening the inventory frees the cursor and takes over WASD/look/hotbar
shortcuts/block interaction, the same way chat does — all three overlays
(chat, inventory, pause) and the death screen are mutually exclusive (only one can be open at a
time) and Escape closes whichever one is open before it would open the pause
menu. Unlike the actual pause menu, chat and the inventory screen don't stop
the world simulating underneath them: gravity, swimming buoyancy, and
momentum keep running while either is open (`player::player_update`), so
you don't, say, freeze mid-fall just because you checked your inventory.

## Crafting

`src/crafting.rs`. No grid: the inventory screen has a **Crafting** panel
listing what you can make, each recipe with its ingredients (red when
you're short, greyed out when you can't make it yet). Click a recipe to
craft one; shift-click to craft as many as you can. Ingredients come out of
storage before the hotbar, and the result goes in like a pickup.

- **Stations.** A recipe is made by hand (anywhere) or at a station block.
  Station recipes show while that block is within 4 blocks of you.
  Right-clicking a station opens the inventory screen to craft at it;
  shift + right-click places a block against it instead.
- **Discovery.** A recipe only appears once you've held one of its
  ingredients (remembered per world). Pick up a log and planks show up.

The first recipes, all by hand:

| Makes | From |
|---|---|
| 4 Planks | 1 Log |
| 1 Stick | 1 Twig (found under trees; leaves drop them too) |
| 5 Sticks | 2 Planks |
| Crafting Bench | 3 Planks + 4 Sticks |

The crafting bench is the first station. Nothing needs it yet; it's where
the next recipes go.

### Knapping

Stone tools aren't crafted at once: clicking one (pickaxe, axe, shovel, hoe,
dagger, longsword, greatsword; each needs a **pebble** plus sticks/twigs, and no bench) opens the
**knapping screen** (`src/knap_ui.rs`, rules in `src/knapping.rs`). A 16x16
stone sits in front of you with the tool head's outline marked in gold.

- **Left click strikes.** A circle of force around the cell, strongest at its
  centre, fading to nothing at its rim (about 3 cells). Cells on the stone's
  edge are safe. A strike on a cell further in (the further, the likelier)
  can **crack** the stone: the crack runs from the strike, bending toward the
  nearest open edge, and either breaks through (losing a band of stone along
  it) or runs out of energy and leaves a flaw (a dark, weakened line). Pieces
  cut off from the main body fall away.
- **Right click taps.** Breaks just the one cell, never cracks, but you only
  get 4 per stone. A circle can't cut the outline's sharp corners exactly;
  taps are how you finish them.
- **Done** ends it. Quality is how close the stone is to the outline:
  chipping into the outline costs twice as much as leaving extra, and a
  perfect match is 100%. It's shown as a name (Trash, Common, Rare, Epic;
  Legendary only at a perfect 100), never a number. Cancel or Escape leaves.
- **Costs.** The pebble is used up by your first strike or tap, so closing
  before touching it costs nothing and closing after loses the stone. The
  haft and binding are used up when you press Done.

How hard it is was measured with a bot that knows exactly what each strike
does (a stronger player than a person): about 61% Rare, 28% Epic, 6% Common
and 4% Legendary, in ~26 clicks. Heavy strikes alone never reach 100%. Each
stone has hidden per-cell toughness and every strike varies a little, so
taps can't be planned perfectly either. Random hitting always gives Trash.
Nothing about a knap in progress is saved.

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

### Rotation

Controlled by `rotation` (`blocks::Rotation`):

- **`"none"`** (the default) — always renders unrotated: `top`/`bottom`
  textures fixed to +y/-y, `side` on the four remaining faces, no matter how
  it was placed.
- **`"log"`** — Minecraft log behavior. Place it against a block's top or
  bottom face and it stands upright as normal; place it against a side face
  and it lies on its side instead, its `top` texture (the end grain) facing
  the face you clicked — i.e. facing you. This unlocks three orientations:
  up-down (the default), east-west, and south-north.

This is state on the *placed instance*, not the definition — two logs
placed against different faces render differently even though they share
one `BlockDef`. It's stored per-cell in `Chunk::axis` (`world.rs`), computed
from the clicked face's normal at placement time (`interact::
axis_from_normal`), and — unlike the fluid sim's per-cell state, which is
fully re-derivable and intentionally never saved — round-trips through
`BlockSetEvent`/`EditLog`/`save::BlockEdit` so a rotated block's orientation
survives a save and reload. `mesher::rotated_tile` does the actual face
remap; it's a no-op for any block whose `rotation` is `"none"`, so adding a
second rotating block someday needs zero mesher changes.

## World generation

`src/terrain.rs` builds every column from a few independent layers, each its
own noise stream. The world is 256 blocks tall with the sea at 26
(`config.rs`), so mountains can tower over 200 blocks above the water. (It
was 64, then 128 tall; chunks explored back then keep loading exactly as
they were, with air above.)

- **Lowlands, uplands and hills.** Land sits a few blocks above the sea at
  the coast and climbs gently to uplands about 24 blocks higher deep
  inland. A separate hill-country mask decides which regions are flat
  plains and which are rolling hills up to about 50 blocks tall.

- **Continents and coasts.** A very low-frequency continent field splits
  land from sea into several separate landmasses. A second field decides
  each stretch of coast's *style*: gentle coasts fade into the sea over a
  long beach. Steep ones are modelled on real sea cliffs: a near-vertical
  face of bare rock under a thin line of soil; an edge that wanders in and
  out into headlands and bays; a wave-cut notch at the foot of some faces;
  a rocky wave-cut platform just under the water, littered with fallen
  boulders; and sea stacks standing offshore - stocky masses of rock that
  taper down to the platform and stand lower than the cliff they broke
  from, never more than about 22 blocks. Steep slopes inland (and on
  mountains) are bare rock too, rather than grass ledges.
- **Mountain ranges.** A range mask (`mountainness`) decides where ranges
  are - big ones, several hundred blocks across. A range's broad base
  rises evenly out of the plains through foothills, and ridged noise
  lifts its core into ridgelines and peaks up to about 230, well above
  the snow line; its steepest faces are bare rock. Heights are eased toward the build ceiling
  rather than clamped, so summits stay pointed instead of flattening.
- **Climate maps.** Two broad, smooth noise maps cover the world
  (`biome::ClimateMaps`): *temperature* (features over a kilometre or so)
  and *humidity* (several hundred blocks). Because they change slowly, cold
  places sit beside cold places and warm beside warm, so biomes come in
  large clumps instead of speckles. Biomes are picked from them by
  declarative tables (`biome::LAND_BIOMES`, `biome::SEA_BIOMES`): the first
  row whose ranges contain a column's climate wins, so a new biome is one
  more row. Grass color follows the same maps: lusher where it's humid,
  yellower where it's dry, cooler near the cold regions.
- **Seas by temperature.** Open ocean is a `warm_sea`, `temperate_sea` or
  `cold_sea`, from the same temperature map as the land, so cold seas lie
  off cold coasts. Seas no longer freeze over. A cold sea only grows a
  patchy fringe of ice in its shallowest water (a couple of blocks deep)
  along the shore. Further out it stays open.
- **Icebergs.** An overlay biome on cold seas (`icebergs`). Bergs are
  blobs of ice with snow on top, rising a few blocks out of the water and
  reaching about three times as deep below it. They're most common hugging
  cold coasts and thin out further offshore, though a few patches drift out
  into open cold water.
- **Land biomes.** Every land column has a *region* biome from the
  climate:
  - **`plains`** - wide, mostly flat grassland with the odd tree, streams
    running through it to the rivers, and ponds along them.
  - **`forest`** - where it's humid: dense trees and gently rolling land
    (steeper where it meets a mountain range). The place to go for wood.
  - **`snow`** - where it's cold.
  - **`alpha_plains`** - the rough, bumpy, puddled grassland that used to
    be the only land biome. It turns up in some of the low basins that
    would otherwise become marshes, and in rare patches elsewhere.

  How rough the land is follows the biome and eases across each border, so
  the terrain never steps where the biome changes.
- **Streams and ponds.** Below river size, smaller waterways a block or
  two wide drain the land everywhere off the mountain ranges, joining the
  same network, so nearly every stream runs on into a river (or straight
  to the sea). On plains streams, some spots widen into round ponds.
- **Biomes, layered by altitude.** On a mountain
  range, `biome::ALTITUDE_ZONES` replaces
  it by height: the foot of a range keeps its region biome, and from height
  135 up it becomes the desolate `mountain` biome (bare rock and scree,
  snow caps from 165). Adding a band between them later (spruce on the lower
  slopes) is one more table entry.
- **Rivers.** Water is routed downhill over one unbounded drainage network
  (`drainage.rs`, a 16-block grid that covers the whole world and is
  computed lazily as you explore), and wherever enough of it gathers a river
  forms with its own water surface. Rivers start in the uplands *above* sea
  level and only ever end at the open ocean. A hollow that would trap the
  water fills and spills over its lowest rim, with the river cutting
  through, and the small ponds dotting the plains are just ponds: a river
  may pass through one but never stops there. Rivers flow on,
  easing down a block at a time with flowing water (the fluid sim's own
  levels, so the surface slopes in eighths of a block rather than stepping
  a whole one). Their
  size depends on how much land drains into them *and* how far they've
  run: a short stream stays a couple of blocks wide, and only a river that
  has come a long way (over a kilometre, for some) grows to 16 blocks
  across. How far a river has cut below its banks varies
  along its length. Some run flush with the ground beside them; others sit
  a few blocks down, the start of a canyon. Where a river runs well below
  the land around it, it runs in a real valley: a floodplain beside the
  water, then sides climbing back to the land at a slope (gentle in the
  lowlands, V-shaped and steep in mountains), reaching up to about 50
  blocks out, and steepening into a gorge only where the land towers over
  the river. Where two rivers' valleys overlap they merge smoothly.
- **Marshes, alpha plains and salt seas.** Where a river runs into a
  depression that overflows, its low ground becomes a marsh (flat mud
  dotted with shallow pools, with the river winding through and on), or in
  grassland sometimes rough alpha plains with puddles in its dips. Rarely, a big depression
  keeps its water instead, as real inland seas with no outlet do. That
  makes a salt sea: a lake on a salt bed ringed by salt flats, where the
  rivers feeding it end. `/locate feature marsh` and `/locate feature
  salt_sea` find them.
- **Waterfalls.** Where a river drops 3 or more blocks between two points
  of the network, it holds its level for as long as the ground can contain
  it, then falls. Inland, that's a waterfall down a step in the valley. At
  a cliff coast it's a waterfall off the cliff edge into the sea (the
  tallest found so far is 28 blocks). The curtain is real water: falling
  water under a flowing top cell, the same thing the fluid sim makes under
  a ledge. In freezing biomes it's frozen solid.

## The training dummy, entities and the player model

`src/block_models.rs`, `src/entities.rs`, `src/player_model.rs`.

**The training dummy is a block** - really two, stacked: `training_dummy` and
`training_dummy_top` (it stands two cells tall). So it collides, saves,
relights, drops and breaks through the ordinary block systems, with nothing
per-frame to simulate. It's an item you place like any block (right click; it
needs the cell above free). Its look is the player model with a blank skin,
drawn by a model entity that `block_models.rs` stands in the lower cell
(recreated from the blocks, never saved, and always facing +Z since a block
has no yaw).

**Mining it takes Shift + left click.** A plain left click does nothing to it
(that click is free for hitting it later). Hold Shift and it mines like
anything else, over time - 10 seconds by hand in Survival (`hardness: 2.0`) -
and drops the dummy item; Creative breaks it instantly while crouching.
Breaking either half removes the other, with one drop.

**Damage from anything.** All damage goes through one `DamageEvent` (the cell
it landed on, how much, and a source: `Melee`, `Projectile` or `Falling`), and
one system (`src/damage.rs`) hands it to whatever's there. The dummy takes all
of them the same way and shows the number - a sword swing, a thrown pebble, and
(once they exist) a falling block just write the event; none needs to know the
dummy exists. Hitting a block that isn't hittable does nothing.

**Hitting it.** A plain left click (no Shift) hits the dummy with whatever's
in your hand, every 0.45 seconds while held. The damage is the held tool's
tier damage at its quality (a stone axe does about 2.3 at quality 0 up to
3.5 at 100), or 1 for a bare hand or any non-tool. The dummy has no health;
it just shows the number: red digits pop up from its head, shoot up a little,
arc over and drift down, and have faded out completely by the time they're
halfway down its legs (`src/particles.rs`). Whole numbers show as `3`, others
to one decimal (`2.9`).

In block files that's `"shape": "model"` (the mesher draws nothing, and its
neighbours' faces still draw), `"upper": "<block>"` (two cells tall) and
`"requires_crouch": true` and `"hittable": true` (a plain click hits it).
For items and blocks, `"mass"` (kg) and `"throwable": true` - see
[Throwing](#throwing). Blocks can also set `"shatters_at"` and `"shatter_keep"`.

**Entities** are the other way to do it, for things that shouldn't be locked
to the grid: `/summon training_dummy` also stands a free-standing dummy
entity two blocks ahead of you, facing you, solid, saved with the world;
`/clearentities` removes those (not the block ones). They have no animation,
gravity, health or combat yet.

**The model** is the classic blocky layout: head 8x8x8, body 8x12x4, arms and
legs 4x12x4 (in 1/16-block pixels, so 2 blocks tall and 1 across at scale 1),
textured from a standard 64x64 skin. Wide arms; a slim variant can be added
later as another layout. It faces -Z at yaw 0, like the player. Faces are
shaded by direction in the vertex colors (so a flat skin still reads as a
solid shape under this unlit renderer), and the whole figure is tinted by the
light at its position, block and sky, so it darkens in caves and at night.
The skin is generated procedurally; there's no PNG override yet.

**Entity files** (`entities/*.json`):

```json
{
  "id": "training_dummy", "name": "Training Dummy",
  "model": "player", "skin": "blank", "height": 2.0,
  "collisionbox_binding": true,
  "collision_size": { "width": 1.0, "depth": 1.0, "height": 2.0 }
}
```

`collisionbox_binding` decides what you bump into:

- `true` - the entity's own box (`collision_size`, centred on its feet), so
  you walk right up to it. The dummy is 1 wide, 1 deep, 2 tall.
- `false` - it behaves like a block: the box is rounded out to every whole
  block cell it touches.

You can stand on it, walk around it, and can't place a block inside it. If
one is summoned on top of you, you aren't pushed out; you can walk out.

## Chat and commands

Press `T` to open a one-line chat box, or `/` to open it with `/` already
typed (the standard command-shortcut convention); `Enter` sends, `Escape`
cancels. Ctrl+A selects the whole input, Ctrl+C copies it, and Ctrl+V pastes
from the OS clipboard (replacing the selection, or appending if nothing's
selected). Sent lines land in a local scrollback that fades out a few
seconds after the box closes — there's no multiplayer, so this exists
mainly as a place to type `/`-prefixed commands.

`src/commands.rs` is the dispatcher. The built-in commands are:

- `/mode <survival|creative>` — also accepts `s`/`c` or `1`/`2`. Changes the
  running world's game mode immediately and persists it to `meta.json`.
- `/texture-report` — a green/yellow/red breakdown of every texture in the
  game (block tiles from `atlas.rs`, sun/moon from `sky.rs`): how many are
  real art or the intended procedural default (green, "working"), how many
  fell back to the checkerboard placeholder because a custom file existed
  but failed to load (yellow, "broken but functioning" - the game keeps
  running), and how many are completely unresolvable (red - a real
  invariant check against `BlockRegistry::texture_names()`, not a
  hardcoded zero; see `texture_report::TextureReport`'s doc comment for
  why it should always read `0`). Below the counts it lists which specific
  names are yellow/red, in matching colors.
- `/summon <entity>` — summon an entity (see
  [Entities](#entities-and-the-player-model)) in front of you; Tab completes
  names. `/clearentities` removes them all.
- `/locate <biome|feature|structure> <name>` — reports the nearest match
  (coordinates + distance) to wherever you're standing. `biome` searches
  for `plains`/`forest`/`alpha_plains`/`snow`/`mountain` (the last being a
  range's desolate upper slopes - see "World generation") and the seas `warm_sea`/`temperate_sea`/
  `cold_sea`/`icebergs`; `feature` searches for
  `river`/`ocean`/`mountain` against the generator's real terrain (the
  same checks worldgen itself uses, not a second guess at what they mean -
  `feature mountain` finds the range itself, `biome mountain` its peaks);
  `structure` is reserved for when this game actually generates
  structures to find, and currently just says so.

- `/blockstate <x> <y> <z> [tag] [value]` (alias `/bs`) — shows a placed
  block's [state tags](#the-two-kinds-of-tags), or sets one: `/blockstate ~ ~ ~
  axis x` turns the log you stand in on its side, `waterlogged ice` freezes the
  water a pebble sits in, `level 3` sets a fluid cell's flow level. `~` means
  your own position (`~2` is two blocks over). Tab completes tag names and
  values.

Running *any* recognized command — even one that fails with a usage error —
permanently sets a `cheats: true` flag on the world's `meta.json`
(`save::WorldMeta::cheats`). This mirrors Minecraft's own "cheats" world flag:
it's never shown in the UI and never cleared, and exists so a future
achievements system can check it and skip a world that's had commands used in
it. An unrecognized command name (a typo, not a real command) does not set it.

Add a command with `CommandRegistry::register`/`CommandSpec::new` (see
`commands::CommandRegistry::with_defaults` for the built-ins' own
registration) — `commands.rs`'s module docs cover the full extension point,
including the cheats-flag rule and how aliases work. The in-game chat
dropdown (`T` or `/`, then start typing) autocompletes both the command
name itself and, for a command built with `CommandSpec::with_arg_candidates`
(`/locate` is the example), its arguments too - one token at a time, reusing
whatever's already fully typed to decide what the next token's candidates
are.

### Colored chat text

Any chat message — typed by a player or produced by a command like
`/texture-report` — can wrap part of itself in `~(#RRGGBB)~text~(#RRGGBB)~`
to render `text` in that hex color, e.g. `~(#ff0000)~careful!~(#ff0000)~`
shows "careful!" in red. `src/text_color.rs` is the whole mechanism (shared
by every caller, not a bespoke path per feature): the marker is a **toggle**,
not a matched pair — the first `~(#hex)~` a parser sees starts a colored run
using that hex, and the *next* one ends it regardless of what hex digits it
contains, so a mistyped closer still closes the span instead of leaving the
rest of the message colored. `chat::ChatLog::push` parses every message once
when it's added to the scrollback; rendering splits the parsed segments
across one Bevy `Text` root entity plus `TextSpan` children (Bevy's
multi-colored-text API), one call reused for anything a player or a command
sends to chat.

## The launcher

`launcher/` is a separate small app (its own workspace member, egui rather
than Bevy) that owns everything to do with versions. **`CraftmjneSetup.exe`
installs the launcher**, and the launcher takes it from there.

It replaces the old in-game auto-updater, which worked by having the running
game rewrite its own executable. That approach fought the operating system:
a process silently replacing its own binary is exactly what antivirus and
EDR heuristics exist to catch, it gave no way to tell whether it had really
taken effect, there was no way back to an older build, and it only ever
swapped the `.exe` — so a release that changed `blocks/` left an installed
copy half-updated. None of that applies now.

### What it does

- **Versions tab** — lists every published GitHub release that has a build
  for your platform. Download one and it's extracted to
  `%LOCALAPPDATA%\Craftmjne\versions\<version>\` (or the OS equivalent) as
  an ordinary, complete folder: executable, `blocks/`, `textures/`. It's
  downloaded once and kept, so switching back to it later is instant, and
  removing one is just deleting that folder.
- **Instances tab** — named launch configurations. Each pins a version and
  optionally overrides render distance, world seed, and any extra command
  line arguments. Editing one is immediate; they're stored in
  `launcher/instances.json`. Every instance also has a permanent, hidden `id`
  (made when it is created, or the first time an older launcher's list is
  opened) that the game is started with, `--instance <id>`. The player's
  keybinds are saved under it (`instances/<id>/`), which is why they survive a
  rename or a version update.
- **Play** — starts the selected instance. The launcher doesn't wait around;
  you can close it and keep playing.

Downloads extract to a scratch directory and are only renamed into place once
they finish, so an interrupted download can't leave behind a half-installed
version that looks runnable but crashes on start.

### Worlds are shared, and migrate forward

Every instance reads and writes the **same** `saves/` folder — the one a
directly-installed copy of the game already uses — so adopting the launcher
doesn't strand existing worlds, and a world follows you between versions
rather than being trapped in whichever instance made it.

What makes that safe is that worlds record the save format they were written
with (`save::SAVE_FORMAT`):

- Opening an **older** world migrates it forward and stamps it, written to
  disk immediately rather than at the next autosave.
- Opening a world written by a **newer** build is refused — loading it would
  mean writing it back out in the older format and silently dropping
  whatever the newer build stored. The worlds list still shows it, greyed
  out and labelled, so it never looks like the world vanished.

`SaveStore::open_world` is the single entry point that does both, so the
too-new check can't be skipped by some other call site.

### The launcher updates itself, from its own branch

On every start the launcher fetches `launcher/manifest.json` from the
`launcher` branch, and if it names a newer version with a build for your
platform, downloads it and swaps it in — then tells you to restart. This is
keyed to a **branch**, not to release tags, which is the point: shipping a
launcher update neither requires nor triggers a game release. It's also a
plain file fetch from `raw.githubusercontent.com`, so it needs no token and
can't hit the API rate limit.

Self-replacing is fine here in a way it wasn't in the game: the launcher is
a small binary, the swap happens seconds after startup with nothing else
going on, and if it fails you simply keep running the launcher you already
have and it says so. No game version's install depends on it — those live in
`versions/` and are never swapped in place at all.

To publish a launcher update: bump `version` in `launcher/Cargo.toml` and
push it to the `launcher` branch. `.github/workflows/launcher-release.yml`
builds every platform, publishes them under a `launcher-v<version>` tag, and
writes the matching manifest back to the branch.

## Releasing a new game version

Releases are cut by hand: bump `version` in `Cargo.toml` (and the matching
`craftmjne` entry near the top of `Cargo.lock`), merge that to `main`, then
either `git tag vX.Y.Z && git push origin vX.Y.Z` or create a Release for
that tag directly on GitHub (Releases → Draft a new release → type the new
tag). Either path fires `release.yml`'s `on: push: tags: ["v*"]` trigger,
which builds Windows/Linux/macOS archives, packages the Windows installer,
and publishes everything to a GitHub Release (usually ~10-15 minutes end to
end). The launcher offers the new version the next time it's opened.

Each `craftmjne-<target>.<zip|tar.gz>` is a complete, self-contained build —
executable plus `blocks/` and `textures/` — because the launcher unpacks it
as-is and the game refuses to start without `blocks/` beside its executable.

Note that some environments — including Claude sessions working in this
repo — get an HTTP 403 pushing tags even though branch pushes work fine, so
tagging/releasing has to be done from a normal git checkout or the GitHub UI.

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
- **Baked lighting + custom shader** — directional face shading, per-vertex
  ambient occlusion and the propagated voxel light (colored block light in the
  vertex RGB, sky light in the vertex alpha) are all baked into vertex colors
  by the mesher. Chunks render with one tiny unlit WGSL fragment shader
  (`src/chunk.wgsl`) that also does alpha cutout (leaves/glass), water
  translucency, and distance fog — no lights, no normals, no shadow passes,
  two pipeline states total. The day/night cycle is a single uniform, so the
  whole world's daylight changes without touching a mesh.
- **Flat typed tables** — `Vec<u16>` chunk storage (Y-major, so column ops are
  contiguous slice copies) and flat `Vec<bool>` block-property tables in every
  hot loop; the mesher's AO neighbourhood offsets are precomputed integers.
- **Streaming with budgets** — chunks generate/mesh sorted by distance with
  capped in-flight tasks; far meshes are dropped, and further out
  (`world::UNLOAD_MARGIN` chunks past the render distance) a chunk's data
  is dropped too and reloaded from its snapshot when you come back, with
  your edits and its water reapplied - so memory stays bounded however far
  you explore (about 210 MB of chunk data at render distance 8). Bevy
  frustum-culls per chunk automatically.
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
├── atlas.rs     # Painters resource: procedural tiles + optional textures/blocks/*.png -> RGBA atlas
├── icons.rs     # bakes isometric ItemModel::Default inventory icons from the atlas
├── terrain.rs   # TerrainGenerator: heightmap, oceans, rivers, biomes, caves, ores, trees
├── snapshot.rs  # saves each generated chunk once so later generator changes never alter it
├── drainage.rs  # unbounded, lazily memoized drainage network rivers are routed over
├── light.rs     # LightPlugin: colored block light + sky light propagation
├── mesher.rs    # culled + AO-baked chunk meshing (runs on task pool)
├── world.rs     # WorldPlugin: ChunkMap, streaming, gen/mesh tasks, edits, save/load
├── render.rs    # RenderSetupPlugin: ChunkMaterial, atlas + icon atlas images, fog
├── chunk.wgsl   # the chunk fragment shader (embedded asset)
├── sky.rs       # SkyPlugin: day/night clock, sun/moon billboards, sky color + world brightness
├── celestial.wgsl # the sun/moon fragment shader (embedded asset)
├── player.rs    # PlayerPlugin: AABB physics, swimming, fly mode, pause/cursor, camera
├── interact.rs  # InteractPlugin: voxel DDA raycast, mining (time, cracks, drops), place/pick, hotbar
├── items.rs     # ItemsPlugin: dropped items - physics, pickup, merging, Q to throw, saved with the world
├── health.rs    # HealthPlugin: health, breath, fall damage, regen, death screen + respawn, hearts HUD
├── knapping.rs  # knapping rules: stone, strikes, cracks, scoring, quality (pure)
├── knap_ui.rs   # the knapping screen
├── damage.rs    # DamageEvent + sources; delivers damage to hittable blocks (the dummy)
├── particles.rs # particle effects (damage numbers): the arc, the pixel font, the billboards
├── block_models.rs # draws the model of Shape::Model blocks (the dummy) from the blocks
├── entities.rs  # entity definitions, summoning, collision boxes (the training dummy)
├── player_model.rs # the blocky player model: boxes, skin layout, mesh data
├── inventory.rs # InventoryPlugin: hotbar+storage, moving items, crafting panel, Creative's item list
├── item_registry.rs # every holdable item: one per block, plus plain items from items/
├── crafting.rs  # recipes from recipes/, stations, discovery, the craft rule
├── chat.rs      # ChatPlugin: chat box UI + input, routes "/" lines to commands::execute
├── commands.rs  # chat command dispatcher (/mode, /texture-report, /locate ...) + the cheats-flag rule
├── text_color.rs   # shared ~(#hex)~text~(#hex)~ chat color-marker parser, used by chat + commands
├── texture_report.rs # TextureReport resource: green/yellow/red texture health, read by /texture-report
└── ui.rs        # UiPlugin: crosshair, hotbar icons, hint, F3 debug panel
blocks/
└── *.json                # one block definition per file - see "Add a block" below
items/
└── *.json                # items that aren't blocks (twig, stick) - see "Add an item" below
recipes/
└── *.json                # one recipe per file - see "Add a recipe" below
textures/blocks/
└── *.png                 # optional hand-supplied art overriding a procedural tile - see "Use real texture files" below
textures/sky/
└── sun.png / moon.png    # optional hand-supplied sun/moon art - see "Day/night cycle" below
launcher/                 # separate crate: the launcher (egui, no Bevy)
├── paths.rs              # where downloaded versions / instances live on disk
├── library.rs            # installed versions - the download-once cache
├── remote.rs             # GitHub releases: what exists, and fetching one
├── instances.rs          # named, editable launch configurations
├── launch.rs             # starting a game build
├── selfupdate.rs         # the launcher updating itself from its own branch
├── jobs.rs               # background work, so the window never blocks
└── app.rs                # the window
installer/
└── craftmjne.nsi        # NSIS script -> CraftmjneSetup.exe (installs the launcher)
.github/workflows/
├── release.yml           # push of a vX.Y.Z tag -> cross-platform build + release
└── launcher-release.yml  # push to the `launcher` branch -> launcher build + manifest
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
(`world::exit_world`, on `OnExit`) writes the session's accumulated block edits
and current player pose back out.

### Autosave and crash recovery

Saving a world has three triggers, and only one of them writes the world's real
save (`data.json`) to a file the crash path could damage:

- **Leaving the world** (back to the menu): a real save, as before.
- **Closing the game with a world open** (the window's close button): an
  *emergency save*, also real (`world::emergency_save`, on `AppExit`).
  `OnExit(InGame)` never runs when the app ends, so without it only the last
  autosave would have survived.
- **Every N minutes while playing** (default 5): an *autosave*, a snapshot
  written on a background thread to `saves/<world>/autosave.json`. It never
  touches `data.json`, so it cannot corrupt the real save, and it is written to
  a temp file and renamed so a crash mid-write leaves nothing half-done.

A real save deletes the snapshot - it is newer, so the snapshot has nothing
left to offer. If the game *crashes*, the snapshot is left behind. The next time
the world is opened (`SaveStore::open_world`, and again in `enter_world`) a
snapshot **newer than `data.json`** (file modification time) becomes the real
save and is removed; an older one, or one that doesn't parse, is just deleted.
The player is never asked anything. A background write that is still in flight
when a real save lands is dropped (`world::persist` numbers writes), so a slow
autosave can't land afterwards and masquerade as crash leftovers.

Settings > **Saving** turns autosave off and sets the interval (1-60 minutes);
stored per launcher instance in `saving.json` (`save::AutosaveOptions`).
Turning it off still leaves the emergency save. What a crash loses is whatever
happened since the last snapshot. Chunk terrain is separate: chunk snapshots
(`snapshot.rs`) are written when a chunk is first generated, so they are already
on disk. Not covered: the OS killing the process or a power cut *during* the
emergency save, and a crash between a world's creation and its first snapshot
(the world then reopens as new).

## Translations

Settings > **Language** picks the language. Shipped: **English** (United States, United Kingdom, New Zealand, India,
South Africa), **Español** (España, México, Puerto Rico), **Português** (Portugal, Brasil), **Français** (France),
**हिन्दी**, **中文 (普通话)** (Mandarin: Simplified and Traditional, plus Taiwanese and Singaporean variants of each) and
**粵語 (廣東話)** (Cantonese: Traditional and Simplified). Languages with regional variants are one heading in the list
that opens to show its variants; each language
shows **how much of the game is translated**, as a percentage, and is named twice: in its own language and in the
one you're using (`Español (España) / Spanish (Spain)` while English is chosen; just once when they're the same). Those
names are keys too - `lang.name.<code>` and `lang.group.<group>` - so a mod adding a language adds its name in each
language it knows; without one, only its own name shows. The choice is saved per launcher instance
(`language.json`). `CRAFT_LANG=<code>` overrides it for one run.

**Everything is translated except commands**: `/give`, its arguments and its syntax stay English in every
language (their descriptions and results are translated). Left in English on purpose: the F3 debug overlay,
developer-facing errors (a broken mod or recipe file, `mod.json` problems), key names in a chord (Ctrl+A), search
syntax (`key:a`), and the launcher (a separate program). A translation of *your own* world or mod names is up to you.

**How it works.** A language is `lang/<code>.json`: a flat `"key": "text"` object and an optional `_meta`:

```json
{ "_meta": { "name": "Español (México)", "group": "Español", "fallback": "es-ES" },
  "Settings": "Configuración",
  "block.stone": "Piedra" }
```

* `name` is how it's listed, `group` the heading variants share, `fallback` the language to try for a missing
  string (before English), `cjk` (`zh-Hans`, `zh-Hant`, `ja`) the ideograph set to draw.
* **Keys.** Interface text uses its English text as the key (`"Worlds"`); named things use `block.<id>`,
  `item.<id>`, `entity.<id>`, `rarity.<id>`. `{name}` is a placeholder. Counts are `key.one` / `key.other`, picked by
  the language's own plural rule (French and Hindi count 0 as singular, Portugal's Portuguese doesn't, Chinese has
  none). An empty string means "not translated yet".
* **`lang/en.json` is the reference** - it lists every key. A language's percentage is the share of those keys it
  has text for (counting what it inherits through `fallback`, since that is what a player sees), so it **drops by
  itself when keys are added and rises as they are translated**; nothing is stored. Today every shipped language is at
  100%. When strings are added without being translated, lower `SHIPPED_PERCENT` in `src/lang.rs` on purpose and say so.
* **Adding strings in code**: wrap them in `lang::tr("English text")` (or `tr_args` / `tr_n`), add the key to
  `lang/en.json`. A test fails if code uses a key `en.json` lacks, including text handed to the UI helpers
  (`theme::button`, `hint`, ...), which translate their text themselves.

**Mods** add `lang/<code>.json` to their folder:

* A file for a language the game has adds to it - a mod translating its own content (`block.mymod:granite`), or
  **another mod's** (an add-on that only ships `lang/fr-FR.json` with that mod's keys).
* A file for a code the game doesn't know, with a `_meta`, **adds a new language to the list** (it can join an
  existing group, e.g. `"group": "Español"` for `es-AR`).
* Ship `lang/en.json` with your own keys so the percentages count them. Mods load after the game, in folder-name
  order, each overriding what came before; the game's names for a language can't be changed by a mod.
* Files are read from the installed mods folder when the game starts and whenever the Language tab opens.

How the variants are made, honestly: a variant is a small file that `fallback`s to its parent and holds only what
differs. **Mexican and Puerto Rican Spanish** differ in vocabulary (the Puerto Rican file is only a handful of words).
**Taiwanese Mandarin** differs from the generic Mandarin in ~60 strings (設定 vs 設置, 資料夾 vs 文件夾...), in either
script. **Cantonese** is written as it is spoken (你死咗！, 冇, 嘅, 喺) for the sentences and uses Hong Kong terms for the
labels; its Simplified file is the same words in simplified characters. **Singaporean Mandarin** is listed and
inherits from the generic Mandarin because this game's text has nothing a Singaporean would say differently - add
strings to `zh-Hans-SG.json` / `zh-Hant-SG.json` and they win. The **English dialects** are the reference text with
the few differences this game's text has (`Trash` is `Rubbish` in the UK, New Zealand and South Africa); spelling
differences between English dialects hardly come up in this game's wording. English dialects always show 100%: they
are the reference text.

Notes on the translations themselves: they were written for this project, not by native-speaking reviewers - treat
them as a solid first pass and correct them freely (they are plain JSON). Hindi is drawn with the game's own
Devanagari, which is still a first pass.

## Fluid flow

Any block with `fluid: true` (water today) spreads and dries up
automatically — no fluid-specific code required. `world.rs` keeps a per-cell
fluid level (a field of the cell's state word, see
[tags](#the-two-kinds-of-tags)) alongside the block grid:

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

**Sloped surfaces.** Water and ice (frozen water) don't draw flat steps
between levels. Each top *corner* of a cell takes the lowest height of the
same-fluid cells that touch it (`mesher::blend_corner`), so a cell runs from
its own height at the upstream edge down to the next cell's height at the
downstream edge - 16 into 15, 16 into 13, whatever the two levels are - and
neighbouring cells always agree on their shared edge, so the surface is one
continuous skin and the step walls only appear against a covered (deeper)
neighbour. The rule only looks at the cells around each corner, so
waterfalls and any future hand-shaped block get the same treatment. The old
one-height-per-cell look with step walls is still compiled and tested as
`mesher::SurfaceStyle::Flat` (`mesh_chunk_styled`); change `SURFACE_STYLE`
to switch globally.

A budgeted queue (`FluidQueue`/`recompute_cell`) reacts to `BlockSetEvent`
and relaxes affected cells a few hundred at a time per tick, so a large
spread is visibly gradual rather than resolving in one frame. The mesher
draws a partial "step" wall between same-fluid neighbours at different
levels instead of culling that face outright, so adjacent flow heights never
show a gap. See the "Known gaps" note in Roadmap ideas for what's simplified.

### Waterlogging

A block with `"waterloggable": true` (pebbles and twigs today) can share its
cell with a fluid, or with what the fluid freezes into. Every such cell has
**two tags**: the main block and the layer it holds, with the fluid tagged
just as if it were a block inside the main block's waterlogged slot. The
registry derives one real block id per pair - `pebble@water`, `pebble@ice` -
so each is `{ block: pebble, layer: water }` in `BlockDef::layered` and
`Tables::base_of`/`layer_of`. Nothing else needed to learn about two-part
cells: saves, snapshots and the edit log record the variant by name, light
multiplies the two halves' `transmission`, and a variant is otherwise its
main block (same textures, drops, hardness, floor-ness; no item of its own).

- **Holding water.** The fluid sim compares *which fluid a cell holds*
  (`Tables::fluid_kind`), not its block id. Water flowing into a pebble turns
  it into `pebble@water` at the water's level instead of overwriting it
  (`world::fluid_host`), spreads on from it like any water cell, and when it
  dries the pebble is left behind (`Tables::dried`).
- **Freezing.** A fluid with `"freezes_to": "ice"` freezes its exposed
  still or flowing cells (not a waterfall column) in cold biomes; the cell's
  own `Tables::frozen` entry says into what, so water becomes `ice` and
  `pebble@water` becomes `pebble@ice`. **Frozen water keeps its geometry:**
  the cell's stored fluid level survives the freeze, and the mesher draws
  ice at a flowing level at the same height the water had
  (`Tables::level_shaped`), so a frozen river keeps its slope and steps.
  Ice at a still or falling level (everything placed by hand, shore ice,
  sheets) is the plain cube it always was. Collision stays a full block.
- **Placing and mining.** Setting a waterloggable block down in water keeps
  the water; mining one takes the block and leaves the layer
  (`BlockRegistry::left_behind`) - water flows on, but ice breaks with it.
- **Drawing.** The mesher draws each half of a two-part cell as its own part
  against neighbours seen as that half, so the pebble is a pebble and the
  water around it is just more water, with no hole in the surface.

**Reloading never re-simulates fluid — it's saved exactly, like any other
block.** Every fluid cell (source, flowing, falling — not just the source a
player placed) is written to disk as a `save::FluidCell {x, y, z, block,
level}` and restored verbatim on load (`ChunkMap::set_block` +
`set_fluid_level_raw`, no `FluidQueue` involvement at all), so a build with
water in it can't come back different just because the simulation happened
to converge differently on reload. This can't reuse `BlockEdit`'s
sparse-diff-of-player-touches approach, because flowing/falling cells are
the simulation's own writes and never fire a `BlockSetEvent` in the first
place (see `set_fluid_cell`'s doc comment) — so `write_save` instead scans
every currently-loaded chunk fresh on every save and falls back to the
previously-saved data only for chunks the player didn't revisit this
session (`world::OriginalFluids`), so an unvisited lake's data is never
silently dropped either. The tradeoff is exactly what you'd expect from
"treat it like a real block": a session with a lot of flowing water makes
for a bigger save file, same as a session with a lot of block edits does.

## Lighting

`src/light.rs` propagates two independent quantities through every cell of
the world, both stored per-block alongside the block ids:

- **Block light**, in **color** — three channels (R, G, B) at `0..=16`,
  emitted by any block whose definition sets a `light`. Each channel
  propagates on its own and loses one level per block travelled, so a torch
  16 blocks away is exactly dark and two differently-colored lamps blend
  properly where they overlap.
- **Sky light** — one channel at `0..=16`, "how much of the open sky reaches
  here". Full strength straight down (a shaft stays bright all the way to
  the bottom), one level per block sideways into caves and under overhangs.

They're combined per fragment as `max(block, sky × sky_color)`, so a torch
does nothing at high noon and takes over completely at night — and the
`sky_color` half is a single uniform the day/night cycle rewrites every
frame. That's what lets **dawn, dusk and a red/blue/green moon change the
light on every surface in the world** without relighting or remeshing
anything: only the sky term moves, and the world's *shape* (which is what
the stored sky-light levels actually describe) never had to be recomputed.

### The torch

`blocks/torch.json` is the built-in light source — level 16, a warm
near-white, currently a plain solid cube (it's the lighting that makes it a
torch; a proper torch model comes later). Add your own by giving any block a
`light`:

```json
{ "id": "ruby_lamp", "light": { "level": 16, "color": [255, 40, 40] } }
```

`level` is brightness in light levels; `color` is a 0-255 RGB hue. Worth
knowing when picking one: because the channels travel independently, a
saturated color also has a *shorter reach* in whatever channels it dims — a
pure red lamp casts red 15 blocks but no green or blue at all. Keep `color`
near white for a light that reaches the same distance in every direction.

### How it's computed

The same budgeted-queue relaxation the fluid simulation uses: cells that
might be stale go on a queue, a bounded number are re-derived per frame from
their six neighbours, and only actual changes enqueue more work. Newly
generated chunks skip most of that — `terrain.rs` fills each column's
straight-down sunlight during generation, which needs no neighbour
information and settles everything outdoors, so a chunk's first mesh is
already correctly lit and the queue only has caves, emitters and chunk seams
left to resolve.

None of it is saved. Light is a pure function of the block grid, so loading
a world recomputes it exactly — unlike fluid state, where spread is
path-dependent and re-deriving it could legitimately land somewhere else.

## Day/night cycle

`src/sky.rs` runs a real-time clock: a **20-minute day** followed by a
**10-minute night**, repeating forever. The sun rises due east and sets due
west over the whole day; the moon does the same over the whole night — only
one is ever up at a time, each completing its entire rise-to-set arc within
its own phase's duration (a stylized simplification, not a real orbit: no
latitude, and the calendar below is a plain 12-month year, not a real
season length). "East"/"west" match this engine's established compass
mapping (`blocks.rs`'s face-order doc: +X east, -X west, -Z north, +Z
south).

The moon also cycles through the 8 standard phases (new, waxing crescent,
first quarter, waxing gibbous, full, waning gibbous, last quarter, waning
crescent), advancing one phase every full day/night cycle — the same
one-phase-per-day convention Minecraft uses. Every phase is a separately
masked copy of the *same* base moon texture (procedural or a custom
`moon.png`) using a proper elliptical terminator (not a flat chord — the
crescent/gibbous shapes genuinely curve with the disc), so overriding the
moon's art automatically gets correct phase shapes with no extra files.

Both are flat, always-camera-facing billboards orbiting the camera at a
fixed radius (scaled to stay inside the far clip plane regardless of
`render_distance`) — cheap, since it needs no new chunk geometry and no
real light source. This is a fully unlit renderer (see "Performance
design" below), so instead of dimming an actual light, the day/night cycle
writes one global brightness multiplier into both chunk materials every
frame (`ChunkMaterialParams::sky_light`) along with the sky/fog color —
two tiny uniform writes, no remeshing, despite touching the whole world's
apparent lighting at once.

The sun and moon are procedurally textured by default (a warm gold disc
and a pale cratered disc), exactly like block textures: drop a
`textures/sky/sun.png` or `textures/sky/moon.png` to override one with
real art instead — see `textures/sky/README.md`. Unlike the block atlas,
there's no shared resolution to keep in sync; each is one standalone
sprite used at its own native size.

A full moon occasionally gets a special color — purely cosmetic for now,
no gameplay effect yet. Twice a year there's a **red moon**, once a year a
**blue moon** (always in summer, never winter), and once a year a **green
moon** (always in autumn, never spring). Each is a plain color tint
multiplied into the moon's existing texture (`sky::moon_event_tint`), not
a separate sprite. A "month" is defined to match the moon's own 8-day
phase cycle (so every month's full moon lands on the same day-of-month),
and a "year" is a plain 12-month calendar — `DayNightClock::season` is a
pure calendar concept with no other effect yet; it exists solely to make
the winter/spring exclusion rules real.

Which month(s) each event lands on is re-rolled every year (seeded by the
world's own seed plus the year number — reproducible, not truly random,
same as terrain generation) rather than fixed, never collides with another
event that same year, and never lands right next to one either — including
across a year boundary (a claimed December is never immediately followed
by a claimed January). There's always at least one plain-moon month
between any two specials, so a blue/red/green chain can never land three
months in a row. All of it — how
often an event happens, which seasons it's excluded from, whether it even
needs a full moon, whether it needs that breathing-room month — lives in
one small declarative table (`sky::MOON_EVENTS`), not hand-written per-event
logic; adding a fourth event or retuning an existing one is a one-line
table edit, not new code.

Computing a year's schedule from scratch walks forward from year `0`
(needed for the boundary rule above), so it's `O(year)` — negligible for
any realistic save, but `update_sky` avoids paying it every single frame
anyway via `sky::MoonScheduleCache`: since the game only ever advances one
year at a time, the cache just remembers the previous year's outcome and
extends it by one, making the common case `O(1)` regardless of how old a
save gets.

The clock (and moon phase) persists per-world (leaving and reloading
resumes the same time of day/phase rather than resetting to dawn and a new
moon), the same way fluid state does — see "Fluid flow" above for the
general "don't silently lose continuous state on reload" principle this
follows.

## The two kinds of tags

Everything that defines or adjusts a block is one of two kinds of tag, and
they do different jobs.

| | **Definition tags** | **State tags** |
|---|---|---|
| What | A fixed fact about a block or item *type* | A value on one *placed* block |
| Example | torches glow, glass shatters at 5 m/s, stone's hardness | which way this log lies, whether this pebble is in water, how far this water has flowed |
| Where | `blocks/*.json`, `items/*.json` | per cell, in the world |
| Changes | never in play; read once at startup | at runtime: a player action, a command, the simulation |
| Code | `src/tags.rs` (`DEFINITION_TAGS`) | `src/states.rs` (`STATE_TAGS`) |

**Definition tags** are listed in one table, grouped as identity, appearance,
world, mining, combat, throwing and breaking, each with the files that take it
and what it does. The loaders warn about any key that isn't in the table and
suggest the nearest real one (`hardnes` -> did you mean `hardness`), so a typo no
longer silently leaves a block that doesn't glow. Unknown keys still load: they
are almost always typos, but they're also what an older build sees in content
written for a newer one.

**State tags** are `axis` (which way a rotating block lies), `waterlogged`
(`none`, `water` or `ice` sharing the cell) and `level` (a fluid's flow level:
`source`, `1`..., `falling`). Each is one row in `STATE_TAGS`: its values for a
given block, how to read it, how to write it. `states::read_states` and
`states::set_state` are the one interface; the `/blockstate` command and any
future interaction use them, and a tag only exists on blocks it applies to
(`level` isn't a thing on stone). To add one, add a row.

**Which blocks have which state tags is itself a definition tag**: `"states":
["waterlogged"]` in a block file. Nothing is a state of a block unless it's
enabled, so stone has none, a log has `axis`, a pebble has `waterlogged`. The
behaviour tags imply their state (`rotation: log` -> `axis`, `waterloggable` ->
`waterlogged`, a fluid or what one freezes into -> `level`), so a block that
already says so needn't repeat it, and declaring the state turns the behaviour on.
Each state tag also says what it *can't* go on, and the game refuses to start
with a block that breaks that rule: a full cube can't be `waterlogged` ("nowhere
for water to go"), only a full block can have an `axis`, and only a fluid (or
what freezes from one) has a `level`. A misspelled state name is refused too.

**Storage.** Every cell has one 16-bit *state word* (`Chunk::state`), the same
two bytes it used to spend on a separate fluid-level array and axis array, so
memory is unchanged. **Which bits mean what depends on the block in the cell.**
The built-in tags sit at fixed positions on the blocks that enable them (`level`
bits 0-7, `axis` 8-9), and a block's own states take whatever bits its built-in
tags leave free, so a block that uses none can use all 16. A cell holds one
block, so two blocks (or two mods) never compete for bits. Nothing is reserved
globally, and the layouts are computed per block at startup
(`BlockRegistry::finalize_states`), from every registered block.

A block declares its own states with `state_defs`: a name and either a count or
value names, the first value being the default.

```json
{ "id": "battery", "state_defs": { "charge": 8, "mode": ["off", "on", "auto"] } }
```

`/blockstate x y z charge 5` reads and sets them like any built-in tag, they're
saved with the world, and breaking or replacing the block clears them. A block
that needs more bits than are free is refused at startup with a message that
points at extended storage.

**Extended storage.** Add `"state_storage": "extended"` and the block's states live
in a per-chunk table instead of the shared word: up to 256 bits per block, enough
for dozens of small states. Only cells of that block pay for it, and only while a
state differs from its default (a freshly placed one has no entry; setting
everything back to default removes it), so a world with ten machines stores ten
small entries however big it is. Normal blocks never look in the table. It is
saved with the cell's edit (`SAVE_FORMAT` 8) and cleared when the block is replaced.
A block uses one tier for all its own states; `axis` and `level` stay in the word.

```json
{ "id": "machine", "state_storage": "extended",
  "state_defs": { "fuel": 256, "heat": 256, "recipe": 256, "output": 256, "mode": ["idle", "run"] } }
```

`waterlogged` is the one tag *not* in the word: its value is which block id the
cell holds (`pebble@water`), because collision, light and meshing are all looked
up by block id. A waterloggable block keeps the level bits clear of its own
states, so it can swap with its water variant without moving anything.

Because the meaning of a bit depends on the block, any code that touches the
word has to check the block first, and any code that changes the block has to
clear the word (`ChunkMap::set_block` does). [`docs/STATE_ACCESS.md`](docs/STATE_ACCESS.md)
lists every such place, and a test fails if a new file starts touching the word
without being added to it.

On disk, an edit now saves the cell's whole state word (`SAVE_FORMAT` 7); an older
world's edits, which only had an axis, still load. Fluid levels are still saved by
the fluid scan.

## Resource packs

A resource pack is a visual and audio layer: textures, block models and sounds. It can change how
anything *looks and sounds*, never how it *plays*. Two kinds:

- **A player's pack** is a folder in `resourcepacks/` next to the game (an optional `pack.json`
  gives it a `name` and `description`). Turn packs on, off and reorder them in **Settings > Packs**;
  the top of the list wins. It can retexture any block, replace any block's model, replace any
  sound and re-skin the menus.
- **A mod's pack** is the `resourcepack/` folder at the top of the mod. It belongs to the mod: it
  loads when the mod is enabled in a world and can't be switched separately. It is now the **only**
  place a mod keeps textures, models and sounds (the old `textures/`, `models/`, `sounds/` folders
  at a mod's top level are no longer read, and the Mods screen says so).

```text
<pack>/
  textures/blocks/<name>.png   block tiles
  textures/gui/<element>.png   menus (see textures/gui/README.md)
  textures/sky/<name>.png      sun and moon
  models/blocks/<block>.json   how a block is drawn
  sounds/<event path>.ogg|.wav sounds
```

**Two ways to load a pack.** *Global* packs (Settings > Packs, or **Resource Packs** in the pause
menu) apply to every world and to the menus. *World* packs (the **This world** tab in the pause menu)
apply only inside that world and are saved with it (`world_packs` in its `meta.json`).

**One load order for mods and packs.** Resource packs can override mods and mods can override resource
packs, so a world has a single list - the **Load order** tab in the pause menu - holding its mods,
its own packs and the global packs. **The game is always at the bottom and can't move.** Higher wins.
- **Auto** (the default) puts each mod after the mods it builds on - a mod names them with
  `"load_after": ["base_mod_id"]` in `mod.json`, and a mod whose block/item files patch
  `base_mod_id:thing` is placed after `base_mod_id` without saying so - then the world's packs, then
  the **global packs on top**, in the order you gave them in the global list. That is how an add-on
  for a popular mod lands after it.
- **Manual**: move any entry Up or Down; the order is saved with the world. New mods you add go in
  just under the global packs, new global packs go on top, entries that are gone drop out, and
  **Auto** forgets your order (global packs back on top).
Packs reorder at once while playing. A mod's *data* (blocks, items, recipes) is laid over the game in
the order the world was opened with, because block ids must not move under loaded chunks - a changed
mod order applies to the data next time the world is opened (its art follows immediately).

**Changes apply at once, in menus and in game.** Pick or reorder a pack and the menu textures,
controller glyphs and sun/moon retexture on the spot; in a world the block art, models and sounds are
rebuilt and every loaded chunk is remeshed (a short burst of remeshing, no re-entering the world).
Per-world packs are swapped out again when you go to another world.

**Ctrl+R reloads resource packs** (rebind it under Controls > Interface > "Reload resource packs").
It rescans the `resourcepacks/` folder and rebuilds everything from the files on disk, so a pack
author working inside the game folder sees an edit in game without leaving the world. Files are not
watched; the key is the trigger.

**Names:** in the game's folder and in a player's pack a plain name is the core game's (`dirt`) and
`<mod id>/<name>` is a mod's; inside a mod's pack a plain name is the mod's own and
`craftmjne/dirt` re-skins vanilla.

### Block models

`models/blocks/<block>.json`:

```json
{
  "boxes": [
    { "from": [0, 0, 0], "to": [16, 8, 16], "textures": { "all": "stone", "top": "stone_top" } },
    { "from": [6, 8, 6], "to": [10, 14, 10], "textures": { "side": "planks" } }
  ],
  "collision": "block"
}
```

Boxes are in sixteenths of the block's own cell. A face with no texture isn't drawn. A face's texture
is `east`/`west`/`top`/`bottom`/`south`/`north`, else `side` (the four sides), else `all`. Each face
shows the part of its texture matching where it sits in the block (the lower half of a slab shows the
lower half of the side texture). A model with up to 64 boxes is fine; a broken one is skipped with a
log line and the block keeps its normal look.

**A model is only a picture.** The block still collides, mines and is targeted as whatever its own
definition says - make a full block *look* like a half block and you still bump into a full block. One
exception: a model in a **mod's** pack may say `"collision": "model"`, and then its boxes *are* the
block's collision and hitbox (you walk over the empty half; the outline and the ray follow the boxes).
The highest mod pack with a model for the block decides; a player's pack can redraw the block but never
move its collision. Lighting, what blocks light, and everything else physical stays with the block
definition. Neighbours still hide faces against a model that fills its whole cell, so a plain
retextured cube costs nothing.

Limits, on purpose for now: inventory icons and dropped items use the block's textures rather than the
model, models take no biome tint, thrown items and the climb assist still treat a model block as a full
cell, and a model always faces the same way (blocks have no yaw).

## Typeface and emoji

Every piece of text in the game - menus, HUD, chat, tooltips - uses one built-in pixel typeface
(`font_gen.rs` writes a real TrueType file in memory; `pixel_font.rs` holds the drawings). The
default font that ships with Bevy is turned off, so there is no other font unless you add one.

- **Grid.** The font is 12 pixels tall (caps 8 rows, lowercase 6, descenders below the baseline,
  accents above), a little larger than the 8x8 convention and independent of the 16x16 block textures.
- **Sharp pixels.** Text is drawn without smoothing, and sizes snap to a multiple of 12 (a size from
  about 13 to 17 draws at 12, 18 to 29 at 24, and so on), so a font pixel is always a whole number of
  screen pixels. Vector fonts from packs are *not* snapped.
- **Characters.** ASCII, the Latin-1 letters with accents (`ç`, `ñ`, `ü`...), common punctuation, `€ £ ¥`.
- **Other scripts.** Cyrillic (Russian, Ukrainian, Belarusian, Serbian letters), Hebrew, Korean,
  Arabic and Hindi (Devanagari) are in the same font, so they work everywhere text does, including
  chat. Details and limits are under *Other scripts* below.
- **Chinese and Japanese** (`src/cjk.rs`): hiragana, katakana (and halfwidth forms), Bopomofo, CJK
  punctuation and symbols, fullwidth forms, **every ideograph of the Basic Multilingual Plane** (the 20,992
  unified ideographs, Extension A's 6,592, and the compatibility ideographs), and the symbols that
  Japanese and Chinese text uses (circled numbers, arrows, maths, box drawing, shapes, Greek). Pinyin
  tone marks and Hepburn macrons (ā ǎ ǜ ...) are part of the Latin font. The glyphs are GNU Unifont's
  16x16 bitmaps under the SIL OFL (`third_party/unifont/`), in three styles because one code point is
  drawn differently by language: **Simplified Chinese** (the base), **Traditional Chinese** and
  **Japanese**. `CjkStyle` (a resource; `Style::from_language("ja")` maps a language tag) picks the
  style and the font is rebuilt in the background when it changes; translations will set it from the
  player's language. A glyph is 16 pixels tall and lands on whole screen pixels like the Latin text, so
  CJK looks 4/3 the Latin height and lines containing it use a taller line height. Coverage limits: of
  the ~70,000 rare ideographs in Extensions B to I (names, historical texts) Unifont draws about 8,000,
  so the rest show as an empty box; Hong Kong or other regional forms beyond the three styles are not
  made. `CRAFT_SHOT=cjk CRAFT_CJK=ja|zh-Hans|zh-Hant` renders a sample screen; `tools/build_cjk_data.py`
  rebuilds the data from a Unifont release.
- **Cherokee, Cree, Inuktitut, Icelandic, Yup'ik and Ndyuka (Njuka)** (`src/syllabics.rs`, `src/pixel_font.rs`):
  - **Cherokee** (the 86 capital syllables, the lowercase ones and the 80 small letters of the Cherokee
    Supplement) and **all of the Canadian Aboriginal syllabics** (640 + 80 extended: Cree, Inuktitut,
    Ojibwe, Naskapi, Blackfoot, Carrier ...) are GNU Unifont bitmaps in a small fallback font
    (`third_party/unifont/`), complete for what Unicode encodes. Same 16-pixel geometry as the Chinese
    and Japanese glyphs, so text containing them gets the taller line height.
  - **Icelandic** (ð þ Ð Þ added; æ Æ ö and the accented vowels were there), **Yup'ik** (voiceless ḿ ń,
    the modifier apostrophe ʼ and the tie of u͡g), **Inuit Latin** (ł Ł ŋ Ŋ) and **Cree Latin** (â ê î ô
    and the macron forms) are drawn in the game's own 12-row Latin font. Ndyuka's Latin alphabet is
    plain Latin with an acute for tone, which the font already had.
  - **Not in Unicode, so not made:** Ndyuka's own **Afaka syllabary** (56 signs, proposed for U+16C80-16CCF)
    and Yup'ik's **Yugtun script**. There is nothing standard to encode them as and I had no reference
    shapes to draw them from, so they are not guessed at. Capital eth/thorn and the Inuit letters are
    first-pass drawings that I judged only from a software-rendered screenshot.
  - `CRAFT_SHOT=natives` renders a sample.
- **Greek, Armenian, Georgian, Syriac, Mandaic, Coptic, Ethiopic, Tifinagh, N'Ko and Vai**
  (`src/pixel_greek.rs`, `src/scripts.rs`):
  - **Modern Greek** (all letters, the tonos and dialytika, ΐ ΰ, the question mark and ano teleia, µ) is drawn
    in the game's own 12-row Latin style. Capital tonos sits above the letter like every accent here, not to
    its left as in print.
  - **Everything else is GNU Unifont's bitmaps** (SIL OFL, `third_party/unifont/`), complete for what Unicode
    encodes: polytonic Greek (Greek Extended) and the archaic letters, **Armenian** (and its ligatures),
    **Georgian** (Mkhedruli, Mtavruli, Asomtavruli, Nuskhuri), **Syriac** (with its supplement), **Mandaic**,
    **Coptic**, **Ethiopic** (with its supplement and both extensions), **Tifinagh** (Tuareg and neo-Tifinagh
    are one Unicode block) and **Vai** and **N'Ko**. Each is checked against the Unicode ranges by a test.
    Combining marks (Syriac points, N'Ko tone marks ...) take no room and sit over the letter before.
  - **Joining.** Syriac, Mandaic and N'Ko are cursive and right-to-left. Unifont draws only each letter's
    isolated shape, so the initial, medial and final forms are *derived* (the letter plus a connecting stroke
    on the baseline) and chosen by `GSUB` from each letter's Unicode joining type. They read as joined writing
    but are not calligraphy, and Syriac's special final-Alaph forms are not made. Judged from a
    software-rendered screenshot only, not by someone who reads these scripts.
  - Same 16-pixel geometry as Chinese/Japanese; `CRAFT_SHOT=world` renders a sample.
- **Mongolian (Cyrillic and traditional), Manchu, South and Southeast Asian scripts, Yi, Miao, Nushu,
  Vietnamese** (`src/scripts.rs`, `src/pixel_vietnamese.rs`, `src/pixel_scripts.rs`):
  - **Own style:** **Vietnamese** (all 134 letters, with stacked circumflex/breve + tone, the horn, the dot
    below and the barred d; capitals set the two marks side by side) and the Cyrillic letters Mongolian,
    Buryat and Kalmyk add (Ө ө Ү ү Ң ң Һ һ).
  - **Unifont bitmaps, complete for what Unicode encodes:** traditional **Mongolian**, Todo, **Manchu**
    and Sibe (one block), Mongolian Supplement, **Phags-pa**, Soyombo, Zanabazar Square; **Gujarati,
    Gurmukhi** (Punjabi), **Bengali** (and Assamese), **Tamil, Kannada, Malayalam, Sinhala**, Telugu and
    Odia; **Tibetan**; **Thaana**; **Ol Chiki** (Santali); **Burmese** (all three Myanmar blocks);
    **Thai, Lao, Khmer**; the Philippine scripts (**Baybayin/Tagalog**, **Hanunoo**, Buhid, Tagbanwa);
    **Buginese (Lontara)**, **Balinese, Javanese, Sundanese, Batak**; **Yi**; **Miao (Pollard)** and Lisu;
    **Nushu**; and the zhuyin tone marks (the Bopomofo letters came with the CJK work).
  - **What is and isn't shaped, said plainly.** Letters, marks and reordering of vowel signs are all
    there, and **Thai and Lao come out as proper clusters**. Unifont's data has no conjunct, stacked or
    ligature glyphs (its OpenType build has no `GSUB` at all), so `conjuncts.rs` *derives* them from the
    consonant bitmaps: **half forms** (Bengali, Gujarati, Odia, Malayalam), **subjoined letters** under the
    letter before (Kannada, Telugu, Khmer, Burmese, Javanese, Balinese, and Gurmukhi's ha/ra/va/ya),
    **reph** (Bengali, Gujarati, Odia, Kannada) and **Tibetan stacks** (its subjoined letters moved under
    the letter). They are squeezed copies of the plain letters: readable and correctly placed, not the
    calligraphic ligatures a native reader would see. Also derived: **Sinhala** conjuncts (yansaya,
    rakaransaya and the stacked forms - written with a zero-width joiner after the al-lakuna, as in Unicode;
    without the joiner the al-lakuna stays visible), **Sundanese** (its virama), **Tamil** க்ஷ and ஸ்ரீ/ஶ்ரீ (the two
    real Tamil ligatures; every other Tamil cluster keeps its pulli), **Bengali ya-phala** (the hook after a whole
    base letter), **Malayalam chillus** typed as consonant + virama + joiner (they become the atomic chillu
    letter), and **vowel signs under a subjoined letter** (Khmer, Burmese, Javanese, Balinese, Sundanese,
    Sinhala, Tibetan: the pair becomes one glyph with the sign moved below the stack). Batak, Buginese and the
    Philippine scripts have no conjuncts to make; what they needed was checked instead: Buginese's vowel e is
    reordered before its letter, and the Philippine kudlit marks take no room. Not done: Bengali ra-phala as a
    hook (it is a half form), Odia ya-phala, the special ligature shapes of Indic scripts (Bengali ক্ষ looks like a
    half ka + ṣa), Kannada/Telugu vowel signs (Unifont draws them as spacing letters, so nothing overprints),
    and Malayalam post-base forms. `CRAFT_SHOT=conjuncts` renders samples.
    Everything was judged only from software-rendered
    screenshots, and the sample words in the preview screens are only to exercise the shaping, not to be
    correct prose.
  - **Mongolian, joined, two ways** (`src/scripts.rs`, `src/mongolian.rs`). Mongolian, Todo, Manchu and Sibe are
    written *downward*, joined along a spine, with columns running left to right.
    - **In any ordinary text** (chat, menus, tooltips) they are drawn **turned a quarter turn**, the way
      Mongolian is shown on a horizontal line: every letter has its initial, medial, final and isolated
      form, chosen by the font's `GSUB` from the neighbours, so a word is one continuous spine reading left
      to right.
    - **Upright**: the **`VerticalText`** component (`VerticalText::new(text, max_height_px, scale)`) draws the
      real vertical script as an image - words stacked downward on one spine, long text wrapping into
      columns that run left to right (a newline starts a column, `U+202F` leaves a small gap, `U+180E`
      breaks the joining, `U+180A` is a connecting stroke, variation selectors are accepted and ignored);
      tint it with `ImageNode::color`. Nothing in the game's own UI uses it yet (there are no
      translations to show); a translated string that should read vertically should be spawned as a
      `VerticalText` instead of a `Text`.
    - Both come from Unifont's isolated letters: the forms are derived by dropping the head or tail and
      extending the spine, so every letter gets the same treatment where a real font draws each positional
      form by hand. The contextual letter rules, the variation selectors' alternate glyphs and rotated
      digits are not made.
  - **Joined scripts, checked in the real text engine.** A test shapes Arabic, Persian, Syriac, N'Ko, Mandaic and
    Mongolian through cosmic-text (the engine Bevy uses) with the game's own fonts registered, and asserts
    every letter of a word takes its initial / medial / final form rather than its isolated one; others check
    lam-alef, Devanagari conjuncts, and that mixed-script lines stay joined. `CRAFT_SHOT=joined` renders them.
    The conjunct scripts (Bengali ... Balinese) have their own engine-level tests: three characters, two glyphs.
  - `CRAFT_SHOT=asia1`, `asia2`, `mongol` (before/after) and `joined` render samples.
- **Emoji.** 12x12 colour emoji (about 700 drawn, the rest generated, see below) drawn in the game's style (faces, hearts, animals, food,
  weather, objects, arrows, symbols, zodiac, flags). They are colour glyphs (COLR/CPAL) in a second
  built-in font used as a fallback, so they work anywhere text does, including chat and signs. In chat,
  `:name:` becomes the emoji (`:heart:`, `:fire:`, `:flag_us:`); anything that is not a known name is
  left as typed. Following the current emoji standard, 🔫 is a **water pistol** (`:water_pistol:`).
  - **Flags.** All 258 country and territory flags (two regional-indicator letters), England, Scotland
    and Wales, the flag-on-a-pole emoji (white, black, triangular, checkered, crossed) and the rainbow,
    transgender and pirate flags. **Texas**: the Lone Star flag of the Republic of Texas has no Unicode
    emoji, so it is written the way Unicode would write the US-TX subdivision flag (🏴 followed by the
    tag letters `u s t x` and the cancel tag) and has the shortcode `:flag_texas:`. A pair of
    regional-indicator letters that is not a country shows as two letter tiles.
  - **Sequences.** Flags, skin-toned hands (👍🏻 ... 👍🏿 for fifteen hand and gesture emoji), keycaps
    (`1️⃣`, `#️⃣`, `*️⃣`) and the ZWJ flags are ligatures in the font, so they work anywhere.
  - **Every Unicode emoji is covered, but not every one is drawn.** `src/emoji/data/standard.txt` is the
    official list of all 3,963 fully-qualified emoji (Unicode emoji-test 18.0), and a test shapes each one
    through the real font and requires exactly one glyph. What is behind each glyph differs:
    - *Drawn or made by rule* (hand art, flags, keycaps, zodiac, toned hands, colour shapes).
    - *Generated by rule* (`src/emoji/standard.rs`): skin tones recolour existing art, people, couples
      and families are drawn from a small person sprite (hair, sex, age, skin tone, a heart / kiss /
      handshake between two, a prop for a profession), and a few combined emoji (black cat, heart on
      fire) are a picture with a badge.
    - *Labelled tile*: everything else is a tile in its group's colour with two letters from its name
      (a skin-toned one takes the tone as its colour). **These are not art** - they exist so every emoji
      works, can be told apart, and can be replaced one at a time. `/emoji-report` shows the counts and
      the first tiles still waiting for a drawing; replacing one is adding a drawing in `src/emoji/`,
      which takes the place of its tile automatically.
    Every glyph also has a `:cldr_name:` shortcode in chat.

### Other scripts

| Script | What is there | Limits |
|---|---|---|
| Cyrillic | all of А-я, Ё ё, Ї ї, Є є, Ґ ґ, Ў ў, І і, Ј ј | |
| Hebrew | the 22 letters, 5 final forms, vowel points (niqqud), dagesh, shin and sin dots | right-to-left layout is the text engine's |
| Korean | all 11,172 syllables (가-힣) plus the standalone letters (ㄱ-ㅣ) | drawn from strokes (`pixel_hangul.rs`), so they are small and plain |
| Arabic | the letters with their joining forms (initial, medial, final, isolated), ء أ إ آ ؤ ئ ة, tatweel, ، ؛ ؟, the lam-alef ligature (ل + ا أ إ آ), vowel marks (tashkeel), Persian and Urdu letters (پ چ ژ گ ک ی ٹ ڈ ڑ ں ھ ہ ے), Arabic-Indic and Persian digits | letter bodies are short next to the Latin text; some Urdu letters (ٹ ڈ ڑ) use a simplified mark; the digits ٢ ٣ ٤ ٥ ٦ are best-effort shapes |
| Hindi (Devanagari) | the whole Devanagari block (and Vedic extensions) from Unifont, with **half forms** (क्ष, ज्ञ, त्त, प्र...), reph (र् over a consonant) and the below-base ra (क्र) derived by `conjuncts.rs`; right-hand signs and visarga are overlaid on their letter instead of taking their own cell | consonants with no stem keep a visible halant; conjuncts are composed, not the special ligature shapes; 16-pixel letters among 12-pixel Latin |

How they work, so a pack author knows what to expect:

- Arabic joining is done by the font's own `GSUB` table (`init`/`medi`/`fina`), which `font_gen.rs`
  writes. Each joined form is its own glyph, named by a private-use code point (U+E000 onward) that only
  exists for the substitution.
- Devanagari's reordering (the *i* sign goes before its consonant) is done by the text engine; the font's
  marks have no advance and are drawn at negative x over the consonant they follow, so the font needs no
  positioning table. Conjuncts are `half`/`rphf`/`blwf` ligatures. Arabic and Hebrew marks are centred
  over their letter by the shaper's own fallback positioning.
- A pack's `fonts/` can replace or extend any of this, as above.

### Fonts in packs, and the `font` tag

Any text carries a **font tag**: a *role* or a *font name*. Roles: `default`, `title`, `button`,
`chat`, `tooltip`, `hud`. A resource pack (or the game folder) can supply fonts in `fonts/`:

- `fonts/<name>.ttf` / `.otf` - a vector font.
- `fonts/<name>.png` - a bitmap sheet of 16x16 cells (cell size = image width / 16; ink = alpha >= 128).
- `fonts/fonts.json` - `{"title": "myfont", "chat": "other"}` points a role at a font name. `default`
  replaces the font for everything that has no role of its own.

A block or item file can say `"font": "myfont"` (a role or a name); its name is then drawn in that font
in tooltips. A tag that names nothing falls back to the default font, never to a missing one. Fonts
reload with the rest of a pack (Ctrl+R).

## Sound

Every sound is an *event*: `block.<group>.break|place|step`, `item.pickup`, `entity.hit`,
`player.hurt`, `ui.click`. A block picks its group with a `sound` tag (`stone`, `dirt`, `grass`,
`wood`, `sand`, `gravel`, `glass`, `snow`, `water`; the default is `stone`, and a group nobody has
sounds for plays `stone`'s). Breaking and placing play at the block; footsteps follow the block under
your feet every 1.8 blocks; taking damage, picking things up and clicking buttons play their own.

The game ships **no audio files**: every default sound is synthesised at startup (seeded noise shaped
per group, three variants each) in `sound.rs`. A pack replaces any of them by putting a file at the
event's path under its `sounds/` folder - `sounds/block/stone/break.ogg` or `.wav`; several variants
are `break1.ogg`, `break2.ogg` and so on (the highest layer with any file for an event replaces the
lower layers' completely). Sound paths are not namespaced - the event names are one shared vocabulary -
so a mod's pack overrides vanilla just by providing the same path, and a player's pack outranks both.
A file that isn't really WAV or Ogg is skipped with a log line and the default plays.

**Settings > Sound** has four sliders (master, blocks, player and items, menus), saved per launcher
instance in `sound.json`. A sound at a position fades out between 4 and 32 blocks from you. On a
machine with no sound device the game runs silently.

## Mods

A mod is a folder of data in the game's `mods/` folder (next to the executable;
the Mods screen shows the exact path). No code: everything a mod can do is
something a block or item file already says (see
[the two kinds of tags](#the-two-kinds-of-tags)).

```text
mods/any_folder_name/
  mod.json            { "id": "granite_pack", "name": "Granite Pack", "version": "1.0", "target_ver": ["2.0"] }
  blocks/*.json       same format as the game's own blocks/
  items/*.json
  recipes/*.json
  tiers/*.json        tool tiers (material, mining speed, damage)
  rarities/*.json     what a tool's quality is called: id, name, min percent, colour
  (mod.json also takes "load_after": ["other_mod_id"] for the automatic load order)
  keybinds/*.json     controls added to Settings > Controls > Keybinds (see below)
  resourcepack/       the mod's textures, models and sounds (see Resource packs above)
```

[`docs/example-mod/`](docs/example-mod) is a complete working one (a test keeps it
working): copy it into `mods/` to try it.

**Every mod needs an `"id"` in `mod.json`, or it doesn't work** (the Mods screen
lists it as "won't work" and says why). The id is the prefix of everything the mod
adds: a mod `granite_pack` with a block `granite` adds `granite_pack:granite`. So two
mods can both add a `granite` and both exist, where before they would have clashed.
Ids are letters, digits, `_` and `-`; `craftmjne` is the game's own.

**`target_ver` says which game versions the mod was made for** - one string or a
list (`"1.3"`, `["1.2", "1.3.x"]`). An entry covers every version that starts with
it (`1.3` covers `1.3.0` and `1.3.7`), and `x` or `*` matches any number in that
place. Load a mod against a version it doesn't name (or one that names none) and the
game **warns, never refuses**: an orange line on the Mods screen, `(!)` beside it in
the create/change-mods lists, and a "Mod warning" line in chat on entering the world.
It's how a mod's author tells players which mod goes with which game version as the
game keeps updating.

**If two mods in a world declare the same id, nothing breaks**: the game renames
them for that world (`granite_pack_1`, `granite_pack_2`, in folder order), keeps
those names for good (a block saved as `granite_pack_1:granite` must stay that
even if the other mod is removed later), and the world carries on.

**Controls.** A mod can add entries to Settings > Controls > Keybinds with one JSON file (or
an array) per control in `keybinds/`:

```json
{ "id": "noon", "name": "Skip to noon", "category": "Weather Pack",
  "default": "Ctrl+N", "command": "/time set noon" }
```

`id` is required (letters, digits, `_`, `-`) and becomes `<mod id>:<id>`, e.g.
`weather:noon`, so it can't collide with the game's own controls or another mod's.
`name` is what the screen shows, `category` the heading (default: the mod's name).
`default` is the key combination it starts on - leave it out (or `"none"`) to start
unbound, so a mod never takes a key from the player unasked. `default_pad` is the same
for a controller (`"south"`, `"a"`, `"right bumper"`, `"dpad up"`, `"none"`, as the
Controller tab names them); a control's keyboard and controller bindings are separate, the
player can change either, and a `command` runs from whichever is pressed. `command` is a chat
command (slash optional) that runs when the control is pressed in a world that uses
the mod; without one the control is only for code to read (below). The player can
rebind, unbind and search it like any other, and the game flags it in red if it
shares a key with another control. A broken file is skipped with a note on the
Keybinds screen. These are read from the installed `mods/` folder, so they appear
in Settings whether or not a world uses the mod, but only act in worlds that do.

**Naming inside a mod's files:**

- A new thing is written with its plain name (`"id": "granite"`) and gets the prefix.
- A reference (a drop, a recipe input, a texture, a tier) written plainly means *this
  mod's, if it has one by that name, otherwise the core game's*. Write
  `other_mod:thing` or `craftmjne:thing` to be explicit.
- The game's own names stay plain (`dirt`); `craftmjne:dirt` is another way to write it.

**Mods sit on top of the game; they never change it.** The game's own files are
only ever read, so taking a mod off puts everything back exactly as it was:

- **Add** a block, item, tool tier or rarity with a plain id. A tier is
  `{"id": "copper", "mining_speed": 2.5, "damage": 2.0}` and a tool names it
  (`"tool": {"kind": "pickaxe", "tier": "copper"}`, which means *this mod's* copper
  if it has one); a rarity is `{"id": "mythic", "min": 95, "color": [1, 0.2, 0.2]}` and
  splits whichever rarity covers 95+.
- **Change** one that exists by giving its full name and only the keys to change:
  `{"id": "craftmjne:dirt", "hardness": 20}` makes dirt take 20 times as long to dig
  and leaves everything else about dirt alone. Objects merge key by key, and a key set
  to `null` goes back to its default. A mod can patch another mod's blocks the same
  way (`other_mod:granite`) if it loads after it.
- **Replace** a recipe with `"replaces": "craftmjne:planks"` in the new recipe file.
- **Re-skin** a core block with `resourcepack/textures/blocks/craftmjne/dirt.png`. A mod's own
  textures are plain `resourcepack/textures/blocks/granite.png`. **Re-model** one with
  `resourcepack/models/blocks/craftmjne/dirt.json`; add `"collision": "model"` there to make the
  new shape the real collision and hitbox.

With several mods, later ones (by folder name) are laid over earlier ones.

**Mods are chosen per world.** Not every installed mod is used in every world: when
you create one, tick the mods it should use. Each world gets its own content when you
enter it, and the next world gets its own again.

**Where a world keeps its mods** (a setting for new worlds, and per world):

- **Game folder** (the default): the world records which mods it uses and runs the
  copies in `mods/`. One copy of each mod on disk. The catch: if you remove a mod from
  `mods/`, every world that uses it can't open.
- **In the world**: the chosen mods are copied into the world's own `mods/` folder when
  it's created. It no longer depends on the game's folder, and you can drop a mod
  folder straight into one world's `mods/` without ever installing it in the game. The
  catch is one copy per world...
- **Branch**: ...which a branch avoids. On a world kept in-world, click **Branch** (or
  choose it when creating a world) to make a new world that uses *that* world's mods
  and stores none of its own. It shares the host's fate: if the host is deleted or
  loses a mod, the branch loses it too. Only an in-world world can be branched from,
  and a branch can't be branched.

**Changing a world's mods later.** Click **Mods** on a world in the list to tick or
untick them. Nothing made with a mod is lost by turning it off: the blocks you built,
the fluids, the items in your pack and on the ground stay in the world's save, out of
sight, and come back exactly where they were when you turn the mod on again (an item
that comes back with no room in the pack lands on the ground beside you). If you build
somewhere a hidden block was, the new block wins. In-world worlds don't delete a mod
you turn off, they put it aside in the world (`mods-disabled/`). A branch's mods are
changed on its host.

**A world that can't find its mods doesn't open.** It stays in the list, greyed, with
the reason (which mods are missing, or that its host is gone). Loading it without them
would drop their blocks from its next save, so it refuses instead, and nothing writes
to its save. A mod whose files are broken is reported the same way, naming the file.
Open **Mods** on that world to turn off the mod that's gone; its things wait in the save.

Not moddable yet: entities, tool *kinds* (axe, pickaxe, ...; a mod's tools use the
existing kinds), world generation, and anything needing code.

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
| `waterloggable` | `false` | can share its cell with a fluid or what one freezes into - see [Waterlogging](#waterlogging) |
| `freezes_to` | none | on a fluid: the block its exposed still sources freeze into (`"ice"`); that block becomes a layer every waterloggable block gets a variant for |
| `flow_distance` | `0` | how far (in blocks) this fluid spreads from a source before drying up; also controls the slope — long distances step down gently, short ones drop off steeply |
| `solid` | `true` (`false` if `fluid`) | collides with entities |
| `selectable` | `true` (`false` if `fluid`) | can be targeted by the crosshair |
| `replaceable` | `false` (`true` if `fluid`) | placing into this cell overwrites it |
| `shape` | `"cube"` | `"plant"` is two copies of the top texture crossed in an X, `height` tall (see `height`), walk-through, replaceable, flat icon, needs solid ground; `"floor"` lies flat on the block below (pebbles, twigs): only its top texture is drawn, a hair above the ground. Defaults to no collision, see-through, replaceable, a flat icon, and needs something solid under it |
| `height` | `1.0` | how tall a `"plant"` stands in blocks (at most 1 per cell; a taller plant is a block with an `upper` half). A plant shows the lower part of its tile when shorter than 1, so pixels stay square |
| `breakable` | `true` | whether it can be mined at all, in either mode; bedrock sets this `false` |
| `hardness` | `1.0` | how long it takes to mine in Survival: `hardness` x 1.5 seconds by hand. `0` is instant (torch), dirt `0.5`, stone `1.5` |
| `drops` | `true` | what mining it in Survival gives: `true` itself, `false` nothing, or a block id (`"dirt"` for grass). An unknown id fails at startup |
| `max_stack` | `124` (`DEFAULT_MAX_STACK`) | how many fit in one inventory/hotbar slot — see "Stacking" above |
| `item` | `true` | `false` means this block has no inventory item at all — left out of Creative's grid, can't be middle-click picked. `air` is the built-in example; use it for a block that's only ever meant to be obtained via a separate item later (a bucket-of-water instead of a raw water block, say) |
| `item_model` | `"default"` | `"default"` \| `"face"` \| `"custom"` — see "Item models" below |
| `custom_item_model` | *(none)* | path to an external model, required when `item_model` is `"custom"` |
| `rotation` | `"none"` | `"none"` \| `"log"` — see "Rotation" below |
| `light` | `{ "level": 0, "color": [255,255,255] }` | makes this block a light source — see "Lighting" below |
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

### Add an item

Every block with `"item": true` is already an item under its own id. For
something you can hold that isn't a block, drop a file in `items/`:

```json
{ "id": "stick", "name": "Stick", "max_stack": 124, "texture": "stick" }
```

Only `id` is required; `texture` defaults to `id` and works like a block
face's (a procedural painter in `atlas.rs`, a `textures/blocks/<name>.png`,
or the checkerboard placeholder). An item id can't also be a block id.
Blocks can drop it with `"drops": "stick"` or
`"drops": {"item": "stick", "chance": 0.2, "count": 1}`. A `"tool": {"kind":
.., "tier": ..}` entry makes it a tool (see [Tools, tiers and
quality](#tools-tiers-and-quality)).

### Add a control

A plugin can add controls the same way a mod's file does, then ask about them like
the game's own:

```rust
use craftmjne::keybinds::{Actions, Bindings, Chord, ControlDef};

// In your plugin's build(): list it on the Keybinds screen.
let mut bindings = app.world_mut().resource_mut::<Bindings>();
bindings.register(
    ControlDef::new("myplugin:zoom", "Zoom", "My Plugin").with_default(Chord::parse("Z").unwrap()),
);

// In any system: ask whether it is held, without knowing which key that is.
fn zoom(act: Actions) {
    if act.pressed("myplugin:zoom") { /* ... */ }   // or act.just_pressed(..)
}
```

`register` returns `false` and changes nothing if the id is taken (the first to claim
it keeps it), so use an id with your own prefix. Add `.with_command("/some command")`
to make pressing it run a chat command, as a mod's file can. The player's saved key
for your control (if any) is applied when you register it. The game's own controls are
`keybinds::Action` values (`act.pressed(Action::Jump)`); anything taking an `Action`
takes a string id too.

### Add a recipe

One file per recipe in `recipes/` (the file name is the recipe's id):

```json
{
  "station": "hand",
  "inputs": [{ "item": "planks", "count": 3 }, { "item": "stick", "count": 4 }],
  "output": { "item": "crafting_bench", "count": 1 }
}
```

`station` is `"hand"` (the default) or a block id, which makes that block a
crafting station. `count` defaults to 1. An unknown item or station fails at
startup with the file named.

Add `"knap": { "stone": "pebble", "outline": [ ...16 strings of 16 chars,
`#` for the head... ] }` to make it a [knapping](#knapping) recipe: `stone`
must be one of the inputs, the output must be a tool, and the outline must
be one connected piece that fits inside every stone (a test checks the
shipped ones).

### Use real texture files instead of procedural art

Every texture is procedurally painted by default — but any tile can be
overridden with real art by dropping a PNG in `textures/blocks/<name>.png`
(same name-resolution rule as `blocks/`: next to the exe when installed, the
repo root for `cargo run`/tests). No code changes, no recompile — see
`textures/blocks/README.md` for the exact filename each built-in block looks
for. Custom and procedural tiles freely mix; anything you don't supply just
keeps using its painter.

**Resolution is automatic, and global, not per-tile.** A texture file can be
16×16, 32×32, or 64×64 — the *whole* atlas renders at the largest size found
among whatever you've supplied (16×16 if none), like a single resource-pack
resolution choice. Every other tile — every procedural one, and any custom
one smaller than that — gets nearest-neighbor upscaled to match (crisp pixel
replication, not blur), and everything downstream (the mesher's UVs, baked
inventory icons, the GPU texture itself) follows automatically. Drop in one
64×64 texture and the whole game gets sharper, not just that one block.

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
exactly this reason). Rivers are the one deliberate exception: they need a
real flow-accumulation simulation over a large area, so every chunk shares
one `drainage::Network`, which memoizes per-cell flow in lazily-created
tiles behind a mutex. Its results don't depend on which chunk asked first.
See `drainage.rs`'s module doc comment before changing how generation is
parallelized.

## Tests

```bash
cargo test              # the game
cargo test --workspace  # the game and the launcher
```

Unit tests cover the noise, atlas, registry, terrain, mesher (face counts, AO,
water bucket, baked light channels), lighting (falloff, occlusion, colored
channels, removal), physics (falling, landing, wall collision), raycasting,
save/load (slugging, sanitization, round-tripping worlds and settings against
a throwaway temp directory — never a real user profile, plus save-format
migration and the refusal to open a world written by a newer build), and the
command dispatcher (`/mode`'s alias forms, persistence, and the cheats-flag
rule). The launcher's own tests cover version ordering, the installed-version
cache, instance config round-tripping, launch-argument construction, the
self-update manifest and its upgrade decision, and that a failed download
leaves no half-installed version behind — all without touching the network.
`tests/headless.rs` boots the real ECS app **without a window or GPU** and
drives the full streaming pipeline through the actual `AppState` machine:
menu → `InGame` → generation tasks → meshing tasks → chunk entities, plus
block edits, remeshing, cross-chunk dirty propagation, and a full
leave-and-relaunch cycle that confirms edits and player position survive a
save/load round trip. It also drops items through the real schedule and
picks them up, and checks the inventory, health and items on the ground
survive a reload, and that a new world doesn't inherit the last one's
inventory.

## Roadmap ideas

Natural next steps the architecture is prepared for: greedy meshing as an
alternative mesher, multiple save slots per world / world deletion and rename in the Worlds
screen, real mod loading (the Mods screen is a placeholder), entities/mobs as
plugins, day/night cycle (shader uniforms already in place), biome-driven
generation parameters, audio (enable Bevy's `bevy_audio` feature), more chat
commands (`commands::execute`'s match is the extension point), an
achievements system that reads `WorldMeta::cheats` to exclude worlds that
have had commands used in them, crafting, tools that mine faster (a tool's
speed divides `hardness`), and hunger.

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

## Arabic and dialects

Modern Standard Arabic (`lang/ar.json`) is the first right-to-left language. Its `_meta` sets `"rtl": true`
(recorded for a future mirrored layout; the UI is not mirrored yet), and it uses all six Arabic plural forms
(`key.zero/.one/.two/.few/.many/.other`). Names that mix scripts in the language list are wrapped in Unicode
isolates so Arabic and Latin text don't reorder each other. Arabic dialects will be separate files that
fall back to it, like Spanish (Mexico) does to Spanish (Spain).

Dialects are ready for words that differ: a dialect file only holds the keys it names differently and inherits
the rest. If the game ever adds an eggplant, `en-GB.json` can say `"block.eggplant": "Aubergine"` and British
English players see it, while everyone else sees "Eggplant" (a test covers this).
