//! **The shelf**: what a save holds that the game can't currently read, kept so
//! turning a mod off never destroys the world's data.
//!
//! A world's save names things: `granite_pack:granite` placed at some position, a
//! `granite_pack:chip` in the inventory. If the mod that defines them is off, those
//! names don't resolve, and the old behaviour was to skip them on load - which
//! meant the *next save* no longer contained them. Turning a mod off would have
//! silently deleted everything made with it, and turning it back on would have
//! brought back nothing.
//!
//! [`split_unknown`] takes the entries whose names don't resolve out of the loaded
//! [`WorldData`] before anything reads it, and [`merge_into`] writes them back into
//! every save. They sit on the shelf untouched until the mod returns, at which point
//! they just load like any other entry. Two rules keep it honest:
//!
//! - **What's live wins.** A shelved edit at a position the player has since built
//!   on is dropped (the new block is the world's truth now), and a shelved fluid at
//!   a position that has live fluid likewise.
//! - **Items come back to the player.** Inventory stacks can't sit in a slot that
//!   something else may now occupy, so they're shelved as a list
//!   (`WorldData::shelved_items`) and, once their mod is back, handed to the player
//!   through the normal "give" path when the world loads.

use bevy::prelude::Resource;

use crate::blocks::BlockRegistry;
use crate::item_registry::ItemRegistry;
use crate::save::{BlockEdit, DroppedItemSave, FluidCell, SlotSave, WorldData};

/// The entries of a save that don't resolve right now.
#[derive(Resource, Default, Clone)]
pub struct Shelf {
    pub edits: Vec<BlockEdit>,
    pub fluids: Vec<FluidCell>,
    pub drops: Vec<DroppedItemSave>,
    pub known: Vec<String>,
    /// Inventory stacks (hotbar, storage, cursor) of items that don't exist now.
    pub items: Vec<SlotSave>,
}

impl Shelf {
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
            && self.fluids.is_empty()
            && self.drops.is_empty()
            && self.known.is_empty()
            && self.items.is_empty()
    }
}

/// Moves everything in `data` whose name doesn't resolve onto the returned shelf,
/// and returns the shelved inventory stacks whose mod *is* back (to give to the
/// player). After this `data` holds only what the game can read.
pub fn split_unknown(data: &mut WorldData, blocks: &BlockRegistry, items: &ItemRegistry) -> (Shelf, Vec<SlotSave>) {
    let mut shelf = Shelf::default();

    let (known, unknown): (Vec<_>, Vec<_>) = std::mem::take(&mut data.edits)
        .into_iter()
        .partition(|e| blocks.by_name(&e.block).is_ok());
    data.edits = known;
    shelf.edits = unknown;

    let (known, unknown): (Vec<_>, Vec<_>) = std::mem::take(&mut data.fluids)
        .into_iter()
        .partition(|f| blocks.by_name(&f.block).is_ok());
    data.fluids = known;
    shelf.fluids = unknown;

    let (known, unknown): (Vec<_>, Vec<_>) = std::mem::take(&mut data.items)
        .into_iter()
        .partition(|d| items.by_name(&d.block).is_some());
    data.items = known;
    shelf.drops = unknown;

    let (known, unknown): (Vec<_>, Vec<_>) =
        std::mem::take(&mut data.known_items).into_iter().partition(|n| items.by_name(n).is_some());
    data.known_items = known;
    shelf.known = unknown;

    // Inventory: an unknown stack leaves its slot empty and goes on the shelf.
    let mut shelve = |slot: &mut Option<SlotSave>| {
        if slot.as_ref().is_some_and(|s| items.by_name(&s.block).is_none()) {
            shelf.items.extend(slot.take());
        }
    };
    if let Some(inventory) = data.inventory.as_mut() {
        inventory.hotbar.iter_mut().for_each(&mut shelve);
        inventory.storage.iter_mut().for_each(&mut shelve);
        shelve(&mut inventory.held);
    }

    // Stacks shelved by an earlier session: back if their item exists, else still shelved.
    let mut returned = Vec::new();
    for stack in std::mem::take(&mut data.shelved_items) {
        if items.by_name(&stack.block).is_some() {
            returned.push(stack);
        } else {
            shelf.items.push(stack);
        }
    }
    (shelf, returned)
}

/// Writes the shelf back into a save about to be written. `data` is the live
/// world; shelved entries lose to it where they collide (see the module docs).
pub fn merge_into(data: &mut WorldData, shelf: &Shelf) {
    let edited: std::collections::HashSet<(i32, i32, i32)> = data.edits.iter().map(|e| (e.x, e.y, e.z)).collect();
    data.edits.extend(shelf.edits.iter().filter(|e| !edited.contains(&(e.x, e.y, e.z))).cloned());

    let wet: std::collections::HashSet<(i32, i32, i32)> = data.fluids.iter().map(|f| (f.x, f.y, f.z)).collect();
    data.fluids.extend(shelf.fluids.iter().filter(|f| !wet.contains(&(f.x, f.y, f.z))).cloned());

    data.items.extend(shelf.drops.iter().cloned());
    for name in &shelf.known {
        if !data.known_items.contains(name) {
            data.known_items.push(name.clone());
        }
    }
    data.shelved_items = shelf.items.clone();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::InventorySave;

    fn registries() -> (BlockRegistry, ItemRegistry) {
        let blocks = BlockRegistry::with_defaults();
        let items = ItemRegistry::with_defaults(&blocks);
        (blocks, items)
    }

    fn edit(x: i32, block: &str) -> BlockEdit {
        BlockEdit { x, y: 40, z: 0, block: block.into(), axis: 0, state: Some(0), ext: None }
    }

    fn slot(block: &str, count: u32) -> Option<SlotSave> {
        Some(SlotSave { block: block.into(), count, quality: 0 })
    }

    fn data_with_mod_stuff() -> WorldData {
        WorldData {
            edits: vec![edit(1, "stone"), edit(2, "gp:granite"), edit(3, "gp:granite")],
            fluids: vec![
                FluidCell { x: 0, y: 1, z: 2, block: "water".into(), level: 1 },
                FluidCell { x: 5, y: 1, z: 2, block: "gp:lava".into(), level: 0 },
            ],
            items: vec![
                DroppedItemSave { x: 0.0, y: 0.0, z: 0.0, block: "stick".into(), count: 1, quality: 0, age: 0.0 },
                DroppedItemSave { x: 1.0, y: 0.0, z: 0.0, block: "gp:chip".into(), count: 4, quality: 0, age: 3.0 },
            ],
            known_items: vec!["stick".into(), "gp:chip".into()],
            inventory: Some(InventorySave {
                hotbar: vec![slot("stone", 3), slot("gp:chip", 7), None],
                storage: vec![slot("gp:granite", 64), slot("dirt", 2)],
                selected: 0,
                held: slot("gp:chip", 1),
            }),
            ..WorldData::default()
        }
    }

    #[test]
    fn what_the_game_cant_read_goes_on_the_shelf_and_the_rest_is_left_alone() {
        let (blocks, items) = registries();
        let mut data = data_with_mod_stuff();
        let (shelf, returned) = split_unknown(&mut data, &blocks, &items);
        assert!(returned.is_empty());
        assert_eq!(data.edits.len(), 1);
        assert_eq!(shelf.edits.len(), 2);
        assert_eq!((data.fluids.len(), shelf.fluids.len()), (1, 1));
        assert_eq!((data.items.len(), shelf.drops.len()), (1, 1));
        assert_eq!((data.known_items.len(), shelf.known.len()), (1, 1));
        let inv = data.inventory.as_ref().unwrap();
        assert_eq!(inv.hotbar[1], None, "the slot is empty in the world...");
        assert_eq!(inv.hotbar[0], slot("stone", 3), "...and ordinary stacks stay");
        assert_eq!(inv.storage, vec![None, slot("dirt", 2)]);
        assert_eq!(inv.held, None);
        assert_eq!(shelf.items.len(), 3, "...while the three stacks wait on the shelf");
    }

    #[test]
    fn a_save_written_with_the_mod_off_still_has_everything_from_the_mod() {
        let (blocks, items) = registries();
        let mut loaded = data_with_mod_stuff();
        let (shelf, _) = split_unknown(&mut loaded, &blocks, &items);
        // The session writes its live state (just the readable part) and merges the shelf.
        let mut saved = loaded.clone();
        merge_into(&mut saved, &shelf);
        assert_eq!(saved.edits.iter().filter(|e| e.block == "gp:granite").count(), 2);
        assert!(saved.fluids.iter().any(|f| f.block == "gp:lava"));
        assert!(saved.items.iter().any(|d| d.block == "gp:chip" && d.count == 4 && d.age == 3.0));
        assert!(saved.known_items.contains(&"gp:chip".to_string()));
        assert_eq!(saved.shelved_items.len(), 3);
        assert_eq!(saved.shelved_items.iter().map(|s| s.count).sum::<u32>(), 7 + 64 + 1);
    }

    #[test]
    fn a_world_survives_the_mod_being_off_for_several_saves_in_a_row() {
        let (blocks, items) = registries();
        let mut data = data_with_mod_stuff();
        for _ in 0..3 {
            let (shelf, returned) = split_unknown(&mut data, &blocks, &items);
            assert!(returned.is_empty());
            merge_into(&mut data, &shelf);
        }
        assert_eq!(data.edits.len(), 3);
        assert_eq!(data.shelved_items.len(), 3, "not duplicated on each pass");
        assert_eq!(data.known_items.len(), 2);
    }

    #[test]
    fn a_shelved_edit_loses_to_what_the_player_has_built_there_since() {
        let (blocks, items) = registries();
        let mut loaded = data_with_mod_stuff();
        let (shelf, _) = split_unknown(&mut loaded, &blocks, &items);
        // While the mod was off the player put dirt where the granite was (x = 2).
        let mut live = loaded.clone();
        live.edits.push(edit(2, "dirt"));
        merge_into(&mut live, &shelf);
        let at_2: Vec<_> = live.edits.iter().filter(|e| e.x == 2).map(|e| e.block.as_str()).collect();
        assert_eq!(at_2, vec!["dirt"], "the new block is the world's truth");
        assert!(live.edits.iter().any(|e| e.x == 3 && e.block == "gp:granite"), "untouched ones are kept");
    }

    #[test]
    fn items_shelved_earlier_come_back_when_their_mod_does() {
        let (blocks, items) = registries();
        let mut data = WorldData {
            shelved_items: vec![SlotSave { block: "stick".into(), count: 5, quality: 0 }, SlotSave { block: "gp:chip".into(), count: 2, quality: 0 }],
            ..WorldData::default()
        };
        let (shelf, returned) = split_unknown(&mut data, &blocks, &items);
        assert_eq!(returned.iter().map(|s| (s.block.as_str(), s.count)).collect::<Vec<_>>(), vec![("stick", 5)]);
        assert_eq!(shelf.items.len(), 1, "the one still unknown stays shelved");
        assert!(data.shelved_items.is_empty(), "taken off the data so it isn't read twice");
        let mut saved = data;
        merge_into(&mut saved, &shelf);
        assert_eq!(saved.shelved_items.len(), 1);
    }

    #[test]
    fn a_world_with_nothing_unknown_has_an_empty_shelf_and_saves_nothing_extra() {
        let (blocks, items) = registries();
        let mut data = WorldData { edits: vec![edit(1, "stone")], ..WorldData::default() };
        let (shelf, returned) = split_unknown(&mut data, &blocks, &items);
        assert!(shelf.is_empty() && returned.is_empty());
        let before = serde_json::to_string(&data).unwrap();
        merge_into(&mut data, &shelf);
        assert_eq!(serde_json::to_string(&data).unwrap(), before);
        assert!(!before.contains("shelved"), "an ordinary save doesn't mention it");
    }
}
