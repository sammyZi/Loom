use anyhow::Result;
use ide_core::{FsEvent, WorkspaceRoot};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tokio::sync::broadcast;

/// Keeps the workspace watcher alive. Dropping it stops the watcher thread and
/// unregisters the recursive watch — without this, opening folder after folder
/// accumulated one live watcher per open.
pub struct WatchGuard {
    stop: Arc<AtomicBool>,
}

impl Drop for WatchGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

const POLL: Duration = Duration::from_millis(250);

pub fn watch_workspace(ws: WorkspaceRoot, tx: broadcast::Sender<FsEvent>) -> Result<WatchGuard> {
    let (raw_tx, raw_rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(raw_tx, Config::default())?;
    watcher.watch(ws.root(), RecursiveMode::Recursive)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    // The watcher handle stays on this thread; dropping it (guard dropped and
    // the pump exits) is what unregisters the OS watch.
    std::thread::Builder::new()
        .name("fs-watch".into())
        .spawn(move || {
            loop {
                if stop_thread.load(Ordering::SeqCst) {
                    break;
                }
                match raw_rx.recv_timeout(POLL) {
                    Ok(res) => {
                        let Ok(event) = res else { continue };
                        for ev in to_fs_events(&ws, event) {
                            let _ = tx.send(ev);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            drop(watcher);
        })
        .ok();
    Ok(WatchGuard { stop })
}

fn to_fs_events(ws: &WorkspaceRoot, event: notify::Event) -> Vec<FsEvent> {
    let kind = match event.kind {
        EventKind::Remove(_) => |rel: String| FsEvent::Removed { path: rel },
        _ => |rel: String| FsEvent::Changed { path: rel },
    };
    let mut out = Vec::new();
    for path in event.paths {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if WorkspaceRoot::is_skipped_dir(name) {
            continue;
        }
        if path.components().any(|c| {
            c.as_os_str()
                .to_str()
                .map(WorkspaceRoot::is_skipped_dir)
                .unwrap_or(false)
        }) {
            continue;
        }
        let rel = ws.rel_to(&path);
        if rel.is_empty() {
            continue;
        }
        out.push(kind(rel));
    }
    out
}
