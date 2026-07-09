//! Background auto-updater. On startup, checks GitHub Releases for a newer
//! version and, if one exists, downloads it and swaps the on-disk binary in
//! place — the running process keeps playing on the current version; the new
//! one takes effect the next time the game is launched (same model Steam,
//! VS Code, etc. use: swap on disk, apply on restart).
//!
//! CI (`.github/workflows/release.yml`) publishes one archive per platform on
//! every `v*` tag, named `craftmjne-<target-triple>.<zip|tar.gz>` — that
//! naming is what `self_update` matches against the running binary's target
//! triple, so keep the two in sync if you add platforms.
//!
//! Disable with `--no-update-check` or the `CRAFTMJNE_NO_UPDATE_CHECK` env
//! var (also auto-disabled under `CRAFT_SMOKE`, so CI screenshots don't
//! depend on network access).

use bevy::prelude::*;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;

pub const REPO_OWNER: &str = "golden-king05";
pub const REPO_NAME: &str = "craftmjne";
const BIN_NAME: &str = "craftmjne";

#[derive(Resource, Clone)]
pub enum UpdateState {
    Disabled,
    Checking,
    UpToDate,
    /// Downloaded and swapped on disk; restart the game to run it.
    Ready { version: String },
    Failed(String),
}

#[derive(Resource)]
pub struct UpdateCheckEnabled(pub bool);

impl Default for UpdateCheckEnabled {
    fn default() -> Self {
        let disabled = std::env::var_os("CRAFTMJNE_NO_UPDATE_CHECK").is_some()
            || std::env::var_os("CRAFT_SMOKE").is_some();
        Self(!disabled)
    }
}

// `Receiver` is not `Sync`; a `Mutex` gives the wrapper the `Sync` bound that
// `Resource` requires (only the spawning thread and this single-threaded
// polling system ever touch it, so the lock is uncontended).
#[derive(Resource)]
struct UpdateChannel(Mutex<Receiver<UpdateState>>);

fn spawn_check(mut commands: Commands, enabled: Res<UpdateCheckEnabled>) {
    if !enabled.0 {
        commands.insert_resource(UpdateState::Disabled);
        return;
    }
    commands.insert_resource(UpdateState::Checking);
    let (tx, rx) = channel();
    commands.insert_resource(UpdateChannel(Mutex::new(rx)));

    std::thread::spawn(move || {
        let outcome = self_update::backends::github::Update::configure()
            .repo_owner(REPO_OWNER)
            .repo_name(REPO_NAME)
            .bin_name(BIN_NAME)
            .show_download_progress(false)
            .no_confirm(true)
            .current_version(env!("CARGO_PKG_VERSION"))
            .build()
            .and_then(|updater| updater.update());

        let state = match outcome {
            Ok(self_update::Status::UpToDate(_)) => UpdateState::UpToDate,
            Ok(self_update::Status::Updated(version)) => UpdateState::Ready { version },
            Err(err) => UpdateState::Failed(err.to_string()),
        };
        let _ = tx.send(state);
    });
}

fn poll_check(mut commands: Commands, channel: Option<Res<UpdateChannel>>) {
    let Some(channel) = channel else { return };
    let Ok(state) = channel.0.lock().unwrap().try_recv() else { return };
    match &state {
        UpdateState::Ready { version } => {
            info!("downloaded craftmjne {version}; restart the game to apply it");
        }
        UpdateState::Failed(err) => {
            // Network hiccups / rate limits / no releases yet are all normal;
            // never block or interrupt play over this.
            warn!("update check failed (playing on current version): {err}");
        }
        _ => {}
    }
    commands.insert_resource(state);
    commands.remove_resource::<UpdateChannel>();
}

pub struct UpdaterPlugin;

impl Plugin for UpdaterPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<UpdateCheckEnabled>() {
            app.insert_resource(UpdateCheckEnabled::default());
        }
        app.insert_resource(UpdateState::Checking)
            .add_systems(Startup, spawn_check)
            .add_systems(Update, poll_check);
    }
}
