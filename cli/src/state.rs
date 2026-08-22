use ide_core::{AgentEvent, FsEvent, ShellEvent, WorkspaceRoot};
use sandbox::{native, Sandbox};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// One in-flight terminal command, foreground or background. `gen` lets a
/// finishing background task unregister only itself, never a newer command
/// that reused the same terminal id.
#[derive(Clone)]
pub struct ShellJob {
    pub token: CancellationToken,
    pub gen: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<RwLock<Option<WorkspaceRoot>>>,
    pub fs_tx: broadcast::Sender<FsEvent>,
    pub agent_tx: broadcast::Sender<AgentEvent>,
    /// Terminal output streamed while a command is still running.
    pub shell_tx: broadcast::Sender<ShellEvent>,
    /// Cancellation token of the run in flight, so /agent/cancel can stop it.
    pub cancel: Arc<Mutex<Option<CancellationToken>>>,
    /// True while an orchestrator pipeline is running. One run at a time: a
    /// second concurrent run used to interleave two event streams into one feed.
    pub running: Arc<AtomicBool>,
    /// One entry per terminal with a command in flight, keyed by the terminal's
    /// id, so /shell/cancel kills that terminal's process tree and no other.
    pub shells: Arc<StdMutex<HashMap<String, ShellJob>>>,
    /// Monotonic generator for ShellJob ids (process-wide).
    pub shell_gen: Arc<AtomicU64>,
    /// The live workspace watcher; replaced (and the old one stopped) on open.
    pub watcher: Arc<Mutex<Option<viewer::WatchGuard>>>,
    pub sandbox: Arc<dyn Sandbox>,
    pub db: Arc<crate::db::Db>,
}

/// Upper bound on simultaneous terminal jobs; mostly to stop a buggy client
/// from forking the machine through background runs.
pub const MAX_SHELL_JOBS: usize = 16;

impl AppState {
    pub fn new() -> Self {
        let (fs_tx, _) = broadcast::channel(256);
        let (agent_tx, _) = broadcast::channel(1024);
        let (shell_tx, _) = broadcast::channel(4096);
        Self {
            workspace: Arc::new(RwLock::new(None)),
            fs_tx,
            agent_tx,
            shell_tx,
            cancel: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            shells: Arc::new(StdMutex::new(HashMap::new())),
            shell_gen: Arc::new(AtomicU64::new(1)),
            watcher: Arc::new(Mutex::new(None)),
            sandbox: native(),
            // A broken session store must not stop the IDE from running, so fall
            // back to an in-memory database and log it.
            db: Arc::new(crate::db::Db::open().unwrap_or_else(|e| {
                tracing::warn!("session db unavailable ({e:#}); history will not persist");
                crate::db::Db::memory()
            })),
        }
    }

    pub async fn require_ws(&self) -> Result<WorkspaceRoot, String> {
        self.workspace
            .read()
            .await
            .clone()
            .ok_or_else(|| "no folder open".into())
    }

    pub async fn blocking_ws<T, F>(&self, f: F) -> axum::response::Response
    where
        T: serde::Serialize + Send + 'static,
        F: FnOnce(&WorkspaceRoot) -> anyhow::Result<T> + Send + 'static,
    {
        use axum::response::IntoResponse;
        match self.require_ws().await {
            Ok(ws) => match tokio::task::spawn_blocking(move || f(&ws)).await {
                Ok(Ok(v)) => axum::Json(v).into_response(),
                Ok(Err(e)) => crate::routes::err(axum::http::StatusCode::BAD_REQUEST, e.to_string()),
                Err(e) => crate::routes::err(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    e.to_string(),
                ),
            },
            Err(e) => crate::routes::err(axum::http::StatusCode::BAD_REQUEST, e),
        }
    }

    pub async fn blocking_db<T, F>(&self, f: F) -> axum::response::Response
    where
        T: serde::Serialize + Send + 'static,
        F: FnOnce(&crate::db::Db) -> anyhow::Result<T> + Send + 'static,
    {
        use axum::response::IntoResponse;
        let db = self.db.clone();
        match tokio::task::spawn_blocking(move || f(&db)).await {
            Ok(Ok(v)) => axum::Json(v).into_response(),
            Ok(Err(e)) => crate::routes::err(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            ),
            Err(e) => crate::routes::err(axum::http::StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}
