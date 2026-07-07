//! Block registry — the central extension point for adding content.
//!
//! Register blocks from your own plugin's `build()` (before startup):
//! ```ignore
//! app.world_mut().resource_mut::<BlockRegistry>().register(BlockDef {
//!     name: "ruby_block".into(),
//!     textures: FaceTextures::all("ruby"),
//!     ..BlockDef::default()
//! });
//! ```
//!
//! At startup the registry is compiled into flat lookup [`Tables`] — these are
//! what the mesher, physics and worldgen tasks use, so hot loops never touch
//! the def structs.

use bevy::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;

pub type BlockId = u16;
pub const AIR: BlockId = 0;

/// Face order used across the whole engine (mesher, tiles table):
/// 0:+x east, 1:-x west, 2:+y top, 3:-y bottom, 4:+z south, 5:-z north
#[derive(Clone, Default)]
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

    fn resolve<'a>(&'a self, block_name: &'a str, face: usize) -> &'a str {
        let pick = |primary: &'a Option<String>| {
            primary
                .as_deref()
                .or(self.all.as_deref())
                .unwrap_or(block_name)
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
    pub name: String,
    /// Collides with entities.
    pub solid: bool,
    /// Does not occlude neighbouring faces (glass, leaves, water).
    pub transparent: bool,
    /// Rendered in the alpha-blended water pass.
    pub translucent: bool,
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
            name: String::new(),
            solid: true,
            transparent: false,
            translucent: false,
            selectable: true,
            replaceable: false,
            breakable: true,
            textures: FaceTextures::default(),
        }
    }
}

/// Flat lookup tables baked from the registry; shared with worldgen/mesh tasks.
pub struct Tables {
    pub solid: Vec<bool>,
    pub opaque: Vec<bool>,
    pub translucent: Vec<bool>,
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

impl BlockRegistry {
    pub fn with_defaults() -> Self {
        let air = BlockDef {
            name: "air".into(),
            solid: false,
            transparent: true,
            selectable: false,
            replaceable: true,
            ..BlockDef::default()
        };
        let mut reg = Self {
            defs: vec![air],
            by_name: HashMap::from([("air".into(), 0)]),
            compiled: false,
        };
        register_default_blocks(&mut reg);
        reg
    }

    pub fn register(&mut self, def: BlockDef) -> BlockId {
        assert!(!self.compiled, "cannot register blocks after startup");
        assert!(!def.name.is_empty(), "block def requires a name");
        assert!(
            !self.by_name.contains_key(&def.name),
            "block {:?} already registered",
            def.name
        );
        let id = self.defs.len() as BlockId;
        self.by_name.insert(def.name.clone(), id);
        self.defs.push(def);
        id
    }

    pub fn id(&self, name: &str) -> BlockId {
        *self
            .by_name
            .get(name)
            .unwrap_or_else(|| panic!("unknown block {name:?}"))
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
            tiles: vec![0; n * 6],
        };
        for (id, def) in self.defs.iter().enumerate().skip(1) {
            tables.solid[id] = def.solid;
            tables.opaque[id] = !def.transparent && !def.translucent;
            tables.translucent[id] = def.translucent;
            for face in 0..6 {
                let tex = def.textures.resolve(&def.name, face);
                let tile = atlas_index.get(tex).unwrap_or_else(|| {
                    panic!("block {:?}: no texture painter registered for {tex:?}", def.name)
                });
                tables.tiles[id * 6 + face] = *tile;
            }
        }
        self.compiled = true;
        Arc::new(tables)
    }
}

fn register_default_blocks(reg: &mut BlockRegistry) {
    let simple = |name: &str| BlockDef { name: name.into(), ..BlockDef::default() };

    reg.register(simple("stone"));
    reg.register(simple("dirt"));
    reg.register(BlockDef {
        name: "grass".into(),
        textures: FaceTextures::tbs("grass_top", "dirt", "grass_side"),
        ..BlockDef::default()
    });
    reg.register(simple("sand"));
    reg.register(simple("gravel"));
    reg.register(BlockDef {
        name: "water".into(),
        solid: false,
        transparent: true,
        translucent: true,
        selectable: false,
        replaceable: true,
        ..BlockDef::default()
    });
    reg.register(BlockDef {
        name: "log".into(),
        textures: FaceTextures::tbs("log_top", "log_top", "log_side"),
        ..BlockDef::default()
    });
    reg.register(BlockDef {
        name: "leaves".into(),
        transparent: true,
        ..BlockDef::default()
    });
    reg.register(simple("planks"));
    reg.register(simple("cobblestone"));
    reg.register(BlockDef {
        name: "glass".into(),
        transparent: true,
        ..BlockDef::default()
    });
    reg.register(BlockDef {
        name: "bedrock".into(),
        breakable: false,
        ..BlockDef::default()
    });
    reg.register(simple("snow"));
    reg.register(simple("bricks"));
    reg.register(simple("coal_ore"));
    reg.register(simple("iron_ore"));
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
        assert!(!tables.opaque[leaves as usize]);
        assert!(!tables.translucent[leaves as usize]);
        // grass has distinct top/bottom/side tiles
        let grass = reg.id("grass") as usize;
        assert_ne!(tables.tiles[grass * 6 + 2], tables.tiles[grass * 6 + 3]);
        assert_ne!(tables.tiles[grass * 6 + 2], tables.tiles[grass * 6]);
    }
}
