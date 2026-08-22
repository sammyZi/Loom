use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use ide_core::{AgentEvent, WorkspaceRoot};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspace", get(workspace_get))
        .route("/workspace/pick", post(workspace_pick))
        .route("/workspace/open", post(workspace_open))
        .route("/fs/list", get(fs_list))
        .route("/files/tree", get(files_tree))
        .route("/files/content", get(files_get).put(files_put))
        .route("/ws/files", get(ws_files))
        .route("/ws/shell", get(ws_shell))
        .route("/git/status", get(git_status))
        .route("/git/diff", get(git_diff))
        .route("/git/commit", post(git_commit))
        .route("/sessions", get(sessions_list).put(sessions_upsert).delete(sessions_clear))
        .route("/sessions/{id}", delete(sessions_delete))
        .route("/sessions/{id}/rename", post(sessions_rename))
        .route("/sessions/{id}/archive", post(sessions_archive))
        .route("/agent/run", post(agent_run))
        .route("/agent/cancel", post(agent_cancel))
        .route("/agent/models", get(agent_models))
        .route("/shell/run", post(shell_run))
        .route("/shell/cancel", post(shell_cancel))
        .route("/ws/agent", get(ws_agent))
}

async fn workspace_get(State(st): State<AppState>) -> impl IntoResponse {
    match st.workspace.read().await.as_ref() {
        Some(ws) => Json(serde_json::json!({ "path": ws.root().to_string_lossy() })).into_response(),
        None => Json(serde_json::json!({ "path": null })).into_response(),
    }
}

async fn workspace_pick(State(st): State<AppState>) -> impl IntoResponse {
    let picked = tokio::task::spawn_blocking(crate::pick::pick_folder).await;
    match picked {
        Ok(Some(path)) => open_path(&st, path).await,
        Ok(None) => (StatusCode::OK, Json(serde_json::json!({ "path": null }))).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct OpenBody {
    path: String,
}

async fn fs_list(Query(q): Query<HashMap<String, String>>) -> impl IntoResponse {
    let path = q.get("path").map(|s| s.to_string());
    match tokio::task::spawn_blocking(move || viewer::list_dir(path.as_deref())).await {
        Ok(Ok((path, parent, entries))) => Json(serde_json::json!({
            "path": path,
            "parent": parent,
            "entries": entries,
        }))
        .into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, e.to_string()),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn workspace_open(State(st): State<AppState>, Json(body): Json<OpenBody>) -> impl IntoResponse {
    open_path(&st, std::path::PathBuf::from(body.path)).await
}

async fn open_path(st: &AppState, path: std::path::PathBuf) -> axum::response::Response {
    // Canonicalize does disk I/O; keep it off the async workers.
    let opened = tokio::task::spawn_blocking(move || WorkspaceRoot::open(&path)).await;
    match opened {
        Ok(Ok(ws)) => {
            let shown = ws.root().to_string_lossy().into_owned();
            // Stop the previous folder's watcher first: dropping the old guard
            // ends its thread, instead of leaking one watcher per open.
            let mut old = st.watcher.lock().await;
            if let Ok(guard) = viewer::watch_workspace(ws.clone(), st.fs_tx.clone()) {
                *old = Some(guard);
            }
            drop(old);
            *st.workspace.write().await = Some(ws);
            Json(serde_json::json!({ "path": shown })).into_response()
        }
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, e.to_string()),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn files_tree(State(st): State<AppState>) -> impl IntoResponse {
    st.blocking_ws(viewer::tree).await
}

async fn files_get(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(path) = q.get("path").cloned() else {
        return err(StatusCode::BAD_REQUEST, "path required");
    };
    st.blocking_ws(move |ws| {
        viewer::read_file(ws, &path)
            .map(|content| serde_json::json!({ "path": path, "content": content }))
    })
    .await
}

#[derive(Deserialize)]
struct PutFile {
    path: String,
    content: String,
}

async fn files_put(State(st): State<AppState>, Json(body): Json<PutFile>) -> impl IntoResponse {
    st.blocking_ws(move |ws| {
        viewer::write_file(ws, &body.path, &body.content)
            .map(|()| serde_json::json!({ "ok": true }))
    })
    .await
}

async fn git_status(State(st): State<AppState>) -> impl IntoResponse {
    st.blocking_ws(viewer::status).await
}

async fn git_diff(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let path = q.get("path").map(|s| s.to_string());
    st.blocking_ws(move |ws| viewer::diff(ws, path.as_deref()).map(|d| serde_json::json!({ "diff": d })))
        .await
}

#[derive(Deserialize)]
struct CommitBody {
    message: String,
}

async fn git_commit(State(st): State<AppState>, Json(body): Json<CommitBody>) -> impl IntoResponse {
    st.blocking_ws(move |ws| {
        viewer::commit(ws, &body.message).map(|id| serde_json::json!({ "id": id }))
    })
    .await
}

async fn sessions_list(State(st): State<AppState>) -> impl IntoResponse {
    st.blocking_db(|db| db.list().map(|sessions| serde_json::json!({ "sessions": sessions })))
        .await
}

async fn sessions_upsert(
    State(st): State<AppState>,
    Json(body): Json<crate::db::Session>,
) -> impl IntoResponse {
    st.blocking_db(move |db| db.upsert(&body).map(|()| serde_json::json!({ "ok": true })))
        .await
}

async fn sessions_delete(
    State(st): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    st.blocking_db(move |db| db.delete(&id).map(|()| serde_json::json!({ "ok": true })))
        .await
}

#[derive(Deserialize)]
struct RenameBody {
    title: String,
}

async fn sessions_rename(
    State(st): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(body): Json<RenameBody>,
) -> impl IntoResponse {
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return err(StatusCode::BAD_REQUEST, "title required");
    }
    st.blocking_db(move |db| db.rename(&id, &title).map(|()| serde_json::json!({ "ok": true })))
        .await
}

async fn sessions_archive(
    State(st): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    st.blocking_db(move |db| db.archive(&id).map(|()| serde_json::json!({ "ok": true })))
        .await
}

async fn sessions_clear(State(st): State<AppState>) -> impl IntoResponse {
    st.blocking_db(|db| db.clear().map(|()| serde_json::json!({ "ok": true })))
        .await
}

#[derive(Deserialize)]
struct RunBody {
    prompt: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    effort: Option<String>,
}

async fn agent_models() -> impl IntoResponse {
    Json(serde_json::json!({
        "models": agent::model_catalog(),
        "default": agent::normalize_model(""),
    }))
}

/// Claims the one agent slot. Dropping it releases the slot, even if the task
/// panics partway through the pipeline.
struct RunSlot(std::sync::Arc<AtomicBool>);

impl Drop for RunSlot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn agent_run(State(st): State<AppState>, Json(body): Json<RunBody>) -> impl IntoResponse {
    let ws = match st.require_ws().await {
        Ok(w) => w,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    if body.prompt.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "prompt required");
    }
    // One run at a time. A second concurrent run used to interleave two event
    // streams into one feed and left the earlier run uncancellable.
    if st
        .running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return err(StatusCode::CONFLICT, "an agent task is already running");
    }
    let model = agent::normalize_model(body.model.as_deref().unwrap_or("")).to_string();
    let mode = orchestrator::Mode::parse(body.mode.as_deref().unwrap_or(""));
    let effort = agent::normalize_effort(body.effort.as_deref().unwrap_or("")).to_string();
    let cancel = CancellationToken::new();
    *st.cancel.lock().await = Some(cancel.clone());
    let sandbox = st.sandbox.clone();
    let events = st.agent_tx.clone();
    let prompt = body.prompt;
    tokio::spawn(async move {
        let _slot = RunSlot(st.running.clone());
        let result =
            orchestrator::run_task(prompt, model, mode, effort, ws, sandbox, events.clone(), cancel.clone())
                .await;
        // The run slot is still held (dropped after this block), so no newer
        // run can have replaced the token: clearing unconditionally is safe.
        *st.cancel.lock().await = None;
        if let Err(e) = result {
            let _ = events.send(AgentEvent::Error { message: e.to_string() });
        }
    });
    Json(serde_json::json!({ "ok": true })).into_response()
}

async fn agent_cancel(State(st): State<AppState>) -> impl IntoResponse {
    if let Some(c) = st.cancel.lock().await.take() {
        c.cancel();
    }
    Json(serde_json::json!({ "ok": true }))
}

#[derive(Deserialize)]
struct ShellBody {
    cmd: String,
    /// Which terminal is asking, so /shell/cancel can kill just that one.
    /// Optional: a client that predates cancellation still runs commands, it
    /// just cannot interrupt them.
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
struct ShellCancelBody {
    id: String,
}

async fn shell_run(State(st): State<AppState>, Json(body): Json<ShellBody>) -> impl IntoResponse {
    let ws = match st.require_ws().await {
        Ok(w) => w,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    let cmd = body.cmd.trim().to_string();
    if cmd.is_empty() {
        return err(StatusCode::BAD_REQUEST, "cmd required");
    }
    let cancel = CancellationToken::new();
    // A terminal runs one command at a time, so anything still registered under
    // this id is stale — cancel it rather than orphaning its process tree.
    // An unnamed caller is left unregistered so it cannot cancel someone else.
    if !body.id.is_empty() {
        if let Some(stale) = st.shells.lock().await.insert(body.id.clone(), cancel.clone()) {
            stale.cancel();
        }
    }
    // Forward output to /ws/shell as it is produced, so the terminal shows a
    // long command's progress instead of nothing until it exits. Unnamed
    // callers get no stream, since nothing could tell their chunks apart.
    let mut sink = None;
    if !body.id.is_empty() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let shell_tx = st.shell_tx.clone();
        let id = body.id.clone();
        tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                let _ = shell_tx.send(ide_core::ShellEvent::Chunk { id: id.clone(), text });
            }
        });
        sink = Some(tx);
    }
    let sandbox = st.sandbox.clone();
    let token = cancel.clone();
    let run = tokio::task::spawn(async move {
        let secs = std::time::Duration::from_secs(60);
        #[cfg(windows)]
        {
            sandbox
                .run_streaming(&ws, "cmd", &["/C".into(), cmd], secs, &token, sink)
                .await
        }
        #[cfg(not(windows))]
        {
            sandbox
                .run_streaming(&ws, "sh", &["-lc".into(), cmd], secs, &token, sink)
                .await
        }
    })
    .await;
    if !body.id.is_empty() {
        st.shells.lock().await.remove(&body.id);
    }
    match run {
        Ok(Ok(out)) => Json(serde_json::json!({
            "exit_code": out.exit_code,
            "stdout": out.stdout,
            "stderr": out.stderr,
        }))
        .into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, e.to_string()),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

/// Kill the process tree of whatever this terminal is running. Idempotent: a
/// terminal with nothing in flight is a no-op, which is what closing an idle
/// tab hits.
async fn shell_cancel(
    State(st): State<AppState>,
    Json(body): Json<ShellCancelBody>,
) -> impl IntoResponse {
    if let Some(c) = st.shells.lock().await.remove(&body.id) {
        c.cancel();
    }
    Json(serde_json::json!({ "ok": true }))
}

async fn ws_files(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pump_ws(socket, st.fs_tx.subscribe()))
}

async fn ws_shell(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pump_ws(socket, st.shell_tx.subscribe()))
}

async fn ws_agent(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pump_ws(socket, st.agent_tx.subscribe()))
}

async fn pump_ws<T: serde::Serialize + Clone + Send + 'static>(
    socket: WebSocket,
    mut rx: tokio::sync::broadcast::Receiver<T>,
) {
    let (mut sink, mut stream) = socket.split();
    loop {
        tokio::select! {
            incoming = stream.next() => {
                if incoming.is_none() { break; }
            }
            msg = rx.recv() => {
                match msg {
                    Ok(ev) => {
                        let Ok(txt) = serde_json::to_string(&ev) else { continue };
                        if sink.send(Message::Text(txt.into())).await.is_err() { break; }
                    }
                    // A slow client missed some events; tell the log so the gap
                    // is visible instead of silently losing half a reply.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("websocket client lagged, dropped {n} events");
                        continue;
                    }
                    Err(_) => break,
                }
            }
        }
    }
}

pub fn err(code: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (code, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}
