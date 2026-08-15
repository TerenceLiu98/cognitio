use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use tauri::{AppHandle, Emitter, Manager};

use crate::{commands::AppState, jobs, logging, models::AppSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Fingerprint {
    size: u64,
    modified: SystemTime,
}

#[derive(Default)]
struct StabilityTracker {
    files: HashMap<PathBuf, (Fingerprint, u8, Instant)>,
}

#[derive(Debug, Eq, PartialEq)]
enum Observation {
    Pending,
    Stable,
    TimedOut,
}

const STABILITY_TIMEOUT: Duration = Duration::from_secs(5 * 60);

impl StabilityTracker {
    fn observe(&mut self, path: PathBuf, fingerprint: Fingerprint) -> Observation {
        self.observe_at(path, fingerprint, Instant::now())
    }

    fn observe_at(&mut self, path: PathBuf, fingerprint: Fingerprint, now: Instant) -> Observation {
        let entry = self.files.entry(path).or_insert((fingerprint, 0, now));
        if entry.0 == fingerprint {
            entry.1 = entry.1.saturating_add(1);
        } else {
            entry.0 = fingerprint;
            entry.1 = 1;
        }
        if entry.1 >= 3 {
            Observation::Stable
        } else if now.duration_since(entry.2) >= STABILITY_TIMEOUT {
            Observation::TimedOut
        } else {
            Observation::Pending
        }
    }

    fn remove_missing(&mut self) {
        self.files.retain(|path, _| path.exists());
    }
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tracker = StabilityTracker::default();
        loop {
            scan_once(&app, &mut tracker);
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
}

fn scan_once(app: &AppHandle, tracker: &mut StabilityTracker) {
    let state = app.state::<AppState>();
    let (enabled, workspace, mut known_hashes) = match state.snapshot.lock() {
        Ok(snapshot) => (
            snapshot.configured && snapshot.watching,
            PathBuf::from(&snapshot.settings.workspace_root),
            jobs::load_all(PathBuf::from(&snapshot.settings.workspace_root).as_path())
                .into_iter()
                .map(|job| job.sha256)
                .collect::<HashSet<_>>(),
        ),
        Err(_) => return,
    };
    if !enabled {
        return;
    }

    let Ok(entries) = fs::read_dir(workspace.join("inbox")) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()).filter(|path| {
        path.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    }) {
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        let fingerprint = Fingerprint {
            size: metadata.len(),
            modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        };
        match tracker.observe(path.clone(), fingerprint) {
            Observation::Pending => continue,
            Observation::TimedOut => {
                tracker.files.remove(&path);
                let message = "PDF did not become stable within five minutes";
                match jobs::create(&workspace, &path)
                    .and_then(|record| jobs::fail(&workspace, &record.id, message))
                {
                    Ok(record) => {
                        known_hashes.insert(record.sha256.clone());
                        if let Ok(entry) =
                            logging::append(&state.config_dir, "error", message, Some(&record.id))
                        {
                            if let Ok(mut snapshot) = state.snapshot.lock() {
                                snapshot.jobs.insert(0, (&record).into());
                                snapshot.logs.push(entry);
                                let current: AppSnapshot = snapshot.clone();
                                let _ = app.emit("app-snapshot", current);
                            }
                        }
                    }
                    Err(error) => {
                        let _ = logging::append(&state.config_dir, "error", &error, None);
                    }
                }
                continue;
            }
            Observation::Stable => {}
        }
        tracker.files.remove(&path);
        let Ok(hash) = jobs::sha256(&path) else {
            continue;
        };
        if !known_hashes.insert(hash) {
            continue;
        }
        match jobs::create(&workspace, &path) {
            Ok(record) => {
                if let Ok(entry) = logging::append(
                    &state.config_dir,
                    "info",
                    &format!("Queued {}", record.filename),
                    Some(&record.id),
                ) {
                    if let Ok(mut snapshot) = state.snapshot.lock() {
                        snapshot.jobs.insert(0, (&record).into());
                        snapshot.logs.push(entry);
                        let current: AppSnapshot = snapshot.clone();
                        let _ = app.emit("app-snapshot", current);
                    }
                }
            }
            Err(error) => {
                let _ = logging::append(&state.config_dir, "error", &error, None);
            }
        }
    }
    tracker.remove_missing();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_three_unchanged_observations() {
        let mut tracker = StabilityTracker::default();
        let path = PathBuf::from("paper.pdf");
        let fingerprint = Fingerprint {
            size: 10,
            modified: SystemTime::UNIX_EPOCH,
        };
        assert_eq!(
            tracker.observe(path.clone(), fingerprint),
            Observation::Pending
        );
        assert_eq!(
            tracker.observe(path.clone(), fingerprint),
            Observation::Pending
        );
        assert_eq!(tracker.observe(path, fingerprint), Observation::Stable);
    }

    #[test]
    fn reports_files_that_never_stabilize() {
        let mut tracker = StabilityTracker::default();
        let path = PathBuf::from("paper.pdf");
        let start = Instant::now();
        let initial = Fingerprint {
            size: 10,
            modified: SystemTime::UNIX_EPOCH,
        };
        assert_eq!(
            tracker.observe_at(path.clone(), initial, start),
            Observation::Pending
        );
        let changed = Fingerprint {
            size: 11,
            modified: SystemTime::UNIX_EPOCH,
        };
        assert_eq!(
            tracker.observe_at(path, changed, start + STABILITY_TIMEOUT),
            Observation::TimedOut
        );
    }
}
