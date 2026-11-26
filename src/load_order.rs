//! One load order for mods and resource packs.
//!
//! A world's content is a stack, lowest first: **the game itself (always the bottom, never movable)**,
//! then every mod of the world, the world's own resource packs and the player's global packs,
//! interleaved however the order says. A later entry wins where two supply the same thing, so a pack
//! can retexture a mod, a mod can override a pack, and an add-on mod sits after the mod it builds on.
//!
//! - **Auto** ([`auto_order`]): mods are sorted so each comes after the mods it names in
//!   `"load_after"` and after any mod whose content it patches (ties by folder name); then the
//!   world's packs; then the global packs on top, in the player's global order.
//! - **Manual**: the player moves entries ([`shift`]) and the list is saved with the world
//!   (`WorldMeta::load_order`). It is reconciled against what exists now ([`resolve`]): entries that
//!   are gone drop out, new mods go in just below the global packs, new global packs go on top.
//!   Pressing Auto forgets the manual order, which puts the global packs back on top.
//!
//! Everything here is pure; `content.rs` turns the result into layers and `world.rs` rebuilds with it.

use std::path::PathBuf;

use crate::mods::ModSource;
use crate::packs::{InstalledPacks, PackSelection};

/// One movable entry, by folder name.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Entry {
    /// A mod of this world (its folder).
    Mod(String),
    /// A resource pack turned on for this world only.
    Pack(String),
    /// A resource pack turned on globally.
    Global(String),
}

impl Entry {
    /// The text saved in `meta.json`: `mod:<folder>`, `pack:<folder>` or `global:<folder>`.
    pub fn key(&self) -> String {
        match self {
            Entry::Mod(f) => format!("mod:{f}"),
            Entry::Pack(f) => format!("pack:{f}"),
            Entry::Global(f) => format!("global:{f}"),
        }
    }

    pub fn parse(key: &str) -> Option<Entry> {
        let (kind, folder) = key.split_once(':')?;
        let folder = folder.to_string();
        match kind {
            "mod" => Some(Entry::Mod(folder)),
            "pack" => Some(Entry::Pack(folder)),
            "global" => Some(Entry::Global(folder)),
            _ => None,
        }
    }

    pub fn folder(&self) -> &str {
        match self {
            Entry::Mod(f) | Entry::Pack(f) | Entry::Global(f) => f,
        }
    }

    pub fn is_global(&self) -> bool {
        matches!(self, Entry::Global(_))
    }
}

/// What the automatic order needs to know about a mod.
#[derive(Clone, Debug, Default)]
pub struct ModNode {
    pub folder: String,
    pub id: String,
    pub load_after: Vec<String>,
    pub patches: Vec<String>,
}

/// Mods sorted so each follows the mods it builds on; ties and cycles fall back to folder order.
pub fn sort_mods(nodes: &[ModNode]) -> Vec<String> {
    let mut remaining: Vec<&ModNode> = nodes.iter().collect();
    remaining.sort_by(|a, b| a.folder.cmp(&b.folder));
    let by_id = |id: &str| nodes.iter().find(|n| n.id == id).map(|n| n.folder.as_str());
    let needs = |n: &ModNode| -> Vec<String> {
        n.load_after.iter().chain(n.patches.iter()).filter_map(|id| by_id(id)).filter(|f| *f != n.folder).map(str::to_string).collect()
    };
    let mut out: Vec<String> = Vec::new();
    while !remaining.is_empty() {
        // The first (by folder name) whose prerequisites are all placed; if none, it's a cycle: take the first.
        let pick = remaining.iter().position(|n| needs(n).iter().all(|f| out.contains(f))).unwrap_or(0);
        out.push(remaining.remove(pick).folder.clone());
    }
    out
}

/// The automatic order, lowest first. `world_packs` and `global` are the player's lists, top of the
/// list (strongest) first, as saved.
pub fn auto_order(mods: &[ModNode], world_packs: &[String], global: &[String]) -> Vec<Entry> {
    let mut order: Vec<Entry> = sort_mods(mods).into_iter().map(Entry::Mod).collect();
    order.extend(world_packs.iter().rev().cloned().map(Entry::Pack));
    order.extend(global.iter().rev().cloned().map(Entry::Global));
    order
}

/// The manual order `saved` (keys) made to fit what exists (`auto`'s entries). `None` is automatic.
pub fn resolve(saved: Option<&[String]>, auto: Vec<Entry>) -> Vec<Entry> {
    let Some(saved) = saved else { return auto };
    let mut order: Vec<Entry> = Vec::new();
    for entry in saved.iter().filter_map(|k| Entry::parse(k)) {
        if auto.contains(&entry) && !order.contains(&entry) {
            order.push(entry);
        }
    }
    for entry in auto {
        if order.contains(&entry) {
            continue;
        }
        if entry.is_global() {
            order.push(entry);
        } else {
            let at = order.iter().position(Entry::is_global).unwrap_or(order.len());
            order.insert(at, entry);
        }
    }
    order
}

pub fn keys(order: &[Entry]) -> Vec<String> {
    order.iter().map(Entry::key).collect()
}

/// Moves `entry` one place towards the top (`up`, later in the list) or the bottom. The game isn't
/// in the list, so nothing can go below it. At either end: unchanged.
pub fn shift(order: &mut [Entry], entry: &Entry, up: bool) {
    let Some(i) = order.iter().position(|e| e == entry) else { return };
    let j = if up { (i + 1 < order.len()).then_some(i + 1) } else { i.checked_sub(1) };
    if let Some(j) = j {
        order.swap(i, j);
    }
}

/// One layer to stack: a mod (its data *and* its `resourcepack/`) or a plain resource pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerSource {
    Mod(ModSource),
    Pack(PathBuf),
}

impl LayerSource {
    pub fn pack_path(&self) -> Option<&PathBuf> {
        match self {
            LayerSource::Pack(p) => Some(p),
            LayerSource::Mod(_) => None,
        }
    }
}

/// Turns an order into layers, skipping anything that isn't there (an entry for a pack that has
/// been deleted does nothing).
pub fn layer_sources(order: &[Entry], mods: &[ModSource], installed: &InstalledPacks) -> Vec<LayerSource> {
    order
        .iter()
        .filter_map(|e| match e {
            Entry::Mod(folder) => mods
                .iter()
                .find(|m| m.path.file_name().and_then(|n| n.to_str()) == Some(folder.as_str()))
                .map(|m| LayerSource::Mod(m.clone())),
            Entry::Pack(folder) | Entry::Global(folder) => installed.0.iter().find(|p| &p.folder == folder).map(|p| LayerSource::Pack(p.path.clone())),
        })
        .collect()
}

/// A world's full order (lowest first): its mods, its own packs and the global packs, arranged by
/// its saved order if it has one, automatically otherwise.
pub fn world_order(
    manual: Option<&[String]>,
    mods: &[ModSource],
    world_packs: &[String],
    global: &PackSelection,
    installed: &InstalledPacks,
) -> Vec<Entry> {
    let nodes: Vec<ModNode> = mods
        .iter()
        .map(|m| {
            let folder = m.path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            match crate::mods::read_mod(&m.path) {
                Ok(info) => ModNode { folder, id: info.id, load_after: info.load_after, patches: info.patches },
                Err(_) => ModNode { folder, ..Default::default() },
            }
        })
        .collect();
    let installed_only = |list: &[String]| -> Vec<String> { list.iter().filter(|f| installed.0.iter().any(|p| &p.folder == *f)).cloned().collect() };
    // A pack on both lists counts once, at the world's position.
    let global_list: Vec<String> = installed_only(&global.enabled).into_iter().filter(|f| !world_packs.contains(f)).collect();
    resolve(manual, auto_order(&nodes, &installed_only(world_packs), &global_list))
}

/// Outside a world: just the global packs.
pub fn menu_layers(global: &PackSelection, installed: &InstalledPacks) -> Vec<LayerSource> {
    let order: Vec<Entry> = global.enabled.iter().rev().cloned().map(Entry::Global).collect();
    layer_sources(&order, &[], installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(folder: &str, id: &str, after: &[&str], patches: &[&str]) -> ModNode {
        ModNode {
            folder: folder.into(),
            id: id.into(),
            load_after: after.iter().map(|s| s.to_string()).collect(),
            patches: patches.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }
    fn folders(o: &[Entry]) -> Vec<String> {
        o.iter().map(Entry::key).collect()
    }

    #[test]
    fn keys_round_trip_and_junk_is_refused() {
        for e in [Entry::Mod("a".into()), Entry::Pack("b:c".into()), Entry::Global("d".into())] {
            assert_eq!(Entry::parse(&e.key()), Some(e));
        }
        assert_eq!(Entry::parse("nonsense"), None);
        assert_eq!(Entry::parse("what:x"), None);
    }

    #[test]
    fn an_addon_loads_after_the_mod_it_builds_on_whatever_the_folder_names() {
        // "aaa_addon" would sort first, but it patches "base_mod".
        let nodes = [node("aaa_addon", "addon", &[], &["base"]), node("zzz_base", "base", &[], &[])];
        assert_eq!(sort_mods(&nodes), ["zzz_base", "aaa_addon"]);
        // The same through an explicit load_after.
        let nodes = [node("aaa_addon", "addon", &["base"], &[]), node("zzz_base", "base", &[], &[])];
        assert_eq!(sort_mods(&nodes), ["zzz_base", "aaa_addon"]);
        // Unrelated mods stay in folder order.
        let nodes = [node("b", "b", &[], &[]), node("a", "a", &[], &[])];
        assert_eq!(sort_mods(&nodes), ["a", "b"]);
    }

    #[test]
    fn a_cycle_or_an_unknown_id_cannot_hang_or_drop_a_mod() {
        let nodes = [node("a", "a", &["b"], &[]), node("b", "b", &["a"], &[]), node("c", "c", &["nobody"], &[])];
        let mut sorted = sort_mods(&nodes);
        assert_eq!(sorted.len(), 3);
        sorted.sort();
        assert_eq!(sorted, ["a", "b", "c"]);
        let own = [node("a", "a", &["a"], &["a"])];
        assert_eq!(sort_mods(&own), ["a"], "a mod naming itself isn't waiting on itself");
    }

    #[test]
    fn auto_puts_mods_then_world_packs_then_global_packs_on_top() {
        let mods = [node("m", "m", &[], &[])];
        // Lists are top-first as saved, the order is lowest-first.
        let order = auto_order(&mods, &s(&["w_top", "w_low"]), &s(&["g_top", "g_low"]));
        assert_eq!(folders(&order), ["mod:m", "pack:w_low", "pack:w_top", "global:g_low", "global:g_top"]);
    }

    #[test]
    fn a_manual_order_is_kept_and_fitted_to_what_exists() {
        let auto = auto_order(&[node("m1", "m1", &[], &[]), node("m2", "m2", &[], &[])], &[], &s(&["g"]));
        // The player put m2 below m1 and a global pack in the middle.
        let saved = s(&["mod:m2", "global:g", "mod:m1", "pack:gone"]);
        let order = resolve(Some(&saved), auto.clone());
        assert_eq!(folders(&order), ["mod:m2", "global:g", "mod:m1"], "kept as arranged, the vanished pack dropped");
        // A brand-new mod goes in just below the first global pack; a new global pack goes on top.
        let auto_more = auto_order(&[node("m1", "m1", &[], &[]), node("m2", "m2", &[], &[]), node("m3", "m3", &[], &[])], &[], &s(&["g2", "g"]));
        let order = resolve(Some(&saved), auto_more);
        assert_eq!(folders(&order), ["mod:m2", "mod:m3", "global:g", "mod:m1", "global:g2"]);
        // None is automatic.
        assert_eq!(resolve(None, auto.clone()), auto);
    }

    #[test]
    fn shifting_moves_one_place_and_stops_at_the_ends() {
        let mut o = vec![Entry::Mod("a".into()), Entry::Mod("b".into()), Entry::Global("g".into())];
        shift(&mut o, &Entry::Mod("a".into()), true);
        assert_eq!(folders(&o), ["mod:b", "mod:a", "global:g"]);
        shift(&mut o, &Entry::Mod("b".into()), false);
        assert_eq!(folders(&o), ["mod:b", "mod:a", "global:g"], "nothing goes below the game, which isn't in the list");
        shift(&mut o, &Entry::Global("g".into()), true);
        assert_eq!(folders(&o), ["mod:b", "mod:a", "global:g"]);
    }

    #[test]
    fn layers_skip_entries_that_are_not_there() {
        let installed = InstalledPacks(vec![crate::packs::PackInfo {
            folder: "p".into(),
            name: "p".into(),
            description: String::new(),
            path: PathBuf::from("/packs/p"),
            warning: None,
        }]);
        let mods = [ModSource { ns: "m".into(), path: PathBuf::from("/mods/mfolder") }];
        let order = [Entry::Mod("mfolder".into()), Entry::Pack("missing".into()), Entry::Global("p".into()), Entry::Mod("gone".into())];
        let layers = layer_sources(&order, &mods, &installed);
        assert_eq!(layers, [LayerSource::Mod(mods[0].clone()), LayerSource::Pack(PathBuf::from("/packs/p"))]);
    }
}
