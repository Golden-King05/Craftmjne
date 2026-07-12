//! HUD: crosshair, hotbar with atlas-tile icons, start hint, F3 debug panel.
//! Everything here (except the update banner, which is meaningful in the
//! menus too) is spawned `OnEnter(AppState::InGame)` and despawned on exit.

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::blocks::{BlockRegistry, BlockTables};
use crate::config::{TILE_SIZE, ATLAS_TILES};
use crate::interact::Hotbar;
use crate::player::{cursor_grabbed, Player};
use crate::render::AtlasImage;
use crate::save::GameMode;
use crate::state::AppState;
use crate::updater::UpdateState;
use crate::world::ChunkMap;

#[derive(Component)]
struct HudRoot;

#[derive(Component)]
struct HotbarRoot;

#[derive(Component)]
struct HotbarLabel;

#[derive(Resource, Default)]
struct HotbarLabelState {
    timer: f32,
}

/// How long the selected block's name stays visible above the hotbar after
/// the selection last changed (scroll, number keys, or the slot's contents
/// changing), Minecraft-style.
const HOTBAR_LABEL_DURATION: f32 = 1.6;

#[derive(Component)]
struct HintText;

#[derive(Component)]
struct UpdateBanner;

#[derive(Component)]
struct DebugText;

#[derive(Resource, Default)]
struct DebugState {
    visible: bool,
    timer: f32,
}

pub(crate) fn tile_rect(tile: u16) -> Rect {
    let x = (tile as usize % ATLAS_TILES * TILE_SIZE) as f32;
    let y = (tile as usize / ATLAS_TILES * TILE_SIZE) as f32;
    Rect::new(x, y, x + TILE_SIZE as f32, y + TILE_SIZE as f32)
}

fn setup_hud(mut commands: Commands) {
    commands
        .spawn((
            HudRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                ..default()
            },
        ))
        .with_children(|root| {
            // Crosshair: two thin bars centered on screen.
            for (w, h) in [(2.0, 16.0), (16.0, 2.0)] {
                root.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(50.0),
                        top: Val::Percent(50.0),
                        width: Val::Px(w),
                        height: Val::Px(h),
                        margin: UiRect {
                            left: Val::Px(-w / 2.0),
                            top: Val::Px(-h / 2.0),
                            ..default()
                        },
                        ..default()
                    },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
                ));
            }

            // Hotbar container (slots are (re)built by `rebuild_hotbar`).
            root.spawn((
                HotbarRoot,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Percent(50.0),
                    bottom: Val::Px(14.0),
                    margin: UiRect { left: Val::Px(-9.0 * 27.0), ..default() },
                    column_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            ));

            // Selected block's name, shown above the hotbar for a few
            // seconds whenever the selection (or its contents) changes. The
            // outer node is a full-width flex row so the label centers
            // itself regardless of how wide the name text ends up being.
            root.spawn((
                Node {
                    width: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(78.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
            ))
            .with_children(|wrap| {
                wrap.spawn((
                    HotbarLabel,
                    Text::new(""),
                    TextFont { font_size: 16.0, ..default() },
                    TextColor(Color::WHITE),
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                    Visibility::Hidden,
                ));
            });

            root.spawn((
                HintText,
                Text::new("WASD move  Space jump  F fly (creative)  T chat  E inventory  1-9 blocks  F3 debug  Esc pause"),
                TextFont { font_size: 14.0, ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(10.0),
                    left: Val::Percent(50.0),
                    margin: UiRect { left: Val::Px(-360.0), ..default() },
                    ..default()
                },
            ));

            root.spawn((
                DebugText,
                Text::new(""),
                TextFont { font_size: 13.0, ..default() },
                TextColor(Color::WHITE),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(10.0),
                    left: Val::Px(10.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                Visibility::Hidden,
            ));
        });
}

fn despawn_hud(mut commands: Commands, roots: Query<Entity, With<HudRoot>>) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// Spawned once at startup — the update-available banner is meaningful
/// whether you're in the menus or in a world, so it lives outside the HUD.
/// Anchored top-right so it never collides with the hotbar (bottom) or the
/// in-game hint text (top-center).
fn setup_update_banner(mut commands: Commands) {
    commands.spawn((
        UpdateBanner,
        Text::new(""),
        TextFont { font_size: 14.0, ..default() },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            right: Val::Px(10.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.12, 0.45, 0.2, 0.85)),
        Visibility::Hidden,
    ));
}

/// Reflects the background update check into a small bottom-of-screen
/// banner: silent while checking or up to date, a one-line note once a
/// newer version has been downloaded and is waiting for a restart.
fn update_banner(state: Res<UpdateState>, mut banners: Query<(&mut Text, &mut Visibility), With<UpdateBanner>>) {
    if !state.is_changed() {
        return;
    }
    let Ok((mut text, mut vis)) = banners.single_mut() else { return };
    match &*state {
        UpdateState::Ready { version } => {
            text.0 = format!("Craftmjne {version} downloaded - restart to update");
            *vis = Visibility::Visible;
        }
        _ => *vis = Visibility::Hidden,
    }
}

/// Rebuilds hotbar slots whenever the hotbar changes (selection or contents).
fn rebuild_hotbar(
    mut commands: Commands,
    hotbar: Res<Hotbar>,
    tables: Option<Res<BlockTables>>,
    atlas: Option<Res<AtlasImage>>,
    roots: Query<Entity, With<HotbarRoot>>,
) {
    let (Some(tables), Some(atlas)) = (tables, atlas) else { return };
    if !hotbar.is_changed() {
        return;
    }
    let Ok(root) = roots.single() else { return };
    commands.entity(root).despawn_related::<Children>();

    for (i, &stack) in hotbar.slots.iter().enumerate() {
        let selected = i == hotbar.selected;
        let tile = tables.0.tiles[stack.id as usize * 6]; // east face as the icon
        let slot = commands
            .spawn((
                Node {
                    width: Val::Px(46.0),
                    height: Val::Px(46.0),
                    border: UiRect::all(Val::Px(2.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BorderColor(if selected {
                    Color::WHITE
                } else {
                    Color::srgba(1.0, 1.0, 1.0, 0.35)
                }),
                BackgroundColor(if selected {
                    Color::srgba(1.0, 1.0, 1.0, 0.18)
                } else {
                    Color::srgba(0.0, 0.0, 0.0, 0.4)
                }),
            ))
            .with_children(|parent| {
                if !stack.is_empty() {
                    parent.spawn((
                        ImageNode {
                            image: atlas.0.clone(),
                            rect: Some(tile_rect(tile)),
                            ..default()
                        },
                        Node {
                            width: Val::Px(34.0),
                            height: Val::Px(34.0),
                            ..default()
                        },
                    ));
                    if stack.count > 1 {
                        parent.spawn((
                            Text::new(stack.count.to_string()),
                            TextFont { font_size: 12.0, ..default() },
                            TextColor(Color::WHITE),
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(3.0),
                                bottom: Val::Px(1.0),
                                ..default()
                            },
                        ));
                    }
                }
            })
            .id();
        commands.entity(root).add_child(slot);
    }
}

/// Shows the selected hotbar slot's block name for `HOTBAR_LABEL_DURATION`
/// seconds whenever the selection (scroll, number keys) or that slot's
/// contents last changed, then fades it back out.
fn hotbar_label(
    time: Res<Time>,
    hotbar: Res<Hotbar>,
    registry: Res<BlockRegistry>,
    mut state: ResMut<HotbarLabelState>,
    mut labels: Query<(&mut Text, &mut Visibility), With<HotbarLabel>>,
) {
    let Ok((mut text, mut vis)) = labels.single_mut() else { return };
    if hotbar.is_changed() {
        let stack = hotbar.slots[hotbar.selected];
        if stack.is_empty() {
            *vis = Visibility::Hidden;
            state.timer = 0.0;
        } else {
            text.0 = registry.def(stack.id).name.clone();
            *vis = Visibility::Visible;
            state.timer = HOTBAR_LABEL_DURATION;
        }
        return;
    }
    if state.timer <= 0.0 {
        return;
    }
    state.timer -= time.delta_secs();
    if state.timer <= 0.0 {
        *vis = Visibility::Hidden;
    }
}

fn hint_visibility(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut hints: Query<&mut Visibility, With<HintText>>,
) {
    let grabbed = cursor_grabbed(windows);
    for mut vis in &mut hints {
        *vis = if grabbed { Visibility::Hidden } else { Visibility::Visible };
    }
}

fn debug_panel(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<DebugState>,
    diagnostics: Res<DiagnosticsStore>,
    map: Res<ChunkMap>,
    game_mode: Res<GameMode>,
    players: Query<&Player>,
    mut texts: Query<(&mut Text, &mut Visibility), With<DebugText>>,
) {
    if keys.just_pressed(KeyCode::F3) {
        state.visible = !state.visible;
        state.timer = 0.0;
    }
    let Ok((mut text, mut vis)) = texts.single_mut() else { return };
    *vis = if state.visible { Visibility::Visible } else { Visibility::Hidden };
    if !state.visible {
        return;
    }
    state.timer -= time.delta_secs();
    if state.timer > 0.0 {
        return;
    }
    state.timer = 0.25;

    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let Ok(p) = players.single() else { return };
    let (generated, meshed) = map.stats();
    text.0 = format!(
        "fps      {fps:.0}\n\
         pos      {:.1} {:.1} {:.1}\n\
         chunk    {} {}\n\
         mode     {}\n\
         gamemode {}\n\
         chunks   {meshed} meshed / {generated} generated\n\
         jobs     gen {}  mesh {}",
        p.pos.x, p.pos.y, p.pos.z,
        (p.pos.x.floor() as i32).div_euclid(16),
        (p.pos.z.floor() as i32).div_euclid(16),
        if p.fly { "fly" } else if p.on_ground { "ground" } else { "air" },
        match *game_mode { GameMode::Survival => "survival", GameMode::Creative => "creative" },
        map.gen_in_flight, map.mesh_in_flight,
    );
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugState>()
            .init_resource::<HotbarLabelState>()
            .add_systems(Startup, setup_update_banner)
            .add_systems(OnEnter(AppState::InGame), setup_hud)
            .add_systems(OnExit(AppState::InGame), despawn_hud)
            .add_systems(
                Update,
                (rebuild_hotbar, hotbar_label, hint_visibility, debug_panel)
                    .run_if(in_state(AppState::InGame)),
            )
            .add_systems(Update, update_banner);
    }
}
