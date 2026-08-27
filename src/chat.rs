//! In-world chat: press T to open a one-line input box (or `/`, which opens
//! it with `/` already typed - the standard "command hotkey" shortcut),
//! Enter to send (appended to a local scrollback that fades out after a few
//! seconds), Escape to cancel. Ctrl+A selects the whole input, Ctrl+C
//! copies it, Ctrl+V pastes over the selection (or appends, if nothing's
//! selected) - see `clipboard_copy`/`clipboard_paste`. There's no
//! multiplayer yet, but `/`-prefixed messages are routed to
//! `CommandRegistry::execute` (see `commands.rs` for the registry and the
//! list of commands).
//!
//! While the input starts with `/` and no space has been typed yet (still
//! composing the command name, not an argument), a dropdown lists every
//! matching command name - built-in or added by a plugin, since it queries
//! `commands::CommandRegistry` directly rather than keeping its own list -
//! alphabetically, from whatever's typed onward (an empty `/` lists all of
//! them). Tab fills in the first (alphabetically nearest) match; clicking
//! any entry fills in that one. Either way it's an autofill, not a submit -
//! Enter still sends it. See `command_suggestions` and `update_chat_suggestions`.
//!
//! Any message - typed by the player or produced by a command - can embed
//! `~(#hex)~ text ~(#hex)~` runs (`text_color`'s shared marker syntax) to
//! color part of itself; `ChatLog::push` parses every message exactly once
//! at push time (`text_color::parse_colored_segments`), and `sync_chat_ui`
//! renders the parsed segments as `TextSpan` children of one `Text` root
//! entity per frame - Bevy's rich-text API for mixing colors within a
//! single text block (see `TextSpan`'s own docs: "children must be
//! `TextSpan`, not `Text`").

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use std::collections::VecDeque;

use crate::commands::{CommandContext, CommandRegistry, CommandSuggestion};
use crate::inventory::InventoryState;
use crate::save::{GameMode, SaveStore};
use crate::state::{ActiveWorld, AppState, PauseState};
use crate::text_color::{parse_colored_segments, ColoredSegment};
use crate::texture_report::TextureReport;

const MAX_MESSAGES: usize = 50;
const VISIBLE_MESSAGES: usize = 8;
const FADE_AFTER_SECS: f32 = 8.0;
const MAX_INPUT_LEN: usize = 256;
/// How many command matches the autocomplete dropdown shows at once. A
/// handful of built-ins plus whatever mods add could get long; capping it
/// keeps the dropdown from swallowing the screen, and the cap is on the
/// already-sorted-and-filtered list, so what's cut off is always the
/// alphabetically-furthest matches, never an arbitrary subset.
const MAX_VISIBLE_SUGGESTIONS: usize = 8;

/// Whether the chat box is open and its current text.
#[derive(Resource, Default)]
pub struct ChatState {
    pub open: bool,
    pub input: String,
    just_opened: bool,
    was_grabbed: bool,
    /// Set by Ctrl+A ("select all") - there's no real cursor/selection
    /// range in this single-line input, so this is the whole-input-or-
    /// nothing stand-in for one: while set, the next keystroke either
    /// replaces the entire input (typing, or Ctrl+V) or clears it
    /// (Backspace), and Ctrl+C copies the whole input instead of being a
    /// no-op. Cleared by any edit, and by opening the box fresh.
    selected: bool,
    /// Command-name matches for whatever's currently typed after a leading
    /// `/`, alphabetically sorted - see `command_suggestions`. Recomputed
    /// every frame by `update_chat_suggestions`, one step *after*
    /// `chat_text_input` in the system chain, so a same-frame Tab press
    /// still reads the previous frame's list - which is exactly what's on
    /// screen at the moment the key is pressed.
    pub suggestions: Vec<CommandSuggestion>,
}

struct ChatMessage {
    /// Parsed once at push time - see `text_color::parse_colored_segments`.
    segments: Vec<ColoredSegment>,
    age: f32,
}

/// Local scrollback. `push` is the only way content gets in - the command
/// dispatcher (`CommandRegistry::execute`) calls it too, same as a plain chat
/// message, which is exactly what lets `/texture-report`'s colored output
/// reuse this same parsing/rendering path instead of a bespoke one.
#[derive(Resource, Default)]
pub struct ChatLog {
    messages: VecDeque<ChatMessage>,
}

impl ChatLog {
    pub fn push(&mut self, text: impl Into<String>) {
        let segments = parse_colored_segments(&text.into());
        self.messages.push_back(ChatMessage { segments, age: 0.0 });
        while self.messages.len() > MAX_MESSAGES {
            self.messages.pop_front();
        }
    }
}

#[derive(Component)]
struct ChatRoot;
#[derive(Component)]
struct ChatLogText;
#[derive(Component)]
struct ChatInputRow;
#[derive(Component)]
struct ChatInputText;
/// Container for the autocomplete dropdown - despawned/respawned each frame
/// its contents change, same "spawn-on-change" pattern as `ChatLogText`
/// itself and `ui::rebuild_hotbar`.
#[derive(Component)]
struct SuggestionRoot;
/// Marks one dropdown entry as clickable, carrying the exact text clicking
/// it fills in (`ChatState::suggestions`'s `text`, which may be an alias -
/// not necessarily the command's primary name).
#[derive(Component)]
struct SuggestionButton(String);

fn setup_chat(mut commands: Commands) {
    commands
        .spawn((
            ChatRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                bottom: Val::Px(70.0),
                width: Val::Px(480.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                ChatLogText,
                Text::new(""),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::WHITE),
            ));
            // Sits between the log and the input row, so the dropdown reads
            // as "here's what would follow what you're typing below it".
            // Starts (and, whenever nothing matches, stays) childless -
            // `sync_suggestions_ui` is what actually populates it.
            root.spawn((SuggestionRoot, Node { flex_direction: FlexDirection::Column, ..default() }));
            root.spawn((
                ChatInputRow,
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                Visibility::Hidden,
            ))
            .with_children(|p| {
                p.spawn((
                    ChatInputText,
                    Text::new(""),
                    TextFont { font_size: 14.0, ..default() },
                    TextColor(Color::WHITE),
                ));
            });
        });
}

fn despawn_chat(mut commands: Commands, roots: Query<Entity, With<ChatRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// T opens the box empty; `/` opens it with `/` already typed - the
/// standard "command hotkey" shortcut (Minecraft does the same) so you
/// don't have to open chat and type the slash separately. Either way this
/// requires the pause menu / inventory screen to be closed, and frees the
/// cursor so it can be clicked into; the prior grab state is remembered so
/// closing restores it exactly, whether the mouse was locked or already
/// released.
fn toggle_chat(
    keys: Res<ButtonInput<KeyCode>>,
    paused: Res<PauseState>,
    inventory: Res<InventoryState>,
    mut chat: ResMut<ChatState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if chat.open || paused.open || inventory.open {
        return;
    }
    let opens_with_slash = keys.just_pressed(KeyCode::Slash);
    if !keys.just_pressed(KeyCode::KeyT) && !opens_with_slash {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    chat.was_grabbed = window.cursor_options.grab_mode != CursorGrabMode::None;
    chat.open = true;
    chat.just_opened = true;
    chat.selected = false;
    chat.input.clear();
    if opens_with_slash {
        chat.input.push('/');
    }
    window.cursor_options.grab_mode = CursorGrabMode::None;
    window.cursor_options.visible = true;
}

fn restore_grab(chat: &ChatState, windows: &mut Query<&mut Window, With<PrimaryWindow>>) {
    if !chat.was_grabbed {
        return;
    }
    if let Ok(mut window) = windows.single_mut() {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }
}

/// Copies `text` to the OS clipboard, silently doing nothing if there's no
/// clipboard to talk to (e.g. a headless/CI environment) - matches this
/// project's general "never crash on an external environment failure"
/// stance elsewhere (save loading, texture loading).
fn clipboard_copy(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

/// Reads text from the OS clipboard, `None` if there's no clipboard to talk
/// to or it doesn't currently hold text.
fn clipboard_paste() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Runs after `toggle_chat`. Reads raw `KeyboardInput` events (rather than
/// `ButtonInput`) so it sees the actual typed characters, same approach as
/// the create-world text fields in `menu.rs`. `keys` is only consulted for
/// the Ctrl modifier (held-state, not a discrete event) that Ctrl+A/C/V ride
/// on.
#[allow(clippy::too_many_arguments)]
fn chat_text_input(
    mut events: EventReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut chat: ResMut<ChatState>,
    mut log: ResMut<ChatLog>,
    mut mode: ResMut<GameMode>,
    mut active: ResMut<ActiveWorld>,
    store: Res<SaveStore>,
    texture_report: Res<TextureReport>,
    registry: Res<CommandRegistry>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !chat.open {
        events.clear();
        return;
    }
    // The same T/slash press that opened the box this frame is still a
    // pending KeyboardInput event; swallow it so it doesn't become the
    // first character typed (the slash itself is pre-filled by toggle_chat).
    if chat.just_opened {
        chat.just_opened = false;
        events.clear();
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    for ev in events.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match ev.key_code {
            KeyCode::Escape => {
                chat.open = false;
                chat.input.clear();
                restore_grab(&chat, &mut windows);
                return;
            }
            KeyCode::Enter => {
                let text = chat.input.trim().to_string();
                if !text.is_empty() {
                    if let Some(rest) = text.strip_prefix('/') {
                        let mut ctx = CommandContext {
                            mode: &mut mode,
                            active: &mut active,
                            store: &store,
                            texture_report: &texture_report,
                        };
                        let outcome = registry.execute(rest, &mut ctx);
                        log.push(outcome.message());
                    } else {
                        log.push(text);
                    }
                }
                chat.open = false;
                chat.input.clear();
                restore_grab(&chat, &mut windows);
                return;
            }
            // Autofills the current best (alphabetically nearest) match, the
            // same one the dropdown lists first and highlights - a discrete
            // "commit to this suggestion" action, not a submit. `chat.
            // suggestions` here is last frame's list (this frame's
            // `update_chat_suggestions` hasn't run yet), which is exactly
            // what's currently rendered on screen, so Tab always completes
            // whatever the player can actually see.
            KeyCode::Tab => {
                if let Some(best) = chat.suggestions.first() {
                    chat.input = format!("/{} ", best.text);
                    chat.selected = false;
                }
                continue;
            }
            KeyCode::Backspace => {
                if chat.selected {
                    chat.input.clear();
                    chat.selected = false;
                } else {
                    chat.input.pop();
                }
                continue;
            }
            KeyCode::KeyA if ctrl => {
                chat.selected = true;
                continue;
            }
            // Only copies when something is actually "selected" (Ctrl+A) -
            // there's no partial-selection concept to copy otherwise, same
            // as a real text field doing nothing on Ctrl+C with no
            // selection.
            KeyCode::KeyC if ctrl => {
                if chat.selected {
                    clipboard_copy(&chat.input);
                }
                continue;
            }
            KeyCode::KeyV if ctrl => {
                if let Some(pasted) = clipboard_paste() {
                    if chat.selected {
                        chat.input.clear();
                        chat.selected = false;
                    }
                    for ch in pasted.chars() {
                        if ch.is_control() {
                            continue;
                        }
                        if chat.input.len() >= MAX_INPUT_LEN {
                            break;
                        }
                        chat.input.push(ch);
                    }
                }
                continue;
            }
            _ => {}
        }
        if let Some(text) = ev.text.clone() {
            if chat.selected {
                chat.input.clear();
                chat.selected = false;
            }
            for ch in text.chars() {
                if ch.is_control() {
                    continue;
                }
                if chat.input.len() < MAX_INPUT_LEN {
                    chat.input.push(ch);
                }
            }
        }
    }
}

fn age_messages(time: Res<Time>, mut log: ResMut<ChatLog>) {
    for msg in &mut log.messages {
        msg.age += time.delta_secs();
    }
}

/// Flattens the currently-visible messages (in chronological order) into one
/// segment list, inserting a plain newline segment between messages - pure
/// so it's directly testable without spinning up UI entities.
fn visible_log_segments(chat: &ChatState, log: &ChatLog) -> Vec<ColoredSegment> {
    let visible: Vec<&ChatMessage> = log
        .messages
        .iter()
        .rev()
        .filter(|m| chat.open || m.age < FADE_AFTER_SECS)
        .take(VISIBLE_MESSAGES)
        .collect();

    let mut combined: Vec<ColoredSegment> = Vec::new();
    for (i, msg) in visible.into_iter().rev().enumerate() {
        if i > 0 {
            combined.push(ColoredSegment { text: "\n".to_string(), color: None });
        }
        combined.extend(msg.segments.iter().cloned());
    }
    if combined.is_empty() {
        combined.push(ColoredSegment { text: String::new(), color: None });
    }
    combined
}

/// Rebuilds the log's colored text every frame: span 0 lives directly on the
/// `ChatLogText` root entity (`Text`/`TextColor`), spans 1+ are respawned as
/// `TextSpan` children - Bevy's rich-text API for mixing colors within one
/// text block (see `TextSpan`'s own docs). Cheap: at most `VISIBLE_MESSAGES`
/// messages' worth of segments, so a full despawn/respawn of the child spans
/// each frame is simpler than diffing them and not worth optimizing away.
fn sync_chat_ui(
    mut commands: Commands,
    chat: Res<ChatState>,
    log: Res<ChatLog>,
    log_texts: Query<Entity, (With<ChatLogText>, Without<ChatInputText>)>,
    mut input_texts: Query<&mut Text, (With<ChatInputText>, Without<ChatLogText>)>,
    mut input_rows: Query<&mut Visibility, With<ChatInputRow>>,
) {
    if let Ok(mut row) = input_rows.single_mut() {
        *row = if chat.open { Visibility::Visible } else { Visibility::Hidden };
    }
    if let Ok(mut text) = input_texts.single_mut() {
        text.0 = format!("> {}_", chat.input);
    }
    let Ok(log_entity) = log_texts.single() else { return };

    let segments = visible_log_segments(&chat, &log);
    let (first, rest) = segments.split_first().expect("visible_log_segments always returns at least one segment");

    let mut entity = commands.entity(log_entity);
    entity.despawn_related::<Children>();
    entity.insert((Text::new(first.text.clone()), TextColor(first.color.unwrap_or(Color::WHITE))));
    entity.with_children(|parent| {
        for seg in rest {
            parent.spawn((
                TextSpan::new(seg.text.clone()),
                TextFont { font_size: 14.0, ..default() },
                TextColor(seg.color.unwrap_or(Color::WHITE)),
            ));
        }
    });
}

/// Command-name suggestions for the current chat input, or empty once the
/// input either isn't a command at all or has moved past the command-name
/// token into arguments.
///
/// Pure and independent of `ChatState.open`/UI - `open` is checked once by
/// `update_chat_suggestions` before calling this, and testing this directly
/// doesn't need a running app. `registry` isn't `ChatState`'s own data (a
/// `CommandRegistry` isn't `Clone` - it owns boxed handler closures - so it
/// can't just be stored there); it's read fresh from the `Res` each call,
/// same as `visible_log_segments` takes `log`/`chat` as plain borrows rather
/// than owning copies.
fn command_suggestions(input: &str, registry: &CommandRegistry) -> Vec<CommandSuggestion> {
    let Some(rest) = input.strip_prefix('/') else { return Vec::new() };
    // A space means the command name is finished and this is now an
    // argument - `/mode c` shouldn't still be offering to complete "mode".
    if rest.contains(char::is_whitespace) {
        return Vec::new();
    }
    registry.suggestions(rest)
}

/// Runs after `chat_text_input` so it sees this frame's edited input before
/// `sync_chat_ui`/`sync_suggestions_ui` render it - see `ChatState::
/// suggestions`'s doc comment for why `chat_text_input`'s Tab handling
/// reading last frame's list (computed here, one step later) is correct
/// rather than stale.
fn update_chat_suggestions(mut chat: ResMut<ChatState>, registry: Res<CommandRegistry>) {
    chat.suggestions = if chat.open { command_suggestions(&chat.input, &registry) } else { Vec::new() };
}

/// Applies the same "commit to a suggestion" action `chat_text_input`'s Tab
/// key does, triggered by clicking a dropdown entry instead. Reuses whatever
/// text that specific button was spawned with (`SuggestionButton`), not
/// necessarily `chat.suggestions.first()` - clicking the third entry down
/// fills in the third entry, not the top one.
fn click_suggestion(mut chat: ResMut<ChatState>, buttons: Query<(&Interaction, &SuggestionButton), Changed<Interaction>>) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            chat.input = format!("/{} ", button.0);
            chat.selected = false;
        }
    }
}

/// One dropdown row: the exact text a click on it fills in, `/name  usage`
/// as its label, and the first entry (the one Tab would pick) tinted so
/// it's visibly the default choice.
fn spawn_suggestion_entry(parent: &mut ChildSpawnerCommands, suggestion: &CommandSuggestion, is_best: bool) {
    parent
        .spawn((
            Button,
            SuggestionButton(suggestion.text.clone()),
            Node { padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)), ..default() },
            BackgroundColor(if is_best {
                Color::srgba(0.3, 0.3, 0.3, 0.85)
            } else {
                Color::srgba(0.0, 0.0, 0.0, 0.55)
            }),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(format!("/{}  {}", suggestion.text, suggestion.usage)),
                TextFont { font_size: 13.0, ..default() },
                TextColor(Color::WHITE),
            ));
        });
}

/// Rebuilds the dropdown's rows every frame - same despawn-and-respawn-the-
/// subtree shape as `sync_chat_ui`'s log text, and cheap for the same reason
/// at `MAX_VISIBLE_SUGGESTIONS`-row scale: simpler than diffing and not
/// worth optimizing away. (Deliberately not gated on `ChatState::
/// is_changed()` - that flips on every keystroke, typed text included, since
/// `chat.input` lives on the same resource, so it wouldn't actually skip the
/// common case of "typing an ordinary message" the way it might look like it
/// does.)
fn sync_suggestions_ui(mut commands: Commands, chat: Res<ChatState>, roots: Query<Entity, With<SuggestionRoot>>) {
    let Ok(root) = roots.single() else { return };
    let mut entity = commands.entity(root);
    entity.despawn_related::<Children>();
    entity.with_children(|parent| {
        for (i, suggestion) in chat.suggestions.iter().take(MAX_VISIBLE_SUGGESTIONS).enumerate() {
            spawn_suggestion_entry(parent, suggestion, i == 0);
        }
    });
}

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatState>()
            .init_resource::<ChatLog>()
            .add_systems(OnEnter(AppState::InGame), setup_chat)
            .add_systems(OnExit(AppState::InGame), despawn_chat)
            .add_systems(
                Update,
                (
                    toggle_chat,
                    chat_text_input,
                    click_suggestion,
                    update_chat_suggestions,
                    age_messages,
                    sync_chat_ui,
                    sync_suggestions_ui,
                )
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandOutcome, CommandSpec};

    fn msg(text: &str, age: f32) -> ChatMessage {
        ChatMessage { segments: parse_colored_segments(text), age }
    }

    fn joined_text(segs: &[ColoredSegment]) -> String {
        segs.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn a_bare_slash_lists_every_command_alphabetically() {
        let registry = CommandRegistry::with_defaults();
        let names: Vec<String> = command_suggestions("/", &registry).into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["gamemode", "mode", "texture-report", "texturereport"]);
    }

    #[test]
    fn typing_narrows_the_suggestions_to_a_prefix_match() {
        let registry = CommandRegistry::with_defaults();
        let names: Vec<String> = command_suggestions("/mo", &registry).into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["mode"]);
    }

    #[test]
    fn a_space_after_the_command_name_ends_suggestions_since_thats_now_an_argument() {
        let registry = CommandRegistry::with_defaults();
        // `/mode c` is composing an argument to `mode`, not still spelling
        // out the command name - offering to complete "mode" again here
        // would be actively wrong, not just unhelpful. (Against the
        // built-in commands, none of which share a name-with-space prefix,
        // `starts_with` alone would already reject these two - see the next
        // test for one that actually distinguishes the guard from that
        // coincidence.)
        assert!(command_suggestions("/mode c", &registry).is_empty());
        assert!(command_suggestions("/mode ", &registry).is_empty());
    }

    #[test]
    fn a_space_ends_suggestions_even_if_it_would_otherwise_still_prefix_match() {
        // A command name is just a String - nothing stops one containing a
        // space, however unusual that'd be. If such a name existed,
        // `"foo ".starts_with` alone could accidentally treat an in-progress
        // argument as still spelling out that command's name (`"big
        // heal"`'s own space lines up exactly with the one the player just
        // typed after "big"). The guard's job is to end suggestions the
        // instant a space appears, full stop, regardless of what any
        // registered name looks like - this is the case that actually
        // requires the guard to exist, rather than following for free from
        // `starts_with`.
        let mut registry = CommandRegistry::with_defaults();
        registry.register(CommandSpec::new("big heal", "/big heal", "deliberately unusual name", |_, _| {
            CommandOutcome::Ok(String::new())
        }));
        assert!(!registry.suggestions("big ").is_empty(), "test setup: this should still prefix-match");
        assert!(command_suggestions("/big ", &registry).is_empty());
    }

    #[test]
    fn plain_text_with_no_leading_slash_has_no_suggestions() {
        let registry = CommandRegistry::with_defaults();
        assert!(command_suggestions("hello", &registry).is_empty());
        assert!(command_suggestions("", &registry).is_empty());
    }

    #[test]
    fn suggestions_include_a_plugin_registered_command() {
        // The point of querying the registry instead of a hardcoded list:
        // a command added the same way a mod would shows up here with zero
        // changes to `command_suggestions` itself.
        let mut registry = CommandRegistry::with_defaults();
        registry.register(CommandSpec::new("heal", "/heal", "Restore full health.", |_, _| {
            CommandOutcome::Ok(String::new())
        }));
        let names: Vec<String> = command_suggestions("/he", &registry).into_iter().map(|s| s.text).collect();
        assert_eq!(names, vec!["heal"]);
    }

    #[test]
    fn visible_log_segments_joins_messages_with_a_plain_newline() {
        let chat = ChatState { open: true, ..Default::default() };
        let mut log = ChatLog::default();
        log.push("first");
        log.push("second");
        assert_eq!(joined_text(&visible_log_segments(&chat, &log)), "first\nsecond");
    }

    #[test]
    fn visible_log_segments_hides_faded_messages_when_chat_is_closed() {
        let chat = ChatState { open: false, ..Default::default() };
        let mut log = ChatLog::default();
        log.messages.push_back(msg("old", FADE_AFTER_SECS + 1.0));
        log.messages.push_back(msg("new", 0.0));
        assert_eq!(joined_text(&visible_log_segments(&chat, &log)), "new");
    }

    #[test]
    fn visible_log_segments_shows_faded_messages_when_chat_is_open() {
        let chat = ChatState { open: true, ..Default::default() };
        let mut log = ChatLog::default();
        log.messages.push_back(msg("old", FADE_AFTER_SECS + 1.0));
        assert_eq!(joined_text(&visible_log_segments(&chat, &log)), "old");
    }

    #[test]
    fn visible_log_segments_caps_at_the_most_recent_visible_messages() {
        let chat = ChatState { open: true, ..Default::default() };
        let mut log = ChatLog::default();
        for i in 0..VISIBLE_MESSAGES + 3 {
            log.push(format!("m{i}"));
        }
        let joined = joined_text(&visible_log_segments(&chat, &log));
        let lines: Vec<&str> = joined.split('\n').collect();
        assert_eq!(lines.len(), VISIBLE_MESSAGES);
        assert_eq!(lines.last(), Some(&format!("m{}", VISIBLE_MESSAGES + 2)).map(|s| s.as_str()).as_ref());
    }

    #[test]
    fn visible_log_segments_preserves_a_messages_own_colored_segments() {
        let chat = ChatState { open: true, ..Default::default() };
        let mut log = ChatLog::default();
        log.push("~(#ff0000)~red~(#ff0000)~");
        let segs = visible_log_segments(&chat, &log);
        assert!(segs.iter().any(|s| s.text == "red" && s.color.is_some()));
    }

    #[test]
    fn an_empty_log_still_produces_one_segment() {
        let chat = ChatState::default();
        let log = ChatLog::default();
        let segs = visible_log_segments(&chat, &log);
        assert_eq!(segs, vec![ColoredSegment { text: String::new(), color: None }]);
    }
}
