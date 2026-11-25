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
use crate::tools::TierBook;

/// Where to read content from. Block, item and recipe directories are in load
/// order (the core game's first, then each mod's, so later ones patch earlier
/// ones); texture directories are the reverse, highest priority first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentPaths {
    pub blocks: Vec<PathBuf>,
    pub items: Vec<PathBuf>,
    pub recipes: Vec<PathBuf>,
    pub textures: Vec<PathBuf>,
}

impl ContentPaths {
    /// The core game alone.
    pub fn vanilla() -> Self {
        Self {
            blocks: vec![find_data_dir("blocks")],
            items: vec![find_data_dir("items")],
            recipes: vec![find_data_dir("recipes")],
            textures: vec![find_data_dir("textures").join("blocks")],
        }
    }

    /// The core game with `mods` (mod folders, in load order) laid over it.
    pub fn with_mods(mut self, mods: &[PathBuf]) -> Self {
        for m in mods {
            self.blocks.push(m.join("blocks"));
            self.items.push(m.join("items"));
            self.recipes.push(m.join("recipes"));
            self.textures.insert(0, m.join("textures").join("blocks"));
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
}

/// Builds content from `paths`. Panics, naming the file, on malformed content,
/// like the loaders it calls - use [`try_build_content`] for content that isn't
/// the core game's.
pub fn build_content(paths: &ContentPaths) -> Content {
    let mut registry = BlockRegistry::load_from_dirs(&paths.blocks);
    let mut items = ItemRegistry::load_from_dirs(&registry, &paths.items).with_tiers(TierBook::with_defaults());
    // A texture name with no painter gets the "missing texture" placeholder
    // instead of `compile()` panicking - see `Painters::ensure_registered`.
    let mut painters = default_painters();
    for name in registry.texture_names().collect::<HashSet<_>>() {
        painters.ensure_registered(&name);
    }
    for name in items.texture_names() {
        painters.ensure_registered(name);
    }
    let mut atlas = build_atlas_in(&painters, &paths.textures);
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
    let tables = registry.compile(&atlas.indices, atlas.tile_size);
    Content { registry, items, recipes, atlas, icons, tables, texture_status, missing_textures }
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A scratch directory removed on drop; `mod_dir` makes a mod folder in it.
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
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn with(mods: &[&Scratch]) -> ContentPaths {
        ContentPaths::vanilla().with_mods(&mods.iter().map(|m| m.0.clone()).collect::<Vec<_>>())
    }

    fn tile_pixel(c: &Content, name: &str) -> [u8; 4] {
        let index = c.atlas.indices[name] as usize;
        let (tiles, size) = (crate::config::ATLAS_TILES, c.atlas.tile_size);
        let (tx, ty) = (index % tiles, index / tiles);
        let at = ((ty * size) * (tiles * size) + tx * size) * 4;
        c.atlas.pixels[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn a_mod_can_change_one_property_of_a_core_block_and_leave_the_rest() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.file("blocks/dirt.json", r#"{"id": "dirt", "hardness": 8.0}"#);
        let modded = build_content(&with(&[&m]));
        let (a, b) = (vanilla.registry.def(vanilla.registry.id("dirt")), modded.registry.def(modded.registry.id("dirt")));
        assert_eq!(b.hardness, 8.0);
        assert_eq!(a.hardness, 0.5);
        // Everything it didn't mention is dirt's own.
        assert_eq!((b.tool, b.tool_response, &b.name), (a.tool, a.tool_response, &a.name));
    }

    #[test]
    fn a_null_in_a_mod_puts_a_key_back_to_its_default() {
        let m = Scratch::new();
        m.file("blocks/dirt.json", r#"{"id": "dirt", "tool": null}"#);
        let c = build_content(&with(&[&m]));
        assert_eq!(c.registry.def(c.registry.id("dirt")).tool, None, "dirt no longer wants a shovel");
    }

    #[test]
    fn the_blocks_core_files_define_keep_their_ids_when_mods_add_blocks() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.file("blocks/battery.json", r#"{"id": "battery", "hardness": 2.0}"#);
        let modded = build_content(&with(&[&m]));
        for (id, def) in vanilla.registry.defs.iter().enumerate().filter(|(_, d)| d.layered.is_none()) {
            assert_eq!(modded.registry.def(id as u16).id, def.id, "block {id}");
        }
        // The derived `pebble@water`-style variants come after every file's
        // block, so they may shift - harmless, since nothing saves a block id
        // as a number (saves, snapshots and edits all store the name).
        assert!(modded.registry.by_name("battery").is_ok());
        assert!(vanilla.registry.by_name("battery").is_err());
    }

    #[test]
    fn a_mod_adds_an_item_and_a_recipe_and_can_replace_a_core_recipe_by_name() {
        let m = Scratch::new();
        m.file("items/widget.json", r#"{"id": "widget"}"#)
            .file("recipes/widget.json", r#"{"inputs": [{"item": "stick", "count": 2}], "output": {"item": "widget", "count": 1}}"#)
            // Same file name as the core recipe: replaces it.
            .file("recipes/planks.json", r#"{"inputs": [{"item": "stick", "count": 9}], "output": {"item": "planks", "count": 1}}"#);
        let c = build_content(&with(&[&m]));
        assert!(c.items.by_name("widget").is_some());
        assert!(c.recipes.recipes.iter().any(|r| r.id == "widget"));
        let planks: Vec<_> = c.recipes.recipes.iter().filter(|r| r.id == "planks").collect();
        assert_eq!(planks.len(), 1, "replaced, not duplicated");
        assert_eq!(planks[0].inputs[0].count, 9);
    }

    #[test]
    fn a_mod_texture_reskins_a_core_block_and_taking_it_off_restores_it() {
        let vanilla = build_content(&ContentPaths::vanilla());
        let m = Scratch::new();
        m.png("textures/blocks/dirt.png", [1, 2, 3, 255]);
        let modded = build_content(&with(&[&m]));
        assert_eq!(tile_pixel(&modded, "dirt"), [1, 2, 3, 255]);
        assert_ne!(tile_pixel(&vanilla, "dirt"), [1, 2, 3, 255]);
        assert_eq!(tile_pixel(&build_content(&ContentPaths::vanilla()), "dirt"), tile_pixel(&vanilla, "dirt"));
    }

    #[test]
    fn with_several_mods_the_later_one_wins() {
        let (a, b) = (Scratch::new(), Scratch::new());
        a.file("blocks/dirt.json", r#"{"id": "dirt", "hardness": 3.0, "name": "Mud"}"#).png("textures/blocks/dirt.png", [9, 9, 9, 255]);
        b.file("blocks/dirt.json", r#"{"id": "dirt", "hardness": 6.0}"#).png("textures/blocks/dirt.png", [7, 7, 7, 255]);
        let c = build_content(&with(&[&a, &b]));
        let dirt = c.registry.def(c.registry.id("dirt"));
        assert_eq!(dirt.hardness, 6.0, "b is laid over a");
        assert_eq!(dirt.name, "Mud", "but keeps what a changed and b didn't");
        assert_eq!(tile_pixel(&c, "dirt"), [7, 7, 7, 255]);
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
        m.file("blocks/dirt.json", r#"{"id": "dirt", "hardness": 20.0}"#)
            .file("recipes/planks.json", r#"{"inputs": [{"item": "stick", "count": 9}], "output": {"item": "planks", "count": 1}}"#)
            .png("textures/blocks/dirt.png", [1, 1, 1, 255]);
        let _ = build_content(&with(&[&m]));
        assert!(before == snapshot(), "a core file changed");
    }

    #[test]
    fn a_broken_mod_is_an_error_naming_the_file_not_a_crash() {
        let m = Scratch::new();
        m.file("blocks/oops.json", r#"{"id": "oops", "hardness": "very"}"#);
        let err = try_build_content(&with(&[&m])).err().expect("refused");
        assert!(err.contains("oops.json"), "{err}");
        let m2 = Scratch::new();
        m2.file("recipes/bad.json", r#"{"inputs": [{"item": "nope", "count": 1}], "output": {"item": "stick", "count": 1}}"#);
        assert!(try_build_content(&with(&[&m2])).err().expect("refused").contains("nope"));
    }

    /// The example mod in `docs/` is what the docs tell people to copy, so it has
    /// to keep working: it loads, does what its description says, and is a mod.
    #[test]
    fn the_example_mod_in_the_docs_works() {
        let path = PathBuf::from("docs").join("example-mod");
        assert_eq!(crate::mods::read_mod(&path).unwrap().name, "Example mod", "it is a valid mod");
        let c = build_content(&ContentPaths::vanilla().with_mods(&[path]));
        assert_eq!(c.registry.def(c.registry.id("dirt")).hardness, 8.0, "dirt is slow to dig");
        assert!(c.registry.by_name("amber").is_ok(), "amber was added");
        assert!(c.recipes.recipes.iter().any(|r| r.id == "amber"), "and a recipe for it");
    }
}
