//! Crafting: ingredient lists, stations and discovery.
//!
//! There's no grid. A recipe is a list of ingredients and an output, and the
//! inventory screen lists every recipe you can see, greyed out when you're
//! missing something; click to craft one, shift-click to craft as many as
//! you can. Recipes are one JSON file each in `recipes/`:
//!
//! ```json
//! {
//!   "station": "hand",
//!   "inputs": [{ "item": "planks", "count": 3 }, { "item": "stick", "count": 4 }],
//!   "output": { "item": "crafting_bench", "count": 1 }
//! }
//! ```
//!
//! - **Stations.** `"station"` is `"hand"` (the default: craftable
//!   anywhere) or a block id. A station recipe shows while that block is
//!   within [`STATION_REACH`] of you, and right-clicking a station block
//!   opens the inventory screen to craft at it (shift + right-click places
//!   a block against it instead). Any block named as a station by some
//!   recipe is one - no flag on the block itself.
//! - **Discovery.** A recipe only shows once you've held at least one of
//!   its ingredients ([`KnownItems`], saved with the world). Pick up a log
//!   and the planks recipe appears; you learn recipes by finding things,
//!   not from a wiki.
//!
//! Ingredients come out of storage before the hotbar (so what you've got in
//! hand is the last thing used up), and the output goes in like a pickup
//! (`items::give`); whatever doesn't fit is dropped at your feet.

use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

use crate::blocks::{find_data_dir, BlockId, BlockRegistry};
use crate::interact::Hotbar;
use crate::inventory::{HeldStack, Inventory};
use crate::item_registry::{ItemId, ItemRegistry, ItemStack};
use crate::items::give;
use crate::world::ChunkMap;

/// How far (in blocks, each axis) a station can be from your eyes and still
/// count as "around" for crafting.
pub const STATION_REACH: i32 = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Station {
    Hand,
    Block(BlockId),
}

#[derive(Clone, Debug)]
pub struct Recipe {
    /// The recipe file's name, without `.json`.
    pub id: String,
    pub station: Station,
    pub inputs: Vec<ItemStack>,
    pub output: ItemStack,
}

#[derive(Deserialize)]
struct StackFile {
    item: String,
    #[serde(default = "one")]
    count: u32,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
struct RecipeFile {
    #[serde(default)]
    station: Option<String>,
    inputs: Vec<StackFile>,
    output: StackFile,
}

#[derive(Resource, Default)]
pub struct RecipeBook {
    pub recipes: Vec<Recipe>,
    stations: HashSet<BlockId>,
}

impl RecipeBook {
    pub fn with_defaults(blocks: &BlockRegistry, items: &ItemRegistry) -> Self {
        Self::load_from_dir(blocks, items, &find_data_dir("recipes"))
    }

    /// Panics, with the file named, on anything that doesn't resolve - an
    /// unknown item or station, no inputs, a zero count. Startup content,
    /// like blocks and items.
    pub fn load_from_dir(blocks: &BlockRegistry, items: &ItemRegistry, dir: &Path) -> Self {
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("could not read recipes from {dir:?}: {err}"))
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        let mut book = Self::default();
        for path in paths {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read recipe {path:?}: {err}"));
            let file: RecipeFile =
                serde_json::from_str(&text).unwrap_or_else(|err| panic!("failed to parse recipe {path:?}: {err}"));
            let stack = |s: &StackFile| {
                let id = items.by_name(&s.item).unwrap_or_else(|| panic!("recipe {path:?}: unknown item {:?}", s.item));
                assert!(s.count > 0, "recipe {path:?}: {:?} has a count of zero", s.item);
                ItemStack { id, count: s.count }
            };
            let station = match file.station.as_deref() {
                None | Some("hand") => Station::Hand,
                Some(name) => Station::Block(
                    blocks.by_name(name).unwrap_or_else(|_| panic!("recipe {path:?}: unknown station block {name:?}")),
                ),
            };
            assert!(!file.inputs.is_empty(), "recipe {path:?} has no inputs");
            let recipe = Recipe {
                id: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                station,
                inputs: file.inputs.iter().map(stack).collect(),
                output: stack(&file.output),
            };
            book.add(recipe);
        }
        book
    }

    pub fn add(&mut self, recipe: Recipe) {
        if let Station::Block(block) = recipe.station {
            self.stations.insert(block);
        }
        self.recipes.push(recipe);
    }

    pub fn is_station(&self, block: BlockId) -> bool {
        self.stations.contains(&block)
    }

    /// Recipes usable at the given stations (hand recipes always are) whose
    /// ingredients you've come across, in file order.
    pub fn visible<'a>(
        &'a self,
        stations: &'a HashSet<BlockId>,
        known: &'a HashSet<ItemId>,
    ) -> impl Iterator<Item = (usize, &'a Recipe)> + 'a {
        self.recipes.iter().enumerate().filter(move |(_, r)| {
            let at_station = match r.station {
                Station::Hand => true,
                Station::Block(block) => stations.contains(&block),
            };
            at_station && r.inputs.iter().any(|i| known.contains(&i.id))
        })
    }
}

/// How many of `item` are in these slots altogether.
pub fn count_of<'a>(slots: impl IntoIterator<Item = &'a ItemStack>, item: ItemId) -> u32 {
    slots.into_iter().filter(|s| !s.is_empty() && s.id == item).map(|s| s.count).sum()
}

/// How many times `recipe` could be crafted from these slots right now.
pub fn times_craftable(recipe: &Recipe, hotbar: &[ItemStack], storage: &[ItemStack]) -> u32 {
    recipe
        .inputs
        .iter()
        .map(|input| count_of(hotbar.iter().chain(storage), input.id) / input.count)
        .min()
        .unwrap_or(0)
}

/// Crafts `recipe` once: takes the ingredients (storage first, then the
/// hotbar, each from its last slot backwards) and adds the output. Returns
/// how many of the output didn't fit (for the caller to drop), or `None`
/// without touching anything if an ingredient is short.
pub fn craft(recipe: &Recipe, hotbar: &mut [ItemStack], storage: &mut [ItemStack], max_stack: u32) -> Option<u32> {
    if times_craftable(recipe, hotbar, storage) == 0 {
        return None;
    }
    for input in &recipe.inputs {
        let mut need = input.count;
        for slot in storage.iter_mut().rev().chain(hotbar.iter_mut().rev()) {
            if need == 0 {
                break;
            }
            if slot.is_empty() || slot.id != input.id {
                continue;
            }
            let taken = need.min(slot.count);
            slot.count -= taken;
            need -= taken;
            if slot.count == 0 {
                *slot = ItemStack::EMPTY;
            }
        }
    }
    Some(give(hotbar, storage, recipe.output, max_stack))
}

/// Every item the player has ever held in this world - what decides which
/// recipes they can see. Saved with the world (`save::WorldData::known_items`).
#[derive(Resource, Default)]
pub struct KnownItems(pub HashSet<ItemId>);

/// The crafting stations within `STATION_REACH` of the player, kept up to
/// date while the inventory screen is open.
#[derive(Resource, Default, PartialEq)]
pub struct NearbyStations(pub HashSet<BlockId>);

/// Station blocks in a cube of `STATION_REACH` around `center`.
pub fn stations_near(map: &ChunkMap, book: &RecipeBook, center: IVec3) -> HashSet<BlockId> {
    let mut found = HashSet::new();
    let r = STATION_REACH;
    for dy in -r..=r {
        for dz in -r..=r {
            for dx in -r..=r {
                let block = map.get_block(center + IVec3::new(dx, dy, dz));
                if book.is_station(block) {
                    found.insert(block);
                }
            }
        }
    }
    found
}

/// Adds anything newly in the hotbar, storage or cursor to `KnownItems`.
pub fn learn_items(
    hotbar: Res<Hotbar>,
    inventory: Res<Inventory>,
    held: Res<HeldStack>,
    mut known: ResMut<KnownItems>,
) {
    if !hotbar.is_changed() && !inventory.is_changed() && !held.is_changed() {
        return;
    }
    let new: Vec<ItemId> = hotbar
        .slots
        .iter()
        .chain(&inventory.slots)
        .chain(std::iter::once(&held.0))
        .filter(|s| !s.is_empty() && !known.0.contains(&s.id))
        .map(|s| s.id)
        .collect();
    if !new.is_empty() {
        known.0.extend(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped() -> (BlockRegistry, ItemRegistry, RecipeBook) {
        let blocks = BlockRegistry::with_defaults();
        let items = ItemRegistry::with_defaults(&blocks);
        let book = RecipeBook::with_defaults(&blocks, &items);
        (blocks, items, book)
    }

    fn recipe<'a>(book: &'a RecipeBook, items: &ItemRegistry, output: &str) -> &'a Recipe {
        let id = items.id(output);
        book.recipes.iter().find(|r| r.output.id == id).unwrap_or_else(|| panic!("no recipe makes {output}"))
    }

    #[test]
    fn the_first_recipes_are_the_ones_asked_for() {
        let (blocks, items, book) = shipped();
        let s = |name: &str, count| ItemStack { id: items.id(name), count };
        let planks = recipe(&book, &items, "planks");
        assert_eq!((planks.inputs.clone(), planks.output), (vec![s("log", 1)], s("planks", 4)));
        let bench = recipe(&book, &items, "crafting_bench");
        assert_eq!((bench.inputs.clone(), bench.output), (vec![s("planks", 3), s("stick", 4)], s("crafting_bench", 1)));
        let sticks: Vec<_> =
            book.recipes.iter().filter(|r| r.output.id == items.id("stick")).map(|r| (r.inputs.clone(), r.output)).collect();
        assert!(sticks.contains(&(vec![s("twig", 1)], s("stick", 1))), "{sticks:?}");
        assert!(sticks.contains(&(vec![s("planks", 2)], s("stick", 5))), "{sticks:?}");
        assert!(book.recipes.iter().all(|r| r.station == Station::Hand), "all of these are by hand");
        assert_eq!(items.block_of(bench.output.id), Some(blocks.id("crafting_bench")));
    }

    #[test]
    fn from_a_log_to_a_crafting_bench() {
        let (_, items, book) = shipped();
        let max = 124;
        let mut hotbar = vec![ItemStack::EMPTY; 9];
        let mut storage = vec![ItemStack::EMPTY; 27];
        hotbar[0] = ItemStack { id: items.id("log"), count: 2 };
        let planks = recipe(&book, &items, "planks");
        let bench = recipe(&book, &items, "crafting_bench");
        let sticks_from_planks = book
            .recipes
            .iter()
            .find(|r| r.output.id == items.id("stick") && r.inputs[0].id == items.id("planks"))
            .unwrap();

        assert_eq!(times_craftable(bench, &hotbar, &storage), 0);
        assert_eq!(craft(bench, &mut hotbar, &mut storage, max), None, "no planks or sticks yet");
        assert_eq!(craft(planks, &mut hotbar, &mut storage, max), Some(0));
        assert_eq!(craft(planks, &mut hotbar, &mut storage, max), Some(0));
        assert_eq!(count_of(hotbar.iter().chain(&storage), items.id("planks")), 8);
        assert_eq!(count_of(hotbar.iter().chain(&storage), items.id("log")), 0);
        assert_eq!(craft(sticks_from_planks, &mut hotbar, &mut storage, max), Some(0));
        assert_eq!(times_craftable(bench, &hotbar, &storage), 1);
        assert_eq!(craft(bench, &mut hotbar, &mut storage, max), Some(0));
        let all = || hotbar.iter().chain(&storage);
        assert_eq!(count_of(all(), items.id("crafting_bench")), 1);
        assert_eq!(count_of(all(), items.id("planks")), 3, "8 - 2 for sticks - 3 for the bench");
        assert_eq!(count_of(all(), items.id("stick")), 1, "5 made, 4 used");
    }

    #[test]
    fn ingredients_come_out_of_storage_before_the_hotbar() {
        let (_, items, book) = shipped();
        let log = items.id("log");
        let mut hotbar = vec![ItemStack { id: log, count: 5 }];
        let mut storage = vec![ItemStack { id: log, count: 1 }];
        craft(recipe(&book, &items, "planks"), &mut hotbar, &mut storage, 124);
        assert!(storage[0].id != log || storage[0].is_empty(), "storage's log wasn't used: {storage:?}");
        assert_eq!(hotbar[0], ItemStack { id: log, count: 5 }, "the hotbar's logs were touched");
    }

    #[test]
    fn output_that_does_not_fit_is_reported_for_dropping() {
        let (_, items, book) = shipped();
        let dirt = items.id("dirt");
        let mut hotbar = vec![ItemStack { id: items.id("log"), count: 1 }];
        let mut storage = vec![ItemStack { id: dirt, count: 124 }];
        // The log's own slot frees up, so 4 planks fit there.
        assert_eq!(craft(recipe(&book, &items, "planks"), &mut hotbar, &mut storage, 2), Some(2));
        assert_eq!(hotbar[0], ItemStack { id: items.id("planks"), count: 2 });
    }

    #[test]
    fn a_recipe_shows_once_you_know_an_ingredient_and_are_at_its_station() {
        let (blocks, items, mut book) = shipped();
        let bench = blocks.id("crafting_bench");
        book.add(Recipe {
            id: "test_bench_only".into(),
            station: Station::Block(bench),
            inputs: vec![ItemStack { id: items.id("stick"), count: 1 }],
            output: ItemStack { id: items.id("dirt"), count: 1 },
        });
        let ids = |stations: &HashSet<BlockId>, known: &HashSet<ItemId>| {
            book.visible(stations, known).map(|(_, r)| r.id.clone()).collect::<Vec<_>>()
        };
        let none = HashSet::new();
        assert!(ids(&none, &none).is_empty(), "nothing known, nothing shown");
        let knows_log = HashSet::from([items.id("log")]);
        assert_eq!(ids(&none, &knows_log), vec!["planks".to_string()]);
        let knows_stick = HashSet::from([items.id("stick")]);
        assert!(!ids(&none, &knows_stick).contains(&"test_bench_only".to_string()), "away from a bench");
        assert!(ids(&HashSet::from([bench]), &knows_stick).contains(&"test_bench_only".to_string()));
        assert!(book.is_station(bench));
        assert!(!book.is_station(blocks.id("dirt")));
    }

    #[test]
    fn stations_are_found_around_the_player_and_not_beyond_reach() {
        let (blocks, _, mut book) = shipped();
        let bench = blocks.id("crafting_bench");
        book.add(Recipe {
            id: "t".into(),
            station: Station::Block(bench),
            inputs: vec![ItemStack { id: 1, count: 1 }],
            output: ItemStack { id: 1, count: 1 },
        });
        let mut map = ChunkMap::default();
        map.chunks.insert(
            IVec2::ZERO,
            crate::world::Chunk {
                blocks: Some(vec![0; crate::config::CS * crate::config::CS * crate::config::H]),
                ..Default::default()
            },
        );
        let at = IVec3::new(8, 40, 8);
        map.set_block(at + IVec3::new(STATION_REACH, 0, 0), bench);
        assert!(stations_near(&map, &book, at).contains(&bench));
        map.set_block(at + IVec3::new(STATION_REACH, 0, 0), 0);
        map.set_block(at + IVec3::new(STATION_REACH + 1, 0, 0), bench);
        assert!(stations_near(&map, &book, at).is_empty());
    }
}
