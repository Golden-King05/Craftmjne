//! Everything a player can hold: one id space for items, separate from
//! block ids.
//!
//! Every block with `"item": true` gets an item automatically, under the
//! same id string (so `"dirt"` is both a block and the item that places it).
//! Plain items that aren't blocks - twigs, sticks, later tools and food -
//! are one JSON file each in `items/`:
//!
//! ```json
//! { "id": "stick", "name": "Stick", "max_stack": 124, "texture": "stick" }
//! ```
//!
//! Only `id` is required: `name` defaults to a title-cased `id`,
//! `max_stack` to `blocks::DEFAULT_MAX_STACK`, and `texture` to `id`. The
//! texture is an atlas tile like any block face, so a procedural painter
//! (`atlas.rs`) or a `textures/blocks/<texture>.png` draws it, and anything
//! else gets the checkerboard placeholder. A plain item shows as that flat
//! image in the inventory and as a spinning flat sprite when dropped.
//!
//! Item ids are assigned blocks first (in block order), then plain items in
//! file-name order. Like block ids they're never saved: saves record items
//! by their id string, so the numbering is free to change between versions.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

use crate::blocks::{find_data_dir, BlockId, BlockRegistry, Drops, AIR, DEFAULT_MAX_STACK};

pub type ItemId = u16;

/// The "no item" id - an empty slot.
pub const NO_ITEM: ItemId = 0;

/// A slot's contents: which item and how many. `id == NO_ITEM` (or `count
/// == 0`) means empty — `is_empty()` treats either as the same thing, so
/// callers don't need to keep both fields in sync by hand.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ItemStack {
    pub id: ItemId,
    pub count: u32,
}

impl ItemStack {
    pub const EMPTY: ItemStack = ItemStack { id: NO_ITEM, count: 0 };

    pub fn is_empty(&self) -> bool {
        self.id == NO_ITEM || self.count == 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// Places this block.
    Block(BlockId),
    /// Isn't a block; drawn from this atlas texture.
    Plain { texture: String },
}

#[derive(Clone, Debug)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    pub max_stack: u32,
    pub kind: ItemKind,
}

#[derive(Deserialize)]
struct ItemFile {
    id: String,
    name: Option<String>,
    max_stack: Option<u32>,
    texture: Option<String>,
}

#[derive(Resource)]
pub struct ItemRegistry {
    pub defs: Vec<ItemDef>,
    by_name: HashMap<String, ItemId>,
    /// `of_block[block]` is that block's item, `NO_ITEM` if it has none.
    of_block: Vec<ItemId>,
    /// Atlas tile per item, filled in by `resolve_tiles` once the atlas is
    /// built - meaningful for plain items only.
    tiles: Vec<u16>,
}

impl ItemRegistry {
    /// Block items plus every `items/*.json` file.
    pub fn with_defaults(blocks: &BlockRegistry) -> Self {
        Self::load_from_dir(blocks, &find_data_dir("items"))
    }

    /// Panics, with a clear message, on an unreadable or malformed file, an
    /// item id that's already a block's or another item's, or a block that
    /// drops an item that doesn't exist - this is startup content, the same
    /// as `BlockRegistry::load_from_dir`.
    pub fn load_from_dir(blocks: &BlockRegistry, dir: &Path) -> Self {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("could not read item definitions from {dir:?}: {err}"))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        let plain = paths
            .iter()
            .map(|path| {
                let text = std::fs::read_to_string(path)
                    .unwrap_or_else(|err| panic!("failed to read item file {path:?}: {err}"));
                let file: ItemFile = serde_json::from_str(&text)
                    .unwrap_or_else(|err| panic!("failed to parse item file {path:?}: {err}"));
                ItemDef {
                    name: file.name.unwrap_or_else(|| crate::blocks::title_case(&file.id)),
                    max_stack: file.max_stack.unwrap_or(DEFAULT_MAX_STACK).max(1),
                    kind: ItemKind::Plain { texture: file.texture.unwrap_or_else(|| file.id.clone()) },
                    id: file.id,
                }
            })
            .collect();
        Self::build(blocks, plain)
    }

    pub fn build(blocks: &BlockRegistry, plain: Vec<ItemDef>) -> Self {
        let empty = ItemDef { id: "air".into(), name: "Nothing".into(), max_stack: 1, kind: ItemKind::Block(AIR) };
        let mut reg = Self {
            defs: vec![empty],
            by_name: HashMap::new(),
            of_block: vec![NO_ITEM; blocks.defs.len()],
            tiles: Vec::new(),
        };
        for (block, def) in blocks.defs.iter().enumerate().skip(1) {
            if !def.item {
                continue;
            }
            let id = reg.push(ItemDef {
                id: def.id.clone(),
                name: def.name.clone(),
                max_stack: def.max_stack.max(1),
                kind: ItemKind::Block(block as BlockId),
            });
            reg.of_block[block] = id;
        }
        for def in plain {
            assert!(!def.id.contains(char::is_whitespace), "item id {:?} must not contain spaces", def.id);
            assert!(blocks.by_name(&def.id).is_err(), "item {:?} has the same id as a block", def.id);
            reg.push(def);
        }
        for def in blocks.defs.iter().skip(1) {
            if let Drops::Item { item, .. } = &def.drops {
                assert!(reg.by_name.contains_key(item), "block {:?}: drops unknown item {item:?}", def.id);
            }
        }
        reg.tiles = vec![0; reg.defs.len()];
        reg
    }

    fn push(&mut self, def: ItemDef) -> ItemId {
        assert!(!self.by_name.contains_key(&def.id), "item {:?} already registered", def.id);
        let id = self.defs.len() as ItemId;
        self.by_name.insert(def.id.clone(), id);
        self.defs.push(def);
        id
    }

    pub fn def(&self, id: ItemId) -> &ItemDef {
        &self.defs[id as usize]
    }

    /// Panics on an unknown name - fine for names written in code.
    pub fn id(&self, name: &str) -> ItemId {
        self.by_name(name).unwrap_or_else(|| panic!("unknown item {name:?}"))
    }

    /// For untrusted names (save files, recipes).
    pub fn by_name(&self, name: &str) -> Option<ItemId> {
        self.by_name.get(name).copied()
    }

    pub fn max_stack(&self, id: ItemId) -> u32 {
        self.def(id).max_stack
    }

    /// The item that places `block`, if it has one.
    pub fn of_block(&self, block: BlockId) -> Option<ItemId> {
        self.of_block.get(block as usize).copied().filter(|&id| id != NO_ITEM)
    }

    /// The block `item` places, if it's a block item.
    pub fn block_of(&self, item: ItemId) -> Option<BlockId> {
        match self.def(item).kind {
            ItemKind::Block(block) if item != NO_ITEM => Some(block),
            _ => None,
        }
    }

    /// The texture every plain item needs in the atlas.
    pub fn texture_names(&self) -> impl Iterator<Item = &str> {
        self.defs.iter().filter_map(|d| match &d.kind {
            ItemKind::Plain { texture } => Some(texture.as_str()),
            ItemKind::Block(_) => None,
        })
    }

    /// Looks up each plain item's atlas tile, once the atlas is built.
    pub fn resolve_tiles(&mut self, atlas_index: &HashMap<String, u16>) {
        for (i, def) in self.defs.iter().enumerate() {
            if let ItemKind::Plain { texture } = &def.kind {
                self.tiles[i] = *atlas_index
                    .get(texture)
                    .unwrap_or_else(|| panic!("item {:?}: no texture painter registered for {texture:?}", def.id));
            }
        }
    }

    /// A plain item's atlas tile.
    pub fn tile(&self, item: ItemId) -> u16 {
        self.tiles[item as usize]
    }

    /// What mining `block` gives, given `roll` - a number in `0..1` drawn
    /// fresh for each break, compared against the drop's `chance`.
    pub fn drop_of(&self, blocks: &BlockRegistry, block: BlockId, roll: f32) -> Option<ItemStack> {
        let (id, count) = match &blocks.def(block).drops {
            Drops::Itself => (self.of_block(block)?, 1),
            Drops::Nothing => return None,
            Drops::Item { item, chance, count } => {
                if roll >= *chance {
                    return None;
                }
                (self.by_name(item)?, *count)
            }
        };
        Some(ItemStack { id, count })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::BlockDef;

    fn shipped() -> (BlockRegistry, ItemRegistry) {
        let blocks = BlockRegistry::with_defaults();
        let items = ItemRegistry::with_defaults(&blocks);
        (blocks, items)
    }

    #[test]
    fn every_block_with_an_item_gets_one_under_the_same_name() {
        let (blocks, items) = shipped();
        for (block, def) in blocks.defs.iter().enumerate().skip(1) {
            match items.of_block(block as BlockId) {
                Some(item) => {
                    assert!(def.item);
                    assert_eq!(items.def(item).id, def.id);
                    assert_eq!(items.block_of(item), Some(block as BlockId));
                }
                None => assert!(!def.item, "{} has no item", def.id),
            }
        }
        assert_eq!(items.of_block(AIR), None);
        assert_eq!(items.block_of(NO_ITEM), None);
    }

    #[test]
    fn plain_items_load_and_place_nothing() {
        let (_, items) = shipped();
        for name in ["twig", "stick"] {
            let id = items.id(name);
            assert_eq!(items.block_of(id), None, "{name} would place a block");
            assert!(matches!(items.def(id).kind, ItemKind::Plain { .. }));
        }
    }

    #[test]
    fn shipped_blocks_drop_what_they_should() {
        let (blocks, items) = shipped();
        let drop = |block: &str, roll: f32| items.drop_of(&blocks, blocks.id(block), roll);
        let one = |item: &str| Some(ItemStack { id: items.id(item), count: 1 });
        assert_eq!(drop("stone", 0.5), one("cobblestone"));
        assert_eq!(drop("grass", 0.5), one("dirt"));
        assert_eq!(drop("dirt", 0.5), one("dirt"));
        assert_eq!(drop("log", 0.5), one("log"));
        assert_eq!(drop("ice", 0.0), None);
        assert_eq!(drop("leaves", 0.0), one("twig"), "a lucky roll");
        assert_eq!(drop("leaves", 0.99), None, "leaves only drop twigs sometimes");
        assert!(!blocks.def(blocks.id("bedrock")).breakable);
    }

    #[test]
    #[should_panic(expected = "drops unknown item")]
    fn a_drop_naming_an_unknown_item_fails_at_startup() {
        let mut blocks = BlockRegistry::with_defaults();
        blocks.register(BlockDef {
            id: "ruby".into(),
            drops: Drops::Item { item: "rubyy".into(), chance: 1.0, count: 1 },
            ..BlockDef::default()
        });
        ItemRegistry::build(&blocks, Vec::new());
    }

    #[test]
    #[should_panic(expected = "same id as a block")]
    fn a_plain_item_cant_reuse_a_block_id() {
        let blocks = BlockRegistry::with_defaults();
        let def = ItemDef {
            id: "dirt".into(),
            name: "Dirt".into(),
            max_stack: 1,
            kind: ItemKind::Plain { texture: "dirt".into() },
        };
        ItemRegistry::build(&blocks, vec![def]);
    }
}
