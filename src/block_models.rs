//! Draws the model of blocks whose shape is `Shape::Model` - the training
//! dummy. The block (two cells tall: `training_dummy` and its
//! `training_dummy_top`) is what's really in the world: it collides, is
//! mined, drops, saves and relights like any block. The mesher draws nothing
//! for it, and this module stands the matching entity model (the entity
//! definition with the same id, see `entities.rs`) in the lower cell.
//!
//! The model entities are bookkeeping, not state: they're recreated from
//! the blocks whenever a chunk is meshed or a block changes, and removed when
//! their block goes away or its chunk unloads, so nothing here is ever saved.
//! The model always faces +Z: a block has no yaw of its own to remember.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::blocks::{BlockId, BlockRegistry, Shape};
use crate::config::{block_index, CS, H};
use crate::entities::{BlockModel, EntityRegistry, EntitySet, SpawnEntity};
use crate::state::AppState;
use crate::world::{BlockSetEvent, ChunkMap, ChunkMeshedEvent};

/// How often (seconds) existing models are checked against their blocks, to
/// catch a chunk that unloaded.
const VALIDATE_EVERY: f32 = 0.5;
/// The way the model faces: toward +Z.
const FACING_YAW: f32 = std::f32::consts::PI;

/// The model entity for each block cell that has one.
#[derive(Resource, Default)]
pub struct BlockModels(pub HashMap<IVec3, Entity>);

/// The entity kind drawn for `block`, if it's a model block with an entity
/// definition of the same id.
fn model_kind(block: BlockId, blocks: &BlockRegistry, entities: &EntityRegistry) -> Option<usize> {
    let def = blocks.def(block);
    (def.shape == Shape::Model).then(|| entities.by_name(&def.id)).flatten()
}

/// Where a model stands for the block in `cell`: centred, feet on the cell's floor.
pub fn model_position(cell: IVec3) -> Vec3 {
    Vec3::new(cell.x as f32 + 0.5, cell.y as f32, cell.z as f32 + 0.5)
}

/// Every cell in the chunk holding one of `model_blocks`.
fn model_cells(coord: IVec2, blocks: &[BlockId], model_blocks: &[BlockId]) -> Vec<IVec3> {
    let mut cells = Vec::new();
    for z in 0..CS {
        for x in 0..CS {
            let column = &blocks[block_index(x, 0, z)..block_index(x, 0, z) + H];
            for (y, id) in column.iter().enumerate() {
                if *id != 0 && model_blocks.contains(id) {
                    cells.push(IVec3::new(coord.x * CS as i32 + x as i32, y as i32, coord.y * CS as i32 + z as i32));
                }
            }
        }
    }
    cells
}

#[allow(clippy::too_many_arguments)]
fn sync_block_models(
    mut commands: Commands,
    mut set_events: EventReader<BlockSetEvent>,
    mut meshed: EventReader<ChunkMeshedEvent>,
    map: Res<ChunkMap>,
    blocks: Res<BlockRegistry>,
    entities: Res<EntityRegistry>,
    mut live: ResMut<BlockModels>,
    mut spawn: EventWriter<SpawnEntity>,
    mut timer: Local<f32>,
    time: Res<Time>,

) {
    // Cells to look at again: those that just changed, and every model block
    // in a chunk that was just (re)meshed (which is also how a chunk that
    // loads in with a dummy already in it gets its model).
    let mut cells: Vec<IVec3> = set_events.read().map(|e| e.pos).collect();
    let model_blocks: Vec<BlockId> = (1..blocks.defs.len() as BlockId)
        .filter(|&id| model_kind(id, &blocks, &entities).is_some())
        .collect();
    if !model_blocks.is_empty() {
        for ChunkMeshedEvent(coord) in meshed.read() {
            if let Some(chunk_blocks) = map.chunks.get(coord).and_then(|c| c.blocks.as_deref()) {
                cells.extend(model_cells(*coord, chunk_blocks, &model_blocks));
            }
        }
    } else {
        meshed.clear();
    }

    for cell in cells {
        let wanted = model_kind(map.get_block(cell), &blocks, &entities);
        match (wanted, live.0.contains_key(&cell)) {
            (Some(kind), false) => {
                let mut ev = SpawnEntity::at(kind, model_position(cell), FACING_YAW);
                ev.block_cell = Some(cell);
                // Registered now so a second look this frame doesn't spawn another;
                // the real entity is attached by `attach_spawned`.
                live.0.insert(cell, Entity::PLACEHOLDER);
                spawn.write(ev);
            }
            (None, true) => {
                if let Some(entity) = live.0.remove(&cell) {
                    if entity != Entity::PLACEHOLDER {
                        commands.entity(entity).despawn();
                    }
                }
            }
            _ => {}
        }
    }

    // Now and then: forget models whose block is gone or whose chunk unloaded.
    *timer += time.delta_secs();
    if *timer >= VALIDATE_EVERY {
        *timer = 0.0;
        let stale: Vec<IVec3> = live
            .0
            .keys()
            .copied()
            .filter(|&cell| !map.is_loaded(cell) || model_kind(map.get_block(cell), &blocks, &entities).is_none())
            .collect();
        for cell in stale {
            if let Some(entity) = live.0.remove(&cell) {
                if entity != Entity::PLACEHOLDER {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

/// Fills in the real entity for a cell once the spawn event has been
/// handled (`entities::spawn_entities` runs earlier in the frame chain).
fn attach_spawned(mut live: ResMut<BlockModels>, spawned: Query<(Entity, &BlockModel), Added<BlockModel>>) {
    for (entity, model) in &spawned {
        live.0.insert(model.cell, entity);
    }
}

fn forget_on_exit(mut live: ResMut<BlockModels>) {
    live.0.clear();
}

pub struct BlockModelsPlugin;

impl Plugin for BlockModelsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockModels>()
            .add_systems(OnExit(AppState::InGame), forget_on_exit)
            .add_systems(
                Update,
                (
                    sync_block_models.before(EntitySet),
                    // After `spawn_entities` has made the entities and their
                    // commands have been applied.
                    attach_spawned.after(EntitySet),
                )
                    .run_if(in_state(AppState::InGame)),
            );
    }
}
