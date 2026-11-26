//! **Definition tags**: the fixed facts about a block or item *type*, written
//! in `blocks/*.json` and `items/*.json` - `hardness`, `shape`, `emission`,
//! `throwable`, `shatters_at`, and so on. "Torches glow" is a definition tag.
//!
//! This is the first of the game's two kinds of tags. The second, *state*
//! tags (`states.rs`), belong to one placed cell and change while the game
//! runs. A definition tag is read once at startup into `Tables`; nothing in
//! play ever edits it.
//!
//! [`DEFINITION_TAGS`] is the one list of every tag a content file may use:
//! its name, which files take it, a group, and what it does. It exists so
//! that
//!
//! - a typo in a content file is *noticed* (`light_emision` used to be
//!   silently ignored, leaving a block that mysteriously doesn't glow): the
//!   loaders warn about any key not in the table, with a suggestion,
//! - the tags are discoverable in one place instead of scattered across
//!   `BlockFile`, `ItemFile` and prose, and
//! - a test can keep the list honest against the files that ship.
//!
//! Adding a tag: add its field to the file struct, then a row here.

use std::path::Path;

/// Which kinds of content file accept a tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Block,
    Item,
    /// Both - a block item reads its block file's value.
    Both,
}

/// How a tag is grouped in documentation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Identity,
    Appearance,
    World,
    Mining,
    Combat,
    Throwing,
    Breaking,
}

pub struct DefinitionTag {
    pub name: &'static str,
    pub scope: Scope,
    pub group: Group,
    pub description: &'static str,
}

const fn tag(name: &'static str, scope: Scope, group: Group, description: &'static str) -> DefinitionTag {
    DefinitionTag { name, scope, group, description }
}

use Group::*;
use Scope::*;

/// Every key a block or item file may contain.
pub const DEFINITION_TAGS: &[DefinitionTag] = &[
    tag("id", Both, Identity, "The unique name. Everything else refers to the block or item by it."),
    tag("name", Both, Identity, "The display name (defaults to the id, capitalised)."),
    tag("transparent", Block, Appearance, "no | partial | full: whether you see through it."),
    tag("texture_scheme", Block, Appearance, "How per-face texture names derive from the id."),
    tag("textures", Block, Appearance, "Explicit texture names per face (all/top/bottom/side)."),
    tag("tinted", Block, Appearance, "Which faces take the biome colour."),
    tag("overlay", Block, Appearance, "A tinted decal texture layered over a face."),
    tag("item_model", Block, Appearance, "How the inventory icon is drawn."),
    tag("custom_item_model", Block, Appearance, "Model path for item_model: custom."),
    tag("block_model", Block, Appearance, "A model file giving the block a real shape."),
    tag("texture", Item, Appearance, "The texture a plain item draws."),
    tag("shape", Block, Appearance, "cube | floor | model: its shape in the world."),
    tag("light", Block, Appearance, "How much light it emits, and what colour."),
    tag("transmission", Block, Appearance, "How much light passes through one block, per channel."),
    tag("solid", Block, World, "Blocks movement."),
    tag("selectable", Block, World, "Can be targeted by the crosshair."),
    tag("replaceable", Block, World, "Placing a block overwrites it (water)."),
    tag("fluid", Block, World, "A swimmable liquid."),
    tag("flow_distance", Block, World, "How far a fluid flows from a source."),
    tag("rotation", Block, World, "none | log: whether a placed one can face different ways (state tag: axis)."),
    tag("waterloggable", Block, World, "May share its cell with a fluid (state tag: waterlogged)."),
    tag("states", Block, World, "Which state tags a placed one has (axis, waterlogged, level). Behaviour tags imply theirs: rotation, waterloggable, fluid."),
    tag("state_defs", Block, World, "States this block declares for itself: a name and a value count (\"charge\": 8) or value names (\"mode\": [\"off\", \"on\"]). The first value is the default."),
    tag("state_storage", Block, World, "compact (default): this block's state_defs share the cell's 16-bit word. extended: they live in a per-chunk table, for far more room."),
    tag("freezes_to", Block, World, "What this fluid becomes when it freezes."),
    tag("upper", Block, World, "A two-cell-tall block: the block placed above it."),
    tag("max_stack", Both, Mining, "How many fit in one slot."),
    tag("item", Block, Mining, "Whether the block has an inventory item at all."),
    tag("breakable", Block, Mining, "Can be mined (bedrock can't)."),
    tag("hardness", Block, Mining, "How long it takes to mine."),
    tag("drops", Block, Mining, "What breaking it yields."),
    tag("tool", Both, Mining, "Blocks: the tool kind that mines it. Items: makes the item a tool ({kind, tier})."),
    tag("requires_tool", Block, Mining, "Without the right tool it drops nothing."),
    tag("sound", Block, Mining, "Sound group the block makes: stone, dirt, grass, wood, sand, gravel, glass, snow, water (or a mod's own)."),
    tag("tool_response", Block, Mining, "How much of a tool's speed-up this block takes (0..1)."),
    tag("requires_crouch", Block, Mining, "Only mined while crouching."),
    tag("hittable", Block, Combat, "A plain left click hits it instead of mining it."),
    tag("attack_cooldown", Both, Combat, "Seconds between melee hits while held."),
    tag("mass", Both, Throwing, "Kilograms."),
    tag("throwable", Both, Throwing, "Left click throws it."),
    tag("throw_energy", Both, Throwing, "Joules behind a throw."),
    tag("throw_drag", Both, Throwing, "Air resistance, kg/m."),
    tag("throw_cooldown", Both, Throwing, "Seconds between throws."),
    tag("throw_damage", Both, Throwing, "Multiplier on impact damage."),
    tag("shatters_at", Block, Breaking, "A thrown thing at least this fast (m/s) breaks it."),
    tag("shatter_keep", Block, Breaking, "Fraction of its speed a projectile keeps after shattering it."),
];

/// The tag called `name`, if there is one.
pub fn definition_tag(name: &str) -> Option<&'static DefinitionTag> {
    DEFINITION_TAGS.iter().find(|t| t.name == name)
}

fn accepts(scope: Scope, wanted: Scope) -> bool {
    scope == Scope::Both || scope == wanted
}

/// Edit distance, for "did you mean".
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb { prev } else { 1 + prev.min(row[j]).min(row[j + 1]) };
            prev = cur;
        }
    }
    row[b.len()]
}

/// A key in a content file that isn't a tag for that kind of file, with the
/// closest real tag if one is plausibly what was meant.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownTag {
    pub key: String,
    pub suggestion: Option<&'static str>,
}

/// Every top-level key of `json` that isn't a definition tag valid for
/// `scope` (`Scope::Block` or `Scope::Item`).
pub fn unknown_tags(json: &serde_json::Value, scope: Scope) -> Vec<UnknownTag> {
    let Some(object) = json.as_object() else { return Vec::new() };
    object
        .keys()
        .filter(|key| !definition_tag(key).is_some_and(|t| accepts(t.scope, scope)))
        .map(|key| {
            let suggestion = DEFINITION_TAGS
                .iter()
                .filter(|t| accepts(t.scope, scope))
                .map(|t| (distance(key, t.name), t.name))
                .filter(|(d, name)| *d <= 2.max(name.len() / 4))
                .min()
                .map(|(_, name)| name);
            UnknownTag { key: key.clone(), suggestion }
        })
        .collect()
}

/// Prints a warning for each unknown key in a content file. A warning, not a
/// panic: an unknown key is almost always a typo, but it's also what an older
/// build sees in content written for a newer one, and that should still load.
pub fn warn_unknown_tags(path: &Path, text: &str, scope: Scope) {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else { return };
    for unknown in unknown_tags(&json, scope) {
        match unknown.suggestion {
            Some(s) => eprintln!("warning: {path:?}: unknown tag \"{}\" (did you mean \"{s}\"?)", unknown.key),
            None => eprintln!("warning: {path:?}: unknown tag \"{}\"", unknown.key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::find_data_dir;
    use serde_json::json;

    #[test]
    fn names_are_unique() {
        let mut names: Vec<_> = DEFINITION_TAGS.iter().map(|t| t.name).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before);
    }

    #[test]
    fn a_typo_is_reported_with_the_tag_it_probably_meant() {
        let found = unknown_tags(&json!({"id": "torch", "light_emision": 1, "hardnes": 0.2, "wibble": 1}), Scope::Block);
        let keys: Vec<_> = found.iter().map(|u| (u.key.as_str(), u.suggestion)).collect();
        assert!(keys.contains(&("hardnes", Some("hardness"))), "{keys:?}");
        assert!(keys.contains(&("wibble", None)), "{keys:?}");
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn a_tag_only_counts_in_the_files_that_take_it() {
        // `texture` is an item tag, `shatters_at` a block tag.
        assert!(unknown_tags(&json!({"id": "x", "shatters_at": 5}), Scope::Item).iter().any(|u| u.key == "shatters_at"));
        assert!(unknown_tags(&json!({"id": "x", "texture": "t"}), Scope::Block).iter().any(|u| u.key == "texture"));
        assert!(unknown_tags(&json!({"id": "x", "shatters_at": 5}), Scope::Block).is_empty());
        assert!(unknown_tags(&json!({"id": "x", "mass": 1, "throwable": true}), Scope::Item).is_empty());
    }

    fn keys_used_in(dir: &str, scope: Scope) -> Vec<(String, UnknownTag)> {
        let mut bad = Vec::new();
        for entry in std::fs::read_dir(find_data_dir(dir)).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
                for u in unknown_tags(&json, scope) {
                    bad.push((path.display().to_string(), u));
                }
            }
        }
        bad
    }

    #[test]
    fn every_key_in_every_shipped_block_and_item_file_is_a_listed_tag() {
        let bad = keys_used_in("blocks", Scope::Block);
        assert!(bad.is_empty(), "block files use unlisted tags: {bad:?}");
        let bad = keys_used_in("items", Scope::Item);
        assert!(bad.is_empty(), "item files use unlisted tags: {bad:?}");
    }

    /// Every tag the table lists for blocks is a field `BlockFile` really
    /// reads: a mistyped value is rejected for it, where an unread key would
    /// be silently ignored. (The other direction - a field missing from the
    /// table - is caught by the shipped-files test above, once any file uses it.)
    #[test]
    fn every_listed_block_tag_is_a_field_the_block_loader_reads() {
        let probes = [json!("@@bad"), json!(12345), json!(true), json!([[1]]), json!({"__bad": 1}), json!(-1)];
        for t in DEFINITION_TAGS.iter().filter(|t| accepts(t.scope, Scope::Block) && t.name != "id") {
            let read = probes.iter().any(|bad| {
                let file = json!({"id": "probe", t.name: bad});
                crate::blocks::parse_block_file_for_test(&file.to_string()).is_err()
            });
            assert!(read, "\"{}\" is listed as a block tag but BlockFile doesn't read it", t.name);
        }
    }
}
