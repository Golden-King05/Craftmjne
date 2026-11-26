//! Building the game's content - blocks, items, recipes, textures, icons and the
//! flat lookup tables - from the core files plus any mods laid over them.
//!
//! [`build_content`] is a pure function of a [`ContentPaths`]: it reads files and
//! returns a [`Content`], touching no resources and writing nothing. That is what
//! lets a world with different mods get its own content (`world::apply_world_mods`
//! builds one on entering the world) and what lets tests build content from a
//! scratch mod folder. The core files are only ever read; see `mods.rs` for how
//! a mod overlays them.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::atlas::{build_atlas_in, default_painters, normalize_tint_mask_tile, AtlasData, TextureStatus};
use crate::blocks::{find_data_dir, BlockRegistry, Tables};
use crate::crafting::RecipeBook;
use crate::icons::{build_icon_atlas, IconAtlasData};
use crate::item_registry::ItemRegistry;
use crate::models::{collision_boxes, compile_model, load_block_models};
use crate::mods::ModSource;
use crate::overlay::{KindDir, Locals};
use crate::packs::PackStack;
use crate::tools::TierBook;

/// Where to read content from, one list per kind. Each is in load order: the
/// core game's directory first, then each mod's, so later ones patch earlier
/// ones. (Textures are looked up from the end - see `atlas::load_custom_tile_in`.)
#[derive(Clone, Debug)]
pub struct ContentPaths {
    pub blocks: Vec<KindDir>,
    pub items: Vec<KindDir>,
    pub recipes: Vec<KindDir>,
    pub tiers: Vec<KindDir>,
    pub rarities: Vec<KindDir>,
    /// The resource-pack layers (the game's, each mod's, the player's), lowest first: where
    /// block textures, models and sounds come from.
    pub stack: PackStack,
}

impl ContentPaths {
    /// The core game alone.
    pub fn vanilla() -> Self {
        Self {
            blocks: vec![KindDir::core(find_data_dir("blocks"))],
            items: vec![KindDir::core(find_data_dir("items"))],
            recipes: vec![KindDir::core(find_data_dir("recipes"))],
            tiers: vec![KindDir::core(find_data_dir("tiers"))],
            rarities: vec![KindDir::core(find_data_dir("rarities"))],
            stack: PackStack::core(),
        }
    }

    /// The directories of block textures, lowest priority first.
    pub fn textures(&self) -> Vec<KindDir> {
        self.stack.dirs("textures/blocks")
    }

    /// The directories of block models, lowest priority first.
    pub fn models(&self) -> Vec<KindDir> {
        self.stack.dirs("models/blocks")
    }

    /// The directories of sounds, lowest priority first.
    pub fn sounds(&self) -> Vec<KindDir> {
        self.stack.dirs("sounds")
    }

    /// The player's enabled resource packs on top (given lowest priority first). Call this
    /// after [`ContentPaths::with_mods`] so a player's pack outranks a mod's.
    pub fn with_packs(mut self, packs: &[PathBuf]) -> Self {
        self.stack = self.stack.with_user_packs(packs);
        self
    }

    /// The core game with `mods` laid over it, in load order, each under its namespace.
    pub fn with_mods(mut self, mods: &[ModSource]) -> Self {
        for m in mods {
            let locals = Arc::new(Locals::scan(&m.path));
            let dir = |kind: &str| KindDir { ns: m.ns.clone(), dir: m.path.join(kind), locals: locals.clone(), extends: true };
            self.blocks.push(dir("blocks"));
            self.items.push(dir("items"));
            self.recipes.push(dir("recipes"));
            self.tiers.push(dir("tiers"));
            self.rarities.push(dir("rarities"));
            self.stack = std::mem::replace(&mut self.stack, PackStack { layers: Vec::new() }).with_mod(&m.ns, &m.path);
        }
        self
    }
}

/// Everything built from one set of content.
pub struct Content {
    pub registry: BlockRegistry,
    pub items: ItemRegistry,
    pub recipes: RecipeBook,
    pub atlas: AtlasData,
    pub icons: IconAtlasData,
    pub tables: Arc<Tables>,
    /// Block textures that aren't plain working art (see `texture_report.rs`).
    pub texture_status: Vec<(String, TextureStatus)>,
    /// Texture names a block needs that never made it into the atlas.
    pub missing_textures: Vec<String>,
    /// Pack models that couldn't be used (and why), for the log and the pack's author.
    pub model_notes: Vec<String>,
}

/// Builds content from `paths`. Panics, naming the file, on malformed content,
/// like the loaders it calls - use [`try_build_content`] for content that isn't
/// the core game's.
pub fn build_content(paths: &ContentPaths) -> Content {
    let mut registry = BlockRegistry::load_from_dirs(&paths.blocks);
    let mut items = ItemRegistry::load_from_dirs(&registry, &paths.items).with_tiers(TierBook::load_from_dirs(&paths.tiers))
        .with_rarities(crate::tools::RarityBook::load_from_dirs(&paths.rarities));
    // A texture name with no painter gets the "missing texture" placeholder
    // instead of `compile()` panicking - see `Painters::ensure_registered`.
    let mut painters = default_painters();
    for name in registry.texture_names().collect::<HashSet<_>>() {
        painters.ensure_registered(&name);
    }
    for name in items.texture_names() {
        painters.ensure_registered(name);
    }
    // Resource-pack models name textures too; each needs a tile (a placeholder if nobody has art).
    let models = load_block_models(&registry, &paths.models());
    for name in models.texture_names() {
        painters.ensure_registered(&name);
    }
    let mut atlas = build_atlas_in(&painters, &paths.textures());
    items.resolve_tiles(&atlas.indices);
    let recipes = RecipeBook::load_from_dirs(&registry, &items, &paths.recipes);

    // Contrast-stretch every texture a block tints so grayscale mask art that
    // isn't near-white still reads at full colour (see
    // `atlas::normalize_tint_mask_tile`).
    for name in registry.tint_mask_names() {
        normalize_tint_mask_tile(&mut atlas, &name);
    }
    // A real invariant check for `/texture-report`'s red tier: every name the
    // registry needs should have ended up in the atlas.
    let missing_textures: Vec<String> =
        registry.texture_names().filter(|name| !atlas.indices.contains_key(name)).collect();
    let texture_status = atlas.texture_status.clone();
    let icons = build_icon_atlas(&registry, &atlas);
    let mut tables = registry.compile(&atlas.indices, atlas.tile_size);
    let visual = models.visual.iter().map(|(id, m)| (*id, compile_model(m, &atlas.indices))).collect();
    let collision = models.collision.iter().map(|(id, m)| (*id, collision_boxes(m))).collect();
    Arc::get_mut(&mut tables).expect("the tables aren't shared yet").apply_models(visual, collision);
    Content { registry, items, recipes, atlas, icons, tables, texture_status, missing_textures, model_notes: models.notes }
}

/// [`build_content`], reporting a problem as an `Err` instead of panicking - for
/// mod content, where a mistake in someone's file must not take the game down.
pub fn try_build_content(paths: &ContentPaths) -> Result<Content, String> {
    std::panic::catch_unwind(|| build_content(paths)).map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "the mod's content could not be loaded".to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A scratch mod folder, removed on drop.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("craftmjne-content-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn file(&self, rel: &str, text: &str) -> &Self {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
            self
        }
        fn png(&self, rel: &str, color: [u8; 4]) -> &Self {
            let path = self.0.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            image::RgbaImage::from_pixel(16, 16, image::Rgba(color)).save(path).unwrap();
            self
        }
        fn path(&self) -> PathBuf {
            self.0.clone()
        }
        fn source(&self, ns: &str) -> ModSource {
            ModSource { ns: ns.to_string(), path: self.0.clone() }
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn with(mods: &[ModSource]) -> ContentPaths {
        ContentPaths::vanilla().with_mods(mods)
    }

    fn tile_pixel(c: &Content, name: &str) -> [u8; 4] {
        let index = c.atlas.indices[name] as usize;
        let (tiles, size) = (crate::config::ATLAS_TILES, c.atlas.tile_size);
        let (tx, ty) = (index % tiles, index / tiles);
        let at = ((ty * size) * (tiles * size) + tx * size) * 4;
        c.atlas.pixels[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn a_mod_changes_one_property_of_a_core_block_by_naming_it_in_full() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.file("blocks/dirt.json", r#"{"id": "craftmjne:dirt", "hardness": 8.0}"#);
        let modded = build_content(&with(&[m.source("slow")]));
        let (a, b) = (vanilla.registry.def(vanilla.registry.id("dirt")), modded.registry.def(modded.registry.id("dirt")));
        assert_eq!((b.hardness, a.hardness), (8.0, 0.5));
        assert_eq!((b.tool, b.tool_response, &b.name), (a.tool, a.tool_response, &a.name), "the rest is dirt's own");
        assert!(modded.registry.by_name("slow:dirt").is_err(), "it patched dirt, it didn't add another");
    }

    #[test]
    fn a_plain_id_in_a_mod_is_a_new_thing_under_the_mods_prefix() {
        let m = Scratch::new();
        m.file("blocks/dirt.json", r#"{"id": "dirt", "hardness": 8.0}"#);
        let c = build_content(&with(&[m.source("slow")]));
        assert_eq!(c.registry.def(c.registry.id("dirt")).hardness, 0.5, "the core dirt is untouched");
        assert_eq!(c.registry.def(c.registry.id("slow:dirt")).hardness, 8.0, "this is the mod's own dirt");
        assert_eq!(c.registry.def(c.registry.id("slow:dirt")).name, "Dirt", "named without the prefix");
    }

    #[test]
    fn a_null_in_a_mod_puts_a_key_back_to_its_default() {
        let m = Scratch::new();
        m.file("blocks/dirt.json", r#"{"id": "craftmjne:dirt", "tool": null}"#);
        let c = build_content(&with(&[m.source("m")]));
        assert_eq!(c.registry.def(c.registry.id("dirt")).tool, None);
    }

    #[test]
    fn two_mods_can_each_add_a_granite_and_both_exist() {
        let (a, b) = (Scratch::new(), Scratch::new());
        a.file("blocks/granite.json", r#"{"id": "granite", "hardness": 2.0}"#);
        b.file("blocks/granite.json", r#"{"id": "granite", "hardness": 5.0}"#);
        let c = build_content(&with(&[a.source("alpha"), b.source("beta")]));
        assert_eq!(c.registry.def(c.registry.id("alpha:granite")).hardness, 2.0);
        assert_eq!(c.registry.def(c.registry.id("beta:granite")).hardness, 5.0);
        assert!(c.items.by_name("alpha:granite").is_some() && c.items.by_name("beta:granite").is_some());
    }

    #[test]
    fn the_blocks_core_files_define_keep_their_ids_when_mods_add_blocks() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.file("blocks/battery.json", r#"{"id": "battery", "hardness": 2.0}"#);
        let modded = build_content(&with(&[m.source("m")]));
        for (id, def) in vanilla.registry.defs.iter().enumerate().filter(|(_, d)| d.layered.is_none()) {
            assert_eq!(modded.registry.def(id as u16).id, def.id, "block {id}");
        }
        // The derived `pebble@water`-style variants come after every file's
        // block, so they may shift - harmless, since nothing saves a block id
        // as a number (saves, snapshots and edits all store the name).
        assert!(modded.registry.by_name("m:battery").is_ok());
        assert!(modded.registry.by_name("battery").is_err(), "not the core game's");
        assert!(vanilla.registry.by_name("m:battery").is_err());
    }

    #[test]
    fn a_mods_own_things_find_each_other_by_plain_name_and_reach_the_core_the_same_way() {
        let m = Scratch::new();
        m.file("blocks/ore.json", r#"{"id": "ore", "drops": {"item": "chip", "count": 2}, "textures": {"all": "stone"}}"#)
            .file("items/chip.json", r#"{"id": "chip"}"#)
            .file("recipes/chip_pile.json", r#"{"inputs": [{"item": "chip", "count": 4}, {"item": "stick", "count": 1}], "output": {"item": "ore", "count": 1}}"#);
        let c = build_content(&with(&[m.source("rocks")]));
        let ore = c.registry.def(c.registry.id("rocks:ore"));
        assert!(matches!(&ore.drops, crate::blocks::Drops::Item { item, count: 2, .. } if item == "rocks:chip"), "{:?}", ore.drops);
        let recipe = c.recipes.recipes.iter().find(|r| r.id == "rocks:chip_pile").expect("recipe");
        assert_eq!(recipe.inputs[0].id, c.items.id("rocks:chip"), "its own chip");
        assert_eq!(recipe.inputs[1].id, c.items.id("stick"), "the core game's stick");
        assert_eq!(recipe.output.id, c.items.id("rocks:ore"));
    }

    #[test]
    fn a_mod_adds_a_recipe_and_replaces_a_core_one_by_saying_so() {
        let m = Scratch::new();
        m.file("items/widget.json", r#"{"id": "widget"}"#)
            .file("recipes/widget.json", r#"{"inputs": [{"item": "stick", "count": 2}], "output": {"item": "widget", "count": 1}}"#)
            .file("recipes/better_planks.json", r#"{"replaces": "craftmjne:planks", "inputs": [{"item": "stick", "count": 9}], "output": {"item": "planks", "count": 1}}"#);
        let c = build_content(&with(&[m.source("w")]));
        assert!(c.recipes.recipes.iter().any(|r| r.id == "w:widget"));
        let planks: Vec<_> = c.recipes.recipes.iter().filter(|r| r.id == "planks").collect();
        assert_eq!(planks.len(), 1, "replaced, not duplicated");
        assert_eq!(planks[0].inputs[0].count, 9);
        assert!(!c.recipes.recipes.iter().any(|r| r.id == "w:better_planks"));
    }

    #[test]
    fn a_mods_own_texture_is_its_own_and_a_folder_named_for_the_core_reskins_core_blocks() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.file("blocks/granite.json", r#"{"id": "granite"}"#)
            .png("resourcepack/textures/blocks/granite.png", [4, 5, 6, 255])
            .png("resourcepack/textures/blocks/craftmjne/dirt.png", [1, 2, 3, 255]);
        let modded = build_content(&with(&[m.source("g")]));
        assert_eq!(tile_pixel(&modded, "g:granite"), [4, 5, 6, 255], "its own art, by plain file name");
        assert_eq!(tile_pixel(&modded, "dirt"), [1, 2, 3, 255], "re-skinned from the craftmjne folder");
        assert_ne!(tile_pixel(&vanilla, "dirt"), [1, 2, 3, 255]);
        assert_eq!(tile_pixel(&build_content(&ContentPaths::vanilla()), "dirt"), tile_pixel(&vanilla, "dirt"), "taking it off restores it");
    }

    #[test]
    fn a_mods_textures_in_the_old_place_are_no_longer_read() {
        let m = Scratch::new();
        m.file("blocks/granite.json", r#"{"id": "granite"}"#).png("textures/blocks/granite.png", [4, 5, 6, 255]);
        let c = build_content(&with(&[m.source("g")]));
        assert_ne!(tile_pixel(&c, "g:granite"), [4, 5, 6, 255], "art belongs in resourcepack/ now; the old folder is ignored");
    }

    #[test]
    fn a_players_pack_reskins_core_and_mod_blocks_and_outranks_a_mods_pack() {
        let (modd, pack) = (Scratch::new(), Scratch::new());
        modd.file("blocks/granite.json", r#"{"id": "granite"}"#)
            .png("resourcepack/textures/blocks/granite.png", [4, 5, 6, 255])
            .png("resourcepack/textures/blocks/craftmjne/dirt.png", [1, 2, 3, 255]);
        // A player's pack: plain names are the core game's; <ns>/ is a mod's.
        pack.png("textures/blocks/dirt.png", [8, 8, 8, 255]).png("textures/blocks/g/granite.png", [7, 0, 7, 255]);
        let paths = with(&[modd.source("g")]).with_packs(&[pack.path()]);
        let c = build_content(&paths);
        assert_eq!(tile_pixel(&c, "dirt"), [8, 8, 8, 255], "the player's pack is above the mod's");
        assert_eq!(tile_pixel(&c, "g:granite"), [7, 0, 7, 255], "and can re-skin a mod's block");
        // Without the pack the mod's own art is back.
        let c = build_content(&with(&[modd.source("g")]));
        assert_eq!((tile_pixel(&c, "dirt"), tile_pixel(&c, "g:granite")), ([1, 2, 3, 255], [4, 5, 6, 255]));
    }

    #[test]
    fn two_players_packs_stack_in_the_order_given() {
        let (low, high) = (Scratch::new(), Scratch::new());
        low.png("textures/blocks/dirt.png", [1, 1, 1, 255]).png("textures/blocks/stone.png", [2, 2, 2, 255]);
        high.png("textures/blocks/dirt.png", [9, 9, 9, 255]);
        let c = build_content(&ContentPaths::vanilla().with_packs(&[low.path(), high.path()]));
        assert_eq!(tile_pixel(&c, "dirt"), [9, 9, 9, 255], "the later (higher) pack wins where both have a file");
        assert_eq!(tile_pixel(&c, "stone"), [2, 2, 2, 255], "and the lower one still supplies what the higher doesn't");
    }

    #[test]
    fn a_players_pack_model_changes_how_a_block_is_drawn_and_nothing_else() {
        let pack = Scratch::new();
        pack.file("models/blocks/dirt.json", r#"{"boxes": [{"from": [0,0,0], "to": [16,8,16], "textures": {"all": "mytex"}}], "collision": "model"}"#)
            .png("textures/blocks/mytex.png", [5, 6, 7, 255]);
        let c = build_content(&ContentPaths::vanilla().with_packs(&[pack.path()]));
        let dirt = c.registry.id("dirt") as usize;
        let model = c.tables.pack_models[dirt].as_ref().expect("the pack's model is installed");
        assert_eq!(model.boxes[0].max[1], 0.5);
        let tile = model.boxes[0].tiles[2].expect("its texture got a tile") as usize;
        assert_eq!(tile, c.atlas.indices["mytex"] as usize, "a texture only the model names still reaches the atlas");
        assert_eq!(tile_pixel(&c, "mytex"), [5, 6, 7, 255], "with the pack's art");
        assert!(!c.tables.occludes[dirt] && c.tables.opaque[dirt], "a half block hides no neighbour's face, but it is still an opaque block for light");
        assert!(c.tables.collision_boxes[dirt].is_none(), "a player's pack can't change collision, even asking to");
        assert!(c.tables.solid[dirt]);
    }

    #[test]
    fn a_mods_model_can_replace_a_core_blocks_collision() {
        let m = Scratch::new();
        m.file("resourcepack/models/blocks/craftmjne/dirt.json", r#"{"boxes": [{"from": [0,0,0], "to": [16,8,16], "textures": {"all": "dirt"}}], "collision": "model"}"#)
            .file("blocks/granite.json", r#"{"id": "granite"}"#)
            .file("resourcepack/models/blocks/granite.json", r#"{"boxes": [{"from": [4,0,4], "to": [12,16,12], "textures": {"all": "granite"}}]}"#)
            .png("resourcepack/textures/blocks/granite.png", [4, 5, 6, 255]);
        let c = build_content(&with(&[m.source("g")]));
        let dirt = c.registry.id("dirt") as usize;
        let boxes = c.tables.collision_boxes[dirt].as_ref().expect("the mod's model took over dirt's collision");
        assert_eq!((boxes[0].min, boxes[0].max), ([0.0; 3], [1.0, 0.5, 1.0]));
        let granite = c.registry.id("g:granite") as usize;
        assert!(c.tables.pack_models[granite].is_some(), "a mod's own block is drawn by its model");
        assert!(c.tables.collision_boxes[granite].is_none(), "without \"collision\": \"model\" its collision is the block's own");
    }

    #[test]
    fn a_broken_model_is_noted_and_the_block_keeps_its_look() {
        let pack = Scratch::new();
        pack.file("models/blocks/dirt.json", "{broken");
        let c = build_content(&ContentPaths::vanilla().with_packs(&[pack.path()]));
        assert!(c.tables.pack_models[c.registry.id("dirt") as usize].is_none());
        assert!(c.model_notes.iter().any(|n| n.contains("dirt.json")), "{:?}", c.model_notes);
    }

    #[test]
    fn a_plain_texture_file_named_like_a_core_block_does_not_reskin_it() {
        let m = Scratch::new();
        m.png("resourcepack/textures/blocks/dirt.png", [9, 9, 9, 255]);
        let c = build_content(&with(&[m.source("m")]));
        assert_ne!(tile_pixel(&c, "dirt"), [9, 9, 9, 255], "that file is the mod's own dirt, not the core game's");
    }

    #[test]
    fn with_several_mods_the_later_one_wins_a_patch() {
        let (a, b) = (Scratch::new(), Scratch::new());
        a.file("blocks/dirt.json", r#"{"id": "craftmjne:dirt", "hardness": 3.0, "name": "Mud"}"#)
            .png("resourcepack/textures/blocks/craftmjne/dirt.png", [9, 9, 9, 255]);
        b.file("blocks/dirt.json", r#"{"id": "craftmjne:dirt", "hardness": 6.0}"#)
            .png("resourcepack/textures/blocks/craftmjne/dirt.png", [7, 7, 7, 255]);
        let c = build_content(&with(&[a.source("a"), b.source("b")]));
        let dirt = c.registry.def(c.registry.id("dirt"));
        assert_eq!((dirt.hardness, dirt.name.as_str()), (6.0, "Mud"), "b over a, keeping what only a changed");
        assert_eq!(tile_pixel(&c, "dirt"), [7, 7, 7, 255]);
    }

    #[test]
    fn a_mod_can_patch_another_mods_block_if_it_loads_after_it() {
        let (a, b) = (Scratch::new(), Scratch::new());
        a.file("blocks/granite.json", r#"{"id": "granite", "hardness": 2.0}"#);
        b.file("blocks/tweak.json", r#"{"id": "alpha:granite", "hardness": 9.0}"#);
        let c = build_content(&with(&[a.source("alpha"), b.source("beta")]));
        assert_eq!(c.registry.def(c.registry.id("alpha:granite")).hardness, 9.0);
        // The other way round, the target doesn't exist yet: an error naming the file.
        let err = try_build_content(&with(&[b.source("beta"), a.source("alpha")])).err().expect("refused");
        assert!(err.contains("alpha:granite") && err.contains("tweak.json"), "{err}");
    }

    /// The point of an overlay: the core files are read and never written.
    #[test]
    fn building_with_mods_never_changes_the_core_files() {
        fn snapshot() -> Vec<(PathBuf, Vec<u8>)> {
            let mut out = Vec::new();
            for dir in ["blocks", "items", "recipes", "textures", "tiers"] {
                let mut stack = vec![find_data_dir(dir)];
                while let Some(d) = stack.pop() {
                    let Ok(entries) = std::fs::read_dir(&d) else { continue };
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            stack.push(p);
                        } else {
                            out.push((p.clone(), std::fs::read(&p).unwrap()));
                        }
                    }
                }
            }
            out.sort();
            out
        }
        let before = snapshot();
        assert!(!before.is_empty());
        let m = Scratch::new();
        m.file("blocks/dirt.json", r#"{"id": "craftmjne:dirt", "hardness": 20.0}"#)
            .file("recipes/p.json", r#"{"replaces": "craftmjne:planks", "inputs": [{"item": "stick", "count": 9}], "output": {"item": "planks", "count": 1}}"#)
            .png("resourcepack/textures/blocks/craftmjne/dirt.png", [1, 1, 1, 255]);
        let _ = build_content(&with(&[m.source("m")]));
        assert!(before == snapshot(), "a core file changed");
    }

    #[test]
    fn a_broken_mod_is_an_error_naming_the_file_not_a_crash() {
        let m = Scratch::new();
        m.file("blocks/oops.json", r#"{"id": "oops", "hardness": "very"}"#);
        let err = try_build_content(&with(&[m.source("m")])).err().expect("refused");
        assert!(err.contains("oops.json"), "{err}");
        let m2 = Scratch::new();
        m2.file("recipes/bad.json", r#"{"inputs": [{"item": "nope", "count": 1}], "output": {"item": "stick", "count": 1}}"#);
        assert!(try_build_content(&with(&[m2.source("m")])).err().expect("refused").contains("nope"));
    }

    /// The example mod in `docs/` is what the docs tell people to copy, so it has
    /// to keep working: it loads, does what its description says, and is a mod.
    #[test]
    fn the_example_mod_in_the_docs_works() {
        let path = PathBuf::from("docs").join("example-mod");
        let info = crate::mods::read_mod(&path).expect("a valid mod");
        let c = build_content(&with(&[ModSource { ns: info.id.clone(), path }]));
        assert_eq!(c.registry.def(c.registry.id("dirt")).hardness, 8.0, "dirt is slow to dig");
        assert!(c.registry.by_name(&format!("{}:amber", info.id)).is_ok(), "amber was added under the mod's id");
        assert!(c.recipes.recipes.iter().any(|r| r.id == format!("{}:amber", info.id)), "and a recipe for it");
    }

    /// A data folder the game reads but the release workflows forget to package
    /// makes every released build panic at startup (this happened with `tiers/` and
    /// `entities/`, unnoticed because nothing builds a release from a test). So:
    /// every `find_data_dir("x")` in the source must be copied by both workflows.
    #[test]
    fn every_data_folder_the_game_reads_is_packaged_by_the_release_workflows() {
        let mut wanted = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir("src").unwrap().flatten() {
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            // Only code before the test module counts (tests mention other names).
            let code = text.split("#[cfg(test)]").next().unwrap_or("");
            for part in code.split("find_data_dir(\"").skip(1) {
                if let Some(name) = part.split('"').next() {
                    wanted.insert(name.to_string());
                }
            }
        }
        assert!(wanted.contains("blocks") && wanted.contains("rarities"), "the scan found the folders: {wanted:?}");
        for workflow in [".github/workflows/release.yml", ".github/workflows/dev-build.yml"] {
            let yaml = std::fs::read_to_string(workflow).unwrap();
            for name in &wanted {
                assert!(yaml.contains(&format!("cp -r {name} ")), "{workflow} doesn't package `{name}/`");
            }
        }
    }

    #[test]
    fn a_mod_adds_a_tool_tier_and_a_tool_made_of_it_and_can_change_a_core_tier() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.file("tiers/copper.json", r#"{"id": "copper", "name": "Copper", "mining_speed": 2.5, "damage": 2.0}"#)
            .file("tiers/stone.json", r#"{"id": "craftmjne:stone", "mining_speed": 9.0}"#)
            .file("items/copper_pickaxe.json", r#"{"id": "copper_pickaxe", "tool": {"kind": "pickaxe", "tier": "copper"}}"#)
            .file("items/hard_axe.json", r#"{"id": "hard_axe", "tool": {"kind": "axe", "tier": "stone"}}"#);
        let c = build_content(&with(&[m.source("metals")]));
        let pick = c.items.def(c.items.id("metals:copper_pickaxe"));
        assert_eq!(pick.tool.as_ref().unwrap().tier, "metals:copper", "its own tier, found by plain name");
        let copper = c.items.tiers.get("metals:copper").expect("the tier exists");
        assert_eq!((copper.name.as_str(), copper.mining_speed), ("Copper", 2.5));
        let axe = c.items.def(c.items.id("metals:hard_axe"));
        assert_eq!(axe.tool.as_ref().unwrap().tier, "stone", "a plain name that isn't the mod's is the core game's");
        assert_eq!(c.items.tiers.get("stone").unwrap().mining_speed, 9.0, "the core tier was changed...");
        assert_eq!(vanilla.items.tiers.get("stone").unwrap().mining_speed, 3.0, "...only while the mod is on");
        assert!(vanilla.items.tiers.get("metals:copper").is_none());
    }

    #[test]
    fn a_tool_naming_a_tier_that_does_not_exist_is_an_error_not_a_crash() {
        let m = Scratch::new();
        m.file("items/bad_pick.json", r#"{"id": "bad_pick", "tool": {"kind": "pickaxe", "tier": "unobtainium"}}"#);
        let err = try_build_content(&with(&[m.source("m")])).err().expect("refused");
        assert!(err.contains("unobtainium"), "{err}");
    }

    #[test]
    fn a_mod_adds_a_rarity_that_splits_an_existing_one_and_can_recolour_a_core_rarity() {
        let m = Scratch::new();
        m.file("rarities/mythic.json", r#"{"id": "mythic", "name": "Mythic", "min": 95, "color": [1.0, 0.2, 0.2]}"#)
            .file("rarities/rare.json", r#"{"id": "craftmjne:rare", "color": [0.0, 1.0, 0.0]}"#);
        let c = build_content(&with(&[m.source("lore")]));
        assert_eq!(c.items.rarity(97).name, "Mythic");
        assert_eq!(c.items.rarity(94).name, "Epic", "below the new one it's still Epic");
        assert_eq!(c.items.rarity(100).name, "Legendary");
        assert_eq!(c.items.rarity(70).color, [0.0, 1.0, 0.0], "Rare was recoloured");
        assert_eq!(c.items.rarity(70).name, "Rare", "and keeps its name");
        let vanilla = build_content(&ContentPaths::vanilla());
        assert_eq!(vanilla.items.rarity(97).name, "Epic", "without the mod nothing changed");
    }

    #[test]
    fn rarities_that_start_at_the_same_quality_or_make_no_sense_are_refused() {
        let m = Scratch::new();
        m.file("rarities/dup.json", r#"{"id": "dup", "min": 20}"#);
        let err = try_build_content(&with(&[m.source("m")])).err().expect("refused");
        assert!(err.contains("both start at quality 20"), "{err}");
        let m2 = Scratch::new();
        m2.file("rarities/high.json", r#"{"id": "high", "min": 150}"#);
        assert!(try_build_content(&with(&[m2.source("m")])).err().expect("refused").contains("above 100"));
        let m3 = Scratch::new();
        m3.file("rarities/dark.json", r#"{"id": "dark", "min": 50, "color": [2.0, 0.0, 0.0]}"#);
        assert!(try_build_content(&with(&[m3.source("m")])).err().expect("refused").contains("0..1"));
    }
}
