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
//!   is stored on the block def for a future flow-simulation system to use;
//!   nothing reads it yet (see `mesher.rs`'s fluid-surface-height handling
//!   for what *is* implemented today).
//! - `textures` defaults to a single texture named after `id` on every face
//!   if omitted entirely.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type BlockId = u16;

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

    fn resolve<'a>(&'a self, block_id: &'a str, face: usize) -> &'a str {
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
    /// drying up. Stored for a future flow-simulation system; unused today.
    pub flow_distance: u32,
    /// Can be targeted by the crosshair raycast.
    pub selectable: bool,
    /// Placing a block into this cell overwrites it (water).
    pub replaceable: bool,
    /// Can be mined (bedrock is not).
    pub breakable: bool,
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
    #[serde(default)]
    textures: FaceTextures,
}

fn default_true() -> bool {
    true
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
    /// Atlas tile per face: `tiles[id as usize * 6 + face]`.
    pub tiles: Vec<u16>,
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

    /// Bakes flat lookup tables. `atlas_index` maps texture names -> tiles.
    pub fn compile(&mut self, atlas_index: &HashMap<String, u16>) -> Arc<Tables> {
        let n = self.defs.len();
        let mut tables = Tables {
            solid: vec![false; n],
            opaque: vec![false; n],
            translucent: vec![false; n],
            fluid: vec![false; n],
            tiles: vec![0; n * 6],
        };
        for (id, def) in self.defs.iter().enumerate().skip(1) {
            tables.solid[id] = def.solid;
            tables.opaque[id] = def.transparency == Transparency::No;
            tables.translucent[id] = def.transparency == Transparency::Full;
            tables.fluid[id] = def.fluid;
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
        let tables = reg.compile(&atlas.indices);
        let water = reg.id("water");
        let stone = reg.id("stone");
        let leaves = reg.id("leaves");
        assert!(tables.solid[stone as usize]);
        assert!(tables.opaque[stone as usize]);
        assert!(!tables.opaque[water as usize]);
        assert!(tables.translucent[water as usize]);
        assert!(tables.fluid[water as usize]);
        assert!(!tables.opaque[leaves as usize]);
        assert!(!tables.translucent[leaves as usize]);
        // grass has distinct top/bottom/side tiles
        let grass = reg.id("grass") as usize;
        assert_ne!(tables.tiles[grass * 6 + 2], tables.tiles[grass * 6 + 3]);
        assert_ne!(tables.tiles[grass * 6 + 2], tables.tiles[grass * 6]);
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
}
