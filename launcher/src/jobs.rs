//! Background work, and how it gets back to the UI.
//!
//! Everything that touches the network runs on its own thread and reports
//! through one channel, because egui redraws on the main thread and a
//! blocking download there would freeze the window. The UI polls
//! [`Jobs::drain`] once per frame; nothing else ever blocks.

use std::sync::mpsc::{channel, Receiver, Sender};

use crate::remote::{DevBuild, Progress, RemoteVersion};
use crate::selfupdate::SelfUpdate;

/// A finished (or failed) background job.
pub enum JobDone {
    Releases(Result<Vec<RemoteVersion>, String>),
    Installed { version: String, result: Result<(), String> },
    SelfUpdate(SelfUpdate),
    /// The game process `app.rs`'s `play` spawned and hid the launcher for
    /// has exited - time to show the launcher window again.
    GameExited,
    /// `remote::fetch_dev_build` finished - `Ok(None)` means no dev build
    /// exists yet for this platform (not an error, just nothing to offer).
    DevBuild(Result<Option<DevBuild>, String>),
    /// A dev build finished downloading. Carries the commit it was
    /// installed *from* (not re-derived from disk afterward) so
    /// `app.rs`'s `poll_jobs` can call `Library::record_dev_commit`
    /// without a second round-trip to figure out what just landed.
    DevInstalled { commit: String, result: Result<(), String> },
}

pub struct Jobs {
    tx: Sender<JobDone>,
    rx: Receiver<JobDone>,
}

impl Default for Jobs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx }
    }
}

impl Jobs {
    /// Runs `work` off the main thread and delivers its result to the next
    /// [`drain`](Self::drain).
    pub fn spawn(&self, work: impl FnOnce() -> JobDone + Send + 'static) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // A send failure just means the launcher is closing and nobody
            // is listening any more, which is not worth a panic.
            let _ = tx.send(work());
        });
    }

    /// Everything that finished since the last call.
    pub fn drain(&self) -> Vec<JobDone> {
        self.rx.try_iter().collect()
    }
}

/// Which version downloads are in flight, and how far along they are.
#[derive(Default)]
pub struct Downloads(Vec<(String, Progress)>);

impl Downloads {
    pub fn start(&mut self, version: &str) -> Progress {
        let progress = Progress::default();
        self.0.push((version.to_string(), progress.clone()));
        progress
    }

    pub fn finish(&mut self, version: &str) {
        self.0.retain(|(v, _)| v != version);
    }

    pub fn progress(&self, version: &str) -> Option<&Progress> {
        self.0.iter().find(|(v, _)| v == version).map(|(_, p)| p)
    }

    pub fn is_busy(&self) -> bool {
        !self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spawned_job_comes_back_through_drain() {
        let jobs = Jobs::default();
        jobs.spawn(|| JobDone::SelfUpdate(SelfUpdate::UpToDate));

        // Poll the way the UI does rather than blocking, so this test can't
        // hang if the job never arrives.
        for _ in 0..200 {
            let done = jobs.drain();
            if !done.is_empty() {
                assert!(matches!(done[0], JobDone::SelfUpdate(SelfUpdate::UpToDate)));
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("background job never reported back");
    }

    #[test]
    fn draining_with_nothing_running_is_empty_and_does_not_block() {
        assert!(Jobs::default().drain().is_empty());
    }

    #[test]
    fn downloads_track_progress_per_version_until_they_finish() {
        let mut downloads = Downloads::default();
        assert!(!downloads.is_busy());
        assert!(downloads.progress("1.0.0").is_none());

        downloads.start("1.0.0");
        downloads.start("2.0.0");
        assert!(downloads.is_busy());
        assert!(downloads.progress("1.0.0").is_some());

        downloads.finish("1.0.0");
        assert!(downloads.progress("1.0.0").is_none());
        assert!(downloads.progress("2.0.0").is_some(), "finishing one must not clear the other");
        assert!(downloads.is_busy());

        downloads.finish("2.0.0");
        assert!(!downloads.is_busy());
    }
}
