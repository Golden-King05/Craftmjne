//! In-world chat: press T to open a one-line input box (or `/`, which opens
//! it with `/` already typed - the standard "command hotkey" shortcut),
//! Enter to send (appended to a local scrollback that fades out after a few
//! seconds), Escape to cancel. Ctrl+A selects the whole input, Ctrl+C
//! copies it, Ctrl+V pastes over the selection (or appends, if nothing's
//! selected) - see `clipboard_copy`/`clipboard_paste`. There's no
//! multiplayer yet, but `/`-prefixed messages are routed to
//! `commands::execute` (see that module for the dispatcher and the list of
//! commands).
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

use crate::commands;
use crate::inventory::InventoryState;
use crate::save::{GameMode, SaveStore};
use crate::state::{ActiveWorld, AppState, PauseState};
use crate::text_color::{parse_colored_segments, ColoredSegment};
use crate::texture_report::TextureReport;

const MAX_MESSAGES: usize = 50;
const VISIBLE_MESSAGES: usize = 8;
const FADE_AFTER_SECS: f32 = 8.0;
const MAX_INPUT_LEN: usize = 256;

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
}

struct ChatMessage {
    /// Parsed once at push time - see `text_color::parse_colored_segments`.
    segments: Vec<ColoredSegment>,
    age: f32,
}

/// Local scrollback. `push` is the only way content gets in - the command
/// dispatcher (`commands::execute`) calls it too, same as a plain chat
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
/// stance elsewhere (updater, save loading, texture loading).
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
                        let outcome = commands::execute(rest, &mut mode, &mut active, &store, &texture_report);
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

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatState>()
            .init_resource::<ChatLog>()
            .add_systems(OnEnter(AppState::InGame), setup_chat)
            .add_systems(OnExit(AppState::InGame), despawn_chat)
            .add_systems(
                Update,
                (toggle_chat, chat_text_input, age_messages, sync_chat_ui)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(text: &str, age: f32) -> ChatMessage {
        ChatMessage { segments: parse_colored_segments(text), age }
    }

    fn joined_text(segs: &[ColoredSegment]) -> String {
        segs.iter().map(|s| s.text.as_str()).collect()
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
