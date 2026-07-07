//! Craftmjne — a well-optimized, extensible voxel game framework on Bevy.
//!
//! Framework usage: everything is a Bevy plugin. Add your own plugins next to
//! the built-in ones; register blocks/painters from your plugin's `build()`
//! (see README "Extending the framework").
//!
//! CLI: `craftmjne [--seed N] [--render-distance N]`

use bevy::prelude::*;

use craftmjne::config::WorldSettings;
use craftmjne::{interact, player, render, ui, world};

fn parse_args() -> WorldSettings {
    let mut settings = WorldSettings::default();
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
            other => eprintln!("unknown argument: {other}"),
        }
    }
    settings
}

fn main() {
    let mut app = App::new();
    app.insert_resource(parse_args())
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
