//! Craftmjne — a well-optimized, extensible voxel game framework on Bevy.
//!
//! Framework usage: everything is a Bevy plugin. Add your own plugins next to
//! the built-in ones; register blocks/painters from your plugin's `build()`
//! (see README "Extending the framework").
//!
//! CLI: `craftmjne [--seed N] [--render-distance N] [--no-update-check] [--version]`

use bevy::prelude::*;

use craftmjne::config::WorldSettings;
use craftmjne::updater::UpdateCheckEnabled;
use craftmjne::{interact, player, render, ui, updater, world};

struct Args {
    settings: WorldSettings,
    update_check: bool,
}

fn parse_args() -> Args {
    let mut settings = WorldSettings::default();
    let mut update_check = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seed" | "-s" => {
                if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                    settings.seed = v;
                }
            }
            "--render-distance" | "-r" => {
                if let Some(v) = args.next().and_then(|v| v.parse().ok()) {
                    settings.render_distance = v;
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
    Args { settings, update_check }
}

fn main() {
    let args = parse_args();
    let mut app = App::new();
    app.insert_resource(args.settings)
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
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            world::WorldPlugin,
            render::RenderSetupPlugin,
            player::PlayerPlugin,
            interact::InteractPlugin,
            ui::UiPlugin,
            updater::UpdaterPlugin,
        ));

    // CI / smoke mode: boot the real game, let the world stream in, save a
    // screenshot and exit. Run with: CRAFT_SMOKE=1 cargo run
    if std::env::var_os("CRAFT_SMOKE").is_some() {
        app.add_systems(Update, smoke_test);
    }

    app.run();
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
