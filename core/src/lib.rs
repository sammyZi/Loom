pub mod permission;
pub use permission::{glob_match, Permission, PermissionEntry, PermissionSet};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".next",
    "dist",
    "__pycache__",
    ".idea",
    ".vscode",
    ".ide-ai-tmp",
];

#[derive(Clone, Debug)]
pub struct WorkspaceRoot {
    root: PathBuf,
}

impl WorkspaceRoot {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = dunce::canonicalize(path.as_ref()).context("workspace path")?;
        if !root.is_dir() {
            bail!("not a directory: {}", root.display());
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rel_to(&self, abs: &Path) -> String {
        dunce::simplified(abs)
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default()
    }

    pub fn is_skipped_dir(name: &str) -> bool {
        SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(name))
    }

    /// Resolve a workspace-relative path. Rejects absolute paths, `..`, and symlink escapes.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let rel = rel.replace('\\', "/").trim_start_matches('/').to_string();
        if rel.is_empty() {
            return Ok(self.root.clone());
        }
        if Path::new(&rel).is_absolute() {
            bail!("absolute paths are not allowed");
        }
        for c in Path::new(&rel).components() {
            match c {
                Component::Normal(_) => {}
                Component::CurDir => {}
                _ => bail!("invalid path component in `{rel}`"),
            }
        }
        let joined = self.root.join(&rel);
        if let Ok(canon) = dunce::canonicalize(&joined) {
            if !is_within(&canon, &self.root) {
                bail!("path escapes workspace");
            }
            return Ok(canon);
        }
        // New file: canonicalize the nearest existing ancestor.
        let mut ancestor = joined.parent();
        while let Some(dir) = ancestor {
            if dir.exists() {
                let canon = dunce::canonicalize(dir).context("canonicalize parent")?;
                if !is_within(&canon, &self.root) {
                    bail!("path escapes workspace");
                }
                break;
            }
            if dir == self.root {
                break;
            }
            ancestor = dir.parent();
        }
        if !joined.starts_with(&self.root) {
            bail!("path escapes workspace");
        }
        Ok(joined)
    }
}

fn is_within(path: &Path, root: &Path) -> bool {
    let p = dunce::simplified(path);
    let r = dunce::simplified(root);
    p == r || p.starts_with(r)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileNode>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FsEvent {
    Changed { path: String },
    Removed { path: String },
}

/// Terminal output as it is produced, so a long-running command shows progress
/// instead of nothing until it exits.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShellEvent {
    Chunk { id: String, text: String },
    /// Placeholder kept next to the shell events; see AgentEvent::Todos.
    /// A background job the client did not start — the agent's dev servers and
    /// watchers. The terminal panel opens a tab for it so a long-running server
    /// gets its own scrollback instead of interleaving into one shared Agent
    /// tab, which made a second command unreadable.
    Opened { id: String, label: String },
}

/// One in-flight terminal command, foreground or background. `gen` lets a
/// finishing background task unregister only itself, never a newer command
/// that reused the same terminal id.
#[derive(Clone)]
pub struct ShellJob {
    pub token: tokio_util::sync::CancellationToken,
    pub gen: u64,
    /// Writes to the command's stdin, so a prompting command ("Enter the new
    /// date:") can be answered from the terminal panel instead of hanging.
    pub stdin: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

/// Registry of every running terminal command, keyed by terminal id. Shared by
/// the HTTP layer (`/shell/cancel`), the agent's background runs, and the
/// global stop button — one place to kill anything.
#[derive(Clone)]
pub struct ShellRegistry {
    map: Arc<std::sync::Mutex<HashMap<String, ShellJob>>>,
    gen: Arc<std::sync::atomic::AtomicU64>,
}

impl Default for ShellRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellRegistry {
    /// Upper bound on simultaneous terminal jobs; mostly to stop a buggy
    /// client from forking the machine through background runs.
    pub const MAX_JOBS: usize = 24;

    pub fn new() -> Self {
        Self {
            map: Arc::new(std::sync::Mutex::new(HashMap::new())),
            gen: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Register `id` for this command. A terminal runs one command at a time,
    /// so anything still registered under the same id is stale — it gets
    /// cancelled rather than orphaned. Returns the generation number, or None
    /// when the job cap is hit.
    pub fn begin(
        &self,
        id: &str,
        token: &tokio_util::sync::CancellationToken,
        stdin: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> Option<u64> {
        let gen = self.gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut jobs = self.map.lock().unwrap();
        if jobs.len() >= Self::MAX_JOBS && !jobs.contains_key(id) {
            return None;
        }
        let job = ShellJob { token: token.clone(), gen, stdin };
        if let Some(stale) = jobs.insert(id.to_string(), job) {
            stale.token.cancel();
        }
        Some(gen)
    }

    /// Type into whatever this terminal is running. False when it is idle or
    /// the command was started without a stdin pipe.
    pub fn write_stdin(&self, id: &str, text: &str) -> bool {
        let jobs = self.map.lock().unwrap();
        match jobs.get(id).and_then(|j| j.stdin.as_ref()) {
            Some(tx) => tx.send(text.to_string()).is_ok(),
            None => false,
        }
    }

    /// Drop the registration only if it is still ours; a newer command on the
    /// same terminal has a higher generation number and must survive.
    pub fn release(&self, id: &str, gen: u64) {
        let mut jobs = self.map.lock().unwrap();
        if jobs.get(id).map(|j| j.gen) == Some(gen) {
            jobs.remove(id);
        }
    }

    /// Kill whatever this terminal is running. Idempotent: a terminal with
    /// nothing in flight is a no-op, which is what closing an idle tab hits.
    /// Works for background jobs too — that is how a dev server stops.
    pub fn cancel_id(&self, id: &str) -> bool {
        let job = self.map.lock().unwrap().remove(id);
        match job {
            Some(j) => {
                j.token.cancel();
                true
            }
            None => false,
        }
    }

    /// Kill every job whose id starts with `prefix` — the global stop button
    /// uses `"agent"` so user terminals are never touched.
    pub fn cancel_prefixed(&self, prefix: &str) -> usize {
        let mut jobs = self.map.lock().unwrap();
        let killed: Vec<String> = jobs
            .keys()
            .filter(|k| k.as_str().starts_with(prefix))
            .cloned()
            .collect();
        for k in &killed {
            if let Some(j) = jobs.remove(k) {
                j.token.cancel();
            }
        }
        killed.len()
    }

    pub fn active_ids(&self) -> Vec<String> {
        self.map.lock().unwrap().keys().cloned().collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Token { text: String },
    ToolCall { name: String, input: serde_json::Value },
    ToolResult { name: String, output: String },
    Think { text: String },
    Diff { path: String, diff: String },
    Status { message: String },
    /// Approval request before a shell command runs (manual mode). The UI
    /// answers it via POST /agent/permission with the same id.
    Ask {
        id: String,
        program: String,
        args: String,
    },
    /// Approximate model context usage for the session: characters of history
    /// about to be sent, against the compaction budget. Drives the UI meter.
    Context { used: u64, limit: u64 },
    /// The agent's task list for a multi-step job, resent whole on every
    /// change so the UI never has to reconcile a partial update.
    Todos { items: Vec<TodoItem> },
    /// Point the Browser panel at a URL — how the agent shows the user the app
    /// it just started.
    Browse { url: String },
    /// Completion tokens reported by the provider for one model call.
    Usage { tokens: u64 },
    Done { summary: String },
    Error { message: String },
    /// Every file this run wrote, mapped to what it held right before this
    /// run touched it — `null` means the file did not exist yet. Sent once
    /// after the run settles, only when something was actually written.
    /// Backs "undo this message" in the feed.
    Snapshot {
        files: std::collections::BTreeMap<String, Option<String>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub text: String,
    /// "pending" | "running" | "done".
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub branch: String,
    pub files: Vec<GitFileStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRole {
    Planner,
    Coder,
    Reviewer,
    Single,
}

impl AgentRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Coder => "coder",
            Self::Reviewer => "reviewer",
            Self::Single => "agent",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_path_escape() {
        let dir = std::env::temp_dir().join("ide_ai_ws_test");
        let _ = fs::create_dir_all(&dir);
        let ws = WorkspaceRoot::open(&dir).unwrap();
        assert!(ws.resolve("../secret").is_err());
        #[cfg(windows)]
        assert!(ws.resolve(r"C:\Windows\System32\cmd.exe").is_err());
        #[cfg(not(windows))]
        assert!(ws.resolve("/etc/passwd").is_err());
        assert!(ws.resolve("ok/file.txt").is_ok());
    }
}
