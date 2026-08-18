use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Token { text: String },
    ToolCall { name: String, input: serde_json::Value },
    ToolResult { name: String, output: String },
    Think { text: String },
    Diff { path: String, diff: String },
    Status { message: String },
    Done { summary: String },
    Error { message: String },
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
