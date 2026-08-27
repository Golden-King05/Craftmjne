//! Chat command dispatcher and registry.
//!
//! Commands live in [`CommandRegistry`], the same "central extension point"
//! shape `blocks.rs`'s `BlockRegistry` already established: `with_defaults`
//! registers the built-ins below (`/mode` and `/texture-report`), and a
//! plugin can `.register()` more of its own before the game starts, the same
//! way a mod adds a block via `BlockRegistry::register`. There is no
//! "built-in vs modded" distinction once startup finishes - both go through
//! the exact same [`CommandSpec`]/[`CommandRegistry::execute`] path, and
//! `chat.rs`'s autocomplete dropdown is driven by querying this same
//! registry (`CommandRegistry::suggestions`), not a separate hardcoded list
//! that could drift from what `execute` actually understands.
//!
//! Successfully invoking *any* recognized command permanently marks the
//! active world's save with `cheats: true` (`save::WorldMeta::cheats`) - the
//! same one-way flag Minecraft uses to disqualify a world from achievements
//! once commands have been used in it. A completely unrecognized command
//! name (a typo, not a real command) does not trip it. This is enforced once
//! in `CommandRegistry::execute`, not per-handler, so a mod's command gets
//! it for free without having to know the flag exists.

use bevy::prelude::Resource;

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

/// What a command handler needs to do its job. Bundled into one struct
/// rather than one parameter per resource, so adding a resource a *future*
/// command needs doesn't change every existing handler's signature -
/// `chat.rs` builds one of these from its own system params each time a
/// command line is submitted (`Res`/`ResMut` deref-coerce straight into the
/// `&`/`&mut` fields), and a mod's handler receives it exactly the same way.
pub struct CommandContext<'a> {
    pub mode: &'a mut GameMode,
    pub active: &'a mut ActiveWorld,
    pub store: &'a SaveStore,
    pub texture_report: &'a TextureReport,
}

/// One registered command: the metadata that drives both dispatch and the
/// chat autocomplete dropdown, plus the handler that actually runs it.
///
/// `handler` takes whitespace-split `args` (e.g. `/mode creative` hands it
/// `["creative"]`) - never the raw remainder string, so a handler never has
/// to re-implement its own splitting/trimming.
pub struct CommandSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub usage: String,
    pub description: String,
    handler: Box<dyn Fn(&[&str], &mut CommandContext) -> CommandOutcome + Send + Sync>,
}

impl CommandSpec {
    pub fn new(
        name: impl Into<String>,
        usage: impl Into<String>,
        description: impl Into<String>,
        handler: impl Fn(&[&str], &mut CommandContext) -> CommandOutcome + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            usage: usage.into(),
            description: description.into(),
            handler: Box::new(handler),
        }
    }

    /// Builder-style: `/mode`'s registration reads
    /// `CommandSpec::new("mode", ...).alias("gamemode")`.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Every name this command answers to, primary first - what both
    /// dispatch (`matches`) and autocomplete (`CommandRegistry::suggestions`)
    /// iterate over, so a command with an alias shows up, and is invocable,
    /// under either spelling with zero special-casing at either call site.
    fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.name.as_str()).chain(self.aliases.iter().map(String::as_str))
    }

    fn matches(&self, typed: &str) -> bool {
        self.names().any(|n| n.eq_ignore_ascii_case(typed))
    }
}

/// One command name/alias offered by the chat autocomplete dropdown,
/// already filtered to a typed prefix and sorted alphabetically by
/// [`CommandRegistry::suggestions`].
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSuggestion {
    /// The exact text that completes the input - may be an alias, not
    /// necessarily the command's primary `name`.
    pub text: String,
    pub usage: String,
    pub description: String,
}

/// Every `/`-command this game (or a plugin extending it) knows how to run,
/// and the single source of truth both `execute` and the chat autocomplete
/// dropdown read from. See the module docs for why this exists instead of a
/// hardcoded match.
#[derive(Resource, Default)]
pub struct CommandRegistry {
    commands: Vec<CommandSpec>,
}

impl CommandRegistry {
    /// The registry as the running game actually starts with: `/mode`
    /// (alias `/gamemode`) and `/texture-report` (alias `/texturereport`),
    /// registered as ordinary entries through the same `register` a plugin
    /// would call - nothing here is special beyond running first.
    pub fn with_defaults() -> Self {
        let mut reg = Self::default();
        reg.register(
            CommandSpec::new(
                "mode",
                "/mode <survival|creative|s|c|1|2>",
                "Change your game mode.",
                |args, ctx| match args.first().and_then(|a| parse_mode_arg(a)) {
                    Some(new_mode) => {
                        *ctx.mode = new_mode;
                        ctx.active.meta.mode = new_mode;
                        CommandOutcome::Ok(format!("Game mode set to {}", mode_label(new_mode)))
                    }
                    None => CommandOutcome::Usage("Usage: /mode <survival|creative|s|c|1|2>".to_string()),
                },
            )
            .alias("gamemode"),
        );
        reg.register(
            CommandSpec::new(
                "texture-report",
                "/texture-report",
                "Report how many block textures are working, placeholder, or missing.",
                |_args, ctx| CommandOutcome::Ok(texture_report_message(ctx.texture_report)),
            )
            .alias("texturereport"),
        );
        reg
    }

    /// Adds a command. A plugin's `build()` calls this the same way it would
    /// call `BlockRegistry::register` to add a block - there's no lock/
    /// "compiled" step like `BlockRegistry` has, since commands are invoked
    /// rarely (chat input, not a hot per-frame loop) and don't need baking
    /// into a flat lookup table for performance.
    pub fn register(&mut self, spec: CommandSpec) {
        self.commands.push(spec);
    }

    /// Runs a `/`-prefixed line (leading slash already stripped, e.g.
    /// `"mode creative"`), and applies the cheats flag centrally so a mod's
    /// command trips it exactly like a built-in one does, with no per-
    /// handler bookkeeping.
    pub fn execute(&self, line: &str, ctx: &mut CommandContext) -> CommandOutcome {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            return CommandOutcome::Unknown(String::new());
        };
        let args: Vec<&str> = parts.collect();

        let outcome = match self.commands.iter().find(|c| c.matches(name)) {
            Some(spec) => (spec.handler)(&args, ctx),
            None => CommandOutcome::Unknown(format!("Unknown command: /{name}")),
        };

        if outcome.counts_as_command_use() {
            ctx.active.meta.cheats = true;
            let _ = ctx.store.save_meta(&ctx.active.slug, &ctx.active.meta);
        }

        outcome
    }

    /// Every invocable name (primary + aliases, across every registered
    /// command) whose text starts with `prefix`, case-insensitively,
    /// alphabetically sorted - what `chat.rs`'s autocomplete dropdown shows.
    /// `prefix` is whatever's already typed after the `/`, so an empty
    /// prefix lists every command there is, A to Z.
    pub fn suggestions(&self, prefix: &str) -> Vec<CommandSuggestion> {
        let prefix_lower = prefix.to_ascii_lowercase();
        let mut out = Vec::new();
        for spec in &self.commands {
            for name in spec.names() {
                if name.to_ascii_lowercase().starts_with(&prefix_lower) {
                    out.push(CommandSuggestion {
                        text: name.to_string(),
                        usage: spec.usage.clone(),
                        description: spec.description.clone(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.text.cmp(&b.text));
        out
    }
}

pub struct CommandsPlugin;

impl bevy::prelude::Plugin for CommandsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        // Not `init_resource` - that would call `CommandRegistry::default`,
        // an *empty* registry with no `/mode` or `/texture-report` at all.
        app.insert_resource(CommandRegistry::with_defaults());
    }
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

    /// Runs `line` against the real default registry, building a
    /// `CommandContext` from the individual pieces each test already has -
    /// the same construction `chat.rs`'s system does from its own params.
    fn run(
        line: &str,
        mode: &mut GameMode,
        active: &mut ActiveWorld,
        store: &SaveStore,
        report: &TextureReport,
    ) -> CommandOutcome {
        let registry = CommandRegistry::with_defaults();
        let mut ctx = CommandContext { mode, active, store, texture_report: report };
        registry.execute(line, &mut ctx)
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
            let outcome = run(&format!("mode {arg}"), &mut mode, &mut active, &store, &no_report());
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
        run("mode creative", &mut mode, &mut active, &store, &no_report());
        assert_eq!(mode, GameMode::Creative);
        assert_eq!(store.load_meta(&active.slug).unwrap().mode, GameMode::Creative);
    }

    #[test]
    fn the_gamemode_alias_invokes_the_same_command_as_mode() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        run("gamemode creative", &mut mode, &mut active, &store, &no_report());
        assert_eq!(mode, GameMode::Creative);
    }

    #[test]
    fn first_recognized_command_sets_cheats_permanently() {
        let store = temp_store();
        let mut active = active_world(&store);
        assert!(!active.meta.cheats);
        let mut mode = GameMode::Survival;

        run("mode creative", &mut mode, &mut active, &store, &no_report());
        assert!(active.meta.cheats);
        assert!(store.load_meta(&active.slug).unwrap().cheats);

        // Switching back to survival doesn't un-set it.
        run("mode survival", &mut mode, &mut active, &store, &no_report());
        assert!(active.meta.cheats);
    }

    #[test]
    fn bad_mode_argument_is_a_usage_error_but_still_counts_as_a_command() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = run("mode not-a-mode", &mut mode, &mut active, &store, &no_report());
        assert!(matches!(outcome, CommandOutcome::Usage(_)));
        assert_eq!(mode, GameMode::Survival); // unchanged
        assert!(active.meta.cheats); // but the attempt still counts
    }

    #[test]
    fn unknown_command_does_not_set_cheats() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = run("teleport 0 0 0", &mut mode, &mut active, &store, &no_report());
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
        let outcome = run("texture-report", &mut mode, &mut active, &store, &report);
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
        let outcome = run("texture-report", &mut mode, &mut active, &store, &report);
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok") };

        assert_eq!(message.lines().count(), 1, "no broken/missing means no detail lines: {message:?}");
    }

    #[test]
    fn texture_report_counts_as_a_command_use() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        run("texture-report", &mut mode, &mut active, &store, &no_report());
        assert!(active.meta.cheats);
    }

    #[test]
    fn suggestions_lists_every_name_alphabetically_for_an_empty_prefix() {
        let registry = CommandRegistry::with_defaults();
        let names: Vec<String> = registry.suggestions("").into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["gamemode", "mode", "texture-report", "texturereport"]);
    }

    #[test]
    fn suggestions_filter_case_insensitively_by_prefix() {
        let registry = CommandRegistry::with_defaults();
        let names: Vec<String> = registry.suggestions("MO").into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["mode"]);

        let names: Vec<String> = registry.suggestions("tex").into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["texture-report", "texturereport"]);
    }

    #[test]
    fn a_registered_alias_can_actually_invoke_its_command() {
        // Not just discoverable in the dropdown - the exact string
        // `suggestions` offers has to be something `execute` really accepts,
        // or autocomplete would be filling in text that then fails.
        let registry = CommandRegistry::with_defaults();
        for suggestion in registry.suggestions("") {
            let store = temp_store();
            let mut active = active_world(&store);
            let mut mode = GameMode::Survival;
            let mut ctx = CommandContext { mode: &mut mode, active: &mut active, store: &store, texture_report: &no_report() };
            let outcome = registry.execute(&suggestion.text, &mut ctx);
            assert!(
                !matches!(outcome, CommandOutcome::Unknown(_)),
                "{:?} was suggested but not recognized",
                suggestion.text
            );
        }
    }

    #[test]
    fn a_plugin_style_registered_command_works_exactly_like_a_built_in_one() {
        // Proves the extension point actually works, not just that it
        // compiles: a command added the same way a mod would gets the
        // cheats flag, shows up in suggestions, and runs.
        let mut registry = CommandRegistry::with_defaults();
        registry.register(CommandSpec::new(
            "heal",
            "/heal",
            "Restore full health.",
            |_args, _ctx| CommandOutcome::Ok("Healed.".to_string()),
        ));

        assert!(registry.suggestions("hea").iter().any(|s| s.text == "heal"));

        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let mut ctx = CommandContext { mode: &mut mode, active: &mut active, store: &store, texture_report: &no_report() };
        let outcome = registry.execute("heal", &mut ctx);
        assert!(matches!(outcome, CommandOutcome::Ok(ref m) if m == "Healed."));
        assert!(active.meta.cheats, "a mod's command should trip cheats exactly like a built-in one");
    }
}
