//! Block models from resource packs: how a block *looks*, as a set of textured boxes.
//!
//! A model is `models/blocks/<block>.json` in a resource pack (see `packs.rs` for where those
//! live and whose names are whose):
//!
//! ```json
//! {
//!   "boxes": [
//!     { "from": [0, 0, 0], "to": [16, 8, 16],
//!       "textures": { "all": "stone", "top": "stone_top" } },
//!     { "from": [6, 8, 6], "to": [10, 14, 10], "textures": { "all": "planks" } }
//!   ],
//!   "collision": "block"
//! }
//! ```
//! `from`/`to` are in sixteenths of a block, inside the block's own cell (0 to 16). A box face
//! with no texture isn't drawn, so a pack can leave out faces nobody sees. A texture on a face
//! comes from `east`/`west`/`top`/`bottom`/`south`/`north` first, then `side` (the four sides),
//! then `all`. Textures are block tiles by name (`textures/blocks/`), and a name no pack
//! supplies gets the usual "missing texture" placeholder. Each face shows the part of its
//! texture that matches where the face sits in the block (so the lower half of a slab shows the
//! lower half of the side texture), the way Minecraft's automatic UVs do.
//!
//! **A model is a picture.** Whatever a player's pack says, the block still collides, is mined
//! and is targeted as the cube (or whatever the block's own definition says). The one exception
//! is a model inside a **mod's** resource pack, which may add `"collision": "model"`: then the
//! model's own boxes become the block's collision and its hitbox. The highest mod pack that has a
//! model for a block decides: `"model"` there replaces the collision, anything else (or no
//! `collision`) leaves the block's own, even if a lower mod said otherwise. A player's pack
//! can still *draw* such a block differently; it just can't move its collision.
//!
//! The model replaces the block's drawing only. Neighbours still hide faces against a model
//! that fills its whole cell (a plain retextured cube costs nothing); a model that doesn't fill
//! it lets the faces behind show, so a half block doesn't leave a hole in the wall beside it.
//! Lighting, light blocking and everything else physical still come from the block definition.
//! Inventory icons and dropped items use the block's own textures, not the model.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

use crate::blocks::{BlockId, BlockRegistry};
use crate::overlay::KindDir;
use crate::packs::file_in;

/// Boxes beyond this are ignored (a model is a handful of boxes, not a mesh).
pub const MAX_BOXES: usize = 64;

/// Face order, as everywhere: 0:+x east, 1:-x west, 2:+y top, 3:-y bottom, 4:+z south, 5:-z north.
pub const FACE_NAMES: [&str; 6] = ["east", "west", "top", "bottom", "south", "north"];

#[derive(Deserialize, Default, Debug)]
struct FaceNames {
    all: Option<String>,
    side: Option<String>,
    top: Option<String>,
    bottom: Option<String>,
    east: Option<String>,
    west: Option<String>,
    south: Option<String>,
    north: Option<String>,
}

impl FaceNames {
    /// The texture name for face `f`, most specific wins.
    fn for_face(&self, f: usize) -> Option<&str> {
        let own = [&self.east, &self.west, &self.top, &self.bottom, &self.south, &self.north][f];
        let side = if matches!(f, 0 | 1 | 4 | 5) { self.side.as_ref() } else { None };
        own.as_ref().or(side).or(self.all.as_ref()).map(String::as_str)
    }
}

#[derive(Deserialize, Debug)]
struct BoxFile {
    from: [f32; 3],
    to: [f32; 3],
    #[serde(default)]
    textures: FaceNames,
}

#[derive(Deserialize, Debug)]
struct ModelFile {
    boxes: Vec<BoxFile>,
    #[serde(default)]
    collision: Option<String>,
}

/// One box of a model, in block units (0 to 1), with the (qualified) texture name of each face.
#[derive(Clone, Debug, PartialEq)]
pub struct RawBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub names: [Option<String>; 6],
}

/// A model as read from a file.
#[derive(Clone, Debug, PartialEq)]
pub struct RawModel {
    pub boxes: Vec<RawBox>,
    /// `"collision": "model"` was set.
    pub collision_model: bool,
}

/// Reads one model file's text. `kd` qualifies texture names the way its block files would.
pub fn parse_model(text: &str, kd: &KindDir) -> Result<RawModel, String> {
    let file: ModelFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.boxes.is_empty() {
        return Err("it has no boxes".to_string());
    }
    let collision_model = match file.collision.as_deref().map(str::trim) {
        None | Some("") | Some("block") => false,
        Some("model") => true,
        Some(other) => return Err(format!("\"collision\" must be \"block\" or \"model\", not {other:?}")),
    };
    let mut boxes = Vec::new();
    for (i, b) in file.boxes.iter().take(MAX_BOXES).enumerate() {
        for a in 0..3 {
            let (lo, hi) = (b.from[a], b.to[a]);
            if !(lo.is_finite() && hi.is_finite()) {
                return Err(format!("box {i} has a number that isn't a number"));
            }
            if lo > hi {
                return Err(format!("box {i}: \"from\" is past \"to\" on the {} axis", ["x", "y", "z"][a]));
            }
            if lo < 0.0 || hi > 16.0 {
                return Err(format!("box {i} sticks out of the block (0 to 16 on every axis)"));
            }
        }
        let min = b.from.map(|v| v / 16.0);
        let max = b.to.map(|v| v / 16.0);
        let names = std::array::from_fn(|f| b.textures.for_face(f).map(|n| kd.qualify_ref(n, |l| &l.textures)));
        boxes.push(RawBox { min, max, names });
    }
    Ok(RawModel { boxes, collision_model })
}

/// Every model found for the registry's blocks.
#[derive(Default, Debug)]
pub struct LoadedModels {
    /// What each block looks like, where a pack changed it.
    pub visual: HashMap<BlockId, RawModel>,
    /// Blocks whose collision and hitbox a mod's model replaced.
    pub collision: HashMap<BlockId, RawModel>,
    /// Things worth telling the player or the pack's author (skipped files).
    pub notes: Vec<String>,
}

impl LoadedModels {
    /// Every texture name any model asks for (they all need a tile in the atlas).
    pub fn texture_names(&self) -> HashSet<String> {
        self.visual.values().flat_map(|m| m.boxes.iter()).flat_map(|b| b.names.iter().flatten()).cloned().collect()
    }
}

fn read(path: &Path, kd: &KindDir, notes: &mut Vec<String>) -> Option<RawModel> {
    match std::fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|t| parse_model(&t, kd)) {
        Ok(m) => Some(m),
        Err(why) => {
            notes.push(format!("{}: {why} (the model is ignored)", path.display()));
            None
        }
    }
}

/// Finds the model for every block in `dirs` (lowest priority first).
///
/// The *look* comes from the highest layer that has a readable model for the block. The
/// *collision* comes from the highest layer **allowed to change collision** (the game's, or a
/// mod's pack - not a player's) that has a readable model, and only if that one says
/// `"collision": "model"`.
pub fn load_block_models(registry: &BlockRegistry, dirs: &[KindDir]) -> LoadedModels {
    let mut out = LoadedModels::default();
    for (i, def) in registry.defs.iter().enumerate().skip(1) {
        if def.layered.is_some() {
            continue; // `pebble@water` is drawn as its pebble
        }
        let id = i as BlockId;
        let mut visual_done = false;
        let mut collision_done = false;
        for kd in dirs.iter().rev() {
            if visual_done && collision_done {
                break;
            }
            let Some(path) = file_in(kd, &def.id, "json") else { continue };
            let Some(model) = read(&path, kd, &mut out.notes) else { continue };
            if !visual_done {
                out.visual.insert(id, model.clone());
                visual_done = true;
            }
            if !collision_done && kd.may_change_collision() {
                collision_done = true;
                if model.collision_model {
                    out.collision.insert(id, model);
                }
            }
        }
    }
    out
}

/// A box ready to draw: block units, and an atlas tile per face.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub tiles: [Option<u16>; 6],
}

/// A model ready for the mesher.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledModel {
    pub boxes: Vec<CompiledBox>,
    /// One box fills the whole cell with every face textured: neighbours may hide faces
    /// against it exactly as against a cube.
    pub fills_cell: bool,
}

pub fn compile_model(raw: &RawModel, atlas: &HashMap<String, u16>) -> CompiledModel {
    let boxes: Vec<CompiledBox> = raw
        .boxes
        .iter()
        .map(|b| CompiledBox { min: b.min, max: b.max, tiles: std::array::from_fn(|f| b.names[f].as_ref().and_then(|n| atlas.get(n).copied())) })
        .collect();
    let fills_cell = boxes.iter().any(|b| b.min == [0.0; 3] && b.max == [1.0; 3] && b.tiles.iter().all(Option::is_some));
    CompiledModel { boxes, fills_cell }
}

/// A collision box, in block units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CollisionBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// The boxes of a model, as collision/hitbox shapes (empty boxes dropped: a flat box has no volume).
pub fn collision_boxes(raw: &RawModel) -> Vec<CollisionBox> {
    raw.boxes
        .iter()
        .filter(|b| (0..3).all(|a| b.max[a] > b.min[a]))
        .map(|b| CollisionBox { min: b.min, max: b.max })
        .collect()
}

/// Where a face's corner lands in its texture: `(u, v)` in 0..1 for a point `p` (block units) on
/// face `f`, so a texture is pinned to the block and a sub-box shows the matching part of it.
/// Matches the full-cube corner layout the mesher's `FACES` use (u = along the face's first
/// tangent, v up it), derived from that layout rather than restated.
pub fn face_uv(face: usize, p: [f32; 3]) -> [f32; 2] {
    // For each face: which axis drives u, whether it runs backwards, and the same for v.
    // (Read off the corner tables in mesher.rs: +x has u = 1 - z, v = y; and so on.)
    const MAP: [([usize; 1], bool, [usize; 1], bool); 6] = [
        ([2], true, [1], false),  // +x east: u = 1 - z, v = y
        ([2], false, [1], false), // -x west: u = z, v = y
        ([0], true, [2], false),  // +y top: u = 1 - x, v = z
        ([0], false, [2], true),  // -y bottom: u = x, v = 1 - z
        ([0], false, [1], false), // +z south: u = x, v = y
        ([0], true, [1], false),  // -z north: u = 1 - x, v = y
    ];
    let (ua, ur, va, vr) = MAP[face];
    let u = p[ua[0]];
    let v = p[va[0]];
    [if ur { 1.0 - u } else { u }, if vr { 1.0 - v } else { v }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn core() -> KindDir {
        KindDir::pack("x")
    }

    fn tmp(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("craftmjne-models-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const SLAB: &str = r#"{"boxes": [{"from": [0, 0, 0], "to": [16, 8, 16], "textures": {"all": "stone", "top": "stone_top"}}]}"#;

    #[test]
    fn a_model_is_boxes_in_sixteenths_with_the_most_specific_texture_winning() {
        let m = parse_model(SLAB, &core()).unwrap();
        assert_eq!(m.boxes.len(), 1);
        let b = &m.boxes[0];
        assert_eq!((b.min, b.max), ([0.0; 3], [1.0, 0.5, 1.0]));
        assert_eq!(b.names[2].as_deref(), Some("stone_top"), "top beats all");
        assert_eq!(b.names[0].as_deref(), Some("stone"));
        assert_eq!(b.names[3].as_deref(), Some("stone"), "all covers the bottom too");
        assert!(!m.collision_model);
        let text = r#"{"boxes": [{"from": [0,0,0], "to": [16,16,16], "textures": {"all": "a", "side": "b", "north": "c"}}]}"#;
        let b = &parse_model(text, &core()).unwrap().boxes[0];
        assert_eq!((b.names[0].as_deref(), b.names[5].as_deref(), b.names[2].as_deref()), (Some("b"), Some("c"), Some("a")));
    }

    #[test]
    fn a_face_with_no_texture_is_left_out() {
        let text = r#"{"boxes": [{"from": [0,0,0], "to": [16,16,16], "textures": {"top": "t"}}]}"#;
        let b = &parse_model(text, &core()).unwrap().boxes[0];
        assert_eq!(b.names.iter().filter(|n| n.is_some()).count(), 1);
    }

    #[test]
    fn bad_models_are_errors_not_crashes() {
        for bad in [
            "{nope",
            r#"{"boxes": []}"#,
            r#"{"boxes": [{"from": [0,0,0], "to": [16,16,17]}]}"#,
            r#"{"boxes": [{"from": [-1,0,0], "to": [16,16,16]}]}"#,
            r#"{"boxes": [{"from": [8,0,0], "to": [4,16,16]}]}"#,
            r#"{"boxes": [{"from": [0,0,0], "to": [16,16,16]}], "collision": "wat"}"#,
        ] {
            assert!(parse_model(bad, &core()).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_models_texture_names_are_qualified_like_a_blocks() {
        use std::sync::Arc;
        let mut locals = crate::overlay::Locals::default();
        locals.textures.insert("granite".to_string());
        let kd = KindDir { ns: "g".into(), dir: "x".into(), locals: Arc::new(locals), extends: true };
        let text = r#"{"boxes": [{"from": [0,0,0], "to": [16,16,16], "textures": {"top": "granite", "side": "dirt", "bottom": "craftmjne:stone"}}]}"#;
        let b = &parse_model(text, &kd).unwrap().boxes[0];
        assert_eq!(b.names[2].as_deref(), Some("g:granite"), "the mod's own tile");
        assert_eq!(b.names[0].as_deref(), Some("dirt"), "anything it doesn't define is the core game's");
        assert_eq!(b.names[3].as_deref(), Some("stone"), "craftmjne:stone is just stone");
    }

    #[test]
    fn a_full_cube_model_fills_its_cell_and_a_slab_does_not() {
        let atlas: HashMap<String, u16> = [("stone".to_string(), 3)].into();
        let cube = r#"{"boxes": [{"from": [0,0,0], "to": [16,16,16], "textures": {"all": "stone"}}]}"#;
        assert!(compile_model(&parse_model(cube, &core()).unwrap(), &atlas).fills_cell);
        assert!(!compile_model(&parse_model(SLAB, &core()).unwrap(), &atlas).fills_cell);
        let missing_face = r#"{"boxes": [{"from": [0,0,0], "to": [16,16,16], "textures": {"top": "stone"}}]}"#;
        assert!(!compile_model(&parse_model(missing_face, &core()).unwrap(), &atlas).fills_cell, "a cube with faces missing can't hide a neighbour's");
        let m = compile_model(&parse_model(SLAB, &core()).unwrap(), &atlas);
        assert_eq!(m.boxes[0].tiles[0], Some(3));
        assert_eq!(m.boxes[0].tiles[2], None, "a name the atlas doesn't have draws nothing rather than the wrong tile");
    }

    #[test]
    fn collision_boxes_drop_flat_boxes() {
        let text = r#"{"boxes": [{"from": [0,0,0], "to": [16,8,16], "textures": {"all": "a"}}, {"from": [0,8,0], "to": [16,8,16]}], "collision": "model"}"#;
        let m = parse_model(text, &core()).unwrap();
        assert!(m.collision_model);
        assert_eq!(collision_boxes(&m), vec![CollisionBox { min: [0.0; 3], max: [1.0, 0.5, 1.0] }]);
    }

    /// The UV rule must agree with the full-cube corner layout the mesher uses for ordinary faces,
    /// or a model cube would show its textures rotated or mirrored compared to the same block
    /// drawn normally. Checked against the mesher's own corner table, not a copy of it.
    #[test]
    fn face_uvs_agree_with_the_meshers_cube_corners() {
        for (f, face) in crate::mesher::face_corner_table().iter().enumerate() {
            for (pos, uv) in face {
                let got = face_uv(f, *pos);
                assert_eq!(got, *uv, "face {f} corner {pos:?}");
            }
        }
    }

    #[test]
    fn a_sub_box_shows_the_part_of_the_texture_where_it_sits() {
        // The lower half of a +z (south) face: v runs 0..0.5, u the full width.
        assert_eq!(face_uv(4, [0.0, 0.0, 1.0]), [0.0, 0.0]);
        assert_eq!(face_uv(4, [1.0, 0.5, 1.0]), [1.0, 0.5]);
    }

    fn registry() -> BlockRegistry {
        BlockRegistry::with_defaults()
    }

    fn put(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn collides(model: &str) -> String {
        format!(r#"{{"boxes": [{{"from": [0,0,0], "to": [16,8,16], "textures": {{"all": "stone"}}}}], "collision": "{model}"}}"#)
    }

    #[test]
    fn the_highest_pack_decides_the_look_and_only_a_mods_pack_can_change_collision() {
        let (modd, user) = (tmp("mod"), tmp("user"));
        put(&modd, "craftmjne/dirt.json", &collides("model"));
        put(&user, "dirt.json", SLAB); // a player's pack: plain name = core dirt, collision "block" by default
        let reg = registry();
        let dirt = reg.id("dirt");
        let dirs = vec![
            KindDir::core(tmp("empty")),
            KindDir { ns: "m".into(), dir: modd.clone(), locals: Default::default(), extends: true },
            KindDir::pack(&user),
        ];
        let loaded = load_block_models(&reg, &dirs);
        assert!(loaded.visual.contains_key(&dirt), "the player's pack draws it");
        assert_eq!(loaded.visual[&dirt].boxes[0].max[1], 0.5);
        assert!(loaded.collision.contains_key(&dirt), "the mod's model changed the collision, and the player's pack above it can't undo that");
        // Without the mod, a player's pack alone can never change collision, even asking for it.
        put(&user, "dirt.json", &collides("model"));
        let loaded = load_block_models(&reg, &[KindDir::core(tmp("e2")), KindDir::pack(&user)]);
        assert!(loaded.visual.contains_key(&dirt) && loaded.collision.is_empty(), "a player's pack is only a picture");
        for d in [modd, user] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn a_higher_mod_that_does_not_ask_for_model_collision_restores_the_blocks_own() {
        let (a, b) = (tmp("a"), tmp("b"));
        put(&a, "craftmjne/dirt.json", &collides("model"));
        put(&b, "craftmjne/dirt.json", &collides("block"));
        let kd = |ns: &str, dir: &Path| KindDir { ns: ns.into(), dir: dir.to_path_buf(), locals: Default::default(), extends: true };
        let reg = registry();
        let dirs = vec![KindDir::core(tmp("e")), kd("a", &a), kd("b", &b)];
        assert!(load_block_models(&reg, &dirs).collision.is_empty(), "the later mod says plain collision");
        let dirs = vec![KindDir::core(tmp("e")), kd("b", &b), kd("a", &a)];
        assert!(load_block_models(&reg, &dirs).collision.contains_key(&reg.id("dirt")), "and the other way round the override stands");
        for d in [a, b] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn an_unreadable_model_is_skipped_with_a_note_and_the_next_layer_down_is_used() {
        let (low, high) = (tmp("low"), tmp("high"));
        put(&low, "dirt.json", SLAB);
        put(&high, "dirt.json", "{broken");
        let reg = registry();
        let loaded = load_block_models(&reg, &[KindDir::pack(&low), KindDir::pack(&high)]);
        assert_eq!(loaded.visual[&reg.id("dirt")].boxes[0].max[1], 0.5, "fell back to the readable one");
        assert!(loaded.notes.iter().any(|n| n.contains("dirt.json")), "{:?}", loaded.notes);
        for d in [low, high] {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}
