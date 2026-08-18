use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use ide_core::{AgentEvent, WorkspaceRoot};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/workspace", get(workspace_get))
        .route("/workspace/pick", post(workspace_pick))
        .route("/workspace/open", post(workspace_open))
        .route("/files/tree", get(files_tree))
        .route("/files/content", get(files_get).put(files_put))
        .route("/ws/files", get(ws_files))
        .route("/git/status", get(git_status))
        .route("/git/diff", get(git_diff))
        .route("/git/commit", post(git_commit))
        .route("/agent/run", post(agent_run))
        .route("/agent/cancel", post(agent_cancel))
        .route("/ws/agent", get(ws_agent))
}

async fn workspace_get(State(st): State<AppState>) -> impl IntoResponse {
    match st.workspace.read().await.as_ref() {
        Some(ws) => Json(serde_json::json!({ "path": ws.root().to_string_lossy() })).into_response(),
        None => Json(serde_json::json!({ "path": null })).into_response(),
    }
}

async fn workspace_pick(State(st): State<AppState>) -> impl IntoResponse {
    let picked = tokio::task::spawn_blocking(|| rfd::FileDialog::new().pick_folder()).await;
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

async fn workspace_open(State(st): State<AppState>, Json(body): Json<OpenBody>) -> impl IntoResponse {
    open_path(&st, std::path::PathBuf::from(body.path)).await
}

async fn open_path(st: &AppState, path: std::path::PathBuf) -> axum::response::Response {
    match WorkspaceRoot::open(&path) {
        Ok(ws) => {
            let shown = ws.root().to_string_lossy().into_owned();
            viewer::watch_workspace(ws.clone(), st.fs_tx.clone()).ok();
            *st.workspace.write().await = Some(ws);
            Json(serde_json::json!({ "path": shown })).into_response()
        }
        Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
    }
}

async fn files_tree(State(st): State<AppState>) -> impl IntoResponse {
    match st.require_ws().await {
        Ok(ws) => match viewer::tree(&ws) {
            Ok(t) => Json(t).into_response(),
            Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn files_get(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let Some(path) = q.get("path") else {
        return err(StatusCode::BAD_REQUEST, "path required");
    };
    match st.require_ws().await {
        Ok(ws) => match viewer::read_file(&ws, path) {
            Ok(content) => Json(serde_json::json!({ "path": path, "content": content })).into_response(),
            Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
        },
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
struct PutFile {
    path: String,
    content: String,
}

async fn files_put(State(st): State<AppState>, Json(body): Json<PutFile>) -> impl IntoResponse {
    match st.require_ws().await {
        Ok(ws) => match viewer::write_file(&ws, &body.path, &body.content) {
            Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
            Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
        },
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn git_status(State(st): State<AppState>) -> impl IntoResponse {
    match st.require_ws().await {
        Ok(ws) => match viewer::status(&ws) {
            Ok(s) => Json(s).into_response(),
            Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
        },
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn git_diff(
    State(st): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    match st.require_ws().await {
        Ok(ws) => match viewer::diff(&ws, q.get("path").map(|s| s.as_str())) {
            Ok(d) => Json(serde_json::json!({ "diff": d })).into_response(),
            Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
        },
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
struct CommitBody {
    message: String,
}

async fn git_commit(State(st): State<AppState>, Json(body): Json<CommitBody>) -> impl IntoResponse {
    match st.require_ws().await {
        Ok(ws) => match viewer::commit(&ws, &body.message) {
            Ok(id) => Json(serde_json::json!({ "id": id })).into_response(),
            Err(e) => err(StatusCode::BAD_REQUEST, e.to_string()),
        },
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
struct RunBody {
    prompt: String,
}

async fn agent_run(State(st): State<AppState>, Json(body): Json<RunBody>) -> impl IntoResponse {
    let ws = match st.require_ws().await {
        Ok(w) => w,
        Err(e) => return err(StatusCode::BAD_REQUEST, e),
    };
    if body.prompt.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "prompt required");
    }
    let cancel = CancellationToken::new();
    *st.cancel.lock().await = Some(cancel.clone());
    let sandbox = st.sandbox.clone();
    let events = st.agent_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = orchestrator::run_task(body.prompt, ws, sandbox, events.clone(), cancel).await
        {
            let _ = events.send(AgentEvent::Error {
                message: e.to_string(),
            });
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

async fn ws_files(ws: WebSocketUpgrade, State(st): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pump_ws(socket, st.fs_tx.subscribe()))
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
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }
}

fn err(code: StatusCode, msg: impl Into<String>) -> axum::response::Response {
    (code, Json(serde_json::json!({ "error": msg.into() }))).into_response()
}
