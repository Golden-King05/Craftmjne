//! The launcher window.
//!
//! Two tabs: **Instances** (named, editable launch configurations) and
//! **Versions** (what's published on GitHub, and what's downloaded here).
//! All state lives in [`LauncherApp`]; every slow operation goes through
//! `jobs` so the window never stops responding.
//!
//! ## Hiding while a game is running
//!
//! `play` doesn't leave the launcher sitting open next to the game, and it
//! doesn't exit either - it hides its window (`egui::ViewportCommand::
//! Visible(false)`) and shows it again once the game process exits, so from
//! the player's side the launcher closes when the game opens and reopens
//! when the game closes, with no second window competing for taskbar space
//! in between.
//!
//! This is one running process the whole time, not two: a background thread
//! calls `Child::wait()` on the spawned game (blocking is fine off the UI
//! thread) and reports back through the same `jobs`/`JobDone` channel every
//! other background operation already uses, and `Context::request_repaint`
//! (documented as safe and expected to call from another thread, and the
//! specific reason it's cloned into the thread below) wakes the UI loop up
//! to actually process that message even while the window has no input to
//! otherwise trigger a redraw.
//!
//! Deliberately *not* implemented as "launcher process exits, then a second
//! process relaunches it once the game closes": that would need the exiting
//! launcher to hand off responsibility for the wait to some other process
//! (itself, re-invoked with an internal flag, or a detached helper), which
//! is real complexity for no benefit here, and "a process that spawns a
//! hidden copy of itself to supervise, then reappears later" is a shape
//! that reads as suspicious to exactly the antivirus heuristics this
//! project is already trying to avoid tripping (see the old in-game
//! updater's self-replace saga in CLAUDE.md). Hiding a window costs none of
//! that.

use eframe::egui;

use crate::instances::{Instance, Instances};
use crate::jobs::{Downloads, JobDone, Jobs};
use crate::launch;
use crate::library::Library;
use crate::paths::Paths;
use crate::remote::{self, RemoteVersion};
use crate::selfupdate::{self, SelfUpdate};
#[cfg(windows)]
use crate::shortcut::{self, Location};

#[derive(PartialEq, Eq, Clone, Copy)]
enum Tab {
    Instances,
    Versions,
}

enum Releases {
    Loading,
    Loaded(Vec<RemoteVersion>),
    Failed(String),
}

/// State of the rolling dev-build check (`remote::fetch_dev_build`) -
/// separate from `Releases` because it only exists once the "Enable dev
/// builds" toggle is on, and "no dev build has been published yet" is a
/// real, expected `Failed`-shaped state here rather than the "GitHub is
/// unreachable" meaning `Releases::Failed` carries.
enum DevBuildState {
    /// The toggle hasn't triggered a fetch yet - covers a saved
    /// `instances.json` that already has it enabled, so a fetch still
    /// needs to happen once, on the first frame this renders.
    Idle,
    Loading,
    Loaded(remote::DevBuild),
    Failed(String),
}

/// How often the Versions tab re-checks the dev build while the toggle is
/// on and the launcher is open, so "is this stale" stays true without
/// having to remember to click "Check again" - see `update`'s use of
/// `last_dev_check`.
const DEV_BUILD_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

pub struct LauncherApp {
    paths: Paths,
    library: Library,
    instances: Instances,
    selected: Option<usize>,
    tab: Tab,
    releases: Releases,
    dev_build: DevBuildState,
    /// When the dev build was last checked (successfully triggered, not
    /// necessarily landed yet) - `update` compares this against
    /// `DEV_BUILD_CHECK_INTERVAL` each frame to decide whether it's due for
    /// another automatic check.
    last_dev_check: std::time::Instant,
    jobs: Jobs,
    downloads: Downloads,
    /// Result of the startup launcher self-check, once it lands.
    self_update: Option<SelfUpdate>,
    status: String,
}

impl LauncherApp {
    pub fn new(paths: Paths) -> Self {
        let instances = Instances::load(&paths.instances_file());
        let jobs = Jobs::default();

        // Both startup checks go out immediately and in parallel: what
        // versions exist, and whether the launcher itself is out of date.
        jobs.spawn(|| JobDone::Releases(remote::fetch_releases()));
        let staging = paths.staging_dir();
        jobs.spawn(move || JobDone::SelfUpdate(selfupdate::check_and_apply(staging)));

        // Dev builds only get checked if the saved preference already has
        // them on - unlike the two checks above, this one is opt-in, not
        // something every launch should spend a request on.
        let dev_build = if instances.dev_builds_enabled {
            jobs.spawn(|| JobDone::DevBuild(remote::fetch_dev_build()));
            DevBuildState::Loading
        } else {
            DevBuildState::Idle
        };

        Self {
            library: Library::new(paths.clone()),
            paths,
            instances,
            selected: None,
            tab: Tab::Instances,
            releases: Releases::Loading,
            dev_build,
            last_dev_check: std::time::Instant::now(),
            jobs,
            downloads: Downloads::default(),
            self_update: None,
            status: String::new(),
        }
    }

    fn save_instances(&mut self) {
        if let Err(err) = self.instances.save(&self.paths.instances_file()) {
            self.status = format!("Couldn't save instances: {err}");
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        for done in self.jobs.drain() {
            match done {
                JobDone::Releases(Ok(list)) => self.releases = Releases::Loaded(list),
                JobDone::Releases(Err(err)) => self.releases = Releases::Failed(err),
                JobDone::Installed { version, result } => {
                    self.downloads.finish(&version);
                    self.status = match result {
                        Ok(()) => format!("Craftmjne {version} is ready to play."),
                        Err(err) => format!("Couldn't install {version}: {err}"),
                    };
                }
                JobDone::SelfUpdate(state) => {
                    if let SelfUpdate::Failed(err) = &state {
                        // Never blocking: the launcher works fine on the
                        // version you already have.
                        self.status = format!("Launcher update check failed: {err}");
                    }
                    self.self_update = Some(state);
                }
                JobDone::GameExited => {
                    self.status = "Welcome back.".to_string();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    // A no-op if the OS doesn't want to yield focus (see the
                    // command's own doc comment) - a nicety, not something
                    // reopening depends on, since Visible(true) above is
                    // what actually brings the window back at all.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                JobDone::DevBuild(Ok(Some(build))) => self.dev_build = DevBuildState::Loaded(build),
                JobDone::DevBuild(Ok(None)) => {
                    self.dev_build =
                        DevBuildState::Failed("No dev build has been published for this platform yet.".to_string());
                }
                JobDone::DevBuild(Err(err)) => self.dev_build = DevBuildState::Failed(err),
                JobDone::DevInstalled { commit, result } => {
                    self.downloads.finish(remote::DEV_VERSION_SLOT);
                    self.status = match result {
                        Ok(()) => match self.library.record_dev_commit(&commit) {
                            Ok(()) => "Dev build installed.".to_string(),
                            // The build is on disk and playable either way -
                            // only the "is it stale" comparison degrades,
                            // silently reading as "needs update" forever
                            // until a later install manages to record it.
                            Err(err) => format!("Dev build installed, but couldn't record its commit: {err}"),
                        },
                        Err(err) => format!("Couldn't install the dev build: {err}"),
                    };
                }
            }
        }
    }

    fn start_download(&mut self, version: &RemoteVersion) {
        if self.downloads.progress(&version.version).is_some() {
            return;
        }
        let progress = self.downloads.start(&version.version);
        let dest = self.paths.version_dir(&version.version);
        let url = version.download_url.clone();
        let name = version.version.clone();
        self.status = format!("Downloading Craftmjne {name}...");
        self.jobs.spawn(move || JobDone::Installed {
            result: remote::install(&url, &dest, &progress),
            version: name,
        });
    }

    fn refresh_dev_build(&mut self) {
        self.dev_build = DevBuildState::Loading;
        self.last_dev_check = std::time::Instant::now();
        self.jobs.spawn(|| JobDone::DevBuild(remote::fetch_dev_build()));
    }

    fn start_dev_download(&mut self, build: &remote::DevBuild) {
        if self.downloads.progress(remote::DEV_VERSION_SLOT).is_some() {
            return;
        }
        let progress = self.downloads.start(remote::DEV_VERSION_SLOT);
        let dest = self.paths.version_dir(remote::DEV_VERSION_SLOT);
        let url = build.download_url.clone();
        let commit = build.commit.clone();
        self.status = "Downloading the dev build...".to_string();
        self.jobs.spawn(move || JobDone::DevInstalled {
            result: remote::install(&url, &dest, &progress),
            commit,
        });
    }

    fn play(&mut self, index: usize, ctx: &egui::Context) {
        let Some(instance) = self.instances.items.get(index).cloned() else { return };
        match launch::launch(&self.library, &instance) {
            Ok(child) => {
                self.status = format!("Playing {} on Craftmjne {}.", instance.name, instance.version);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                let ctx = ctx.clone();
                self.jobs.spawn(move || {
                    // Best-effort: a `wait()` error here (already reaped,
                    // OS-level failure) still means the game is gone, so
                    // this waits it out at most rather than getting stuck
                    // with the launcher hidden forever over a wait() that
                    // can't itself be retried.
                    let mut child = child;
                    let _ = child.wait();
                    // `Context::request_repaint` is explicitly documented as
                    // safe (and expected) to call from a background thread -
                    // it's what wakes eframe's loop to actually process
                    // `JobDone::GameExited` even though the hidden window
                    // has had no input to otherwise trigger a redraw.
                    ctx.request_repaint();
                    JobDone::GameExited
                });
            }
            Err(err) => self.status = err,
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_jobs(ctx);
        // Downloads report progress from another thread, which egui has no
        // way to know about - keep repainting while any are in flight so the
        // byte counter actually moves.
        if self.downloads.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        // Re-checks the dev build on a timer while the toggle is on, so
        // "Update available" stays accurate without needing "Check again"
        // clicked by hand. `request_repaint_after` is what makes this fire
        // at all while the window is otherwise idle (egui doesn't call
        // `update` on a schedule of its own, only in response to input or
        // an explicit repaint request) - scheduled for exactly when the
        // next check is actually due, so this isn't polling every frame in
        // between.
        if self.instances.dev_builds_enabled {
            if self.last_dev_check.elapsed() >= DEV_BUILD_CHECK_INTERVAL {
                self.refresh_dev_build();
            }
            ctx.request_repaint_after(DEV_BUILD_CHECK_INTERVAL.saturating_sub(self.last_dev_check.elapsed()));
        }

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.heading("Craftmjne");
                ui.label(
                    egui::RichText::new(format!("Launcher {}", selfupdate::CURRENT_VERSION))
                        .weak(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut self.tab, Tab::Versions, "Versions");
                    ui.selectable_value(&mut self.tab, Tab::Instances, "Instances");
                    ui.separator();
                    self.shortcut_row(ui);
                });
            });
            ui.add_space(6.0);
        });

        if let Some(SelfUpdate::Applied { version, notes }) = &self.self_update {
            egui::TopBottomPanel::top("launcher-update").show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(format!("Launcher updated to {version}."))
                            .strong()
                            .color(egui::Color32::from_rgb(120, 220, 120)),
                    );
                    ui.label("Restart the launcher to use it.");
                    if let Some(notes) = notes {
                        ui.label(egui::RichText::new(notes).weak());
                    }
                });
                ui.add_space(4.0);
            });
        }

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "Ready." } else { &self.status });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} installed · {}",
                            self.library.installed().len(),
                            remote::format_bytes(self.library.total_bytes())
                        ))
                        .weak(),
                    );
                });
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Instances => self.instances_tab(ui),
            Tab::Versions => self.versions_tab(ui),
        });
    }
}

impl LauncherApp {
    fn instances_tab(&mut self, ui: &mut egui::Ui) {
        let installed = self.library.installed();

        ui.horizontal(|ui| {
            if ui.button("New instance").clicked() {
                let version = installed
                    .first()
                    .cloned()
                    .or_else(|| match &self.releases {
                        Releases::Loaded(list) => list.first().map(|r| r.version.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                self.instances.add(Instance::new("New Instance", version));
                self.selected = Some(self.instances.items.len() - 1);
                self.save_instances();
            }
            if installed.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "No versions downloaded yet - open the Versions tab first.",
                    )
                    .weak(),
                );
            }
        });
        ui.separator();

        if self.instances.items.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("No instances yet.");
                ui.label(
                    egui::RichText::new(
                        "An instance is a name, a game version, and the options to start it with.",
                    )
                    .weak(),
                );
            });
            return;
        }

        egui::SidePanel::left("instance-list").resizable(true).show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, instance) in self.instances.items.iter().enumerate() {
                    let ready = self.library.is_installed(&instance.version);
                    let label = if ready {
                        format!("{}  ({})", instance.name, instance.version)
                    } else {
                        format!("{}  ({} - not downloaded)", instance.name, instance.version)
                    };
                    if ui.selectable_label(self.selected == Some(i), label).clicked() {
                        self.selected = Some(i);
                    }
                }
            });
        });

        let Some(index) = self.selected.filter(|i| *i < self.instances.items.len()) else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label("Select an instance to edit it.");
            });
            return;
        };

        let mut changed = false;
        let mut delete = false;
        let mut play = false;

        {
            let instance = &mut self.instances.items[index];
            ui.horizontal(|ui| {
                ui.label("Name");
                changed |= ui.text_edit_singleline(&mut instance.name).changed();
            });

            ui.horizontal(|ui| {
                ui.label("Version");
                let current = instance.version.clone();
                egui::ComboBox::from_id_salt("instance-version")
                    .selected_text(&current)
                    .show_ui(ui, |ui| {
                        for version in &installed {
                            changed |= ui
                                .selectable_value(&mut instance.version, version.clone(), version)
                                .changed();
                        }
                        if installed.is_empty() {
                            ui.label(egui::RichText::new("nothing downloaded").weak());
                        }
                    });
            });

            ui.horizontal(|ui| {
                let mut on = instance.render_distance.is_some();
                if ui.checkbox(&mut on, "Render distance").changed() {
                    instance.render_distance = on.then_some(8);
                    changed = true;
                }
                if let Some(rd) = &mut instance.render_distance {
                    changed |= ui.add(egui::Slider::new(rd, 2..=24)).changed();
                }
            });

            ui.horizontal(|ui| {
                let mut on = instance.seed.is_some();
                if ui.checkbox(&mut on, "Fixed world seed").changed() {
                    instance.seed = on.then_some(1337);
                    changed = true;
                }
                if let Some(seed) = &mut instance.seed {
                    changed |= ui.add(egui::DragValue::new(seed)).changed();
                }
            });

            ui.horizontal(|ui| {
                ui.label("Extra arguments");
                changed |= ui.text_edit_singleline(&mut instance.extra_args).changed();
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let ready = self.library.is_installed(&instance.version);
                play = ui
                    .add_enabled(ready, egui::Button::new("  Play  "))
                    .on_disabled_hover_text("This instance's version isn't downloaded yet.")
                    .clicked();
                delete = ui.button("Delete instance").clicked();
            });
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(format!(
                "Worlds are shared by every instance: {}",
                self.paths.saves_dir().display()
            ))
            .weak(),
        );

        if play {
            self.play(index, ui.ctx());
        }
        if delete {
            self.instances.remove(index);
            self.selected = None;
            changed = true;
        }
        if changed {
            self.save_instances();
        }
    }

    fn versions_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.releases = Releases::Loading;
                self.jobs.spawn(|| JobDone::Releases(remote::fetch_releases()));
            }
            ui.label(
                egui::RichText::new("Downloaded versions are kept, so each one is fetched once.")
                    .weak(),
            );
        });
        ui.separator();

        let mut to_download: Option<RemoteVersion> = None;
        let mut to_remove: Option<String> = None;

        match &self.releases {
            Releases::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking for published versions...");
                });
            }
            Releases::Failed(err) => {
                ui.colored_label(egui::Color32::from_rgb(230, 130, 130), "Couldn't reach GitHub.");
                ui.label(egui::RichText::new(err).weak());
                ui.add_space(8.0);
                ui.label("Versions you've already downloaded still work offline:");
                for version in self.library.installed() {
                    ui.label(format!("  • {version}"));
                }
            }
            Releases::Loaded(list) if list.is_empty() => {
                ui.label("No published versions have a build for this platform yet.");
            }
            Releases::Loaded(list) => {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for release in list {
                        let installed = self.library.is_installed(&release.version);
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(&release.version).strong());
                                if !release.name.is_empty() && release.name != release.version {
                                    ui.label(egui::RichText::new(&release.name).weak());
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(progress) =
                                            self.downloads.progress(&release.version)
                                        {
                                            ui.spinner();
                                            ui.label(remote::format_bytes(progress.bytes()));
                                        } else if installed {
                                            if ui.button("Remove").clicked() {
                                                to_remove = Some(release.version.clone());
                                            }
                                            ui.label(
                                                egui::RichText::new("Downloaded")
                                                    .color(egui::Color32::from_rgb(120, 220, 120)),
                                            );
                                        } else if ui.button("Download").clicked() {
                                            to_download = Some(release.clone());
                                        }
                                    },
                                );
                            });
                            if !release.date.is_empty() {
                                ui.label(egui::RichText::new(&release.date).weak().small());
                            }
                        });
                    }
                });
            }
        }

        if let Some(release) = to_download {
            self.start_download(&release);
        }
        if let Some(version) = to_remove {
            self.status = match self.library.remove(&version) {
                Ok(()) => format!("Removed Craftmjne {version}. Your worlds are untouched."),
                Err(err) => format!("Couldn't remove {version}: {err}"),
            };
        }

        self.dev_build_panel(ui);
    }

    /// The "Enable dev builds" toggle and, once on, the rolling dev build's
    /// status - at the bottom of the Versions tab since it's the exception,
    /// not something most people installing this launcher want to see by
    /// default. Once installed, a dev build is playable through the exact
    /// same instance/version machinery as any tagged release - it just
    /// installs under the fixed `remote::DEV_VERSION_SLOT` ("dev") name
    /// instead of a version number, so nothing in `instances.rs`/`launch.rs`
    /// needed to change to support it.
    fn dev_build_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.separator();

        let was_enabled = self.instances.dev_builds_enabled;
        ui.checkbox(
            &mut self.instances.dev_builds_enabled,
            "Enable dev builds (unstable - built straight from main, not a real release)",
        );
        if self.instances.dev_builds_enabled != was_enabled {
            self.save_instances();
            if self.instances.dev_builds_enabled {
                self.refresh_dev_build();
            }
        }
        if !self.instances.dev_builds_enabled {
            return;
        }

        let mut want_refresh = false;
        let mut to_download_dev: Option<remote::DevBuild> = None;

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Dev build").strong());
            if ui.small_button("Check again").clicked() {
                want_refresh = true;
            }
        });

        match &self.dev_build {
            // Reached only if the toggle was already on when this app was
            // constructed and that startup fetch somehow hasn't landed yet
            // (or, defensively, any other way this state is seen at all) -
            // the toggle-changed branch above already covers the live
            // flip-it-on case.
            DevBuildState::Idle => want_refresh = true,
            DevBuildState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Checking for a dev build...");
                });
            }
            DevBuildState::Failed(err) => {
                ui.colored_label(egui::Color32::from_rgb(230, 130, 130), err);
            }
            DevBuildState::Loaded(build) => {
                let installed_commit = self.library.dev_commit();
                ui.horizontal(|ui| {
                    ui.label(format!("Latest: {}", build.short_commit));
                    if let Some(progress) = self.downloads.progress(remote::DEV_VERSION_SLOT) {
                        ui.spinner();
                        ui.label(remote::format_bytes(progress.bytes()));
                    } else if installed_commit.is_none() {
                        if ui.button("Download").clicked() {
                            to_download_dev = Some(build.clone());
                        }
                    } else if installed_commit.as_deref() == Some(build.commit.as_str()) {
                        ui.colored_label(egui::Color32::from_rgb(120, 220, 120), "Up to date");
                    } else {
                        if ui.button("Update").clicked() {
                            to_download_dev = Some(build.clone());
                        }
                        ui.colored_label(egui::Color32::from_rgb(230, 200, 120), "Update available");
                    }
                });
            }
        }

        if want_refresh {
            self.refresh_dev_build();
        }
        if let Some(build) = to_download_dev {
            self.start_dev_download(&build);
        }
    }

    /// "Add a shortcut" buttons in the header, for anyone who got the
    /// launcher a way that skips the NSIS installer's own shortcut step
    /// (the rolling self-update, a manual download, a dev build) and wants
    /// one without hunting down the install folder by hand. Windows-only,
    /// same as the feature itself (`shortcut.rs`) - hidden entirely rather
    /// than shown greyed-out or erroring on click, since there's no Desktop
    /// or Start Menu concept to offer on the other platforms this launcher
    /// ships for.
    #[cfg(windows)]
    fn shortcut_row(&mut self, ui: &mut egui::Ui) {
        if ui.small_button("Add Desktop shortcut").clicked() {
            self.status = shortcut::create(Location::Desktop).unwrap_or_else(|e| e);
        }
        if ui.small_button("Add Start Menu shortcut").clicked() {
            self.status = shortcut::create(Location::StartMenu).unwrap_or_else(|e| e);
        }
    }

    #[cfg(not(windows))]
    fn shortcut_row(&mut self, _ui: &mut egui::Ui) {}
}
