//! First-person player: AABB physics against the voxel grid, swimming,
//! fly mode, and fixed-timestep integration (120 Hz substeps) so behaviour is
//! framerate-independent. The `Player` component lives on the camera entity;
//! `pos` is the feet position, the camera sits at eye height above it.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, PrimaryWindow};

use crate::blocks::{BlockTables, Tables};
use crate::config::{WorldSettings, CHUNK_SIZE, SEA_LEVEL, WORLD_HEIGHT};
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
        water_id: u16,
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

        if self.fly {
            let speed = FLY_SPEED * if sprint { 2.5 } else { 1.0 };
            self.vel.x = wish.x * speed;
            self.vel.z = wish.y * speed;
            self.vel.y = (keys.pressed(KeyCode::Space) as i32
                - keys.pressed(KeyCode::ShiftLeft) as i32) as f32
                * speed;
        } else {
            let body = self.pos + Vec3::Y * 0.6;
            let in_water = map.get_block(body.floor().as_ivec3()) == water_id;
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

        self.on_ground = false;
        self.move_axis(map, tables, 0, self.vel.x * dt);
        self.move_axis(map, tables, 2, self.vel.z * dt);
        self.move_axis(map, tables, 1, self.vel.y * dt);
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

fn cursor_grab(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = windows.single_mut() else { return };
    if mouse.just_pressed(MouseButton::Left) && window.cursor_options.grab_mode == CursorGrabMode::None {
        // Locked is ideal; X11 only supports Confined, so fall back.
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }
    if keys.just_pressed(KeyCode::Escape) {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
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
    registry: Res<crate::blocks::BlockRegistry>,
    mut players: Query<(&mut Player, &mut Transform)>,
) {
    let Some(tables) = tables else { return };
    let Ok((mut player, mut transform)) = players.single_mut() else { return };

    if keys.just_pressed(KeyCode::KeyF) {
        player.fly = !player.fly;
        player.vel.y = 0.0;
    }

    if !player.spawned {
        player.try_spawn(&map, &tables.0);
    } else {
        let water_id = registry.id("water");
        player.accumulator = (player.accumulator + time.delta_secs()).min(0.25);
        while player.accumulator >= STEP {
            player.accumulator -= STEP;
            player.step(STEP, &keys, &map, &tables.0, water_id);
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
        app.add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (cursor_grab, mouse_look, player_update).chain().in_set(PlayerSet),
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
            player.step(STEP, &keys, &map, &tables, 9999);
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
            player.step(STEP, &keys, &map, &tables, 9999);
        }
        // stopped just before the wall (wall face at x=12, half width 0.3)
        assert!(player.pos.x > 11.0 && player.pos.x <= 12.0 - HALF_W + 0.01,
            "x = {}", player.pos.x);
        assert!((player.pos.y - 11.0).abs() < 0.01, "sank: y = {}", player.pos.y);
    }
}
