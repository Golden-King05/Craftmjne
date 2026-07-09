//! Block targeting (voxel DDA raycast), breaking, placing, block picking,
//! and hotbar selection.

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;

use crate::blocks::{BlockId, BlockRegistry, AIR};
use crate::config::WORLD_HEIGHT;
use crate::player::{cursor_grabbed, Player};
use crate::state::AppState;
use crate::world::{BlockSetEvent, ChunkMap};

const REACH: f32 = 6.0;
const ACTION_REPEAT: f32 = 0.22; // seconds between repeats while a button is held

pub struct RayHit {
    pub pos: IVec3,
    pub normal: IVec3,
}

/// Voxel raycast (Amanatides & Woo DDA). Steps exactly through the grid cells
/// along the ray — no float sampling, no missed corners.
pub fn raycast_voxel(
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
    mut hit_test: impl FnMut(IVec3) -> bool,
) -> Option<RayHit> {
    let mut cell = origin.floor().as_ivec3();
    let step = IVec3::new(
        if dir.x > 0.0 { 1 } else { -1 },
        if dir.y > 0.0 { 1 } else { -1 },
        if dir.z > 0.0 { 1 } else { -1 },
    );
    let t_delta = Vec3::new(
        if dir.x != 0.0 { (1.0 / dir.x).abs() } else { f32::INFINITY },
        if dir.y != 0.0 { (1.0 / dir.y).abs() } else { f32::INFINITY },
        if dir.z != 0.0 { (1.0 / dir.z).abs() } else { f32::INFINITY },
    );
    let bound = |o: f32, c: i32, d: f32| {
        if d > 0.0 { c as f32 + 1.0 - o } else { o - c as f32 }
    };
    let mut t_max = Vec3::new(
        bound(origin.x, cell.x, dir.x) * t_delta.x,
        bound(origin.y, cell.y, dir.y) * t_delta.y,
        bound(origin.z, cell.z, dir.z) * t_delta.z,
    );

    let mut normal = IVec3::ZERO;
    let mut t = 0.0;
    while t <= max_dist {
        if hit_test(cell) {
            if normal == IVec3::ZERO {
                return None; // started inside a block
            }
            return Some(RayHit { pos: cell, normal });
        }
        if t_max.x < t_max.y && t_max.x < t_max.z {
            cell.x += step.x;
            t = t_max.x;
            t_max.x += t_delta.x;
            normal = IVec3::new(-step.x, 0, 0);
        } else if t_max.y < t_max.z {
            cell.y += step.y;
            t = t_max.y;
            t_max.y += t_delta.y;
            normal = IVec3::new(0, -step.y, 0);
        } else {
            cell.z += step.z;
            t = t_max.z;
            t_max.z += t_delta.z;
            normal = IVec3::new(0, 0, -step.z);
        }
    }
    None
}

#[derive(Resource)]
pub struct Hotbar {
    pub slots: Vec<BlockId>,
    pub selected: usize,
}

#[derive(Resource, Default)]
pub struct Target(pub Option<RayHit>);

#[derive(Resource, Default)]
struct ActionTimers {
    break_t: f32,
    place_t: f32,
}

fn setup_hotbar(mut commands: Commands, registry: Res<BlockRegistry>) {
    let slots = [
        "grass", "dirt", "stone", "cobblestone", "planks", "log", "leaves", "glass", "bricks",
    ]
    .iter()
    .map(|n| registry.id(n))
    .collect();
    commands.insert_resource(Hotbar { slots, selected: 0 });
}

fn select_slot(
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: EventReader<MouseWheel>,
    mut hotbar: ResMut<Hotbar>,
) {
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5,
        KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
    ];
    for (i, key) in DIGITS.iter().enumerate() {
        if keys.just_pressed(*key) && i < hotbar.slots.len() {
            hotbar.selected = i;
        }
    }
    let scroll: f32 = wheel.read().map(|e| e.y).sum();
    if scroll != 0.0 {
        let n = hotbar.slots.len() as i32;
        let step = if scroll < 0.0 { 1 } else { -1 };
        hotbar.selected = ((hotbar.selected as i32 + step).rem_euclid(n)) as usize;
    }
}

#[allow(clippy::too_many_arguments)]
fn interact(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut map: ResMut<ChunkMap>,
    registry: Res<BlockRegistry>,
    mut hotbar: ResMut<Hotbar>,
    mut target: ResMut<Target>,
    mut timers: Local<ActionTimers>,
    players: Query<&Player>,
    mut block_events: EventWriter<BlockSetEvent>,
    mut gizmos: Gizmos,
) {
    let Ok(player) = players.single() else { return };

    // Crosshair target.
    target.0 = if player.spawned {
        raycast_voxel(player.eye(), player.look_dir(), REACH, |cell| {
            let id = map.get_block(cell);
            id != AIR && registry.def(id).selectable
        })
    } else {
        None
    };

    if let Some(hit) = &target.0 {
        gizmos.cuboid(
            Transform::from_translation(hit.pos.as_vec3() + Vec3::splat(0.5))
                .with_scale(Vec3::splat(1.002)),
            Color::srgba(0.05, 0.05, 0.05, 0.9),
        );
    }

    let dt = time.delta_secs();
    timers.break_t -= dt;
    timers.place_t -= dt;
    if !mouse.pressed(MouseButton::Left) {
        timers.break_t = 0.0;
    }
    if !mouse.pressed(MouseButton::Right) {
        timers.place_t = 0.0;
    }
    if !cursor_grabbed(windows) {
        return;
    }
    let Some(hit) = &target.0 else { return };

    // Break (left click / hold).
    if mouse.pressed(MouseButton::Left) && timers.break_t <= 0.0 {
        let id = map.get_block(hit.pos);
        if registry.def(id).breakable {
            if let Some(prev) = map.set_block(hit.pos, AIR) {
                block_events.write(BlockSetEvent { pos: hit.pos, id: AIR, prev });
            }
        }
        timers.break_t = ACTION_REPEAT;
    }

    // Place (right click / hold).
    if mouse.pressed(MouseButton::Right) && timers.place_t <= 0.0 {
        let place_pos = hit.pos + hit.normal;
        let id = hotbar.slots[hotbar.selected];
        if place_pos.y >= 0 && place_pos.y < WORLD_HEIGHT {
            let existing = map.get_block(place_pos);
            let replaceable = existing == AIR || registry.def(existing).replaceable;
            let blocked = registry.def(id).solid && player.intersects_block(place_pos);
            if replaceable && !blocked {
                if let Some(prev) = map.set_block(place_pos, id) {
                    block_events.write(BlockSetEvent { pos: place_pos, id, prev });
                }
            }
        }
        timers.place_t = ACTION_REPEAT;
    }

    // Pick block (middle click): put the targeted block in the current slot.
    if mouse.just_pressed(MouseButton::Middle) {
        let id = map.get_block(hit.pos);
        if id != AIR {
            if let Some(existing) = hotbar.slots.iter().position(|&s| s == id) {
                hotbar.selected = existing;
            } else {
                let sel = hotbar.selected;
                hotbar.slots[sel] = id;
            }
        }
    }
}

pub struct InteractPlugin;

impl Plugin for InteractPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>()
            .add_systems(Startup, setup_hotbar)
            .add_systems(
                Update,
                (select_slot, interact)
                    .chain()
                    .after(crate::player::PlayerSet)
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raycast_hits_the_expected_cell_and_face() {
        // Block at (5, 0, 0); ray along +x from origin height 0.5.
        let hit = raycast_voxel(
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::X,
            10.0,
            |c| c == IVec3::new(5, 0, 0),
        )
        .expect("hit");
        assert_eq!(hit.pos, IVec3::new(5, 0, 0));
        assert_eq!(hit.normal, IVec3::new(-1, 0, 0)); // entered from -x side

        // Diagonal ray down onto a floor: normal must be +y.
        let hit = raycast_voxel(
            Vec3::new(0.5, 5.5, 0.5),
            Vec3::new(0.4, -1.0, 0.3).normalize(),
            20.0,
            |c| c.y < 0,
        )
        .expect("hit floor");
        assert_eq!(hit.normal, IVec3::Y);
        assert_eq!(hit.pos.y, -1);
    }

    #[test]
    fn raycast_respects_max_distance_and_inside_start() {
        assert!(raycast_voxel(Vec3::splat(0.5), Vec3::X, 3.0, |c| c.x == 5).is_none());
        // starting inside a hit cell yields None (no face to place against)
        assert!(raycast_voxel(Vec3::splat(0.5), Vec3::X, 3.0, |_| true).is_none());
    }
}
