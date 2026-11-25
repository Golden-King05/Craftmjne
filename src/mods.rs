//! **Mods**: folders of data (blocks, items, recipes, textures) that add to or
//! change the game's content. A mod is data only - no code - so everything it
//! can do is something a block or item file can already say; see `tags.rs`.
//!
//! ```text
//! mods/<id>/
//!   mod.json            { "name": "Batteries", "version": "1.0", "description": "..." }
//!   blocks/*.json       same format as the game's own blocks/
//!   items/*.json
//!   recipes/*.json
//!   textures/blocks/*.png
//! ```
//!
//! The folder name is the mod's id. Every folder is optional except
//! `mod.json`, which is what marks a directory as a mod at all.
//!
//! ## Mods sit on top of the game; they never change it
//!
//! A mod is an **overlay**. The game's own files are only ever read: nothing a
//! mod does edits, replaces or deletes them, so taking a mod off puts everything
//! back exactly as it was. What an overlay can do:
//!
//! - **Add** a block or item: a new `id`.
//! - **Change** a core block or item: give a file the same `id` and only the
//!   keys to change. `{"id": "dirt", "hardness": 8}` makes dirt slow to mine and
//!   leaves every other property of dirt alone. Objects merge key by key, and a
//!   key set to `null` is removed so it falls back to its default.
//! - **Replace** a core recipe by naming a file the same as the core one; a new
//!   name adds a recipe.
//! - **Re-skin** with a texture of the same name as a core one.
//!
//! With several mods, later ones are laid over earlier ones (load order is by
//! mod id). The blocks the core files define keep their ids whatever mods are on
//! (derived waterlogged variants may shift, which nothing depends on: saves
//! store names, never numeric ids).
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

/// What `mod.json` says. Every field is optional.
#[derive(Deserialize, Default)]
struct Manifest {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
}

/// An installed mod.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModInfo {
    /// The folder name.
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: PathBuf,
}

/// Whether `id` can be a mod's folder name: letters, digits, `_` and `-`.
/// (It also names the texture and block ids a mod adds, so no spaces or colons.)
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
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

/// Reads the mod in `path`, or says why it isn't one.
pub fn read_mod(path: &Path) -> Result<ModInfo, String> {
    let id = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    if !is_valid_id(&id) {
        return Err(format!("{id:?} isn't a valid mod folder name (letters, digits, _ and - only)"));
    }
    let text = std::fs::read_to_string(path.join(MANIFEST)).map_err(|_| format!("{id}: no {MANIFEST}"))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|e| format!("{id}: bad {MANIFEST}: {e}"))?;
    Ok(ModInfo {
        name: manifest.name.unwrap_or_else(|| id.clone()),
        version: manifest.version.unwrap_or_default(),
        description: manifest.description.unwrap_or_default(),
        id,
        path: path.to_path_buf(),
    })
}

/// Every mod in `dir`, sorted by id. A folder that isn't a valid mod is skipped.
pub fn scan_mods(dir: &Path) -> Vec<ModInfo> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut mods: Vec<ModInfo> =
        entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).filter_map(|p| read_mod(&p).ok()).collect();
    mods.sort_by(|a, b| a.id.cmp(&b.id));
    mods
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

/// The mod folders world `slug` runs, in load order (sorted by id). `installed`
/// is the game's `mods/` folder.
///
/// See the module docs for the three storage styles. An `Err` means the world
/// must not open: loading it without those mods would drop their blocks from
/// its next save.
pub fn resolve_world_mods(
    store: &SaveStore,
    slug: &str,
    meta: &WorldMeta,
    installed: &Path,
) -> Result<Vec<PathBuf>, ModError> {
    if let Some(host) = &meta.branch_of {
        let host_meta = store.load_meta(host).map_err(|_| ModError::HostMissing(host.clone()))?;
        if host_meta.branch_of.is_some() || host_meta.mod_storage != ModStorage::World {
            return Err(ModError::HostCannotShare(host.clone()));
        }
        return Ok(scan_mods(&store.world_mods_dir(host)).into_iter().map(|m| m.path).collect());
    }
    match meta.mod_storage {
        ModStorage::Instance => {
            let mut missing = Vec::new();
            let mut paths = Vec::new();
            let mut ids = meta.mods.clone();
            ids.sort();
            ids.dedup();
            for id in ids {
                match read_mod(&installed.join(&id)) {
                    Ok(m) => paths.push(m.path),
                    Err(_) => missing.push(id),
                }
            }
            if missing.is_empty() {
                Ok(paths)
            } else {
                Err(ModError::NotInstalled(missing))
            }
        }
        ModStorage::World => {
            let dir = store.world_mods_dir(slug);
            let present: Vec<String> = scan_mods(&dir).into_iter().map(|m| m.id).collect();
            let missing: Vec<String> = meta.mods.iter().filter(|id| !present.contains(id)).cloned().collect();
            if !missing.is_empty() {
                return Err(ModError::MissingFromWorld { holder: slug.to_string(), missing });
            }
            Ok(scan_mods(&dir).into_iter().map(|m| m.path).collect())
        }
    }
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
    pub mods: Vec<PathBuf>,
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
