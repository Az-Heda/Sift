//! Automatic monitoring (V3): watch the tracked folders and rescan when files change.
//!
//! The watcher only *triggers* the normal incremental scan, which reuses cached hashes, so
//! nothing new can happen to a user's files here: monitoring never deletes or moves anything.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::Connection;
use tauri::AppHandle;

pub const SETTING_KEY: &str = "monitor.enabled";

/// Changes must stop for this long before a scan starts (copying a big file emits a stream
/// of events; this waits for it to finish).
const QUIET: Duration = Duration::from_secs(5);
/// How long to wait for a running scan to end before giving up on this trigger.
const BUSY_RETRIES: u32 = 24;
const BUSY_WAIT: Duration = Duration::from_secs(5);

/// Whether monitoring is on. Off unless the user turned it on.
pub fn enabled(conn: &Connection) -> Result<bool, String> {
    Ok(crate::db::get_setting(conn, SETTING_KEY)?.as_deref() == Some("1"))
}

/// True when a change to `path` could affect scan results: a supported file type outside
/// excluded folders (system/cache folders, user exclusions, the quarantine folder).
pub fn is_relevant(path: &Path, roots: &[PathBuf], ignored: &[PathBuf]) -> bool {
    // Judge folder names below the tracked folder only, as scans do: a folder the user added
    // on purpose is scanned even if it lives somewhere like ~/.cache.
    let below_root = roots.iter().find_map(|r| path.strip_prefix(r).ok()).unwrap_or(path);
    crate::scan::is_supported_path(path)
        && !crate::scan::in_excluded_dir(below_root)
        && !ignored.iter().any(|dir| path.starts_with(dir))
}

/// Calls `fire` once a burst of signals has been quiet for `quiet`. Returns when the sender
/// goes away (the watcher was replaced or switched off), without firing for a pending burst.
pub fn debounce(rx: &Receiver<()>, quiet: Duration, mut fire: impl FnMut()) {
    while rx.recv().is_ok() {
        loop {
            match rx.recv_timeout(quiet) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        fire();
    }
}

/// Starts watching `folders`. Dropping the returned watcher stops monitoring.
pub fn start(app: &AppHandle, folders: &[PathBuf], ignored: Vec<PathBuf>) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let roots = folders.to_vec();
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(ev) = res {
            if !ev.kind.is_access() && ev.paths.iter().any(|p| is_relevant(p, &roots, &ignored)) {
                let _ = tx.send(());
            }
        }
    })
    .map_err(|e| e.to_string())?;

    let mut watching = 0;
    for f in folders {
        // A folder that vanished (unplugged drive) just isn't watched.
        if watcher.watch(f, RecursiveMode::Recursive).is_ok() {
            watching += 1;
        }
    }
    if watching == 0 && !folders.is_empty() {
        return Err("none of the folders could be watched".into());
    }

    let app = app.clone();
    std::thread::spawn(move || {
        debounce(&rx, QUIET, || {
            for _ in 0..BUSY_RETRIES {
                match crate::launch_scan(&app) {
                    Err(e) if e == crate::SCAN_BUSY => std::thread::sleep(BUSY_WAIT),
                    _ => break,
                }
            }
        })
    });
    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn relevance_follows_scan_rules() {
        let roots = [PathBuf::from("/photos"), PathBuf::from("/home/me/.cache/pics")];
        let ignored = [PathBuf::from("/data/quarantine")];
        let rel = |p: &str| is_relevant(Path::new(p), &roots, &ignored);
        assert!(rel("/photos/a.JPG"));
        assert!(rel("/photos/clip.mp4"));
        assert!(!rel("/photos/notes.txt"));
        assert!(!rel("/photos/folder"));
        assert!(!rel("/photos/.git/x.png"));
        assert!(!rel("/photos/node_modules/pkg/x.png"));
        assert!(!rel("/data/quarantine/1_a.jpg"));
        assert!(!rel("/photos/.Trash-1000/a.jpg"));
        // A tracked folder inside an excluded-looking path is still watched.
        assert!(rel("/home/me/.cache/pics/a.png"));
        assert!(!rel("/home/me/.cache/pics/.git/a.png"));
    }

    #[test]
    fn a_burst_fires_once_after_it_goes_quiet() {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            debounce(&rx, Duration::from_millis(80), || done_tx.send(()).unwrap());
        });
        for _ in 0..5 {
            tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(20)); // faster than the quiet period
        }
        assert!(done_rx.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(done_rx.recv_timeout(Duration::from_millis(250)).is_err(), "must fire only once");

        tx.send(()).unwrap(); // a later burst fires again
        assert!(done_rx.recv_timeout(Duration::from_secs(2)).is_ok());
        drop(tx);
        worker.join().unwrap();
    }

    #[test]
    fn dropping_the_sender_cancels_a_pending_burst() {
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            debounce(&rx, Duration::from_millis(300), || done_tx.send(()).unwrap());
        });
        tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(30));
        drop(tx);
        worker.join().unwrap();
        assert!(done_rx.try_recv().is_err());
    }

    #[test]
    fn real_file_changes_reach_the_watcher() {
        // Uses the OS watcher directly (no Tauri), mirroring `start`'s event filter.
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel::<()>();
        let roots = vec![dir.path().to_path_buf()];
        let mut w = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(ev) = res {
                if !ev.kind.is_access() && ev.paths.iter().any(|p| is_relevant(p, &roots, &[])) {
                    let _ = tx.send(());
                }
            }
        })
        .unwrap();
        w.watch(dir.path(), RecursiveMode::Recursive).unwrap();
        std::fs::write(dir.path().join("new.png"), b"x").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "no event for a new image");
    }
}
