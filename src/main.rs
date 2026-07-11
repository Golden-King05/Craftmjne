//! Craftmjne — a well-optimized, extensible voxel game framework on Bevy.
//!
//! Framework usage: everything is a Bevy plugin. Add your own plugins next to
//! the built-in ones; register blocks/painters from your plugin's `build()`
//! (see README "Extending the framework").
//!
//! CLI: `craftmjne [--seed N] [--render-distance N] [--no-update-check] [--version]`

use bevy::prelude::*;

use craftmjne::config::WorldSettings;
use craftmjne::save::{GameMode, SaveStore};
use craftmjne::state::{ActiveWorld, AppState};
use craftmjne::updater::UpdateCheckEnabled;
use craftmjne::{chat, interact, inventory, menu, player, render, ui, updater, world};

struct Args {
    seed: u32,
    render_distance: Option<i32>, // CLI override; None means "use the saved graphics setting"
    update_check: bool,
}

fn parse_args() -> Args {
    let mut seed = WorldSettings::default().seed;
    let mut render_distance = None;
    let mut update_check = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" | "-s" => {
                if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                    seed = v;
                }
            }
            "--render-distance" | "-r" => {
                if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                    render_distance = Some(v);
                }
            }
            "--no-update-check" => update_check = false,
            "--version" | "-V" => {
                println!("craftmjne {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => eprintln!("unknown argument: {other}"),
        }
    }
    Args { seed, render_distance, update_check }
}

fn main() {
    let args = parse_args();
    let smoke = std::env::var_os("CRAFT_SMOKE").is_some();

    // CI / smoke mode: redirect saves to a throwaway directory so automated
    // runs never touch (or depend on) a real user profile.
    let store = if smoke {
        let dir = std::env::temp_dir().join(format!("craftmjne-smoke-{}", std::process::id()));
        SaveStore::at(dir)
    } else {
        SaveStore::default()
    };

    let graphics = store.load_graphics_settings();
    let settings = WorldSettings {
        seed: args.seed,
        render_distance: args.render_distance.unwrap_or(graphics.render_distance),
    };

    let mut app = App::new();
    app.insert_resource(settings)
        .insert_resource(store)
        .insert_resource(UpdateCheckEnabled(args.update_check))
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Craftmjne".into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .init_state::<AppState>()
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            world::WorldPlugin,
            render::RenderSetupPlugin,
            player::PlayerPlugin,
            interact::InteractPlugin,
            inventory::InventoryPlugin,
            chat::ChatPlugin,
            ui::UiPlugin,
            updater::UpdaterPlugin,
            menu::MenuPlugin,
        ));

    // CI / smoke mode: skip the menu, drop straight into a throwaway world,
    // let it stream in, save a screenshot, and exit. Run with:
    // CRAFT_SMOKE=1 cargo run -- --seed 7
    if smoke {
        app.add_systems(Update, enter_smoke_world.run_if(in_state(AppState::MainMenu)))
            .add_systems(Update, smoke_test);
    }

    app.run();
}

/// Runs only while still in `MainMenu`, so it fires exactly once: creates a
/// throwaway world and jumps straight into it, skipping the menu entirely.
fn enter_smoke_world(
    mut commands: Commands,
    settings: Res<WorldSettings>,
    store: Res<SaveStore>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    let (slug, meta) = store
        .create_world("smoke-test", settings.seed, GameMode::default())
        .expect("create smoke-test world");
    commands.insert_resource(ActiveWorld { slug, meta });
    next_state.set(AppState::InGame);
}

fn smoke_test(
    mut commands: Commands,
    time: Res<Time>,
    mut phase: Local<u32>,
    mut exit: EventWriter<AppExit>,
    map: Res<craftmjne::world::ChunkMap>,
) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let t = time.elapsed_secs();
    if *phase == 0 && t > 20.0 {
        let (generated, meshed) = map.stats();
        info!("smoke: {generated} chunks generated, {meshed} meshed; capturing smoke.png");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk("smoke.png"));
        *phase = 1;
    } else if *phase == 1 && t > 24.0 {
        info!("smoke: done");
        exit.write(AppExit::Success);
    }
}
