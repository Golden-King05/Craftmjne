//! Controls that mods add to the Keybinds screen.
//!
//! A mod puts one JSON file per control (or an array of them) in `keybinds/`:
//!
//! ```json
//! {
//!   "id": "noon",
//!   "name": "Skip to noon",
//!   "category": "Weather Pack",
//!   "default": "Ctrl+N",
//!   "command": "/time set noon"
//! }
//! ```
//!
//! - `id` (required): letters, digits, `_`, `-`. The control is then called
//!   `<mod id>:<id>` everywhere - in the saved controls, in search (`granite_pack:noon`),
//!   and to code asking `Actions::just_pressed("granite_pack:noon")`.
//! - `name`: what the screen shows (default: the id).
//! - `category`: the heading it is listed under (default: the mod's name).
//! - `default`: the chord it starts on (`Ctrl+N`, `Left Click`...); missing or
//!   `none` starts it unbound, so a mod can never take a key from the player unasked.
//! - `command`: a chat command (slash optional) run when the control is pressed in a
//!   world that uses the mod. Without one the control is only for code to read.
//!
//! These are read from the **installed** mods folder, so they show up in Settings
//! whether or not any world uses the mod; in play they only do anything in a world
//! that has the mod on. A bad file is skipped with a note, never an error - one
//! mistake in one mod must not stop the screen (or the game) opening.

use std::path::Path;

use bevy::prelude::*;
use serde::Deserialize;

use crate::keybinds::{Bindings, Chord, ControlDef, ControlSource};
use crate::mods::{is_valid_id, scan_mods, InstalledMods, ModInfo};
use crate::state::AppState;

pub const FOLDER: &str = "keybinds";

#[derive(Deserialize)]
struct ControlFile {
    id: Option<String>,
    name: Option<String>,
    category: Option<String>,
    default: Option<String>,
    /// A controller button (`south`, `right bumper`, `none`).
    default_pad: Option<String>,
    command: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FileBody {
    Many(Vec<ControlFile>),
    One(ControlFile),
}

/// What went wrong with mod controls, for the Keybinds screen to show.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct ControlNotes(pub Vec<String>);

/// The controls one mod defines, and a note for everything skipped.
pub fn controls_of(info: &ModInfo) -> (Vec<ControlDef>, Vec<String>) {
    let mut defs = Vec::new();
    let mut notes = Vec::new();
    let dir = info.path.join(FOLDER);
    let Ok(read) = std::fs::read_dir(&dir) else { return (defs, notes) };
    let mut files: Vec<_> = read.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    for path in files {
        let file = format!("{}/{}/{}", info.name, FOLDER, path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
        let body: FileBody = match std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
            Ok(body) => body,
            Err(err) => {
                notes.push(format!("{file}: couldn't be read ({err})"));
                continue;
            }
        };
        let entries = match body {
            FileBody::Many(list) => list,
            FileBody::One(one) => vec![one],
        };
        for entry in entries {
            let Some(local) = entry.id.as_deref().map(str::trim).filter(|id| is_valid_id(id)) else {
                notes.push(format!("{file}: a control needs an \"id\" of letters, digits, _ or -"));
                continue;
            };
            let default = match entry.default.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(text) if text.eq_ignore_ascii_case("none") => None,
                Some(text) => match Chord::parse(text) {
                    Some(chord) => Some(chord),
                    None => {
                        notes.push(format!("{file}: \"{text}\" isn't a key combination I know, so {local} starts unbound"));
                        None
                    }
                },
            };
            let default_pad = match entry.default_pad.as_deref().map(str::trim) {
                None | Some("") => None,
                Some(text) if text.eq_ignore_ascii_case("none") => None,
                Some(text) => match crate::gamepad::PadButton::parse(text) {
                    Some(button) => Some(button),
                    None => {
                        notes.push(format!("{file}: \"{text}\" isn't a controller button I know, so {local} starts unbound on a controller"));
                        None
                    }
                },
            };
            defs.push(ControlDef {
                id: format!("{}:{local}", info.id),
                label: entry.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| local.to_string()),
                category: entry.category.filter(|c| !c.trim().is_empty()).unwrap_or_else(|| info.name.clone()),
                default,
                default_pad,
                command: entry.command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty()),
                source: Some(ControlSource { mod_id: info.id.clone(), mod_name: info.name.clone(), path: info.path.clone() }),
            });
        }
    }
    (defs, notes)
}

/// Every installed mod's controls, plus notes.
pub fn load_mod_controls(installed: &Path) -> (Vec<ControlDef>, Vec<String>) {
    let mut defs = Vec::new();
    let mut notes = Vec::new();
    for info in scan_mods(installed) {
        let (d, n) = controls_of(&info);
        defs.extend(d);
        notes.extend(n);
    }
    (defs, notes)
}

/// Re-reads the installed mods' controls into [`Bindings`]. Touches nothing when
/// nothing changed, so it doesn't make the settings screen rebuild for no reason.
pub fn rescan_mod_controls(installed: Option<Res<InstalledMods>>, mut bindings: ResMut<Bindings>, mut notes: ResMut<ControlNotes>) {
    let Some(installed) = installed else { return };
    let (defs, mut found) = load_mod_controls(&installed.0);
    if bindings.mod_defs() != defs {
        for id in bindings.set_mod_controls(defs) {
            found.push(format!("{id} was ignored: another control already has that id"));
        }
    }
    if notes.0 != found {
        notes.0 = found;
    }
}

pub struct ModControlsPlugin;

impl Plugin for ModControlsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ControlNotes>()
            .add_systems(Startup, rescan_mod_controls)
            .add_systems(OnEnter(AppState::Settings), rescan_mod_controls)
            .add_systems(OnEnter(AppState::InGame), rescan_mod_controls);
    }
}

#[cfg(test)]
pub(crate) mod support {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static N: AtomicU64 = AtomicU64::new(0);

    pub(crate) struct Installed(pub std::path::PathBuf);
    impl Drop for Installed {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An installed-mods folder holding `mod_id` with the given `keybinds/` files.
    pub(crate) fn install(mod_id: &str, files: &[(&str, &str)]) -> Installed {
        let dir = std::env::temp_dir().join(format!("craftmjne-modkeys-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        add_mod(&dir, mod_id, files);
        Installed(dir)
    }

    pub(crate) fn add_mod(installed: &Path, mod_id: &str, files: &[(&str, &str)]) {
        let folder = installed.join(mod_id);
        std::fs::create_dir_all(folder.join(FOLDER)).unwrap();
        std::fs::write(folder.join("mod.json"), format!(r#"{{"id": "{mod_id}", "name": "Mod {mod_id}", "target_ver": "2"}}"#)).unwrap();
        for (name, body) in files {
            std::fs::write(folder.join(FOLDER).join(name), body).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::support::*;

    #[test]
    fn a_mods_control_is_namespaced_labelled_and_starts_where_it_says() {
        let i = install("weather", &[("noon.json", r#"{"id": "noon", "name": "Skip to noon", "category": "Weather", "default": "Ctrl+N", "command": "/time set noon"}"#)]);
        let (defs, notes) = load_mod_controls(&i.0);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(defs.len(), 1);
        let d = &defs[0];
        assert_eq!(d.id, "weather:noon");
        assert_eq!((d.label.as_str(), d.category.as_str()), ("Skip to noon", "Weather"));
        assert_eq!(d.default, Chord::parse("Ctrl+N"));
        assert_eq!(d.command.as_deref(), Some("/time set noon"));
        assert_eq!(d.source.as_ref().unwrap().mod_id, "weather");
    }

    #[test]
    fn everything_but_the_id_is_optional_and_nothing_is_bound_unasked() {
        let i = install("m", &[("a.json", r#"{"id": "a"}"#)]);
        let (defs, notes) = load_mod_controls(&i.0);
        assert!(notes.is_empty());
        let d = &defs[0];
        assert_eq!((d.label.as_str(), d.category.as_str()), ("a", "Mod m"), "name defaults to the id, category to the mod's name");
        assert_eq!(d.default, None, "no default means unbound");
        assert_eq!(d.command, None);
    }

    #[test]
    fn a_file_can_hold_several_controls() {
        let i = install("m", &[("both.json", r#"[{"id": "x", "default": "F7"}, {"id": "y", "default": "none"}]"#)]);
        let (defs, _) = load_mod_controls(&i.0);
        assert_eq!(defs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["m:x", "m:y"]);
        assert_eq!(defs[1].default, None);
    }

    #[test]
    fn bad_files_are_skipped_with_a_note_and_the_good_ones_still_load() {
        let i = install(
            "m",
            &[
                ("1_good.json", r#"{"id": "ok", "default": "F8"}"#),
                ("2_garbage.json", "not json"),
                ("3_noid.json", r#"{"name": "No id"}"#),
                ("4_badid.json", r#"{"id": "has space"}"#),
                ("5_badkey.json", r#"{"id": "k", "default": "Ctrl+Nope"}"#),
                ("notes.txt", "ignored, not json"),
            ],
        );
        let (defs, notes) = load_mod_controls(&i.0);
        assert_eq!(defs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["m:ok", "m:k"], "the bad-key control still exists, unbound");
        assert_eq!(defs[1].default, None);
        assert_eq!(notes.len(), 4, "{notes:?}");
        assert!(notes.iter().any(|n| n.contains("2_garbage.json") && n.contains("couldn't be read")));
        assert!(notes.iter().any(|n| n.contains("3_noid.json") && n.contains("\"id\"")));
        assert!(notes.iter().any(|n| n.contains("Ctrl+Nope")));
    }

    #[test]
    fn a_mod_cannot_take_a_built_in_control_and_a_clash_between_mods_is_reported() {
        // "jump" has no colon so a mod can't even spell it; the id is always `mod:name`.
        let i = install("a", &[("j.json", r#"{"id": "jump", "default": "Space"}"#)]);
        add_mod(&i.0, "b", &[]);
        let mut bindings = Bindings::default();
        let (defs, _) = load_mod_controls(&i.0);
        assert!(bindings.set_mod_controls(defs).is_empty());
        assert_eq!(bindings.get("jump"), Action_default("jump"), "the built-in is untouched");
        assert_eq!(bindings.get("a:jump"), Chord::parse("Space"));
        // Registering the same id twice (two installed mods declaring one id) keeps the first.
        let again = ControlDef::new("a:jump", "Other", "X");
        assert!(!bindings.register(again));
        assert_eq!(bindings.def("a:jump").unwrap().label, "jump");
    }

    #[allow(non_snake_case)]
    fn Action_default(id: &str) -> Option<Chord> {
        crate::keybinds::Action::from_id(id).unwrap().default_chord()
    }

    #[test]
    fn a_players_key_for_a_mod_control_survives_the_mod_being_removed_and_returning() {
        let i = install("m", &[("c.json", r#"{"id": "c", "default": "F7"}"#)]);
        let mut bindings = Bindings::default();
        bindings.set_mod_controls(load_mod_controls(&i.0).0);
        bindings.set("m:c", Chord::parse("Ctrl+K"));
        let saved = bindings.overrides();
        assert_eq!(saved["m:c"], "Ctrl+K");

        // Mod removed: its control is gone from the screen but its key is still in what gets saved.
        bindings.set_mod_controls(Vec::new());
        assert!(bindings.def("m:c").is_none());
        assert_eq!(bindings.overrides()["m:c"], "Ctrl+K", "a removed mod must not cost the player their key");

        // A fresh start with the mod absent also keeps it...
        let mut restarted = Bindings::from_overrides(&saved);
        assert_eq!(restarted.overrides()["m:c"], "Ctrl+K");
        // ...and the mod coming back picks it up instead of its default.
        restarted.set_mod_controls(load_mod_controls(&i.0).0);
        assert_eq!(restarted.get("m:c"), Chord::parse("Ctrl+K"));
        assert!(!restarted.is_default("m:c"));
    }

    #[test]
    fn reset_all_also_clears_keys_parked_for_absent_mods() {
        let mut saved = std::collections::BTreeMap::new();
        saved.insert("gone:thing".to_string(), "F9".to_string());
        let mut bindings = Bindings::from_overrides(&saved);
        assert!(bindings.overrides().contains_key("gone:thing"));
        bindings.reset_all();
        assert!(bindings.overrides().is_empty());
    }

    #[test]
    fn the_example_mod_in_the_docs_adds_a_working_control() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs");
        let (defs, notes) = load_mod_controls(&docs);
        assert!(notes.is_empty(), "{notes:?}");
        let find = defs.iter().find(|d| d.id == "example_mod:find_snow").expect("the example's control");
        assert_eq!(find.default, Chord::parse("Ctrl+F7"));
        let command = find.command.as_deref().unwrap();
        // It names a command that really exists, so the example isn't a dead key.
        let registry = crate::commands::CommandRegistry::with_defaults();
        let name = command.trim_start_matches('/').split_whitespace().next().unwrap();
        assert!(registry.suggestions(name).iter().any(|s| s.text == name), "{name} is not a command");
    }

    #[test]
    fn a_missing_keybinds_folder_is_simply_no_controls() {
        let dir = std::env::temp_dir().join(format!("craftmjne-modkeys-none-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("m")).unwrap();
        std::fs::write(dir.join("m").join("mod.json"), r#"{"id": "m"}"#).unwrap();
        let (defs, notes) = load_mod_controls(&dir);
        assert!(defs.is_empty() && notes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
