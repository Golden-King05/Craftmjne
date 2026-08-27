//! Instances: a named, editable configuration that pins one downloaded game
//! version and the options to start it with.
//!
//! Worlds are deliberately **not** part of an instance. Every instance reads
//! and writes the one shared `saves/` folder (see `paths.rs`), so a world
//! follows you between versions the way Minecraft's do rather than being
//! trapped in whichever instance created it. What makes that safe is the
//! game's save-format versioning: a world records the format it was written
//! with, gets migrated forward on load, and an older build refuses to open a
//! world a newer one wrote instead of silently mangling it (see
//! `craftmjne`'s `save.rs`).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// One launch configuration. Everything except `version` is optional
/// polish - a brand new instance with just a name and a version is
/// completely usable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub name: String,
    /// The release version this instance runs, matching a directory under
    /// `Paths::versions_dir`. May legitimately name a version that isn't
    /// downloaded (yet, or any more) - `Library::is_installed` is what
    /// decides whether it can actually be launched right now.
    pub version: String,
    /// Overrides the game's own default render distance when set.
    #[serde(default)]
    pub render_distance: Option<i32>,
    /// Fixed world seed for new worlds, if this instance wants one.
    #[serde(default)]
    pub seed: Option<u32>,
    /// Anything else to pass through on the command line, split on
    /// whitespace. An escape hatch for flags the launcher doesn't model.
    #[serde(default)]
    pub extra_args: String,
}

impl Instance {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            render_distance: None,
            seed: None,
            extra_args: String::new(),
        }
    }

    /// The command-line arguments this instance starts the game with.
    pub fn launch_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(rd) = self.render_distance {
            args.push("--render-distance".to_string());
            args.push(rd.to_string());
        }
        if let Some(seed) = self.seed {
            args.push("--seed".to_string());
            args.push(seed.to_string());
        }
        args.extend(self.extra_args.split_whitespace().map(str::to_string));
        args
    }
}

/// The whole instance list, as stored in `instances.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Instances {
    #[serde(default)]
    pub items: Vec<Instance>,
}

impl Instances {
    /// Reads `instances.json`, falling back to an empty list for a missing
    /// *or unreadable* file. Losing the instance list is annoying, but it's
    /// pure configuration that takes seconds to recreate - refusing to start
    /// the launcher over it would be worse, and it's the same
    /// never-crash-on-bad-data stance the game takes with its own saves.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string());
        std::fs::write(path, text)
    }

    /// Adds an instance under a name that isn't already taken, returning the
    /// name actually used. Duplicate names aren't an error worth refusing an
    /// action over - they're just confusing in a list - so this does what a
    /// file manager does and appends a counter.
    pub fn add(&mut self, mut instance: Instance) -> String {
        if self.items.iter().any(|i| i.name == instance.name) {
            let base = instance.name.clone();
            let mut n = 2;
            while self.items.iter().any(|i| i.name == format!("{base} ({n})")) {
                n += 1;
            }
            instance.name = format!("{base} ({n})");
        }
        let name = instance.name.clone();
        self.items.push(instance);
        name
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_instance_launches_with_no_arguments_at_all() {
        assert!(Instance::new("Default", "1.0.0").launch_args().is_empty());
    }

    #[test]
    fn set_options_become_command_line_flags() {
        let mut instance = Instance::new("Tuned", "1.0.0");
        instance.render_distance = Some(12);
        instance.seed = Some(42);
        assert_eq!(
            instance.launch_args(),
            vec!["--render-distance", "12", "--seed", "42"]
        );
    }

    #[test]
    fn extra_args_are_split_on_whitespace_and_appended() {
        let mut instance = Instance::new("Raw", "1.0.0");
        instance.extra_args = "  --some-future-flag   --version ".to_string();
        assert_eq!(instance.launch_args(), vec!["--some-future-flag", "--version"]);
    }

    #[test]
    fn adding_a_duplicate_name_gets_a_counter_instead_of_being_refused() {
        let mut instances = Instances::default();
        assert_eq!(instances.add(Instance::new("Main", "1.0.0")), "Main");
        assert_eq!(instances.add(Instance::new("Main", "1.0.0")), "Main (2)");
        assert_eq!(instances.add(Instance::new("Main", "1.0.0")), "Main (3)");
        assert_eq!(instances.items.len(), 3);
    }

    #[test]
    fn instances_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!("craftmjne-launcher-test-{}", std::process::id()));
        let path = dir.join("instances.json");
        let _ = std::fs::remove_dir_all(&dir);

        let mut instances = Instances::default();
        let mut tuned = Instance::new("Tuned", "1.2.2");
        tuned.render_distance = Some(6);
        instances.add(tuned);
        instances.save(&path).unwrap();

        let loaded = Instances::load(&path);
        assert_eq!(loaded.items, instances.items);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_or_corrupt_instances_file_loads_as_empty_instead_of_failing() {
        assert!(Instances::load(Path::new("/nonexistent/instances.json")).items.is_empty());

        let dir = std::env::temp_dir().join(format!("craftmjne-launcher-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("instances.json");
        std::fs::write(&path, "{ not json at all").unwrap();
        assert!(Instances::load(&path).items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_instances_file_without_the_optional_fields_still_loads() {
        // Everything but name/version is `#[serde(default)]`, so a file
        // written by an earlier launcher keeps working.
        let json = r#"{"items":[{"name":"Old","version":"1.0.0"}]}"#;
        let instances: Instances = serde_json::from_str(json).unwrap();
        assert_eq!(instances.items[0].name, "Old");
        assert_eq!(instances.items[0].render_distance, None);
        assert!(instances.items[0].extra_args.is_empty());
    }

    #[test]
    fn removing_an_out_of_range_index_is_a_no_op_rather_than_a_panic() {
        let mut instances = Instances::default();
        instances.add(Instance::new("Only", "1.0.0"));
        instances.remove(9);
        assert_eq!(instances.items.len(), 1);
        instances.remove(0);
        assert!(instances.items.is_empty());
    }
}
