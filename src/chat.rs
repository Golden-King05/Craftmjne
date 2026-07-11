//! In-world chat: press T to open a one-line input box, Enter to send
//! (appended to a local scrollback that fades out after a few seconds),
//! Escape to cancel. There's no multiplayer yet, but `/`-prefixed messages
//! are routed to `commands::execute` (see that module for the dispatcher
//! and the list of commands).

use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};
use std::collections::VecDeque;

use crate::commands;
use crate::inventory::InventoryState;
use crate::save::{GameMode, SaveStore};
use crate::state::{ActiveWorld, AppState, PauseState};

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
}

struct ChatMessage {
    text: String,
    age: f32,
}

/// Local scrollback. `push` is the only way content gets in - a future
/// command dispatcher would call it too, same as a plain chat message.
#[derive(Resource, Default)]
pub struct ChatLog {
    messages: VecDeque<ChatMessage>,
}

impl ChatLog {
    pub fn push(&mut self, text: impl Into<String>) {
        self.messages.push_back(ChatMessage { text: text.into(), age: 0.0 });
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

/// T opens the box (unless it's already open, or the pause menu / inventory
/// screen is up) and frees the cursor so it can be clicked into; the prior
/// grab state is remembered so closing restores it exactly, whether the
/// mouse was locked or already released.
fn toggle_chat(
    keys: Res<ButtonInput<KeyCode>>,
    paused: Res<PauseState>,
    inventory: Res<InventoryState>,
    mut chat: ResMut<ChatState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if chat.open || paused.open || inventory.open || !keys.just_pressed(KeyCode::KeyT) {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    chat.was_grabbed = window.cursor_options.grab_mode != CursorGrabMode::None;
    chat.open = true;
    chat.just_opened = true;
    chat.input.clear();
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

/// Runs after `toggle_chat`. Reads raw `KeyboardInput` events (rather than
/// `ButtonInput`) so it sees the actual typed characters, same approach as
/// the create-world text fields in `menu.rs`.
#[allow(clippy::too_many_arguments)]
fn chat_text_input(
    mut events: EventReader<KeyboardInput>,
    mut chat: ResMut<ChatState>,
    mut log: ResMut<ChatLog>,
    mut mode: ResMut<GameMode>,
    mut active: ResMut<ActiveWorld>,
    store: Res<SaveStore>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !chat.open {
        events.clear();
        return;
    }
    // The same T press that opened the box this frame is still a pending
    // KeyboardInput event; swallow it so it doesn't become the first
    // character typed.
    if chat.just_opened {
        chat.just_opened = false;
        events.clear();
        return;
    }
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
                        let outcome = commands::execute(rest, &mut mode, &mut active, &store);
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
                chat.input.pop();
                continue;
            }
            _ => {}
        }
        if let Some(text) = ev.text.clone() {
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

fn sync_chat_ui(
    chat: Res<ChatState>,
    log: Res<ChatLog>,
    mut log_texts: Query<&mut Text, (With<ChatLogText>, Without<ChatInputText>)>,
    mut input_texts: Query<&mut Text, (With<ChatInputText>, Without<ChatLogText>)>,
    mut input_rows: Query<&mut Visibility, With<ChatInputRow>>,
) {
    if let Ok(mut row) = input_rows.single_mut() {
        *row = if chat.open { Visibility::Visible } else { Visibility::Hidden };
    }
    if let Ok(mut text) = input_texts.single_mut() {
        text.0 = format!("> {}_", chat.input);
    }
    let Ok(mut text) = log_texts.single_mut() else { return };
    let visible: Vec<&str> = log
        .messages
        .iter()
        .rev()
        .filter(|m| chat.open || m.age < FADE_AFTER_SECS)
        .take(VISIBLE_MESSAGES)
        .map(|m| m.text.as_str())
        .collect();
    text.0 = visible.into_iter().rev().collect::<Vec<_>>().join("\n");
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
