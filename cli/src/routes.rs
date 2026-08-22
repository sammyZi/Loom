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
        .route("/sessions/archived", get(sessions_archived))
        .route("/sessions/{id}", delete(sessions_delete))
        .route("/sessions/{id}/rename", post(sessions_rename))
        .route("/sessions/{id}/archive", post(sessions_archive))
        .route("/sessions/{id}/unarchive", post(sessions_unarchive))
        .route("/agent/run", post(agent_run))
        .route("/agent/cancel", post(agent_cancel))
        .route("/agent/permission", post(agent_permission))
        .route("/agent/models", get(agent_models))
        .route("/settings/providers", get(providers_get).post(providers_post))
        .route("/shell/run", post(shell_run))
        .route("/shell/cancel", post(shell_cancel))
        .route("/shell/input", post(shell_input))
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

async fn sessions_unarchive(
    State(st): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    st.blocking_db(move |db| db.unarchive(&id).map(|()| serde_json::json!({ "ok": true })))
        .await
}

async fn sessions_archived(State(st): State<AppState>) -> impl IntoResponse {
    st.blocking_db(|db| {
        db.list_archived()
            .map(|sessions| serde_json::json!({ "sessions": sessions }))
    })
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
    /// Ties this run to a chat thread; earlier prompts and answers are replayed
    /// to the model so follow-ups keep their context.
    #[serde(default)]
    session_id: Option<String>,
}

/// How much chat memory rides along with a run. Old turns fall off the front.
const HISTORY_CHAR_BUDGET: usize = 24_000;

fn trim_history(history: &mut Vec<agent::Message>) {
    let mut size: usize = history.iter().map(|m| m.preview().len()).sum();
    while history.len() > 2 && size > HISTORY_CHAR_BUDGET {
        let dropped = history.remove(0).preview().len();
        size = size.saturating_sub(dropped);
    }
}

/// Live where possible: a configured gateway is asked what it actually serves,
/// so every model the key grants shows up instead of a curated handful.
async fn agent_models(State(st): State<AppState>) -> impl IntoResponse {
    let settings = st.snapshot_settings();
    Json(agent::model_groups_live(&settings).await)
}

#[derive(Deserialize)]
struct ProviderBody {
    provider: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    base_url: Option<String>,
    /// Remove the stored key for this provider (fall back to env vars).
    #[serde(default)]
    clear: bool,
}

async fn providers_get(State(st): State<AppState>) -> impl IntoResponse {
    let settings = st.snapshot_settings();
    Json(agent::model_groups(&settings))
}

/// Save or clear one provider's credentials. Keys are written only to the
/// user's config.json and never echoed back in any response.
async fn providers_post(
    State(st): State<AppState>,
    Json(body): Json<ProviderBody>,
) -> impl IntoResponse {
    if agent::provider_def(&body.provider).is_none() {
        return err(StatusCode::BAD_REQUEST, format!("unknown provider {}", body.provider));
    }
    {
        let mut settings = st.settings.lock().unwrap();
        let cfg = settings.providers.entry(body.provider.clone()).or_default();
        if body.clear {
            cfg.api_key = None;
            cfg.base_url = None;
        } else {
            if let Some(k) = body.api_key.as_deref() {
                let k = k.trim();
                if !k.is_empty() {
                    cfg.api_key = Some(k.to_string());
                }
            }
            if let Some(u) = body.base_url.as_deref() {
                let u = u.trim();
                cfg.base_url = if u.is_empty() { None } else { Some(u.to_string()) };
            }
        }
        if let Err(e) = settings.save() {
            tracing::error!("saving config.json failed: {e:#}");
            return err(StatusCode::INTERNAL_SERVER_ERROR, "could not save settings");
        }
    }
    let settings = st.snapshot_settings();
    Json(agent::model_groups(&settings)).into_response()
}

#[derive(Deserialize)]
struct PermissionBody {
    id: String,
    allow: bool,
}

/// Answer an AgentEvent::Ask raised by manual mode's run_command gate.
async fn agent_permission(
    State(st): State<AppState>,
    Json(body): Json<PermissionBody>,
) -> impl IntoResponse {
    if let Some(gate) = st.active_perm.lock().await.as_ref() {
        gate.answer(&body.id, body.allow);
        return Json(serde_json::json!({ "ok": true })).into_response();
    }
    err(StatusCode::CONFLICT, "no approval request is open")
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
    let settings = st.snapshot_settings();
    let model = agent::normalize_model(body.model.as_deref().unwrap_or(""), &settings);
    let mode = orchestrator::Mode::parse(body.mode.as_deref().unwrap_or(""));
    let effort = agent::normalize_effort(body.effort.as_deref().unwrap_or("")).to_string();
    let cancel = CancellationToken::new();
    *st.cancel.lock().await = Some(cancel.clone());

    // Chat memory: seed this run with prior user/assistant turns of the same
    // session, and remember today's exchange once it finishes.
    let session_key = body.session_id.filter(|s| !s.trim().is_empty());
    let mut history: Vec<agent::Message> = match &session_key {
        Some(id) => {
            let map = st.histories.lock().await;
            map.get(id).cloned().unwrap_or_default()
        }
        None => Vec::new(),
    };
    trim_history(&mut history);

    // Manual mode gates shell commands behind explicit approval.
    let perm = if mode == orchestrator::Mode::Manual {
        let gate = agent::PermGate::new();
        *st.active_perm.lock().await = Some(gate.clone());
        Some(gate)
    } else {
        *st.active_perm.lock().await = None;
        None
    };

    let env = agent::RunEnv {
        ws,
        sandbox: st.sandbox.clone(),
        events: st.agent_tx.clone(),
        shell_tx: st.shell_tx.clone(),
        shells: st.shells.clone(),
        cancel: cancel.clone(),
        settings,
    };
    let prompt = body.prompt;
    tokio::spawn(async move {
        let _slot = RunSlot(st.running.clone());
        let result = orchestrator::run_task(prompt.clone(), model, mode, effort, env, history, perm)
            .await;
        // The run slot is still held (dropped after this block), so no newer
        // run can have replaced the token: clearing unconditionally is safe.
        *st.cancel.lock().await = None;
        *st.active_perm.lock().await = None;
        if let Ok(answer) = &result {
            if let Some(id) = &session_key {
                let mut map = st.histories.lock().await;
                let entry = map.entry(id.clone()).or_default();
                entry.push(agent::Message::user_text(prompt.clone()));
                entry.push(agent::Message {
                    role: "assistant".into(),
                    content: Some(serde_json::json!(answer)),
                    tool_calls: None,
                    tool_call_id: None,
                });
                trim_history(entry);
            }
        }
        if let Err(e) = result {
            // A deliberate stop is not an error to the user; close the run
            // cleanly so the UI resets instead of showing a red failure.
            if cancel.is_cancelled() && e.to_string().contains("cancelled") {
                let _ = st
                    .agent_tx
                    .send(AgentEvent::Done { summary: "Stopped".into() });
            } else {
                let _ = st.agent_tx.send(AgentEvent::Error { message: e.to_string() });
            }
        }
    });
    Json(serde_json::json!({ "ok": true })).into_response()
}

/// Global stop: abort the LLM stream, kill every process tree the agent
/// started (background jobs included), and release any pending approval gate.
/// User terminals are deliberately untouched — only ids prefixed `agent`.
async fn agent_cancel(State(st): State<AppState>) -> impl IntoResponse {
    if let Some(c) = st.cancel.lock().await.take() {
        c.cancel();
    }
    *st.active_perm.lock().await = None;
    let killed = st.shells.cancel_prefixed("agent");
    if killed > 0 {
        tracing::info!("stop button killed {killed} agent job(s)");
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
    /// Keep the process tree alive after the HTTP response returns — for dev
    /// servers and watchers that must outlive the call. Output keeps streaming
    /// over /ws/shell until /shell/cancel stops it or it exits on its own.
    #[serde(default)]
    background: bool,
}

#[derive(Deserialize)]
struct ShellCancelBody {
    id: String,
}

/// Timeout for background runs. They are meant to be killed explicitly, not to
/// expire while a dev server is mid-reload.
const BG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Marker appended to the stream when a background job leaves the world, so
/// terminals can flip their tab back to idle even though no response follows.
pub const EXIT_NOTE_STOPPED: &str = "\n[stopped]";
pub const EXIT_NOTE_DONE: &str = "\n[exited code ";

async fn next_shell_job(
    st: &AppState,
    id: &str,
    token: &CancellationToken,
    stdin: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> Option<u64> {
    st.shells.begin(id, token, stdin)
}

#[derive(Deserialize)]
struct ShellInputBody {
    id: String,
    text: String,
}

/// Type into a running command. This is what makes prompting programs usable:
/// without it `date` or `npm init` asked a question nobody could answer.
async fn shell_input(
    State(st): State<AppState>,
    Json(body): Json<ShellInputBody>,
) -> impl IntoResponse {
    let ok = st.shells.write_stdin(&body.id, &body.text);
    Json(serde_json::json!({ "ok": ok }))
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
    let named = !body.id.is_empty();

    // Unnamed callers get no stream, since nothing could tell their chunks
    // apart, and cannot be cancelled — they are one-shot.
    let mut sink = None;
    let token = CancellationToken::new();
    let mut gen = 0u64;
    // Foreground runs get a live stdin pipe so the user can answer prompts.
    // Background jobs do not: nobody is watching them, so EOF is kinder.
    let mut stdin_source = None;
    if named {
        let stdin_tx = if body.background {
            None
        } else {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            stdin_source = Some(rx);
            Some(tx)
        };
        match next_shell_job(&st, &body.id, &token, stdin_tx).await {
            Some(g) => gen = g,
            None => {
                return err(
                    StatusCode::TOO_MANY_REQUESTS,
                    format!(
                        "too many running terminal jobs (max {})",
                        ide_core::ShellRegistry::MAX_JOBS
                    ),
                )
            }
        }
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

    let program: &str = if cfg!(windows) { "cmd" } else { "sh" };
    let args: Vec<String> = if cfg!(windows) {
        vec!["/C".into(), cmd.clone()]
    } else {
        vec!["-lc".into(), cmd.clone()]
    };
    let timeout = if body.background { BG_TIMEOUT } else { std::time::Duration::from_secs(60) };

    if body.background && named {
        // Fire and forget: respond immediately; output streams over /ws/shell
        // and the spawned task below reports the exit and releases the slot.
        let sandbox = st.sandbox.clone();
        let id = body.id.clone();
        let shell_tx = st.shell_tx.clone();
        tokio::task::spawn(async move {
            let out = sandbox
                .run_streaming(&ws, program, &args, timeout, &token, sink, None)
                .await;
            let note = match &out {
                Ok(o) if token.is_cancelled() => EXIT_NOTE_STOPPED.to_string(),
                Ok(o) => format!("{EXIT_NOTE_DONE}{}]", o.exit_code),
                Err(_) => EXIT_NOTE_STOPPED.to_string(),
            };
            let _ = shell_tx.send(ide_core::ShellEvent::Chunk { id: id.clone(), text: note });
            st
                .shells
                .release(&id, gen);
        });
        return Json(serde_json::json!({
            "started": true,
            "background": true,
            "id": body.id,
        }))
        .into_response();
    }

    let sandbox = st.sandbox.clone();
    let run = tokio::task::spawn(async move {
        sandbox.run_streaming(&ws, program, &args, timeout, &token, sink, stdin_source).await
    })
    .await;
    if named {
        st.shells.release(&body.id, gen);
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
/// tab hits. Works for background jobs too — that is how a dev server stops.
async fn shell_cancel(
    State(st): State<AppState>,
    Json(body): Json<ShellCancelBody>,
) -> impl IntoResponse {
    st.shells.cancel_id(&body.id);
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
