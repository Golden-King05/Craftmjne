//! First-person player: AABB physics against the voxel grid, swimming,
//! fly mode, and fixed-timestep integration (120 Hz substeps) so behaviour is
//! framerate-independent. The `Player` component lives on the camera entity;
//! `pos` is the feet position, the camera sits at eye height above it.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::blocks::{BlockTables, Tables};
use crate::chat::ChatState;
use crate::config::{WorldSettings, CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
use crate::inventory::InventoryState;
use crate::save::GameMode;
use crate::state::{AppState, PauseState};
use crate::world::ChunkMap;

pub const HALF_W: f32 = 0.3;
pub const HEIGHT: f32 = 1.8;
pub const EYE_HEIGHT: f32 = 1.62;
const EPS: f32 = 1e-3;

const GRAVITY: f32 = -30.0;
const JUMP_SPEED: f32 = 8.8;
const WALK_SPEED: f32 = 5.2;
const SPRINT_MULT: f32 = 1.6;
const FLY_SPEED: f32 = 16.0;
const STEP: f32 = 1.0 / 120.0;
const LOOK_SENS: f32 = 0.0022;
/// Steady upward speed applied while swimming into a climbable pool edge -
/// lets you climb out onto land by just swimming into the ledge, rather
/// than needing to hold Space and wait for buoyancy to slowly float you up
/// over it.
const CLIMB_SPEED: f32 = 3.4;
/// How many blocks of wall above the current feet position the swim-to-
/// shore assist will search for an opening - keeps it scoped to "getting
/// out of a pool" rather than a general climb-any-cliff assist.
const MAX_CLIMB_HEIGHT: i32 = 4;

#[derive(Component)]
pub struct Player {
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub fly: bool,
    pub spawned: bool,
    accumulator: f32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            pos: Vec3::new(8.5, WORLD_HEIGHT as f32 + 4.0, 8.5),
            vel: Vec3::ZERO,
            yaw: std::f32::consts::PI * 0.75,
            pitch: -0.25,
            on_ground: false,
            fly: false,
            spawned: false,
            accumulator: 0.0,
        }
    }
}

impl Player {
    pub fn look_dir(&self) -> Vec3 {
        let cp = self.pitch.cos();
        Vec3::new(-self.yaw.sin() * cp, self.pitch.sin(), -self.yaw.cos() * cp)
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::Y * EYE_HEIGHT
    }

    pub fn intersects_block(&self, b: IVec3) -> bool {
        let (bx, by, bz) = (b.x as f32, b.y as f32, b.z as f32);
        bx + 1.0 > self.pos.x - HALF_W
            && bx < self.pos.x + HALF_W
            && by + 1.0 > self.pos.y
            && by < self.pos.y + HEIGHT
            && bz + 1.0 > self.pos.z - HALF_W
            && bz < self.pos.z + HALF_W
    }

    /// Axis-separated AABB sweep: move, then clamp against overlapping voxels.
    fn move_axis(&mut self, map: &ChunkMap, tables: &Tables, axis: usize, delta: f32) {
        if delta == 0.0 {
            return;
        }
        self.pos[axis] += delta;

        let min = IVec3::new(
            (self.pos.x - HALF_W).floor() as i32,
            self.pos.y.floor() as i32,
            (self.pos.z - HALF_W).floor() as i32,
        );
        let max = IVec3::new(
            (self.pos.x + HALF_W).floor() as i32,
            (self.pos.y + HEIGHT).floor() as i32,
            (self.pos.z + HALF_W).floor() as i32,
        );

        let mut bound: Option<i32> = None;
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                for x in min.x..=max.x {
                    if !map.is_solid(tables, IVec3::new(x, y, z)) {
                        continue;
                    }
                    let v = [x, y, z][axis];
                    bound = Some(match bound {
                        Some(b) if delta > 0.0 => b.min(v),
                        Some(b) => b.max(v + 1),
                        None if delta > 0.0 => v,
                        None => v + 1,
                    });
                }
            }
        }
        let Some(bound) = bound else { return };

        if axis == 1 {
            if delta > 0.0 {
                self.pos.y = bound as f32 - HEIGHT - EPS;
            } else {
                self.pos.y = bound as f32 + EPS;
                self.on_ground = true;
            }
        } else if delta > 0.0 {
            self.pos[axis] = bound as f32 - HALF_W - EPS;
        } else {
            self.pos[axis] = bound as f32 + HALF_W + EPS;
        }
        self.vel[axis] = 0.0;
    }

    fn step(
        &mut self,
        dt: f32,
        keys: &ButtonInput<KeyCode>,
        map: &ChunkMap,
        tables: &Tables,
    ) {
        // Wish direction from WASD, rotated by yaw.
        let f = keys.pressed(KeyCode::KeyW) as i32 - keys.pressed(KeyCode::KeyS) as i32;
        let r = keys.pressed(KeyCode::KeyD) as i32 - keys.pressed(KeyCode::KeyA) as i32;
        let (sy, cy) = self.yaw.sin_cos();
        let mut wish = Vec2::new(
            -sy * f as f32 + cy * r as f32,
            -cy * f as f32 - sy * r as f32,
        );
        if wish.length_squared() > 0.0 {
            wish = wish.normalize();
        }
        let sprint = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let body = self.pos + Vec3::Y * 0.6;
        let in_water = tables.fluid[map.get_block(body.floor().as_ivec3()) as usize];

        if self.fly {
            let speed = FLY_SPEED * if sprint { 2.5 } else { 1.0 };
            self.vel.x = wish.x * speed;
            self.vel.z = wish.y * speed;
            self.vel.y = (keys.pressed(KeyCode::Space) as i32
                - keys.pressed(KeyCode::ShiftLeft) as i32) as f32
                * speed;
        } else {
            let speed = WALK_SPEED
                * if sprint { SPRINT_MULT } else { 1.0 }
                * if in_water { 0.55 } else { 1.0 };
            let control: f32 = if self.on_ground || in_water { 20.0 } else { 5.0 };
            let blend = (control * dt).min(1.0);
            self.vel.x += (wish.x * speed - self.vel.x) * blend;
            self.vel.z += (wish.y * speed - self.vel.z) * blend;

            if in_water {
                self.vel.y += GRAVITY * 0.3 * dt;
                if keys.pressed(KeyCode::Space) {
                    self.vel.y += (4.5 - self.vel.y) * (12.0 * dt).min(1.0);
                }
                self.vel.y = self.vel.y.max(-4.0);
            } else {
                self.vel.y = (self.vel.y + GRAVITY * dt).max(-50.0);
                if keys.pressed(KeyCode::Space) && self.on_ground {
                    self.vel.y = JUMP_SPEED;
                }
            }
        }

        if !self.fly && wish.length_squared() > 0.0 {
            self.assist_climb_out(map, tables, wish);
        }

        self.on_ground = false;
        self.move_axis(map, tables, 0, self.vel.x * dt);
        self.move_axis(map, tables, 2, self.vel.z * dt);
        self.move_axis(map, tables, 1, self.vel.y * dt);
    }

    /// While swimming toward a solid wall ahead, looks for an opening within
    /// `MAX_CLIMB_HEIGHT` blocks above the current feet position (with room
    /// above it to stand) and, if found, gives `vel.y` a steady upward floor
    /// of `CLIMB_SPEED` - so swimming into a pool edge climbs you out over a
    /// second or so instead of just bumping the wall. Scans upward (rather
    /// than only checking right at the current feet cell) so this keeps
    /// pulling you up from anywhere in a multi-block-deep pool, not just the
    /// last block below the surface; bounded by `MAX_CLIMB_HEIGHT` so it
    /// can't turn into a general climb-any-cliff exploit.
    ///
    /// Checks fluid at the *feet* cell rather than reusing `step`'s
    /// chest-height `in_water` sample: that sample flips to "not in water"
    /// the instant your chest clears the surface, which is well before your
    /// feet actually reach ledge height, and full gravity reasserting itself
    /// at that exact moment would cut the climb short and drop you back in -
    /// staying keyed on the feet cell keeps the assist active for the whole
    /// climb, right up until you're actually standing on the ledge.
    fn assist_climb_out(&mut self, map: &ChunkMap, tables: &Tables, wish: Vec2) {
        let feet = self.pos.y.floor() as i32;
        let feet_wet = tables.fluid
            [map.get_block(IVec3::new(self.pos.x.floor() as i32, feet, self.pos.z.floor() as i32)) as usize];
        if !feet_wet {
            return;
        }
        let ahead = self.pos + Vec3::new(wish.x, 0.0, wish.y) * (HALF_W + 0.2);
        let (ax, az) = (ahead.x.floor() as i32, ahead.z.floor() as i32);
        let solid_at = |dy: i32| map.is_solid(tables, IVec3::new(ax, feet + dy, az));
        if !solid_at(0) {
            return;
        }
        if let Some(opening) = (1..=MAX_CLIMB_HEIGHT).find(|&dy| !solid_at(dy)) {
            if !solid_at(opening + 1) {
                self.vel.y = self.vel.y.max(CLIMB_SPEED);
            }
        }
    }

    /// Wait for terrain, then drop the player on a dry column near the origin.
    fn try_spawn(&mut self, map: &ChunkMap, tables: &Tables) {
        let mut r = 0;
        while r <= 24 {
            let mut dz = -r;
            while dz <= r {
                let mut dx = -r;
                while dx <= r {
                    if let Some(y) = map.surface_y(tables, 8 + dx, 8 + dz) {
                        if y > SEA_LEVEL {
                            self.pos = Vec3::new(
                                (8 + dx) as f32 + 0.5,
                                y as f32 + 1.0 + EPS,
                                (8 + dz) as f32 + 0.5,
                            );
                            self.vel = Vec3::ZERO;
                            self.spawned = true;
                            return;
                        }
                    }
                    dx += 4;
                }
                dz += 4;
            }
            r += 4;
        }
    }
}

fn spawn_camera(mut commands: Commands, settings: Res<WorldSettings>) {
    let far = (settings.render_distance * CHUNK_SIZE) as f32 * 2.0;
    let player = Player::default();
    commands.spawn((
        Camera3d::default(),
        Msaa::Off,
        Tonemapping::None,
        Projection::from(PerspectiveProjection {
            fov: 75.0_f32.to_radians(),
            near: 0.1,
            far,
            ..default()
        }),
        Transform::from_translation(player.eye()),
        player,
    ));
}

/// `OnEnter(AppState::InGame)`: grabs the mouse immediately (no "click to
/// play" step) and makes sure a stale pause flag from a previous session
/// can't leave the game start paused.
fn enter_game_grab(mut paused: ResMut<PauseState>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    paused.open = false;
    let Ok(mut window) = windows.single_mut() else { return };
    window.cursor_options.grab_mode = CursorGrabMode::Locked;
    window.cursor_options.visible = false;
}

/// Escape toggles the pause menu (see `menu::sync_pause_screen` for the
/// overlay itself): first press frees the cursor and pauses, second press
/// re-grabs and resumes. `menu::handle_menu_buttons` does the same re-grab
/// when "Resume" is clicked instead of pressed via Escape.
///
/// Also carries a click-to-regrab fallback for the rare case the OS steals
/// the pointer lock (e.g. alt-tab) without going through our own pause flow.
///
/// Skips entirely while chat or the inventory screen is open — those own
/// cursor grab and Escape in their own state (closing themselves, not
/// opening the pause menu).
fn cursor_grab(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    chat: Res<ChatState>,
    inventory: Res<InventoryState>,
    mut paused: ResMut<PauseState>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if chat.open || inventory.open {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };

    if keys.just_pressed(KeyCode::Escape) {
        paused.open = !paused.open;
        if paused.open {
            window.cursor_options.grab_mode = CursorGrabMode::None;
            window.cursor_options.visible = true;
        } else {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
        }
        return;
    }

    if !paused.open
        && mouse.just_pressed(MouseButton::Left)
        && window.cursor_options.grab_mode == CursorGrabMode::None
    {
        // Locked is ideal; X11 only supports Confined, so fall back.
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }
}

pub fn cursor_grabbed(windows: Query<&Window, With<PrimaryWindow>>) -> bool {
    windows
        .single()
        .map(|w| w.cursor_options.grab_mode != CursorGrabMode::None)
        .unwrap_or(false)
}

fn mouse_look(
    mut motion: EventReader<MouseMotion>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut players: Query<&mut Player>,
) {
    let grabbed = windows
        .single()
        .map(|w| w.cursor_options.grab_mode != CursorGrabMode::None)
        .unwrap_or(false);
    let Ok(mut player) = players.single_mut() else { return };
    for ev in motion.read() {
        if !grabbed {
            continue;
        }
        player.yaw -= ev.delta.x * LOOK_SENS;
        player.pitch = (player.pitch - ev.delta.y * LOOK_SENS).clamp(-1.553, 1.553);
    }
}

fn player_update(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<ChunkMap>,
    tables: Option<Res<BlockTables>>,
    mode: Res<GameMode>,
    chat: Res<ChatState>,
    paused: Res<PauseState>,
    inventory: Res<InventoryState>,
    mut players: Query<(&mut Player, &mut Transform)>,
) {
    let Some(tables) = tables else { return };
    let Ok((mut player, mut transform)) = players.single_mut() else { return };

    // Only the pause menu actually freezes the world. Chat and the
    // inventory screen just take over WASD/Space/mouse-look - gravity,
    // buoyancy, and momentum keep simulating underneath them, same as
    // vanilla Minecraft (opening your inventory doesn't stop you falling).
    if !paused.open {
        if !player.spawned {
            player.try_spawn(&map, &tables.0);
        } else {
            let frozen = chat.open || inventory.open;

            // Flying is a creative-only convenience; survival always keeps
            // both feet (eventually) on the ground.
            if !frozen {
                if *mode == GameMode::Creative {
                    if keys.just_pressed(KeyCode::KeyF) {
                        player.fly = !player.fly;
                        player.vel.y = 0.0;
                    }
                } else if player.fly {
                    player.fly = false;
                }
            }

            let real_keys: &ButtonInput<KeyCode> = &keys;
            let no_keys = ButtonInput::<KeyCode>::default();
            let input = if frozen { &no_keys } else { real_keys };

            player.accumulator = (player.accumulator + time.delta_secs()).min(0.25);
            while player.accumulator >= STEP {
                player.accumulator -= STEP;
                player.step(STEP, input, &map, &tables.0);
            }
        }
    }

    transform.translation = player.eye();
    transform.rotation = Quat::from_euler(EulerRot::YXZ, player.yaw, player.pitch, 0.0);
}

/// Label for player input/physics; interaction systems run after this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerSet;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseState>()
            .add_systems(Startup, spawn_camera)
            .add_systems(OnEnter(AppState::InGame), enter_game_grab)
            .add_systems(
                Update,
                (cursor_grab, mouse_look, player_update)
                    .chain()
                    .in_set(PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{block_index, CS, H};
    use crate::world::Chunk;

    /// A 3x3 grid of chunks with a solid stone floor at y <= 10.
    fn flat_world() -> (ChunkMap, std::sync::Arc<Tables>) {
        let mut reg = crate::blocks::BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices);
        let stone = reg.id("stone");

        let mut map = ChunkMap::default();
        for cz in -1..=1 {
            for cx in -1..=1 {
                let mut blocks = vec![0u16; CS * CS * H];
                for z in 0..CS {
                    for x in 0..CS {
                        for y in 0..=10 {
                            blocks[block_index(x, y, z)] = stone;
                        }
                    }
                }
                map.chunks.insert(
                    IVec2::new(cx, cz),
                    Chunk { blocks: Some(blocks), ..Chunk::default() },
                );
            }
        }
        (map, tables)
    }

    #[test]
    fn player_falls_and_lands_on_the_floor() {
        let (map, tables) = flat_world();
        let keys = ButtonInput::<KeyCode>::default();
        let mut player = Player {
            pos: Vec3::new(8.5, 20.0, 8.5),
            spawned: true,
            ..Player::default()
        };
        for _ in 0..600 {
            player.step(STEP, &keys, &map, &tables);
        }
        assert!(player.on_ground);
        assert!((player.pos.y - 11.0).abs() < 0.01, "y = {}", player.pos.y);
    }

    #[test]
    fn player_walks_without_sinking_and_is_stopped_by_walls() {
        let (mut map, tables) = flat_world();
        // wall at x = 12 across the walking line
        let stone = 1u16;
        {
            let chunk = map.chunks.get_mut(&IVec2::ZERO).unwrap();
            let blocks = chunk.blocks.as_mut().unwrap();
            for z in 0..CS {
                for y in 11..14 {
                    blocks[block_index(12, y, z)] = stone;
                }
            }
        }
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyD); // strafe +x at yaw where right = +x
        let mut player = Player {
            pos: Vec3::new(8.5, 11.001, 8.5),
            yaw: 0.0,
            spawned: true,
            ..Player::default()
        };
        for _ in 0..600 {
            player.step(STEP, &keys, &map, &tables);
        }
        // stopped just before the wall (wall face at x=12, half width 0.3)
        assert!(player.pos.x > 11.0 && player.pos.x <= 12.0 - HALF_W + 0.01,
            "x = {}", player.pos.x);
        assert!((player.pos.y - 11.0).abs() < 0.01, "sank: y = {}", player.pos.y);
    }

    #[test]
    fn swimming_into_a_flush_shore_climbs_out_without_pressing_space() {
        let mut reg = crate::blocks::BlockRegistry::with_defaults();
        let atlas = crate::atlas::build_atlas(&crate::atlas::default_painters());
        let tables = reg.compile(&atlas.indices);
        let stone = reg.id("stone");
        let water = reg.id("water");

        // Pool at x < 12 (floor at y<=10, three blocks of water above it);
        // shore at x >= 12 is flush stone up to the same height instead -
        // a normal one-block-tall pool edge with clear headroom above it.
        let mut blocks = vec![0u16; CS * CS * H];
        for z in 0..CS {
            for x in 0..CS {
                for y in 0..=10 {
                    blocks[block_index(x, y, z)] = stone;
                }
                for y in 11..=13 {
                    blocks[block_index(x, y, z)] = if x < 12 { water } else { stone };
                }
            }
        }
        let mut map = ChunkMap::default();
        map.chunks.insert(IVec2::ZERO, Chunk { blocks: Some(blocks), ..Chunk::default() });

        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyD); // +x, same convention as the wall test above
        let mut player = Player {
            pos: Vec3::new(11.3, 13.0, 8.5),
            yaw: 0.0,
            spawned: true,
            ..Player::default()
        };
        for _ in 0..600 {
            player.step(STEP, &keys, &map, &tables);
        }

        assert!(player.pos.x > 12.0, "never made it onto the shore: x = {}", player.pos.x);
        assert!(player.pos.y >= 13.99, "didn't climb up onto land: y = {}", player.pos.y);
    }
}
