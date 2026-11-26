//! How content from the core game and from mods is combined, and how names are
//! kept apart.
//!
//! ## Namespaces
//!
//! Every mod has an **id** (`mod.json`'s `"id"`, required), and everything it
//! adds lives under it: a mod `granite_pack` that defines a block `granite` adds
//! `granite_pack:granite`. Two mods can each add a `granite` without clashing. The
//! core game is the namespace [`CORE_NS`] (`craftmjne`), but its names stay bare -
//! `dirt`, not `craftmjne:dirt` - so existing saves and code keep working;
//! `craftmjne:dirt` is accepted as another way to write it.
//!
//! Inside a mod's files:
//!
//! - A **new** thing is written with its plain name (`"id": "granite"`) and gets
//!   the mod's prefix.
//! - A **reference** to another thing (a drop, a recipe input, a texture, a tier)
//!   written plainly means "this mod's, if it has one by that name, otherwise the
//!   core game's". Write `other_mod:thing` or `craftmjne:thing` to be explicit.
//! - To **change** something that already exists, give its full name:
//!   `{"id": "craftmjne:dirt", "hardness": 8}` patches the core game's dirt, and
//!   `{"id": "other_mod:granite", ...}` patches another mod's block.
//!
//! ## Layering
//!
//! Files that end up with the same full id are combined by [`merge_json`], later
//! sources over earlier ones; the core game is only ever read.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

/// The core game's namespace.
pub const CORE_NS: &str = "craftmjne";

/// The names a mod defines itself, so a plain reference in its files can be
/// told apart from one meant for the core game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locals {
    /// Block and item ids (they share one namespace: a block's item has its id).
    pub things: HashSet<String>,
    pub tiers: HashSet<String>,
    pub rarities: HashSet<String>,
    /// Texture names (file stems under `textures/blocks/`).
    pub textures: HashSet<String>,
}

fn json_ids(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return HashSet::new() };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter_map(|v| v.get("id").and_then(|i| i.as_str()).map(str::to_string))
        .filter(|id| !id.contains(':'))
        .collect()
}

impl Locals {
    /// What the mod in `root` defines.
    pub fn scan(root: &Path) -> Self {
        let mut things = json_ids(&root.join("blocks"));
        things.extend(json_ids(&root.join("items")));
        let textures = std::fs::read_dir(root.join(crate::packs::MOD_PACK_DIR).join("textures").join("blocks"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "png"))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
            .collect();
        Self { things, tiers: json_ids(&root.join("tiers")), rarities: json_ids(&root.join("rarities")), textures }
    }
}

/// One directory of one kind of content (blocks, items, ...), and whose it is.
#[derive(Clone, Debug)]
pub struct KindDir {
    /// [`CORE_NS`] for the core game, else the mod's (assigned) id.
    pub ns: String,
    pub dir: PathBuf,
    pub locals: Arc<Locals>,
    /// Whether this directory may also hold *other* namespaces' files in `<ns>/`
    /// subfolders (a re-skin of someone else's thing). True for mods and for a player's
    /// resource packs; false only for the game's own built-in files.
    pub extends: bool,
}

impl KindDir {
    /// A directory of the core game's.
    pub fn core(dir: impl Into<PathBuf>) -> Self {
        Self { ns: CORE_NS.to_string(), dir: dir.into(), locals: Arc::new(Locals::default()), extends: false }
    }

    /// A directory of a player's resource pack: plain names mean the core game's (as in
    /// the game's own folder), `<ns>/<name>` re-skins a mod's.
    pub fn pack(dir: impl Into<PathBuf>) -> Self {
        Self { extends: true, ..Self::core(dir) }
    }

    /// Whether files here may change what a block *does* (its collision and hitbox) rather
    /// than only how it looks: the game's own and a mod's resource pack may, a player's
    /// standalone pack may not.
    pub fn may_change_collision(&self) -> bool {
        !(self.is_core() && self.extends)
    }

    pub fn is_core(&self) -> bool {
        self.ns == CORE_NS
    }

    /// The full id a plain id written in this directory's files gets.
    pub fn qualify_new(&self, raw: &str) -> String {
        if raw.contains(':') {
            return normalize(raw);
        }
        if self.is_core() {
            raw.to_string()
        } else {
            format!("{}:{raw}", self.ns)
        }
    }

    /// A reference written in this directory's files: this mod's thing if it has
    /// one called that (`pick` chooses which kind of name to look in), else as written.
    pub fn qualify_ref(&self, raw: &str, pick: fn(&Locals) -> &HashSet<String>) -> String {
        if raw.contains(':') {
            return normalize(raw);
        }
        if !self.is_core() && pick(&self.locals).contains(raw) {
            format!("{}:{raw}", self.ns)
        } else {
            raw.to_string()
        }
    }
}

/// `craftmjne:dirt` is `dirt`; anything else is left alone.
pub fn normalize(name: &str) -> String {
    match name.split_once(':') {
        Some((ns, rest)) if ns == CORE_NS => rest.to_string(),
        _ => name.to_string(),
    }
}

/// Splits a full name into (namespace, local name): `dirt` is (`craftmjne`, `dirt`).
pub fn split_name(name: &str) -> (&str, &str) {
    name.split_once(':').unwrap_or((CORE_NS, name))
}

/// Patches `base` with `patch`, the way a mod patches a core file: objects merge
/// key by key (recursively), anything else is replaced, and a `null` removes the
/// key so the field falls back to its default. Neither input is a file; the
/// core files are never written.
pub fn merge_json(base: &mut Value, patch: &Value) {
    match (base, patch) {
        (Value::Object(b), Value::Object(p)) => {
            for (key, value) in p {
                if value.is_null() {
                    b.remove(key);
                } else if let Some(existing) = b.get_mut(key) {
                    merge_json(existing, value);
                } else {
                    b.insert(key.clone(), value.clone());
                }
            }
        }
        (base, patch) => *base = patch.clone(),
    }
}

/// The `*.json` files in `dirs` in order (a directory that doesn't exist is
/// skipped, except the first when `first_required`), each directory sorted by name.
pub fn json_files(dirs: &[KindDir], what: &str) -> Vec<(usize, PathBuf)> {
    let mut all = Vec::new();
    for (i, kd) in dirs.iter().enumerate() {
        let entries = match std::fs::read_dir(&kd.dir) {
            Ok(entries) => entries,
            Err(err) if i == 0 && kd.is_core() => {
                panic!("could not read {what} from {:?}: {err} (expected one *.json file each)", kd.dir)
            }
            Err(_) => continue,
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();
        all.extend(paths.into_iter().map(|p| (i, p)));
    }
    all
}

/// Reads every file in `dirs` and combines those that end up with the same full
/// id by [`merge_json`], later ones patching earlier ones. Returns one entry per
/// id in order of first appearance, with the path of the last file that touched
/// it. `qualify` rewrites a file's references (drops, textures, ...) from the
/// point of view of the directory it came from, before merging.
///
/// A file whose id is written with someone else's namespace is a *patch* and its
/// target has to exist (panics naming the file otherwise - mod content is loaded
/// through `try_build_content`, which turns that into an error message).
pub fn overlay_by_id(
    dirs: &[KindDir],
    what: &str,
    scope: Option<crate::tags::Scope>,
    qualify: impl Fn(&KindDir, &mut Value),
) -> Vec<(PathBuf, Value)> {
    let mut order: Vec<String> = Vec::new();
    let mut merged: HashMap<String, (PathBuf, Value)> = HashMap::new();
    // (id, which directory) pairs seen: the same id twice in one folder is a
    // mistake, where the same id in two sources is a patch.
    let mut seen: HashSet<(String, usize)> = HashSet::new();
    for (i, path) in json_files(dirs, what) {
        let kd = &dirs[i];
        let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
        if let Some(scope) = scope {
            crate::tags::warn_unknown_tags(&path, &text, scope);
        }
        let mut value: Value =
            serde_json::from_str(&text).unwrap_or_else(|err| panic!("failed to parse {path:?}: {err}"));
        let raw = value
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{path:?} needs an \"id\""))
            .to_string();
        let id = kd.qualify_new(&raw);
        let patching_other = raw.contains(':') && split_name(&id).0 != kd.ns;
        if !seen.insert((id.clone(), i)) {
            panic!("{path:?}: \"{id}\" is already defined in this folder");
        }
        qualify(kd, &mut value);
        value["id"] = Value::String(id.clone());
        match merged.get_mut(&id) {
            Some((last, base)) => {
                merge_json(base, &value);
                *last = path;
            }
            None if patching_other => {
                panic!("{path:?} changes \"{id}\", which doesn't exist (is that mod on, and loaded before this one?)")
            }
            None => {
                order.push(id.clone());
                merged.insert(id, (path, value));
            }
        }
    }
    order.into_iter().filter_map(|id| merged.remove(&id)).collect()
}

/// Rewrites a string field of `value` (if present) through `f`.
pub fn rewrite_str(value: &mut Value, key: &str, f: impl Fn(&str) -> String) {
    if let Some(Value::String(s)) = value.get_mut(key) {
        *s = f(s);
    }
}

/// Qualifies the texture names in a `textures`/`overlay` object (`all`, `top`,
/// `bottom`, `side`).
pub fn qualify_face_textures(kd: &KindDir, value: &mut Value, key: &str) {
    if let Some(Value::Object(faces)) = value.get_mut(key) {
        for face in faces.values_mut() {
            if let Value::String(s) = face {
                *s = kd.qualify_ref(s, |l| &l.textures);
            }
        }
    }
}

/// A block file's references, from the point of view of the mod it came from.
pub fn qualify_block_refs(kd: &KindDir, value: &mut Value) {
    let thing = |s: &str| kd.qualify_ref(s, |l| &l.things);
    rewrite_str(value, "freezes_to", thing);
    rewrite_str(value, "upper", thing);
    match value.get_mut("drops") {
        Some(Value::String(s)) => *s = thing(s),
        Some(d @ Value::Object(_)) => rewrite_str(d, "item", thing),
        _ => {}
    }
    qualify_face_textures(kd, value, "textures");
    qualify_face_textures(kd, value, "overlay");
}

/// An item file's references.
pub fn qualify_item_refs(kd: &KindDir, value: &mut Value) {
    rewrite_str(value, "texture", |s| kd.qualify_ref(s, |l| &l.textures));
    if let Some(tool) = value.get_mut("tool") {
        rewrite_str(tool, "tier", |s| kd.qualify_ref(s, |l| &l.tiers));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn kd(ns: &str, things: &[&str], textures: &[&str]) -> KindDir {
        let locals = Locals {
            things: things.iter().map(|s| s.to_string()).collect(),
            textures: textures.iter().map(|s| s.to_string()).collect(),
            ..Locals::default()
        };
        KindDir { ns: ns.to_string(), dir: PathBuf::new(), locals: Arc::new(locals), extends: ns != CORE_NS }
    }

    #[test]
    fn core_names_stay_bare_and_the_core_prefix_is_another_way_to_write_them() {
        let core = KindDir::core("x");
        assert_eq!(core.qualify_new("dirt"), "dirt");
        assert_eq!(core.qualify_new("craftmjne:dirt"), "dirt");
        assert_eq!(normalize("craftmjne:dirt"), "dirt");
        assert_eq!(normalize("mymod:dirt"), "mymod:dirt");
        assert_eq!(split_name("dirt"), (CORE_NS, "dirt"));
        assert_eq!(split_name("mymod:granite"), ("mymod", "granite"));
    }

    #[test]
    fn a_mods_new_things_get_its_prefix_and_two_mods_can_both_have_a_granite() {
        let (a, b) = (kd("alpha", &["granite"], &[]), kd("beta", &["granite"], &[]));
        assert_eq!(a.qualify_new("granite"), "alpha:granite");
        assert_eq!(b.qualify_new("granite"), "beta:granite");
        assert_eq!(a.qualify_new("craftmjne:dirt"), "dirt", "the core's, explicitly");
        assert_eq!(a.qualify_new("beta:granite"), "beta:granite", "another mod's, explicitly");
    }

    #[test]
    fn a_plain_reference_means_this_mods_thing_if_it_has_one_else_the_cores() {
        let m = kd("alpha", &["granite"], &["granite_top"]);
        assert_eq!(m.qualify_ref("granite", |l| &l.things), "alpha:granite");
        assert_eq!(m.qualify_ref("dirt", |l| &l.things), "dirt");
        assert_eq!(m.qualify_ref("beta:granite", |l| &l.things), "beta:granite");
        assert_eq!(m.qualify_ref("craftmjne:granite", |l| &l.things), "granite");
        assert_eq!(m.qualify_ref("granite_top", |l| &l.textures), "alpha:granite_top");
    }

    #[test]
    fn block_references_are_qualified_in_every_place_a_block_names_something() {
        let m = kd("alpha", &["granite", "chip"], &["granite_side"]);
        let mut v = json!({
            "id": "granite", "upper": "granite", "freezes_to": "ice",
            "drops": {"item": "chip", "count": 2},
            "textures": {"side": "granite_side", "top": "stone"},
            "overlay": {"side": "granite_side"}
        });
        qualify_block_refs(&m, &mut v);
        assert_eq!(v["upper"], "alpha:granite");
        assert_eq!(v["freezes_to"], "ice");
        assert_eq!(v["drops"]["item"], "alpha:chip");
        assert_eq!(v["textures"]["side"], "alpha:granite_side");
        assert_eq!(v["textures"]["top"], "stone");
        assert_eq!(v["overlay"]["side"], "alpha:granite_side");
        let mut s = json!({"id": "x", "drops": "chip"});
        qualify_block_refs(&m, &mut s);
        assert_eq!(s["drops"], "alpha:chip");
        let mut f = json!({"id": "x", "drops": false});
        qualify_block_refs(&m, &mut f);
        assert_eq!(f["drops"], false);
    }

    #[test]
    fn merging_patches_keys_recursively_and_null_removes() {
        let mut base = json!({"id": "dirt", "hardness": 0.5, "textures": {"top": "a", "side": "b"}, "tool": "shovel"});
        merge_json(&mut base, &json!({"hardness": 8, "textures": {"side": "c"}, "tool": null}));
        assert_eq!(base, json!({"id": "dirt", "hardness": 8, "textures": {"top": "a", "side": "c"}}));
    }
}
