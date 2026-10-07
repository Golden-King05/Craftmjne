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

use bevy::prelude::{Resource, Vec3};

use crate::biome::Biome;
use crate::config::SEA_LEVEL;
use crate::save::{GameMode, SaveStore};
use crate::state::ActiveWorld;
use crate::terrain::{Feature, TerrainGenerator};
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

/// `/locate`'s first argument - which kind of thing to search for. Mirrors
/// `Biome`/`Feature`'s own `ALL`/`name`/`parse` shape (one declarative list
/// instead of a hand-matched set of string literals), even though this one
/// lives here rather than in a worldgen module - it's a property of the
/// *command*, not of the terrain itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LocateQualifier {
    Biome,
    Feature,
    /// Not wired to anything yet - `locate_handler` always answers
    /// "aren't implemented yet" for this qualifier. Still listed in `ALL`
    /// (so it's discoverable in autocomplete and `/locate structure`
    /// gives a real answer instead of "unknown qualifier") - reserved for
    /// when this game actually generates structures to find.
    Structure,
}

impl LocateQualifier {
    const ALL: [LocateQualifier; 3] =
        [LocateQualifier::Biome, LocateQualifier::Feature, LocateQualifier::Structure];

    fn name(self) -> &'static str {
        match self {
            LocateQualifier::Biome => "biome",
            LocateQualifier::Feature => "feature",
            LocateQualifier::Structure => "structure",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.name().eq_ignore_ascii_case(s))
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `/locate`'s own argument-completion hook (`CommandSpec::arg_candidates`):
/// qualifiers at the first position, then whatever names that specific
/// qualifier accepts at the second - reads `Biome::ALL`/`Feature::ALL`
/// directly, so a third biome or feature shows up here with zero changes.
fn locate_arg_candidates(prior: &[&str]) -> Vec<ArgCandidate> {
    match prior {
        [] => LocateQualifier::ALL
            .iter()
            .map(|q| ArgCandidate::new(q.name(), format!("Locate the nearest {}", q.name())))
            .collect(),
        [qualifier] => match LocateQualifier::parse(qualifier) {
            Some(LocateQualifier::Biome) => Biome::ALL
                .iter()
                .map(|b| ArgCandidate::new(b.name(), format!("Nearest {} biome", capitalize(b.name()))))
                .collect(),
            Some(LocateQualifier::Feature) => Feature::ALL
                .iter()
                .map(|f| ArgCandidate::new(f.name(), format!("Nearest {}", f.name())))
                .collect(),
            // `Structure` takes no second argument yet, and an
            // unrecognized qualifier has nothing to suggest either.
            Some(LocateQualifier::Structure) | None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Formats a `/locate` hit as a chat message. `is_water` picks the
/// displayed Y: a found ocean/river column's *ground* is typically well
/// underwater and not a useful place to stand, so those report the water
/// surface (`SEA_LEVEL`) instead of `TerrainGenerator::effective_height`'s
/// literal (submerged) ground height; every other qualifier reports real
/// ground level, one block above it so the coordinate is standable-on.
fn locate_result_message(ctx: &CommandContext, label: &str, found: (i32, i32), origin: (i32, i32), is_water: bool) -> String {
    let (x, z) = found;
    let y = if is_water { SEA_LEVEL } else { ctx.world_gen.effective_height(x, z) + 1 };
    let dx = f64::from(x - origin.0);
    let dz = f64::from(z - origin.1);
    let distance = (dx * dx + dz * dz).sqrt().round() as i64;
    format!("Nearest {label}: ({x}, {y}, {z}) - {distance} blocks away")
}

/// `/locate`'s handler: dispatches on the qualifier, then (for `biome`/
/// `feature`) parses the second argument and runs the matching search -
/// `TerrainGenerator::locate_biome`/`locate_feature` are the actual
/// searches; this just validates input and formats the result.
fn locate_handler(args: &[&str], ctx: &mut CommandContext) -> CommandOutcome {
    const USAGE: &str = "Usage: /locate <biome|feature|structure> <name>";
    let Some(qualifier) = args.first().and_then(|a| LocateQualifier::parse(a)) else {
        return CommandOutcome::Usage(USAGE.to_string());
    };

    if qualifier == LocateQualifier::Structure {
        return CommandOutcome::Usage("Structures aren't implemented yet.".to_string());
    }

    let Some(name) = args.get(1) else { return CommandOutcome::Usage(USAGE.to_string()) };
    let origin = (ctx.player_pos.x.floor() as i32, ctx.player_pos.z.floor() as i32);

    match qualifier {
        LocateQualifier::Biome => {
            let Some(biome) = Biome::parse(name) else {
                let valid: Vec<&str> = Biome::ALL.iter().map(|b| b.name()).collect();
                return CommandOutcome::Usage(format!("Unknown biome {name:?}. Try: {}", valid.join(", ")));
            };
            match ctx.world_gen.locate_biome(biome, origin.0, origin.1) {
                Some(found) => CommandOutcome::Ok(locate_result_message(
                    ctx,
                    &format!("{} biome", capitalize(biome.name())),
                    found,
                    origin,
                    false,
                )),
                None => CommandOutcome::Ok(format!("Couldn't find a nearby {} biome.", biome.name())),
            }
        }
        LocateQualifier::Feature => {
            let Some(feature) = Feature::parse(name) else {
                let valid: Vec<&str> = Feature::ALL.iter().map(|f| f.name()).collect();
                return CommandOutcome::Usage(format!("Unknown feature {name:?}. Try: {}", valid.join(", ")));
            };
            let is_water = matches!(feature, Feature::Ocean | Feature::River);
            match ctx.world_gen.locate_feature(feature, origin.0, origin.1) {
                Some(found) => {
                    CommandOutcome::Ok(locate_result_message(ctx, feature.name(), found, origin, is_water))
                }
                None => CommandOutcome::Ok(format!("Couldn't find a nearby {}.", feature.name())),
            }
        }
        LocateQualifier::Structure => unreachable!("handled above"),
    }
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
    /// The active world's generator - the one source of truth `/locate`
    /// searches against, same as worldgen itself uses.
    pub world_gen: &'a TerrainGenerator,
    /// Where to search outward from - the player's current position.
    pub player_pos: Vec3,
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
    /// Optional argument-completion hook: given the fully-typed argument
    /// tokens *before* whichever one the player is still typing (empty for
    /// the first argument), returns every value that token could complete
    /// to at that point - e.g. `/locate`'s is called with `[]` for the
    /// qualifier position and `["biome"]` for the biome-name position.
    /// `None` for a command with no completable arguments (`/mode`,
    /// `/texture-report`) - the dropdown simply stops offering anything
    /// once the command name itself is fully typed, same as before this
    /// existed.
    arg_candidates: Option<Box<dyn Fn(&[&str]) -> Vec<ArgCandidate> + Send + Sync>>,
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
            arg_candidates: None,
        }
    }

    /// Builder-style: `/mode`'s registration reads
    /// `CommandSpec::new("mode", ...).alias("gamemode")`.
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Builder-style, mirroring `alias`: opts this command into argument
    /// autocomplete - see `arg_candidates`'s own doc comment for the
    /// closure's shape.
    pub fn with_arg_candidates(
        mut self,
        candidates: impl Fn(&[&str]) -> Vec<ArgCandidate> + Send + Sync + 'static,
    ) -> Self {
        self.arg_candidates = Some(Box::new(candidates));
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

/// One value an argument could be completed to, at whatever position
/// `CommandSpec::arg_candidates` was called for - see
/// `CommandRegistry::arg_suggestions`, which turns these into the same
/// [`CommandSuggestion`] shape the top-level command-name dropdown uses.
pub struct ArgCandidate {
    pub value: String,
    pub description: String,
}

impl ArgCandidate {
    pub fn new(value: impl Into<String>, description: impl Into<String>) -> Self {
        Self { value: value.into(), description: description.into() }
    }
}

/// One full `/command arg1 arg2...` completion offered by the chat
/// autocomplete dropdown, already filtered to whatever's been typed so far
/// and sorted alphabetically - see [`CommandRegistry::suggestions`] (for
/// the command name itself) and [`CommandRegistry::arg_suggestions`] (for
/// an argument).
#[derive(Clone, Debug, PartialEq)]
pub struct CommandSuggestion {
    /// The exact text that replaces everything after the leading `/` -
    /// just a command name/alias while completing that, or the whole
    /// `name arg1 arg2` line once completing an argument of it.
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
        reg.register(
            CommandSpec::new(
                "locate",
                "/locate <biome|feature|structure> <name>",
                "Find the nearest biome, terrain feature, or (later) structure.",
                locate_handler,
            )
            .with_arg_candidates(locate_arg_candidates),
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

    /// Completions for one argument of `command_name` (exactly as typed,
    /// alias or not - echoed back verbatim in each result, never silently
    /// rewritten to the command's primary name), given the tokens already
    /// fully typed before it (`prior`) and whatever prefix of the current
    /// token is typed so far (`partial`). Empty if `command_name` isn't a
    /// real command, or is one with no `arg_candidates` hook at all - see
    /// `chat.rs`'s `command_suggestions` for where `prior`/`partial` come
    /// from a raw chat input string.
    pub fn arg_suggestions(&self, command_name: &str, prior: &[&str], partial: &str) -> Vec<CommandSuggestion> {
        let Some(spec) = self.commands.iter().find(|c| c.matches(command_name)) else {
            return Vec::new();
        };
        let Some(candidates) = &spec.arg_candidates else { return Vec::new() };

        let partial_lower = partial.to_ascii_lowercase();
        let mut out: Vec<CommandSuggestion> = candidates(prior)
            .into_iter()
            .filter(|c| c.value.to_ascii_lowercase().starts_with(&partial_lower))
            .map(|c| {
                let mut tokens: Vec<&str> = vec![command_name];
                tokens.extend_from_slice(prior);
                tokens.push(&c.value);
                CommandSuggestion {
                    text: tokens.join(" "),
                    usage: spec.usage.clone(),
                    description: c.description,
                }
            })
            .collect();
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

    /// A real generator/biome-noise pair, same as a running game builds in
    /// `world::enter_world` - `/locate`'s tests need an actual world to
    /// search, not a stub.
    fn test_world_gen() -> TerrainGenerator {
        TerrainGenerator::new(1, &crate::blocks::BlockRegistry::with_defaults())
    }

    /// Runs `line` against the real default registry, building a
    /// `CommandContext` from the individual pieces each test already has -
    /// the same construction `chat.rs`'s system does from its own params.
    #[allow(clippy::too_many_arguments)]
    fn run(
        line: &str,
        mode: &mut GameMode,
        active: &mut ActiveWorld,
        store: &SaveStore,
        report: &TextureReport,
        world_gen: &TerrainGenerator,
        player_pos: Vec3,
    ) -> CommandOutcome {
        let registry = CommandRegistry::with_defaults();
        let mut ctx =
            CommandContext { mode, active, store, texture_report: report, world_gen, player_pos };
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
            let outcome = run(&format!("mode {arg}"), &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
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
        run("mode creative", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
        assert_eq!(mode, GameMode::Creative);
        assert_eq!(store.load_meta(&active.slug).unwrap().mode, GameMode::Creative);
    }

    #[test]
    fn the_gamemode_alias_invokes_the_same_command_as_mode() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        run("gamemode creative", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
        assert_eq!(mode, GameMode::Creative);
    }

    #[test]
    fn first_recognized_command_sets_cheats_permanently() {
        let store = temp_store();
        let mut active = active_world(&store);
        assert!(!active.meta.cheats);
        let mut mode = GameMode::Survival;

        run("mode creative", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
        assert!(active.meta.cheats);
        assert!(store.load_meta(&active.slug).unwrap().cheats);

        // Switching back to survival doesn't un-set it.
        run("mode survival", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
        assert!(active.meta.cheats);
    }

    #[test]
    fn bad_mode_argument_is_a_usage_error_but_still_counts_as_a_command() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = run("mode not-a-mode", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
        assert!(matches!(outcome, CommandOutcome::Usage(_)));
        assert_eq!(mode, GameMode::Survival); // unchanged
        assert!(active.meta.cheats); // but the attempt still counts
    }

    #[test]
    fn unknown_command_does_not_set_cheats() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let outcome = run("teleport 0 0 0", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
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
        let outcome = run("texture-report", &mut mode, &mut active, &store, &report, &test_world_gen(), Vec3::ZERO);
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
        let outcome = run("texture-report", &mut mode, &mut active, &store, &report, &test_world_gen(), Vec3::ZERO);
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok") };

        assert_eq!(message.lines().count(), 1, "no broken/missing means no detail lines: {message:?}");
    }

    #[test]
    fn texture_report_counts_as_a_command_use() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        run("texture-report", &mut mode, &mut active, &store, &no_report(), &test_world_gen(), Vec3::ZERO);
        assert!(active.meta.cheats);
    }

    #[test]
    fn suggestions_lists_every_name_alphabetically_for_an_empty_prefix() {
        let registry = CommandRegistry::with_defaults();
        let names: Vec<String> = registry.suggestions("").into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["gamemode", "locate", "mode", "texture-report", "texturereport"]);
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
            let world_gen = test_world_gen();
                let mut ctx = CommandContext {
                mode: &mut mode,
                active: &mut active,
                store: &store,
                texture_report: &no_report(),
                world_gen: &world_gen,
                player_pos: Vec3::ZERO,
            };
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
        let world_gen = test_world_gen();
        let mut ctx = CommandContext {
            mode: &mut mode,
            active: &mut active,
            store: &store,
            texture_report: &no_report(),
            world_gen: &world_gen,
            player_pos: Vec3::ZERO,
        };
        let outcome = registry.execute("heal", &mut ctx);
        assert!(matches!(outcome, CommandOutcome::Ok(ref m) if m == "Healed."));
        assert!(active.meta.cheats, "a mod's command should trip cheats exactly like a built-in one");
    }

    #[test]
    fn locate_biome_finds_a_real_column_of_that_biome_and_reports_its_distance() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome =
            run("locate biome snow", &mut mode, &mut active, &store, &no_report(), &world_gen, Vec3::ZERO);
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok, got a usage/unknown result") };
        assert!(message.contains("Snow biome"), "{message:?}");
        assert!(message.contains("blocks away"), "{message:?}");
    }

    #[test]
    fn locate_biome_mountain_finds_a_high_column_on_a_range() {
        // The altitude-zone biome, not a region one - the search has to go
        // through the generator's full `biome_at`, not region noise alone.
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome =
            run("locate biome mountain", &mut mode, &mut active, &store, &no_report(), &world_gen, Vec3::ZERO);
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok, got a usage/unknown result") };
        assert!(message.contains("Mountain biome"), "{message:?}");
        // The reported coordinate really is a Mountain-biome column.
        let coords: Vec<i32> = message
            .split(|c: char| !(c.is_ascii_digit() || c == '-'))
            .filter_map(|t| t.parse().ok())
            .collect();
        assert_eq!(world_gen.biome_at(coords[0], coords[2]), Biome::Mountain, "{message:?}");
    }

    #[test]
    fn locate_feature_mountain_finds_a_real_elevated_column() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome = run(
            "locate feature mountain",
            &mut mode,
            &mut active,
            &store,
            &no_report(),
            &world_gen,
            Vec3::ZERO,
        );
        let CommandOutcome::Ok(message) = outcome else { panic!("expected Ok, got a usage/unknown result") };
        assert!(message.contains("mountain"), "{message:?}");
        assert!(message.contains("blocks away"), "{message:?}");
    }

    #[test]
    fn locate_structure_reports_it_is_not_implemented_yet() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome = run(
            "locate structure anything",
            &mut mode,
            &mut active,
            &store,
            &no_report(),
            &world_gen,
            Vec3::ZERO,
        );
        let CommandOutcome::Usage(message) = outcome else { panic!("expected a Usage result") };
        assert!(message.contains("not implemented") || message.contains("implemented yet"), "{message:?}");
    }

    #[test]
    fn locate_rejects_an_unknown_qualifier() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome =
            run("locate bogus snow", &mut mode, &mut active, &store, &no_report(), &world_gen, Vec3::ZERO);
        assert!(matches!(outcome, CommandOutcome::Usage(_)));
    }

    #[test]
    fn locate_biome_rejects_an_unknown_biome_name_and_lists_the_real_ones() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome = run(
            "locate biome desert",
            &mut mode,
            &mut active,
            &store,
            &no_report(),
            &world_gen,
            Vec3::ZERO,
        );
        let CommandOutcome::Usage(message) = outcome else { panic!("expected a Usage result") };
        assert!(message.contains("plains") && message.contains("snow"), "{message:?}");
    }

    #[test]
    fn locate_with_no_name_argument_is_a_usage_error() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        let outcome =
            run("locate biome", &mut mode, &mut active, &store, &no_report(), &world_gen, Vec3::ZERO);
        assert!(matches!(outcome, CommandOutcome::Usage(_)));
    }

    #[test]
    fn a_recognized_locate_invocation_still_counts_as_a_command_use_even_on_a_usage_error() {
        let store = temp_store();
        let mut active = active_world(&store);
        let mut mode = GameMode::Survival;
        let world_gen = test_world_gen();
        run("locate biome desert", &mut mode, &mut active, &store, &no_report(), &world_gen, Vec3::ZERO);
        assert!(active.meta.cheats);
    }
}
