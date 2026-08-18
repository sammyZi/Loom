use core::{AgentEvent, FsEvent, WorkspaceRoot};
use sandbox::{native, Sandbox};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub workspace: Arc<RwLock<Option<WorkspaceRoot>>>,
    pub fs_tx: broadcast::Sender<FsEvent>,
    pub agent_tx: broadcast::Sender<AgentEvent>,
    pub cancel: Arc<Mutex<Option<CancellationToken>>>,
    pub sandbox: Arc<dyn Sandbox>,
}

impl AppState {
    pub fn new() -> Self {
        let (fs_tx, _) = broadcast::channel(256);
        let (agent_tx, _) = broadcast::channel(1024);
        Self {
            workspace: Arc::new(RwLock::new(None)),
            fs_tx,
            agent_tx,
            cancel: Arc::new(Mutex::new(None)),
            sandbox: native(),
        }
    }

    pub async fn require_ws(&self) -> Result<WorkspaceRoot, String> {
        self.workspace
            .read()
            .await
            .clone()
            .ok_or_else(|| "no folder open".into())
    }
}
