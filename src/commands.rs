//! Chat command dispatcher. `/mode <survival|creative|s|c|1|2>` is the first
//! command; add more by extending `execute`'s match.
//!
//! Successfully invoking *any* recognized command permanently marks the
//! active world's save with `cheats: true` (`save::WorldMeta::cheats`) - the
//! same one-way flag Minecraft uses to disqualify a world from achievements
//! once commands have been used in it. A completely unrecognized command
//! name (a typo, not a real command) does not trip it.

use crate::save::{GameMode, SaveStore};
use crate::state::ActiveWorld;

pub enum CommandOutcome {
    /// Recognized and executed.
    Ok(String),
    /// Recognized command, invalid/missing arguments.
    Usage(String),
    /// Not a recognized command name.
    Unknown(String),
}

impl CommandOutcome {
    pub fn message(self) -> String {
        match self {
            CommandOutcome::Ok(m) | CommandOutcome::Usage(m) | CommandOutcome::Unknown(m) => m,
        }
    }

    fn counts_as_command_use(&self) -> bool {
        !matches!(self, CommandOutcome::Unknown(_))
    }
}

fn parse_mode_arg(arg: &str) -> Option<GameMode> {
    match arg.to_ascii_lowercase().as_str() {
        "survival" | "s" | "1" => Some(GameMode::Survival),
        "creative" | "c" | "2" => Some(GameMode::Creative),
        _ => None,
    }
}

fn mode_label(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Survival",
        GameMode::Creative => "Creative",
    }
}

/// Executes a `/`-prefixed chat message, `line` being the text with the
/// leading slash already stripped (e.g. `"mode creative"`). Mutates the live
/// `GameMode` resource so the effect is immediate, and persists both the new
/// mode and the cheats flag to the active world's `meta.json`.
pub fn execute(line: &str, mode: &mut GameMode, active: &mut ActiveWorld, store: &SaveStore) -> CommandOutcome {
    let mut parts = line.split_whitespace();
    let Some(name) = parts.next() else {
        return CommandOutcome::Unknown(String::new());
    };
    let name = name.to_ascii_lowercase();

    let outcome = match name.as_str() {
        "mode" | "gamemode" => match parts.next().and_then(parse_mode_arg) {
            Some(new_mode) => {
                *mode = new_mode;
                active.meta.mode = new_mode;
                CommandOutcome::Ok(format!("Game mode set to {}", mode_label(new_mode)))
            }
            None => CommandOutcome::Usage("Usage: /mode <survival|creative|s|c|1|2>".to_string()),
        },
        _ => CommandOutcome::Unknown(format!("Unknown command: /{name}")),
    };

    if outcome.counts_as_command_use() {
        active.meta.cheats = true;
        let _ = store.save_meta(&active.slug, &active.meta);
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

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
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn temp_store() -> TempStore {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("craftmjne-cmd-test-{}-{n}", std::process::id()));
        TempStore { store: SaveStore::at(root.clone()), root }
    }

    fn active_world(store: &SaveStore) -> ActiveWorld {
        let (slug, meta) = store.create_world("Cmd Test", 1, GameMode::Survival).unwrap();
        ActiveWorld { slug, meta }
    }

    #[test]
    fn mode_command_accepts_all_alias_forms() {
        for (arg, expected) in [
            ("survival", GameMode::Survival),
            ("Creative", GameMode::Creative),
            ("s", GameMode::Survival),
            ("c", GameMode::Creative),
            ("1", GameMode::Survival),
            ("2", GameMode::Creative),
        ] {
            let store = temp_store();
            let mut active = active_world(&store);
            let mut mode = GameMode::Survival;
            let outcome = execute(&format!("mode {arg}"), &mut mode, &mut active, &store);
            assert!(matches!(outcome, CommandOutcome::Ok(_)));
            assert_eq!(mode, expected, "arg {arg}");
            assert_eq!(active.meta.mode, expected, "arg {arg}");
        }
    }

    #[test]
    fn mode_command_persists_and_applies_immediately() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        execute("mode creative", &mut mode, &mut active, &store);
        assert_eq!(mode, GameMode::Creative);
        assert_eq!(store.load_meta(&active.slug).unwrap().mode, GameMode::Creative);
    }

    #[test]
    fn first_recognized_command_sets_cheats_permanently() {
        let store = temp_store();
        let mut active = active_world(&store);
        assert!(!active.meta.cheats);
        let mut mode = GameMode::Survival;

        execute("mode creative", &mut mode, &mut active, &store);
        assert!(active.meta.cheats);
        assert!(store.load_meta(&active.slug).unwrap().cheats);

        // Switching back to survival doesn't un-set it.
        execute("mode survival", &mut mode, &mut active, &store);
        assert!(active.meta.cheats);
    }

    #[test]
    fn bad_mode_argument_is_a_usage_error_but_still_counts_as_a_command() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = execute("mode not-a-mode", &mut mode, &mut active, &store);
        assert!(matches!(outcome, CommandOutcome::Usage(_)));
        assert_eq!(mode, GameMode::Survival); // unchanged
        assert!(active.meta.cheats); // but the attempt still counts
    }

    #[test]
    fn unknown_command_does_not_set_cheats() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = execute("teleport 0 0 0", &mut mode, &mut active, &store);
        assert!(matches!(outcome, CommandOutcome::Unknown(_)));
        assert!(!active.meta.cheats);
    }
}
