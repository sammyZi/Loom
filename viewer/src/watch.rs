use anyhow::Result;
use ide_core::{FsEvent, WorkspaceRoot};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc;
use tokio::sync::broadcast;

pub fn watch_workspace(ws: WorkspaceRoot, tx: broadcast::Sender<FsEvent>) -> Result<()> {
    std::thread::Builder::new()
        .name("fs-watch".into())
        .spawn(move || {
            let _ = run(ws, tx);
        })
        .ok();
    Ok(())
}

fn run(ws: WorkspaceRoot, tx: broadcast::Sender<FsEvent>) -> Result<()> {
    let (raw_tx, raw_rx) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(raw_tx, Config::default())?;
    watcher.watch(ws.root(), RecursiveMode::Recursive)?;
    for res in raw_rx {
        let Ok(event) = res else { continue };
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
            let ev = match event.kind {
                EventKind::Remove(_) => FsEvent::Removed { path: rel },
                _ => FsEvent::Changed { path: rel },
            };
            let _ = tx.send(ev);
        }
    }
    Ok(())
}
