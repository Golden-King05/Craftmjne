//! **Mods**: folders of data (blocks, items, recipes, textures) that add to or
//! change the game's content. A mod is data only - no code - so everything it
//! can do is something a block or item file can already say; see `tags.rs`.
//!
//! ```text
//! mods/<any folder name>/
//!   mod.json            { "id": "granite_pack", "name": "Granite Pack", "version": "1.0" }
//!   blocks/*.json       same format as the game's own blocks/
//!   items/*.json
//!   recipes/*.json
//!   tiers/*.json
//!   textures/blocks/*.png
//! ```
//!
//! **`mod.json` must have an `"id"`, or the mod doesn't work.** The id is the
//! prefix of everything the mod adds (`granite_pack:granite`), which is what lets
//! two mods each add a `granite` without clashing. See `overlay.rs` for the naming
//! rules. If two mods in one world declare the same id the game doesn't break: it
//! renames them there (`granite_pack_1`, `granite_pack_2`) and remembers that
//! ([`assign_namespaces`]).
//!
//! ## Mods sit on top of the game; they never change it
//!
//! A mod is an **overlay**. The game's own files are only ever read: nothing a
//! mod does edits, replaces or deletes them, so taking a mod off puts everything
//! back exactly as it was. What an overlay can do:
//!
//! - **Add** a block, item or tier: a plain id, which gets the mod's prefix.
//! - **Change** one that exists by giving its full name and only the keys to
//!   change: `{"id": "craftmjne:dirt", "hardness": 8}` makes dirt slow to mine and
//!   leaves every other property of dirt alone (`other_mod:granite` patches another
//!   mod's). Objects merge key by key, and a key set to `null` is removed so it
//!   falls back to its default.
//! - **Replace** a recipe with `"replaces": "craftmjne:planks"`.
//! - **Re-skin** with `textures/blocks/craftmjne/dirt.png`; the mod's own textures
//!   are plain `textures/blocks/granite.png`.
//!
//! With several mods, later ones (by folder name) are laid over earlier ones. The
//! blocks the core files define keep their ids whatever mods are on (derived
//! waterlogged variants may shift, which nothing depends on: saves store names,
//! never numeric ids).
//!
//! ## Which mods a world uses, and where they live
//!
//! The game keeps one `mods/` folder next to it (the *instance*). Each world
//! chooses how it relates to it ([`ModStorage`]):
//!
//! - **Instance**: the world records which mods it uses (`WorldMeta::mods`) and
//!   runs the copies in the game's folder. Cheap, but deleting a mod from that
//!   folder breaks every world that needs it, so such a world refuses to open
//!   (naming what's missing) rather than loading without the mod and saving
//!   over it.
//! - **World**: the chosen mods are copied into the world's own `mods/` folder
//!   when it's created, and *every* mod in that folder is used. Because the
//!   world's folder is what decides, a mod can be dropped into one world
//!   without ever being installed in the game.
//! - **Branch** (`WorldMeta::branch_of`): a world that stores nothing of its own
//!   and uses another world's mods. Share a modpack across worlds without a copy
//!   each - and share its fate: if the host is deleted or loses a mod, so does
//!   the branch (it refuses to open, as above).
//!
//! [`resolve_world_mods`] turns all of that into the list of mod folders to load.

use std::path::{Path, PathBuf};

use bevy::prelude::Resource;
use serde::Deserialize;

use crate::save::{ModStorage, SaveStore, WorldMeta};

/// The file that makes a directory a mod.
pub const MANIFEST: &str = "mod.json";

/// The version of the game this build is (`Cargo.toml`'s `version`).
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `target_ver` in `mod.json`: one version, or a list of them.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum TargetVer {
    One(String),
    Many(Vec<String>),
}

/// What `mod.json` says. Only `id` is required to work; `target_ver` is what a
/// mod should always say (see [`ModInfo::warning`]).
#[derive(Deserialize, Default)]
struct Manifest {
    id: Option<String>,
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    target_ver: Option<TargetVer>,
}

/// Whether one `target_ver` entry covers `game`. An entry is dotted numbers; a
/// shorter one covers every version that starts with it (`"1.3"` covers `1.3.0`
/// and `1.3.7`), and `x` or `*` in any place matches any number there
/// (`"1.x.2"`). So a mod can name one exact version, a whole minor series, or a
/// whole major one.
pub fn version_matches(entry: &str, game: &str) -> bool {
    let entry: Vec<&str> = entry.trim().trim_start_matches('v').split('.').collect();
    let game: Vec<&str> = game.trim().trim_start_matches('v').split('.').collect();
    if entry.is_empty() || entry.iter().any(|p| p.is_empty()) || entry.len() > game.len() {
        return false;
    }
    entry.iter().zip(&game).all(|(e, g)| matches!(*e, "x" | "X" | "*") || e == g)
}

/// An installed mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModInfo {
    /// The folder it's in (what worlds remember it by).
    pub folder: String,
    /// The id it declares in `mod.json`: the prefix of everything it adds.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// The game versions it says it works with (`target_ver`); empty if it
    /// doesn't say.
    pub target_ver: Vec<String>,
    pub path: PathBuf,
}

impl ModInfo {
    /// A warning to show when this mod is used with game version `game`, or `None`
    /// if it says it works with it. Only ever a warning: the mod still loads, since
    /// the author may simply not have updated the line yet. It exists so a player
    /// isn't left guessing which mod goes with which version of the game.
    pub fn warning(&self, game: &str) -> Option<String> {
        if self.target_ver.is_empty() {
            return Some(format!(
                "{} doesn't say which game version it's made for (no \"target_ver\"); this is {game}.",
                self.name
            ));
        }
        if self.target_ver.iter().any(|t| version_matches(t, game)) {
            return None;
        }
        Some(format!(
            "{} is made for game version {}, but this is {game}; it may not work.",
            self.name,
            self.target_ver.join(" or ")
        ))
    }

    /// Every note about this mod worth showing: the version warning and a stale layout (art
    /// kept at the top level instead of in its `resourcepack/` folder).
    pub fn notes(&self, game: &str) -> Vec<String> {
        let mut out: Vec<String> = self.warning(game).into_iter().collect();
        if let Some(layout) = crate::packs::legacy_layout_note(&self.path) {
            out.push(format!("{}: {layout}", self.name));
        }
        out
    }
}

/// A mod as a world runs it: where it is, and the namespace it was given there.
/// Usually that's the id it declares; when two of a world's mods declare the same
/// one, [`assign_namespaces`] renames them (`id_1`, `id_2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModSource {
    pub ns: String,
    pub path: PathBuf,
}

/// Whether `id` can be a mod's id: letters, digits, `_` and `-`. (It prefixes
/// every name the mod adds, so no spaces or colons - and `craftmjne` is the core
/// game's own.)
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id != crate::overlay::CORE_NS && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Where the game's installed mods live: `mods/` next to the executable, or in
/// the working directory (how `cargo run` finds it). Not created here.
pub fn installed_mods_dir() -> PathBuf {
    let beside_exe = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("mods")));
    if let Some(dir) = &beside_exe {
        if dir.is_dir() {
            return dir.clone();
        }
    }
    let cwd = PathBuf::from("mods");
    if cwd.is_dir() {
        return cwd;
    }
    beside_exe.unwrap_or(cwd)
}

/// Reads the mod in `path`, or says why it won't work. **A mod without an `id`
/// doesn't work**: the id is what keeps its names from clashing with anyone else's.
pub fn read_mod(path: &Path) -> Result<ModInfo, String> {
    let folder = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let text = std::fs::read_to_string(path.join(MANIFEST)).map_err(|_| format!("{folder}: no {MANIFEST}"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("{folder}: bad {MANIFEST}: {e}"))?;
    let Some(id) = manifest.id else {
        return Err(format!("{folder}: {MANIFEST} needs an \"id\" (a mod without one doesn't work)"));
    };
    if !is_valid_id(&id) {
        return Err(format!(
            "{folder}: the id {id:?} isn't valid (letters, digits, _ and - only, and not \"{}\")",
            crate::overlay::CORE_NS
        ));
    }
    Ok(ModInfo {
        name: manifest.name.unwrap_or_else(|| id.clone()),
        version: manifest.version.unwrap_or_default(),
        description: manifest.description.unwrap_or_default(),
        target_ver: match manifest.target_ver {
            Some(TargetVer::One(v)) => vec![v],
            Some(TargetVer::Many(v)) => v,
            None => Vec::new(),
        },
        id,
        folder,
        path: path.to_path_buf(),
    })
}

/// Every mod in `dir` that works, sorted by folder name, and the folders that
/// don't (with why), for the Mods screen to show.
pub fn scan_all(dir: &Path) -> (Vec<ModInfo>, Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return (Vec::new(), Vec::new()) };
    let (mut good, mut bad) = (Vec::new(), Vec::new());
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
        // A folder with no mod.json at all isn't trying to be a mod.
        if !path.join(MANIFEST).is_file() {
            continue;
        }
        match read_mod(&path) {
            Ok(m) => good.push(m),
            Err(why) => bad.push((path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string(), why)),
        }
    }
    good.sort_by(|a, b| a.folder.cmp(&b.folder));
    bad.sort();
    (good, bad)
}

/// Every mod in `dir` that works, sorted by folder name.
pub fn scan_mods(dir: &Path) -> Vec<ModInfo> {
    scan_all(dir).0
}

/// The conflict manager. Gives each of a world's mods the namespace it runs under,
/// so two mods that declare the same id can both be on without their names clashing.
///
/// `existing` is what the world already assigned (folder -> namespace) and is kept
/// as it is: a block saved as `x_1:granite` must still be `x_1:granite` next time,
/// even if the other `x` mod has since been removed. Mods new to the world get
/// their declared id - unless another mod new to it, or one it already has, uses
/// that id too, in which case the newcomers sharing it become `id_1`, `id_2`, ...
/// (in folder order). A mod that isn't in `mods` right now *keeps* its entry: a
/// mod can be turned off and on again, and what the world saved under its namespace
/// has to find it under the same one.
pub fn assign_namespaces(
    mods: &[ModInfo],
    existing: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    use std::collections::{BTreeMap, HashSet};
    let mut out: BTreeMap<String, String> = existing.clone();
    let mut used: HashSet<String> = out.values().cloned().collect();
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut newcomers: Vec<&ModInfo> = mods.iter().filter(|m| !out.contains_key(&m.folder)).collect();
    newcomers.sort_by(|a, b| a.folder.cmp(&b.folder));
    for m in newcomers {
        groups.entry(m.id.as_str()).or_default().push(m.folder.as_str());
    }
    for (declared, folders) in groups {
        if folders.len() == 1 && !used.contains(declared) {
            used.insert(declared.to_string());
            out.insert(folders[0].to_string(), declared.to_string());
            continue;
        }
        let mut n = 1;
        for folder in folders {
            let name = loop {
                let candidate = format!("{declared}_{n}");
                n += 1;
                if !used.contains(&candidate) {
                    break candidate;
                }
            };
            used.insert(name.clone());
            out.insert(folder.to_string(), name);
        }
    }
    out
}

/// Copies directory `src` to `dst` (created), recursively.
pub fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Why a world's mods couldn't be put together.
#[derive(Debug, PartialEq, Eq)]
pub enum ModError {
    /// Instance storage names mods that aren't in the game's `mods/` folder.
    NotInstalled(Vec<String>),
    /// The world it was branched from is gone.
    HostMissing(String),
    /// The world it was branched from can't share mods: it's a branch itself, or
    /// keeps them in the game's folder.
    HostCannotShare(String),
    /// A mod the world needs is missing from its (or its host's) folder.
    MissingFromWorld { holder: String, missing: Vec<String> },
}

impl ModError {
    /// What to tell the player.
    pub fn message(&self) -> String {
        match self {
            Self::NotInstalled(ids) => format!(
                "This world needs mods that aren't installed: {}. Put them back in the game's mods folder.",
                ids.join(", ")
            ),
            Self::HostMissing(host) => {
                format!("This world shares its mods with the world \"{host}\", which no longer exists.")
            }
            Self::HostCannotShare(host) => {
                format!("The world \"{host}\" can't share its mods (it isn't a world-storage world, or it's a branch itself).")
            }
            Self::MissingFromWorld { holder, missing } => {
                format!("The world \"{holder}\" is missing mods this world needs: {}.", missing.join(", "))
            }
        }
    }
}

/// What a world runs: its mods with the namespace each got, and the full
/// folder -> namespace assignment to remember in the world's meta
/// (`WorldMeta::mod_ids`), so the names it saves stay the names it loads.
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedMods {
    pub sources: Vec<ModSource>,
    pub ids: std::collections::BTreeMap<String, String>,
    /// One line for each mod that isn't made for this game version
    /// ([`ModInfo::warning`]).
    pub warnings: Vec<String>,
}

fn resolved(infos: Vec<ModInfo>, existing: &std::collections::BTreeMap<String, String>) -> ResolvedMods {
    let ids = assign_namespaces(&infos, existing);
    let sources = infos.iter().map(|m| ModSource { ns: ids[&m.folder].clone(), path: m.path.clone() }).collect();
    let warnings = infos.iter().flat_map(|m| m.notes(GAME_VERSION)).collect();
    ResolvedMods { sources, ids, warnings }
}

/// The mods world `slug` runs, in load order (by folder name), each with its
/// namespace. `installed` is the game's `mods/` folder.
///
/// See the module docs for the three storage styles. An `Err` means the world
/// must not open: loading it without those mods would drop their blocks from
/// its next save.
pub fn resolve_world_mods(
    store: &SaveStore,
    slug: &str,
    meta: &WorldMeta,
    installed: &Path,
) -> Result<ResolvedMods, ModError> {
    if let Some(host) = &meta.branch_of {
        let host_meta = store.load_meta(host).map_err(|_| ModError::HostMissing(host.clone()))?;
        if host_meta.branch_of.is_some() || host_meta.mod_storage != ModStorage::World {
            return Err(ModError::HostCannotShare(host.clone()));
        }
        // The host's own assignments, so both worlds name things alike.
        return Ok(resolved(scan_mods(&store.world_mods_dir(host)), &host_meta.mod_ids));
    }
    match meta.mod_storage {
        ModStorage::Instance => {
            let mut missing = Vec::new();
            let mut infos = Vec::new();
            let mut folders = meta.mods.clone();
            folders.sort();
            folders.dedup();
            for folder in folders {
                match read_mod(&installed.join(&folder)) {
                    Ok(m) => infos.push(m),
                    Err(_) => missing.push(folder),
                }
            }
            if missing.is_empty() {
                Ok(resolved(infos, &meta.mod_ids))
            } else {
                Err(ModError::NotInstalled(missing))
            }
        }
        ModStorage::World => {
            let dir = store.world_mods_dir(slug);
            let infos = scan_mods(&dir);
            let missing: Vec<String> =
                meta.mods.iter().filter(|f| !infos.iter().any(|m| &m.folder == *f)).cloned().collect();
            if !missing.is_empty() {
                return Err(ModError::MissingFromWorld { holder: slug.to_string(), missing });
            }
            Ok(resolved(infos, &meta.mod_ids))
        }
    }
}

/// Where a world-storage world puts a mod that's turned off (still its own copy).
pub fn disabled_mods_dir(store: &SaveStore, slug: &str) -> PathBuf {
    store.world_dir(slug).join("mods-disabled")
}

/// Changes which mods an existing world uses to exactly `chosen` (folder names).
/// Nothing a mod made is lost by turning it off: the world's save keeps the
/// entries it can't read (`shelf.rs`), and they load again when it's back.
///
/// - **Game folder** worlds just record the new list (every chosen mod has to be
///   installed).
/// - **In-world** worlds move a mod that's turned off into the world's
///   `mods-disabled/` (a copy that may be the only one is never deleted) and bring
///   a turned-on one back from there, or copy it in from the game's folder.
/// - A **branch** has no mods of its own to change (it uses its host's).
///
/// The namespace each mod was ever given is kept (`WorldMeta::mod_ids`).
pub fn set_world_mods(
    store: &SaveStore,
    slug: &str,
    installed: &Path,
    chosen: &[String],
) -> std::io::Result<WorldMeta> {
    let invalid = |msg: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg);
    let mut meta = store.load_meta(slug)?;
    if meta.branch_of.is_some() {
        return Err(invalid("a branch uses its host world's mods; change them there".into()));
    }
    let mut chosen: Vec<String> = chosen.to_vec();
    chosen.sort();
    chosen.dedup();
    match meta.mod_storage {
        ModStorage::Instance => {
            let missing: Vec<&String> = chosen.iter().filter(|f| read_mod(&installed.join(f)).is_err()).collect();
            if !missing.is_empty() {
                return Err(invalid(format!("not installed: {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
            }
        }
        ModStorage::World => {
            let (mods_dir, off_dir) = (store.world_mods_dir(slug), disabled_mods_dir(store, slug));
            // Check everything can be done before moving anything.
            for folder in &chosen {
                let have = mods_dir.join(folder).join(MANIFEST).is_file()
                    || off_dir.join(folder).join(MANIFEST).is_file()
                    || read_mod(&installed.join(folder)).is_ok();
                if !have {
                    return Err(invalid(format!("{folder}: no such mod")));
                }
            }
            std::fs::create_dir_all(&mods_dir)?;
            for current in scan_mods(&mods_dir) {
                if !chosen.contains(&current.folder) {
                    std::fs::create_dir_all(&off_dir)?;
                    let target = off_dir.join(&current.folder);
                    let _ = std::fs::remove_dir_all(&target);
                    std::fs::rename(&current.path, &target)?;
                }
            }
            for folder in &chosen {
                let here = mods_dir.join(folder);
                if here.join(MANIFEST).is_file() {
                    continue;
                }
                let off = off_dir.join(folder);
                if off.join(MANIFEST).is_file() {
                    std::fs::rename(&off, &here)?;
                } else {
                    copy_dir_all(&installed.join(folder), &here)?;
                }
            }
        }
    }
    meta.mods = chosen;
    let infos: Vec<ModInfo> = match meta.mod_storage {
        ModStorage::Instance => meta.mods.iter().filter_map(|f| read_mod(&installed.join(f)).ok()).collect(),
        ModStorage::World => scan_mods(&store.world_mods_dir(slug)),
    };
    meta.mod_ids = assign_namespaces(&infos, &meta.mod_ids);
    store.save_meta(slug, &meta)?;
    Ok(meta)
}

/// Copies `chosen` mods from the game's folder into world `slug`, for a world
/// created with `World` storage.
pub fn copy_mods_into_world(store: &SaveStore, slug: &str, installed: &Path, chosen: &[String]) -> std::io::Result<()> {
    let dest = store.world_mods_dir(slug);
    std::fs::create_dir_all(&dest)?;
    for id in chosen {
        copy_dir_all(&installed.join(id), &dest.join(id))?;
    }
    Ok(())
}

/// The game's installed-mods folder. A resource so tests can point it elsewhere.
#[derive(Resource, Clone, Debug)]
pub struct InstalledMods(pub PathBuf);

impl Default for InstalledMods {
    fn default() -> Self {
        Self(installed_mods_dir())
    }
}

/// What the loaded content was built from: the mod folders (in load order) and
/// the block texture names it reported, so a world with different mods can swap
/// it for its own and tidy the texture report.
#[derive(Resource, Default, Debug)]
pub struct LoadedContent {
    pub mods: Vec<ModSource>,
    /// The player's resource packs (paths, lowest priority first) the content was built with.
    pub packs: Vec<std::path::PathBuf>,
    pub block_textures: Vec<String>,
    /// Bumped every time content is installed, so things built from it (the GPU
    /// atlas) can tell they're out of date.
    pub generation: u64,
}

/// Whether the active world's content is in place. `world::enter_world` waits on
/// it: a world whose mods couldn't be assembled doesn't open.
#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct ContentOk(pub bool);

/// A message for the main menu (why a world wouldn't open).
#[derive(Resource, Default, Debug)]
pub struct MenuNotice(pub Option<String>);

/// How a new world gets its mods.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModSetup {
    /// Use these mods from the game's folder ([`ModStorage::Instance`]).
    Instance(Vec<String>),
    /// Copy these mods into the world ([`ModStorage::World`]).
    World(Vec<String>),
    /// Share this world's mods ([`WorldMeta::branch_of`]).
    Branch(String),
}

/// Creates a world with its mods set up. Fails, creating nothing, if a chosen mod
/// isn't installed or a branch's host can't be branched from.
pub fn create_world_with_mods(
    store: &SaveStore,
    name: &str,
    seed: u32,
    mode: crate::save::GameMode,
    setup: &ModSetup,
    installed: &Path,
) -> std::io::Result<(String, WorldMeta)> {
    let invalid = |msg: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg);
    // Check everything before creating a folder.
    let (storage, mods, branch) = match setup {
        ModSetup::Instance(ids) | ModSetup::World(ids) => {
            let missing: Vec<&String> = ids.iter().filter(|id| read_mod(&installed.join(id)).is_err()).collect();
            if !missing.is_empty() {
                return Err(invalid(format!("not installed: {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))));
            }
            let storage = if matches!(setup, ModSetup::World(_)) { ModStorage::World } else { ModStorage::Instance };
            (storage, ids.clone(), None)
        }
        ModSetup::Branch(host) => {
            let host_meta = store.load_meta(host).map_err(|_| invalid(format!("no world \"{host}\" to branch")))?;
            if host_meta.mod_storage != ModStorage::World || host_meta.branch_of.is_some() {
                return Err(invalid(format!("\"{host}\" can't be branched from")));
            }
            let ids = scan_mods(&store.world_mods_dir(host)).into_iter().map(|m| m.id).collect();
            (ModStorage::World, ids, Some(host.clone()))
        }
    };
    let (slug, mut meta) = store.create_world(name, seed, mode)?;
    meta.mod_storage = storage;
    meta.mods = mods;
    meta.branch_of = branch;
    // Which namespace each chosen mod runs under here (the conflict manager).
    if meta.branch_of.is_none() {
        let infos: Vec<ModInfo> = meta.mods.iter().filter_map(|f| read_mod(&installed.join(f)).ok()).collect();
        meta.mod_ids = assign_namespaces(&infos, &Default::default());
    }
    if storage == ModStorage::World && meta.branch_of.is_none() {
        if let Err(err) = copy_mods_into_world(store, &slug, installed, &meta.mods) {
            let _ = std::fs::remove_dir_all(store.world_dir(&slug));
            return Err(err);
        }
    }
    store.save_meta(&slug, &meta)?;
    Ok((slug, meta))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::save::GameMode;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A scratch root holding a `SaveStore` and an "installed mods" folder.
    struct Env {
        root: PathBuf,
        store: SaveStore,
        installed: PathBuf,
    }
    impl Env {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("craftmjne-mods-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
            let installed = root.join("installed");
            std::fs::create_dir_all(&installed).unwrap();
            Self { store: SaveStore::at(root.join("data")), root, installed }
        }
        /// Installs folder `folder` declaring mod id `id`.
        fn install_as(&self, folder: &str, id: &str) {
            let dir = self.installed.join(folder);
            std::fs::create_dir_all(dir.join("blocks")).unwrap();
            std::fs::write(dir.join(MANIFEST), format!(r#"{{"id": "{id}", "name": "{folder}"}}"#)).unwrap();
            std::fs::write(dir.join("blocks").join("thing.json"), r#"{"id": "thing"}"#).unwrap();
        }
        fn install(&self, id: &str) {
            self.install_as(id, id);
        }
        fn create(&self, name: &str, setup: &ModSetup) -> (String, WorldMeta) {
            create_world_with_mods(&self.store, name, 1, GameMode::Survival, setup, &self.installed).unwrap()
        }
        fn resolve(&self, slug: &str) -> Result<Vec<String>, ModError> {
            let meta = self.store.load_meta(slug).unwrap();
            resolve_world_mods(&self.store, slug, &meta, &self.installed)
                .map(|r| r.sources.iter().map(|s| s.ns.clone()).collect())
        }
    }
    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn info(folder: &str, id: &str) -> ModInfo {
        ModInfo {
            folder: folder.into(),
            id: id.into(),
            name: id.into(),
            version: String::new(),
            description: String::new(),
            target_ver: Vec::new(),
            path: PathBuf::from(folder),
        }
    }

    #[test]
    fn a_mod_without_an_id_does_not_work_and_says_why() {
        let env = Env::new();
        let dir = env.installed.join("anon");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST), r#"{"name": "No id"}"#).unwrap();
        let why = read_mod(&dir).unwrap_err();
        assert!(why.contains("needs an \"id\""), "{why}");
        let (good, bad) = scan_all(&env.installed);
        assert!(good.is_empty());
        assert_eq!(bad.len(), 1, "it's reported, so the Mods screen can show it");
        assert!(bad[0].1.contains("doesn't work"));
    }

    #[test]
    fn an_id_must_be_plain_and_cannot_be_the_core_games() {
        for bad in ["", "has space", "a:b", "craftmjne", "über"] {
            assert!(!is_valid_id(bad), "{bad:?}");
        }
        for good in ["granite_pack", "my-mod", "Mod2"] {
            assert!(is_valid_id(good), "{good:?}");
        }
        let env = Env::new();
        env.install_as("f", "a:b");
        assert!(read_mod(&env.installed.join("f")).unwrap_err().contains("isn't valid"));
    }

    #[test]
    fn only_folders_with_a_manifest_are_considered_and_the_id_not_the_folder_names_the_mod() {
        let env = Env::new();
        env.install_as("folder_one", "alpha");
        std::fs::create_dir_all(env.installed.join("no_manifest")).unwrap();
        std::fs::write(env.installed.join("stray.txt"), "x").unwrap();
        let (good, bad) = scan_all(&env.installed);
        assert!(bad.is_empty(), "a folder with no mod.json isn't trying to be a mod");
        assert_eq!(
            (good[0].folder.as_str(), good[0].id.as_str(), good[0].name.as_str()),
            ("folder_one", "alpha", "folder_one")
        );
    }

    #[test]
    fn the_conflict_manager_leaves_unique_ids_alone() {
        let out = assign_namespaces(&[info("a", "alpha"), info("b", "beta")], &BTreeMap::new());
        assert_eq!((out["a"].as_str(), out["b"].as_str()), ("alpha", "beta"));
    }

    #[test]
    fn two_mods_with_the_same_id_both_get_a_numbered_one_and_nothing_breaks() {
        let mods = [info("zeta", "x"), info("alpha_folder", "x"), info("other", "y")];
        let out = assign_namespaces(&mods, &BTreeMap::new());
        assert_eq!(out["alpha_folder"], "x_1", "in folder order");
        assert_eq!(out["zeta"], "x_2");
        assert_eq!(out["other"], "y");
        let all: std::collections::HashSet<_> = out.values().collect();
        assert_eq!(all.len(), 3, "every namespace is distinct");
    }

    #[test]
    fn an_assignment_is_remembered_even_after_the_conflict_goes_away() {
        let first = assign_namespaces(&[info("a", "x"), info("b", "x")], &BTreeMap::new());
        // `b` is removed: `a` keeps `x_1`, because blocks saved as `x_1:thing` need it.
        let after = assign_namespaces(&[info("a", "x")], &first);
        assert_eq!(after["a"], "x_1");
        assert_eq!(after["b"], "x_2", "a mod that's off keeps its namespace, in case it comes back");
        // A newcomer declaring `x` can't be given a namespace already in use.
        let later = assign_namespaces(&[info("a", "x"), info("c", "x")], &after);
        assert_eq!(later["a"], "x_1");
        assert_ne!(later["c"], "x_1");
        assert_ne!(later["c"], "x_2", "and nobody else takes it meanwhile");
    }

    #[test]
    fn a_newcomer_declaring_an_id_an_existing_mod_already_uses_gets_a_number() {
        let existing = BTreeMap::from([("a".to_string(), "x".to_string())]);
        let out = assign_namespaces(&[info("a", "x"), info("b", "x")], &existing);
        assert_eq!(out["a"], "x", "the existing one is not renamed");
        assert_eq!(out["b"], "x_1");
    }

    #[test]
    fn an_instance_world_runs_the_installed_copies_and_breaks_if_one_is_removed() {
        let env = Env::new();
        env.install("alpha");
        env.install("beta");
        let (slug, meta) = env.create("W", &ModSetup::Instance(names(&["beta", "alpha"])));
        assert_eq!(meta.mod_storage, ModStorage::Instance);
        assert_eq!(env.resolve(&slug).unwrap(), names(&["alpha", "beta"]), "load order is by folder");
        assert!(!env.store.world_mods_dir(&slug).exists(), "nothing is copied into an instance world");
        std::fs::remove_dir_all(env.installed.join("beta")).unwrap();
        assert_eq!(env.resolve(&slug), Err(ModError::NotInstalled(names(&["beta"]))));
    }

    #[test]
    fn a_world_remembers_the_namespaces_its_mods_were_given() {
        let env = Env::new();
        env.install_as("one", "dup");
        env.install_as("two", "dup");
        let (slug, meta) = env.create("W", &ModSetup::Instance(names(&["one", "two"])));
        assert_eq!(meta.mod_ids, BTreeMap::from([("one".into(), "dup_1".into()), ("two".into(), "dup_2".into())]));
        assert_eq!(env.store.load_meta(&slug).unwrap().mod_ids, meta.mod_ids, "saved with the world");
        assert_eq!(env.resolve(&slug).unwrap(), names(&["dup_1", "dup_2"]));
    }

    #[test]
    fn a_world_storage_world_keeps_its_own_copies_and_survives_the_game_folder_changing() {
        let env = Env::new();
        env.install("alpha");
        env.install("beta");
        let (slug, _) = env.create("W", &ModSetup::World(names(&["alpha"])));
        assert_eq!(env.resolve(&slug).unwrap(), names(&["alpha"]), "only the chosen mod was copied");
        std::fs::remove_dir_all(env.installed.join("alpha")).unwrap();
        assert_eq!(env.resolve(&slug).unwrap(), names(&["alpha"]), "independent of the game's folder");
        // A mod dropped straight into the world's folder - never installed in the game - is used too.
        let drop = env.store.world_mods_dir(&slug).join("gamma");
        std::fs::create_dir_all(&drop).unwrap();
        std::fs::write(drop.join(MANIFEST), r#"{"id": "gamma"}"#).unwrap();
        assert_eq!(env.resolve(&slug).unwrap(), names(&["alpha", "gamma"]));
        std::fs::remove_dir_all(env.store.world_mods_dir(&slug).join("alpha")).unwrap();
        assert!(matches!(env.resolve(&slug), Err(ModError::MissingFromWorld { .. })));
    }

    #[test]
    fn a_branch_uses_its_hosts_mods_and_names_without_keeping_copies_and_shares_its_fate() {
        let env = Env::new();
        env.install("alpha");
        let (host, _) = env.create("Host", &ModSetup::World(names(&["alpha"])));
        let (branch, meta) = env.create("Branch", &ModSetup::Branch(host.clone()));
        assert_eq!(meta.branch_of.as_deref(), Some(host.as_str()));
        assert!(meta.mod_ids.is_empty(), "a branch decides nothing of its own");
        assert!(!env.store.world_mods_dir(&branch).exists());
        assert_eq!(env.resolve(&branch).unwrap(), names(&["alpha"]));
        let extra = env.store.world_mods_dir(&host).join("delta");
        std::fs::create_dir_all(&extra).unwrap();
        std::fs::write(extra.join(MANIFEST), r#"{"id": "delta"}"#).unwrap();
        assert_eq!(env.resolve(&branch).unwrap(), names(&["alpha", "delta"]));
        std::fs::remove_dir_all(env.store.world_dir(&host)).unwrap();
        assert_eq!(env.resolve(&branch), Err(ModError::HostMissing(host)));
    }

    #[test]
    fn only_a_world_storage_world_that_isnt_a_branch_can_be_branched_from() {
        let env = Env::new();
        env.install("alpha");
        let (inst, _) = env.create("Inst", &ModSetup::Instance(names(&["alpha"])));
        let (host, _) = env.create("Host", &ModSetup::World(names(&["alpha"])));
        let (branch, _) = env.create("Branch", &ModSetup::Branch(host));
        let try_branch = |h: &str| {
            create_world_with_mods(&env.store, "X", 1, GameMode::Survival, &ModSetup::Branch(h.into()), &env.installed)
        };
        assert!(try_branch(&inst).is_err());
        assert!(try_branch(&branch).is_err(), "no branches of branches");
        assert!(try_branch("nope").is_err());
        assert_eq!(env.store.list_worlds().len(), 3, "refusals create nothing");
    }

    #[test]
    fn choosing_a_mod_that_isnt_installed_creates_nothing() {
        let env = Env::new();
        let err = create_world_with_mods(&env.store, "W", 1, GameMode::Survival, &ModSetup::Instance(names(&["ghost"])), &env.installed);
        assert!(err.unwrap_err().to_string().contains("ghost"));
        assert!(env.store.list_worlds().is_empty());
    }

    #[test]
    fn an_older_world_with_no_mod_fields_is_an_instance_world_with_no_mods() {
        let old: WorldMeta = serde_json::from_str(r#"{"name":"Old","seed":1,"created_at":0,"last_played_at":0}"#).unwrap();
        assert_eq!((old.mod_storage, old.mods.len(), old.branch_of, old.mod_ids.len()), (ModStorage::Instance, 0, None, 0));
    }

    #[test]
    fn an_instance_worlds_mods_can_be_changed_and_nothing_on_disk_is_touched() {
        let env = Env::new();
        env.install("alpha");
        env.install("beta");
        let (slug, _) = env.create("W", &ModSetup::Instance(names(&["alpha"])));
        let meta = set_world_mods(&env.store, &slug, &env.installed, &names(&["beta", "alpha"])).unwrap();
        assert_eq!(meta.mods, names(&["alpha", "beta"]));
        assert_eq!(env.resolve(&slug).unwrap(), names(&["alpha", "beta"]));
        set_world_mods(&env.store, &slug, &env.installed, &[]).unwrap();
        assert!(env.resolve(&slug).unwrap().is_empty());
        assert!(env.installed.join("alpha").is_dir() && env.installed.join("beta").is_dir());
        let err = set_world_mods(&env.store, &slug, &env.installed, &names(&["ghost"])).unwrap_err();
        assert!(err.to_string().contains("ghost"));
        assert!(env.resolve(&slug).unwrap().is_empty(), "a refused change changes nothing");
    }

    #[test]
    fn turning_an_in_world_mod_off_keeps_its_copy_and_turning_it_on_brings_it_back() {
        let env = Env::new();
        env.install("alpha");
        let (slug, _) = env.create("W", &ModSetup::World(names(&["alpha"])));
        // A mod that only this world has (dropped in by hand, never installed).
        let only = env.store.world_mods_dir(&slug).join("only_here");
        std::fs::create_dir_all(&only).unwrap();
        std::fs::write(only.join(MANIFEST), r#"{"id": "only_here"}"#).unwrap();
        set_world_mods(&env.store, &slug, &env.installed, &[]).unwrap();
        assert!(env.resolve(&slug).unwrap().is_empty());
        let off = disabled_mods_dir(&env.store, &slug);
        assert!(off.join("only_here").join(MANIFEST).is_file(), "the only copy is moved aside, never deleted");
        assert!(off.join("alpha").is_dir());
        // Back on: from the world's own shelf (the only-here mod isn't installed anywhere).
        set_world_mods(&env.store, &slug, &env.installed, &names(&["only_here", "alpha"])).unwrap();
        assert_eq!(env.resolve(&slug).unwrap(), names(&["alpha", "only_here"]));
        assert!(!off.join("only_here").exists());
    }

    #[test]
    fn a_mods_namespace_survives_it_being_turned_off_and_on() {
        let env = Env::new();
        env.install_as("one", "dup");
        env.install_as("two", "dup");
        let (slug, _) = env.create("W", &ModSetup::Instance(names(&["one", "two"])));
        set_world_mods(&env.store, &slug, &env.installed, &names(&["two"])).unwrap();
        assert_eq!(env.resolve(&slug).unwrap(), names(&["dup_2"]), "two keeps dup_2 although one is off");
        let meta = set_world_mods(&env.store, &slug, &env.installed, &names(&["one", "two"])).unwrap();
        assert_eq!(meta.mod_ids["one"], "dup_1", "and one is dup_1 again, so what it saved still finds it");
    }

    #[test]
    fn a_branchs_mods_are_changed_on_its_host_not_on_it() {
        let env = Env::new();
        env.install("alpha");
        let (host, _) = env.create("Host", &ModSetup::World(names(&["alpha"])));
        let (branch, _) = env.create("Branch", &ModSetup::Branch(host));
        let err = set_world_mods(&env.store, &branch, &env.installed, &[]).unwrap_err();
        assert!(err.to_string().contains("host"));
    }

    #[test]
    fn a_target_version_covers_exact_prefix_and_wildcard_matches_only() {
        for (entry, game, expect) in [
            ("1.3.0", "1.3.0", true),
            ("1.3.0", "1.3.1", false),
            ("1.3", "1.3.0", true),
            ("1.3", "1.3.7", true),
            ("1.3", "1.4.0", false),
            ("1", "1.9.2", true),
            ("1", "2.0.0", false),
            ("1.x", "1.9.2", true),
            ("1.*.0", "1.4.0", true),
            ("1.x.0", "1.4.1", false),
            ("v1.3", "1.3.0", true),
            ("1.3.0.1", "1.3.0", false),
            ("", "1.3.0", false),
            ("1..0", "1.3.0", false),
            ("abc", "1.3.0", false),
        ] {
            assert_eq!(version_matches(entry, game), expect, "{entry:?} vs {game:?}");
        }
    }

    fn with_target(target: &[&str]) -> ModInfo {
        ModInfo { target_ver: target.iter().map(|s| s.to_string()).collect(), ..info("f", "id") }
    }

    #[test]
    fn a_mod_made_for_another_version_gets_a_warning_and_one_made_for_this_one_does_not() {
        assert_eq!(with_target(&["1.3"]).warning("1.3.0"), None);
        assert_eq!(with_target(&["1.2", "1.3.x"]).warning("1.3.4"), None, "any one of several will do");
        let w = with_target(&["1.2", "1.1"]).warning("1.3.0").expect("a warning");
        assert!(w.contains("1.2 or 1.1") && w.contains("1.3.0") && w.contains("may not work"), "{w}");
        let none = with_target(&[]).warning("1.3.0").expect("a warning");
        assert!(none.contains("target_ver"), "{none}");
    }

    #[test]
    fn target_ver_can_be_one_version_or_a_list_and_a_mod_without_it_still_loads() {
        let env = Env::new();
        let write = |folder: &str, body: &str| {
            let dir = env.installed.join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(MANIFEST), body).unwrap();
            read_mod(&dir).unwrap()
        };
        assert_eq!(write("a", r#"{"id": "a", "target_ver": "1.3"}"#).target_ver, names(&["1.3"]));
        assert_eq!(write("b", r#"{"id": "b", "target_ver": ["1.2", "1.3"]}"#).target_ver, names(&["1.2", "1.3"]));
        assert!(write("c", r#"{"id": "c"}"#).target_ver.is_empty(), "still a working mod, with a warning");
    }

    #[test]
    fn a_worlds_mods_report_a_warning_for_each_one_made_for_another_version() {
        let env = Env::new();
        let install = |folder: &str, target: &str| {
            env.install(folder);
            std::fs::write(env.installed.join(folder).join(MANIFEST), format!(r#"{{"id": "{folder}", "target_ver": {target}}}"#)).unwrap();
        };
        install("good", &format!("\"{GAME_VERSION}\""));
        install("old", r#"["0.1"]"#);
        let (slug, _) = env.create("W", &ModSetup::Instance(names(&["good", "old"])));
        let meta = env.store.load_meta(&slug).unwrap();
        let resolved = resolve_world_mods(&env.store, &slug, &meta, &env.installed).unwrap();
        assert_eq!(resolved.warnings.len(), 1);
        assert!(resolved.warnings[0].contains("old") && resolved.warnings[0].contains("0.1"), "{:?}", resolved.warnings);
        assert_eq!(resolved.sources.len(), 2, "a warning doesn't stop the mod loading");
    }
}
