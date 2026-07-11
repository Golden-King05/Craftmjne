//! End-to-end headless test: runs the real ECS app (no window, no GPU) and
//! verifies the chunk pipeline — generation tasks, padded meshing tasks,
//! entity spawning, block edits and remeshing — through the actual schedule,
//! including the `AppState::InGame` transition and per-world save/load.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use craftmjne::config::WorldSettings;
use craftmjne::player::Player;
use craftmjne::render::{ChunkMaterial, ChunkMaterials};
use craftmjne::save::{GameMode, SaveStore};
use craftmjne::state::{ActiveWorld, AppState};
use craftmjne::world::{ChunkMap, WorldPlugin};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A throwaway save directory, removed when the returned guard drops, so
/// concurrently-running tests never share (or race on) real save state.
struct TempSaves(std::path::PathBuf);
impl Drop for TempSaves {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn temp_saves() -> TempSaves {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    TempSaves(std::env::temp_dir().join(format!("craftmjne-headless-test-{}-{n}", std::process::id())))
}

/// Builds the real app (minus rendering/windowing) and drives it into
/// `AppState::InGame` for a freshly created world, exactly as the menu would.
fn headless_app(temp: &TempSaves) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), StatesPlugin));
    app.init_asset::<Mesh>();
    app.init_asset::<Image>();
    app.init_asset::<ChunkMaterial>();
    app.insert_resource(WorldSettings { seed: 7, render_distance: 2 });
    app.insert_resource(SaveStore::at(temp.0.clone()));
    // Placeholder material handles: no render app in this test.
    app.insert_resource(ChunkMaterials {
        solid: Handle::default(),
        water: Handle::default(),
    });
    app.init_state::<AppState>();
    app.add_plugins(WorldPlugin);
    app.world_mut().spawn(Player::default());

    let (slug, meta) = app.world().resource::<SaveStore>().create_world("Test World", 7, GameMode::Survival).unwrap();
    app.world_mut().insert_resource(ActiveWorld { slug, meta });
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
    app.update(); // process the MainMenu -> InGame transition (runs `enter_world`)

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
    let temp = temp_saves();
    let mut app = headless_app(&temp);

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
    let temp = temp_saves();
    let mut app = headless_app(&temp);
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
    let temp = temp_saves();
    let mut app = headless_app(&temp);
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

#[test]
fn leaving_and_reentering_a_world_persists_edits_and_player_pose() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    assert!(run_until(
        &mut app,
        |app| app.world().resource::<ChunkMap>().stats().1 >= 9,
        2000,
    ));

    // Make an edit and move the player, then leave the world (OnExit saves).
    let edit_pos = {
        let map = app.world().resource::<ChunkMap>();
        let tables = app.world().resource::<craftmjne::blocks::BlockTables>().clone();
        let y = map.surface_y(&tables.0, 8, 8).unwrap();
        IVec3::new(8, y, 8)
    };
    app.world_mut().resource_mut::<ChunkMap>().set_block(edit_pos, 0);
    {
        let mut players = app.world_mut().query::<&mut Player>();
        let mut player = players.single_mut(app.world_mut()).unwrap();
        player.pos = Vec3::new(100.5, 40.0, 100.5);
        player.spawned = true;
    }
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update(); // runs `exit_world`, writing the save to disk

    // Re-enter the same world fresh (simulating quit-and-relaunch): a new
    // app, pointed at the same save directory.
    let mut app2 = App::new();
    app2.add_plugins((MinimalPlugins, AssetPlugin::default(), StatesPlugin));
    app2.init_asset::<Mesh>();
    app2.init_asset::<Image>();
    app2.init_asset::<ChunkMaterial>();
    app2.insert_resource(WorldSettings { seed: 7, render_distance: 2 });
    app2.insert_resource(SaveStore::at(temp.0.clone()));
    app2.insert_resource(ChunkMaterials { solid: Handle::default(), water: Handle::default() });
    app2.init_state::<AppState>();
    app2.add_plugins(WorldPlugin);
    app2.world_mut().spawn(Player::default());

    let store = app2.world().resource::<SaveStore>();
    let (slug, meta) = store.list_worlds().into_iter().next().expect("world was saved");
    app2.world_mut().insert_resource(ActiveWorld { slug, meta });
    app2.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
    app2.update();

    // Player position restored immediately on entry.
    {
        let mut players = app2.world_mut().query::<&Player>();
        let player = players.single(app2.world()).unwrap();
        assert_eq!(player.pos, Vec3::new(100.5, 40.0, 100.5));
    }

    // The edited block re-applies once its chunk regenerates.
    assert!(run_until(&mut app2, |app| app.world().resource::<ChunkMap>().get_block(edit_pos) == 0, 2000));
}
