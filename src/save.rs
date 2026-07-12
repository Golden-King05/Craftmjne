//! Local world-save storage: one folder per world under the OS's per-user
//! app-data directory — the same directory the installer and auto-updater
//! use (`%LOCALAPPDATA%\Craftmjne` on Windows), so saves are naturally
//! scoped to the OS user account, matching how most desktop games do it.
//!
//! Layout: `<app_data_dir>/saves/<slug>/{meta.json, data.json}`
//!   - `meta.json` — name, seed, game mode, timestamps. Cheap to read for
//!     the world list.
//!   - `data.json` — player position + block edits. Only touched when a
//!     world is actually entered or left (autosave included).
//!
//! Block edits are stored by block *name* rather than numeric id, so saves
//! stay valid even if a mod changes block registration order.
//!
//! All operations hang off [`SaveStore`], which just wraps a root directory —
//! production code uses `SaveStore::default()` (the real per-user app data
//! dir); tests construct one pointed at a throwaway temp directory, so the
//! logic here is fully testable without touching a real user profile.

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn app_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("Craftmjne");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join("Library/Application Support/Craftmjne");
        }
    }
    // Linux (and fallback for any other target): XDG data dir.
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("craftmjne");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/craftmjne");
    }
    PathBuf::from(".craftmjne")
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Chosen when a world is created and fixed for its lifetime. For now the
/// only behavioural difference is flying (see `player.rs`); more will hang
/// off this as the framework grows (block breaking speed, hunger, etc).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Resource, Default)]
pub enum GameMode {
    #[default]
    Survival,
    Creative,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorldMeta {
    pub name: String,
    pub seed: u32,
    /// `#[serde(default)]` so worlds saved before game modes existed still
    /// load - they come back as `Survival`, the more restrictive default.
    #[serde(default)]
    pub mode: GameMode,
    /// Set permanently the first time any chat command is used in this
    /// world (see `commands.rs`) and never cleared again - mirrors
    /// Minecraft's own "cheats" world flag, which exists so a save that's
    /// had commands run in it can be disqualified from future achievements.
    /// Not surfaced anywhere in the UI on purpose.
    #[serde(default)]
    pub cheats: bool,
    pub created_at: u64,
    pub last_played_at: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Default)]
pub struct PlayerSave {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub fly: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct BlockEdit {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: String,
    /// The placed orientation (`blocks::AXIS_X/Y/Z`) for a rotating block;
    /// meaningless (and ignored) for anything else, so an old save missing
    /// this field entirely just defaults to `0` with no ill effect.
    #[serde(default)]
    pub axis: u8,
}

/// One fluid cell's exact state, saved so a reload restores water (and any
/// other fluid) exactly as it was left - not just the source a player
/// placed, but every cell it spread into - with zero re-simulation. Unlike
/// `BlockEdit` (a sparse diff of cells a player touched), this is closer to
/// a full snapshot of a chunk's fluid footprint - see `world.rs`'s
/// `write_save` for why a fluid can't use the same sparse-diff approach a
/// solid block edit can (every fluid cell matters, not just ones that
/// differ from what terrain generation would have produced).
#[derive(Serialize, Deserialize, Clone)]
pub struct FluidCell {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub block: String,
    pub level: u8,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct WorldData {
    pub player: Option<PlayerSave>,
    pub edits: Vec<BlockEdit>,
    #[serde(default)]
    pub fluids: Vec<FluidCell>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct GraphicsSettings {
    pub render_distance: i32,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self { render_distance: 8 }
    }
}

/// Keeps directory names filesystem-safe and stable; the human-readable
/// name (with any characters the player typed) is preserved as-is in
/// `meta.json` — only the on-disk folder name is sanitized.
fn slugify(name: &str) -> String {
    let collapsed = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' { c } else { '_' })
        .collect::<String>();
    let mut slug = collapsed.split_whitespace().collect::<Vec<_>>().join("_");
    slug.truncate(48);
    if slug.is_empty() {
        slug = "world".to_string();
    }
    slug
}

/// Injected as a `Resource` (real per-user dir in production, a throwaway
/// temp dir in tests and smoke mode) so nothing in the engine hardcodes
/// where saves live — see `main.rs` and `tests/headless.rs`.
#[derive(Resource)]
pub struct SaveStore {
    root: PathBuf,
}

impl Default for SaveStore {
    fn default() -> Self {
        Self { root: app_data_dir() }
    }
}

impl SaveStore {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    fn saves_dir(&self) -> PathBuf {
        self.root.join("saves")
    }

    fn settings_path(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    fn meta_path(&self, slug: &str) -> PathBuf {
        self.saves_dir().join(slug).join("meta.json")
    }

    fn data_path(&self, slug: &str) -> PathBuf {
        self.saves_dir().join(slug).join("data.json")
    }

    fn unique_slug(&self, base: &str) -> String {
        let dir = self.saves_dir();
        if !dir.join(base).exists() {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base}_{n}");
            if !dir.join(&candidate).exists() {
                return candidate;
            }
            n += 1;
        }
    }

    pub fn list_worlds(&self) -> Vec<(String, WorldMeta)> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.saves_dir()) else { return out };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(slug) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if let Ok(meta) = self.load_meta(slug) {
                out.push((slug.to_string(), meta));
            }
        }
        out.sort_by(|a, b| b.1.last_played_at.cmp(&a.1.last_played_at));
        out
    }

    pub fn create_world(&self, name: &str, seed: u32, mode: GameMode) -> io::Result<(String, WorldMeta)> {
        let slug = self.unique_slug(&slugify(name));
        fs::create_dir_all(self.saves_dir().join(&slug))?;
        let now = now_unix();
        let meta = WorldMeta {
            name: if name.trim().is_empty() { "New World".to_string() } else { name.trim().to_string() },
            seed,
            mode,
            cheats: false,
            created_at: now,
            last_played_at: now,
        };
        self.save_meta(&slug, &meta)?;
        Ok((slug, meta))
    }

    pub fn load_meta(&self, slug: &str) -> io::Result<WorldMeta> {
        let text = fs::read_to_string(self.meta_path(slug))?;
        serde_json::from_str(&text).map_err(io::Error::other)
    }

    pub fn save_meta(&self, slug: &str, meta: &WorldMeta) -> io::Result<()> {
        fs::create_dir_all(self.saves_dir().join(slug))?;
        fs::write(self.meta_path(slug), serde_json::to_string_pretty(meta).map_err(io::Error::other)?)
    }

    pub fn touch_last_played(&self, slug: &str) {
        if let Ok(mut meta) = self.load_meta(slug) {
            meta.last_played_at = now_unix();
            let _ = self.save_meta(slug, &meta);
        }
    }

    pub fn load_data(&self, slug: &str) -> WorldData {
        fs::read_to_string(self.data_path(slug))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save_data(&self, slug: &str, data: &WorldData) -> io::Result<()> {
        fs::create_dir_all(self.saves_dir().join(slug))?;
        fs::write(self.data_path(slug), serde_json::to_string(data).map_err(io::Error::other)?)
    }

    pub fn load_graphics_settings(&self) -> GraphicsSettings {
        fs::read_to_string(self.settings_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save_graphics_settings(&self, settings: &GraphicsSettings) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::write(
            self.settings_path(),
            serde_json::to_string_pretty(settings).map_err(io::Error::other)?,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A `SaveStore` rooted at a fresh temp directory, cleaned up on drop.
    struct TempStore {
        store: SaveStore,
        root: PathBuf,
    }
    impl std::ops::Deref for TempStore {
        type Target = SaveStore;
        fn deref(&self) -> &SaveStore {
            &self.store
        }
    }
    impl Drop for TempStore {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    fn temp_store() -> TempStore {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("craftmjne-test-{}-{n}", std::process::id()));
        TempStore { store: SaveStore::at(root.clone()), root }
    }

    #[test]
    fn create_list_and_load_round_trip() {
        let store = temp_store();
        let (slug, meta) = store.create_world("My World", 42, GameMode::Survival).unwrap();
        assert_eq!(meta.name, "My World");
        assert_eq!(meta.seed, 42);
        assert_eq!(meta.mode, GameMode::Survival);

        let listed = store.list_worlds();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].0, slug);
        assert_eq!(listed[0].1.name, "My World");

        let reloaded = store.load_meta(&slug).unwrap();
        assert_eq!(reloaded.seed, 42);
    }

    #[test]
    fn duplicate_names_get_distinct_slugs() {
        let store = temp_store();
        let (slug_a, _) = store.create_world("Home", 1, GameMode::Survival).unwrap();
        let (slug_b, _) = store.create_world("Home", 2, GameMode::Survival).unwrap();
        assert_ne!(slug_a, slug_b);
        assert_eq!(store.list_worlds().len(), 2);
    }

    #[test]
    fn unsafe_characters_are_sanitized_out_of_the_slug() {
        let store = temp_store();
        let (slug, _) = store.create_world("../../etc/passwd", 1, GameMode::Survival).unwrap();
        assert!(!slug.contains('/'));
        assert!(!slug.contains(".."));
    }

    #[test]
    fn blank_name_falls_back_to_a_default() {
        let store = temp_store();
        let (_, meta) = store.create_world("   ", 1, GameMode::Survival).unwrap();
        assert_eq!(meta.name, "New World");
    }

    #[test]
    fn world_data_round_trips_player_and_edits() {
        let store = temp_store();
        let (slug, _) = store.create_world("Edits", 7, GameMode::Survival).unwrap();

        assert!(store.load_data(&slug).player.is_none()); // fresh world: no save yet

        let data = WorldData {
            player: Some(PlayerSave { x: 1.0, y: 2.0, z: 3.0, yaw: 0.5, pitch: -0.2, fly: true }),
            edits: vec![BlockEdit { x: 1, y: 2, z: 3, block: "stone".into(), axis: 0 }],
            fluids: vec![FluidCell { x: 4, y: 5, z: 6, block: "water".into(), level: 2 }],
        };
        store.save_data(&slug, &data).unwrap();

        let loaded = store.load_data(&slug);
        assert_eq!(loaded.player.unwrap().x, 1.0);
        assert_eq!(loaded.edits.len(), 1);
        assert_eq!(loaded.edits[0].block, "stone");
        assert_eq!(loaded.fluids.len(), 1);
        assert_eq!(loaded.fluids[0].level, 2);
    }

    #[test]
    fn missing_fluids_field_in_old_saves_loads_as_empty() {
        let store = temp_store();
        let (slug, _) = store.create_world("Old Water", 1, GameMode::Survival).unwrap();
        // Simulate a data.json written before `fluids` existed.
        fs::write(
            store.data_path(&slug),
            r#"{"player":null,"edits":[{"x":1,"y":2,"z":3,"block":"stone"}]}"#,
        )
        .unwrap();
        let data = store.load_data(&slug);
        assert_eq!(data.edits.len(), 1);
        assert!(data.fluids.is_empty());
    }

    #[test]
    fn touch_last_played_bumps_the_timestamp_forward() {
        let store = temp_store();
        let (slug, meta) = store.create_world("Timestamps", 1, GameMode::Survival).unwrap();
        let mut earlier = meta.clone();
        earlier.last_played_at = 0;
        store.save_meta(&slug, &earlier).unwrap();

        store.touch_last_played(&slug);
        assert!(store.load_meta(&slug).unwrap().last_played_at > 0);
    }

    #[test]
    fn graphics_settings_round_trip_and_default_when_missing() {
        let store = temp_store();
        assert_eq!(store.load_graphics_settings().render_distance, 8);
        store.save_graphics_settings(&GraphicsSettings { render_distance: 12 }).unwrap();
        assert_eq!(store.load_graphics_settings().render_distance, 12);
    }

    #[test]
    fn creative_mode_round_trips() {
        let store = temp_store();
        let (slug, meta) = store.create_world("Creative", 1, GameMode::Creative).unwrap();
        assert_eq!(meta.mode, GameMode::Creative);
        let reloaded = store.load_meta(&slug).unwrap();
        assert_eq!(reloaded.mode, GameMode::Creative);
    }

    #[test]
    fn missing_mode_field_in_old_saves_defaults_to_survival() {
        let store = temp_store();
        let (slug, _) = store.create_world("Old", 1, GameMode::Creative).unwrap();
        // Simulate a meta.json written before `mode` existed.
        fs::write(
            store.meta_path(&slug),
            r#"{"name":"Old","seed":1,"created_at":0,"last_played_at":0}"#,
        )
        .unwrap();
        let meta = store.load_meta(&slug).unwrap();
        assert_eq!(meta.mode, GameMode::Survival);
        assert!(!meta.cheats); // missing `cheats` also defaults sanely
    }

    #[test]
    fn new_worlds_start_without_cheats() {
        let store = temp_store();
        let (_, meta) = store.create_world("Fresh", 1, GameMode::Survival).unwrap();
        assert!(!meta.cheats);
    }

    #[test]
    fn cheats_flag_round_trips_once_set() {
        let store = temp_store();
        let (slug, mut meta) = store.create_world("Cheated", 1, GameMode::Survival).unwrap();
        meta.cheats = true;
        store.save_meta(&slug, &meta).unwrap();
        assert!(store.load_meta(&slug).unwrap().cheats);
    }
}
