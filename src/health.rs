//! Survival health: hearts, breath underwater, fall damage, slow
//! regeneration, death and respawning.
//!
//! - **Health** is 0..=[`MAX_HEALTH`] hit points, drawn as ten hearts (two
//!   points each) above the hotbar.
//! - **Falls** of more than [`SAFE_FALL`] blocks hurt, one point per block
//!   beyond it. The physics measures the fall (`Player::landed_fall`); water
//!   and flying cancel one.
//! - **Breath** lasts [`MAX_AIR`] seconds with your head underwater, shown
//!   as bubbles while it isn't full. Out of breath, you take
//!   [`DROWN_DAMAGE`] a second. It refills quickly in air.
//! - **Regeneration** gives back a point every [`REGEN_SECONDS`] while you
//!   have breath, starting [`REGEN_DELAY`] after the last hit. There's no
//!   hunger yet, so this is what keeps a long session survivable.
//! - **Death** opens the death screen (`DeathState`), drops everything you
//!   carry where you fell, and "Respawn" puts you back at the world's spawn
//!   with full health.
//!
//! Creative takes no damage and never runs out of breath. Health and breath
//! are saved with the player (`save::PlayerSave`).

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::blocks::{BlockTables, ItemStack};
use crate::chat::ChatState;
use crate::interact::Hotbar;
use crate::inventory::{Inventory, InventoryState};
use crate::items::{SpawnDrop, THROWN_PICKUP_DELAY};
use crate::player::{Player, PlayerSet};
use crate::save::GameMode;
use crate::state::{AppState, DeathState, PauseState};
use crate::world::ChunkMap;

pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_AIR: f32 = 15.0;
/// A fall up to this many blocks is harmless.
pub const SAFE_FALL: f32 = 3.0;
/// Damage per second with no breath left.
pub const DROWN_DAMAGE: f32 = 2.0;
/// Seconds per point of health regenerated.
pub const REGEN_SECONDS: f32 = 4.0;
/// Regeneration waits this long after the last damage.
pub const REGEN_DELAY: f32 = 5.0;
/// Breath comes back this many times faster than it runs out.
const AIR_REFILL_RATE: f32 = 5.0;
/// Below this height you've fallen out of the world.
const VOID_Y: f32 = -32.0;
/// How long the red flash lasts after being hurt.
const HURT_FLASH: f32 = 0.35;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vitals {
    pub health: f32,
    pub air: f32,
    /// Seconds since the last damage (drives `REGEN_DELAY`).
    since_hurt: f32,
    regen_t: f32,
    drown_t: f32,
    /// Counts down from `HURT_FLASH` after damage; the HUD's red flash.
    pub hurt_flash: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self { health: MAX_HEALTH, air: MAX_AIR, since_hurt: REGEN_DELAY, regen_t: 0.0, drown_t: 0.0, hurt_flash: 0.0 }
    }
}

impl Vitals {
    /// Restores saved health and breath, clamped to what's possible.
    pub fn restored(health: f32, air: f32) -> Self {
        Self { health: health.clamp(0.0, MAX_HEALTH), air: air.clamp(0.0, MAX_AIR), ..Self::default() }
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }

    pub fn hurt(&mut self, amount: f32) {
        if amount <= 0.0 {
            return;
        }
        self.health = (self.health - amount).max(0.0);
        self.since_hurt = 0.0;
        self.regen_t = 0.0;
        self.hurt_flash = HURT_FLASH;
    }

    /// Advances breath, drowning and regeneration by `dt` seconds.
    pub fn tick(&mut self, dt: f32, head_underwater: bool) {
        self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        self.since_hurt += dt;
        if head_underwater {
            self.air = (self.air - dt).max(0.0);
        } else {
            self.air = (self.air + dt * AIR_REFILL_RATE).min(MAX_AIR);
            self.drown_t = 0.0;
        }
        if self.air <= 0.0 {
            self.drown_t += dt;
            while self.drown_t >= 1.0 {
                self.drown_t -= 1.0;
                self.hurt(DROWN_DAMAGE);
            }
        }
        if self.health < MAX_HEALTH && self.air > 0.0 && self.since_hurt >= REGEN_DELAY && !self.is_dead() {
            self.regen_t += dt;
            while self.regen_t >= REGEN_SECONDS {
                self.regen_t -= REGEN_SECONDS;
                self.health = (self.health + 1.0).min(MAX_HEALTH);
            }
        } else {
            self.regen_t = 0.0;
        }
    }
}

/// Damage from landing after falling `distance` blocks.
pub fn fall_damage(distance: f32) -> f32 {
    (distance - SAFE_FALL).round().max(0.0)
}

/// Everything a dead player was carrying, as drops scattered around where
/// they fell - each slot thrown a little way in its own direction, so the
/// pile spreads out rather than stacking in one spot.
pub fn death_drops(at: Vec3, slots: impl Iterator<Item = ItemStack>) -> Vec<SpawnDrop> {
    slots
        .filter(|s| !s.is_empty())
        .enumerate()
        .map(|(i, stack)| {
            let angle = i as f32 * 2.399_963; // the golden angle: evenly spread, never repeating
            let speed = 1.5 + (i % 3) as f32 * 0.8;
            SpawnDrop {
                pos: at + Vec3::Y * 0.8,
                stack,
                vel: Vec3::new(angle.cos() * speed, 3.0, angle.sin() * speed),
                pickup_delay: THROWN_PICKUP_DELAY,
            }
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn tick_vitals(
    time: Res<Time>,
    mode: Res<GameMode>,
    map: Res<ChunkMap>,
    tables: Option<Res<BlockTables>>,
    paused: Res<PauseState>,
    mut death: ResMut<DeathState>,
    mut chat: ResMut<ChatState>,
    mut inventory_screen: ResMut<InventoryState>,
    mut hotbar: ResMut<Hotbar>,
    mut inventory: ResMut<Inventory>,
    mut players: Query<&mut Player>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut drops: EventWriter<SpawnDrop>,
) {
    let Some(tables) = tables else { return };
    let Ok(mut player) = players.single_mut() else { return };
    if paused.open || death.open || !player.spawned {
        return;
    }
    let fall = std::mem::take(&mut player.landed_fall);
    if *mode == GameMode::Creative {
        player.vitals = Vitals { health: player.vitals.health, ..Vitals::default() };
        return;
    }
    let head = player.eye().floor().as_ivec3();
    let head_underwater = tables.0.fluid[map.get_block(head) as usize];
    player.vitals.hurt(fall_damage(fall));
    player.vitals.tick(time.delta_secs(), head_underwater);
    if player.pos.y < VOID_Y {
        player.vitals.hurt(MAX_HEALTH);
    }
    if !player.vitals.is_dead() {
        return;
    }

    death.open = true;
    chat.open = false;
    inventory_screen.open = false;
    for drop in death_drops(player.pos, hotbar.slots.iter().chain(inventory.slots.iter()).copied()) {
        drops.write(drop);
    }
    hotbar.slots.fill(ItemStack::EMPTY);
    inventory.slots.fill(ItemStack::EMPTY);
    if let Ok(mut window) = windows.single_mut() {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }
}

#[derive(Component)]
struct VitalsHud;
#[derive(Component)]
struct HurtFlashOverlay;
#[derive(Component)]
struct DeathScreen;
#[derive(Component)]
struct RespawnButton;

/// The pixel heart, 7 wide by 6 tall.
const HEART: [&str; 6] = [".XX.XX.", "XXXXXXX", "XXXXXXX", ".XXXXX.", "..XXX..", "...X..."];
const HEART_PX: f32 = 2.0;

fn spawn_vitals_hud(mut commands: Commands) {
    commands.spawn((
        VitalsHud,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            bottom: Val::Px(72.0),
            // The hotbar is 454px wide, centred; this row spans the same.
            margin: UiRect { left: Val::Px(-227.0), ..default() },
            width: Val::Px(454.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexEnd,
            ..default()
        },
    ));
    commands.spawn((
        HurtFlashOverlay,
        Node { position_type: PositionType::Absolute, width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() },
        BackgroundColor(Color::NONE),
    ));
}

fn despawn_health_ui(
    mut commands: Commands,
    roots: Query<Entity, Or<(With<VitalsHud>, With<HurtFlashOverlay>, With<DeathScreen>)>>,
) {
    for e in &roots {
        commands.entity(e).despawn();
    }
}

/// What the hearts/bubbles row shows; it's rebuilt only when this changes.
#[derive(PartialEq, Clone, Copy)]
struct HudKey {
    survival: bool,
    half_hearts: u32,
    bubbles: Option<u32>,
}

fn sync_vitals_hud(
    mut commands: Commands,
    mode: Res<GameMode>,
    players: Query<&Player>,
    roots: Query<Entity, With<VitalsHud>>,
    mut flashes: Query<&mut BackgroundColor, With<HurtFlashOverlay>>,
    mut last: Local<Option<HudKey>>,
) {
    let Ok(player) = players.single() else { return };
    let vitals = player.vitals;
    if let Ok(mut flash) = flashes.single_mut() {
        flash.0 = Color::srgba(0.8, 0.0, 0.0, 0.35 * vitals.hurt_flash / HURT_FLASH);
    }
    let key = HudKey {
        survival: *mode == GameMode::Survival,
        half_hearts: vitals.health.ceil().clamp(0.0, MAX_HEALTH) as u32,
        bubbles: (vitals.air < MAX_AIR).then(|| (vitals.air / MAX_AIR * 10.0).ceil() as u32),
    };
    let Ok(root) = roots.single() else { return };
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    commands.entity(root).despawn_related::<Children>();
    if !key.survival {
        return;
    }
    commands.entity(root).with_children(|row| {
        row.spawn(Node { column_gap: Val::Px(3.0), ..default() }).with_children(|hearts| {
            for i in 0..(MAX_HEALTH as u32 / 2) {
                let filled_cols = match key.half_hearts.saturating_sub(i * 2) {
                    0 => 0,
                    1 => 4,
                    _ => 7,
                };
                spawn_heart(hearts, filled_cols);
            }
        });
        row.spawn(Node { column_gap: Val::Px(3.0), ..default() }).with_children(|bubbles| {
            for i in 0..10 {
                let shown = key.bubbles.is_some_and(|n| i >= 10 - n);
                bubbles.spawn((
                    Node {
                        width: Val::Px(12.0),
                        height: Val::Px(12.0),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderRadius::MAX,
                    BorderColor(if shown { Color::srgba(0.85, 0.95, 1.0, 0.95) } else { Color::NONE }),
                    BackgroundColor(if shown { Color::srgba(0.25, 0.55, 0.95, 0.9) } else { Color::NONE }),
                ));
            }
        });
    });
}

/// One heart, red in its first `filled_cols` pixel columns (7 is a full
/// heart, 4 a half) and dark in the rest.
fn spawn_heart(parent: &mut ChildSpawnerCommands, filled_cols: usize) {
    parent
        .spawn(Node { flex_direction: FlexDirection::Column, ..default() })
        .with_children(|heart| {
            for row in HEART {
                heart.spawn(Node::default()).with_children(|line| {
                    for (col, c) in row.chars().enumerate() {
                        let color = match c {
                            'X' if col < filled_cols => Color::srgb(0.86, 0.1, 0.12),
                            'X' => Color::srgba(0.12, 0.04, 0.04, 0.75),
                            _ => Color::NONE,
                        };
                        line.spawn((
                            Node { width: Val::Px(HEART_PX), height: Val::Px(HEART_PX), ..default() },
                            BackgroundColor(color),
                        ));
                    }
                });
            }
        });
}

fn sync_death_screen(mut commands: Commands, death: Res<DeathState>, screens: Query<Entity, With<DeathScreen>>) {
    if !death.is_changed() {
        return;
    }
    for e in &screens {
        commands.entity(e).despawn();
    }
    if !death.open {
        return;
    }
    commands
        .spawn((
            DeathScreen,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(24.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.45, 0.0, 0.0, 0.45)),
        ))
        .with_children(|screen| {
            screen.spawn((Text::new("You died!"), TextFont { font_size: 48.0, ..default() }, TextColor(Color::WHITE)));
            screen.spawn((
                Text::new("Everything you were carrying was dropped where you fell."),
                TextFont { font_size: 16.0, ..default() },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
            ));
            screen
                .spawn((
                    Button,
                    RespawnButton,
                    Node { padding: UiRect::axes(Val::Px(28.0), Val::Px(10.0)), ..default() },
                    BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
                ))
                .with_children(|b| {
                    b.spawn((Text::new("Respawn"), TextFont { font_size: 20.0, ..default() }, TextColor(Color::WHITE)));
                });
        });
}

/// "Respawn": a fresh player at the world's spawn (found the same way as on
/// a brand-new world, by `Player::try_spawn`), facing the same way.
fn respawn(
    mut death: ResMut<DeathState>,
    mut players: Query<&mut Player>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<RespawnButton>)>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    for (interaction, mut bg) in &mut buttons {
        bg.0 = match interaction {
            Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.35),
            Interaction::Hovered => Color::srgba(1.0, 1.0, 1.0, 0.25),
            Interaction::None => Color::srgba(1.0, 1.0, 1.0, 0.15),
        };
        if *interaction != Interaction::Pressed {
            continue;
        }
        if let Ok(mut player) = players.single_mut() {
            let (yaw, pitch) = (player.yaw, player.pitch);
            *player = Player::default();
            player.yaw = yaw;
            player.pitch = pitch;
        }
        death.open = false;
        if let Ok(mut window) = windows.single_mut() {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
        }
    }
}

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<SpawnDrop>()
            .init_resource::<DeathState>()
            .add_systems(OnEnter(AppState::InGame), spawn_vitals_hud)
            .add_systems(OnExit(AppState::InGame), despawn_health_ui)
            .add_systems(
                Update,
                (tick_vitals, sync_vitals_hud, sync_death_screen, respawn)
                    .chain()
                    .after(PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_hurt_one_point_per_block_past_three() {
        assert_eq!(fall_damage(0.0), 0.0);
        assert_eq!(fall_damage(1.25), 0.0, "a jump");
        assert_eq!(fall_damage(3.0), 0.0);
        assert_eq!(fall_damage(4.0), 1.0);
        assert_eq!(fall_damage(13.0), 10.0);
        assert!(fall_damage(23.0) >= MAX_HEALTH, "a 23-block fall is fatal from full health");
    }

    #[test]
    fn holding_your_breath_lasts_then_drowning_hurts() {
        let mut v = Vitals::default();
        let dt = 0.05;
        let mut t = 0.0;
        while t < MAX_AIR - 0.1 {
            v.tick(dt, true);
            t += dt;
        }
        assert_eq!(v.health, MAX_HEALTH, "hurt before the breath ran out");
        assert!(v.air > 0.0);
        for _ in 0..(3.0 / dt) as usize {
            v.tick(dt, true);
        }
        assert_eq!(v.air, 0.0);
        assert!(
            (MAX_HEALTH - 2.0 * DROWN_DAMAGE..MAX_HEALTH).contains(&v.health),
            "after ~3s without breath, health is {}",
            v.health
        );
        // Surfacing refills breath well before a fresh dive would run it out.
        for _ in 0..(MAX_AIR / AIR_REFILL_RATE / dt) as usize + 2 {
            v.tick(dt, false);
        }
        assert_eq!(v.air, MAX_AIR);
    }

    #[test]
    fn health_regenerates_slowly_and_only_after_a_pause() {
        let mut v = Vitals::default();
        v.hurt(6.0);
        assert_eq!(v.health, 14.0);
        let dt = 0.1;
        for _ in 0..((REGEN_DELAY - 0.5) / dt) as usize {
            v.tick(dt, false);
        }
        assert_eq!(v.health, 14.0, "regenerated straight after being hurt");
        for _ in 0..((0.5 + REGEN_SECONDS * 3.0 + 0.2) / dt) as usize {
            v.tick(dt, false);
        }
        assert_eq!(v.health, 17.0, "one point per {REGEN_SECONDS}s");
        for _ in 0..(REGEN_SECONDS * 10.0 / dt) as usize {
            v.tick(dt, false);
        }
        assert_eq!(v.health, MAX_HEALTH, "never past full");
    }

    #[test]
    fn the_dead_stay_dead_until_respawned() {
        let mut v = Vitals::default();
        v.hurt(100.0);
        assert!(v.is_dead());
        assert_eq!(v.health, 0.0);
        for _ in 0..1000 {
            v.tick(0.1, false);
        }
        assert!(v.is_dead(), "regenerated back from death");
    }

    #[test]
    fn restored_vitals_are_clamped() {
        assert_eq!(Vitals::restored(55.0, -3.0).health, MAX_HEALTH);
        assert_eq!(Vitals::restored(55.0, -3.0).air, 0.0);
    }

    #[test]
    fn death_drops_every_carried_stack_and_spreads_them_out() {
        let stacks = [
            ItemStack { id: 3, count: 10 },
            ItemStack::EMPTY,
            ItemStack { id: 4, count: 1 },
            ItemStack { id: 3, count: 64 },
        ];
        let drops = death_drops(Vec3::new(5.0, 20.0, 5.0), stacks.into_iter());
        assert_eq!(drops.len(), 3, "the empty slot isn't dropped");
        let total: u32 = drops.iter().map(|d| d.stack.count).sum();
        assert_eq!(total, 75, "nothing lost or duplicated");
        for (i, a) in drops.iter().enumerate() {
            for b in &drops[i + 1..] {
                let (va, vb) = (Vec2::new(a.vel.x, a.vel.z), Vec2::new(b.vel.x, b.vel.z));
                assert!(va.normalize().dot(vb.normalize()) < 0.9, "two stacks thrown the same way");
            }
        }
    }
}
