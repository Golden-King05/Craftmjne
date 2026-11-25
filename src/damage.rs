//! Damage, from any source, to anything that takes it.
//!
//! Everything that hurts something writes one [`DamageEvent`]: where it
//! landed (a block cell), how much, and what it was ([`DamageSource`]). The
//! receiving side is a single system here, so a new way of dealing damage
//! (a falling block, an explosion) only has to write the event - it never
//! needs to know what the target is - and a new target only has to be added
//! here, not taught about every source.
//!
//! Right now the one target is a `hittable` block (the training dummy): it
//! has no health, it just shows the number as a particle. Sources written so
//! far: a held tool or fist (`Melee`) and a thrown pebble (`Projectile`).
//! `Falling` is for falling blocks and falls, which don't exist yet, but are
//! accepted and tested like the others.

use bevy::prelude::*;

use crate::blocks::{BlockId, BlockRegistry, AIR};
use crate::particles::DamageNumberEvent;
use crate::state::AppState;
use crate::world::ChunkMap;

/// What dealt the damage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DamageSource {
    /// A held tool, sword or bare hand.
    Melee,
    /// Something thrown or shot.
    Projectile,
    /// Something that fell on it (or, later, a fall).
    Falling,
}

impl DamageSource {
    pub const ALL: [DamageSource; 3] = [DamageSource::Melee, DamageSource::Projectile, DamageSource::Falling];
}

/// `amount` damage was dealt at the block cell `cell`.
#[derive(Event, Clone, Copy, Debug)]
pub struct DamageEvent {
    pub cell: IVec3,
    pub amount: f32,
    pub source: DamageSource,
}

/// The feet of whatever a block hit belongs to: the cell itself, or for the
/// upper half of a two-cell-tall block the cell below it, centred.
pub fn hit_origin(registry: &BlockRegistry, pos: IVec3, block: BlockId) -> Vec3 {
    let base = if registry.lower_half(block).is_some() { pos - IVec3::Y } else { pos };
    Vec3::new(base.x as f32 + 0.5, base.y as f32, base.z as f32 + 0.5)
}

/// Delivers each [`DamageEvent`] to the block it landed on. A `hittable` block
/// takes it (and shows the number); anything else ignores it.
fn damage_blocks(
    mut events: EventReader<DamageEvent>,
    map: Res<ChunkMap>,
    blocks: Res<BlockRegistry>,
    mut numbers: EventWriter<DamageNumberEvent>,
) {
    for event in events.read() {
        let block = map.get_block(event.cell);
        if block != AIR && blocks.def(block).hittable && event.amount > 0.0 {
            numbers.write(DamageNumberEvent { origin: hit_origin(&blocks, event.cell, block), amount: event.amount });
        }
    }
}

pub struct DamagePlugin;

impl Plugin for DamagePlugin {
    fn build(&self, app: &mut App) {
        app.add_event::<DamageEvent>()
            .add_systems(Update, damage_blocks.run_if(in_state(AppState::InGame)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;

    fn app() -> (App, BlockRegistry) {
        let reg = BlockRegistry::with_defaults();
        let mut map = ChunkMap::default();
        map.chunks.insert(
            IVec2::ZERO,
            Chunk {
                blocks: Some(vec![0; crate::config::CS * crate::config::CS * crate::config::H]),
                ..Default::default()
            },
        );
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .init_state::<AppState>()
            .insert_resource(map)
            .insert_resource(BlockRegistry::with_defaults())
            .add_event::<DamageNumberEvent>()
            .init_resource::<Shown>()
            .add_systems(Update, |mut events: EventReader<DamageNumberEvent>, mut shown: ResMut<Shown>| {
                shown.0.extend(events.read().copied());
            })
            .add_plugins(DamagePlugin);
        app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
        app.update();
        (app, reg)
    }

    /// Every number shown since the last call.
    #[derive(Resource, Default)]
    struct Shown(Vec<DamageNumberEvent>);

    fn numbers(app: &mut App) -> Vec<DamageNumberEvent> {
        // One more update so the collector (which may run before `damage_blocks`
        // within a frame) has seen everything the last update produced.
        app.update();
        std::mem::take(&mut app.world_mut().resource_mut::<Shown>().0)
    }

    #[test]
    fn the_dummy_takes_damage_from_every_kind_of_source() {
        let (mut app, reg) = app();
        let (lower, upper) = (reg.id("training_dummy"), reg.id("training_dummy_top"));
        let base = IVec3::new(4, 10, 4);
        {
            let mut map = app.world_mut().resource_mut::<ChunkMap>();
            map.set_block(base, lower);
            map.set_block(base + IVec3::Y, upper);
        }
        for (i, source) in DamageSource::ALL.into_iter().enumerate() {
            let amount = 1.5 + i as f32;
            // Whichever half is struck, the number comes from its feet.
            for cell in [base, base + IVec3::Y] {
                app.world_mut().send_event(DamageEvent { cell, amount, source });
                app.update();
                let got = numbers(&mut app);
                assert_eq!(got.len(), 1, "{source:?} on {cell}: {got:?}");
                assert_eq!(got[0].amount, amount);
                assert_eq!(got[0].origin, Vec3::new(4.5, 10.0, 4.5));
            }
        }
    }

    #[test]
    fn nothing_else_takes_damage_and_empty_hits_do_nothing() {
        let (mut app, reg) = app();
        let base = IVec3::new(4, 10, 4);
        app.world_mut().resource_mut::<ChunkMap>().set_block(base, reg.id("stone"));
        for cell in [base, base + IVec3::Y * 5] {
            app.world_mut().send_event(DamageEvent { cell, amount: 3.0, source: DamageSource::Melee });
            app.update();
            assert!(numbers(&mut app).is_empty(), "stone and air show no number");
        }
        let dummy = base + IVec3::X;
        app.world_mut().resource_mut::<ChunkMap>().set_block(dummy, reg.id("training_dummy"));
        app.world_mut().send_event(DamageEvent { cell: dummy, amount: 0.0, source: DamageSource::Falling });
        app.update();
        assert!(numbers(&mut app).is_empty(), "zero damage shows nothing");
    }

    #[test]
    fn a_hit_on_either_half_is_measured_from_the_dummys_feet() {
        let reg = BlockRegistry::with_defaults();
        let (lower, upper) = (reg.id("training_dummy"), reg.id("training_dummy_top"));
        assert!(reg.def(lower).hittable && reg.def(upper).hittable && !reg.def(reg.id("stone")).hittable);
        assert_eq!(hit_origin(&reg, IVec3::new(4, 10, -3), lower), Vec3::new(4.5, 10.0, -2.5));
        assert_eq!(hit_origin(&reg, IVec3::new(4, 11, -3), upper), Vec3::new(4.5, 10.0, -2.5));
    }
}
