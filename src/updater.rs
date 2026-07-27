//! Background auto-updater. On startup, checks GitHub Releases for a newer
//! version and, if one exists, downloads and stages the new binary in a
//! scratch directory — the running process keeps playing on the current
//! version, same as before.
//!
//! Unlike a naive implementation, staging is *all* that happens in the
//! background: the actual on-disk swap (this process rewriting its own
//! executable file, via the `self_replace` crate) is deliberately deferred
//! until the game is actually closing (see `gate_quit`/`apply_update_then_exit`
//! below), so there's a real, visible "Updating..." step right before the
//! window disappears, instead of the risky, disruptive part happening
//! silently sometime mid-session with nothing to show for it but a small
//! banner - which left no way to tell whether it had actually taken effect,
//! and gave antivirus/EDR heuristics (a running process silently rewriting
//! its own binary is exactly what they're built to flag) a wide, unwatched
//! window to intervene in.
//!
//! CI (`.github/workflows/release.yml`) publishes one archive per platform on
//! every `v*` tag, named `craftmjne-<target-triple>.<zip|tar.gz>` — that
//! naming is what `asset_for` matches against the running binary's target
//! triple, so keep the two in sync if you add platforms.
//!
//! Disable with `--no-update-check` or the `CRAFTMJNE_NO_UPDATE_CHECK` env
//! var (also auto-disabled under `CRAFT_SMOKE`, so CI screenshots don't
//! depend on network access).

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowCloseRequested};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const REPO_OWNER: &str = "golden-king05";
pub const REPO_NAME: &str = "craftmjne";
const BIN_NAME: &str = "craftmjne";

/// How long the "Updating..." overlay stays up once the swap's been
/// attempted before the process actually exits - long enough to register as
/// a real, deliberate step rather than an imperceptible flash, short enough
/// not to feel like a hang. The swap itself (a couple of same-directory
/// renames plus one copy, see `self_replace::self_replace`) finishes in well
/// under this either way.
const MIN_APPLY_VISIBLE: Duration = Duration::from_millis(700);
/// A failure is worth actually reading before the window vanishes, unlike
/// the "it worked" case which has nothing to act on.
const MIN_APPLY_VISIBLE_FAILED: Duration = Duration::from_millis(2500);

#[derive(Resource, Clone)]
pub enum UpdateState {
    Disabled,
    Checking,
    UpToDate,
    /// Downloaded and staged on disk, not yet swapped in - `apply_update_on_exit`
    /// performs the actual swap once the game is closing (see `gate_quit`).
    Ready { version: String, staged_exe: PathBuf },
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

/// Fire this instead of writing `AppExit` directly from UI/input code (the
/// in-game Quit button, a future confirm-to-quit dialog, etc.) - it's what
/// lets a staged update actually get applied before the process exits,
/// instead of being silently abandoned. See `gate_quit`.
#[derive(Event, Default)]
pub struct QuitRequested;

/// `Receiver` is not `Sync`; a `Mutex` gives the wrapper the `Sync` bound that
/// `Resource` requires (only the spawning thread and this single-threaded
/// polling system ever touch it, so the lock is uncontended).
#[derive(Resource)]
struct UpdateChannel(Mutex<Receiver<UpdateState>>);

/// Present only while the game is closing with a staged update to apply -
/// its existence is what the UI banner and `apply_update_then_exit` key off
/// of. `staged_exe` is `Some` until the swap's been attempted once (cleared
/// right after, regardless of outcome, so it only ever runs once);
/// `outcome` is `None` until then, then holds what happened for `ui.rs`'s
/// banner to display during the final visible dwell before exit.
#[derive(Resource)]
pub(crate) struct PendingExit {
    staged_exe: Option<PathBuf>,
    pub(crate) outcome: Option<Result<(), String>>,
    started: Instant,
}

fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("craftmjne-update")
}

fn spawn_check(mut commands: Commands, enabled: Res<UpdateCheckEnabled>) {
    if !enabled.0 {
        commands.insert_resource(UpdateState::Disabled);
        return;
    }
    commands.insert_resource(UpdateState::Checking);
    let (tx, rx) = channel();
    commands.insert_resource(UpdateChannel(Mutex::new(rx)));

    std::thread::spawn(move || {
        let _ = tx.send(check_and_stage());
    });
}

/// Checks GitHub Releases for a newer version and, if one exists, downloads
/// and extracts just the binary into `staging_dir()` - deliberately stops
/// short of `self_replace`'ing it in, unlike calling `self_update`'s own
/// high-level `Update::update()` would (see the module docs for why).
fn check_and_stage() -> UpdateState {
    let current_version = env!("CARGO_PKG_VERSION");
    let releases = match self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .and_then(|list| list.fetch())
    {
        Ok(releases) => releases,
        Err(err) => return UpdateState::Failed(err.to_string()),
    };

    let target = self_update::get_target();
    // GitHub's `/releases` listing is newest-published first; the first
    // release that's actually newer than us is the one to take, matching
    // `self_update`'s own internal `get_latest_releases` behavior.
    let newer = releases.into_iter().find(|r| {
        self_update::version::bump_is_greater(current_version, &r.version).unwrap_or(false)
    });
    let Some(release) = newer else {
        return UpdateState::UpToDate;
    };
    let Some(asset) = release.asset_for(target, None) else {
        return UpdateState::Failed(format!("no release asset found for target {target:?}"));
    };

    match stage(&asset) {
        Ok(staged_exe) => UpdateState::Ready { version: release.version, staged_exe },
        Err(err) => UpdateState::Failed(err),
    }
}

/// Downloads `asset` and extracts just the platform binary into
/// `staging_dir()`, returning its path. Mirrors the download/extract half of
/// `self_update::update::Update::update_extended` exactly (down to the
/// `Accept` header GitHub's asset API endpoint needs to return raw bytes
/// instead of JSON) - only the final `self_replace` call is left out.
fn stage(asset: &self_update::update::ReleaseAsset) -> Result<PathBuf, String> {
    let dir = staging_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let tmp_archive_dir = self_update::TempDir::new().map_err(|e| e.to_string())?;
    let archive_path = tmp_archive_dir.path().join(&asset.name);
    let mut file = std::fs::File::create(&archive_path).map_err(|e| e.to_string())?;

    let mut download = self_update::Download::from_url(&asset.download_url);
    download.set_header(http::header::ACCEPT, "application/octet-stream".parse().unwrap());
    download.download_to(&mut file).map_err(|e| e.to_string())?;

    let bin_path = format!("{BIN_NAME}{}", std::env::consts::EXE_SUFFIX);
    self_update::Extract::from_source(&archive_path)
        .extract_file(&dir, &bin_path)
        .map_err(|e| e.to_string())?;

    Ok(dir.join(bin_path))
}

fn poll_check(mut commands: Commands, channel: Option<Res<UpdateChannel>>) {
    let Some(channel) = channel else { return };
    let Ok(state) = channel.0.lock().unwrap().try_recv() else { return };
    match &state {
        UpdateState::Ready { version, .. } => {
            info!("staged craftmjne {version}; will apply on next quit");
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

/// The single authority over whether the game actually exits right now.
/// Both the OS window's close button and any in-game "Quit" action must
/// route through `QuitRequested` (never write `AppExit` directly) so a
/// staged update gets one chance to apply first - see `apply_update_then_exit`.
fn gate_quit(
    mut quit_events: EventReader<QuitRequested>,
    mut close_events: EventReader<WindowCloseRequested>,
    mut commands: Commands,
    windows: Query<Entity, With<PrimaryWindow>>,
    state: Res<UpdateState>,
    pending: Option<Res<PendingExit>>,
    mut exit: EventWriter<AppExit>,
) {
    let requested = quit_events.read().count() > 0 || close_events.read().count() > 0;
    if !requested || pending.is_some() {
        return;
    }
    if let UpdateState::Ready { staged_exe, .. } = &*state {
        commands.insert_resource(PendingExit {
            staged_exe: Some(staged_exe.clone()),
            outcome: None,
            started: Instant::now(),
        });
    } else {
        for window in &windows {
            commands.entity(window).despawn();
        }
        exit.write(AppExit::Success);
    }
}

/// Drives a `PendingExit` to completion: performs the deferred swap once
/// (first frame this resource exists), then keeps the "Updating..."/failure
/// banner up for a minimum dwell so it's actually readable before the
/// window despawns and the process exits for real.
fn apply_update_then_exit(
    mut commands: Commands,
    pending: Option<ResMut<PendingExit>>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut exit: EventWriter<AppExit>,
) {
    let Some(mut pending) = pending else { return };
    if let Some(staged_exe) = pending.staged_exe.take() {
        pending.outcome = Some(
            self_update::self_replace::self_replace(&staged_exe)
                .map(|()| {
                    let _ = std::fs::remove_dir_all(staging_dir());
                })
                .map_err(|err| err.to_string()),
        );
        return; // let a frame render the outcome before exiting
    }

    let min_visible = match &pending.outcome {
        Some(Err(_)) => MIN_APPLY_VISIBLE_FAILED,
        _ => MIN_APPLY_VISIBLE,
    };
    if pending.started.elapsed() < min_visible {
        return;
    }
    for window in &windows {
        commands.entity(window).despawn();
    }
    exit.write(AppExit::Success);
    commands.remove_resource::<PendingExit>();
}

pub struct UpdaterPlugin;

impl Plugin for UpdaterPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<UpdateCheckEnabled>() {
            app.insert_resource(UpdateCheckEnabled::default());
        }
        app.insert_resource(UpdateState::Checking)
            .add_event::<QuitRequested>()
            .add_systems(Startup, spawn_check)
            .add_systems(Update, (poll_check, gate_quit, apply_update_then_exit).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Just `gate_quit` on its own - deliberately never adds
    /// `apply_update_then_exit` to the schedule, since that system is the
    /// one that calls the real `self_replace::self_replace`, which would
    /// rewrite *this test binary's own executable* on disk if it ever ran
    /// here. These tests only need to verify `gate_quit`'s branching (does a
    /// staged update defer the exit, or not), never the swap itself.
    fn gate_only_app() -> App {
        let mut app = App::new();
        app.add_event::<QuitRequested>()
            .add_event::<WindowCloseRequested>()
            .add_systems(Update, gate_quit);
        app
    }

    fn fake_ready() -> UpdateState {
        UpdateState::Ready {
            version: "9.9.9".into(),
            staged_exe: PathBuf::from("/nonexistent/staged/craftmjne"),
        }
    }

    #[test]
    fn quit_with_no_staged_update_exits_immediately() {
        let mut app = gate_only_app();
        app.insert_resource(UpdateState::UpToDate);
        app.world_mut().send_event(QuitRequested);
        app.update();

        assert!(
            !app.world().contains_resource::<PendingExit>(),
            "nothing to apply - there should be no gate at all"
        );
        assert_eq!(
            app.world().resource::<Events<AppExit>>().len(),
            1,
            "expected the quit to go straight through to a real AppExit"
        );
    }

    #[test]
    fn quit_with_a_staged_update_defers_to_pending_exit_instead_of_exiting() {
        let mut app = gate_only_app();
        app.insert_resource(fake_ready());
        app.world_mut().send_event(QuitRequested);
        app.update();

        assert!(
            app.world().contains_resource::<PendingExit>(),
            "a staged update must gate the real exit instead of firing it immediately"
        );
        assert!(
            app.world().resource::<Events<AppExit>>().is_empty(),
            "must not exit yet - the swap hasn't happened"
        );
    }

    #[test]
    fn a_repeated_quit_request_does_not_reset_an_already_pending_exit() {
        let mut app = gate_only_app();
        app.insert_resource(fake_ready());
        app.world_mut().send_event(QuitRequested);
        app.update();
        let first_started = app.world().resource::<PendingExit>().started;

        app.world_mut().send_event(QuitRequested);
        app.update();

        assert_eq!(
            app.world().resource::<PendingExit>().started,
            first_started,
            "gate_quit must leave an in-progress PendingExit alone, not restart its timer"
        );
    }

    #[test]
    fn a_window_close_request_is_gated_the_same_way_as_an_in_game_quit() {
        let mut app = gate_only_app();
        app.insert_resource(fake_ready());
        app.world_mut().send_event(WindowCloseRequested { window: Entity::PLACEHOLDER });
        app.update();

        assert!(
            app.world().contains_resource::<PendingExit>(),
            "the OS close button must go through the same staged-update gate as the Quit button"
        );
    }
}
