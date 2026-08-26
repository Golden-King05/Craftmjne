//! Chat command dispatcher. `/mode <survival|creative|s|c|1|2>` and
//! `/texture-report` (green/yellow/red working-texture counts, see
//! `texture_report::TextureReport`) are the built-in commands; add more by
//! extending `execute`'s match.
//!
//! Successfully invoking *any* recognized command permanently marks the
//! active world's save with `cheats: true` (`save::WorldMeta::cheats`) - the
//! same one-way flag Minecraft uses to disqualify a world from achievements
//! once commands have been used in it. A completely unrecognized command
//! name (a typo, not a real command) does not trip it.

use crate::save::{GameMode, SaveStore};
use crate::state::ActiveWorld;
use crate::text_color::colorize;
use crate::texture_report::TextureReport;

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

/// Builds `/texture-report`'s message: green/yellow/red counts up top, then
/// which specific names are yellow (broken but functioning - showing the
/// placeholder) or red (completely broken - see `TextureReport`'s doc
/// comment for what that actually means), each colored to match, using the
/// exact same `~(#hex)~` marker syntax a player could type themselves -
/// there's no separate rendering path for system-generated color.
fn texture_report_message(report: &TextureReport) -> String {
    let (working, placeholder, missing) = report.counts();
    let (placeholder_names, missing_names) = report.broken_names();

    let mut lines = vec![format!(
        "Textures: {}  {}  {}",
        colorize(&format!("{working} working"), "00ff00"),
        colorize(&format!("{placeholder} broken but functioning"), "ffff00"),
        colorize(&format!("{missing} completely broken"), "ff0000"),
    )];
    if !placeholder_names.is_empty() {
        lines.push(colorize(&format!("Broken but functioning: {}", placeholder_names.join(", ")), "ffff00"));
    }
    if !missing_names.is_empty() {
        lines.push(colorize(&format!("Completely broken: {}", missing_names.join(", ")), "ff0000"));
    }
    lines.join("\n")
}

/// Executes a `/`-prefixed chat message, `line` being the text with the
/// leading slash already stripped (e.g. `"mode creative"`). Mutates the live
/// `GameMode` resource so the effect is immediate, and persists both the new
/// mode and the cheats flag to the active world's `meta.json`.
pub fn execute(
    line: &str,
    mode: &mut GameMode,
    active: &mut ActiveWorld,
    store: &SaveStore,
    texture_report: &TextureReport,
) -> CommandOutcome {
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
        "texture-report" | "texturereport" => CommandOutcome::Ok(texture_report_message(texture_report)),
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

    fn no_report() -> TextureReport {
        TextureReport::default()
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
            let outcome = execute(&format!("mode {arg}"), &mut mode, &mut active, &store, &no_report());
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
        execute("mode creative", &mut mode, &mut active, &store, &no_report());
        assert_eq!(mode, GameMode::Creative);
        assert_eq!(store.load_meta(&active.slug).unwrap().mode, GameMode::Creative);
    }

    #[test]
    fn first_recognized_command_sets_cheats_permanently() {
        let store = temp_store();
        let mut active = active_world(&store);
        assert!(!active.meta.cheats);
        let mut mode = GameMode::Survival;

        execute("mode creative", &mut mode, &mut active, &store, &no_report());
        assert!(active.meta.cheats);
        assert!(store.load_meta(&active.slug).unwrap().cheats);

        // Switching back to survival doesn't un-set it.
        execute("mode survival", &mut mode, &mut active, &store, &no_report());
        assert!(active.meta.cheats);
    }

    #[test]
    fn bad_mode_argument_is_a_usage_error_but_still_counts_as_a_command() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = execute("mode not-a-mode", &mut mode, &mut active, &store, &no_report());
        assert!(matches!(outcome, CommandOutcome::Usage(_)));
        assert_eq!(mode, GameMode::Survival); // unchanged
        assert!(active.meta.cheats); // but the attempt still counts
    }

    #[test]
    fn unknown_command_does_not_set_cheats() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = execute("teleport 0 0 0", &mut mode, &mut active, &store, &no_report());
        assert!(matches!(outcome, CommandOutcome::Unknown(_)));
        assert!(!active.meta.cheats);
    }

    #[test]
    fn texture_report_prints_green_yellow_red_counts_and_names() {
        let mut report = TextureReport::default();
        report.extend([
            ("stone".to_string(), crate::atlas::TextureStatus::Working),
            ("ruby".to_string(), crate::atlas::TextureStatus::Placeholder),
        ]);
        report.set_missing(vec!["ghost".to_string()]);

        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = execute("texture-report", &mut mode, &mut active, &store, &report);
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok") };

        assert!(message.contains("1 working"));
        assert!(message.contains("1 broken but functioning"));
        assert!(message.contains("1 completely broken"));
        assert!(message.contains("ruby"));
        assert!(message.contains("ghost"));
        // The counts and the detail lines are wrapped in the shared color
        // marker syntax, not a bespoke format.
        assert!(message.contains("~(#00ff00)~"));
        assert!(message.contains("~(#ffff00)~"));
        assert!(message.contains("~(#ff0000)~"));
    }

    #[test]
    fn texture_report_omits_detail_lines_when_nothing_is_broken() {
        let mut report = TextureReport::default();
        report.extend([("stone".to_string(), crate::atlas::TextureStatus::Working)]);

        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = execute("texture-report", &mut mode, &mut active, &store, &report);
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok") };

        assert_eq!(message.lines().count(), 1, "no broken/missing means no detail lines: {message:?}");
    }

    #[test]
    fn texture_report_counts_as_a_command_use() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        execute("texture-report", &mut mode, &mut active, &store, &no_report());
        assert!(active.meta.cheats);
    }
}
