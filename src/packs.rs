//! Resource packs: the visual and audio layer - textures, block models and sounds - that can
//! sit on top of the game without changing how anything *plays*.
//!
//! **Two kinds.**
//! - *A player's pack* is a folder in `resourcepacks/` beside the game (`pack.json` optional:
//!   `{"name": "...", "description": "..."}`). The player turns packs on and off and orders them
//!   in Settings > Resource Packs; the top of the list wins. It can retexture any block,
//!   replace any block's model, replace any sound, and re-skin the menus. It *cannot* change a
//!   block's collision or what you can mine - a model from a player's pack is only a picture.
//! - *A mod's pack* is the `resourcepack/` folder at the top of a mod. It is part of the mod:
//!   it can't be turned on or off by itself, only by enabling the mod. It is the **only** place
//!   a mod keeps textures, models and sounds. A model in it may set `"collision": "model"` to
//!   make the model's own shape the block's collision and hitbox.
//!
//! **Layout** (the same in both, and in the game's own folder, which is the bottom layer):
//! ```text
//! textures/blocks/<name>.png      block tiles (and item textures)
//! textures/gui/<element>.png      menus (see textures/gui/README.md); gui/glyphs/ for buttons
//! textures/sky/<name>.png         sun, moon
//! models/blocks/<block>.json      a block's look (see models.rs)
//! sounds/<event path>.ogg|.wav    sounds (see sounds.rs)
//! ```
//! **Whose name is whose.** In the game's folder and in a player's pack a plain name is the core
//! game's (`dirt`) and `<ns>/<name>` is a mod's (`granite_pack/granite`). In a mod's pack a
//! plain name is the mod's own and `<ns>/<name>` is someone else's, so a mod re-skins vanilla
//! dirt with `craftmjne/dirt`.
//!
//! **Order.** Lowest first: the game, then each mod of the world in load order, then the
//! player's packs (so a player can always retexture a mod's art). The highest layer holding a
//! file wins.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::overlay::{KindDir, CORE_NS};
use crate::save::SaveStore;

/// The folder at the top of a mod that is its resource pack.
pub const MOD_PACK_DIR: &str = "resourcepack";
pub const PACK_MANIFEST: &str = "pack.json";

/// Where a player's packs live: `resourcepacks/` next to the executable, or in the working
/// directory (how `cargo run` finds it). Not created here.
pub fn user_packs_dir() -> PathBuf {
    let beside_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("resourcepacks")));
    if let Some(dir) = &beside_exe {
        if dir.is_dir() {
            return dir.clone();
        }
    }
    let cwd = PathBuf::from("resourcepacks");
    if cwd.is_dir() {
        return cwd;
    }
    beside_exe.unwrap_or(cwd)
}

#[derive(Deserialize, Default)]
struct Manifest {
    name: Option<String>,
    description: Option<String>,
}

/// A player's pack found on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct PackInfo {
    /// The folder name - what the selection remembers it by.
    pub folder: String,
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    /// Something wrong with it that still lets it load (an unreadable `pack.json`).
    pub warning: Option<String>,
}

/// Reads one pack folder. Never fails: a missing or broken `pack.json` just means the folder's
/// name is used (and, if broken, a warning is kept).
pub fn read_pack(path: &Path) -> PackInfo {
    let folder = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let (manifest, warning) = match std::fs::read_to_string(path.join(PACK_MANIFEST)) {
        Err(_) => (Manifest::default(), None),
        Ok(text) => match serde_json::from_str::<Manifest>(&text) {
            Ok(m) => (m, None),
            Err(e) => (Manifest::default(), Some(format!("{folder}: {PACK_MANIFEST} couldn't be read ({e}), using the folder name"))),
        },
    };
    PackInfo {
        name: manifest.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| folder.clone()),
        description: manifest.description.unwrap_or_default(),
        folder,
        path: path.to_path_buf(),
        warning,
    }
}

/// Every pack folder in `dir`, by folder name. Anything that isn't a folder is skipped.
pub fn scan_packs(dir: &Path) -> Vec<PackInfo> {
    let mut found: Vec<PackInfo> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')))
        .map(|p| read_pack(&p))
        .collect();
    found.sort_by(|a, b| a.folder.to_lowercase().cmp(&b.folder.to_lowercase()));
    found
}

/// The packs the player has turned on, top of the list (highest priority) first. Saved per
/// launcher instance. A folder named here that isn't installed is kept (the pack may be
/// back later) but does nothing.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackSelection {
    #[serde(default)]
    pub enabled: Vec<String>,
}

impl PackSelection {
    pub fn is_enabled(&self, folder: &str) -> bool {
        self.enabled.iter().any(|f| f == folder)
    }

    /// Turns a pack on at the top of the list. Already on: unchanged.
    pub fn enable(&mut self, folder: &str) {
        if !self.is_enabled(folder) {
            self.enabled.insert(0, folder.to_string());
        }
    }

    pub fn disable(&mut self, folder: &str) {
        self.enabled.retain(|f| f != folder);
    }

    /// Moves an enabled pack one place towards the top (`up`) or bottom. At the end: unchanged.
    pub fn shift(&mut self, folder: &str, up: bool) {
        let Some(i) = self.enabled.iter().position(|f| f == folder) else { return };
        let j = if up { i.checked_sub(1) } else { (i + 1 < self.enabled.len()).then_some(i + 1) };
        if let Some(j) = j {
            self.enabled.swap(i, j);
        }
    }

    /// The installed enabled packs, **lowest priority first** - the order layers are stacked in.
    pub fn active<'a>(&self, installed: &'a [PackInfo]) -> Vec<&'a PackInfo> {
        self.enabled.iter().rev().filter_map(|f| installed.iter().find(|p| &p.folder == f)).collect()
    }

    /// The paths of [`PackSelection::active`], lowest first.
    pub fn active_paths(&self, installed: &[PackInfo]) -> Vec<PathBuf> {
        self.active(installed).into_iter().map(|p| p.path.clone()).collect()
    }
}

/// The packs installed right now (a scan of [`user_packs_dir`], redone when the screen opens).
#[derive(Resource, Clone, Debug, Default)]
pub struct InstalledPacks(pub Vec<PackInfo>);

/// The paths of the enabled, installed packs (lowest priority first) from the optional
/// resources - none when the plugin isn't there (a headless test app).
pub fn active_pack_paths(selection: Option<&PackSelection>, installed: Option<&InstalledPacks>) -> Vec<PathBuf> {
    match (selection, installed) {
        (Some(s), Some(i)) => s.active_paths(&i.0),
        _ => Vec::new(),
    }
}

/// One layer of the stack: a pack's root and whose names its plain files are.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub ns: String,
    pub root: PathBuf,
    /// A player's pack or a mod's (may hold other namespaces' files in `<ns>/` folders).
    pub extends: bool,
    /// What this layer's mod defines (so a plain texture name in its models means its own).
    pub locals: Arc<crate::overlay::Locals>,
}

impl Layer {
    /// The directory for one kind of file in this layer (`"textures/blocks"`, `"models/blocks"`, `"sounds"`).
    pub fn kind_dir(&self, section: &str) -> KindDir {
        let dir = section.split('/').fold(self.root.clone(), |p, part| p.join(part));
        KindDir { ns: self.ns.clone(), dir, locals: self.locals.clone(), extends: self.extends }
    }
}

/// The game's own folder (the bottom layer): where `textures/` is found.
pub fn core_root() -> PathBuf {
    crate::blocks::find_data_dir("textures").parent().map(Path::to_path_buf).unwrap_or_default()
}

/// The layers, lowest first.
#[derive(Clone, Debug, PartialEq)]
pub struct PackStack {
    pub layers: Vec<Layer>,
}

impl PackStack {
    /// Just the game's own files.
    pub fn core() -> Self {
        Self { layers: vec![Layer { ns: CORE_NS.to_string(), root: core_root(), extends: false, locals: Default::default() }] }
    }

    /// Adds a mod's resource pack on top.
    pub fn with_mod(mut self, ns: &str, mod_root: &Path) -> Self {
        self.layers.push(Layer {
            ns: ns.to_string(),
            root: mod_root.join(MOD_PACK_DIR),
            extends: true,
            locals: Arc::new(crate::overlay::Locals::scan(mod_root)),
        });
        self
    }

    /// Adds the player's packs (given lowest first) on top.
    pub fn with_user_packs(mut self, paths: &[PathBuf]) -> Self {
        self.layers.extend(paths.iter().map(|p| Layer { ns: CORE_NS.to_string(), root: p.clone(), extends: true, locals: Default::default() }));
        self
    }

    /// Every layer's directory of one kind, lowest first.
    pub fn dirs(&self, section: &str) -> Vec<KindDir> {
        self.layers.iter().map(|l| l.kind_dir(section)).collect()
    }
}

/// The file for `name` (`ns:local` or plain) with extension `ext` under `section` in the highest
/// layer holding one, using the same whose-name-is-whose rule as textures.
pub fn find_file(dirs: &[KindDir], name: &str, ext: &str) -> Option<(usize, PathBuf)> {
    dirs.iter().enumerate().rev().find_map(|(i, kd)| file_in(kd, name, ext).map(|p| (i, p)))
}

/// The file for `name` in this one layer, if it has it (same rule as [`find_file`]).
pub fn file_in(kd: &KindDir, name: &str, ext: &str) -> Option<PathBuf> {
    let (ns, local) = crate::overlay::split_name(name);
    let file = format!("{local}.{ext}");
    let path = if kd.ns == ns {
        kd.dir.join(&file)
    } else if !kd.extends {
        return None;
    } else {
        kd.dir.join(ns).join(&file)
    };
    path.is_file().then_some(path)
}

/// A note for a mod that still keeps art in the old place: textures, models and sounds now
/// belong in its `resourcepack/` folder, and the old folders are no longer read.
pub fn legacy_layout_note(mod_root: &Path) -> Option<String> {
    let old: Vec<&str> = ["textures", "models", "sounds"].into_iter().filter(|d| mod_root.join(d).is_dir()).collect();
    (!old.is_empty()).then(|| {
        format!(
            "it has a {} folder at the top level, which is no longer read - move {} into {MOD_PACK_DIR}/",
            old.join(" and a "),
            if old.len() == 1 { "it" } else { "them" }
        )
    })
}

pub struct PackPlugin;

impl Plugin for PackPlugin {
    fn build(&self, app: &mut App) {
        let selection = app.world().get_resource::<SaveStore>().map(|s| s.load_pack_selection()).unwrap_or_default();
        let installed = scan_packs(&user_packs_dir());
        app.insert_resource(selection).insert_resource(InstalledPacks(installed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("craftmjne-packs-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn info(folder: &str) -> PackInfo {
        PackInfo { folder: folder.into(), name: folder.into(), description: String::new(), path: PathBuf::from(folder), warning: None }
    }

    #[test]
    fn a_pack_is_a_folder_and_its_manifest_is_optional() {
        let dir = tmp("scan");
        std::fs::create_dir_all(dir.join("plain")).unwrap();
        std::fs::create_dir_all(dir.join("Fancy")).unwrap();
        std::fs::write(dir.join("Fancy").join(PACK_MANIFEST), r#"{"name": "Fancy Art", "description": "pretty"}"#).unwrap();
        std::fs::create_dir_all(dir.join("broken")).unwrap();
        std::fs::write(dir.join("broken").join(PACK_MANIFEST), "{nope").unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("a_file.txt"), "x").unwrap();
        let found = scan_packs(&dir);
        let folders: Vec<_> = found.iter().map(|p| p.folder.as_str()).collect();
        assert_eq!(folders, ["broken", "Fancy", "plain"], "folders only, hidden ones skipped, sorted ignoring case");
        let fancy = &found[1];
        assert_eq!((fancy.name.as_str(), fancy.description.as_str()), ("Fancy Art", "pretty"));
        assert_eq!(found[2].name, "plain", "no manifest: the folder's name");
        assert!(found[0].warning.is_some() && found[0].name == "broken", "a broken manifest still loads, with a note");
        assert!(scan_packs(&dir.join("nowhere")).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enabling_puts_a_pack_on_top_and_the_stack_is_lowest_first() {
        let mut s = PackSelection::default();
        s.enable("a");
        s.enable("b");
        s.enable("a"); // already on: no change
        assert_eq!(s.enabled, ["b", "a"], "the newest is on top");
        let installed = [info("a"), info("b"), info("c")];
        let active: Vec<_> = s.active(&installed).iter().map(|p| p.folder.as_str()).collect();
        assert_eq!(active, ["a", "b"], "lowest priority first, so the top of the list is stacked last and wins");
        s.disable("a");
        assert_eq!(s.enabled, ["b"]);
    }

    #[test]
    fn shifting_moves_one_place_and_stops_at_the_ends() {
        let mut s = PackSelection { enabled: vec!["a".into(), "b".into(), "c".into()] };
        s.shift("c", true);
        assert_eq!(s.enabled, ["a", "c", "b"]);
        s.shift("a", true);
        assert_eq!(s.enabled, ["a", "c", "b"], "already on top");
        s.shift("b", false);
        assert_eq!(s.enabled, ["a", "c", "b"], "already at the bottom");
        s.shift("a", false);
        assert_eq!(s.enabled, ["c", "a", "b"]);
        s.shift("nope", true);
        assert_eq!(s.enabled.len(), 3);
    }

    #[test]
    fn a_pack_that_is_gone_is_remembered_but_does_nothing() {
        let s = PackSelection { enabled: vec!["gone".into(), "here".into()] };
        let installed = [info("here")];
        assert_eq!(s.active_paths(&installed), [PathBuf::from("here")]);
        assert_eq!(s.enabled.len(), 2, "still selected, in case it comes back");
    }

    #[test]
    fn a_selection_survives_json_and_an_old_file_reads_as_empty() {
        let s = PackSelection { enabled: vec!["x".into(), "y".into()] };
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<PackSelection>(&text).unwrap(), s);
        assert_eq!(serde_json::from_str::<PackSelection>("{}").unwrap(), PackSelection::default());
    }

    #[test]
    fn find_file_applies_the_whose_name_rule_and_the_highest_layer_wins() {
        let (core, user, modroot) = (tmp("core"), tmp("user"), tmp("mod"));
        for (dir, file) in [(&core, "dirt.png"), (&user, "dirt.png"), (&user, "granite_pack/granite.png"), (&modroot, "granite.png"), (&modroot, "craftmjne/dirt.png")] {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        let dirs = vec![
            KindDir::core(&core),
            KindDir { ns: "granite_pack".into(), dir: modroot.clone(), locals: Default::default(), extends: true },
            KindDir::pack(&user),
        ];
        let (i, _) = find_file(&dirs, "dirt", "png").unwrap();
        assert_eq!(i, 2, "the player's pack is above the mod's and the game's");
        let (i, p) = find_file(&dirs, "granite_pack:granite", "png").unwrap();
        assert_eq!(i, 2, "the player's pack re-skins a mod's block with <ns>/<name>: {p:?}");
        assert!(p.ends_with("granite_pack/granite.png"));
        // Remove the player's pack's copy: the mod's own file is next.
        std::fs::remove_file(user.join("granite_pack/granite.png")).unwrap();
        assert_eq!(find_file(&dirs, "granite_pack:granite", "png").unwrap().0, 1);
        // The game's folder never holds anyone else's names.
        assert!(find_file(&[KindDir::core(&core)], "granite_pack:granite", "png").is_none());
        for d in [core, user, modroot] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn only_the_game_and_a_mods_pack_may_change_collision() {
        assert!(KindDir::core("x").may_change_collision());
        assert!(!KindDir::pack("x").may_change_collision(), "a player's pack is only ever a picture");
        let modd = KindDir { ns: "m".into(), dir: "x".into(), locals: Default::default(), extends: true };
        assert!(modd.may_change_collision());
    }

    #[test]
    fn a_stack_builds_its_directories_lowest_first() {
        let stack = PackStack::core().with_mod("m", Path::new("/mods/m")).with_user_packs(&[PathBuf::from("/p/low"), PathBuf::from("/p/high")]);
        let dirs = stack.dirs("textures/blocks");
        assert_eq!(dirs.len(), 4);
        assert!(dirs[0].is_core() && !dirs[0].extends);
        assert_eq!(dirs[1].ns, "m");
        assert!(dirs[1].dir.ends_with("resourcepack/textures/blocks"));
        assert!(dirs[3].dir.starts_with("/p/high") && dirs[3].extends && dirs[3].is_core());
    }

    #[test]
    fn an_old_style_mod_layout_is_noticed() {
        let root = tmp("legacy");
        assert_eq!(legacy_layout_note(&root), None);
        std::fs::create_dir_all(root.join("textures")).unwrap();
        std::fs::create_dir_all(root.join("sounds")).unwrap();
        let note = legacy_layout_note(&root).unwrap();
        assert!(note.contains("textures") && note.contains("sounds") && note.contains("resourcepack/"), "{note}");
        std::fs::create_dir_all(root.join(MOD_PACK_DIR).join("textures")).unwrap();
        assert!(legacy_layout_note(&root).is_some(), "having the new folder too doesn't hide a stale old one");
        let _ = std::fs::remove_dir_all(root);
    }
}
