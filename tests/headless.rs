//! End-to-end headless test: runs the real ECS app (no window, no GPU) and
//! verifies the chunk pipeline — generation tasks, padded meshing tasks,
//! entity spawning, block edits and remeshing — through the actual schedule.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use std::time::Duration;

use craftmjne::config::WorldSettings;
use craftmjne::player::Player;
use craftmjne::render::{ChunkMaterial, ChunkMaterials};
use craftmjne::world::{ChunkMap, WorldPlugin};

fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app.init_asset::<Mesh>();
    app.init_asset::<Image>();
    app.init_asset::<ChunkMaterial>();
    app.insert_resource(WorldSettings { seed: 7, render_distance: 2 });
    // Placeholder material handles: no render app in this test.
    app.insert_resource(ChunkMaterials {
        solid: Handle::default(),
        water: Handle::default(),
    });
    app.add_plugins(WorldPlugin);
    app.world_mut().spawn(Player::default());
    app
}

fn run_until(app: &mut App, mut done: impl FnMut(&mut App) -> bool, max_iters: u32) -> bool {
    for _ in 0..max_iters {
        app.update();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

#[test]
fn world_streams_generates_and_meshes() {
    let mut app = headless_app();

    let ok = run_until(
        &mut app,
        |app| {
            let (generated, meshed) = app.world().resource::<ChunkMap>().stats();
            generated >= 25 && meshed >= 9
        },
        2000,
    );
    let (generated, meshed) = app.world().resource::<ChunkMap>().stats();
    assert!(ok, "pipeline stalled: generated={generated} meshed={meshed}");

    // Chunk entities with mesh handles exist, and the meshes are real assets.
    let world = app.world_mut();
    let mesh_entities: Vec<Mesh3d> = world
        .query::<&Mesh3d>()
        .iter(world)
        .cloned()
        .collect();
    assert!(!mesh_entities.is_empty());
    let meshes = world.resource::<Assets<Mesh>>();
    let mesh = meshes.get(&mesh_entities[0].0).expect("mesh asset exists");
    assert!(mesh.count_vertices() > 0);
}

#[test]
fn block_edit_marks_dirty_and_remeshes() {
    let mut app = headless_app();
    assert!(run_until(
        &mut app,
        |app| app.world().resource::<ChunkMap>().stats().1 >= 9,
        2000,
    ));

    // Find the surface of the spawn column and knock a block out.
    let (surface, edit_pos) = {
        let map = app.world().resource::<ChunkMap>();
        let tables = app
            .world()
            .resource::<craftmjne::blocks::BlockTables>()
            .clone();
        let y = map.surface_y(&tables.0, 8, 8).expect("spawn column generated");
        (y, IVec3::new(8, y, 8))
    };
    assert!(surface > 0);

    let before = {
        let map = app.world().resource::<ChunkMap>();
        map.get_block(edit_pos)
    };
    assert_ne!(before, 0);

    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        let prev = map.set_block(edit_pos, 0).expect("edit applies");
        assert_eq!(prev, before);
        let chunk = map.chunks.get(&IVec2::ZERO).unwrap();
        assert!(chunk.dirty || chunk.meshing);
    }

    // The edit must be readable back and the chunk must remesh (dirty clears
    // once a fresh mesh for the new version has been applied).
    assert_eq!(app.world().resource::<ChunkMap>().get_block(edit_pos), 0);
    let ok = run_until(
        &mut app,
        |app| {
            let map = app.world().resource::<ChunkMap>();
            let c = map.chunks.get(&IVec2::ZERO).unwrap();
            c.meshed && !c.dirty && !c.meshing
        },
        2000,
    );
    assert!(ok, "chunk never remeshed after edit");
}

#[test]
fn edge_edit_dirties_neighbour_chunk() {
    let mut app = headless_app();
    assert!(run_until(
        &mut app,
        |app| app.world().resource::<ChunkMap>().stats().1 >= 9,
        2000,
    ));

    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        // x=0 sits on the border between chunk (0,0) and chunk (-1,0).
        map.set_block(IVec3::new(0, 30, 8), 1);
        let neighbour = map.chunks.get(&IVec2::new(-1, 0)).unwrap();
        assert!(neighbour.dirty || neighbour.meshing);
    }
}
