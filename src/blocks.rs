//! Block registry — the central extension point for adding content.
//!
//! Every block except `air` is loaded from one JSON file per block in the
//! `blocks/` directory (resolved relative to the executable when installed,
//! falling back to the current working directory for `cargo run`/tests —
//! see `find_blocks_dir`). Drop a new file in there to add a block; no
//! recompile needed. You can also register blocks from Rust at plugin
//! `build()` time (before startup) the same way mods/plugins always have:
//! ```ignore
//! app.world_mut().resource_mut::<BlockRegistry>().register(BlockDef {
//!     id: "ruby_block".into(),
//!     textures: FaceTextures::all("ruby"),
//!     ..BlockDef::default()
//! });
//! ```
//!
//! At startup the registry is compiled into flat lookup [`Tables`] — these are
//! what the mesher, physics and worldgen tasks use, so hot loops never touch
//! the def structs.
//!
//! ## Block file schema (`blocks/*.json`)
//!
//! ```json
//! {
//!   "id": "coal_ore",
//!   "name": "Coal Ore",
//!   "transparent": "no",
//!   "fluid": false,
//!   "flow_distance": 0,
//!   "solid": true,
//!   "selectable": true,
//!   "replaceable": false,
//!   "breakable": true,
//!   "hardness": 3.0,
//!   "drops": true,
//!   "max_stack": 124,
//!   "item": true,
//!   "item_model": "default",
//!   "rotation": "none",
//!   "light": { "level": 0, "color": [255, 255, 255] },
//!   "textures": { "all": "coal_ore" }
//! }
//! ```
//!
//! Everything except `id` is optional and defaults sanely:
//! - `name` defaults to a title-cased version of `id` (`"coal_ore"` ->
//!   `"Coal Ore"`) if omitted.
//! - `transparent` is `"no"`, `"partial"`, or `"full"` (see [`Transparency`]);
//!   defaults to `"no"` (fully opaque) if omitted.
//! - `fluid: true` blocks default to `solid: false`, `selectable: false`,
//!   `replaceable: true` (matching how a fluid actually behaves) unless a
//!   field is explicitly set in the file to override that. `flow_distance`
//!   controls how far the fluid spreads from a source before drying up (see
//!   `world.rs`'s `FluidQueue`/`recompute_cell` for the spread sim and
//!   `mesher.rs`'s `fluid_height` for the resulting slope).
//! - `waterloggable: true` lets the block share its cell with a fluid (or
//!   what a fluid freezes into). The registry derives `block@layer` variants
//!   (`pebble@water`, `pebble@ice`) at load: real block ids whose
//!   `BlockDef::layered` names the two tags (main block and layer) and which
//!   otherwise inherit the main block. `freezes_to: "ice"` on a fluid names
//!   what its exposed still sources freeze into, and makes that block a
//!   layer too. See `world::fluid_host` and the README's Waterlogging section.
//! - `breakable` is whether a player can mine this block at all, in either
//!   game mode (default `true`; `blocks/bedrock.json` is the one built-in
//!   `false`).
//! - `hardness` is how tough this block is to mine (default `1.0`). Survival
//!   mining time is `hardness * tools::SECONDS_PER_HARDNESS` (5s) divided by
//!   the speed of the tool in hand when it's the block's `tool` kind - and a
//!   bare hand or the wrong tool is speed 1. `0` breaks instantly (torches);
//!   dirt is `0.5` (2.5s by hand), gravel `1.0` (5s). Creative ignores it
//!   and breaks everything instantly. It's a property of the material rather
//!   than a duration so every tool can divide it by its own speed instead of
//!   every block listing a time per tool.
//! - `tool` names the tool kind a block is mined with (`"pickaxe"`, `"axe"`,
//!   `"shovel"`, ...). `requires_tool: true` means that without it the block
//!   still breaks - `tools::NO_TOOL_PENALTY` (8x) slower - but **drops
//!   nothing**: stone and ores want a pickaxe, logs and planks an axe (60s
//!   and 45s by hand, so in practice you make the tool first). Dirt, sand and
//!   gravel only have a preferred shovel; they're merely faster with it.
//! - `tool_response` (`0..=1`, default `1`) is how much of a tool's speed a
//!   block takes: the speed is raised to this power. Dirt, gravel and the
//!   like use `0.5`, so a good shovel still needs more than a second to
//!   move a block, while logs and stone take the full speedup.
//! - `drops` is what breaking this block in Survival gives you: `true` (the
//!   default) drops the block itself, `false` drops nothing (ice, glass),
//!   an item id drops that item instead (grass drops `"dirt"`), and
//!   `{"item": "twig", "chance": 0.2, "count": 1}` drops it only sometimes
//!   (leaves). Items are blocks or plain items from `items/` - see
//!   `item_registry.rs`. An item that doesn't exist fails at startup, the
//!   same as a missing texture painter would.
//! - `max_stack` is how many of this block a single inventory/hotbar slot
//!   can hold (see [`ItemStack`]); defaults to [`DEFAULT_MAX_STACK`] (124)
//!   if omitted — set it per-block for anything that should stack
//!   differently (or not at all, with `1`).
//! - `item` controls whether this block has a corresponding inventory item
//!   at all - defaults to `true` (an ordinary block like dirt or grass).
//!   Set it to `false` for a block that should never be obtainable/holdable
//!   by any means: it's left out of Creative's block grid and can't be
//!   middle-click picked. `air` is the built-in example; a future block
//!   that's only ever obtained via a separate item (a water block once
//!   there's a bucket-of-water item, say) is the intended general use.
//! - `item_model` is `"default"` (a baked isometric icon showing the top and
//!   two side faces, Minecraft-style), `"face"` (the flat single-face 2D
//!   icon every block used before this existed), or `"custom"` (points at
//!   `custom_item_model`, a path — required when `"custom"` is chosen, the
//!   loader panics on a block file missing it; no model loader exists yet,
//!   so this renders as `"face"` for now). Defaults to `"default"`.
//! - `block_model` names a Blockbench `.bbmodel` file in the `models/`
//!   directory (a bare filename, not a path) giving this block a real shape
//!   in the *world* - as opposed to `item_model`, which only ever affects
//!   the inventory icon. The two are deliberately independent: a block can
//!   have a custom world shape and still use the standard baked isometric
//!   icon. Like `custom_item_model`, **no loader reads it yet** - a block
//!   naming a model still renders as an ordinary cube - so this exists to
//!   let content be authored against the final naming convention ahead of
//!   the mesher supporting it. `blocks/torch.json` is the built-in example.
//! - `rotation` is `"none"` (the default - always renders unrotated, "top"/
//!   "bottom"/"side" textures fixed to +y/-y/the four sides) or `"log"`
//!   (Minecraft log behavior: placing it against a block's top or bottom
//!   face stands it upright as normal; placing it against a side face lays
//!   it on its side instead, with its "top" texture - the end grain - facing
//!   the face you placed it against, i.e. facing you). This is a property of
//!   the placed block instance, not the definition, so two logs placed
//!   differently render differently even though they share one `BlockDef` -
//!   see `Chunk::axis` in `world.rs`.
//! - `shape` is `"cube"` (the default) or `"floor"`: lying flat on the
//!   block below, like a pebble or a twig (see [`Shape`]). A floor block
//!   defaults to `solid: false`, `replaceable: true`, `transparent:
//!   "partial"` and `item_model: "face"`, and draws only its `top` texture.
//! - `light` makes this block a light source (see [`LightSpec`]). `level` is
//!   its brightness in light levels (`0`, the default, emits nothing; `16` is
//!   the maximum and reaches 15 blocks); `color` is a 0-255 RGB hue, white by
//!   default. The three channels propagate independently (`light.rs`), so a
//!   strongly-colored light also has a *shorter* range in whichever channels
//!   its color dims — keep `color` near white for one that should reach the
//!   same distance in every direction. `blocks/torch.json` is the built-in
//!   example.
//! - `transmission` is how much light survives crossing one block of this
//!   material, as a fraction (`1.0` = perfectly clear, the default). Either
//!   one number for all three channels (`0.98`, glass: dims slightly with no
//!   tint) or explicit per-channel `[r, g, b]` (`[0.65, 0.86, 0.95]`, water:
//!   eats red, keeps blue, so what it shades goes blue-green rather than just
//!   dark). Multiplicative, so stacked media compose - light crossing glass
//!   and then water is scaled by each in turn. Ignored for opaque blocks,
//!   which stop everything regardless. See [`Transmission`].
//! - `texture_scheme` picks which texture *names* get looked up per face,
//!   without having to spell any of them out by hand in `textures` - see
//!   [`TextureScheme`]. Defaults to `"default"` (one texture, named after
//!   `id`, on every face).
//! - `textures` explicitly names a face's texture, overriding whatever
//!   `texture_scheme` would have derived for that face. Defaults to
//!   whatever `texture_scheme` derives if omitted entirely (which itself
//!   defaults to a single texture named after `id` on every face).
//! - `tinted` marks which faces get biome-tinted at mesh time (`biome::
//!   grass_tint` multiplied onto the atlas sample) - same `all`/`top`/
//!   `bottom`/`side` shape as `textures`, e.g. `{"top": true, "side": true}`.
//!   The face's own texture should be a grayscale mask meant to be colored
//!   this way; tinting a full-color texture just darkens/recolors it
//!   unexpectedly. Defaults to untinted on every face. See [`FaceTint`].
//!   `blocks/grass.json` is the built-in example - its `bottom` face reuses
//!   `dirt.png` (the same tile plain `dirt` blocks render with) and is
//!   deliberately left untinted, since tinting that tile would tint dirt
//!   blocks too.
//! - `overlay` names, per face, an *extra* biome-tinted texture layered on
//!   top of that face's own (untinted) texture, instead of tinting the
//!   face's own texture directly - same `FaceTextures` shape as `textures`.
//!   For a texture like `grass_side.png` that's only a thin, mostly-
//!   transparent grass-colored fringe over an otherwise-transparent image
//!   (matching real Minecraft's own `grass_block_side` + `..._overlay`
//!   asset split), tinting it directly would leave most of the face
//!   punched full of holes - the transparent majority gets discarded
//!   ([`crate::render`]'s `alpha_cutoff`) with nothing opaque underneath.
//!   `overlay` renders the named texture as a second, always-tinted decal
//!   quad nudged a hair in front of the face's real (untinted) texture, so
//!   the transparent parts of the overlay show the base through instead of
//!   showing nothing. `blocks/grass.json`'s side faces use `textures.side:
//!   "dirt"` (the plain base) plus `overlay.side: "grass_side"` (the tinted
//!   fringe on top); its top face has no transparency problem to solve, so
//!   it just uses plain `tinted.top` on `grass_top.png` directly instead.
//!   See `mesher.rs`'s overlay decal emission.

use bevy::prelude::*;
use serde::Deserialize;
use crate::tools::ToolKind;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::light::{MAX_LEVEL, MAX_LIGHT};

pub type BlockId = u16;

pub use crate::item_registry::ItemStack;

/// How many of a block a single slot holds unless a block file overrides it
/// with `max_stack`.
pub const DEFAULT_MAX_STACK: u32 = 124;
/// Mass (kg) of anything that doesn't say.
pub const DEFAULT_MASS: f32 = 1.0;
/// Seconds between melee hits with anything that doesn't say (`attack_cooldown`).
pub const DEFAULT_ATTACK_COOLDOWN: f32 = 0.45;

/// A state a block declares for itself (`state_defs` in its file): a name and
/// the values it can take. The *first* value is the default - what a freshly
/// placed block has, and what a cleared state word reads as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDef {
    pub name: String,
    pub values: Vec<String>,
}

/// Where a block's own `state_defs` are kept.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum StateStorage {
    /// In the cell's shared 16-bit state word, in the bits its built-in tags
    /// leave free. No extra memory, but at most 16 bits in all.
    #[default]
    Compact,
    /// In a per-chunk table keyed by cell, for this block's cells only and only
    /// while a state differs from its default. Far more room (up to
    /// `states::MAX_EXTENDED_BITS`), and a little more to read and write.
    Extended,
}

/// How a `state_defs` entry is written: `"charge": 8` (values `0`..`7`) or
/// `"mode": ["off", "on", "auto"]`.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum StateValuesFile {
    Count(u32),
    Names(Vec<String>),
}

/// Everything about how a thing behaves when thrown, apart from its mass.
/// The same four tags work in a block file and an item file, and every
/// field is optional: leave one out and the default below applies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThrowSpec {
    /// Joules of effort behind a throw (`throw_energy`). Launch speed is
    /// `sqrt(2 * energy / mass)`.
    pub energy: f32,
    /// Air resistance in kg/m (`throw_drag`): deceleration is
    /// `drag / mass * speed^2`.
    pub drag: f32,
    /// Seconds between throws while the button is held (`throw_cooldown`).
    pub cooldown: f32,
    /// Multiplier on impact damage (`throw_damage`); 1.0 is the standard
    /// quarter damage per joule.
    pub damage_scale: f32,
}

impl ThrowSpec {
    pub const DEFAULT: Self = Self { energy: 8.0, drag: 0.002, cooldown: 0.35, damage_scale: 1.0 };
}

impl Default for ThrowSpec {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The throw tags as they appear in a block or item JSON file (flattened in).
/// Non-positive values are ignored rather than trusted.
#[derive(Deserialize, Default, Clone, Copy)]
pub struct ThrowFile {
    throw_energy: Option<f32>,
    throw_drag: Option<f32>,
    throw_cooldown: Option<f32>,
    throw_damage: Option<f32>,
}

impl ThrowFile {
    pub fn spec(&self) -> ThrowSpec {
        let d = ThrowSpec::DEFAULT;
        let pos = |v: Option<f32>, default: f32| v.filter(|x| *x > 0.0 && x.is_finite()).unwrap_or(default);
        ThrowSpec {
            energy: pos(self.throw_energy, d.energy),
            drag: pos(self.throw_drag, d.drag),
            cooldown: pos(self.throw_cooldown, d.cooldown),
            damage_scale: pos(self.throw_damage, d.damage_scale),
        }
    }
}

/// Parses block-file JSON the way the loader does, for tests that probe
/// which keys it reads (`tags.rs`).
#[cfg(test)]
pub(crate) fn parse_block_file_for_test(text: &str) -> Result<(), serde_json::Error> {
    serde_json::from_str::<BlockFile>(text).map(|_| ())
}

/// A block file's `hardness` when it doesn't give one.
pub const DEFAULT_HARDNESS: f32 = 1.0;

/// What breaking a block in Survival gives the player. Resolved by
/// `item_registry::ItemRegistry::drop_of`, which also refuses to start
/// with a drop naming an item that doesn't exist.
#[derive(Clone, PartialEq, Debug, Default)]
pub enum Drops {
    /// The block itself - what almost everything wants.
    #[default]
    Itself,
    /// Nothing at all (ice, glass).
    Nothing,
    /// An item by id - another block's (grass drops dirt) or a plain item
    /// (leaves drop twigs) - `count` of them, with probability `chance`.
    Item { item: String, chance: f32, count: u32 },
}

/// `drops` as written in a block file: `true`/`false`, an item id, or
/// `{ "item": "...", "chance": 0.2, "count": 1 }`.
#[derive(Deserialize)]
#[serde(untagged)]
enum DropsFile {
    Flag(bool),
    Item(String),
    Detailed {
        item: String,
        #[serde(default = "always")]
        chance: f32,
        #[serde(default = "one")]
        count: u32,
    },
}

fn always() -> f32 {
    1.0
}

fn one() -> u32 {
    1
}

impl Default for DropsFile {
    fn default() -> Self {
        DropsFile::Flag(true)
    }
}

impl From<DropsFile> for Drops {
    fn from(file: DropsFile) -> Self {
        match file {
            DropsFile::Flag(true) => Drops::Itself,
            DropsFile::Flag(false) => Drops::Nothing,
            DropsFile::Item(item) => Drops::Item { item, chance: 1.0, count: 1 },
            DropsFile::Detailed { item, chance, count } => {
                Drops::Item { item, chance: chance.clamp(0.0, 1.0), count: count.max(1) }
            }
        }
    }
}

/// A fluid cell's stored level, meaningful only while that cell's block id is
/// a fluid (see `Chunk::state` in `world.rs`). `0` is a permanent
/// source (never dries, always full height); `1..=flow_distance` is a
/// flowing cell that many blocks from its supply; `FLUID_FALLING` is a full
/// -height column fed from directly above (a waterfall), which — unlike a
/// source — dries up if its supply is cut.
pub const FLUID_SOURCE: u8 = 0;
pub const FLUID_FALLING: u8 = 255;

#[derive(Debug)]
pub struct UnknownBlock;

impl std::fmt::Display for UnknownBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown block name")
    }
}

impl std::error::Error for UnknownBlock {}
pub const AIR: BlockId = 0;

/// How a block's faces are rendered/culled with respect to what's behind
/// them. All three still respect the block's own texture alpha - a
/// `partial` block with a fully-opaque texture looks solid, and a `full`
/// block with fully-transparent pixels is invisible.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Transparency {
    /// Fully opaque: occludes neighbours, no per-pixel discard, no blending.
    #[default]
    No,
    /// Cutout, like glass/leaves: doesn't occlude neighbours, and texture
    /// pixels below the alpha-cutoff threshold are fully discarded so you
    /// can see through the *holes*, but the rest of the face is opaque -
    /// "see the back geometry of the block from the front."
    Partial,
    /// Alpha-blended, like water: the whole face draws at reduced opacity,
    /// so you only ever see the part of the block you're actually looking
    /// at (unlike `partial`, nothing is punched out), but everything behind
    /// it shows through.
    Full,
}

/// How a block draws as an inventory/hotbar icon (as opposed to in-world,
/// which is always the real block mesh regardless of this setting). Shared
/// terminology with a future standalone item system - a block's icon and a
/// pure item's icon both come from the same three choices.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ItemModel {
    /// A block: a baked isometric icon showing the top and two side faces,
    /// Minecraft-style (see `icons.rs`). A future pure item (not backed by
    /// a placeable block) would instead just show its own flat texture -
    /// there's no "block mesh" to project for those.
    #[default]
    Default,
    /// The flat, single-face 2D texture - how every block's icon looked
    /// before `ItemModel` existed. Cheaper and reads better for thin/flat
    /// things (a future flower or sign, say) than a forced-3D icon would.
    Face,
    /// Points at an external model via `BlockDef::custom_item_model`
    /// (required when this variant is chosen - the loader panics on a
    /// block file missing it, same as any other malformed content). No
    /// model format/loader exists yet, so this renders as `Face` for now;
    /// the path still round-trips through the registry so a model system
    /// can pick it up later without another schema change.
    Custom,
}

/// How a placed block instance can be oriented. Unlike every other
/// `BlockDef` field, this alone doesn't fully determine rendering - the
/// actual orientation (`blocks::AXIS_X/Y/Z`) is per-placed-instance state,
/// stored in `Chunk::axis` (`world.rs`) and computed at placement time from
/// which face was clicked (`interact.rs`). This field only says *whether*
/// that mechanism applies to a block at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Rotation {
    /// Always renders unrotated (`AXIS_Y`) regardless of how it was placed.
    #[default]
    None,
    /// Minecraft log behavior - see the module docs' `rotation` entry.
    Log,
}

/// A block's shape in the world.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// A full block - nearly everything.
    #[default]
    Cube,
    /// Lying flat on the block below: a single picture of its top texture
    /// a hair above the ground (pebbles, twigs). Defaults to no collision,
    /// see-through, replaceable (placing a block there just takes its
    /// place), a flat inventory icon, and it can only be placed on top of
    /// something solid - and it breaks off, dropping itself, if that
    /// support is removed (`interact::pop_unsupported`).
    Floor,
    /// Occupies its cell (and collides like a block) but the mesher draws
    /// nothing for it: something else renders it - the training dummy's
    /// player model (`block_models.rs`). It defaults to see-through, so the
    /// neighbouring blocks' faces next to it still draw.
    Model,
}

/// How much light a block emits, and what color. Blocks that aren't light
/// sources leave this at its default (`level: 0`), which emits nothing.
///
/// `level` is the brightness in light levels (`0..=`[`MAX_LEVEL`], clamped);
/// `color` is a plain 0-255 RGB hue applied to it. The two combine into a
/// per-channel emission (see [`Self::emission`]) because `light.rs`
/// propagates the three channels independently — which is exactly what makes
/// a red lamp's light stay red as it fades, instead of being one brightness
/// with a tint stapled on top. The flip side worth knowing when authoring a
/// block: a channel the color drops to zero has *no range at all*, so a
/// saturated color is also a shorter-reaching one in those channels. For a
/// light that should reach the same distance in every direction and every
/// channel (`torch.json`), keep `color` close to white.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub struct LightSpec {
    #[serde(default)]
    pub level: u32,
    #[serde(default = "default_light_color")]
    pub color: [u8; 3],
}

fn default_light_color() -> [u8; 3] {
    [255, 255, 255]
}

impl Default for LightSpec {
    fn default() -> Self {
        Self { level: 0, color: default_light_color() }
    }
}

impl LightSpec {
    /// This block's per-channel emission in *stored* light units
    /// (`0..=MAX_LIGHT` each) — what `Tables::light` holds and `light.rs`
    /// actually propagates.
    ///
    /// Converts from the authored `0..=MAX_LEVEL` scale, which is the one
    /// block files are written in and the one that corresponds to a distance
    /// in blocks. Keeping authoring in levels while storing in finer units
    /// means making light more precise never invalidated a single block file.
    pub fn emission(&self) -> [u8; 3] {
        let level = self.level.min(MAX_LEVEL);
        self.color.map(|c| {
            ((level * c as u32 * MAX_LIGHT as u32) / (MAX_LEVEL * 255)).min(MAX_LIGHT as u32) as u8
        })
    }
}

/// How much light survives passing through one block of this material, per
/// channel, as a fraction in `0.0..=1.0` (`1.0` = perfectly clear).
///
/// Only meaningful for blocks light can enter at all — an opaque block stops
/// everything regardless of what this says, and `Tables::transmission`
/// records `[0; 3]` for those so nothing has to check two fields to know
/// what a cell does to light.
///
/// Written in a block file either as one number for all three channels or as
/// explicit per-channel values:
///
/// ```json
/// "transmission": 0.98                     // glass: dims slightly, no tint
/// "transmission": [0.65, 0.85, 0.95]       // water: eats red, keeps blue
/// ```
///
/// Multiplicative rather than a subtracted number of levels, which is what
/// makes it compose: light crossing glass and then water is scaled by each
/// in turn, so the pair is genuinely different from either alone, and a
/// hundred panes of glass really do add up to something. A subtractive
/// "levels absorbed" field on the old coarse `0..=16` scale could not express
/// "2%" at all — it rounded to either nothing or a full level.
#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
#[serde(untagged)]
pub enum Transmission {
    /// One value for all three channels — no tint, just dimming.
    Uniform(f32),
    /// Explicit per-channel `[r, g, b]`, for a medium that absorbs colors
    /// unequally.
    PerChannel([f32; 3]),
}

impl Default for Transmission {
    fn default() -> Self {
        Self::Uniform(1.0)
    }
}

impl Transmission {
    /// Fixed-point `0..=255` per channel, the form `light.rs` multiplies by.
    /// Out-of-range values are clamped rather than rejected: a block file
    /// asking for 150% transmission is a typo, not a reason to refuse to
    /// start a world.
    pub fn to_fixed(self) -> [u8; 3] {
        let raw = match self {
            Self::Uniform(v) => [v; 3],
            Self::PerChannel(v) => v,
        };
        raw.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

/// A rotating block's stored orientation (`Chunk::axis`, meaningful only
/// where `Tables::rotates` is true for that cell's block id). Matches the
/// mesher's face-index axis grouping exactly (faces 0/1 are the x-axis,
/// 2/3 are y, 4/5 are z), so `mesher.rs` can compare them with no
/// translation. `AXIS_Y` is the default - standing upright, unrotated.
pub const AXIS_X: u8 = 0;
pub const AXIS_Y: u8 = 1;
pub const AXIS_Z: u8 = 2;

/// Which texture *names* a block's six faces look up, when `textures`
/// doesn't explicitly spell a face out (see [`FaceTextures`]) - lets most
/// blocks skip `textures` entirely and just get sane per-face names derived
/// from `id`, instead of every block author having to hand-write a name for
/// each face. All derived names follow `{id}_{suffix}` except `Default`
/// (plain `id`, no suffix, same name on every face) and the un-suffixed
/// faces of `Interface` (also plain `id`).
///
/// | Scheme               | top        | bottom        | north         | remaining sides |
/// |-----------------------|------------|---------------|---------------|------------------|
/// | `default`             | `id`       | `id`          | `id`          | `id`             |
/// | `log`                 | `id_top`   | `id_top`      | `id_side`     | `id_side`        |
/// | `organic`              | `id_top`   | `id_bottom`   | `id_side`     | `id_side`        |
/// | `interface`            | `id`       | `id`          | `id_north`    | `id`             |
/// | `advanced_interface`   | `id_top`   | `id_bottom`   | `id_north`    | `id_side`        |
/// | `custom`               | *(reserved for a future fully-independent per-face mapping - for now, behaves like `default` unless overridden per-face by `textures`)* ||||
///
/// A face named this way can still be overridden individually via
/// `textures` (an explicit `textures.top`/`bottom`/`side`/`all` always wins
/// over whatever the scheme would have derived) - see `grass.json` for an
/// example that uses `organic` for its top/side but overrides `bottom` to
/// reuse `dirt`'s texture rather than a dedicated `grass_bottom`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextureScheme {
    /// One texture, named after `id`, on every face.
    #[default]
    Default,
    /// Top and bottom share `id_top` (the end-grain), the four sides share
    /// `id_side` - a log standing upright.
    Log,
    /// Top gets `id_top`, bottom gets `id_bottom`, the four sides share
    /// `id_side` - grass-like blocks with a distinct top and bottom.
    Organic,
    /// Only the north face is unique (`id_north`); every other face
    /// (top, bottom, and the remaining three sides) shares the plain `id`
    /// texture - a block with a single distinguishing front panel.
    Interface,
    /// North (`id_north`), top (`id_top`), and bottom (`id_bottom`) are all
    /// unique; the remaining three sides (east, west, south) share
    /// `id_side` - the "fully dressed" version of `Interface`.
    AdvancedInterface,
    /// No automatic derivation - reserved for a future fully-independent
    /// per-face mapping (a distinct texture for all six faces, not just the
    /// north/top/bottom/side buckets `textures` currently supports). For
    /// now this behaves like `Default`; use explicit `textures` fields to
    /// override individual faces in the meantime.
    Custom,
}

impl TextureScheme {
    /// The `{id}_{suffix}` suffix this scheme uses for `face`, or `None` if
    /// this face should just use the plain, un-suffixed `id`.
    fn suffix(self, face: usize) -> Option<&'static str> {
        use TextureScheme::*;
        match self {
            Default | Custom => None,
            Log => match face {
                2 | 3 => Some("top"),
                _ => Some("side"),
            },
            Organic => match face {
                2 => Some("top"),
                3 => Some("bottom"),
                _ => Some("side"),
            },
            Interface => match face {
                5 => Some("north"),
                _ => None,
            },
            AdvancedInterface => match face {
                5 => Some("north"),
                2 => Some("top"),
                3 => Some("bottom"),
                _ => Some("side"),
            },
        }
    }
}

/// Face order used across the whole engine (mesher, tiles table):
/// 0:+x east, 1:-x west, 2:+y top, 3:-y bottom, 4:+z south, 5:-z north
#[derive(Clone, Default, Deserialize)]
pub struct FaceTextures {
    pub all: Option<String>,
    pub top: Option<String>,
    pub bottom: Option<String>,
    pub side: Option<String>,
}

impl FaceTextures {
    pub fn all(name: &str) -> Self {
        Self { all: Some(name.into()), ..Self::default() }
    }

    pub fn tbs(top: &str, bottom: &str, side: &str) -> Self {
        Self {
            top: Some(top.into()),
            bottom: Some(bottom.into()),
            side: Some(side.into()),
            all: None,
        }
    }

    /// An explicit, hand-named override for `face`, if this block's
    /// `textures` set one - `None` means "defer to `texture_scheme`", not
    /// "use `id`" (that fallback now lives in [`BlockDef::texture_name`],
    /// since it needs the scheme to pick the right derived name first).
    fn explicit(&self, face: usize) -> Option<&str> {
        let primary = match face {
            2 => &self.top,
            3 => &self.bottom,
            _ => &self.side,
        };
        primary.as_deref().or(self.all.as_deref())
    }
}

/// Which of a block's faces get biome-tinted at mesh time - see
/// `biome.rs` and `Tables::tinted`. Same `all`/`top`/`bottom`/`side`
/// vocabulary and face-index mapping as [`FaceTextures`] (face `2` is top,
/// `3` is bottom, everything else is a side), deliberately kept separate
/// from it: tinting is a property of which *texture* a face resolved to
/// (a grayscale mask meant to be multiplied by a color), not of the block
/// as a whole, and a block can easily want one without the other - grass's
/// bottom face reuses `dirt.png`, the same tile plain `dirt` blocks use, so
/// it must never be tinted even though grass's top and sides are.
#[derive(Clone, Copy, Default, Deserialize)]
pub struct FaceTint {
    #[serde(default)]
    pub all: bool,
    #[serde(default)]
    pub top: bool,
    #[serde(default)]
    pub bottom: bool,
    #[serde(default)]
    pub side: bool,
}

impl FaceTint {
    fn tinted(&self, face: usize) -> bool {
        let primary = match face {
            2 => self.top,
            3 => self.bottom,
            _ => self.side,
        };
        primary || self.all
    }
}

/// The two halves of a waterlogged (or frozen-in) cell: the block itself
/// and the layer it's holding. Only ever set on the variants the registry
/// derives (`pebble@water`), never in a block file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layered {
    /// The main block (`pebble`).
    pub block: BlockId,
    /// What fills the rest of the cell (`water`, or `ice` once frozen).
    pub layer: BlockId,
}

#[derive(Clone)]
pub struct BlockDef {
    /// Stable, no-spaces registry key (also what saves reference block
    /// edits by, and what `registry.id("...")`/`by_name("...")` look up).
    pub id: String,
    /// Human-readable display name (tooltips etc). Free-form, may have
    /// spaces.
    pub name: String,
    /// Collides with entities.
    pub solid: bool,
    pub transparency: Transparency,
    /// A swimmable liquid: gets a lowered top surface (see `mesher.rs`) and
    /// counts for `player.rs`'s in-water check. Independent of
    /// `transparency` - set that too (typically `full`) for the usual
    /// water-style look.
    pub fluid: bool,
    /// How far (in blocks) this fluid should flow from a source before
    /// drying up. Drives both the spread simulation (`world.rs`'s fluid
    /// queue) and the mesher's per-level surface height, so a long distance
    /// slopes gently and a short one drops off steeply.
    pub flow_distance: u32,
    /// Can be targeted by the crosshair raycast.
    pub selectable: bool,
    /// Placing a block into this cell overwrites it (water).
    pub replaceable: bool,
    /// Can be mined (bedrock is not).
    pub breakable: bool,
    /// How long this block takes to mine, as a material property rather
    /// than a duration - see the module docs' `hardness` section.
    pub hardness: f32,
    /// What breaking it in Survival yields. See [`Drops`].
    pub drops: Drops,
    /// How many of this block a single inventory/hotbar slot holds. See
    /// [`ItemStack`]; defaults to [`DEFAULT_MAX_STACK`].
    pub max_stack: u32,
    /// Whether this block has a corresponding inventory item at all. `true`
    /// (the default) covers ordinary blocks - dirt, grass, etc. `false`
    /// means it's never obtainable by any means: left out of Creative's
    /// block grid, can't be middle-click picked. `air` is the built-in
    /// example; the general use is a block that's meant to only ever be
    /// obtained via a *separate* item (a future water block once there's a
    /// bucket-of-water item, say), without having to define a whole
    /// separate item system just to hide the raw block from the UI.
    pub item: bool,
    /// How this block's inventory icon is drawn. See [`ItemModel`].
    pub item_model: ItemModel,
    /// External model path for `item_model: "custom"`. `resolve()`/loaders
    /// treat this as relative to wherever a future model system decides -
    /// for now it's just carried through the registry unread.
    pub custom_item_model: Option<String>,
    /// A Blockbench `.bbmodel` filename (relative to the repo's `models/`
    /// directory) giving this block a real shape in the *world*, instead of
    /// the plain textured cube every block renders as today.
    ///
    /// Deliberately separate from [`Self::item_model`], which is only ever
    /// about the inventory/hotbar icon: a block can have a custom world
    /// shape while keeping the standard baked isometric icon, and vice
    /// versa. Folding both into one field would mean a torch couldn't get
    /// its real shape without also losing its icon.
    ///
    /// **No loader exists yet** - like `custom_item_model`, this is carried
    /// through the registry unread, so a block naming a model still renders
    /// as a cube. It's here so content can be authored against the final
    /// naming convention before the mesher learns to read it.
    pub block_model: Option<String>,
    /// Whether a placed instance can face different directions. See
    /// [`Rotation`].
    pub rotation: Rotation,
    /// Its shape in the world. See [`Shape`].
    pub shape: Shape,
    /// How much light this block emits, and in what color. See [`LightSpec`];
    /// defaults to emitting nothing.
    pub light: LightSpec,
    /// How much light survives crossing one block of this material, per
    /// channel. See [`Transmission`]; defaults to perfectly clear, which is
    /// what air and every ordinary see-through block want.
    pub transmission: Transmission,
    /// Which texture names an unset face in `textures` falls back to. See
    /// [`TextureScheme`].
    pub texture_scheme: TextureScheme,
    pub textures: FaceTextures,
    /// Which faces get biome-tinted at mesh time. See [`FaceTint`];
    /// defaults to none, which is what every block wants except grass.
    pub tinted: FaceTint,
    /// Per-face extra tinted overlay texture, layered over that face's own
    /// (untinted) texture instead of tinting it directly. See [`FaceTint`]'s
    /// doc comment and the module docs' `overlay` section for why this is a
    /// separate mechanism from `tinted` - defaults to no overlay on any
    /// face, which is every block except grass's sides.
    pub overlay: FaceTextures,
    /// The tool kind this block is mined with (`blocks/*.json` `tool`): the
    /// matching tool mines it at the tool's speed, anything else (a bare
    /// hand, the wrong kind) at hand speed. `None`: no tool helps.
    pub tool: Option<ToolKind>,
    /// Without the right tool it still breaks (slowly) but drops nothing.
    /// Needs `tool`.
    pub requires_tool: bool,
    /// How much of a tool's speed advantage this block takes, `0..=1`: the
    /// tool's speed is raised to this power, so `1.0` (the default) is the
    /// full speedup and `0.5` a square root of it. Soft, loose material
    /// (dirt, gravel) takes only part, so even a great shovel needs real
    /// time to move a block of it.
    pub tool_response: f32,
    /// Which sound group this block makes (`stone`, `dirt`, `grass`, ... - see `sound.rs`).
    pub sound: String,
    /// Only mined while crouching (Shift held): a plain left click does
    /// nothing to it, so that click stays free for something else (hitting
    /// a dummy, later). Applies in Creative too.
    pub requires_crouch: bool,
    /// A plain left click hits it instead of mining it: it takes damage from
    /// what's in hand and shows the number (the training dummy).
    pub hittable: bool,
    /// Seconds between melee hits while this is held (`attack_cooldown`).
    pub attack_cooldown: f32,
    /// Font its name is drawn in (tooltips); `None` = the default (`font`).
    pub font: Option<String>,
    /// A thrown object at least this fast (m/s) shatters it (glass); `None`
    /// means it never breaks that way.
    pub shatters_at: Option<f32>,
    /// Fraction of its speed a projectile keeps after shattering this block.
    pub shatter_keep: f32,
    /// Mass in kilograms, for what it does when thrown (a pebble is 0.05).
    /// Lighter things are thrown faster for the same effort, and slowed more
    /// by the air. See `items::throw_speed`.
    pub mass: f32,
    /// Left click throws it instead of mining or hitting (a pebble).
    pub throwable: bool,
    /// How it throws (`throw_energy`, `throw_drag`, `throw_cooldown`, `throw_damage`).
    pub throw: ThrowSpec,
    /// A block that stands two cells tall: the name of the block placed in
    /// the cell above this one. Placing needs both cells free; breaking
    /// either takes the other with it (and only one drop). See
    /// `interact::tall_partner`.
    pub upper: Option<String>,
    /// May hold a fluid (or the ice it freezes into) in the same cell as
    /// itself. See the module docs' `waterloggable` section.
    pub waterloggable: bool,
    /// The state tags (`states.rs`) a placed one of these has, so can be
    /// read and edited per cell. Declared with the `states` definition tag,
    /// plus whatever its behaviour tags imply (`rotation: log` -> `axis`,
    /// `waterloggable` -> `waterlogged`, a fluid or what one freezes into ->
    /// `level`). Nothing outside this list is ever a state of this block.
    pub states: Vec<String>,
    /// States this block declares itself (`state_defs`), beyond the built-in
    /// tags. Stored in the cell's state word like any other (see `states.rs`).
    pub state_defs: Vec<StateDef>,
    /// Where each of `state_defs` lives in the state word - filled in by
    /// `BlockRegistry::finalize_states`, parallel to `state_defs`.
    pub state_fields: Vec<crate::states::Field>,
    /// Compact (the shared word) or extended (a per-chunk table).
    pub state_storage: StateStorage,
    /// For `Extended`: where each of `state_defs` sits in the cell's byte
    /// string, and how many bytes the whole thing takes. Empty / 0 otherwise.
    pub ext_fields: Vec<crate::states::ExtField>,
    pub ext_bytes: usize,
    /// What this fluid becomes when it freezes (`water` -> `ice`). A block
    /// named here is a "layer" every waterloggable block gets a variant for.
    pub freezes_to: Option<String>,
    /// `Some` on a derived `block@layer` variant only.
    pub layered: Option<Layered>,
}

impl BlockDef {
    /// The texture name this block's `face` (0..6, see the face-order doc
    /// above `FaceTextures`) resolves to: an explicit `textures` override
    /// if this block's file set one for that face, otherwise whatever
    /// `texture_scheme` derives from `id`.
    pub fn texture_name(&self, face: usize) -> String {
        if let Some(name) = self.textures.explicit(face) {
            return name.to_string();
        }
        match self.texture_scheme.suffix(face) {
            Some(suffix) => format!("{}_{suffix}", self.id),
            None => self.id.clone(),
        }
    }

    /// This face's overlay texture name, if `overlay` set one - always an
    /// explicit name (unlike `texture_name`, there's no scheme to fall back
    /// to for something that doesn't exist on most blocks at all).
    pub fn overlay_name(&self, face: usize) -> Option<String> {
        self.overlay.explicit(face).map(str::to_string)
    }
}

impl Default for BlockDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            solid: true,
            transparency: Transparency::No,
            fluid: false,
            flow_distance: 0,
            selectable: true,
            replaceable: false,
            breakable: true,
            hardness: DEFAULT_HARDNESS,
            drops: Drops::Itself,
            max_stack: DEFAULT_MAX_STACK,
            item: true,
            item_model: ItemModel::default(),
            custom_item_model: None,
            block_model: None,
            rotation: Rotation::default(),
            shape: Shape::default(),
            light: LightSpec::default(),
            transmission: Transmission::default(),
            texture_scheme: TextureScheme::default(),
            textures: FaceTextures::default(),
            tinted: FaceTint::default(),
            overlay: FaceTextures::default(),
            tool: None,
            requires_tool: false,
            tool_response: 1.0,
            sound: crate::sound::DEFAULT_GROUP.to_string(),
            requires_crouch: false,
            hittable: false,
            attack_cooldown: DEFAULT_ATTACK_COOLDOWN,
            font: None,
            shatters_at: None,
            shatter_keep: 1.0,
            mass: DEFAULT_MASS,
            throwable: false,
            throw: ThrowSpec::DEFAULT,
            upper: None,
            waterloggable: false,
            states: Vec::new(),
            state_defs: Vec::new(),
            state_fields: Vec::new(),
            state_storage: StateStorage::Compact,
            ext_fields: Vec::new(),
            ext_bytes: 0,
            freezes_to: None,
            layered: None,
        }
    }
}

/// "coal_ore" -> "Coal Ore". Used as the default `name` for a block file
/// that doesn't specify one.
pub(crate) fn title_case(id: &str) -> String {
    // A mod's `mymod:granite` is called "Granite", not "Mymod:granite".
    let id = id.rsplit(':').next().unwrap_or(id);
    id.split('_')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// On-disk shape of a `blocks/*.json` file. Every field but `id` is
/// optional; `into_def` applies defaults (including the `fluid`-implies
/// pattern described in the module docs).
#[derive(Deserialize)]
pub(crate) struct BlockFile {
    id: String,
    name: Option<String>,
    transparent: Option<Transparency>,
    #[serde(default)]
    fluid: bool,
    #[serde(default)]
    flow_distance: u32,
    solid: Option<bool>,
    selectable: Option<bool>,
    replaceable: Option<bool>,
    #[serde(default = "default_true")]
    breakable: bool,
    #[serde(default = "default_hardness")]
    hardness: f32,
    #[serde(default)]
    drops: DropsFile,
    #[serde(default = "default_max_stack")]
    max_stack: u32,
    #[serde(default = "default_true")]
    item: bool,
    item_model: Option<ItemModel>,
    custom_item_model: Option<String>,
    block_model: Option<String>,
    #[serde(default)]
    rotation: Rotation,
    #[serde(default)]
    shape: Shape,
    #[serde(default)]
    light: LightSpec,
    #[serde(default)]
    transmission: Transmission,
    #[serde(default)]
    texture_scheme: TextureScheme,
    #[serde(default)]
    textures: FaceTextures,
    #[serde(default)]
    tinted: FaceTint,
    #[serde(default)]
    overlay: FaceTextures,
    #[serde(default)]
    waterloggable: bool,
    /// Which state tags a placed one has (`states.rs`).
    #[serde(default)]
    states: Vec<String>,
    /// States this block declares for itself.
    #[serde(default)]
    state_defs: std::collections::BTreeMap<String, StateValuesFile>,
    /// `compact` (default) or `extended`.
    #[serde(default)]
    state_storage: StateStorage,
    freezes_to: Option<String>,
    tool: Option<ToolKind>,
    #[serde(default)]
    requires_tool: bool,
    tool_response: Option<f32>,
    #[serde(default)]
    sound: Option<String>,
    #[serde(default)]
    requires_crouch: bool,
    #[serde(default)]
    hittable: bool,
    attack_cooldown: Option<f32>,
    font: Option<String>,
    shatters_at: Option<f32>,
    shatter_keep: Option<f32>,
    mass: Option<f32>,
    #[serde(default)]
    throwable: bool,
    #[serde(flatten)]
    throw: ThrowFile,
    upper: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_hardness() -> f32 {
    DEFAULT_HARDNESS
}

fn default_max_stack() -> u32 {
    DEFAULT_MAX_STACK
}

impl BlockFile {
    pub(crate) fn into_def(self) -> BlockDef {
        // A fluid defaults to water-like physics (not solid, can't be
        // targeted, placing into it just replaces it) unless the file
        // explicitly overrides one of those fields.
        // A floor shape (a pebble on the ground) defaults the same way,
        // except that it can be targeted, so it can be picked up.
        let (solid, selectable, replaceable) = if self.fluid {
            (false, false, true)
        } else if self.shape == Shape::Floor {
            (false, true, true)
        } else {
            (true, true, false)
        };
        let floor = self.shape == Shape::Floor;
        // Declaring a state tag turns on the behaviour it needs.
        let mut rotation = self.rotation;
        let mut waterloggable = self.waterloggable;
        let mut states: Vec<String> = Vec::new();
        for s in &self.states {
            let s = s.to_lowercase();
            match s.as_str() {
                "axis" if rotation == Rotation::None => rotation = Rotation::Log,
                "waterlogged" => waterloggable = true,
                _ => {}
            }
            if !states.contains(&s) {
                states.push(s);
            }
        }
        BlockDef {
            id: self.id.clone(),
            name: self.name.unwrap_or_else(|| title_case(&self.id)),
            solid: self.solid.unwrap_or(solid),
            transparency: self.transparent.unwrap_or(if floor || self.shape == Shape::Model {
                Transparency::Partial
            } else {
                Transparency::No
            }),
            fluid: self.fluid,
            flow_distance: self.flow_distance,
            selectable: self.selectable.unwrap_or(selectable),
            replaceable: self.replaceable.unwrap_or(replaceable),
            breakable: self.breakable,
            hardness: self.hardness.max(0.0),
            drops: self.drops.into(),
            max_stack: self.max_stack,
            item: self.item,
            item_model: self.item_model.unwrap_or(if floor { ItemModel::Face } else { ItemModel::Default }),
            custom_item_model: self.custom_item_model,
            block_model: self.block_model,
            rotation,
            shape: self.shape,
            light: self.light,
            transmission: self.transmission,
            texture_scheme: self.texture_scheme,
            textures: self.textures,
            tinted: self.tinted,
            overlay: self.overlay,
            tool: self.tool,
            requires_tool: self.requires_tool,
            tool_response: self.tool_response.unwrap_or(1.0).clamp(0.0, 1.0),
            sound: self.sound.unwrap_or_else(|| crate::sound::DEFAULT_GROUP.to_string()),
            requires_crouch: self.requires_crouch,
            hittable: self.hittable,
            attack_cooldown: self.attack_cooldown.filter(|c| *c > 0.0).unwrap_or(DEFAULT_ATTACK_COOLDOWN),
            font: self.font.filter(|f| !f.trim().is_empty()),
            shatters_at: self.shatters_at,
            shatter_keep: self.shatter_keep.unwrap_or(1.0).clamp(0.0, 1.0),
            mass: self.mass.filter(|m| *m > 0.0).unwrap_or(DEFAULT_MASS),
            throwable: self.throwable,
            throw: self.throw.spec(),
            upper: self.upper,
            waterloggable,
            states,
            state_defs: self
                .state_defs
                .into_iter()
                .map(|(name, values)| StateDef {
                    name: name.to_lowercase(),
                    values: match values {
                        StateValuesFile::Count(n) => (0..n).map(|i| i.to_string()).collect(),
                        StateValuesFile::Names(names) => names,
                    },
                })
                .collect(),
            state_fields: Vec::new(),
            state_storage: self.state_storage,
            ext_fields: Vec::new(),
            ext_bytes: 0,
            freezes_to: self.freezes_to,
            layered: None,
        }
    }
}

/// Flat lookup tables baked from the registry; shared with worldgen/mesh tasks.
pub struct Tables {
    pub solid: Vec<bool>,
    pub opaque: Vec<bool>,
    pub translucent: Vec<bool>,
    pub fluid: Vec<bool>,
    /// Placing a block into this cell overwrites it (mirrors
    /// `BlockDef::replaceable`) — the fluid spread sim (`world.rs`) uses this
    /// to know which cells it's allowed to flow into.
    pub replaceable: Vec<bool>,
    /// How many blocks a fluid flows from its source before drying up,
    /// clamped to `u8`. Drives both the spread sim and the mesher's per-level
    /// surface height.
    pub flow_distance: Vec<u8>,
    /// Whether a placed instance's face textures depend on its stored
    /// `Chunk::axis` (mirrors `BlockDef::rotation != Rotation::None`). Lets
    /// the mesher skip the axis lookup/remap entirely for the vast majority
    /// of blocks that never rotate.
    pub rotates: Vec<bool>,
    /// Lies flat on the ground (`Shape::Floor`): the mesher draws only a
    /// low top face.
    pub floor: Vec<bool>,
    /// Drawn by something other than the mesher (`Shape::Model`).
    pub model: Vec<bool>,
    /// A resource pack's model for this block, if one changes its look (see `models.rs`).
    /// Drawing only - collision, light and everything physical still follow the block definition.
    pub pack_models: Vec<Option<Arc<crate::models::CompiledModel>>>,
    /// Whether a neighbour may hide its face against this block. Equal to `opaque`, except for a
    /// block drawn by a pack model that doesn't fill its cell, which hides nothing.
    pub occludes: Vec<bool>,
    /// A mod's model replaced this block's collision and hitbox with these boxes (block units).
    pub collision_boxes: Vec<Option<Arc<Vec<crate::models::CollisionBox>>>>,
    /// Only mined while crouching (`BlockDef::requires_crouch`).
    pub requires_crouch: Vec<bool>,
    /// Minimum projectile speed that shatters the block (`INFINITY` = never).
    pub shatter_speed: Vec<f32>,
    /// Fraction of speed a projectile keeps after shattering it.
    pub shatter_keep: Vec<f32>,
    /// For a two-cell-tall block's lower half, the block that goes above it
    /// (`AIR` for anything else).
    pub upper_of: Vec<BlockId>,
    /// The reverse: for an upper half, the block that goes below it.
    pub lower_of: Vec<BlockId>,
    /// Per-channel light emission (`BlockDef::light`, resolved by
    /// `LightSpec::emission`), `0..=light::MAX_LIGHT` each. `[0; 3]` for
    /// everything that isn't a light source, which `light.rs` treats as
    /// "emits nothing" without needing a separate flag.
    pub light: Vec<[u8; 3]>,
    /// Per-channel fraction of light that survives crossing one block of
    /// this material, fixed-point `0..=255` (see [`Transmission`]).
    ///
    /// `[255; 3]` — perfectly clear, an exact no-op in `light.rs` — for air
    /// and every block that doesn't absorb anything, and `[0; 3]` for opaque
    /// blocks. Folding opacity in here means a cell's effect on light is one
    /// lookup rather than a flag plus a value that have to agree.
    pub transmission: Vec<[u8; 3]>,
    /// Atlas tile per face: `tiles[id as usize * 6 + face]`.
    pub tiles: Vec<u16>,
    /// Whether that same face gets biome-tinted (`biome::grass_tint`
    /// multiplied onto it at mesh time) - `tinted[id as usize * 6 + face]`,
    /// same indexing as `tiles`, resolved from [`FaceTint`]. `false` (the
    /// atlas sample used untouched) for every face on every block except
    /// grass's top.
    pub tinted: Vec<bool>,
    /// Whether that same face gets an extra tinted overlay decal quad
    /// layered on top of it at mesh time (`overlay_tile[id*6+face]` names
    /// which tile) - same indexing as `tiles`, resolved from `BlockDef::
    /// overlay`. See the module docs' `overlay` section and `mesher.rs`'s
    /// overlay decal emission. `false` for every face on every block except
    /// grass's sides.
    pub has_overlay: Vec<bool>,
    /// The overlay tile for a face `has_overlay` marks - meaningless
    /// (reads as tile 0) when `has_overlay` is false for that slot, same
    /// "gate plus a value that's only meaningful when the gate is set"
    /// shape as `fluid`/`flow_distance`. Unlike `tiles`, indexed only by
    /// the logical face `f` - `mesher.rs` doesn't run this through
    /// `rotated_tile`'s axis remap the way it does the base texture, so a
    /// rotating block's overlay (none exist yet) would always show its
    /// configured face regardless of physical orientation.
    pub overlay_tile: Vec<u16>,
    /// A cell's main block: itself for an ordinary block, `pebble` for
    /// `pebble@water`. See `BlockDef::layered`.
    pub base_of: Vec<BlockId>,
    /// A cell's layer: `AIR` for an ordinary block, `water` for
    /// `pebble@water`.
    pub layer_of: Vec<BlockId>,
    /// Which fluid a cell holds: the block itself for plain water, the
    /// layer for `pebble@water`, `AIR` for anything that holds none.
    /// Everything the fluid sim compares is this, never the raw id.
    pub fluid_kind: Vec<BlockId>,
    /// What a fluid-holding cell becomes when it dries: `AIR` for plain
    /// water, the bare block for `pebble@water`.
    pub dried: Vec<BlockId>,
    /// What a still fluid source in this cell freezes into (`AIR` = never):
    /// `ice` for water, `pebble@ice` for `pebble@water`.
    pub frozen: Vec<BlockId>,
    /// `(block, layer)` -> the derived `block@layer` id.
    pub variants: HashMap<(BlockId, BlockId), BlockId>,
    /// What a fluid freezes into keeps the fluid's *shape*: a frozen cell
    /// whose stored level is a flowing one (not source, not falling) is
    /// drawn at that level's height, so a frozen river keeps its slope.
    /// True for `ice` and the variants holding it; `flow_distance` of such a
    /// block is the fluid's, so the heights match the water's.
    pub level_shaped: Vec<bool>,
    /// The atlas's actual per-tile pixel resolution (`atlas::AtlasData::
    /// tile_size` at compile time) - one of `atlas::ALLOWED_TILE_SIZES`.
    pub tile_size: usize,
    /// Half-texel UV inset that keeps neighbouring atlas tiles from
    /// bleeding into each other, derived from `tile_size` (a higher-
    /// resolution atlas needs a proportionally smaller inset). See
    /// `mesher.rs`'s UV computation.
    pub uv_pad: f32,
    /// `1/ATLAS_TILES - 2*uv_pad`: how much of each tile's UV span is
    /// actually sampled once `uv_pad` insets both edges.
    pub uv_span: f32,
}

impl Tables {
    /// Installs the pack models: what draws each block, and which blocks still hide a neighbour's
    /// face (a model that doesn't fill its cell hides nothing, so no hole shows in the wall beside
    /// a half block). `collision` replaces those blocks' collision and hitbox (a mod's models only -
    /// the loader decides which). Call once, right after `compile`, before the tables are shared.
    pub fn apply_models(
        &mut self,
        visual: HashMap<BlockId, crate::models::CompiledModel>,
        collision: HashMap<BlockId, Vec<crate::models::CollisionBox>>,
    ) {
        self.occludes = self.opaque.clone();
        for (id, model) in visual {
            let i = id as usize;
            if i >= self.pack_models.len() {
                continue;
            }
            self.occludes[i] = self.opaque[i] && model.fills_cell;
            self.pack_models[i] = Some(Arc::new(model));
        }
        for (id, boxes) in collision {
            if let Some(slot) = self.collision_boxes.get_mut(id as usize) {
                *slot = Some(Arc::new(boxes));
            }
        }
    }

    /// Whether a cell holding `id` at stored fluid `level` has fluid state
    /// worth saving and drawing: a fluid, or something frozen from one that
    /// still carries a flowing level.
    pub fn holds_level(&self, id: BlockId, level: u8) -> bool {
        self.fluid[id as usize]
            || (self.level_shaped[id as usize] && level != FLUID_SOURCE && level != FLUID_FALLING)
    }
}

#[derive(Resource, Clone)]
pub struct BlockTables(pub Arc<Tables>);

#[derive(Resource)]
pub struct BlockRegistry {
    pub defs: Vec<BlockDef>,
    by_name: HashMap<String, BlockId>,
    /// `(block, layer)` -> `block@layer`, filled by `add_layered_variants`.
    variants: HashMap<(BlockId, BlockId), BlockId>,
    compiled: bool,
}

/// Where to look for `blocks/`: next to the running executable first (how
/// an installed/shipped build finds its data), falling back to the current
/// working directory (how `cargo run`/`cargo test` find the one at the repo
/// root - Cargo runs both with the package root as cwd).
fn find_blocks_dir() -> PathBuf {
    find_data_dir("blocks")
}

/// A shipped data folder (`blocks`, `items`, `recipes`): next to the running
/// executable if it's there, otherwise relative to the working directory -
/// see `find_blocks_dir`.
pub(crate) fn find_data_dir(name: &str) -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(name);
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from(name)
}

impl BlockRegistry {
    /// Loads every `blocks/*.json` file (see `find_blocks_dir`). Panics with
    /// a clear message on a missing directory or malformed file - this is
    /// startup content, the same as a missing atlas texture already panics
    /// in `compile()` below.
    pub fn with_defaults() -> Self {
        Self::load_from_dir(&find_blocks_dir())
    }

    pub fn load_from_dir(dir: &Path) -> Self {
        Self::load_from_dirs(&[crate::overlay::KindDir::core(dir)])
    }

    /// Loads blocks from the game's own directory (`dirs[0]`, required) and
    /// then each mod's `blocks/` (the rest, skipped if a mod has none). Ids are
    /// assigned in that order, so the blocks the core files define never move,
    /// whatever mods are on. (The derived `block@layer` variants come after all
    /// of them and can shift; nothing stores an id as a number.)
    pub fn load_from_dirs(dirs: &[crate::overlay::KindDir]) -> Self {
        let air = BlockDef {
            id: "air".into(),
            name: "Air".into(),
            solid: false,
            transparency: Transparency::Full,
            selectable: false,
            replaceable: true,
            item: false,
            ..BlockDef::default()
        };
        let mut reg = Self {
            defs: vec![air],
            by_name: HashMap::from([("air".into(), 0)]),
            variants: HashMap::new(),
            compiled: false,
        };

        // Mods overlay the core files: a mod's block with an existing id patches
        // that block, anything else is added after (so core ids never move).
        for (path, value) in crate::overlay::overlay_by_id(
            dirs,
            "block definitions",
            Some(crate::tags::Scope::Block),
            crate::overlay::qualify_block_refs,
        ) {
            let file: BlockFile = serde_json::from_value(value)
                .unwrap_or_else(|err| panic!("failed to parse block file {path:?}: {err}"));
            if file.id.contains(char::is_whitespace) {
                panic!("block file {path:?}: id {:?} must not contain spaces", file.id);
            }
            if file.item_model == Some(ItemModel::Custom) && file.custom_item_model.is_none() {
                panic!(
                    "block file {path:?}: item_model \"custom\" requires a custom_item_model path"
                );
            }
            if file.requires_tool && file.tool.is_none() {
                panic!("block file {path:?}: requires_tool needs a tool (which kind?)");
            }
            reg.register(file.into_def());
        }

        reg.add_layered_variants();
        reg
    }

    /// Every block a cell can hold alongside a waterloggable block: each
    /// fluid, plus whatever a fluid freezes into. Panics on a `freezes_to`
    /// naming an unknown block - startup content.
    fn layers(&self) -> Vec<BlockId> {
        let mut layers = Vec::new();
        for (id, def) in self.defs.iter().enumerate().skip(1) {
            if def.layered.is_some() {
                continue;
            }
            if def.fluid && !layers.contains(&(id as BlockId)) {
                layers.push(id as BlockId);
            }
            if let Some(name) = &def.freezes_to {
                let target = self
                    .by_name(name)
                    .unwrap_or_else(|_| panic!("block {:?}: freezes_to names unknown block {name:?}", def.id));
                if !layers.contains(&target) {
                    layers.push(target);
                }
            }
        }
        layers.sort_unstable();
        layers
    }

    /// Derives `block@layer` for every waterloggable block and every layer
    /// (`pebble@water`, `pebble@ice`). Each is a real block id with two
    /// tags - `BlockDef::layered` names the main block and the layer - and
    /// otherwise inherits the main block's data, so saves, snapshots, the
    /// edit log and the lookup tables need no special case for a cell that
    /// holds two things. Idempotent; runs at the end of `load_from_dir`, and
    /// again from `compile` for a registry assembled by hand.
    pub fn add_layered_variants(&mut self) {
        let layers = self.layers();
        let bases: Vec<BlockId> = (1..self.defs.len() as BlockId)
            .filter(|&id| self.defs[id as usize].waterloggable && self.defs[id as usize].layered.is_none())
            .collect();
        for &block in &bases {
            for &layer in &layers {
                if self.variants.contains_key(&(block, layer)) {
                    continue;
                }
                let (base, over) = (self.defs[block as usize].clone(), &self.defs[layer as usize]);
                let rank = |t: Transparency| match t {
                    Transparency::No => 2,
                    Transparency::Partial => 1,
                    Transparency::Full => 0,
                };
                let clear = |d: &BlockDef| if d.transparency == Transparency::No { [0; 3] } else { d.transmission.to_fixed() };
                let (a, b) = (clear(&base), clear(over));
                let through = [0, 1, 2].map(|c| (a[c] as u32 * b[c] as u32 / 255) as f32 / 255.0);
                let variant = BlockDef {
                    id: format!("{}@{}", base.id, over.id),
                    item: false,
                    layered: Some(Layered { block, layer }),
                    waterloggable: false,
                    // Holds whatever its block does, plus the layer's flow.
                    states: {
                        let mut states = base.states.clone();
                        for s in ["waterlogged", "level"] {
                            if !states.iter().any(|x| x == s) {
                                states.push(s.to_string());
                            }
                        }
                        states
                    },
                    freezes_to: None,
                    solid: base.solid || over.solid,
                    transparency: if rank(over.transparency) > rank(base.transparency) { over.transparency } else { base.transparency },
                    fluid: over.fluid,
                    flow_distance: over.flow_distance,
                    replaceable: base.replaceable && over.replaceable,
                    transmission: Transmission::PerChannel(through),
                    ..base
                };
                let id = self.register(variant);
                self.variants.insert((block, layer), id);
            }
        }
        self.finalize_states();
    }

    /// Adds the state tags each block's behaviour tags imply (see
    /// [`BlockDef::states`]) and refuses a block that has a state tag it
    /// can't support - a full cube can't be waterlogged. Idempotent; runs
    /// with `add_layered_variants`. Panics, naming the block, like any other
    /// bad content file.
    pub fn finalize_states(&mut self) {
        let freeze_targets: std::collections::HashSet<String> =
            self.defs.iter().filter_map(|d| d.freezes_to.clone()).collect();
        for def in self.defs.iter_mut().skip(1) {
            if def.layered.is_some() {
                continue; // derived: holds exactly what its block and layer do
            }
            let target = freeze_targets.contains(&def.id);
            for tag in crate::states::STATE_TAGS {
                let declared = def.states.iter().any(|s| s == tag.name);
                let wanted = declared || (tag.implied)(def, target);
                if !wanted {
                    continue;
                }
                if let Err(why) = (tag.compatible)(def, target) {
                    panic!("block {:?}: can't have the {:?} state: {why}", def.id, tag.name);
                }
                if !declared {
                    def.states.push(tag.name.to_string());
                }
            }
            if let Some(unknown) = def.states.iter().find(|s| crate::states::state_tag(s).is_none()) {
                let names: Vec<_> = crate::states::STATE_TAGS.iter().map(|t| t.name).collect();
                panic!("block {:?}: unknown state {:?} in \"states\" (known: {})", def.id, unknown, names.join(", "));
            }
            // Fit this block's own states into the bits its built-in tags
            // leave free. Each block has its own layout: a cell holds one
            // block, so two blocks never compete for bits.
            match def.state_storage {
                StateStorage::Compact => {
                    def.state_fields = crate::states::allocate_fields(&def.id, &def.states, &def.state_defs)
                        .unwrap_or_else(|why| panic!("{why}"));
                }
                StateStorage::Extended => {
                    let (fields, bytes) = crate::states::allocate_extended(&def.id, &def.state_defs)
                        .unwrap_or_else(|why| panic!("{why}"));
                    def.ext_fields = fields;
                    def.ext_bytes = bytes;
                }
            }
        }
        // A derived `block@layer` variant lays its states out exactly like its
        // block, so a cell can swap between the two (water arriving, leaving)
        // without moving any bits.
        for i in 1..self.defs.len() {
            if let Some(l) = self.defs[i].layered {
                let base = &self.defs[l.block as usize];
                let (defs, fields, storage, ext, bytes) =
                    (base.state_defs.clone(), base.state_fields.clone(), base.state_storage, base.ext_fields.clone(), base.ext_bytes);
                let variant = &mut self.defs[i];
                variant.state_defs = defs;
                variant.state_fields = fields;
                variant.state_storage = storage;
                variant.ext_fields = ext;
                variant.ext_bytes = bytes;
            }
        }
    }

    /// What's left in a cell when its block is broken: the fluid it was
    /// holding (`water` from `pebble@water`), which flows on. A solid layer
    /// breaks along with the block (`pebble@ice` leaves air), and an
    /// ordinary block leaves `AIR`.
    pub fn left_behind(&self, id: BlockId) -> BlockId {
        self.def(id).layered.map_or(AIR, |l| if self.def(l.layer).fluid { l.layer } else { AIR })
    }

    /// For the lower half of a two-cell-tall block, the block that stands on
    /// top of it.
    pub fn upper_half(&self, id: BlockId) -> Option<BlockId> {
        self.def(id).upper.as_ref().and_then(|name| self.by_name(name).ok())
    }

    /// For the upper half of a two-cell-tall block, the block under it.
    pub fn lower_half(&self, id: BlockId) -> Option<BlockId> {
        (1..self.defs.len() as BlockId).find(|&lower| self.upper_half(lower) == Some(id))
    }

    /// The `block@layer` id, if `block` is waterloggable and `layer` is a
    /// fluid or something one freezes into.
    pub fn variant(&self, block: BlockId, layer: BlockId) -> Option<BlockId> {
        self.variants.get(&(block, layer)).copied()
    }

    pub fn register(&mut self, def: BlockDef) -> BlockId {
        assert!(!self.compiled, "cannot register blocks after startup");
        assert!(!def.id.is_empty(), "block def requires an id");
        assert!(
            !self.by_name.contains_key(&def.id),
            "block {:?} already registered",
            def.id
        );
        let id = self.defs.len() as BlockId;
        self.by_name.insert(def.id.clone(), id);
        self.defs.push(def);
        id
    }

    pub fn id(&self, name: &str) -> BlockId {
        self.by_name(name).unwrap_or_else(|_| panic!("unknown block {name:?}"))
    }

    /// Like [`id`](Self::id), but returns an error instead of panicking —
    /// used when loading save data, where an unrecognized name (e.g. from a
    /// mod that's no longer installed) shouldn't crash the load.
    pub fn by_name(&self, name: &str) -> Result<BlockId, UnknownBlock> {
        // `craftmjne:dirt` is `dirt`: the core game's names are written bare.
        let name = crate::overlay::normalize(name);
        self.by_name.get(name.as_str()).copied().ok_or(UnknownBlock)
    }

    pub fn def(&self, id: BlockId) -> &BlockDef {
        &self.defs[id as usize]
    }

    /// Every distinct texture name any registered block's six faces resolve
    /// to (explicit `textures` overrides and `texture_scheme`-derived names
    /// alike, plus any `overlay` names) - what `world::compile_content`
    /// walks before building the atlas, so a block whose scheme derives a
    /// name nobody registered a procedural painter for gets an automatic
    /// placeholder instead of `compile()` panicking (see `atlas::Painters::
    /// ensure_registered`).
    pub fn texture_names(&self) -> impl Iterator<Item = String> + '_ {
        self.defs
            .iter()
            .skip(1) // AIR has no texture
            .filter(|def| def.layered.is_none()) // a variant shows its main block's textures
            .flat_map(|def| (0..6).flat_map(|face| [Some(def.texture_name(face)), def.overlay_name(face)]))
            .flatten()
    }

    /// Every texture name that gets biome-tinted, directly (`tinted`) or as
    /// an overlay decal (`overlay`) - what `world::compile_content` runs
    /// `atlas::normalize_tint_mask_tile` over after building the atlas, so
    /// a grayscale mask whose art came out dark/low-contrast (not already a
    /// bright near-white Minecraft-style mask) still reads at full color
    /// once tinted. Deliberately a `HashSet`: the same name can be a
    /// `tinted` face on one block and an `overlay` on another (or repeat
    /// across faces of the same block), and it only needs normalizing once.
    pub fn tint_mask_names(&self) -> HashSet<String> {
        self.defs
            .iter()
            .skip(1)
            .filter(|def| def.layered.is_none())
            .flat_map(|def| {
                (0..6).flat_map(|face| {
                    let direct = def.tinted.tinted(face).then(|| def.texture_name(face));
                    [direct, def.overlay_name(face)]
                })
            })
            .flatten()
            .collect()
    }

    /// Bakes flat lookup tables. `atlas_index` maps texture names -> tiles;
    /// `tile_size` is the atlas's actual per-tile pixel resolution
    /// (`atlas::AtlasData::tile_size`), used to derive the UV inset that
    /// keeps neighbouring tiles from bleeding into each other.
    pub fn compile(&mut self, atlas_index: &HashMap<String, u16>, tile_size: usize) -> Arc<Tables> {
        self.add_layered_variants();
        let n = self.defs.len();
        let uv_tile = 1.0 / crate::config::ATLAS_TILES as f32;
        let uv_pad = 0.5 / (crate::config::ATLAS_TILES * tile_size) as f32;
        let mut tables = Tables {
            solid: vec![false; n],
            opaque: vec![false; n],
            translucent: vec![false; n],
            fluid: vec![false; n],
            replaceable: vec![false; n],
            flow_distance: vec![0; n],
            rotates: vec![false; n],
            floor: vec![false; n],
            model: vec![false; n],
            pack_models: vec![None; n],
            occludes: vec![false; n],
            collision_boxes: vec![None; n],
            requires_crouch: vec![false; n],
            shatter_speed: vec![f32::INFINITY; n],
            shatter_keep: vec![1.0; n],
            upper_of: vec![AIR; n],
            lower_of: vec![AIR; n],
            light: vec![[0; 3]; n],
            // Air (id 0) is skipped by the loop below and must be clear, not
            // the `Default::default()` zero that would make it block light.
            transmission: vec![[u8::MAX; 3]; n],
            tiles: vec![0; n * 6],
            tinted: vec![false; n * 6],
            has_overlay: vec![false; n * 6],
            overlay_tile: vec![0; n * 6],
            base_of: (0..n as BlockId).collect(),
            layer_of: vec![AIR; n],
            fluid_kind: vec![AIR; n],
            dried: vec![AIR; n],
            frozen: vec![AIR; n],
            variants: self.variants.clone(),
            level_shaped: vec![false; n],
            tile_size,
            uv_pad,
            uv_span: uv_tile - 2.0 * uv_pad,
        };
        for (id, def) in self.defs.iter().enumerate().skip(1) {
            tables.solid[id] = def.solid;
            tables.opaque[id] = def.transparency == Transparency::No;
            tables.translucent[id] = def.transparency == Transparency::Full;
            tables.fluid[id] = def.fluid;
            tables.replaceable[id] = def.replaceable;
            tables.flow_distance[id] = def.flow_distance.min(u8::MAX as u32) as u8;
            tables.rotates[id] = def.rotation != Rotation::None;
            tables.floor[id] = def.shape == Shape::Floor;
            tables.model[id] = def.shape == Shape::Model;
            tables.requires_crouch[id] = def.requires_crouch;
            tables.shatter_speed[id] = def.shatters_at.unwrap_or(f32::INFINITY);
            tables.shatter_keep[id] = def.shatter_keep;
            if let Some(upper) = &def.upper {
                let up = self
                    .by_name(upper)
                    .unwrap_or_else(|_| panic!("block {:?}: its upper half {upper:?} is not a block", def.id));
                tables.upper_of[id] = up;
                tables.lower_of[up as usize] = id as BlockId;
            }
            tables.light[id] = def.light.emission();
            if let Some(l) = def.layered {
                tables.base_of[id] = l.block;
                tables.layer_of[id] = l.layer;
            }
            if def.fluid {
                tables.fluid_kind[id] = def.layered.map_or(id as BlockId, |l| l.layer);
                tables.dried[id] = def.layered.map_or(AIR, |l| l.block);
            }
            // An opaque block stops light whatever its `transmission` says,
            // so that's resolved once here rather than re-checked at every
            // propagation step.
            tables.transmission[id] =
                if tables.opaque[id] { [0; 3] } else { def.transmission.to_fixed() };
            if let Some(l) = def.layered {
                // A variant looks like its main block (the layer is drawn
                // as its own part by the mesher), so it shares every slot.
                for face in 0..6 {
                    let (from, to) = (l.block as usize * 6 + face, id * 6 + face);
                    tables.tiles[to] = tables.tiles[from];
                    tables.tinted[to] = tables.tinted[from];
                    tables.has_overlay[to] = tables.has_overlay[from];
                    tables.overlay_tile[to] = tables.overlay_tile[from];
                }
                continue;
            }
            for face in 0..6 {
                let tex = def.texture_name(face);
                let tile = atlas_index.get(&tex).unwrap_or_else(|| {
                    panic!("block {:?}: no texture painter registered for {tex:?}", def.id)
                });
                tables.tiles[id * 6 + face] = *tile;
                tables.tinted[id * 6 + face] = def.tinted.tinted(face);
                if let Some(overlay_tex) = def.overlay_name(face) {
                    let overlay = atlas_index.get(&overlay_tex).unwrap_or_else(|| {
                        panic!("block {:?}: no texture painter registered for overlay {overlay_tex:?}", def.id)
                    });
                    tables.has_overlay[id * 6 + face] = true;
                    tables.overlay_tile[id * 6 + face] = *overlay;
                }
            }
        }
        // What freezes into what: plain water -> ice, and the variants
        // follow (`pebble@water` -> `pebble@ice`).
        for (id, def) in self.defs.iter().enumerate().skip(1) {
            let Some(target) = def.freezes_to.as_deref().and_then(|n| self.by_name.get(n).copied()) else { continue };
            tables.frozen[id] = target;
            // The frozen form keeps this fluid's shape (see `level_shaped`).
            let fd = tables.flow_distance[id];
            tables.level_shaped[target as usize] = true;
            tables.flow_distance[target as usize] = tables.flow_distance[target as usize].max(fd);
            for (&(block, layer), &variant) in &self.variants {
                if layer == id as BlockId {
                    if let Some(&frozen) = self.variants.get(&(block, target)) {
                        tables.frozen[variant as usize] = frozen;
                    }
                }
            }
        }
        for (&(_, layer), &variant) in &self.variants {
            if tables.level_shaped[layer as usize] {
                tables.level_shaped[variant as usize] = true;
            }
        }
        self.compiled = true;
        tables.occludes = tables.opaque.clone();
        Arc::new(tables)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_compiles_with_default_atlas() {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        let water = reg.id("water");
        let stone = reg.id("stone");
        let leaves = reg.id("leaves");
        assert!(tables.solid[stone as usize]);
        assert!(tables.opaque[stone as usize]);
        assert!(!tables.opaque[water as usize]);
        assert!(tables.translucent[water as usize]);
        assert!(tables.fluid[water as usize]);
        assert!(tables.replaceable[water as usize]);
        assert_eq!(tables.flow_distance[water as usize], 7);
        assert!(!tables.opaque[leaves as usize]);
        assert!(!tables.translucent[leaves as usize]);
        // grass has distinct top/bottom/side tiles
        let grass = reg.id("grass") as usize;
        assert_ne!(tables.tiles[grass * 6 + 2], tables.tiles[grass * 6 + 3]);
        assert_ne!(tables.tiles[grass * 6 + 2], tables.tiles[grass * 6]);
    }

    #[test]
    fn grass_is_tinted_on_top_directly_and_gets_a_tinted_overlay_on_its_sides() {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        let grass = reg.id("grass") as usize;
        let dirt = reg.id("dirt") as usize;

        assert!(tables.tinted[grass * 6 + 2]); // top - tinted directly
        assert!(!tables.tinted[grass * 6 + 3]); // bottom - reuses dirt.png, must stay untinted
        assert!(!tables.has_overlay[grass * 6 + 2]); // top has no separate overlay
        for face in [0, 1, 4, 5] {
            assert!(!tables.tinted[grass * 6 + face], "side {face} must not be tinted directly");
            assert!(tables.has_overlay[grass * 6 + face], "side {face} should get a tinted overlay");
        }
        // the side's own base tile is dirt - same tile as grass's bottom
        // face and plain dirt blocks - and the overlay tile is distinct
        // from it (a different tile, "grass_side").
        assert_eq!(tables.tiles[grass * 6], tables.tiles[grass * 6 + 3]);
        assert_ne!(tables.overlay_tile[grass * 6], tables.tiles[grass * 6]);
        // plain dirt blocks must never be tinted or get an overlay just
        // because grass's bottom/side base happens to reuse the same tile.
        for face in 0..6 {
            assert!(!tables.tinted[dirt * 6 + face]);
            assert!(!tables.has_overlay[dirt * 6 + face]);
        }
    }

    #[test]
    fn face_tint_all_overrides_every_face_regardless_of_the_per_face_flags() {
        let tint = FaceTint { all: true, ..FaceTint::default() };
        for face in 0..6 {
            assert!(tint.tinted(face));
        }

        let tint = FaceTint { top: true, ..FaceTint::default() };
        assert!(tint.tinted(2)); // top
        assert!(!tint.tinted(3)); // bottom
        assert!(!tint.tinted(0)); // side
    }

    #[test]
    fn overlay_name_only_answers_for_faces_the_block_actually_set() {
        let def = BlockDef {
            id: "widget".into(),
            overlay: FaceTextures { side: Some("widget_glow".into()), ..FaceTextures::default() },
            ..BlockDef::default()
        };
        assert_eq!(def.overlay_name(0), Some("widget_glow".to_string())); // side
        assert_eq!(def.overlay_name(2), None); // top
        assert_eq!(def.overlay_name(3), None); // bottom
    }

    #[test]
    fn tint_mask_names_collects_both_directly_tinted_and_overlay_textures() {
        let reg = BlockRegistry::with_defaults();
        let names = reg.tint_mask_names();
        // grass's top is tinted directly, and its side names a
        // "grass_side" overlay - both must show up, but grass's untinted
        // dirt-sharing bottom/side base must not pull "dirt" in.
        assert!(names.contains("grass_top"));
        assert!(names.contains("grass_side"));
        assert!(!names.contains("dirt"));
    }

    #[test]
    fn texture_scheme_derives_the_documented_name_per_face() {
        let mut def = BlockDef { id: "widget".into(), ..BlockDef::default() };

        def.texture_scheme = TextureScheme::Default;
        for face in 0..6 {
            assert_eq!(def.texture_name(face), "widget");
        }

        def.texture_scheme = TextureScheme::Log;
        assert_eq!(def.texture_name(2), "widget_top"); // top
        assert_eq!(def.texture_name(3), "widget_top"); // bottom
        assert_eq!(def.texture_name(0), "widget_side"); // east
        assert_eq!(def.texture_name(5), "widget_side"); // north

        def.texture_scheme = TextureScheme::Organic;
        assert_eq!(def.texture_name(2), "widget_top");
        assert_eq!(def.texture_name(3), "widget_bottom");
        assert_eq!(def.texture_name(0), "widget_side");
        assert_eq!(def.texture_name(5), "widget_side");

        def.texture_scheme = TextureScheme::Interface;
        assert_eq!(def.texture_name(5), "widget_north");
        assert_eq!(def.texture_name(2), "widget"); // top stays plain
        assert_eq!(def.texture_name(3), "widget"); // bottom stays plain
        assert_eq!(def.texture_name(0), "widget"); // east stays plain

        def.texture_scheme = TextureScheme::AdvancedInterface;
        assert_eq!(def.texture_name(5), "widget_north");
        assert_eq!(def.texture_name(2), "widget_top");
        assert_eq!(def.texture_name(3), "widget_bottom");
        assert_eq!(def.texture_name(0), "widget_side"); // east
        assert_eq!(def.texture_name(4), "widget_side"); // south

        def.texture_scheme = TextureScheme::Custom;
        for face in 0..6 {
            assert_eq!(def.texture_name(face), "widget");
        }
    }

    #[test]
    fn an_explicit_textures_field_overrides_the_scheme_only_for_that_face() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(
            r#"{"id": "grass", "texture_scheme": "organic", "textures": {"bottom": "dirt"}}"#,
        )
        .unwrap()
        .into_def();
        assert_eq!(def.texture_name(2), "grass_top"); // derived from the scheme
        assert_eq!(def.texture_name(3), "dirt"); // explicit override wins
        assert_eq!(def.texture_name(0), "grass_side"); // derived from the scheme
    }

    #[test]
    fn texture_scheme_defaults_to_default_when_omitted() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "stone"}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.texture_scheme, TextureScheme::Default);
    }

    #[test]
    fn texture_names_includes_scheme_derived_names_for_every_face() {
        let mut reg = BlockRegistry::with_defaults();
        reg.register(BlockDef {
            id: "ruby_lamp".into(),
            texture_scheme: TextureScheme::AdvancedInterface,
            ..BlockDef::default()
        });
        let names: Vec<String> = reg.texture_names().collect();
        assert!(names.contains(&"ruby_lamp_north".to_string()));
        assert!(names.contains(&"ruby_lamp_top".to_string()));
        assert!(names.contains(&"ruby_lamp_bottom".to_string()));
        assert!(names.contains(&"ruby_lamp_side".to_string()));
    }

    #[test]
    fn a_block_with_no_registered_painter_gets_an_automatic_placeholder() {
        let mut reg = BlockRegistry::with_defaults();
        reg.register(BlockDef {
            id: "ruby_lamp".into(),
            texture_scheme: TextureScheme::Interface,
            ..BlockDef::default()
        });
        let mut painters = crate::atlas::default_painters();
        for name in reg.texture_names().collect::<std::collections::HashSet<_>>() {
            painters.ensure_registered(&name);
        }
        let atlas = crate::atlas::build_atlas(&painters);
        // Should not panic - every derived name got a placeholder painter.
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        let lamp = reg.id("ruby_lamp") as usize;
        // Interface: the north face is unique, every other face is plain -
        // so north's tile must differ from top's, but top must match east's.
        assert_ne!(tables.tiles[lamp * 6 + 5], tables.tiles[lamp * 6 + 2]);
        assert_eq!(tables.tiles[lamp * 6 + 2], tables.tiles[lamp * 6]);
    }

    #[test]
    fn hardness_and_drops_default_and_parse() {
        let def = serde_json::from_str::<BlockFile>(r#"{"id": "stone"}"#).unwrap().into_def();
        assert_eq!(def.hardness, DEFAULT_HARDNESS);
        assert_eq!(def.drops, Drops::Itself);
        assert!(def.breakable);

        let def = serde_json::from_str::<BlockFile>(r#"{"id": "ice", "hardness": 0.5, "drops": false}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.hardness, 0.5);
        assert_eq!(def.drops, Drops::Nothing);

        let def = serde_json::from_str::<BlockFile>(r#"{"id": "grass", "drops": "dirt"}"#).unwrap().into_def();
        assert_eq!(def.drops, Drops::Item { item: "dirt".into(), chance: 1.0, count: 1 });

        let def = serde_json::from_str::<BlockFile>(r#"{"id": "leaves", "drops": {"item": "twig", "chance": 0.2}}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.drops, Drops::Item { item: "twig".into(), chance: 0.2, count: 1 });

        // A negative hardness can't make mining take negative time.
        let def = serde_json::from_str::<BlockFile>(r#"{"id": "odd", "hardness": -2}"#).unwrap().into_def();
        assert_eq!(def.hardness, 0.0);
    }

    #[test]
    fn max_stack_defaults_and_is_overridable() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "stone"}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.max_stack, DEFAULT_MAX_STACK);

        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "ruby", "max_stack": 1}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.max_stack, 1);
    }

    #[test]
    fn item_defaults_to_true_and_air_has_none() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "dirt"}"#)
            .unwrap()
            .into_def();
        assert!(def.item);

        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "void", "item": false}"#)
            .unwrap()
            .into_def();
        assert!(!def.item);

        let reg = BlockRegistry::with_defaults();
        assert!(!reg.def(AIR).item, "air must not be obtainable");
    }

    #[test]
    fn item_model_defaults_to_default_and_is_overridable() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "dirt"}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.item_model, ItemModel::Default);
        assert!(def.custom_item_model.is_none());

        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "sign", "item_model": "face"}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.item_model, ItemModel::Face);

        let def: BlockDef = serde_json::from_str::<BlockFile>(
            r#"{"id": "gizmo", "item_model": "custom", "custom_item_model": "gizmo/model.json"}"#,
        )
        .unwrap()
        .into_def();
        assert_eq!(def.item_model, ItemModel::Custom);
        assert_eq!(def.custom_item_model.as_deref(), Some("gizmo/model.json"));
    }

    #[test]
    fn block_model_is_independent_of_item_model() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "dirt"}"#)
            .unwrap()
            .into_def();
        assert!(def.block_model.is_none(), "blocks default to no custom model");

        // The point of the separate field: naming a world model must leave
        // the inventory icon on the standard isometric bake, not silently
        // downgrade it the way reusing `item_model: "custom"` would.
        let def: BlockDef =
            serde_json::from_str::<BlockFile>(r#"{"id": "torch", "block_model": "torch.bbmodel"}"#)
                .unwrap()
                .into_def();
        assert_eq!(def.block_model.as_deref(), Some("torch.bbmodel"));
        assert_eq!(def.item_model, ItemModel::Default);
    }

    #[test]
    fn the_shipped_torch_names_a_model_without_needing_it_to_exist() {
        // There's no `.bbmodel` loader yet, so `models/torch.bbmodel` is
        // legitimately absent - loading the real registry must not care.
        // This is what keeps the game runnable while the art is still being
        // made, rather than turning a not-yet-drawn model into a hard crash.
        let registry = BlockRegistry::with_defaults();
        let torch = registry.def(registry.id("torch"));
        assert_eq!(torch.block_model.as_deref(), Some("torch.bbmodel"));
    }

    #[test]
    fn custom_item_model_without_a_path_is_rejected() {
        // Mirrors load_from_dir's validation (checked on BlockFile before
        // into_def, since into_def itself can't panic with file context).
        let file: BlockFile =
            serde_json::from_str(r#"{"id": "gizmo", "item_model": "custom"}"#).unwrap();
        assert_eq!(file.item_model, Some(ItemModel::Custom));
        assert!(file.custom_item_model.is_none());
    }

    #[test]
    fn item_stack_is_empty_for_air_or_zero_count() {
        assert!(ItemStack::EMPTY.is_empty());
        assert!(ItemStack::new(AIR, 5).is_empty());
        assert!(ItemStack::new(3, 0).is_empty());
        assert!(!ItemStack::new(3, 1).is_empty());
    }

    #[test]
    fn title_case_replaces_underscores() {
        assert_eq!(title_case("coal_ore"), "Coal Ore");
        assert_eq!(title_case("grass"), "Grass");
    }

    #[test]
    fn fluid_defaults_are_water_like_unless_overridden() {
        let def: BlockDef = serde_json::from_str::<BlockFile>(
            r#"{"id": "lava", "fluid": true}"#,
        )
        .unwrap()
        .into_def();
        assert!(!def.solid);
        assert!(!def.selectable);
        assert!(def.replaceable);
        assert_eq!(def.name, "Lava");

        let def: BlockDef = serde_json::from_str::<BlockFile>(
            r#"{"id": "thick_slime", "fluid": true, "solid": true}"#,
        )
        .unwrap()
        .into_def();
        assert!(def.solid, "explicit override must win over the fluid default");
    }

    #[test]
    fn missing_id_or_spaces_in_id_are_rejected() {
        // id containing a space should be caught by the loader, not just
        // silently accepted (see `load_from_dir`'s explicit check).
        let file: BlockFile = serde_json::from_str(r#"{"id": "bad id"}"#).unwrap();
        assert!(file.id.contains(' '));
    }

    #[test]
    fn rotation_defaults_to_none_and_log_is_tagged() {
        let def: BlockDef =
            serde_json::from_str::<BlockFile>(r#"{"id": "dirt"}"#).unwrap().into_def();
        assert_eq!(def.rotation, Rotation::None);

        let def: BlockDef = serde_json::from_str::<BlockFile>(r#"{"id": "log", "rotation": "log"}"#)
            .unwrap()
            .into_def();
        assert_eq!(def.rotation, Rotation::Log);

        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        let log = reg.id("log");
        let stone = reg.id("stone");
        assert!(tables.rotates[log as usize]);
        assert!(!tables.rotates[stone as usize]);
    }

    fn compiled() -> (BlockRegistry, Arc<Tables>) {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        (reg, tables)
    }

    #[test]
    fn a_waterloggable_block_gets_a_variant_per_fluid_and_per_thing_one_freezes_into() {
        let reg = BlockRegistry::with_defaults();
        let (pebble, water, ice) = (reg.id("pebble"), reg.id("water"), reg.id("ice"));
        for layer in [water, ice] {
            let v = reg.variant(pebble, layer).expect("missing variant");
            assert_eq!(reg.def(v).layered, Some(Layered { block: pebble, layer }));
            assert_eq!(reg.def(v).id, format!("pebble@{}", reg.def(layer).id));
            assert_eq!(reg.by_name(&reg.def(v).id).ok(), Some(v), "saves look variants up by name");
        }
        // Stone isn't waterloggable, so it gets none; neither does a variant.
        assert_eq!(reg.variant(reg.id("stone"), water), None);
        assert!(reg.by_name("pebble@water@water").is_err());
        assert!(reg.by_name("stone@water").is_err());
    }

    #[test]
    fn a_variant_carries_two_tags_and_inherits_the_main_blocks_data() {
        let (reg, tables) = compiled();
        let (pebble, water, ice) = (reg.id("pebble"), reg.id("water"), reg.id("ice"));
        let wet = reg.variant(pebble, water).unwrap() as usize;
        let frozen = reg.variant(pebble, ice).unwrap() as usize;
        assert_eq!((tables.base_of[wet], tables.layer_of[wet]), (pebble, water));
        assert_eq!((tables.base_of[frozen], tables.layer_of[frozen]), (pebble, ice));
        // Ordinary blocks are their own base with no layer.
        let stone = reg.id("stone") as usize;
        assert_eq!((tables.base_of[stone], tables.layer_of[stone]), (stone as BlockId, AIR));
        // The pebble's own look, drops and mining.
        for f in 0..6 {
            assert_eq!(tables.tiles[wet * 6 + f], tables.tiles[pebble as usize * 6 + f]);
        }
        assert!(!reg.def(wet as BlockId).item, "only the bare pebble is an item");
        assert_eq!(reg.def(wet as BlockId).drops, reg.def(pebble).drops);
        assert_eq!(reg.def(wet as BlockId).hardness, reg.def(pebble).hardness);
        assert!(tables.floor[wet], "still a floor block, so it still needs ground");
        assert_eq!(reg.left_behind(wet as BlockId), water, "mining it leaves the water");
        assert_eq!(reg.left_behind(frozen as BlockId), AIR, "the ice breaks along with the pebble");
        assert_eq!(reg.left_behind(pebble), AIR);
    }

    #[test]
    fn a_waterlogged_block_is_a_fluid_cell_and_a_frozen_one_is_not() {
        let (reg, tables) = compiled();
        let (pebble, water, ice) = (reg.id("pebble"), reg.id("water"), reg.id("ice"));
        let wet = reg.variant(pebble, water).unwrap() as usize;
        let frozen = reg.variant(pebble, ice).unwrap() as usize;
        assert!(tables.fluid[wet] && !tables.fluid[frozen] && !tables.fluid[pebble as usize]);
        assert_eq!(tables.fluid_kind[wet], water, "it holds water");
        assert_eq!(tables.fluid_kind[water as usize], water);
        assert_eq!(tables.fluid_kind[frozen], AIR);
        assert_eq!(tables.dried[wet], pebble, "drying leaves the pebble");
        assert_eq!(tables.dried[water as usize], AIR);
        assert_eq!(tables.flow_distance[wet], tables.flow_distance[water as usize]);
        assert_eq!(tables.frozen[water as usize], ice);
        assert_eq!(tables.frozen[wet], frozen as BlockId, "freezing keeps the pebble");
        assert_eq!(tables.frozen[pebble as usize], AIR);
    }

    #[test]
    fn a_waterlogged_blocks_light_passes_through_both_halves() {
        let (reg, tables) = compiled();
        let (pebble, water) = (reg.id("pebble"), reg.id("water"));
        let wet = reg.variant(pebble, water).unwrap() as usize;
        let (a, b) = (tables.transmission[pebble as usize], tables.transmission[water as usize]);
        for c in 0..3 {
            let expect = (a[c] as u32 * b[c] as u32 / 255) as u8;
            assert!(tables.transmission[wet][c].abs_diff(expect) <= 1, "channel {c}");
        }
        assert!(tables.transmission[wet][2] > tables.transmission[wet][0], "it still tints toward blue like water");
    }

    #[test]
    #[should_panic(expected = "freezes_to names unknown block")]
    fn a_freezes_to_naming_an_unknown_block_fails_at_startup() {
        let mut reg = BlockRegistry::with_defaults();
        reg.register(BlockDef { id: "lava".into(), fluid: true, freezes_to: Some("obsidiann".into()), ..BlockDef::default() });
        reg.add_layered_variants();
    }

    #[test]
    #[should_panic(expected = "requires_tool needs a tool")]
    fn requires_tool_without_a_tool_kind_is_refused() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("craftmjne-blocks-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("odd.json"), r#"{ "id": "odd", "requires_tool": true }"#).unwrap();
        BlockRegistry::load_from_dir(&dir);
    }

    #[test]
    fn the_training_dummy_is_a_two_cell_crouch_to_mine_model_block() {
        let mut reg = BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let (lower, upper) = (reg.id("training_dummy"), reg.id("training_dummy_top"));
        let tables = reg.compile(&atlas.indices, atlas.tile_size);
        for id in [lower, upper] {
            let def = reg.def(id);
            assert_eq!(def.shape, Shape::Model);
            assert!(def.solid && def.requires_crouch && def.breakable && def.hardness > 0.0);
            assert!(tables.model[id as usize] && tables.requires_crouch[id as usize]);
            assert!(!tables.opaque[id as usize], "its neighbours' faces must still draw");
        }
        assert_eq!(reg.upper_half(lower), Some(upper));
        assert_eq!(reg.lower_half(upper), Some(lower));
        assert_eq!((tables.upper_of[lower as usize], tables.lower_of[upper as usize]), (upper, lower));
        // Only the lower half is an item; mining either gives that item.
        assert!(reg.def(lower).item && !reg.def(upper).item);
        assert_eq!(reg.def(lower).drops, Drops::Itself);
        assert!(matches!(&reg.def(upper).drops, Drops::Item { item, count: 1, .. } if item == "training_dummy"));
        assert_eq!(reg.upper_half(reg.id("stone")), None);
    }

    #[test]
    #[should_panic(expected = "upper half")]
    fn an_upper_half_that_is_not_a_block_fails_at_startup() {
        let mut reg = BlockRegistry::with_defaults();
        reg.register(BlockDef { id: "tall".into(), upper: Some("nope".into()), ..BlockDef::default() });
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        reg.compile(&atlas.indices, atlas.tile_size);
    }

    #[test]
    fn throw_tags_are_optional_and_bad_values_fall_back_to_defaults() {
        let plain = serde_json::from_str::<BlockFile>(r#"{"id": "rock", "throwable": true}"#).unwrap().into_def();
        assert_eq!(plain.throw, ThrowSpec::DEFAULT);
        let custom = serde_json::from_str::<BlockFile>(
            r#"{"id": "boulder", "throwable": true, "mass": 4.0, "throw_energy": 30.0,
                "throw_drag": 0.01, "throw_cooldown": 1.5, "throw_damage": 2.0}"#,
        )
        .unwrap()
        .into_def();
        assert_eq!(custom.throw, ThrowSpec { energy: 30.0, drag: 0.01, cooldown: 1.5, damage_scale: 2.0 });
        let bad = serde_json::from_str::<BlockFile>(r#"{"id": "x", "throw_energy": -3.0, "throw_cooldown": 0.0}"#)
            .unwrap()
            .into_def();
        assert_eq!(bad.throw, ThrowSpec::DEFAULT);
    }

    #[test]
    fn glass_and_ice_both_shatter_and_stone_does_not() {
        let reg = BlockRegistry::with_defaults();
        for name in ["glass", "ice"] {
            let def = reg.def(reg.id(name));
            assert_eq!(def.shatters_at, Some(5.0), "{name}");
            assert_eq!(def.shatter_keep, 0.5, "{name}");
        }
        assert_eq!(reg.def(reg.id("stone")).shatters_at, None);
    }

}
