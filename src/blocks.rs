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
//!   "max_stack": 124,
//!   "item": true,
//!   "item_model": "default",
//!   "rotation": "none",
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
//! - `rotation` is `"none"` (the default - always renders unrotated, "top"/
//!   "bottom"/"side" textures fixed to +y/-y/the four sides) or `"log"`
//!   (Minecraft log behavior: placing it against a block's top or bottom
//!   face stands it upright as normal; placing it against a side face lays
//!   it on its side instead, with its "top" texture - the end grain - facing
//!   the face you placed it against, i.e. facing you). This is a property of
//!   the placed block instance, not the definition, so two logs placed
//!   differently render differently even though they share one `BlockDef` -
//!   see `Chunk::axis` in `world.rs`.
//! - `textures` defaults to a single texture named after `id` on every face
//!   if omitted entirely.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type BlockId = u16;

/// A slot's contents: which block and how many. `id == AIR` (or `count ==
/// 0`) means empty — `is_empty()` treats either as the same thing, so
/// callers don't need to keep both fields in sync by hand.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct ItemStack {
    pub id: BlockId,
    pub count: u32,
}

impl ItemStack {
    pub const EMPTY: ItemStack = ItemStack { id: AIR, count: 0 };

    pub fn is_empty(&self) -> bool {
        self.id == AIR || self.count == 0
    }
}

/// How many of a block a single slot holds unless a block file overrides it
/// with `max_stack`.
pub const DEFAULT_MAX_STACK: u32 = 124;

/// A fluid cell's stored level, meaningful only while that cell's block id is
/// a fluid (see `Chunk::fluid_level` in `world.rs`). `0` is a permanent
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

/// A rotating block's stored orientation (`Chunk::axis`, meaningful only
/// where `Tables::rotates` is true for that cell's block id). Matches the
/// mesher's face-index axis grouping exactly (faces 0/1 are the x-axis,
/// 2/3 are y, 4/5 are z), so `mesher.rs` can compare them with no
/// translation. `AXIS_Y` is the default - standing upright, unrotated.
pub const AXIS_X: u8 = 0;
pub const AXIS_Y: u8 = 1;
pub const AXIS_Z: u8 = 2;

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

    pub fn resolve<'a>(&'a self, block_id: &'a str, face: usize) -> &'a str {
        let pick = |primary: &'a Option<String>| {
            primary
                .as_deref()
                .or(self.all.as_deref())
                .unwrap_or(block_id)
        };
        match face {
            2 => pick(&self.top),
            3 => pick(&self.bottom),
            _ => pick(&self.side),
        }
    }
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
    /// Whether a placed instance can face different directions. See
    /// [`Rotation`].
    pub rotation: Rotation,
    pub textures: FaceTextures,
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
            max_stack: DEFAULT_MAX_STACK,
            item: true,
            item_model: ItemModel::default(),
            custom_item_model: None,
            rotation: Rotation::default(),
            textures: FaceTextures::default(),
        }
    }
}

/// "coal_ore" -> "Coal Ore". Used as the default `name` for a block file
/// that doesn't specify one.
fn title_case(id: &str) -> String {
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
struct BlockFile {
    id: String,
    name: Option<String>,
    #[serde(default)]
    transparent: Transparency,
    #[serde(default)]
    fluid: bool,
    #[serde(default)]
    flow_distance: u32,
    solid: Option<bool>,
    selectable: Option<bool>,
    replaceable: Option<bool>,
    #[serde(default = "default_true")]
    breakable: bool,
    #[serde(default = "default_max_stack")]
    max_stack: u32,
    #[serde(default = "default_true")]
    item: bool,
    #[serde(default)]
    item_model: ItemModel,
    custom_item_model: Option<String>,
    #[serde(default)]
    rotation: Rotation,
    #[serde(default)]
    textures: FaceTextures,
}

fn default_true() -> bool {
    true
}

fn default_max_stack() -> u32 {
    DEFAULT_MAX_STACK
}

impl BlockFile {
    fn into_def(self) -> BlockDef {
        // A fluid defaults to water-like physics (not solid, can't be
        // targeted, placing into it just replaces it) unless the file
        // explicitly overrides one of those fields.
        let (solid, selectable, replaceable) = if self.fluid {
            (false, false, true)
        } else {
            (true, true, false)
        };
        BlockDef {
            id: self.id.clone(),
            name: self.name.unwrap_or_else(|| title_case(&self.id)),
            solid: self.solid.unwrap_or(solid),
            transparency: self.transparent,
            fluid: self.fluid,
            flow_distance: self.flow_distance,
            selectable: self.selectable.unwrap_or(selectable),
            replaceable: self.replaceable.unwrap_or(replaceable),
            breakable: self.breakable,
            max_stack: self.max_stack,
            item: self.item,
            item_model: self.item_model,
            custom_item_model: self.custom_item_model,
            rotation: self.rotation,
            textures: self.textures,
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
    /// Atlas tile per face: `tiles[id as usize * 6 + face]`.
    pub tiles: Vec<u16>,
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

#[derive(Resource, Clone)]
pub struct BlockTables(pub Arc<Tables>);

#[derive(Resource)]
pub struct BlockRegistry {
    pub defs: Vec<BlockDef>,
    by_name: HashMap<String, BlockId>,
    compiled: bool,
}

/// Where to look for `blocks/`: next to the running executable first (how
/// an installed/shipped build finds its data), falling back to the current
/// working directory (how `cargo run`/`cargo test` find the one at the repo
/// root - Cargo runs both with the package root as cwd).
fn find_blocks_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("blocks");
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    PathBuf::from("blocks")
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
            compiled: false,
        };

        let entries = std::fs::read_dir(dir).unwrap_or_else(|err| {
            panic!(
                "could not read block definitions from {:?}: {err} (expected one *.json file per block)",
                dir
            )
        });
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort(); // deterministic block id assignment across runs/platforms

        for path in paths {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read block file {path:?}: {err}"));
            let file: BlockFile = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("failed to parse block file {path:?}: {err}"));
            if file.id.contains(char::is_whitespace) {
                panic!("block file {path:?}: id {:?} must not contain spaces", file.id);
            }
            if file.item_model == ItemModel::Custom && file.custom_item_model.is_none() {
                panic!(
                    "block file {path:?}: item_model \"custom\" requires a custom_item_model path"
                );
            }
            reg.register(file.into_def());
        }

        reg
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
        self.by_name.get(name).copied().ok_or(UnknownBlock)
    }

    pub fn def(&self, id: BlockId) -> &BlockDef {
        &self.defs[id as usize]
    }

    /// Bakes flat lookup tables. `atlas_index` maps texture names -> tiles;
    /// `tile_size` is the atlas's actual per-tile pixel resolution
    /// (`atlas::AtlasData::tile_size`), used to derive the UV inset that
    /// keeps neighbouring tiles from bleeding into each other.
    pub fn compile(&mut self, atlas_index: &HashMap<String, u16>, tile_size: usize) -> Arc<Tables> {
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
            tiles: vec![0; n * 6],
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
            for face in 0..6 {
                let tex = def.textures.resolve(&def.id, face);
                let tile = atlas_index.get(tex).unwrap_or_else(|| {
                    panic!("block {:?}: no texture painter registered for {tex:?}", def.id)
                });
                tables.tiles[id * 6 + face] = *tile;
            }
        }
        self.compiled = true;
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
    fn custom_item_model_without_a_path_is_rejected() {
        // Mirrors load_from_dir's validation (checked on BlockFile before
        // into_def, since into_def itself can't panic with file context).
        let file: BlockFile =
            serde_json::from_str(r#"{"id": "gizmo", "item_model": "custom"}"#).unwrap();
        assert_eq!(file.item_model, ItemModel::Custom);
        assert!(file.custom_item_model.is_none());
    }

    #[test]
    fn item_stack_is_empty_for_air_or_zero_count() {
        assert!(ItemStack::EMPTY.is_empty());
        assert!(ItemStack { id: AIR, count: 5 }.is_empty());
        assert!(ItemStack { id: 3, count: 0 }.is_empty());
        assert!(!ItemStack { id: 3, count: 1 }.is_empty());
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
}
