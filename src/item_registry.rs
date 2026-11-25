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
use crate::tools::{HeldTool, Tier, TierBook, ToolKind, ToolSpec, DEFAULT_TOOL_QUALITY, MAX_QUALITY};

pub type ItemId = u16;

/// The "no item" id - an empty slot.
pub const NO_ITEM: ItemId = 0;

/// A slot's contents: which item and how many. `id == NO_ITEM` (or `count
/// == 0`) means empty — `is_empty()` treats either as the same thing, so
/// callers don't need to keep both fields in sync by hand.
///
/// `quality` (a whole percent, see `tools.rs`) is per-stack data that only
/// means something for tools, which never stack; it's `0` for everything
/// else. Anything that splits, moves or copies a stack has to carry it along
/// - `ItemStack { count, ..stack }` rather than rebuilding from `id` - and
/// anything that merges two stacks has to check [`ItemStack::same_item`].
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ItemStack {
    pub id: ItemId,
    pub count: u32,
    pub quality: u8,
}

impl ItemStack {
    pub const EMPTY: ItemStack = ItemStack { id: NO_ITEM, count: 0, quality: 0 };

    /// A stack with no quality (everything that isn't a tool).
    pub const fn new(id: ItemId, count: u32) -> Self {
        Self { id, count, quality: 0 }
    }

    pub const fn with_quality(self, quality: u8) -> Self {
        Self { quality, ..self }
    }

    /// Whether two stacks are the same thing, so they may merge: the same
    /// item *and* the same quality.
    pub fn same_item(&self, other: &ItemStack) -> bool {
        self.id == other.id && self.quality == other.quality
    }

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
    /// `Some` for a tool: its kind and which tier's stats it uses.
    pub tool: Option<ToolSpec>,
    /// Kilograms, for throwing.
    pub mass: f32,
    /// Left click throws it.
    pub throwable: bool,
}

#[derive(Deserialize)]
struct ToolFile {
    kind: ToolKind,
    tier: String,
}

#[derive(Deserialize)]
struct ItemFile {
    id: String,
    name: Option<String>,
    max_stack: Option<u32>,
    texture: Option<String>,
    tool: Option<ToolFile>,
    mass: Option<f32>,
    #[serde(default)]
    throwable: bool,
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
    /// The tiers tool items name. Empty until `with_tiers`.
    pub tiers: TierBook,
}

impl ItemRegistry {
    /// Block items, every `items/*.json` file, and the `tiers/` they name.
    pub fn with_defaults(blocks: &BlockRegistry) -> Self {
        Self::load_from_dir(blocks, &find_data_dir("items")).with_tiers(TierBook::with_defaults())
    }

    /// Attaches the tier list, panicking with a clear message if a tool item
    /// names a tier that doesn't exist - startup content, like a missing
    /// texture or an unknown drop.
    pub fn with_tiers(mut self, tiers: TierBook) -> Self {
        for def in &self.defs {
            if let Some(tool) = &def.tool {
                assert!(
                    tiers.get(&tool.tier).is_some(),
                    "item {:?}: tool names unknown tier {:?}",
                    def.id,
                    tool.tier
                );
            }
        }
        self.tiers = tiers;
        self
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
                let tool = file.tool.map(|t| ToolSpec { kind: t.kind, tier: t.tier });
                ItemDef {
                    name: file.name.unwrap_or_else(|| crate::blocks::title_case(&file.id)),
                    // Quality is per-stack, so a tool can't share a slot.
                    max_stack: if tool.is_some() { 1 } else { file.max_stack.unwrap_or(DEFAULT_MAX_STACK).max(1) },
                    kind: ItemKind::Plain { texture: file.texture.unwrap_or_else(|| file.id.clone()) },
                    tool,
                    mass: file.mass.filter(|m| *m > 0.0).unwrap_or(crate::blocks::DEFAULT_MASS),
                    throwable: file.throwable,
                    id: file.id,
                }
            })
            .collect();
        Self::build(blocks, plain)
    }

    pub fn build(blocks: &BlockRegistry, plain: Vec<ItemDef>) -> Self {
        let empty = ItemDef { id: "air".into(), name: "Nothing".into(), max_stack: 1, kind: ItemKind::Block(AIR), tool: None, mass: 0.0, throwable: false };
        let mut reg = Self {
            defs: vec![empty],
            by_name: HashMap::new(),
            of_block: vec![NO_ITEM; blocks.defs.len()],
            tiles: Vec::new(),
            tiers: TierBook::default(),
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
                tool: None,
                mass: def.mass,
                throwable: def.throwable,
            });
            reg.of_block[block] = id;
        }
        // A `pebble@water` is still a pebble: breaking it drops (and
        // picking it gives) the pebble's item.
        for (block, def) in blocks.defs.iter().enumerate() {
            if let Some(l) = def.layered {
                reg.of_block[block] = reg.of_block[l.block as usize];
            }
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

    /// `Some` if `id` is a tool.
    pub fn tool(&self, id: ItemId) -> Option<&ToolSpec> {
        self.def(id).tool.as_ref()
    }

    /// The tier a tool item belongs to.
    pub fn tier_of(&self, id: ItemId) -> Option<&Tier> {
        self.tiers.get(&self.tool(id)?.tier)
    }

    /// What mining sees of a stack in hand: `Some` for a tool, with its
    /// speed at this stack's quality; `None` for a bare hand or anything
    /// else.
    pub fn held_tool(&self, stack: &ItemStack) -> Option<HeldTool> {
        if stack.is_empty() {
            return None;
        }
        let spec = self.tool(stack.id)?;
        let tier = self.tiers.get(&spec.tier)?;
        Some(HeldTool { kind: spec.kind, speed: tier.mining_speed_at(stack.quality) })
    }

    /// How much damage a hit with `stack` in hand does: the tool's tier
    /// damage at its quality, or [`HAND_DAMAGE`] for a bare hand or anything
    /// that isn't a tool.
    pub fn attack_damage(&self, stack: &ItemStack) -> f32 {
        if stack.is_empty() {
            return crate::tools::HAND_DAMAGE;
        }
        self.tool(stack.id)
            .and_then(|spec| self.tiers.get(&spec.tier))
            .map_or(crate::tools::HAND_DAMAGE, |tier| tier.damage_at(stack.quality))
    }

    /// Whether left click throws `stack` (it isn't empty and is throwable).
    pub fn throwable(&self, stack: &ItemStack) -> bool {
        !stack.is_empty() && self.def(stack.id).throwable
    }

    pub fn mass(&self, id: ItemId) -> f32 {
        self.def(id).mass
    }

    /// A fresh stack of `count` of `id`: a tool starts at the default
    /// quality (`tools::DEFAULT_TOOL_QUALITY`), anything else has none.
    /// For items that appear without being made - Creative's list.
    pub fn stack(&self, id: ItemId, count: u32) -> ItemStack {
        let quality = if self.tool(id).is_some() { DEFAULT_TOOL_QUALITY } else { 0 };
        ItemStack { id, count, quality }
    }

    /// A stack read back from a save (or any untrusted source): the count
    /// is clamped to what one slot holds, and quality is clamped to a
    /// percent and dropped entirely for anything that isn't a tool, so a
    /// hand-edited save can't put an impossible stack in a slot.
    pub fn restore(&self, id: ItemId, count: u32, quality: u8) -> ItemStack {
        let quality = if self.tool(id).is_some() { quality.min(MAX_QUALITY) } else { 0 };
        ItemStack { id, count: count.min(self.max_stack(id)), quality }
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
        Some(ItemStack::new(id, count))
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
            if def.layered.is_some() {
                continue; // `pebble@water` shares the pebble's item; covered below
            }
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
        let (blocks, items) = shipped();
        let stick = items.id("stick");
        assert_eq!(items.block_of(stick), None, "a stick would place a block");
        assert!(matches!(items.def(stick).kind, ItemKind::Plain { .. }));
        // Twigs and pebbles are blocks lying on the ground, so their items
        // put them back down.
        for name in ["twig", "pebble"] {
            assert_eq!(items.block_of(items.id(name)), Some(blocks.id(name)));
        }
    }

    #[test]
    fn shipped_blocks_drop_what_they_should() {
        let (blocks, items) = shipped();
        let drop = |block: &str, roll: f32| items.drop_of(&blocks, blocks.id(block), roll);
        let one = |item: &str| Some(ItemStack::new(items.id(item), 1));
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
            tool: None,
            mass: 1.0,
            throwable: false,
        };
        ItemRegistry::build(&blocks, vec![def]);
    }

    #[test]
    fn tool_items_load_with_a_kind_and_tier_and_never_stack() {
        let (_blocks, items) = shipped();
        for (name, kind) in [
            ("stone_axe", ToolKind::Axe),
            ("stone_pickaxe", ToolKind::Pickaxe),
            ("stone_shovel", ToolKind::Shovel),
            ("stone_hoe", ToolKind::Hoe),
            ("stone_dagger", ToolKind::Dagger),
            ("stone_longsword", ToolKind::Longsword),
            ("stone_greatsword", ToolKind::Greatsword),
        ] {
            let id = items.id(name);
            assert_eq!(items.tool(id).map(|t| t.kind), Some(kind), "{name}");
            assert_eq!(items.tier_of(id).map(|t| t.id.as_str()), Some("stone"));
            assert_eq!(items.max_stack(id), 1, "{name}: quality is per-stack, so a tool can't share a slot");
        }
        assert!(items.tool(items.id("stick")).is_none());
        assert!(items.tier_of(items.id("stick")).is_none());
        assert_eq!(items.max_stack(items.id("stick")), DEFAULT_MAX_STACK, "plain items still stack");
    }

    #[test]
    fn a_fresh_tool_starts_at_the_default_quality_and_anything_else_has_none() {
        let (_blocks, items) = shipped();
        assert_eq!(items.stack(items.id("stone_pickaxe"), 1).quality, DEFAULT_TOOL_QUALITY);
        assert_eq!(items.stack(items.id("stick"), 7), ItemStack::new(items.id("stick"), 7));
        assert_eq!(items.stack(items.id("dirt"), 1).quality, 0);
    }

    #[test]
    fn restoring_a_stack_clamps_quality_and_drops_it_for_non_tools() {
        let (_blocks, items) = shipped();
        let pick = items.id("stone_pickaxe");
        assert_eq!(items.restore(pick, 1, 83).quality, 83);
        assert_eq!(items.restore(pick, 1, 250).quality, MAX_QUALITY, "a hand-edited quality is clamped");
        assert_eq!(items.restore(pick, 40, 10).count, 1, "and a tool can't be restored as a stack");
        assert_eq!(items.restore(items.id("dirt"), 5, 90).quality, 0, "dirt has no quality");
    }

    #[test]
    fn two_stacks_of_one_tool_at_different_qualities_are_not_the_same_item() {
        let (_blocks, items) = shipped();
        let pick = items.id("stone_pickaxe");
        let (a, b) = (ItemStack::new(pick, 1).with_quality(80), ItemStack::new(pick, 1).with_quality(81));
        assert!(a.same_item(&a));
        assert!(!a.same_item(&b));
        assert_ne!(a, b);
    }

    #[test]
    #[should_panic(expected = "unknown tier")]
    fn a_tool_naming_an_unknown_tier_fails_at_startup() {
        let blocks = BlockRegistry::with_defaults();
        let def = ItemDef {
            id: "mithril_pick".into(),
            name: "Mithril Pick".into(),
            max_stack: 1,
            kind: ItemKind::Plain { texture: "stick".into() },
            tool: Some(ToolSpec { kind: ToolKind::Pickaxe, tier: "mithril".into() }),
            mass: 1.0,
            throwable: false,
        };
        ItemRegistry::build(&blocks, vec![def]).with_tiers(TierBook::with_defaults());
    }

    #[test]
    fn a_hit_does_the_tiers_damage_at_the_tools_quality_and_a_hand_does_one() {
        let (_blocks, items) = shipped();
        let axe = items.id("stone_axe");
        let stone = items.tiers.get("stone").unwrap().clone();
        let at = |q| items.attack_damage(&ItemStack { id: axe, count: 1, quality: q });
        assert_eq!(at(0), stone.damage_at(0));
        assert_eq!(at(100), stone.damage_at(100));
        assert!(at(100) > at(0), "quality hits harder");
        assert_eq!(items.attack_damage(&ItemStack::EMPTY), crate::tools::HAND_DAMAGE);
        assert_eq!(items.attack_damage(&ItemStack::new(items.id("stick"), 3)), crate::tools::HAND_DAMAGE);
    }

    #[test]
    fn a_pebble_is_a_light_throwable_that_stacks_and_most_things_are_not_throwable() {
        let (_blocks, items) = shipped();
        let pebble = items.id("pebble");
        assert!(items.throwable(&ItemStack::new(pebble, 1)));
        assert_eq!(items.mass(pebble), 0.05);
        assert!(items.max_stack(pebble) > 1, "pebbles stack");
        assert!(!items.throwable(&ItemStack::new(items.id("stick"), 1)));
        assert!(!items.throwable(&ItemStack::new(items.id("stone_axe"), 1)));
        assert!(!items.throwable(&ItemStack::EMPTY));
        assert_eq!(items.mass(items.id("stick")), crate::blocks::DEFAULT_MASS);
    }

    #[test]
    fn the_pebble_is_the_only_throwable_thing() {
        let (blocks, items) = shipped();
        let throwable: Vec<&str> = items.defs.iter().skip(1).filter(|d| d.throwable).map(|d| d.id.as_str()).collect();
        assert_eq!(throwable, vec!["pebble"]);
        assert_eq!(blocks.defs.iter().filter(|d| d.throwable && d.layered.is_none()).map(|d| d.id.as_str()).collect::<Vec<_>>(), vec!["pebble"]);
    }
}
