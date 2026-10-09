//! End-to-end headless test: runs the real ECS app (no window, no GPU) and
//! verifies the chunk pipeline — generation tasks, padded meshing tasks,
//! entity spawning, block edits and remeshing — through the actual schedule,
//! including the `AppState::InGame` transition and per-world save/load.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use craftmjne::blocks::{BlockRegistry, AXIS_Y};
use craftmjne::config::{WorldSettings, CHUNK_SIZE};
use craftmjne::light::{LightPlugin, LightQueue, LEVEL_STEP, MAX_LIGHT};
use craftmjne::player::Player;
use craftmjne::render::{ChunkMaterial, ChunkMaterials};
use craftmjne::save::{FluidCell, GameMode, SaveStore};
use craftmjne::sky::DayNightClock;
use craftmjne::snapshot::ChunkStore;
use craftmjne::terrain::TerrainGenerator;
use craftmjne::state::{ActiveWorld, AppState};
use craftmjne::world::{BlockSetEvent, ChunkMap, WorldPlugin};

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
    app.add_plugins((WorldPlugin, LightPlugin));
    app.world_mut().spawn(Player::default());

    let (slug, meta) = app.world().resource::<SaveStore>().create_world("Test World", 7, GameMode::Survival).unwrap();
    app.world_mut().insert_resource(ActiveWorld { slug, meta });
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
    app.update(); // process the MainMenu -> InGame transition (runs `enter_world`)

    app
}

/// Builds a fresh app pointed at an *existing* save directory and loads its
/// (only) world, simulating a quit-and-relaunch: a brand new app, same save
/// on disk, no state carried over except what's on disk.
fn reload_app(temp: &TempSaves) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default(), StatesPlugin));
    app.init_asset::<Mesh>();
    app.init_asset::<Image>();
    app.init_asset::<ChunkMaterial>();
    app.insert_resource(WorldSettings { seed: 7, render_distance: 2 });
    app.insert_resource(SaveStore::at(temp.0.clone()));
    app.insert_resource(ChunkMaterials { solid: Handle::default(), water: Handle::default() });
    app.init_state::<AppState>();
    app.add_plugins((WorldPlugin, LightPlugin));
    app.world_mut().spawn(Player::default());

    let store = app.world().resource::<SaveStore>();
    let (slug, meta) = store.list_worlds().into_iter().next().expect("world was saved");
    app.world_mut().insert_resource(ActiveWorld { slug, meta });
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::InGame);
    app.update();

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

/// A fresh world starts its day/night clock partway into a bright morning
/// (see `sky::NEW_WORLD_START_TIME` - not literal dawn, which renders almost
/// fully dark) and at a new moon; leaving mid-cycle and reloading resumes
/// from wherever it was left, rather than resetting - the same "don't
/// silently lose continuous state on reload" principle as fluid levels
/// (`water_restores_exactly_after_leaving_and_reentering_a_world`).
/// `SkyPlugin` itself isn't part of this headless app (it needs rendering
/// infrastructure this test doesn't set up), so the clock is advanced by
/// hand here - this test is purely about `world.rs`'s save/load wiring, not
/// `sky.rs`'s own per-frame advancing (covered by its pure-logic unit tests).
#[test]
fn time_of_day_and_moon_phase_persist_across_a_reload() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    {
        let clock = app.world().resource::<DayNightClock>();
        assert_eq!(
            clock.elapsed,
            craftmjne::sky::NEW_WORLD_START_TIME,
            "a fresh world starts partway into a bright morning, not literal (near-black) dawn"
        );
        assert_eq!(clock.moon_phase(), 0, "a fresh world starts at a new moon");
    }

    {
        let mut clock = app.world_mut().resource_mut::<DayNightClock>();
        clock.elapsed = 543.0;
        clock.day_count = 11; // moon_phase() == 3, waxing gibbous
    }
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update(); // runs `exit_world`, writing the save to disk

    let app2 = reload_app(&temp);
    let clock = app2.world().resource::<DayNightClock>();
    assert_eq!(clock.elapsed, 543.0);
    assert_eq!(clock.day_count, 11);
    assert_eq!(clock.moon_phase(), 3);
}

/// `world::enter_world` only hands a brand-new world `sky::
/// NEW_WORLD_START_TIME` when it has genuinely never been saved - an
/// existing save whose `data.json` predates the `time_of_day` field (and so
/// deserializes it via `#[serde(default)]`, save.rs's own
/// `missing_time_of_day_field_in_old_saves_defaults_to_dawn` covers that
/// deserialization directly) must still resume at literal dawn, unaffected.
/// The two only converge on the same fallback value by coincidence today
/// (both currently land on `WorldData::default()`), so this guards the
/// distinction `world_data_exists` exists to draw rather than trusting
/// "no code path changed that" - a save with real content (edits, a player
/// position) simply has no `NEW_WORLD_START_TIME` to apply.
#[test]
fn an_existing_save_still_resumes_at_literal_dawn_not_a_bright_morning() {
    let temp = temp_saves();
    let app = headless_app(&temp);
    // Confirm the premise a genuinely new world gets the bright-morning
    // start, before manufacturing the "already has a save" scenario below.
    assert_eq!(app.world().resource::<DayNightClock>().elapsed, craftmjne::sky::NEW_WORLD_START_TIME);

    // Simulate this world having already been saved once (whatever the
    // reason data.json exists - an old pre-day/night save is the case that
    // motivates this, but any existing save qualifies).
    let slug = app.world().resource::<ActiveWorld>().slug.clone();
    app.world().resource::<SaveStore>().save_data(&slug, &craftmjne::save::WorldData::default()).unwrap();

    let app2 = reload_app(&temp);
    assert_eq!(
        app2.world().resource::<DayNightClock>().elapsed,
        0.0,
        "an existing save must resume at literal dawn, not get the new-world override"
    );
}

/// Every fluid cell's exact state (id *and* level) is saved and restored
/// verbatim - not just the source a player placed, but every cell it spread
/// into (the simulation's own writes, never routed through `BlockSetEvent` -
/// see `world.rs`'s `set_fluid_cell` doc comment). So a reload must bring
/// the whole spread back with zero re-simulation, not just the lone source.
#[test]
fn water_restores_exactly_after_leaving_and_reentering_a_world() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    assert!(run_until(
        &mut app,
        |app| app.world().resource::<ChunkMap>().stats().1 >= 9,
        2000,
    ));

    let water = app.world().resource::<BlockRegistry>().id("water");

    // Records every write as a real saved edit (set_block + the same
    // BlockSetEvent interact.rs fires), so both the pocket we clear and the
    // source we place reproduce identically after a reload regardless of
    // what this seed's terrain naturally put there.
    let edit = |app: &mut App, pos: IVec3, id: u16| {
        let prev = {
            let mut map = app.world_mut().resource_mut::<ChunkMap>();
            map.set_block(pos, id)
        };
        if let Some(prev) = prev {
            app.world_mut().send_event(BlockSetEvent { pos, id, prev, axis: AXIS_Y });
        }
    };

    let surface = {
        let map = app.world().resource::<ChunkMap>();
        let tables = app.world().resource::<craftmjne::blocks::BlockTables>().clone();
        map.surface_y(&tables.0, 8, 8).unwrap()
    };
    let src = IVec3::new(8, surface + 1, 8);
    let neighbour = IVec3::new(9, surface + 1, 8);
    for dz in -1..=1 {
        for dx in -1..=1 {
            edit(&mut app, IVec3::new(8 + dx, surface + 1, 8 + dz), 0);
        }
    }
    edit(&mut app, src, water);

    // Let it spread sideways into its neighbour before saving.
    assert!(
        run_until(&mut app, |app| app.world().resource::<ChunkMap>().get_block(neighbour) == water, 500),
        "water never spread to its neighbour before saving"
    );

    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update(); // exit_world writes the save

    let mut app2 = reload_app(&temp);

    // Both the source and the neighbour it spread into come back the moment
    // their chunk generates - not "eventually, once the fluid queue works
    // it out again," but restored outright, so a small budget of iterations
    // (rather than the generous ones other tests use to allow for gradual
    // simulation) is enough to prove this isn't quietly still re-deriving.
    assert!(run_until(&mut app2, |app| app.world().resource::<ChunkMap>().get_block(src) == water, 200));
    assert!(
        run_until(&mut app2, |app| app.world().resource::<ChunkMap>().get_block(neighbour) == water, 200),
        "water didn't restore exactly to its neighbour after reload"
    );
}

/// `EditLog` used to reset to empty on every `enter_world` and `write_save`
/// only ever serialized *it* - so an edit whose chunk the player didn't
/// happen to revisit this session (its data lived only in `PendingEdits`,
/// which nothing serializes) would vanish the moment the *next* autosave or
/// exit overwrote the save file with "this session's edits" alone. Two
/// reload cycles are exactly what's needed to catch this: one to load the
/// old edit without revisiting it, one more to prove it actually survived
/// the save that happened in between.
#[test]
fn edits_in_unvisited_chunks_survive_a_second_reload() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);

    // Move out to a far chunk, let it stream in, and edit it there.
    let far = IVec3::new(400, 40, 400);
    let far_coord = IVec2::new(far.x.div_euclid(CHUNK_SIZE), far.z.div_euclid(CHUNK_SIZE));
    {
        let mut players = app.world_mut().query::<&mut Player>();
        let mut player = players.single_mut(app.world_mut()).unwrap();
        player.pos = Vec3::new(far.x as f32 + 0.5, far.y as f32, far.z as f32 + 0.5);
        player.spawned = true;
    }
    assert!(run_until(
        &mut app,
        |app| {
            app.world().resource::<ChunkMap>().chunks.get(&far_coord).is_some_and(|c| c.blocks.is_some())
        },
        2000,
    ));

    let glass = app.world().resource::<BlockRegistry>().id("glass");
    {
        let prev = app.world_mut().resource_mut::<ChunkMap>().set_block(far, glass).expect("chunk loaded");
        app.world_mut().send_event(BlockSetEvent { pos: far, id: glass, prev, axis: AXIS_Y });
    }
    app.update(); // let record_edits pick up the event

    // Head back toward spawn before leaving, so the saved player position -
    // and thus session 2's streaming radius - never comes near `far` again.
    {
        let mut players = app.world_mut().query::<&mut Player>();
        let mut player = players.single_mut(app.world_mut()).unwrap();
        player.pos = Vec3::new(8.5, 40.0, 8.5);
    }
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update(); // exit_world: session 1's save includes the `far` edit

    // Session 2: reload near spawn, touch nothing near `far`, leave again.
    // This is the save that used to silently drop the `far` edit.
    let mut app2 = reload_app(&temp);
    for _ in 0..5 {
        app2.update();
    }
    app2.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app2.update();

    // Session 3: reload again and go back to `far` - the edit from session 1
    // must still be there even though session 2 never touched that chunk.
    let mut app3 = reload_app(&temp);
    {
        let mut players = app3.world_mut().query::<&mut Player>();
        let mut player = players.single_mut(app3.world_mut()).unwrap();
        player.pos = Vec3::new(far.x as f32 + 0.5, far.y as f32, far.z as f32 + 0.5);
        player.spawned = true;
    }
    assert!(
        run_until(&mut app3, |app| app.world().resource::<ChunkMap>().get_block(far) == glass, 2000),
        "edit in a chunk unvisited during session 2 was lost after session 2's save"
    );
}

/// Runs the app until the chunks around spawn are meshed *and* the lighting
/// queue has fully settled, so light values can be asserted exactly.
fn run_until_lit(app: &mut App) {
    let ok = run_until(
        app,
        |app| {
            app.world().resource::<ChunkMap>().stats().1 >= 9
                && app.world().resource::<LightQueue>().is_empty()
        },
        4000,
    );
    assert!(ok, "chunks never finished meshing with settled lighting");
}

fn surface_y(app: &App, x: i32, z: i32) -> i32 {
    let tables = app.world().resource::<craftmjne::blocks::BlockTables>().clone();
    app.world()
        .resource::<ChunkMap>()
        .surface_y(&tables.0, x, z)
        .expect("column generated")
}

/// A column whose cell just above the ground is genuinely *air*, returned as
/// that cell's position.
///
/// `ChunkMap::surface_y` finds the topmost **solid** block, and water isn't
/// solid - so on a lake column the cell above the "surface" is water, not
/// air. That's fine for tests that only care about the ground, but a test
/// about open sky or about a torch in air gets a very different (and
/// correct) answer there: water attenuates light per channel, so the cell
/// reads as tinted and dimmed rather than fully lit. Scan for a dry column
/// instead of assuming a hardcoded one is dry, since where the generator
/// puts lakes is not this test's business.
fn open_air_above_ground(app: &App) -> IVec3 {
    open_air_run(app, 0)
}

/// Like `open_air_above_ground`, but with `len` more cells of air running
/// east from it - for a test that watches light spread sideways. On sloping
/// ground the cells a few blocks east of the first dry column can be inside
/// the hillside, which is dark however correct the lighting is.
fn open_air_run(app: &App, len: i32) -> IVec3 {
    let map = app.world().resource::<ChunkMap>();
    for z in 0..CHUNK_SIZE {
        for x in 0..CHUNK_SIZE - len {
            let y = surface_y(app, x, z);
            let pos = IVec3::new(x, y + 1, z);
            if (0..=len).all(|dx| map.get_block(pos + IVec3::X * dx) == craftmjne::blocks::AIR) {
                return pos;
            }
        }
    }
    panic!("no dry column with {len} blocks of open air east of it in the spawn chunk");
}

#[test]
fn sky_light_fills_open_air_and_a_roof_cuts_it_off() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    run_until_lit(&mut app);

    let open = open_air_above_ground(&app);
    assert_eq!(
        app.world().resource::<ChunkMap>().get_light(open).sky,
        [MAX_LIGHT; 3],
        "open air above the ground should see the full sky"
    );

    // Roof it over two blocks up. The cell underneath can no longer see the
    // sky directly, so it must end up dimmer than full daylight.
    let roof = open + IVec3::Y;
    let stone = app.world().resource::<BlockRegistry>().id("stone");
    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        map.set_block(roof, stone);
    }
    app.world_mut().send_event(BlockSetEvent { pos: roof, id: stone, prev: 0, axis: AXIS_Y });
    run_until_lit(&mut app);

    let under_roof = app.world().resource::<ChunkMap>().get_light(open).sky;
    assert!(
        under_roof.iter().all(|&c| c < MAX_LIGHT),
        "a roof should cut the sky light underneath it, still {under_roof:?}"
    );
}

#[test]
fn a_placed_torch_lights_the_cells_around_it_in_its_own_color() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    run_until_lit(&mut app);

    let torch_pos = open_air_run(&app, 4);
    let torch = app.world().resource::<BlockRegistry>().id("torch");
    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        map.set_block(torch_pos, torch);
    }
    app.world_mut().send_event(BlockSetEvent { pos: torch_pos, id: torch, prev: 0, axis: AXIS_Y });
    run_until_lit(&mut app);

    let map = app.world().resource::<ChunkMap>();
    let beside = map.get_light(torch_pos + IVec3::X).block;
    assert_eq!(beside[0], MAX_LIGHT - LEVEL_STEP, "one block from a full-strength torch");
    assert!(
        beside[0] > beside[2],
        "the torch's light should stay warm as it spreads, got {beside:?}"
    );
    // ...and fade with distance rather than filling the whole area evenly.
    let further = map.get_light(torch_pos + IVec3::X * 4).block;
    assert!(further[0] < beside[0] && further[0] > 0, "expected falloff, got {further:?}");
}

#[test]
fn breaking_a_torch_takes_its_light_back_out_of_the_world() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    run_until_lit(&mut app);

    let torch_pos = open_air_run(&app, 3);
    let probe = torch_pos + IVec3::X * 3;
    let torch = app.world().resource::<BlockRegistry>().id("torch");
    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        map.set_block(torch_pos, torch);
    }
    app.world_mut().send_event(BlockSetEvent { pos: torch_pos, id: torch, prev: 0, axis: AXIS_Y });
    run_until_lit(&mut app);
    assert!(app.world().resource::<ChunkMap>().get_light(probe).block[0] > 0);

    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        map.set_block(torch_pos, 0);
    }
    app.world_mut().send_event(BlockSetEvent { pos: torch_pos, id: 0, prev: torch, axis: AXIS_Y });
    run_until_lit(&mut app);

    assert_eq!(
        app.world().resource::<ChunkMap>().get_light(probe).block,
        [0; 3],
        "removing the only light source must leave nothing behind"
    );
}

/// Chunks a world has generated are saved, and loaded back instead of
/// regenerated - so a chunk keeps whatever terrain it was first made with.
/// Stands in for "the generator changed" by replacing spawn's snapshot with
/// one made by a different seed's generator, then checking a reload shows
/// exactly that terrain rather than what seed 7 would generate.
#[test]
fn a_reload_shows_saved_chunk_snapshots_not_freshly_generated_terrain() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    let origin = IVec2::ZERO;
    assert!(run_until(
        &mut app,
        |app| app.world().resource::<ChunkMap>().chunks.get(&origin).is_some_and(|c| c.blocks.is_some()),
        2000,
    ));
    let slug = app.world().resource::<ActiveWorld>().slug.clone();
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update();

    let chunks = app.world().resource::<SaveStore>().chunks_dir(&slug);
    assert!(chunks.join("c.0.0.bin").is_file(), "generated chunks should be saved");
    let registry = app.world().resource::<BlockRegistry>();
    let other_gen = TerrainGenerator::new(99, registry);
    std::fs::remove_file(chunks.join("c.0.0.bin")).unwrap();
    let replaced = ChunkStore::new(Some(chunks.clone()), registry).load_or_generate(&other_gen, 0, 0).blocks;
    assert!(replaced != TerrainGenerator::new(7, registry).generate(0, 0).blocks);

    let mut app2 = reload_app(&temp);
    assert!(run_until(
        &mut app2,
        |app| app.world().resource::<ChunkMap>().chunks.get(&origin).is_some_and(|c| c.blocks.is_some()),
        2000,
    ));
    let loaded = app2.world().resource::<ChunkMap>().chunks[&origin].blocks.clone().unwrap();
    assert!(loaded == replaced);
}

/// A world saved before chunk snapshots existed has its fluid saved over
/// the *old* terrain - every ocean and river cell. Once that terrain is
/// generated anew, putting the old water back would leave seas hanging over
/// the new land, so fluid only goes back onto a chunk restored from its
/// snapshot.
#[test]
fn fluid_saved_over_since_regenerated_terrain_is_not_put_back() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    let origin = IVec2::ZERO;
    assert!(run_until(
        &mut app,
        |app| app.world().resource::<ChunkMap>().chunks.get(&origin).is_some_and(|c| c.blocks.is_some()),
        2000,
    ));
    let slug = app.world().resource::<ActiveWorld>().slug.clone();
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update();

    // Turn it into a pre-snapshot world: no chunks saved, and fluid saved
    // where only the old terrain had water - here, high in the air.
    let store = app.world().resource::<SaveStore>();
    std::fs::remove_dir_all(store.chunks_dir(&slug)).unwrap();
    let mut data = store.load_data(&slug);
    let floating = IVec3::new(4, 60, 4);
    data.fluids.push(FluidCell { x: floating.x, y: floating.y, z: floating.z, block: "water".into(), level: 3 });
    store.save_data(&slug, &data).unwrap();

    let mut app2 = reload_app(&temp);
    assert!(run_until(
        &mut app2,
        |app| app.world().resource::<ChunkMap>().chunks.get(&origin).is_some_and(|c| c.blocks.is_some()),
        2000,
    ));
    assert_eq!(app2.world().resource::<ChunkMap>().get_block(floating), 0);
}

/// Sunlight under the sea has to be what the light propagation itself would
/// settle to, everywhere - not only in the cells it happened to revisit.
/// The generator's straight-down sky fill used to pass full sunlight
/// through water, propagation dimmed it, and propagation only revisits a
/// chunk's border cells: every chunk edge showed as a dark line across the
/// sea floor. Checks every water cell of a settled ocean chunk is a fixed
/// point of `recompute_light_cell`.
#[test]
fn sunlight_under_the_sea_has_no_seams_at_chunk_borders() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);

    // Somewhere properly at sea, found from the generator rather than
    // hardcoded, so terrain tuning can't quietly move it onto land.
    let gen = TerrainGenerator::new(7, app.world().resource::<BlockRegistry>());
    let sea = (0..4000)
        .step_by(16)
        .flat_map(|r| [(r, 0), (-r, 0), (0, r), (0, -r)])
        .map(|(x, z)| IVec2::new(x / CHUNK_SIZE, z / CHUNK_SIZE))
        .find(|c| {
            (0..CHUNK_SIZE).all(|i| {
                let col = gen.column_profile(c.x * CHUNK_SIZE + i, c.y * CHUNK_SIZE + i);
                col.water_top.is_some() && col.height < 20
            })
        })
        .expect("no open sea found");
    {
        let mut players = app.world_mut().query::<&mut Player>();
        let mut player = players.single_mut(app.world_mut()).unwrap();
        player.pos = Vec3::new((sea.x * CHUNK_SIZE + 8) as f32, 40.0, (sea.y * CHUNK_SIZE + 8) as f32);
        player.spawned = true;
        player.fly = true;
    }
    assert!(run_until(
        &mut app,
        |app| {
            let map = app.world().resource::<ChunkMap>();
            (-1..=1).all(|dz| {
                (-1..=1).all(|dx| map.chunks.get(&(sea + IVec2::new(dx, dz))).is_some_and(|c| c.meshed))
            }) && app.world().resource::<LightQueue>().is_empty()
        },
        4000,
    ));

    let tables = app.world().resource::<craftmjne::blocks::BlockTables>().0.clone();
    let water = app.world().resource::<BlockRegistry>().id("water");

    // In open sea, what the generator fills in straight down is already the
    // final answer - nothing sideways can beat it. If it isn't, the
    // propagation has to fix every cell by cascading in from the chunk's
    // borders, and until it gets there the borders show as lines.
    let generated = gen.generate(sea.x, sea.y);
    {
        let map = app.world().resource::<ChunkMap>();
        let settled = map.chunks[&sea].light.as_ref().unwrap();
        let blocks = map.chunks[&sea].blocks.as_ref().unwrap();
        let mismatched = (0..blocks.len())
            .filter(|&i| blocks[i] == water && generated.light[i].sky != settled[i].sky)
            .count();
        assert_eq!(mismatched, 0, "the generator's sky fill disagrees with settled light under the sea");
    }

    let mut wrong = Vec::new();
    app.world_mut().resource_scope(|_, mut map: Mut<ChunkMap>| {
        let (mut queue, mut touched) = (Default::default(), Default::default());
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                for y in 0..40 {
                    let pos = IVec3::new(sea.x * CHUNK_SIZE + x, y, sea.y * CHUNK_SIZE + z);
                    if map.get_block(pos) != water {
                        continue;
                    }
                    let before = map.get_light(pos);
                    craftmjne::light::recompute_light_cell(&mut map, &tables, pos, &mut queue, &mut touched);
                    if map.get_light(pos) != before {
                        wrong.push((pos, before.sky, map.get_light(pos).sky));
                    }
                }
            }
        }
    });
    assert!(wrong.is_empty(), "{} water cells not settled, e.g. {:?}", wrong.len(), &wrong[..wrong.len().min(3)]);
}

/// Mining straight down opens a shaft to the sky: sunlight has to follow
/// the hole all the way down as each block is broken.
#[test]
fn digging_a_shaft_lets_sunlight_down_it() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    run_until_lit(&mut app);

    let top = open_air_above_ground(&app) - IVec3::Y;
    for depth in 0..4 {
        let pos = top - IVec3::Y * depth;
        let prev = app.world_mut().resource_mut::<ChunkMap>().set_block(pos, 0).expect("chunk loaded");
        app.world_mut().send_event(BlockSetEvent { pos, id: 0, prev, axis: AXIS_Y });
        app.update();
    }
    run_until_lit(&mut app);
    let map = app.world().resource::<ChunkMap>();
    for depth in 0..4 {
        let pos = top - IVec3::Y * depth;
        assert_eq!(map.get_light(pos).sky, [MAX_LIGHT; 3], "shaft cell {depth} deep isn't sunlit");
    }
}

/// Mining while the world around is still loading: the hole's light can't
/// wait behind every streaming chunk's light seeding. Digs as soon as the
/// spawn chunk exists, with lots of background light work still queued,
/// and gives the edit only a few frames.
#[test]
fn a_mined_hole_lights_up_even_while_chunks_are_still_loading() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    assert!(run_until(
        &mut app,
        |app| {
            let map = app.world().resource::<ChunkMap>();
            map.chunks.get(&IVec2::ZERO).is_some_and(|c| c.blocks.is_some()) && map.stats().0 >= 12
        },
        2000,
    ));
    // Pile on background work, like a dozen chunks landing at once would:
    // every cell of every loaded chunk, far more than a few frames can get
    // through. (Re-seeding chunks used to be enough, but seeding now skips
    // cells that can't change, so it barely queues anything.)
    {
        let coords: Vec<IVec2> = app.world().resource::<ChunkMap>().chunks.keys().copied().collect();
        let mut lights = app.world_mut().resource_mut::<LightQueue>();
        for coord in coords {
            for z in 0..CHUNK_SIZE {
                for x in 0..CHUNK_SIZE {
                    for y in 0..craftmjne::config::WORLD_HEIGHT {
                        lights.push(IVec3::new(coord.x * CHUNK_SIZE + x, y, coord.y * CHUNK_SIZE + z));
                    }
                }
            }
        }
    }
    let top = open_air_above_ground(&app) - IVec3::Y;
    let prev = app.world_mut().resource_mut::<ChunkMap>().set_block(top, 0).unwrap();
    app.world_mut().send_event(BlockSetEvent { pos: top, id: 0, prev, axis: AXIS_Y });
    for _ in 0..3 {
        app.update();
    }
    assert!(!app.world().resource::<LightQueue>().is_empty(), "test setup: background work should remain");
    assert_eq!(app.world().resource::<ChunkMap>().get_light(top).sky, [MAX_LIGHT; 3]);
}

/// Streaming a world in while moving must not build a light backlog. A
/// chunk landing used to queue every cell whose light wasn't full sky (all
/// water) and every border cell, whether or not anything could change
/// them; the queue grew to ~870k cells while flying at render distance 8,
/// lighting lagged far behind the terrain, and growing the queue itself
/// stalled frames.
#[test]
fn streaming_while_moving_keeps_the_light_backlog_small() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    app.world_mut().resource_mut::<WorldSettings>().render_distance = 6;
    let mut most = 0;
    for frame in 0..600 {
        {
            let mut players = app.world_mut().query::<&mut Player>();
            let mut player = players.single_mut(app.world_mut()).unwrap();
            player.pos = Vec3::new(frame as f32 * 0.4, 50.0, 0.0);
            player.spawned = true;
            player.fly = true;
        }
        app.update();
        most = most.max(app.world().resource::<LightQueue>().len());
        std::thread::sleep(Duration::from_millis(2));
    }
    let generated = app.world().resource::<ChunkMap>().stats().0;
    assert!(generated > 150, "test setup: only {generated} chunks streamed in");
    assert!(most < 40_000, "the light backlog reached {most} cells");
}

/// Far-away chunks are dropped from memory and come back exactly as they
/// were - their edits *and* fluid state - whether you walk back to them or
/// save and reload while they're unloaded.
#[test]
fn chunks_unloaded_far_away_come_back_exactly_as_they_were() {
    let temp = temp_saves();
    let mut app = headless_app(&temp);
    assert!(run_until(&mut app, |app| app.world().resource::<ChunkMap>().stats().1 >= 9, 2000));

    let (stone, water) = {
        let reg = app.world().resource::<BlockRegistry>();
        (reg.id("stone"), reg.id("water"))
    };
    // A player edit (through the edit log)...
    let edited = open_air_above_ground(&app);
    let prev = app.world_mut().resource_mut::<ChunkMap>().set_block(edited, stone).unwrap();
    app.world_mut().send_event(BlockSetEvent { pos: edited, id: stone, prev, axis: AXIS_Y });
    // ...and a fluid cell no edit records - a flowing level, as the fluid
    // sim would leave - which only the fluid save can bring back.
    let wet = open_air_run(&app, 2) + IVec3::X * 2;
    {
        let mut map = app.world_mut().resource_mut::<ChunkMap>();
        map.set_block(wet, water);
        map.set_fluid_level_raw(wet, 3);
    }
    app.update();
    let spawn = IVec2::ZERO;
    let capture = |app: &App| {
        let chunk = &app.world().resource::<ChunkMap>().chunks[&spawn];
        (chunk.blocks.clone().unwrap(), chunk.fluid_level.clone().unwrap())
    };
    let before = capture(&app);
    assert_eq!(before.0[craftmjne::config::block_index(
        wet.x as usize, wet.y as usize, wet.z as usize)], water);

    let fly_to = |app: &mut App, x: f32| {
        let mut players = app.world_mut().query::<&mut Player>();
        let mut player = players.single_mut(app.world_mut()).unwrap();
        player.pos = Vec3::new(x, 200.0, 8.0);
        player.spawned = true;
        player.fly = true;
    };
    let gone = |app: &mut App| !app.world().resource::<ChunkMap>().chunks.contains_key(&spawn);
    let back = |app: &mut App| {
        app.world().resource::<ChunkMap>().chunks.get(&spawn).is_some_and(|c| c.blocks.is_some())
    };

    // Away: the spawn chunk unloads, and memory holds only what's near.
    fly_to(&mut app, 16.0 * 20.0);
    assert!(run_until(&mut app, gone, 2000), "the spawn chunk never unloaded");
    let r = app.world().resource::<WorldSettings>().render_distance + craftmjne::world::UNLOAD_MARGIN;
    let loaded = app.world().resource::<ChunkMap>().chunks.len() as i32;
    assert!(loaded <= (2 * r + 1) * (2 * r + 1), "{loaded} chunks still loaded far from most of them");

    // Back again: exactly as it was.
    fly_to(&mut app, 8.0);
    assert!(run_until(&mut app, back, 2000), "the spawn chunk never came back");
    let after = capture(&app);
    assert!(after.0 == before.0, "blocks changed after unloading and reloading the chunk");
    assert!(after.1 == before.1, "fluid levels changed after unloading and reloading the chunk");

    // Away again, and save and reload while it's unloaded.
    fly_to(&mut app, 16.0 * 20.0);
    assert!(run_until(&mut app, gone, 2000));
    app.world_mut().resource_mut::<NextState<AppState>>().set(AppState::MainMenu);
    app.update();
    let mut app2 = reload_app(&temp);
    fly_to(&mut app2, 8.0);
    assert!(run_until(&mut app2, back, 2000));
    let reloaded = capture(&app2);
    assert!(reloaded.0 == before.0, "blocks changed after a save made while the chunk was unloaded");
    assert!(reloaded.1 == before.1, "fluid changed after a save made while the chunk was unloaded");
}
